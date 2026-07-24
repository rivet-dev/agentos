import { WebReadableStream, WebTextDecoderStream, WebTextEncoderStream, WebTransformStream, WebWritableStream, sandboxStructuredClone, undiciWebidlModule } from "../prelude.js";
import { CustomEvent, Event, EventTarget } from "./dom-events.js";
import { AbortController, AbortSignal } from "./abort.js";
import { FallbackReadableStream, FallbackWritableStream } from "./web-streams-fallback.js";
import { TextDecoder, TextEncoder2 } from "./text-encoding.js";

function defineGlobal(name, value) {
  globalThis[name] = value;
}
if (typeof globalThis.global === "undefined") {
  defineGlobal("global", globalThis);
}
if (typeof globalThis.RegExp === "function" && !("__agentOsRgiEmojiCompat" in globalThis.RegExp)) {
  const NativeRegExp = globalThis.RegExp;
  const rgiEmojiPattern = "^\\p{RGI_Emoji}$";
  const rgiEmojiBaseClass = "[\\u{00A9}\\u{00AE}\\u{203C}\\u{2049}\\u{2122}\\u{2139}\\u{2194}-\\u{21AA}\\u{231A}-\\u{23FF}\\u{24C2}\\u{25AA}-\\u{27BF}\\u{2934}-\\u{2935}\\u{2B05}-\\u{2B55}\\u{3030}\\u{303D}\\u{3297}\\u{3299}\\u{1F000}-\\u{1FAFF}]";
  const rgiEmojiKeycap = "[#*0-9]\\uFE0F?\\u20E3";
  const rgiEmojiFallbackSource = "^(?:" + rgiEmojiKeycap + "|\\p{Regional_Indicator}{2}|" + rgiEmojiBaseClass + "(?:\\uFE0F|\\u200D(?:" + rgiEmojiKeycap + "|" + rgiEmojiBaseClass + ")|[\\u{1F3FB}-\\u{1F3FF}])*)$";
  try {
    new NativeRegExp(rgiEmojiPattern, "v");
  } catch (error) {
    if (String(error?.message ?? error).includes("RGI_Emoji")) {
      const CompatRegExp = function CompatRegExp2(pattern, flags) {
        const normalizedPattern = pattern instanceof NativeRegExp && flags === void 0 ? pattern.source : String(pattern);
        const normalizedFlags = flags === void 0 ? pattern instanceof NativeRegExp ? pattern.flags : "" : String(flags);
        try {
          return new NativeRegExp(pattern, flags);
        } catch (innerError) {
          if (normalizedPattern === rgiEmojiPattern && normalizedFlags === "v") {
            return new NativeRegExp(rgiEmojiFallbackSource, "u");
          }
          throw innerError;
        }
      };
      Object.setPrototypeOf(CompatRegExp, NativeRegExp);
      CompatRegExp.prototype = NativeRegExp.prototype;
      Object.defineProperty(CompatRegExp.prototype, "constructor", {
        value: CompatRegExp,
        writable: true,
        configurable: true
      });
      defineGlobal(
        "RegExp",
        Object.assign(CompatRegExp, { __agentOsRgiEmojiCompat: true })
      );
    }
  }
}

defineGlobal("TextEncoder", TextEncoder2);
defineGlobal("TextDecoder", TextDecoder);
defineGlobal("Event", Event);
defineGlobal("CustomEvent", CustomEvent);
defineGlobal("EventTarget", EventTarget);
defineGlobal("AbortSignal", AbortSignal);
defineGlobal("AbortController", AbortController);
defineGlobal("structuredClone", sandboxStructuredClone);
if (
  globalThis.WebAssembly &&
  typeof globalThis.WebAssembly.instantiateStreaming !== "function"
) {
  globalThis.WebAssembly.instantiateStreaming = async function instantiateStreaming(source, imports) {
    const response = await source;
    if (response == null || typeof response.arrayBuffer !== "function") {
      throw new TypeError("WebAssembly.instantiateStreaming requires a Response or promise for one");
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    return globalThis.WebAssembly.instantiate(bytes, imports);
  };
}
defineGlobal("ReadableStream", typeof WebReadableStream === "function" ? WebReadableStream : FallbackReadableStream);
defineGlobal("WritableStream", typeof WebWritableStream === "function" ? WebWritableStream : FallbackWritableStream);
if (typeof WebTransformStream === "function") {
  defineGlobal("TransformStream", WebTransformStream);
}
if (typeof WebTextEncoderStream === "function") {
  defineGlobal("TextEncoderStream", WebTextEncoderStream);
}
if (typeof WebTextDecoderStream === "function") {
  defineGlobal("TextDecoderStream", WebTextDecoderStream);
}
const undiciWebidl = undiciWebidlModule?.webidl ?? undiciWebidlModule;
if (undiciWebidl?.is) {
  undiciWebidl.is.ReadableStream = (value) =>
    value != null &&
    (value instanceof globalThis.ReadableStream ||
      typeof value.getReader === "function");
  undiciWebidl.is.AbortSignal = (value) =>
    value != null &&
    (value instanceof globalThis.AbortSignal ||
      (typeof value.aborted === "boolean" &&
        typeof value.addEventListener === "function"));
}
if (undiciWebidl?.converters?.AbortSignal) {
  undiciWebidl.converters.AbortSignal = (value, ...args) => {
    if (
      value != null &&
      (value instanceof globalThis.AbortSignal ||
        (typeof value.aborted === "boolean" &&
          typeof value.addEventListener === "function"))
    ) {
      return value;
    }
    return undiciWebidl.interfaceConverter(
      undiciWebidl.is.AbortSignal,
      "AbortSignal"
    )(value, ...args);
  };
}

export { defineGlobal, TextEncoder2, TextDecoder, Event, CustomEvent, EventTarget, AbortSignal, AbortController, FallbackWritableStream, FallbackReadableStream, undiciWebidl };
export { withCode, createEncodingNotSupportedError, createEncodingInvalidDataError, createInvalidDecodeInputError, trimAsciiWhitespace, normalizeEncodingLabel, toUint8Array, encodeUtf8ScalarValue, encodeUtf8, appendCodePoint, isContinuationByte, decodeUtf8, decodeUtf16, PatchedTextEncoder, PatchedTextDecoder } from "./text-encoding.js";
export { normalizeAddEventListenerOptions, normalizeRemoveEventListenerOptions, isAbortSignalLike, PatchedEvent, PatchedCustomEvent, PatchedEventTarget } from "./dom-events.js";
export { ensureNamedConstructor, createAbortSignalReason, createAbortedSignal, normalizeAbortSignalTimeout } from "./abort.js";
