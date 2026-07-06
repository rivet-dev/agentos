function normalizeAddEventListenerOptions(options) {
  if (typeof options === "boolean") {
    return {
      capture: options,
      once: false,
      passive: false
    };
  }
  if (options == null) {
    return {
      capture: false,
      once: false,
      passive: false
    };
  }
  const normalized = Object(options);
  return {
    capture: Boolean(normalized.capture),
    once: Boolean(normalized.once),
    passive: Boolean(normalized.passive),
    signal: normalized.signal
  };
}
function normalizeRemoveEventListenerOptions(options) {
  if (typeof options === "boolean") {
    return options;
  }
  if (options == null) {
    return false;
  }
  return Boolean(Object(options).capture);
}
function isAbortSignalLike(value) {
  return typeof value === "object" && value !== null && "aborted" in value && typeof value.addEventListener === "function" && typeof value.removeEventListener === "function";
}
var PatchedEvent = class {
  static NONE = 0;
  static CAPTURING_PHASE = 1;
  static AT_TARGET = 2;
  static BUBBLING_PHASE = 3;
  type;
  bubbles;
  cancelable;
  composed;
  detail = null;
  defaultPrevented = false;
  target = null;
  currentTarget = null;
  eventPhase = 0;
  returnValue = true;
  cancelBubble = false;
  timeStamp = Date.now();
  isTrusted = false;
  srcElement = null;
  inPassiveListener = false;
  propagationStopped = false;
  immediatePropagationStopped = false;
  constructor(type, init) {
    if (arguments.length === 0) {
      throw new TypeError("The event type must be provided");
    }
    const normalizedInit = init == null ? {} : Object(init);
    this.type = String(type);
    this.bubbles = Boolean(normalizedInit.bubbles);
    this.cancelable = Boolean(normalizedInit.cancelable);
    this.composed = Boolean(normalizedInit.composed);
  }
  get [Symbol.toStringTag]() {
    return "Event";
  }
  preventDefault() {
    if (this.cancelable && !this.inPassiveListener) {
      this.defaultPrevented = true;
      this.returnValue = false;
    }
  }
  stopPropagation() {
    this.propagationStopped = true;
    this.cancelBubble = true;
  }
  stopImmediatePropagation() {
    this.propagationStopped = true;
    this.immediatePropagationStopped = true;
    this.cancelBubble = true;
  }
  composedPath() {
    return this.target ? [this.target] : [];
  }
  _setPassive(value) {
    this.inPassiveListener = value;
  }
  _isPropagationStopped() {
    return this.propagationStopped;
  }
  _isImmediatePropagationStopped() {
    return this.immediatePropagationStopped;
  }
};
var PatchedCustomEvent = class extends PatchedEvent {
  constructor(type, init) {
    super(type, init);
    const normalizedInit = init == null ? null : Object(init);
    this.detail = normalizedInit && "detail" in normalizedInit ? normalizedInit.detail : null;
  }
  get [Symbol.toStringTag]() {
    return "CustomEvent";
  }
};
var PatchedEventTarget = class {
  listeners = /* @__PURE__ */ new Map();
  addEventListener(type, listener, options) {
    const normalized = normalizeAddEventListenerOptions(options);
    if (normalized.signal !== void 0 && !isAbortSignalLike(normalized.signal)) {
      throw new TypeError(
        'The "signal" option must be an instance of AbortSignal.'
      );
    }
    if (listener == null) {
      return void 0;
    }
    if (typeof listener !== "function" && (typeof listener !== "object" || listener === null)) {
      return void 0;
    }
    if (normalized.signal?.aborted) {
      return void 0;
    }
    const records = this.listeners.get(type) ?? [];
    const existing = records.find(
      (record2) => record2.listener === listener && record2.capture === normalized.capture
    );
    if (existing) {
      return void 0;
    }
    const record = {
      listener,
      capture: normalized.capture,
      once: normalized.once,
      passive: normalized.passive,
      kind: typeof listener === "function" ? "function" : "object",
      signal: normalized.signal
    };
    if (normalized.signal) {
      record.abortListener = () => {
        this.removeEventListener(type, listener, normalized.capture);
      };
      normalized.signal.addEventListener("abort", record.abortListener, {
        once: true
      });
    }
    records.push(record);
    this.listeners.set(type, records);
    return void 0;
  }
  removeEventListener(type, listener, options) {
    if (listener == null) {
      return;
    }
    const capture = normalizeRemoveEventListenerOptions(options);
    const records = this.listeners.get(type);
    if (!records) {
      return;
    }
    const nextRecords = records.filter((record) => {
      const match = record.listener === listener && record.capture === capture;
      if (match && record.signal && record.abortListener) {
        record.signal.removeEventListener("abort", record.abortListener);
      }
      return !match;
    });
    if (nextRecords.length === 0) {
      this.listeners.delete(type);
      return;
    }
    this.listeners.set(type, nextRecords);
  }
  dispatchEvent(event) {
    if (typeof event !== "object" || event === null || typeof event.type !== "string") {
      throw new TypeError("Argument 1 must be an Event");
    }
    const patchedEvent = event;
    const records = (this.listeners.get(patchedEvent.type) ?? []).slice();
    patchedEvent.target = this;
    patchedEvent.currentTarget = this;
    patchedEvent.eventPhase = 2;
    for (const record of records) {
      const active = this.listeners.get(patchedEvent.type)?.includes(record);
      if (!active) {
        continue;
      }
      if (record.once) {
        this.removeEventListener(patchedEvent.type, record.listener, record.capture);
      }
      patchedEvent._setPassive(record.passive);
      if (record.kind === "function") {
        record.listener.call(this, patchedEvent);
      } else {
        const handleEvent = record.listener.handleEvent;
        if (typeof handleEvent === "function") {
          handleEvent.call(record.listener, patchedEvent);
        }
      }
      patchedEvent._setPassive(false);
      if (patchedEvent._isImmediatePropagationStopped()) {
        break;
      }
      if (patchedEvent._isPropagationStopped()) {
        break;
      }
    }
    patchedEvent.currentTarget = null;
    patchedEvent.eventPhase = 0;
    return !patchedEvent.defaultPrevented;
  }
};
var Event = PatchedEvent;
var CustomEvent = PatchedCustomEvent;
var EventTarget = PatchedEventTarget;

export { normalizeAddEventListenerOptions, normalizeRemoveEventListenerOptions, isAbortSignalLike, PatchedEvent, PatchedCustomEvent, PatchedEventTarget, Event, CustomEvent, EventTarget };
