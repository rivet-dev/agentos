import { exposeCustomGlobal } from "../global-exposure.js";

// The guest's async context lives in V8's continuation-preserved embedder data.
// V8 saves that value with every promise reaction and queued microtask and puts
// it back for the continuation, so native `await` and promise chains follow it
// with no JavaScript hook and no per-promise cost here.
//
// A frame is an immutable Map from AsyncLocalStorage instance to store. Frames
// are shared by every continuation that captured them, so writing a store always
// makes a new frame.
//
// Callbacks that V8 does not schedule (timers, immediates, nextTick, runtime
// events) capture a frame here and re-enter it when they run.

type AsyncContextFrame = Map<object, unknown> | undefined;

// The frame a resource's events run in. A symbol keeps it out of guest subclass
// fields, `Object.keys`, and JSON.
const kAsyncContextFrame = Symbol("kAsyncContextFrame");

const engineGlobals = globalThis as unknown as Record<string, unknown>;
const getAsyncContextFrame = engineGlobals.__agentOsGetAsyncContext as () => AsyncContextFrame;
const setAsyncContextFrame = engineGlobals.__agentOsSetAsyncContext as (
	frame: AsyncContextFrame,
) => void;
if (
	typeof getAsyncContextFrame !== "function" ||
	typeof setAsyncContextFrame !== "function"
) {
	throw new Error(
		"V8 runtime did not install the async context functions before the bridge loaded",
	);
}
// Guests get the context only through node:async_hooks.
delete engineGlobals.__agentOsGetAsyncContext;
delete engineGlobals.__agentOsSetAsyncContext;

function frameWithStore(
	frame: AsyncContextFrame,
	storage: object,
	store: unknown,
): AsyncContextFrame {
	const next = new Map(frame);
	next.set(storage, store);
	return next;
}

function runInAsyncContextFrame(frame, callback, thisArg, args) {
	const prior = getAsyncContextFrame();
	setAsyncContextFrame(frame);
	try {
		return Reflect.apply(callback, thisArg, args);
	} finally {
		setAsyncContextFrame(prior);
	}
}

/** Binds a runtime event callback to the context of the resource that owns it. */
function bindAsyncContextFrame(frame, callback) {
	return function (...args) {
		return runInAsyncContextFrame(frame, callback, this, args);
	};
}

class AsyncResource {
	type;
	constructor(type = "AgentOsAsyncResource") {
		this.type = type;
		this[kAsyncContextFrame] = getAsyncContextFrame();
	}
	static bind(fn, type = undefined, thisArg = undefined) {
		return new AsyncResource(type || fn.name || "bound-anonymous-fn").bind(fn, thisArg);
	}
	bind(fn, thisArg = undefined) {
		if (typeof fn !== "function") {
			throw new TypeError(
				`The "fn" argument must be of type function. Received ${typeof fn}`,
			);
		}
		const resource = this;
		const bound =
			thisArg === undefined
				? function (...args) {
						return resource.runInAsyncScope(fn, this, ...args);
					}
				: (...args) => resource.runInAsyncScope(fn, thisArg, ...args);
		Object.defineProperty(bound, "length", {
			configurable: true,
			value: fn.length,
		});
		bound.asyncResource = resource;
		return bound;
	}
	emitBefore() {}
	emitAfter() {}
	emitDestroy() {
		return this;
	}
	asyncId() {
		return 0;
	}
	triggerAsyncId() {
		return 0;
	}
	runInAsyncScope(fn, thisArg, ...args) {
		return runInAsyncContextFrame(this[kAsyncContextFrame], fn, thisArg, args);
	}
}

class AsyncLocalStorage {
	_enabled = false;
	static bind(fn) {
		return AsyncResource.bind(fn);
	}
	static snapshot() {
		return AsyncLocalStorage.bind((callback, ...args) => callback(...args));
	}
	// Node 22 semantics: getStore() returns undefined in every context until the
	// next run() or enterWith().
	disable() {
		this._enabled = false;
	}
	enterWith(store) {
		this._enabled = true;
		setAsyncContextFrame(frameWithStore(getAsyncContextFrame(), this, store));
	}
	exit(callback, ...args) {
		return runInAsyncContextFrame(
			frameWithStore(getAsyncContextFrame(), this, undefined),
			callback,
			null,
			args,
		);
	}
	getStore() {
		return this._enabled ? getAsyncContextFrame()?.get(this) : undefined;
	}
	run(store, callback, ...args) {
		this._enabled = true;
		return runInAsyncContextFrame(
			frameWithStore(getAsyncContextFrame(), this, store),
			callback,
			null,
			args,
		);
	}
}

const builtinAsyncHooksModule = {
	AsyncLocalStorage,
	AsyncResource,
	createHook() {
		return {
			enable() {
				return this;
			},
			disable() {
				return this;
			},
		};
	},
	executionAsyncId() {
		return 0;
	},
	triggerAsyncId() {
		return 0;
	},
};

exposeCustomGlobal("_asyncHooksModule", builtinAsyncHooksModule);

export {
	bindAsyncContextFrame,
	builtinAsyncHooksModule,
	getAsyncContextFrame,
	kAsyncContextFrame,
	runInAsyncContextFrame,
};
