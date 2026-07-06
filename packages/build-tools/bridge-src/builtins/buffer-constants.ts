// Dependency-free-ish leaf for the buffer size caps. It imports ONLY from the
// vendor leaves (esbuild-runtime + the vendored buffer), never from the cyclic
// builtins, so `process.ts`/`module-loader.ts` can import these constants without
// re-creating the import cycle that caused the earlier TDZ bug. The caps are read
// dynamically from the vendored buffer module (exactly as the pre-split source did)
// so a future buffer upgrade that exposes Buffer.kMaxLength/kStringMaxLength is
// honored rather than silently overridden by a literal.
import { __toESM } from "../vendor/esbuild-runtime.js";
import { require_buffer } from "../vendor/buffer.js";

var import_buffer2 = __toESM(require_buffer(), 1);
var BUFFER_MAX_LENGTH = typeof import_buffer2.Buffer.kMaxLength === "number" ? import_buffer2.Buffer.kMaxLength : 2147483647;
var BUFFER_MAX_STRING_LENGTH = typeof import_buffer2.Buffer.kStringMaxLength === "number" ? import_buffer2.Buffer.kStringMaxLength : 536870888;
var BUFFER_CONSTANTS = Object.freeze({
  MAX_LENGTH: BUFFER_MAX_LENGTH,
  MAX_STRING_LENGTH: BUFFER_MAX_STRING_LENGTH
});

export { BUFFER_MAX_LENGTH, BUFFER_MAX_STRING_LENGTH, BUFFER_CONSTANTS };
