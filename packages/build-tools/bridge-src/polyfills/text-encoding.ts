function withCode(error, code) {
  error.code = code;
  return error;
}
function createEncodingNotSupportedError(label) {
  return withCode(
    new RangeError(`The "${label}" encoding is not supported`),
    "ERR_ENCODING_NOT_SUPPORTED"
  );
}
function createEncodingInvalidDataError(encoding) {
  return withCode(
    new TypeError(`The encoded data was not valid for encoding ${encoding}`),
    "ERR_ENCODING_INVALID_ENCODED_DATA"
  );
}
function createInvalidDecodeInputError() {
  return withCode(
    new TypeError(
      'The "input" argument must be an instance of ArrayBuffer, SharedArrayBuffer, or ArrayBufferView.'
    ),
    "ERR_INVALID_ARG_TYPE"
  );
}
function trimAsciiWhitespace(value) {
  return value.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "");
}
function normalizeEncodingLabel(label) {
  const normalized = trimAsciiWhitespace(
    label === void 0 ? "utf-8" : String(label)
  ).toLowerCase();
  switch (normalized) {
    case "utf-8":
    case "utf8":
    case "unicode-1-1-utf-8":
    case "unicode11utf8":
    case "unicode20utf8":
    case "x-unicode20utf8":
      return "utf-8";
    case "utf-16":
    case "utf-16le":
    case "ucs-2":
    case "ucs2":
    case "csunicode":
    case "iso-10646-ucs-2":
    case "unicode":
    case "unicodefeff":
      return "utf-16le";
    case "utf-16be":
    case "unicodefffe":
      return "utf-16be";
    default:
      throw createEncodingNotSupportedError(normalized);
  }
}
function toUint8Array(input) {
  if (input === void 0) {
    return new Uint8Array(0);
  }
  if (ArrayBuffer.isView(input)) {
    return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  }
  if (input instanceof ArrayBuffer) {
    return new Uint8Array(input);
  }
  if (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer) {
    return new Uint8Array(input);
  }
  throw createInvalidDecodeInputError();
}
function encodeUtf8ScalarValue(codePoint, bytes) {
  if (codePoint <= 127) {
    bytes.push(codePoint);
    return;
  }
  if (codePoint <= 2047) {
    bytes.push(192 | codePoint >> 6, 128 | codePoint & 63);
    return;
  }
  if (codePoint <= 65535) {
    bytes.push(
      224 | codePoint >> 12,
      128 | codePoint >> 6 & 63,
      128 | codePoint & 63
    );
    return;
  }
  bytes.push(
    240 | codePoint >> 18,
    128 | codePoint >> 12 & 63,
    128 | codePoint >> 6 & 63,
    128 | codePoint & 63
  );
}
function encodeUtf8(input = "") {
  const value = String(input);
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (codeUnit >= 55296 && codeUnit <= 56319) {
      const nextIndex = index + 1;
      if (nextIndex < value.length) {
        const nextCodeUnit = value.charCodeAt(nextIndex);
        if (nextCodeUnit >= 56320 && nextCodeUnit <= 57343) {
          const codePoint = 65536 + (codeUnit - 55296 << 10) + (nextCodeUnit - 56320);
          encodeUtf8ScalarValue(codePoint, bytes);
          index = nextIndex;
          continue;
        }
      }
      encodeUtf8ScalarValue(65533, bytes);
      continue;
    }
    if (codeUnit >= 56320 && codeUnit <= 57343) {
      encodeUtf8ScalarValue(65533, bytes);
      continue;
    }
    encodeUtf8ScalarValue(codeUnit, bytes);
  }
  return new Uint8Array(bytes);
}
function appendCodePoint(output, codePoint) {
  if (codePoint <= 65535) {
    output.push(String.fromCharCode(codePoint));
    return;
  }
  const adjusted = codePoint - 65536;
  output.push(
    String.fromCharCode(55296 + (adjusted >> 10)),
    String.fromCharCode(56320 + (adjusted & 1023))
  );
}
function isContinuationByte(value) {
  return value >= 128 && value <= 191;
}
function decodeUtf8(bytes, fatal, stream, encoding) {
  const output = [];
  for (let index = 0; index < bytes.length; ) {
    const first = bytes[index];
    if (first <= 127) {
      output.push(String.fromCharCode(first));
      index += 1;
      continue;
    }
    let needed = 0;
    let codePoint = 0;
    if (first >= 194 && first <= 223) {
      needed = 1;
      codePoint = first & 31;
    } else if (first >= 224 && first <= 239) {
      needed = 2;
      codePoint = first & 15;
    } else if (first >= 240 && first <= 244) {
      needed = 3;
      codePoint = first & 7;
    } else {
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      index += 1;
      continue;
    }
    if (index + needed >= bytes.length) {
      if (stream) {
        return {
          text: output.join(""),
          pending: Array.from(bytes.slice(index))
        };
      }
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      break;
    }
    const second = bytes[index + 1];
    if (!isContinuationByte(second)) {
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      index += 1;
      continue;
    }
    if (first === 224 && second < 160 || first === 237 && second > 159 || first === 240 && second < 144 || first === 244 && second > 143) {
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      index += 1;
      continue;
    }
    codePoint = codePoint << 6 | second & 63;
    if (needed >= 2) {
      const third = bytes[index + 2];
      if (!isContinuationByte(third)) {
        if (fatal) {
          throw createEncodingInvalidDataError(encoding);
        }
        output.push("\uFFFD");
        index += 1;
        continue;
      }
      codePoint = codePoint << 6 | third & 63;
    }
    if (needed === 3) {
      const fourth = bytes[index + 3];
      if (!isContinuationByte(fourth)) {
        if (fatal) {
          throw createEncodingInvalidDataError(encoding);
        }
        output.push("\uFFFD");
        index += 1;
        continue;
      }
      codePoint = codePoint << 6 | fourth & 63;
    }
    if (codePoint >= 55296 && codePoint <= 57343) {
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      index += needed + 1;
      continue;
    }
    appendCodePoint(output, codePoint);
    index += needed + 1;
  }
  return { text: output.join(""), pending: [] };
}
function decodeUtf16(bytes, encoding, fatal, stream, bomSeen) {
  const output = [];
  let endian = encoding === "utf-16be" ? "be" : "le";
  if (!bomSeen && encoding === "utf-16le" && bytes.length >= 2) {
    if (bytes[0] === 254 && bytes[1] === 255) {
      endian = "be";
    }
  }
  for (let index = 0; index < bytes.length; ) {
    if (index + 1 >= bytes.length) {
      if (stream) {
        return {
          text: output.join(""),
          pending: Array.from(bytes.slice(index))
        };
      }
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      break;
    }
    const first = bytes[index];
    const second = bytes[index + 1];
    const codeUnit = endian === "le" ? first | second << 8 : first << 8 | second;
    index += 2;
    if (codeUnit >= 55296 && codeUnit <= 56319) {
      if (index + 1 >= bytes.length) {
        if (stream) {
          return {
            text: output.join(""),
            pending: Array.from(bytes.slice(index - 2))
          };
        }
        if (fatal) {
          throw createEncodingInvalidDataError(encoding);
        }
        output.push("\uFFFD");
        continue;
      }
      const nextFirst = bytes[index];
      const nextSecond = bytes[index + 1];
      const nextCodeUnit = endian === "le" ? nextFirst | nextSecond << 8 : nextFirst << 8 | nextSecond;
      if (nextCodeUnit >= 56320 && nextCodeUnit <= 57343) {
        const codePoint = 65536 + (codeUnit - 55296 << 10) + (nextCodeUnit - 56320);
        appendCodePoint(output, codePoint);
        index += 2;
        continue;
      }
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      continue;
    }
    if (codeUnit >= 56320 && codeUnit <= 57343) {
      if (fatal) {
        throw createEncodingInvalidDataError(encoding);
      }
      output.push("\uFFFD");
      continue;
    }
    output.push(String.fromCharCode(codeUnit));
  }
  return { text: output.join(""), pending: [] };
}
var PatchedTextEncoder = class {
  encode(input = "") {
    return encodeUtf8(input);
  }
  encodeInto(input, destination) {
    const value = String(input);
    let read = 0;
    let written = 0;
    for (let index = 0; index < value.length; index += 1) {
      const codeUnit = value.charCodeAt(index);
      let chunk = "";
      if (codeUnit >= 55296 && codeUnit <= 56319 && index + 1 < value.length) {
        const nextCodeUnit = value.charCodeAt(index + 1);
        if (nextCodeUnit >= 56320 && nextCodeUnit <= 57343) {
          chunk = value.slice(index, index + 2);
        }
      }
      if (chunk === "") {
        chunk = value[index] ?? "";
      }
      const encoded = encodeUtf8(chunk);
      if (written + encoded.length > destination.length) {
        break;
      }
      destination.set(encoded, written);
      written += encoded.length;
      read += chunk.length;
      if (chunk.length === 2) {
        index += 1;
      }
    }
    return { read, written };
  }
  get encoding() {
    return "utf-8";
  }
  get [Symbol.toStringTag]() {
    return "TextEncoder";
  }
};
var PatchedTextDecoder = class {
  normalizedEncoding;
  fatalFlag;
  ignoreBOMFlag;
  pendingBytes = [];
  bomSeen = false;
  constructor(label, options) {
    const normalizedOptions = options == null ? {} : Object(options);
    this.normalizedEncoding = normalizeEncodingLabel(label);
    this.fatalFlag = Boolean(
      normalizedOptions.fatal
    );
    this.ignoreBOMFlag = Boolean(
      normalizedOptions.ignoreBOM
    );
  }
  get encoding() {
    return this.normalizedEncoding;
  }
  get fatal() {
    return this.fatalFlag;
  }
  get ignoreBOM() {
    return this.ignoreBOMFlag;
  }
  get [Symbol.toStringTag]() {
    return "TextDecoder";
  }
  decode(input, options) {
    const normalizedOptions = options == null ? {} : Object(options);
    const stream = Boolean(
      normalizedOptions.stream
    );
    const incoming = toUint8Array(input);
    const merged = new Uint8Array(this.pendingBytes.length + incoming.length);
    merged.set(this.pendingBytes, 0);
    merged.set(incoming, this.pendingBytes.length);
    const decoded = this.normalizedEncoding === "utf-8" ? decodeUtf8(
      merged,
      this.fatalFlag,
      stream,
      this.normalizedEncoding
    ) : decodeUtf16(
      merged,
      this.normalizedEncoding,
      this.fatalFlag,
      stream,
      this.bomSeen
    );
    this.pendingBytes = decoded.pending;
    let text = decoded.text;
    if (!this.bomSeen && text.length > 0) {
      if (!this.ignoreBOMFlag && text.charCodeAt(0) === 65279) {
        text = text.slice(1);
      }
      this.bomSeen = true;
    }
    if (!stream && this.pendingBytes.length > 0) {
      const pending = this.pendingBytes;
      this.pendingBytes = [];
      if (this.fatalFlag) {
        throw createEncodingInvalidDataError(this.normalizedEncoding);
      }
      return text + "\uFFFD".repeat(Math.ceil(pending.length / 2));
    }
    return text;
  }
};
var TextEncoder2 = PatchedTextEncoder;
var TextDecoder = PatchedTextDecoder;

export { withCode, createEncodingNotSupportedError, createEncodingInvalidDataError, createInvalidDecodeInputError, trimAsciiWhitespace, normalizeEncodingLabel, toUint8Array, encodeUtf8ScalarValue, encodeUtf8, appendCodePoint, isContinuationByte, decodeUtf8, decodeUtf16, PatchedTextEncoder, PatchedTextDecoder, TextEncoder2, TextDecoder };
