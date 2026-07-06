import { NativeAbortControllerGlobal, NativeAbortSignalGlobal } from "../prelude.js";
import { Event, EventTarget } from "./dom-events.js";

var AbortSignal = typeof NativeAbortSignalGlobal === "function" ? NativeAbortSignalGlobal : class extends EventTarget {
  constructor() {
    super();
    this.aborted = false;
    this.reason = void 0;
  }
  throwIfAborted() {
    if (this.aborted) {
      throw this.reason instanceof Error ? this.reason : new Error(String(this.reason ?? "AbortError"));
    }
  }
};
var AbortController = typeof NativeAbortControllerGlobal === "function" ? NativeAbortControllerGlobal : class {
  constructor() {
    this.signal = new AbortSignal();
  }
  abort(reason) {
    if (this.signal.aborted) {
      return;
    }
    this.signal.aborted = true;
    this.signal.reason = reason;
    this.signal.dispatchEvent(new Event("abort"));
  }
};
function ensureNamedConstructor(ctor, expectedName) {
  if (typeof ctor !== "function") {
    return;
  }
  try {
    if (ctor.name !== expectedName) {
      Object.defineProperty(ctor, "name", {
        configurable: true,
        value: expectedName
      });
    }
  } catch {
  }
}
ensureNamedConstructor(AbortSignal, "AbortSignal");
ensureNamedConstructor(AbortController, "AbortController");
try {
  const signalCtor = Object.getPrototypeOf(new AbortController().signal)?.constructor;
  ensureNamedConstructor(signalCtor, "AbortSignal");
} catch {
}
try {
  globalThis.AbortSignal = AbortSignal;
} catch {
}
try {
  globalThis.AbortController = AbortController;
} catch {
}
function createAbortSignalReason(reason) {
  if (reason !== void 0) {
    return reason;
  }
  if (typeof globalThis.DOMException === "function") {
    return new globalThis.DOMException("This operation was aborted", "AbortError");
  }
  const error = new Error("This operation was aborted");
  error.name = "AbortError";
  return error;
}
function createAbortedSignal(reason) {
  const controller = new AbortController();
  controller.abort(createAbortSignalReason(reason));
  return controller.signal;
}
function normalizeAbortSignalTimeout(delay) {
  if (typeof delay !== "number") {
    throw new TypeError(`The "delay" argument must be of type number. Received ${typeof delay}`);
  }
  if (!Number.isFinite(delay) || delay < 0) {
    throw new RangeError(`The value of "delay" is out of range. It must be >= 0. Received ${String(delay)}`);
  }
  return Math.trunc(delay);
}
if (typeof AbortSignal.abort !== "function") {
  Object.defineProperty(AbortSignal, "abort", {
    configurable: true,
    writable: true,
    value(reason = void 0) {
      return createAbortedSignal(reason);
    }
  });
}
if (typeof AbortSignal.timeout !== "function") {
  Object.defineProperty(AbortSignal, "timeout", {
    configurable: true,
    writable: true,
    value(delay) {
      const timeout = normalizeAbortSignalTimeout(delay);
      const controller = new AbortController();
      const timer = setTimeout(() => {
        controller.abort(createAbortSignalReason());
      }, timeout);
      if (typeof timer?.unref === "function") {
        timer.unref();
      }
      controller.signal.addEventListener("abort", () => {
        clearTimeout(timer);
      }, { once: true });
      return controller.signal;
    }
  });
}
if (typeof AbortSignal.any !== "function") {
  Object.defineProperty(AbortSignal, "any", {
    configurable: true,
    writable: true,
    value(signals) {
      if (!signals || typeof signals[Symbol.iterator] !== "function") {
        throw new TypeError("The \"signals\" argument must be an iterable");
      }
      const inputs = Array.from(signals);
      const controller = new AbortController();
      if (inputs.length === 0) {
        return controller.signal;
      }
      const listeners = [];
      const abortFromSignal = (signal) => {
        while (listeners.length > 0) {
          const [candidate, listener] = listeners.pop();
          candidate.removeEventListener?.("abort", listener);
        }
        controller.abort(signal.reason);
      };
      for (const signal of inputs) {
        if (!signal || typeof signal.aborted !== "boolean" || typeof signal.addEventListener !== "function") {
          throw new TypeError("The \"signals\" argument must contain AbortSignal instances");
        }
        if (signal.aborted) {
          abortFromSignal(signal);
          return controller.signal;
        }
        const onAbort = () => abortFromSignal(signal);
        listeners.push([signal, onAbort]);
        signal.addEventListener("abort", onAbort, { once: true });
      }
      return controller.signal;
    }
  });
}

export { AbortSignal, AbortController, ensureNamedConstructor, createAbortSignalReason, createAbortedSignal, normalizeAbortSignalTimeout };
