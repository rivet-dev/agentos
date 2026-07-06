import { BUFFER_MAX_LENGTH, BUFFER_MAX_STRING_LENGTH } from "./buffer-constants.js";
import { require_buffer } from "../vendor/buffer.js";
import { __toESM } from "../vendor/esbuild-runtime.js";

var import_buffer2 = __toESM(require_buffer(), 1);

var bufferPolyfillMutable = import_buffer2.Buffer;

if (typeof bufferPolyfillMutable.kMaxLength !== "number") {
  bufferPolyfillMutable.kMaxLength = BUFFER_MAX_LENGTH;
}

if (typeof bufferPolyfillMutable.kStringMaxLength !== "number") {
  bufferPolyfillMutable.kStringMaxLength = BUFFER_MAX_STRING_LENGTH;
}

if (typeof bufferPolyfillMutable.constants !== "object" || bufferPolyfillMutable.constants === null) {
  bufferPolyfillMutable.constants = {
    MAX_LENGTH: BUFFER_MAX_LENGTH,
    MAX_STRING_LENGTH: BUFFER_MAX_STRING_LENGTH
  };
}

var bufferProto = import_buffer2.Buffer.prototype;

if (typeof bufferProto.utf8Slice !== "function") {
  const encodings = ["utf8", "latin1", "ascii", "hex", "base64", "ucs2", "utf16le"];
  for (const enc of encodings) {
    if (typeof bufferProto[enc + "Slice"] !== "function") {
      bufferProto[enc + "Slice"] = function(start, end) {
        return this.toString(enc, start, end);
      };
    }
    if (typeof bufferProto[enc + "Write"] !== "function") {
      bufferProto[enc + "Write"] = function(string, offset, length) {
        return this.write(string, offset ?? 0, length ?? this.length - (offset ?? 0), enc);
      };
    }
  }
}

var bufferCtorMutable = import_buffer2.Buffer;

if (typeof bufferCtorMutable.allocUnsafe === "function" && !bufferCtorMutable.allocUnsafe._secureExecPatched) {
  const originalAllocUnsafe = bufferCtorMutable.allocUnsafe;
  bufferCtorMutable.allocUnsafe = function patchedAllocUnsafe(size) {
    try {
      return originalAllocUnsafe.call(this, size);
    } catch (error) {
      if (error instanceof RangeError && typeof size === "number" && size > BUFFER_MAX_LENGTH) {
        throw new Error("Array buffer allocation failed");
      }
      throw error;
    }
  };
  bufferCtorMutable.allocUnsafe._secureExecPatched = true;
}

var Buffer3 = import_buffer2.Buffer;
export { Buffer3, bufferCtorMutable, bufferPolyfillMutable, bufferProto, import_buffer2 };
