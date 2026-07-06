import { getNowMs } from "./process.js";
import { _queueMicrotask } from "./timers.js";

function createPerfHooksOutOfRangeError(name, requirement, value) {
  const error = new RangeError(
    `The value of "${name}" is out of range. It must be ${requirement}. Received ${String(value)}`
  );
  error.code = "ERR_OUT_OF_RANGE";
  return error;
}

function normalizePerformanceEntry(entry) {
  return {
    name: String(entry?.name ?? ""),
    entryType: String(entry?.entryType ?? ""),
    startTime: Number(entry?.startTime ?? 0),
    duration: Number(entry?.duration ?? 0)
  };
}

function createPerformanceObserverEntryList(entries) {
  return {
    getEntries() {
      return entries.slice();
    },
    getEntriesByName(name, type = void 0) {
      const normalizedName = String(name ?? "");
      const matching = entries.filter((entry) => entry.name === normalizedName);
      if (typeof type === "string") {
        return matching.filter((entry) => entry.entryType === type);
      }
      return matching;
    },
    getEntriesByType(type) {
      const normalizedType = String(type ?? "");
      return entries.filter((entry) => entry.entryType === normalizedType);
    }
  };
}

function createPerformanceHistogram() {
  const values = [];
  return {
    percentile(percentile) {
      const normalizedPercentile = Number(percentile);
      if (!Number.isFinite(normalizedPercentile) || normalizedPercentile <= 0 || normalizedPercentile > 100) {
        throw createPerfHooksOutOfRangeError("percentile", "> 0 && <= 100", percentile);
      }
      if (values.length === 0) {
        return 0;
      }
      const sorted = values.slice().sort((left, right) => left - right);
      const index = Math.min(
        sorted.length - 1,
        Math.max(0, Math.ceil(normalizedPercentile / 100 * sorted.length) - 1)
      );
      return sorted[index];
    },
    record(value) {
      const normalizedValue = typeof value === "bigint" ? Number(value) : Number(value);
      if (!Number.isInteger(normalizedValue)) {
        throw createPerfHooksOutOfRangeError("val", "an integer", value);
      }
      if (normalizedValue < 1 || normalizedValue > Number.MAX_SAFE_INTEGER) {
        throw createPerfHooksOutOfRangeError(
          "val",
          `>= 1 && <= ${Number.MAX_SAFE_INTEGER}`,
          value
        );
      }
      values.push(normalizedValue);
    }
  };
}

var builtinPerformance = (() => {
  const marks = /* @__PURE__ */ new Map();
  const measures = [];
  const observers = /* @__PURE__ */ new Set();
  const perf = typeof performance !== "undefined" && performance && typeof performance.now === "function" ? performance : {
    now() {
      return getNowMs();
    },
    timeOrigin: Date.now() - getNowMs()
  };
  if (typeof perf.mark !== "function") {
    perf.mark = function(name) {
      const entry = {
        name: String(name ?? ""),
        entryType: "mark",
        startTime: perf.now(),
        duration: 0
      };
      const entries = marks.get(entry.name) ?? [];
      entries.push(entry);
      marks.set(entry.name, entries);
      return entry;
    };
  }
  if (typeof perf.measure !== "function") {
    perf.measure = function(name, startMarkOrOptions, endMark) {
      const normalizedName = String(name ?? "");
      let startTime = 0;
      let endTimeMs = perf.now();
      if (typeof startMarkOrOptions === "string") {
        const startEntries = marks.get(startMarkOrOptions);
        if (startEntries?.length) {
          startTime = startEntries[startEntries.length - 1].startTime;
        }
        if (typeof endMark === "string") {
          const endEntries = marks.get(endMark);
          if (endEntries?.length) {
            endTimeMs = endEntries[endEntries.length - 1].startTime;
          }
        }
      } else if (startMarkOrOptions && typeof startMarkOrOptions === "object") {
        if (typeof startMarkOrOptions.start === "number") {
          startTime = startMarkOrOptions.start;
        } else if (typeof startMarkOrOptions.startMark === "string") {
          const startEntries = marks.get(startMarkOrOptions.startMark);
          if (startEntries?.length) {
            startTime = startEntries[startEntries.length - 1].startTime;
          }
        }
        if (typeof startMarkOrOptions.end === "number") {
          endTimeMs = startMarkOrOptions.end;
        } else if (typeof startMarkOrOptions.endMark === "string") {
          const endEntries = marks.get(startMarkOrOptions.endMark);
          if (endEntries?.length) {
            endTimeMs = endEntries[endEntries.length - 1].startTime;
          }
        }
      }
      const entry = {
        name: normalizedName,
        entryType: "measure",
        startTime,
        duration: Math.max(0, endTimeMs - startTime)
      };
      measures.push(entry);
      return entry;
    };
  }
  if (typeof perf.getEntriesByName !== "function") {
    perf.getEntriesByName = function(name, type = void 0) {
      const normalizedName = String(name ?? "");
      const markEntries = marks.get(normalizedName) ?? [];
      const combined = [...markEntries, ...measures.filter((entry) => entry.name === normalizedName)];
      if (typeof type === "string") {
        return combined.filter((entry) => entry.entryType === type);
      }
      return combined;
    };
  }
  if (typeof perf.getEntries !== "function") {
    perf.getEntries = function() {
      return [...marks.values()].flatMap((entries) => [...entries]).concat(measures);
    };
  }
  if (typeof perf.getEntriesByType !== "function") {
    perf.getEntriesByType = function(type) {
      const normalizedType = String(type ?? "");
      return perf.getEntries().filter((entry) => entry.entryType === normalizedType);
    };
  }
  if (typeof perf.clearMarks !== "function") {
    perf.clearMarks = function(name = void 0) {
      if (typeof name === "undefined") {
        marks.clear();
        return;
      }
      marks.delete(String(name));
    };
  }
  if (typeof perf.clearMeasures !== "function") {
    perf.clearMeasures = function(name = void 0) {
      if (typeof name === "undefined") {
        measures.length = 0;
        return;
      }
      const normalizedName = String(name);
      for (let index = measures.length - 1; index >= 0; index -= 1) {
        if (measures[index]?.name === normalizedName) {
          measures.splice(index, 1);
        }
      }
    };
  }
  const queueObserverDelivery = (observer) => {
    if (observer._deliveryQueued) {
      return;
    }
    observer._deliveryQueued = true;
    _queueMicrotask(() => {
      observer._deliveryQueued = false;
      if (!observer._connected) {
        return;
      }
      const records = observer.takeRecords();
      observer._callback(createPerformanceObserverEntryList(records), observer);
    });
  };
  const emitEntry = (entry) => {
    const normalizedEntry = normalizePerformanceEntry(entry);
    for (const observer of observers) {
      if (!observer._entryTypes.has(normalizedEntry.entryType)) {
        continue;
      }
      observer._records.push(normalizedEntry);
      queueObserverDelivery(observer);
    }
  };
  const originalMark = perf.mark.bind(perf);
  perf.mark = function(...args) {
    const entry = originalMark(...args);
    emitEntry(entry);
    return entry;
  };
  const originalMeasure = perf.measure.bind(perf);
  perf.measure = function(...args) {
    const entry = originalMeasure(...args);
    emitEntry(entry);
    return entry;
  };
  perf.__agentOSObservers = observers;
  return perf;
})();

var builtinPerfHooksModule = {
  PerformanceObserver: class {
    constructor(callback) {
      if (typeof callback !== "function") {
        throw new TypeError("PerformanceObserver callback must be a function");
      }
      this._callback = callback;
      this._connected = false;
      this._deliveryQueued = false;
      this._entryTypes = /* @__PURE__ */ new Set();
      this._records = [];
    }
    static get supportedEntryTypes() {
      return ["mark", "measure"];
    }
    observe(options = {}) {
      const entryTypes = Array.isArray(options?.entryTypes) ? options.entryTypes.map((entryType) => String(entryType)) : typeof options?.type === "string" ? [String(options.type)] : [];
      if (entryTypes.length === 0) {
        throw new TypeError("PerformanceObserver.observe() requires an entryTypes array or type string");
      }
      this.disconnect();
      this._entryTypes = new Set(entryTypes);
      this._connected = true;
      builtinPerformance.__agentOSObservers.add(this);
      if (options?.buffered) {
        for (const entryType of this._entryTypes) {
          for (const entry of builtinPerformance.getEntriesByType(entryType)) {
            this._records.push(normalizePerformanceEntry(entry));
          }
        }
        if (this._records.length > 0) {
          this._deliveryQueued = true;
          _queueMicrotask(() => {
            this._deliveryQueued = false;
            if (!this._connected) {
              return;
            }
            const records = this.takeRecords();
            this._callback(createPerformanceObserverEntryList(records), this);
          });
        }
      }
    }
    disconnect() {
      this._connected = false;
      builtinPerformance.__agentOSObservers.delete(this);
    }
    takeRecords() {
      const records = this._records.slice();
      this._records.length = 0;
      return records;
    }
  },
  constants: {},
  createHistogram() {
    return createPerformanceHistogram();
  },
  performance: builtinPerformance
};
export { builtinPerfHooksModule, builtinPerformance, createPerfHooksOutOfRangeError, createPerformanceHistogram, createPerformanceObserverEntryList, normalizePerformanceEntry };
