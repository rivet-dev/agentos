import { createBridgeSyncFacade } from "./fs.js";

// Keep values on the owning isolate: a JSON/CBOR bridge would lose graph
// identity, bigint, and typed views before V8's serializer could see them.
const nativeSerialize = createBridgeSyncFacade("v8.serialize");
const nativeDeserialize = createBridgeSyncFacade("v8.deserialize");
function v8Serialize(value) {
	return Buffer.from(nativeSerialize.applySyncPromise(void 0, [value]));
}
function v8Deserialize(value) {
	if (!ArrayBuffer.isView(value)) {
		const error = new TypeError("The buffer argument must be a TypedArray or DataView");
		error.code = "ERR_INVALID_ARG_TYPE";
		throw error;
	}
	return nativeDeserialize.applySyncPromise(void 0, [value]);
}

class V8Serializer {
	constructor() {
		this._value = null;
	}
	writeHeader() {}
	writeValue(value) {
		this._value = value;
	}
	releaseBuffer() {
		return v8Serialize(this._value);
	}
	transferArrayBuffer() {}
}

const nativeDeserializerOperations = new Map();
function deserializerOperation(receiver, operation, ...args) {
 let call = nativeDeserializerOperations.get(operation);
 if (!call) {
  const name="v8.deserializer."+operation, bridge=globalThis[name];
  if(typeof bridge!=="function" && typeof bridge?.applySync!=="function") throw new Error("Native V8 Deserializer bridge is unavailable");
  call=createBridgeSyncFacade(name);nativeDeserializerOperations.set(operation,call);
 }
 return call.applySyncPromise(void 0, [receiver, ...args]);
}
class V8Deserializer {
 constructor(buffer) { deserializerOperation(this, "create", buffer); }
 readHeader() { return deserializerOperation(this, "readHeader"); }
 readValue() { return deserializerOperation(this, "readValue"); }
 readUint32() { return deserializerOperation(this, "readUint32"); }
 readUint64() { return deserializerOperation(this, "readUint64"); }
 readDouble() { return deserializerOperation(this, "readDouble"); }
 readRawBytes(length) { return Buffer.from(deserializerOperation(this, "readRawBytes", length)); }
 getWireFormatVersion() { return deserializerOperation(this, "getWireFormatVersion"); }
 transferArrayBuffer(id, buffer) { deserializerOperation(this, "transferArrayBuffer", id, buffer); }
 _readHostObject() { throw new Error("Unable to deserialize host object"); }
}
class V8DefaultDeserializer extends V8Deserializer {
 _readHostObject() { return deserializerOperation(this, "readHostObject"); }
}

var nativeV8HeapStatistics = createBridgeSyncFacade("v8.getHeapStatistics");

function getHeapStatistics() {
	const statistics = nativeV8HeapStatistics.applySyncPromise(void 0, []);
	if (!statistics || typeof statistics !== "object") {
		const error = new Error(
			"The native V8 heap-statistics bridge is unavailable",
		);
		error.code = "ERR_AGENTOS_V8_HEAP_STATISTICS";
		throw error;
	}
	return statistics;
}

function getHeapSpaceStatistics() {
	return [];
}

function getHeapCodeStatistics() {
	return {
		code_and_metadata_size: 0,
		bytecode_and_metadata_size: 0,
		external_script_source_size: 0,
		cpu_profiler_metadata_size: 0,
	};
}

function getCppHeapStatistics() {
	return {
		committed_size_bytes: 0,
		resident_size_bytes: 0,
		used_size_bytes: 0,
		space_statistics: [],
	};
}

function getHeapSnapshot() {
	return Readable.fromWeb(
		new ReadableStream({
			start(controller) {
				controller.enqueue(Buffer.from("{}"));
				controller.close();
			},
		}),
	);
}

var builtinV8Module = {
	cachedDataVersionTag() {
		return 0;
	},
	DefaultDeserializer: V8DefaultDeserializer,
	DefaultSerializer: V8Serializer,
	Deserializer: V8Deserializer,
	GCProfiler: class GCProfiler {
		start() {}
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
	setFlagsFromString() {},
	setHeapSnapshotNearHeapLimit() {
		return [];
	},
	startCpuProfile() {
		return {
			stop() {
				return {};
			},
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
	},
};

export {
	builtinV8Module,
	getCppHeapStatistics,
	getHeapCodeStatistics,
	getHeapSnapshot,
	getHeapSpaceStatistics,
	getHeapStatistics,
	V8Deserializer,
	V8Serializer,
	v8Deserialize,
	v8Serialize,
};
