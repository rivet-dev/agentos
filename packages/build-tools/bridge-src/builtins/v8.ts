// node:v8 serialize/deserialize is a JSON stub, not the V8 wire format. Values
// JSON cannot keep exactly throw ERR_NOT_IMPLEMENTED instead of round-tripping
// as corrupted data (Map -> {}, Date -> string, shared refs -> copies).
function v8UnsupportedValueError(description) {
  const error = new TypeError(
    `node:v8 serialize does not support ${description} in agentOS; only JSON-safe values are supported`
  );
  error.code = "ERR_NOT_IMPLEMENTED";
  return error;
}

function describeV8Value(value) {
  if (value === null || (typeof value !== "object" && typeof value !== "function")) {
    return typeof value;
  }
  const tag = Object.prototype.toString.call(value).slice(8, -1);
  const name = value.constructor && typeof value.constructor.name === "string" ? value.constructor.name : "";
  return name && name !== tag ? `${tag} (${name})` : tag;
}

function assertJsonSafeV8Value(value, seen) {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return;
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value) || Object.is(value, -0)) {
      throw v8UnsupportedValueError(`the number ${Object.is(value, -0) ? "-0" : String(value)}`);
    }
    return;
  }
  if (typeof value !== "object") {
    throw v8UnsupportedValueError(typeof value);
  }
  if (seen.has(value)) {
    throw v8UnsupportedValueError("shared or circular object references");
  }
  seen.add(value);
  const proto = Object.getPrototypeOf(value);
  if (Array.isArray(value) && proto === Array.prototype) {
    if (Object.keys(value).length !== value.length) {
      throw v8UnsupportedValueError("sparse arrays or arrays with extra properties");
    }
    for (const item of value) {
      assertJsonSafeV8Value(item, seen);
    }
    return;
  }
  if (proto !== Object.prototype && proto !== null) {
    throw v8UnsupportedValueError(describeV8Value(value));
  }
  for (const key of Object.keys(value)) {
    assertJsonSafeV8Value(value[key], seen);
  }
}

function v8Serialize(value) {
  assertJsonSafeV8Value(value, new Set());
  return Buffer.from(JSON.stringify(value), "utf8");
}

function v8Deserialize(value) {
  const buffer = Buffer.isBuffer(value) ? value : Buffer.from(value ?? []);
  try {
    return JSON.parse(buffer.toString("utf8"));
  } catch (cause) {
    const error = new TypeError(
      "node:v8 deserialize in agentOS only reads buffers produced by its own JSON-based serialize, not the V8 wire format",
      { cause }
    );
    error.code = "ERR_NOT_IMPLEMENTED";
    throw error;
  }
}

class V8Serializer {
  constructor() {
    this._value = null;
  }
  writeHeader() {
  }
  writeValue(value) {
    this._value = value;
  }
  releaseBuffer() {
    return v8Serialize(this._value);
  }
  transferArrayBuffer() {
  }
}

class V8Deserializer {
  constructor(buffer) {
    this._buffer = buffer;
  }
  readHeader() {
  }
  readValue() {
    return v8Deserialize(this._buffer);
  }
  transferArrayBuffer() {
  }
}

function configuredHeapLimitBytes() {
  const configured = Number(globalThis.__agentOSV8HeapLimitBytes);
  if (Number.isFinite(configured) && configured > 0) {
    return configured;
  }
  return 128 * 1024 * 1024;
}

function getHeapStatistics() {
  const heapLimit = configuredHeapLimitBytes();
  return {
    total_heap_size: Math.max(64 * 1024 * 1024, Math.floor(heapLimit / 2)),
    total_heap_size_executable: 1024 * 1024,
    total_physical_size: Math.max(64 * 1024 * 1024, Math.floor(heapLimit / 2)),
    total_available_size: Math.max(0, heapLimit - 64 * 1024 * 1024),
    used_heap_size: Math.max(0, Math.min(heapLimit, Math.floor(heapLimit * 0.4))),
    heap_size_limit: heapLimit,
    malloced_memory: 8192,
    peak_malloced_memory: 16384,
    does_zap_garbage: 0,
    number_of_native_contexts: 1,
    number_of_detached_contexts: 0,
    total_global_handles_size: 16384,
    used_global_handles_size: 8192,
    external_memory: 0
  };
}

function getHeapSpaceStatistics() {
  return [];
}

function getHeapCodeStatistics() {
  return {
    code_and_metadata_size: 0,
    bytecode_and_metadata_size: 0,
    external_script_source_size: 0,
    cpu_profiler_metadata_size: 0
  };
}

function getCppHeapStatistics() {
  return {
    committed_size_bytes: 0,
    resident_size_bytes: 0,
    used_size_bytes: 0,
    space_statistics: []
  };
}

function getHeapSnapshot() {
  return Readable.fromWeb(
    new ReadableStream({
      start(controller) {
        controller.enqueue(Buffer.from("{}"));
        controller.close();
      }
    })
  );
}

var builtinV8Module = {
  cachedDataVersionTag() {
    return 0;
  },
  DefaultDeserializer: V8Deserializer,
  DefaultSerializer: V8Serializer,
  Deserializer: V8Deserializer,
  GCProfiler: class GCProfiler {
    start() {
    }
    stop() {
      return [];
    }
  },
  Serializer: V8Serializer,
  deserialize: v8Deserialize,
  getCppHeapStatistics,
  getHeapCodeStatistics,
  getHeapSnapshot,
  getHeapSpaceStatistics,
  getHeapStatistics,
  isStringOneByteRepresentation(value) {
    return typeof value === "string" && !/[^\x00-\xff]/.test(value);
  },
  promiseHooks: {},
  queryObjects() {
    return [];
  },
  serialize: v8Serialize,
  setFlagsFromString() {
  },
  setHeapSnapshotNearHeapLimit() {
    return [];
  },
  startCpuProfile() {
    return {
      stop() {
        return {};
      }
    };
  },
  startupSnapshot: {},
  stopCoverage() {
    return [];
  },
  takeCoverage() {
    return [];
  },
  writeHeapSnapshot() {
    return "";
  }
};
export { V8Deserializer, V8Serializer, builtinV8Module, configuredHeapLimitBytes, getCppHeapStatistics, getHeapCodeStatistics, getHeapSnapshot, getHeapSpaceStatistics, getHeapStatistics, v8Deserialize, v8Serialize };
