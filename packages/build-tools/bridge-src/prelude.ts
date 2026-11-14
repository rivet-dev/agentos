import * as bufferStdlibModuleNs from "node:buffer";
import * as constantsStdlibModuleNs from "node:constants";
import * as eventsStdlibModuleNs from "node:events";
import * as pathStdlibModuleNs from "node:path";
import * as punycodeStdlibModuleNs from "node:punycode";
import * as querystringStdlibModuleNs from "node:querystring";
import * as streamStdlibModuleNs from "node:stream";
import * as stringDecoderStdlibModuleNs from "node:string_decoder";
import * as urlStdlibModuleNs from "node:url";
import * as utilStdlibModuleNs from "node:util";
import undiciApiModule from "undici/lib/api/index.js";
import undiciAgentModule from "undici/lib/dispatcher/agent.js";
import undiciClientModule from "undici/lib/dispatcher/client.js";
import undiciGlobalModule from "undici/lib/global.js";
import undiciFormDataModule from "undici/lib/web/fetch/formdata.js";
import undiciHeadersModule from "undici/lib/web/fetch/headers.js";
import undiciFetchModule from "undici/lib/web/fetch/index.js";
import undiciRequestModule from "undici/lib/web/fetch/request.js";
import undiciResponseModule from "undici/lib/web/fetch/response.js";
import undiciWebidlModule from "undici/lib/web/webidl/index.js";
import {
	WebReadableStream,
	WebTextDecoderStream,
	WebTextEncoderStream,
	WebTransformStream,
	WebWritableStream,
} from "./undici-shims/web-streams-global.js";
import loadWebSocketModule from "./undici-shims/websocket-lazy.js";

const EarlyBufferGlobal =
	bufferStdlibModuleNs.Buffer ??
	bufferStdlibModuleNs.default?.Buffer ??
	bufferStdlibModuleNs.default;
if (typeof EarlyBufferGlobal === "function") {
	delete EarlyBufferGlobal.TYPED_ARRAY_SUPPORT;
}
function normalizeBase64UrlEncoding(encoding) {
	return typeof encoding === "string" && encoding.toLowerCase() === "base64url"
		? "base64"
		: encoding;
}
function base64ToBase64Url(value) {
	return String(value)
		.replace(/\+/g, "-")
		.replace(/\//g, "_")
		.replace(/=+$/g, "");
}
function installBase64UrlBufferEncoding(BufferCtor) {
	if (
		typeof BufferCtor !== "function" ||
		BufferCtor.__agentOSBase64UrlPatched
	) {
		return;
	}
	const originalIsEncoding =
		typeof BufferCtor.isEncoding === "function"
			? BufferCtor.isEncoding.bind(BufferCtor)
			: null;
	const originalByteLength =
		typeof BufferCtor.byteLength === "function"
			? BufferCtor.byteLength.bind(BufferCtor)
			: null;
	const originalToString =
		typeof BufferCtor.prototype?.toString === "function"
			? BufferCtor.prototype.toString
			: null;
	const originalWrite =
		typeof BufferCtor.prototype?.write === "function"
			? BufferCtor.prototype.write
			: null;
	BufferCtor.isEncoding = function isEncodingPatched(encoding) {
		return (
			(typeof encoding === "string" &&
				encoding.toLowerCase() === "base64url") ||
			originalIsEncoding?.(encoding) === true
		);
	};
	if (originalByteLength) {
		BufferCtor.byteLength = function byteLengthPatched(
			value,
			encoding,
			...rest
		) {
			return originalByteLength(
				value,
				normalizeBase64UrlEncoding(encoding),
				...rest,
			);
		};
	}
	if (originalToString) {
		BufferCtor.prototype.toString = function toStringPatched(
			encoding,
			...rest
		) {
			if (
				typeof encoding === "string" &&
				encoding.toLowerCase() === "base64url"
			) {
				return base64ToBase64Url(
					originalToString.call(this, "base64", ...rest),
				);
			}
			return originalToString.call(this, encoding, ...rest);
		};
	}
	if (originalWrite) {
		BufferCtor.prototype.write = function writePatched(
			string,
			offset,
			length,
			encoding,
		) {
			if (typeof offset === "string") {
				offset = normalizeBase64UrlEncoding(offset);
			} else if (typeof length === "string") {
				length = normalizeBase64UrlEncoding(length);
			} else if (typeof encoding === "string") {
				encoding = normalizeBase64UrlEncoding(encoding);
			}
			return originalWrite.call(this, string, offset, length, encoding);
		};
	}
	BufferCtor.__agentOSBase64UrlPatched = true;
}
installBase64UrlBufferEncoding(EarlyBufferGlobal);
// Node accepts every genuine Uint8Array (including foreign-realm views) here.
// The vendored browser Buffer implementation still accepts only Buffer.
if (typeof EarlyBufferGlobal === "function") {
	const typedArrayTag = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), Symbol.toStringTag).get;
	EarlyBufferGlobal.prototype.equals = function equals(other) {
		if (Reflect.apply(typedArrayTag, other, []) !== "Uint8Array") {
			throw Object.assign(new TypeError('The "otherBuffer" argument must be a Buffer or Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
		}
		return EarlyBufferGlobal.compare(this, other) === 0;
	};
}
if (typeof EarlyBufferGlobal === "function") {
	globalThis.Buffer = EarlyBufferGlobal;
}

const EarlyUtilTypes =
	utilStdlibModuleNs.types ?? utilStdlibModuleNs.default?.types;
const StructuredCloneTypedArrayCtors = new Map([
	["[object Int8Array]", Int8Array],
	["[object Uint8Array]", Uint8Array],
	["[object Uint8ClampedArray]", Uint8ClampedArray],
	["[object Int16Array]", Int16Array],
	["[object Uint16Array]", Uint16Array],
	["[object Int32Array]", Int32Array],
	["[object Uint32Array]", Uint32Array],
	["[object Float32Array]", Float32Array],
	["[object Float64Array]", Float64Array],
	["[object BigInt64Array]", BigInt64Array],
	["[object BigUint64Array]", BigUint64Array],
]);
function createStructuredCloneDataCloneError(
	message = "The object could not be cloned.",
) {
	if (typeof globalThis.DOMException === "function") {
		return new globalThis.DOMException(message, "DataCloneError");
	}
	const error = new Error(message);
	error.name = "DataCloneError";
	error.code = 25;
	return error;
}
function cloneStructuredArrayBuffer(value) {
	const clone = new ArrayBuffer(value.byteLength);
	new Uint8Array(clone).set(new Uint8Array(value));
	return clone;
}
function cloneStructuredValue(value, seen) {
	if (value === null) {
		return value;
	}
	const valueType = typeof value;
	if (valueType === "function" || valueType === "symbol") {
		throw createStructuredCloneDataCloneError();
	}
	if (valueType !== "object") {
		return value;
	}
	const cached = seen.get(value);
	if (cached !== void 0) {
		return cached;
	}
	const tag = Object.prototype.toString.call(value);
	const TypedArrayCtor = StructuredCloneTypedArrayCtors.get(tag);
	if (TypedArrayCtor) {
		const clonedBuffer = cloneStructuredValue(value.buffer, seen);
		const clone = new TypedArrayCtor(
			clonedBuffer,
			value.byteOffset,
			value.length,
		);
		seen.set(value, clone);
		return clone;
	}
	if (tag === "[object ArrayBuffer]") {
		const clone = cloneStructuredArrayBuffer(value);
		seen.set(value, clone);
		return clone;
	}
	if (tag === "[object DataView]") {
		const clonedBuffer = cloneStructuredValue(value.buffer, seen);
		const clone = new DataView(
			clonedBuffer,
			value.byteOffset,
			value.byteLength,
		);
		seen.set(value, clone);
		return clone;
	}
	if (tag === "[object Date]") {
		const clone = new Date(value.getTime());
		seen.set(value, clone);
		return clone;
	}
	if (tag === "[object RegExp]") {
		const clone = new RegExp(value.source, value.flags);
		clone.lastIndex = value.lastIndex;
		seen.set(value, clone);
		return clone;
	}
	if (tag === "[object Map]") {
		const clone = new Map();
		seen.set(value, clone);
		for (const [entryKey, entryValue] of value.entries()) {
			clone.set(
				cloneStructuredValue(entryKey, seen),
				cloneStructuredValue(entryValue, seen),
			);
		}
		return clone;
	}
	if (tag === "[object Set]") {
		const clone = new Set();
		seen.set(value, clone);
		for (const entryValue of value.values()) {
			clone.add(cloneStructuredValue(entryValue, seen));
		}
		return clone;
	}
	if (tag === "[object Array]") {
		const clone = new Array(value.length);
		seen.set(value, clone);
		for (const key of Object.keys(value)) {
			clone[key] = cloneStructuredValue(value[key], seen);
		}
		return clone;
	}
	if (tag === "[object Object]") {
		const clone =
			Object.getPrototypeOf(value) === null ? Object.create(null) : {};
		seen.set(value, clone);
		for (const key of Object.keys(value)) {
			clone[key] = cloneStructuredValue(value[key], seen);
		}
		return clone;
	}
	throw createStructuredCloneDataCloneError();
}
const structuredCloneBufferTransfer = ArrayBuffer.prototype.transfer;
const structuredCloneBufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get;
const structuredCloneApply = Reflect.apply;
const StructuredCloneUint8Array = Uint8Array;
function validateStructuredTransfer(buffer) {
 try {
  structuredCloneApply(structuredCloneBufferLength, buffer, []);
  // A detached zero-byte buffer must remain distinct from a live empty buffer.
  new StructuredCloneUint8Array(buffer);
 } catch { throw createStructuredCloneDataCloneError("Transfer list contains an invalid ArrayBuffer"); }
}
function sandboxStructuredClone(value, options = void 0) {
 const transfers = new Set();
 const list = options == null ? undefined : options.transfer;
 if (list !== undefined) {
  if (list == null || typeof list[Symbol.iterator] !== "function") throw new TypeError("Transfer list must be iterable");
  for (const buffer of list) {
   if (transfers.has(buffer)) throw createStructuredCloneDataCloneError("Transfer list contains a duplicate ArrayBuffer");
   validateStructuredTransfer(buffer);
   transfers.add(buffer);
  }
  if (transfers.size && typeof structuredCloneBufferTransfer !== "function") throw createStructuredCloneDataCloneError("ArrayBuffer transfer is unavailable");
 }
 // Validate and clone the complete graph before detaching any source. A getter
 // can fail or detach a listed buffer, so revalidate before committing transfers.
 const cloned = cloneStructuredValue(value, new Map());
 for (const buffer of transfers) validateStructuredTransfer(buffer);
 for (const buffer of transfers) structuredCloneApply(structuredCloneBufferTransfer, buffer, [0]);
 return cloned;
}
if (EarlyUtilTypes && typeof EarlyUtilTypes.isProxy !== "function") {
	EarlyUtilTypes.isProxy = () => false;
} else if (EarlyUtilTypes) {
	try {
		EarlyUtilTypes.isProxy({});
	} catch (_error) {
		EarlyUtilTypes.isProxy = () => false;
	}
}

("use strict");

export {
	base64ToBase64Url,
	bufferStdlibModuleNs,
	cloneStructuredArrayBuffer,
	cloneStructuredValue,
	constantsStdlibModuleNs,
	createStructuredCloneDataCloneError,
	EarlyBufferGlobal,
	EarlyUtilTypes,
	eventsStdlibModuleNs,
	installBase64UrlBufferEncoding,
	loadWebSocketModule,
	normalizeBase64UrlEncoding,
	pathStdlibModuleNs,
	punycodeStdlibModuleNs,
	querystringStdlibModuleNs,
	StructuredCloneTypedArrayCtors,
	sandboxStructuredClone,
	streamStdlibModuleNs,
	stringDecoderStdlibModuleNs,
	undiciAgentModule,
	undiciApiModule,
	undiciClientModule,
	undiciFetchModule,
	undiciFormDataModule,
	undiciGlobalModule,
	undiciHeadersModule,
	undiciRequestModule,
	undiciResponseModule,
	undiciWebidlModule,
	urlStdlibModuleNs,
	utilStdlibModuleNs,
	WebReadableStream,
	WebTextDecoderStream,
	WebTextEncoderStream,
	WebTransformStream,
	WebWritableStream,
};
