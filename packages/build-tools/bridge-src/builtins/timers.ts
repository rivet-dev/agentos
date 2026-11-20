import { exposeCustomGlobal } from "../global-exposure.js";
import { bridgeDispatchSync } from "../transport.js";
import {
	getAsyncContextFrame,
	runInAsyncContextFrame,
} from "./async-context.js";
import { once } from "./events.js";
import {
	_exited,
	isProcessExitError,
	routeAsyncCallbackError,
	scheduleAsyncRethrow,
} from "./process.js";

var builtinTimersPromisesModule = {
	scheduler: {
		wait(delay = 0, options = void 0) {
			return builtinTimersPromisesModule.setTimeout(delay, void 0, options);
		},
		yield() {
			return builtinTimersPromisesModule.setImmediate();
		},
	},
	setImmediate(value = void 0, options = void 0) {
		if (options?.signal?.aborted) {
			return Promise.reject(
				options.signal.reason ?? new Error("The operation was aborted"),
			);
		}
		return new Promise((resolve, reject) => {
			const onAbort = () =>
				reject(options.signal.reason ?? new Error("The operation was aborted"));
			options?.signal?.addEventListener?.("abort", onAbort, { once: true });
			globalThis.setImmediate?.(() => {
				options?.signal?.removeEventListener?.("abort", onAbort);
				resolve(value);
			}) ??
				globalThis.setTimeout?.(() => {
					options?.signal?.removeEventListener?.("abort", onAbort);
					resolve(value);
				}, 0);
		});
	},
	setInterval(delay = 1, value = void 0, options = void 0) {
		let active = true;
		const signal = options?.signal;
		if (signal?.aborted) {
			active = false;
		}
		return {
			[Symbol.asyncIterator]() {
				return this;
			},
			async next() {
				if (!active) {
					return { done: true, value: void 0 };
				}
				try {
					const intervalValue = await builtinTimersPromisesModule.setTimeout(
						delay,
						value,
						{ signal },
					);
					return active
						? { done: false, value: intervalValue }
						: { done: true, value: void 0 };
				} catch (error) {
					active = false;
					throw error;
				}
			},
			async return() {
				active = false;
				return { done: true, value: void 0 };
			},
		};
	},
	setTimeout(delay = 1, value = void 0, options = void 0) {
		if (options?.signal?.aborted) {
			return Promise.reject(
				options.signal.reason ?? new Error("The operation was aborted"),
			);
		}
		return new Promise((resolve, reject) => {
			const timer = globalThis.setTimeout?.(() => {
				options?.signal?.removeEventListener?.("abort", onAbort);
				resolve(value);
			}, delay ?? 0);
			const onAbort = () => {
				if (typeof globalThis.clearTimeout === "function") {
					globalThis.clearTimeout(timer);
				}
				reject(options.signal.reason ?? new Error("The operation was aborted"));
			};
			options?.signal?.addEventListener?.("abort", onAbort, { once: true });
		});
	},
};

// A rejection handler must not catch process.exit(), which unwinds as a thrown
// ProcessExitError. A retained context can run the bridge again, so patch once.
if (!Promise.prototype.__agentOSProcessExitThenPatched) {
	const nativePromiseThen = Promise.prototype.then;
	// biome-ignore lint/suspicious/noThenProperty: Intercept rejection handlers so process.exit unwinds cannot be caught as ordinary errors.
	Promise.prototype.then = function (onFulfilled, onRejected) {
		return nativePromiseThen.call(
			this,
			onFulfilled,
			typeof onRejected === "function"
				? (error) => {
						if (isProcessExitError(error)) {
							throw error;
						}
						return onRejected(error);
					}
				: onRejected,
		);
	};
	Object.defineProperty(Promise.prototype, "__agentOSProcessExitThenPatched", {
		value: true,
		configurable: true,
	});
}

var TIMER_DISPATCH = {
	create: "kernelTimerCreate",
	arm: "kernelTimerArm",
	clear: "kernelTimerClear",
};

var _queueMicrotask =
	typeof queueMicrotask === "function"
		? queueMicrotask
		: function (fn) {
				// Synchronous callback failures belong to the native terminal owner;
				// returned Promises retain the existing adoption behavior.
				void Promise.resolve().then(() => {
					try {
						return fn();
					} catch (error) {
						const outcome = routeAsyncCallbackError(error);
						if (!outcome.handled) {
							scheduleAsyncRethrow(outcome.rethrow);
						}
					}
				});
			};

function normalizeTimerDelay(delay) {
	const numericDelay = Number(delay ?? 0);
	if (!Number.isFinite(numericDelay) || numericDelay <= 0) {
		return 0;
	}
	return Math.floor(numericDelay);
}

function getTimerId(timer) {
	if (timer && typeof timer === "object" && timer._id !== void 0) {
		return timer._id;
	}
	if (typeof timer === "number") {
		return timer;
	}
	return void 0;
}

function createKernelTimer(delayMs, repeat) {
	try {
		return bridgeDispatchSync(TIMER_DISPATCH.create, delayMs, repeat);
	} catch (error) {
		if (error instanceof Error && error.message.includes("EAGAIN")) {
			throw new Error(
				"ERR_RESOURCE_BUDGET_EXCEEDED: maximum number of timers exceeded",
			);
		}
		throw error;
	}
}

function armKernelTimer(timerId) {
	bridgeDispatchSync(TIMER_DISPATCH.arm, timerId);
}

var TimerHandle = class {
	_id;
	_destroyed;
	_refed;
	_kernelId;
	_entry;
	_firing = false;
	_cleared = false;
	constructor(id) {
		this._id = id;
		this._kernelId = id;
		this._destroyed = false;
		this._refed = true;
	}
	ref() {
		this._refed = true;
		return this;
	}
	unref() {
		this._refed = false;
		return this;
	}
	hasRef() {
		return this._refed;
	}
	refresh() {
		if (this._cleared || !this._entry) return this;
		if (!this._destroyed) {
			try {
				armKernelTimer(this._kernelId);
				return this;
			} catch (error) {
				// A native expiry can precede delivery of its JS callback.
				if (
					!`${error?.code ?? ""} ${error?.message ?? ""}`.includes(
						"ERR_AGENTOS_JAVASCRIPT_TIMER_UNKNOWN",
					)
				)
					throw error;
			}
		}
		const id = createKernelTimer(this._entry.delay, this._entry.repeat);
		try {
			armKernelTimer(id);
		} catch (error) {
			try {
				bridgeDispatchSync(TIMER_DISPATCH.clear, id);
			} catch (cleanupError) {
				throw new AggregateError(
					[error, cleanupError],
					"Timer admission failed",
				);
			}
			throw error;
		}
		_timerEntries.delete(this._kernelId);
		_timerHandles.delete(this._id);
		this._kernelId = id;
		if (!this._firing) this._id = id;
		this._destroyed = false;
		_timerEntries.set(id, this._entry);
		_timerHandles.set(this._id, this);
		return this;
	}
	close() {
		clearTimeout2(this);
		return this;
	}
	[Symbol.dispose]() {
		clearTimeout2(this);
	}
	[Symbol.toPrimitive]() {
		return this._id;
	}
};

var _timerEntries = /* @__PURE__ */ new Map();
// Live public identities only; fired callbacks retain their own handle.
var _timerHandles = new Map();
var _firingTimer = void 0;

var _immediateEntries = /* @__PURE__ */ new Map();

var _nextImmediateId = -1;

var _timerDrainResolvers = [];

function getRefedTimerCount() {
	if (typeof _exited !== "undefined" && _exited) {
		return 0;
	}
	let count = 0;
	for (const entry of _timerEntries.values()) {
		if (entry.handle?.hasRef?.() !== false) {
			count += 1;
		}
	}
	return count;
}

function getPendingImmediateCount() {
	if (typeof _exited !== "undefined" && _exited) {
		return 0;
	}
	return _immediateEntries.size;
}

function getPendingTimerDrainCount() {
	return getRefedTimerCount() + getPendingImmediateCount();
}

function checkTimerDrain() {
	if (getPendingTimerDrainCount() === 0 && _timerDrainResolvers.length > 0) {
		const resolvers = _timerDrainResolvers;
		_timerDrainResolvers = [];
		resolvers.forEach((r) => {
			r();
		});
	}
}

function _getPendingTimerCount() {
	return getPendingTimerDrainCount();
}

function _getPendingImmediateCount() {
	return getPendingImmediateCount();
}

function _waitForTimerDrain() {
	if (getPendingTimerDrainCount() === 0) return Promise.resolve();
	return new Promise((resolve) => {
		_timerDrainResolvers.push(resolve);
		checkTimerDrain();
	});
}

// An uncaughtException handler runs in the context that threw.
function routeAsyncCallbackErrorInFrame(frame, error) {
	return runInAsyncContextFrame(frame, routeAsyncCallbackError, void 0, [
		error,
	]);
}

const nextTickState = { queue: [] };
const nextTickPush = Function.call.bind(Array.prototype.push);
const nextTickShift = Function.call.bind(Array.prototype.shift);
function enqueueNextTick(entry) {
	nextTickPush(nextTickState.queue, entry);
}

var _nextTickScheduled = false;

function flushNextTickQueue() {
	_nextTickScheduled = false;
	while (nextTickState.queue.length > 0) {
		const entry = nextTickShift(nextTickState.queue);
		if (!entry) {
			break;
		}
		try {
			runInAsyncContextFrame(
				entry.asyncContextFrame,
				entry.callback,
				void 0,
				entry.args,
			);
		} catch (error) {
			const outcome = routeAsyncCallbackErrorInFrame(
				entry.asyncContextFrame,
				error,
			);
			if (!outcome.handled) {
				nextTickState.queue.length = 0;
				scheduleAsyncRethrow(outcome.rethrow);
			}
			return nextTickState.queue.length;
		}
	}
	return nextTickState.queue.length;
}

function scheduleNextTickFlush(notify) {
	_nextTickScheduled = true;
	notify();
}

function timerDispatch(_eventType, payload) {
	const timerId =
		typeof payload === "number" ? payload : Number(payload?.timerId);
	if (!Number.isFinite(timerId)) return;
	const entry = _timerEntries.get(timerId);
	if (!entry) return;
	if (!entry.repeat) {
		entry.handle._destroyed = true;
		_timerEntries.delete(timerId);
		_timerHandles.delete(entry.handle._id);
	}
	const previousFiring = _firingTimer;
	_firingTimer = entry.handle;
	entry.handle._firing = true;
	try {
		runInAsyncContextFrame(
			entry.asyncContextFrame,
			entry.callback,
			entry.handle,
			entry.args,
		);
	} catch (error) {
		const outcome = routeAsyncCallbackErrorInFrame(
			entry.asyncContextFrame,
			error,
		);
		if (!outcome.handled) {
			throw outcome.rethrow;
		}
		return;
	} finally {
		entry.handle._firing = false;
		_firingTimer = previousFiring;
	}
	if (typeof _exited !== "undefined" && _exited) {
		checkTimerDrain();
		return;
	}
	if (entry.repeat && _timerEntries.has(timerId)) {
		armKernelTimer(timerId);
	}
	checkTimerDrain();
}

function createTimer(callback, delay, args, repeat) {
	const actualDelay = Math.max(1, normalizeTimerDelay(delay));
	const id = createKernelTimer(actualDelay, repeat);
	try {
		armKernelTimer(id);
	} catch (error) {
		try {
			bridgeDispatchSync(TIMER_DISPATCH.clear, id);
		} catch (cleanupError) {
			throw new AggregateError([error, cleanupError], "Timer admission failed");
		}
		throw error;
	}
	const handle = new TimerHandle(id);
	const entry = {
		handle,
		callback,
		asyncContextFrame: getAsyncContextFrame(),
		args,
		repeat,
		delay: actualDelay,
	};
	handle._entry = entry;
	_timerEntries.set(id, entry);
	_timerHandles.set(id, handle);
	return handle;
}

function setTimeout2(callback, delay, ...args) {
	return createTimer(callback, delay, args, false);
}

function clearTimeout2(timer) {
	const id = getTimerId(timer);
	if (id === void 0) return;
	const handle =
		timer instanceof TimerHandle
			? timer
			: (_timerHandles.get(id) ??
				(_firingTimer?._id === id ? _firingTimer : void 0));
	if (handle) {
		bridgeDispatchSync(TIMER_DISPATCH.clear, handle._kernelId);
		handle._destroyed = true;
		handle._cleared = true;
		handle._entry = void 0;
		_timerEntries.delete(handle._kernelId);
		_timerHandles.delete(handle._id);
	}
	checkTimerDrain();
}

function setInterval(callback, delay, ...args) {
	return createTimer(callback, delay, args, true);
}

function clearInterval(timer) {
	clearTimeout2(timer);
}

exposeCustomGlobal("_timerDispatch", timerDispatch);

function _drainImmediates() {
	const entries = Array.from(_immediateEntries.entries());
	for (const [immediateId, entry] of entries) {
		if (_immediateEntries.get(immediateId) !== entry) {
			continue;
		}
		entry.handle._destroyed = true;
		_immediateEntries.delete(immediateId);
		try {
			runInAsyncContextFrame(
				entry.asyncContextFrame,
				entry.callback,
				entry.handle,
				entry.args,
			);
		} catch (error) {
			const outcome = routeAsyncCallbackErrorInFrame(
				entry.asyncContextFrame,
				error,
			);
			if (!outcome.handled) {
				throw outcome.rethrow;
			}
			return;
		}
		if (typeof _exited !== "undefined" && _exited) {
			checkTimerDrain();
			return;
		}
	}
	checkTimerDrain();
}

exposeCustomGlobal("_getPendingTimerCount", _getPendingTimerCount);

exposeCustomGlobal("_drainImmediates", _drainImmediates);

exposeCustomGlobal("_getPendingImmediateCount", _getPendingImmediateCount);

exposeCustomGlobal("_waitForTimerDrain", _waitForTimerDrain);

function setImmediate(callback, ...args) {
	const id = _nextImmediateId--;
	const handle = new TimerHandle(id);
	_immediateEntries.set(id, {
		handle,
		callback,
		asyncContextFrame: getAsyncContextFrame(),
		args,
		repeat: false,
	});
	return handle;
}

function clearImmediate(timer) {
	const id = getTimerId(timer);
	if (id === void 0) return;
	const entry = _immediateEntries.get(id);
	if (entry) {
		entry.handle._destroyed = true;
		_immediateEntries.delete(id);
		checkTimerDrain();
		return;
	}
	if (id < 0) return;
	clearTimeout2(timer);
}

export {
	_drainImmediates,
	_getPendingImmediateCount,
	_getPendingTimerCount,
	_immediateEntries,
	_nextImmediateId,
	_nextTickScheduled,
	_queueMicrotask,
	_timerDrainResolvers,
	_timerEntries,
	_waitForTimerDrain,
	armKernelTimer,
	builtinTimersPromisesModule,
	checkTimerDrain,
	clearImmediate,
	clearInterval,
	clearTimeout2,
	createKernelTimer,
	enqueueNextTick,
	flushNextTickQueue,
	getPendingImmediateCount,
	getPendingTimerDrainCount,
	getRefedTimerCount,
	getTimerId,
	normalizeTimerDelay,
	scheduleNextTickFlush,
	setImmediate,
	setInterval,
	setTimeout2,
	TIMER_DISPATCH,
	TimerHandle,
	timerDispatch,
};

exposeCustomGlobal(
	"__agentOSNextTickHooks",
	Object.freeze({ flush: flushNextTickQueue, state: nextTickState }),
);
