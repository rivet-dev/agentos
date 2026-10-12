import { createBridgeSyncFacade } from "./fs.js";

// This compatibility format is JSON, not the native V8 serialization format.
function v8UnsupportedValueError(description) {
	const error = new TypeError(
		`node:v8 serialize does not support ${description} in agentOS; only JSON-compatible data is supported`,
	);
	error.code = "ERR_NOT_IMPLEMENTED";
	return error;
}

function encodeV8JsonValue(value, seen) {
	if (
		value === null ||
		typeof value === "string" ||
		typeof value === "boolean"
	) {
		return JSON.stringify(value);
	}
	if (typeof value === "number") {
		if (!Number.isFinite(value) || Object.is(value, -0)) {
			throw v8UnsupportedValueError("non-finite numbers or -0");
		}
		return JSON.stringify(value);
	}
	if (typeof value !== "object") {
		throw v8UnsupportedValueError(typeof value);
	}
	if (seen.has(value)) {
		throw v8UnsupportedValueError("shared or circular references");
	}
	seen.add(value);
	const prototype = Object.getPrototypeOf(value);
	const array = Array.isArray(value);
	if (
		array
			? prototype !== Array.prototype
			: prototype !== Object.prototype && prototype !== null
	) {
		throw v8UnsupportedValueError("non-plain objects");
	}
	const descriptors = Object.getOwnPropertyDescriptors(value);
	for (const key of Reflect.ownKeys(descriptors)) {
		if (typeof key === "symbol" && descriptors[key].enumerable) {
			throw v8UnsupportedValueError("enumerable symbol properties");
		}
	}
	const dataValue = (descriptor) => {
		if (
			!descriptor ||
			!Object.prototype.hasOwnProperty.call(descriptor, "value")
		) {
			throw v8UnsupportedValueError("accessor properties or sparse arrays");
		}
		return encodeV8JsonValue(descriptor.value, seen);
	};
	if (array) {
		const length = descriptors.length.value;
		for (const key of Object.keys(descriptors)) {
			if (!descriptors[key].enumerable) continue;
			const index = +key;
			if (
				!Number.isInteger(index) ||
				index < 0 ||
				index >= length ||
				String(index) !== key
			) {
				throw v8UnsupportedValueError(
					"arrays with extra enumerable properties",
				);
			}
		}
		const items = [];
		for (let index = 0; index < length; index += 1) {
			items.push(dataValue(descriptors[index]));
		}
		return `[${items.join(",")}]`;
	}
	const entries = [];
	for (const key of Object.keys(descriptors)) {
		const descriptor = descriptors[key];
		if (descriptor.enumerable) {
			entries.push(`${JSON.stringify(key)}:${dataValue(descriptor)}`);
		}
	}
	return `{${entries.join(",")}}`;
}

function v8Serialize(value) {
	return Buffer.from(encodeV8JsonValue(value, new Set()), "utf8");
}

function v8Deserialize(value) {
	const buffer = Buffer.isBuffer(value) ? value : Buffer.from(value ?? []);
	try {
		return JSON.parse(buffer.toString("utf8"));
	} catch (cause) {
		const error = new TypeError(
			"node:v8 deserialize in agentOS only reads its JSON compatibility format, not native V8 serialization",
			{ cause },
		);
		error.code = "ERR_NOT_IMPLEMENTED";
		throw error;
	}
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

class V8Deserializer {
	constructor(buffer) {
		this._buffer = buffer;
	}
	readHeader() {}
	readValue() {
		return v8Deserialize(this._buffer);
	}
	transferArrayBuffer() {}
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
	DefaultDeserializer: V8Deserializer,
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
