import { once } from "./events.js";
import { exposeCustomGlobal } from "../global-exposure.js";
import { getAsyncContextFrame, runInAsyncContextFrame } from "./async-context.js";
import { bridgeDispatchSync } from "../transport.js";
import { _exited, isProcessExitError, routeAsyncCallbackError, scheduleAsyncRethrow } from "./process.js";

var builtinTimersPromisesModule = {
  scheduler: {
    wait(delay = 0, options = void 0) {
      return builtinTimersPromisesModule.setTimeout(delay, void 0, options);
    },
    yield() {
      return builtinTimersPromisesModule.setImmediate();
    }
  },
  setImmediate(value = void 0, options = void 0) {
    if (options?.signal?.aborted) {
      return Promise.reject(options.signal.reason ?? new Error("The operation was aborted"));
    }
    return new Promise((resolve, reject) => {
      const onAbort = () => reject(options.signal.reason ?? new Error("The operation was aborted"));
      options?.signal?.addEventListener?.("abort", onAbort, { once: true });
      globalThis.setImmediate?.(() => {
        options?.signal?.removeEventListener?.("abort", onAbort);
        resolve(value);
      }) ?? globalThis.setTimeout?.(() => {
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
          const intervalValue = await builtinTimersPromisesModule.setTimeout(delay, value, { signal });
          return active ? { done: false, value: intervalValue } : { done: true, value: void 0 };
        } catch (error) {
          active = false;
          throw error;
        }
      },
      async return() {
        active = false;
        return { done: true, value: void 0 };
      }
    };
  },
  setTimeout(delay = 1, value = void 0, options = void 0) {
    if (options?.signal?.aborted) {
      return Promise.reject(options.signal.reason ?? new Error("The operation was aborted"));
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
  }
};

// A rejection handler must not catch process.exit(), which unwinds as a thrown
// ProcessExitError. A retained context can run the bridge again, so patch once.
if (!Promise.prototype.__agentOSProcessExitThenPatched) {
  const nativePromiseThen = Promise.prototype.then;
  Promise.prototype.then = function(onFulfilled, onRejected) {
    return nativePromiseThen.call(
      this,
      onFulfilled,
      typeof onRejected === "function" ? (error) => {
        if (isProcessExitError(error)) {
          throw error;
        }
        return onRejected(error);
      } : onRejected
    );
  };
  Object.defineProperty(Promise.prototype, "__agentOSProcessExitThenPatched", {
    value: true,
    configurable: true
  });
}

var TIMER_DISPATCH = {
  create: "kernelTimerCreate",
  arm: "kernelTimerArm",
  clear: "kernelTimerClear"
};

var _queueMicrotask = typeof queueMicrotask === "function" ? queueMicrotask : function(fn) {
  Promise.resolve().then(fn);
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
        "ERR_RESOURCE_BUDGET_EXCEEDED: maximum number of timers exceeded"
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
  constructor(id) {
    this._id = id;
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
    if (!this._destroyed && _timerEntries.has(this._id)) {
      armKernelTimer(this._id);
    }
    return this;
  }
  [Symbol.toPrimitive]() {
    return this._id;
  }
};

var _timerEntries = /* @__PURE__ */ new Map();

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
    resolvers.forEach((r) => r());
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
  return runInAsyncContextFrame(frame, routeAsyncCallbackError, void 0, [error]);
}

var _nextTickQueue = [];

var _nextTickScheduled = false;

function flushNextTickQueue() {
  _nextTickScheduled = false;
  while (_nextTickQueue.length > 0) {
    const entry = _nextTickQueue.shift();
    if (!entry) {
      break;
    }
    try {
      runInAsyncContextFrame(entry.asyncContextFrame, entry.callback, void 0, entry.args);
    } catch (error) {
      const outcome = routeAsyncCallbackErrorInFrame(entry.asyncContextFrame, error);
      if (!outcome.handled && outcome.rethrow !== null) {
        _nextTickQueue.length = 0;
        scheduleAsyncRethrow(outcome.rethrow);
      }
      return;
    }
  }
}

function scheduleNextTickFlush() {
  if (_nextTickScheduled) {
    return;
  }
  _nextTickScheduled = true;
  _queueMicrotask(flushNextTickQueue);
}

function timerDispatch(_eventType, payload) {
  const timerId = typeof payload === "number" ? payload : Number(payload?.timerId);
  if (!Number.isFinite(timerId)) return;
  const entry = _timerEntries.get(timerId);
  if (!entry) return;
  if (!entry.repeat) {
    entry.handle._destroyed = true;
    _timerEntries.delete(timerId);
  }
  try {
    runInAsyncContextFrame(entry.asyncContextFrame, entry.callback, entry.handle, entry.args);
  } catch (error) {
    const outcome = routeAsyncCallbackErrorInFrame(entry.asyncContextFrame, error);
    if (!outcome.handled && outcome.rethrow !== null) {
      throw outcome.rethrow;
    }
    return;
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

function setTimeout2(callback, delay, ...args) {
  const actualDelay = Math.max(1, normalizeTimerDelay(delay));
  const id = createKernelTimer(actualDelay, false);
  const handle = new TimerHandle(id);
  _timerEntries.set(id, {
    handle,
    callback,
    asyncContextFrame: getAsyncContextFrame(),
    args,
    repeat: false
  });
  armKernelTimer(id);
  return handle;
}

function clearTimeout2(timer) {
  const id = getTimerId(timer);
  if (id === void 0) return;
  const entry = _timerEntries.get(id);
  if (entry) {
    entry.handle._destroyed = true;
    _timerEntries.delete(id);
  }
  bridgeDispatchSync(TIMER_DISPATCH.clear, id);
  checkTimerDrain();
}

function setInterval(callback, delay, ...args) {
  const actualDelay = Math.max(1, normalizeTimerDelay(delay));
  const id = createKernelTimer(actualDelay, true);
  const handle = new TimerHandle(id);
  _timerEntries.set(id, {
    handle,
    callback,
    asyncContextFrame: getAsyncContextFrame(),
    args,
    repeat: true
  });
  armKernelTimer(id);
  return handle;
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
      runInAsyncContextFrame(entry.asyncContextFrame, entry.callback, entry.handle, entry.args);
    } catch (error) {
      const outcome = routeAsyncCallbackErrorInFrame(entry.asyncContextFrame, error);
      if (!outcome.handled && outcome.rethrow !== null) {
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
    repeat: false
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
export { TIMER_DISPATCH, TimerHandle, _drainImmediates, _getPendingImmediateCount, _getPendingTimerCount, _immediateEntries, _nextImmediateId, _nextTickQueue, _nextTickScheduled, _queueMicrotask, _timerDrainResolvers, _timerEntries, _waitForTimerDrain, armKernelTimer, builtinTimersPromisesModule, checkTimerDrain, clearImmediate, clearInterval, clearTimeout2, createKernelTimer, flushNextTickQueue, getPendingImmediateCount, getPendingTimerDrainCount, getRefedTimerCount, getTimerId, normalizeTimerDelay, scheduleNextTickFlush, setImmediate, setInterval, setTimeout2, timerDispatch };
