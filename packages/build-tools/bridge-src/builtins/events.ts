import { process2, routeAsyncCallbackError } from "./process.js";
import { exposeCustomGlobal } from "../global-exposure.js";
import { Event } from "../polyfills/index.js";

var eventsErrorMonitor = Symbol("events.errorMonitor");
var eventsDefaultMaxListeners = 10;
function emitEventEmitterMeta(emitter, metaEvent, args) {
  if (metaEvent === "newListener" && args[0] === "newListener") {
    return false;
  }
  if (metaEvent === "removeListener" && args[0] === "removeListener") {
    return false;
  }
  return emitEventRecords(emitter, metaEvent, args);
}
function cloneEventListeners(emitter, event) {
  ensureEventEmitterInitialized(emitter);
  const listeners = emitter._events[event];
  return Array.isArray(listeners) ? listeners.slice() : [];
}
function removeEventListenerRecord(emitter, event, listener, onceOnly = false) {
  ensureEventEmitterInitialized(emitter);
  const listeners = emitter._events[event];
  if (!Array.isArray(listeners) || listeners.length === 0) {
    return emitter;
  }
  let removedRecord = null;
  const next = listeners.slice();
  for (let index = next.length - 1; index >= 0; index -= 1) {
    const record = next[index];
    if (record.listener !== listener && record.rawListener !== listener) {
      continue;
    }
    if (onceOnly && !record.once) {
      continue;
    }
    removedRecord = record;
    next.splice(index, 1);
    break;
  }
  if (removedRecord === null) {
    return emitter;
  }
  if (next.length === 0) {
    delete emitter._events[event];
  } else {
    emitter._events[event] = next;
  }
  emitEventEmitterMeta(emitter, "removeListener", [event, removedRecord.listener]);
  return emitter;
}
function removeAllEventListenerRecords(emitter, event) {
  const key = String(event);
  const listeners = cloneEventListeners(emitter, key);
  for (let index = listeners.length - 1; index >= 0; index -= 1) {
    removeEventListenerRecord(emitter, key, listeners[index].listener);
  }
  return emitter;
}
function emitEventRecords(emitter, event, args) {
  const listeners = cloneEventListeners(emitter, event);
  if (listeners.length === 0) {
    return false;
  }
  for (const record of listeners) {
    if (record.once) {
      removeEventListenerRecord(emitter, event, record.listener, true);
    }
    try {
      record.listener.apply(emitter, args);
    } catch (error) {
      const outcome = routeAsyncCallbackError(error);
      if (!outcome.handled && outcome.rethrow !== null) {
        throw outcome.rethrow;
      }
      return true;
    }
  }
  return true;
}
function topLevelEventListenerCount(emitter, event) {
  return cloneEventListeners(emitter, event).length;
}
function topLevelGetEventListeners(emitter, event) {
  return cloneEventListeners(emitter, event).map((record) => record.listener);
}
function topLevelGetRawEventListeners(emitter, event) {
  return cloneEventListeners(emitter, event).map((record) => record.rawListener ?? record.listener);
}
function createOnceRawListener(emitter, event, listener) {
  function onceRawListener(...args) {
    removeEventListenerRecord(emitter, event, onceRawListener, true);
    return listener.apply(emitter, args);
  }
  Object.defineProperty(onceRawListener, "listener", {
    value: listener,
    configurable: true,
    enumerable: false,
    writable: false
  });
  return onceRawListener;
}
function topLevelGetMaxListeners(emitter) {
  if (emitter && typeof emitter.getMaxListeners === "function") {
    return emitter.getMaxListeners();
  }
  return eventsDefaultMaxListeners;
}
function topLevelSetMaxListeners(n, ...emitters) {
  for (const emitter of emitters) {
    if (emitter && typeof emitter.setMaxListeners === "function") {
      emitter.setMaxListeners(n);
    }
  }
}
function addAbortListener(signal, listener) {
  if (!signal || typeof signal.addEventListener !== "function") {
    throw new TypeError("AbortSignal is required");
  }
  const wrapped = () => listener();
  if (signal.aborted) {
    queueMicrotask(wrapped);
    return { dispose() {
    } };
  }
  signal.addEventListener("abort", wrapped, { once: true });
  return {
    dispose() {
      signal.removeEventListener("abort", wrapped);
    }
  };
}
function once(emitter, eventName) {
  return new Promise((resolve, reject) => {
    const onEvent = (...args) => {
      if (typeof emitter.removeListener === "function") {
        emitter.removeListener("error", onError);
      }
      resolve(args);
    };
    const onError = (error) => {
      if (typeof emitter.removeListener === "function") {
        emitter.removeListener(eventName, onEvent);
      }
      reject(error);
    };
    emitter.once(eventName, onEvent);
    if (eventName !== "error" && typeof emitter.once === "function") {
      emitter.once("error", onError);
    }
  });
}
function initializeEventEmitter(target) {
  target._events = Object.create(null);
  target._maxListeners = eventsDefaultMaxListeners;
  target._maxListenersWarned = /* @__PURE__ */ new Set();
}
function ensureEventEmitterInitialized(target) {
  if (!target || (typeof target !== "object" && typeof target !== "function")) {
    return;
  }
  if (typeof target._events === "undefined") {
    initializeEventEmitter(target);
    return;
  }
  if (!(target._maxListenersWarned instanceof Set)) {
    target._maxListenersWarned = /* @__PURE__ */ new Set();
  }
  // An emitter can acquire `_events` without going through our constructor (e.g.
  // a subclass that sets up its own event storage). If `_maxListeners` was never
  // initialized, `maybeWarnEventEmitterListeners` would treat `total <= undefined`
  // as false and fire a spurious MaxListenersExceededWarning on the *first*
  // listener (reported count "1"). Default it to the standard limit so the
  // threshold check is meaningful.
  if (typeof target._maxListeners !== "number") {
    target._maxListeners = eventsDefaultMaxListeners;
  }
}
function createMaxListenersExceededWarning(emitter, event, total) {
  const maxListeners = Number.isFinite(emitter._maxListeners) ? emitter._maxListeners : eventsDefaultMaxListeners;
  const warning = new Error(
    `Possible EventEmitter memory leak detected. ${total} ${event} listeners added to [EventEmitter]. MaxListeners is ${maxListeners}. Use emitter.setMaxListeners() to increase limit`
  );
  warning.name = "MaxListenersExceededWarning";
  warning.emitter = emitter;
  warning.type = event;
  warning.count = total;
  return warning;
}
function maybeWarnEventEmitterListeners(emitter, event, total) {
  ensureEventEmitterInitialized(emitter);
  if (!(emitter._maxListenersWarned instanceof Set)) {
    emitter._maxListenersWarned = /* @__PURE__ */ new Set();
  }
  if (emitter._maxListeners <= 0 || emitter._maxListenersWarned.has(event) || total <= emitter._maxListeners) {
    return;
  }
  emitter._maxListenersWarned.add(event);
  const warning = createMaxListenersExceededWarning(emitter, event, total);
  if (process2 && typeof process2.emitWarning === "function") {
    process2.emitWarning(warning);
    return;
  }
  if (typeof _error !== "undefined") {
    _error.applySync(void 0, [`${warning.name}: ${warning.message}`]);
  }
}
function addEventListenerRecord(emitter, event, record, prepend = false) {
  ensureEventEmitterInitialized(emitter);
  const listeners = emitter._events[event] ?? [];
  if (prepend) {
    listeners.unshift(record);
  } else {
    listeners.push(record);
  }
  emitter._events[event] = listeners;
  maybeWarnEventEmitterListeners(emitter, event, listeners.length);
}
function EventEmitter() {
  if (!this || (typeof this !== "object" && typeof this !== "function")) {
    return new EventEmitter();
  }
  initializeEventEmitter(this);
}
EventEmitter.prototype.addListener = function(event, listener) {
  return this.on(event, listener);
};
EventEmitter.prototype.on = function(event, listener) {
  if (typeof listener !== "function") {
    throw new TypeError("listener must be a function");
  }
  const key = String(event);
  emitEventEmitterMeta(this, "newListener", [key, listener]);
  addEventListenerRecord(this, key, { listener, once: false });
  return this;
};
EventEmitter.prototype.once = function(event, listener) {
  if (typeof listener !== "function") {
    throw new TypeError("listener must be a function");
  }
  const key = String(event);
  emitEventEmitterMeta(this, "newListener", [key, listener]);
  addEventListenerRecord(this, key, {
    listener,
    rawListener: createOnceRawListener(this, key, listener),
    once: true
  });
  return this;
};
EventEmitter.prototype.prependListener = function(event, listener) {
  if (typeof listener !== "function") {
    throw new TypeError("listener must be a function");
  }
  const key = String(event);
  emitEventEmitterMeta(this, "newListener", [key, listener]);
  addEventListenerRecord(this, key, { listener, once: false }, true);
  return this;
};
EventEmitter.prototype.prependOnceListener = function(event, listener) {
  if (typeof listener !== "function") {
    throw new TypeError("listener must be a function");
  }
  const key = String(event);
  emitEventEmitterMeta(this, "newListener", [key, listener]);
  addEventListenerRecord(this, key, {
    listener,
    rawListener: createOnceRawListener(this, key, listener),
    once: true
  }, true);
  return this;
};
EventEmitter.prototype.removeListener = function(event, listener) {
  return removeEventListenerRecord(this, String(event), listener);
};
EventEmitter.prototype.off = function(event, listener) {
  return removeEventListenerRecord(this, String(event), listener);
};
EventEmitter.prototype.removeAllListeners = function(event) {
  ensureEventEmitterInitialized(this);
  if (typeof event === "undefined") {
    for (const key of Object.keys(this._events)) {
      if (key === "removeListener") {
        continue;
      }
      removeAllEventListenerRecords(this, key);
    }
    delete this._events.removeListener;
  } else {
    removeAllEventListenerRecords(this, String(event));
  }
  return this;
};
EventEmitter.prototype.emit = function(event, ...args) {
  const key = String(event);
  if (key === "error" && topLevelEventListenerCount(this, key) === 0) {
    throw args[0] instanceof Error ? args[0] : new Error(String(args[0] ?? "Unhandled error event"));
  }
  let handled = emitEventRecords(this, key, args);
  if (key === "error") {
    handled = emitEventRecords(this, String(eventsErrorMonitor), args) || handled;
  }
  return handled;
};
EventEmitter.prototype.listeners = function(event) {
  return topLevelGetEventListeners(this, String(event));
};
EventEmitter.prototype.rawListeners = function(event) {
  return topLevelGetRawEventListeners(this, String(event));
};
EventEmitter.prototype.listenerCount = function(event) {
  return topLevelEventListenerCount(this, String(event));
};
EventEmitter.prototype.eventNames = function() {
  ensureEventEmitterInitialized(this);
  return Object.keys(this._events);
};
EventEmitter.prototype.setMaxListeners = function(n) {
  ensureEventEmitterInitialized(this);
  this._maxListeners = Number(n);
  return this;
};
EventEmitter.prototype.getMaxListeners = function() {
  ensureEventEmitterInitialized(this);
  return Number.isFinite(this._maxListeners) ? this._maxListeners : eventsDefaultMaxListeners;
};
EventEmitter.once = once;
// Node 12.16+ async-iterator helper: `for await (const [a] of events.on(emitter, "data")) {}`.
EventEmitter.on = function on(emitter, eventName, options) {
  const signal = options && options.signal;
  if (signal && signal.aborted) throw signal.reason ?? new Error("The operation was aborted");
  const removeL = (ev, fn) => (emitter.off ?? emitter.removeListener).call(emitter, ev, fn);
  const queue = [];
  const unconsumed = [];
  let error = null;
  let finished = false;
  const cleanup = () => {
    removeL(eventName, onEvent);
    removeL("error", onError);
    if (signal && signal.removeEventListener) signal.removeEventListener("abort", onAbort);
  };
  const iterator = {
    next() {
      const value = queue.shift();
      if (value !== undefined) return Promise.resolve({ value, done: false });
      if (error) { const e = error; error = null; cleanup(); return Promise.reject(e); }
      if (finished) return Promise.resolve({ value: undefined, done: true });
      return new Promise((resolve, reject) => unconsumed.push({ resolve, reject }));
    },
    return() {
      finished = true;
      cleanup();
      for (const c of unconsumed) c.resolve({ value: undefined, done: true });
      unconsumed.length = 0;
      return Promise.resolve({ value: undefined, done: true });
    },
    throw(err) { error = err; cleanup(); return Promise.reject(err); },
    [Symbol.asyncIterator]() { return this; },
  };
  function onEvent(...args) {
    const c = unconsumed.shift();
    if (c) c.resolve({ value: args, done: false });
    else queue.push(args);
  }
  function onError(err) {
    const c = unconsumed.shift();
    if (c) { cleanup(); c.reject(err); }
    else error = err;
  }
  function onAbort() { iterator.return(); }
  emitter.on(eventName, onEvent);
  emitter.on("error", onError);
  if (signal && signal.addEventListener) signal.addEventListener("abort", onAbort, { once: true });
  return iterator;
};
EventEmitter.addAbortListener = function addAbortListener(signal, listener) {
  if (signal && signal.aborted) {
    queueMicrotask(() => listener(typeof Event === "function" ? new Event("abort") : { type: "abort" }));
  } else if (signal && signal.addEventListener) {
    signal.addEventListener("abort", listener, { once: true });
  }
  return {
    [Symbol.dispose]() {
      if (signal && signal.removeEventListener) signal.removeEventListener("abort", listener);
    },
  };
};
EventEmitter.getEventListeners = topLevelGetEventListeners;
EventEmitter.getMaxListeners = topLevelGetMaxListeners;
EventEmitter.setMaxListeners = topLevelSetMaxListeners;
Object.defineProperty(EventEmitter, "defaultMaxListeners", {
  get() {
    return eventsDefaultMaxListeners;
  },
  set(value) {
    eventsDefaultMaxListeners = Number(value);
  }
});
var eventsModule = {
  addAbortListener,
  defaultMaxListeners: eventsDefaultMaxListeners,
  errorMonitor: eventsErrorMonitor,
  EventEmitter,
  getEventListeners: topLevelGetEventListeners,
  getMaxListeners: topLevelGetMaxListeners,
  listenerCount: topLevelEventListenerCount,
  once,
  setMaxListeners: topLevelSetMaxListeners
};
exposeCustomGlobal("_eventsModule", eventsModule);
export { eventsErrorMonitor, eventsDefaultMaxListeners, emitEventEmitterMeta, cloneEventListeners, removeEventListenerRecord, removeAllEventListenerRecords, emitEventRecords, topLevelEventListenerCount, topLevelGetEventListeners, topLevelGetRawEventListeners, createOnceRawListener, topLevelGetMaxListeners, topLevelSetMaxListeners, addAbortListener, once, initializeEventEmitter, ensureEventEmitterInitialized, createMaxListenersExceededWarning, maybeWarnEventEmitterListeners, addEventListenerRecord, EventEmitter, eventsModule };
