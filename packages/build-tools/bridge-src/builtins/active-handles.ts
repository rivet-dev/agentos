import { exposeCustomGlobal } from "../global-exposure.js";
import { bridgeDispatchSync } from "../transport.js";
import { _exited } from "./process.js";
import {
	_immediateEntries,
	_timerEntries,
	clearImmediate,
	clearTimeout2,
} from "./timers.js";

var HANDLE_DISPATCH = {
	register: "kernelHandleRegister",
	unregister: "kernelHandleUnregister",
	list: "kernelHandleList",
};
var _activeHandles = /* @__PURE__ */ new Map();
// Optional close callbacks, keyed like _activeHandles.
var _handleClosers = /* @__PURE__ */ new Map();
var _waitResolvers = [];

var _activeHandleDrainScheduled = false;

function scheduleActiveHandleDrain() {
	if (_activeHandleDrainScheduled) return;
	_activeHandleDrainScheduled = true;
	const defer =
		typeof globalThis.setImmediate === "function"
			? globalThis.setImmediate
			: queueMicrotask;
	defer(() => {
		_activeHandleDrainScheduled = false;
		if (_activeHandles.size !== 0 || _waitResolvers.length === 0) return;
		const resolvers = _waitResolvers;
		_waitResolvers = [];
		for (const resolve of resolvers) resolve();
	});
}

function _registerHandle2(id, description, close) {
	try {
		bridgeDispatchSync(HANDLE_DISPATCH.register, id, description);
	} catch (error) {
		if (error instanceof Error && error.message.includes("EAGAIN")) {
			throw new Error(
				"ERR_RESOURCE_BUDGET_EXCEEDED: maximum active handles exceeded",
			);
		}
		throw error;
	}
	_activeHandles.set(id, description);
	if (typeof close === "function") _handleClosers.set(id, close);
}
function _unregisterHandle2(id) {
	_activeHandles.delete(id);
	_handleClosers.delete(id);
	const remaining = _activeHandles.size;
	try {
		bridgeDispatchSync(HANDLE_DISPATCH.unregister, id);
	} catch {}
	if (remaining === 0 && _waitResolvers.length > 0) scheduleActiveHandleDrain();
}
function _waitForActiveHandles() {
	if (typeof _exited !== "undefined" && _exited) {
		return Promise.resolve();
	}
	const getPendingTimerCount = globalThis._getPendingTimerCount;
	const waitForTimerDrain = globalThis._waitForTimerDrain;
	const hasHandles = _getActiveHandles().length > 0;
	const hasTimers =
		typeof getPendingTimerCount === "function" && getPendingTimerCount() > 0;
	if (!hasHandles && !hasTimers) {
		return Promise.resolve();
	}
	const promises = [];
	if (hasHandles) {
		promises.push(
			new Promise((resolve) => {
				let settled = false;
				const complete = () => {
					if (settled) return;
					settled = true;
					resolve();
				};
				_waitResolvers.push(complete);
				if (_getActiveHandles().length === 0) {
					complete();
				}
			}),
		);
	}
	if (hasTimers && typeof waitForTimerDrain === "function") {
		promises.push(waitForTimerDrain());
	}
	return Promise.all(promises).then(() => {
		const timersRemain =
			typeof getPendingTimerCount === "function" && getPendingTimerCount() > 0;
		if (_getActiveHandles().length > 0 || timersRemain) {
			return _waitForActiveHandles();
		}
	});
}
var CONTEXT_HANDLE_RELEASED = "ERR_AGENTOS_CONTEXT_HANDLE_RELEASED";
function contextHandleReleasedError(description) {
	const error = new Error(
		`${CONTEXT_HANDLE_RELEASED}: ${description} was closed because the context call that opened it completed; finish I/O inside one call, or use spawn for long-lived work`,
	);
	error.code = CONTEXT_HANDLE_RELEASED;
	return error;
}
// A context call or evaluation completes when its result settles. Whatever
// would keep a Node process alive is released: ref'd timers are cancelled and
// never run; ref'd sockets, servers, and child processes are closed, failing
// any pending read or request with CONTEXT_HANDLE_RELEASED instead of ending
// it cleanly. Unref'd timers (including the runtime's own) are left alone.
function _releaseLeftoverHandles() {
	const released = [];
	for (const [id, entry] of Array.from(_timerEntries)) {
		if (!entry.handle._refed) continue;
		clearTimeout2(id);
		released.push(entry.repeat ? "setInterval" : "setTimeout");
	}
	for (const [id, entry] of Array.from(_immediateEntries)) {
		if (!entry.handle._refed) continue;
		clearImmediate(id);
		released.push("setImmediate");
	}
	for (const [id, close] of Array.from(_handleClosers)) {
		if (!_handleClosers.has(id)) continue;
		const description = String(_activeHandles.get(id)).split(":")[0];
		released.push(description);
		_handleClosers.delete(id);
		close(contextHandleReleasedError(description));
	}
	if (released.length === 0 || typeof _error === "undefined") return;
	const counts = new Map();
	for (const name of released) counts.set(name, (counts.get(name) ?? 0) + 1);
	const summary = Array.from(
		counts,
		([name, count]) => `${name} x${count}`,
	).join(", ");
	_error.applySync(void 0, [
		`agentOS: the context call completed with work still pending; released ${summary}. Released timers never run; pending I/O on released handles fails with ${CONTEXT_HANDLE_RELEASED}.\n`,
	]);
}
function _getActiveHandles() {
	return Array.from(_activeHandles.values());
}
function _processExitRequested() {
	return typeof _exited !== "undefined" && _exited;
}
exposeCustomGlobal("_registerHandle", _registerHandle2);
exposeCustomGlobal("_unregisterHandle", _unregisterHandle2);
exposeCustomGlobal("_waitForActiveHandles", _waitForActiveHandles);
exposeCustomGlobal("_getActiveHandles", _getActiveHandles);
exposeCustomGlobal("_processExitRequested", _processExitRequested);

exposeCustomGlobal("_releaseLeftoverHandles", _releaseLeftoverHandles);
export {
	_activeHandles,
	_getActiveHandles,
	_processExitRequested,
	_registerHandle2,
	_unregisterHandle2,
	_waitForActiveHandles,
	_waitResolvers,
	HANDLE_DISPATCH,
};
