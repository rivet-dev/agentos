// POC mini-realm: load Node.js's REAL lib/*.js builtins inside an agentOS V8
// session, with internalBinding() provided by JS shims instead of node's C++.
//
// Inputs (set by the Rust test before this script runs):
//   globalThis.__nodeSources   - { "<builtin id>": "<source>" } from node's lib/
//   globalThis.__nodeConstants - the `constants` binding value (harvested from
//                                a real linux node via internal/test/binding)
//
// Node compiles every regular builtin with the wrapper parameters
//   (exports, require, module, process, internalBinding, primordials)
// and per-context scripts with
//   (exports, primordials, privateSymbols, perIsolateSymbols)
// (src/builtin_info.h). We replicate exactly that with `new Function`.
(function () {
  'use strict';

  const sources = globalThis.__nodeSources;
  const constantsBinding = globalThis.__nodeConstants;
  if (!sources || !constantsBinding) {
    throw new Error('POC bootstrap: __nodeSources/__nodeConstants not injected');
  }

  function compileBuiltin(id, params) {
    const source = sources[id];
    if (source === undefined) {
      throw new Error(`POC: no source for builtin '${id}'`);
    }
    // eslint-disable-next-line no-new-func
    return new Function(...params, `${source}\n//# sourceURL=node:${id}`);
  }

  // Private/per-isolate symbols: auto-create a Symbol per name accessed.
  function makeLazySymbols(tag) {
    const cache = { __proto__: null };
    return new Proxy(Object.create(null), {
      get(_t, prop) {
        if (typeof prop !== 'string') return undefined;
        cache[prop] ??= Symbol(`${tag}:${prop}`);
        return cache[prop];
      },
    });
  }
  const privateSymbols = makeLazySymbols('private');
  const perIsolateSymbols = makeLazySymbols('perIsolate');

  // ---- primordials (node's real per-context script, unmodified) ----
  const primordials = { __proto__: null };
  compileBuiltin('internal/per_context/primordials', [
    'exports', 'primordials', 'privateSymbols', 'perIsolateSymbols',
  ])({}, primordials, privateSymbols, perIsolateSymbols);

  // ---- minimal process ----
  const process = {
    platform: 'linux',
    arch: 'x64',
    env: { __proto__: null },
    argv: ['node'],
    execArgv: [],
    pid: 1,
    ppid: 0,
    version: 'v26.0.0',
    versions: { node: '26.0.0', v8: '13.0', modules: '140', uv: '1.52.1' },
    emitWarning() {},
    // Promise-based so we don't depend on embedder-provided queueMicrotask.
    nextTick(cb, ...args) { Promise.resolve().then(() => cb(...args)); },
    cwd() { return '/'; },
    umask() { return 0o22; },
    memoryUsage() { return { rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0 }; },
  };

  // ---- binding shims ----

  // Wrap a shim so that destructuring any not-yet-implemented property yields
  // a named function that throws on CALL: loads succeed, uses fail loudly.
  function withAutoStubs(bindingName, target) {
    return new Proxy(target, {
      get(t, prop, receiver) {
        if (prop in t || typeof prop !== 'string') {
          return Reflect.get(t, prop, receiver);
        }
        return function pocStub() {
          throw new Error(`POC: internalBinding('${bindingName}').${prop} is not implemented`);
        };
      },
    });
  }

  // A binding we have not written at all: fail loudly on ANY property access,
  // so load-time gaps name themselves.
  function missingBinding(name) {
    return new Proxy(Object.create(null), {
      get(_t, prop) {
        if (typeof prop !== 'string') return undefined;
        throw new Error(`POC: internalBinding('${name}') has no shim (accessed .${String(prop)})`);
      },
    });
  }

  // -- util --
  function getOwnNonIndexProperties(obj, filter) {
    const ONLY_ENUMERABLE = 2;
    const names = Object.getOwnPropertyNames(obj);
    const out = [];
    for (const name of names) {
      if (String(Number(name)) === name && Number(name) >= 0) continue; // index
      if (filter & ONLY_ENUMERABLE) {
        const desc = Object.getOwnPropertyDescriptor(obj, name);
        if (!desc || !desc.enumerable) continue;
      }
      out.push(name);
    }
    if (!(filter & 16 /* SKIP_SYMBOLS */)) {
      for (const sym of Object.getOwnPropertySymbols(obj)) {
        if (filter & 2) {
          const desc = Object.getOwnPropertyDescriptor(obj, sym);
          if (!desc || !desc.enumerable) continue;
        }
        out.push(sym);
      }
    }
    return out;
  }

  const hiddenValues = new WeakMap();
  const utilBinding = withAutoStubs('util', {
    constants: {
      ALL_PROPERTIES: 0, ONLY_WRITABLE: 1, ONLY_ENUMERABLE: 2,
      ONLY_CONFIGURABLE: 4, SKIP_STRINGS: 8, SKIP_SYMBOLS: 16,
    },
    privateSymbols,
    getOwnNonIndexProperties,
    isInsideNodeModules: () => false,
    guessHandleType: () => 'PIPE',
    sleep() {},
    constructSharedArrayBuffer: (len) => new SharedArrayBuffer(len),
    getConstructorName: (obj) => (obj?.constructor?.name ?? ''),
    getPromiseDetails: () => [0],
    getProxyDetails: () => undefined,
    previewEntries: () => [[], false],
    getExternalValue: () => 0,
    setHiddenValue(obj, sym, val) {
      let map = hiddenValues.get(obj);
      if (!map) { map = new Map(); hiddenValues.set(obj, map); }
      map.set(sym, val);
      return true;
    },
    getHiddenValue(obj, sym) { return hiddenValues.get(obj)?.get(sym); },
    defineLazyProperties(target, id, keys, enumerable = true) {
      for (const key of keys) {
        let set, value, isSet = false;
        Object.defineProperty(target, key, {
          get() {
            if (isSet) return value;
            value = requireBuiltin(id)[key];
            isSet = true;
            return value;
          },
          set(v) { value = v; isSet = true; },
          configurable: true,
          enumerable,
        });
      }
      return target;
    },
    shouldAbortOnUncaughtToggle: [true],
    WeakReference: class WeakReference {
      #ref;
      constructor(value) { this.#ref = new WeakRef(value); }
      get() { return this.#ref.deref(); }
      incRef() {}
      decRef() {}
    },
  });

  // -- types --
  const brand = (v) => Object.prototype.toString.call(v);
  const typesBinding = withAutoStubs('types', {
    isAnyArrayBuffer: (v) => brand(v) === '[object ArrayBuffer]' || brand(v) === '[object SharedArrayBuffer]',
    isArrayBuffer: (v) => brand(v) === '[object ArrayBuffer]',
    isSharedArrayBuffer: (v) => brand(v) === '[object SharedArrayBuffer]',
    isDataView: (v) => brand(v) === '[object DataView]',
    isDate: (v) => brand(v) === '[object Date]',
    isMap: (v) => brand(v) === '[object Map]',
    isSet: (v) => brand(v) === '[object Set]',
    isWeakMap: (v) => brand(v) === '[object WeakMap]',
    isWeakSet: (v) => brand(v) === '[object WeakSet]',
    isMapIterator: (v) => brand(v) === '[object Map Iterator]',
    isSetIterator: (v) => brand(v) === '[object Set Iterator]',
    isRegExp: (v) => brand(v) === '[object RegExp]',
    isNativeError: (v) => v instanceof Error,
    isPromise: (v) => brand(v) === '[object Promise]',
    isProxy: () => false,
    isExternal: () => false,
    isModuleNamespaceObject: (v) => brand(v) === '[object Module]',
    isArgumentsObject: (v) => brand(v) === '[object Arguments]',
    isBoxedPrimitive: (v) =>
      ['[object Number]', '[object String]', '[object Boolean]', '[object BigInt]', '[object Symbol]']
        .includes(brand(v)) && typeof v === 'object',
    isNumberObject: (v) => typeof v === 'object' && brand(v) === '[object Number]',
    isStringObject: (v) => typeof v === 'object' && brand(v) === '[object String]',
    isBooleanObject: (v) => typeof v === 'object' && brand(v) === '[object Boolean]',
    isBigIntObject: (v) => typeof v === 'object' && brand(v) === '[object BigInt]',
    isSymbolObject: (v) => typeof v === 'object' && brand(v) === '[object Symbol]',
    isGeneratorFunction: (v) => typeof v === 'function' && brand(v) === '[object GeneratorFunction]',
    isAsyncFunction: (v) => typeof v === 'function' && brand(v) === '[object AsyncFunction]',
    isGeneratorObject: (v) => brand(v) === '[object Generator]',
    isCryptoKey: () => false,
    isKeyObject: () => false,
  });

  // -- string_decoder (constants from src/string_decoder.h; enum order from
  //    src/node.h `enum encoding`) --
  const encodings = ['ascii', 'utf8', 'base64', 'utf16le', 'latin1', 'hex', 'buffer', 'base64url'];
  const stringDecoderBinding = withAutoStubs('string_decoder', {
    encodings,
    kIncompleteCharactersStart: 0,
    kIncompleteCharactersEnd: 4,
    kMissingBytes: 4,
    kBufferedBytes: 5,
    kEncodingField: 6,
    kNumFields: 7,
    kSize: 7,
  });

  // -- options --
  const cliOptions = {
    __proto__: null,
    '--stack-trace-limit': 10,
    '--no-deprecation': false,
    '--throw-deprecation': false,
    '--pending-deprecation': false,
    '--trace-deprecation': false,
    '--preserve-symlinks': false,
    '--verify-base-objects': false,
  };
  const optionsBinding = withAutoStubs('options', {
    getCLIOptionsValues: () => cliOptions,
    getCLIOptionsInfo: () => ({ __proto__: null, options: new Map(), aliases: new Map() }),
    getOptionsAsFlags: () => [],
    getEmbedderOptions: () => ({
      __proto__: null,
      shouldNotRegisterESMLoader: true,
      noGlobalSearchPaths: true,
      noBrowserGlobals: true,
      hasEmbedderPreload: false,
    }),
    getEnvOptionsInputType: () => ({ __proto__: null }),
    getNamespaceOptionsInputType: () => ({ __proto__: null }),
  });

  // -- config --
  const configBinding = {
    hasIntl: false,
    hasSmallICU: false,
    hasOpenSSL: false,
    hasQuic: false,
    hasInspector: false,
    hasSQLite: false,
    hasNodeOptions: false,
    noBrowserGlobals: true,
    bits: 64,
  };

  // -- messaging (only DOMException is reached from util paths) --
  class POCDOMException extends Error {
    constructor(message, name) {
      super(message);
      this.name = typeof name === 'object' ? name?.name ?? 'Error' : (name ?? 'Error');
    }
  }
  const messagingBinding = withAutoStubs('messaging', {
    DOMException: POCDOMException,
    // Called at load by internal/worker/js_transferable (pulled in by internal/url).
    setDeserializerCreateObjectFunction() {},
  });

  // -- buffer: pure-JS codecs standing in for node_buffer.cc / simdutf --
  const B64_STD = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const B64_URL = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  const B64_REV = new Int8Array(256).fill(-1);
  for (let i = 0; i < 64; i++) B64_REV[B64_STD.charCodeAt(i)] = i;
  B64_REV['-'.charCodeAt(0)] = 62;
  B64_REV['_'.charCodeAt(0)] = 63;

  function utf8ByteLength(str) {
    let bytes = 0;
    for (let i = 0; i < str.length; i++) {
      const c = str.charCodeAt(i);
      if (c < 0x80) bytes += 1;
      else if (c < 0x800) bytes += 2;
      else if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length &&
               str.charCodeAt(i + 1) >= 0xdc00 && str.charCodeAt(i + 1) < 0xe000) {
        bytes += 4; i++;
      } else bytes += 3;
    }
    return bytes;
  }

  // Write as many WHOLE characters as fit in `length` bytes; return bytes written.
  function utf8WriteInto(buf, string, offset, length) {
    let pos = offset;
    const end = offset + length;
    for (let i = 0; i < string.length; i++) {
      let cp = string.charCodeAt(i);
      if (cp >= 0xd800 && cp < 0xdc00 && i + 1 < string.length) {
        const lo = string.charCodeAt(i + 1);
        if (lo >= 0xdc00 && lo < 0xe000) { cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00); i++; }
        else cp = 0xfffd;
      } else if (cp >= 0xdc00 && cp < 0xe000) {
        cp = 0xfffd;
      }
      if (cp < 0x80) {
        if (pos + 1 > end) break;
        buf[pos++] = cp;
      } else if (cp < 0x800) {
        if (pos + 2 > end) break;
        buf[pos++] = 0xc0 | (cp >> 6);
        buf[pos++] = 0x80 | (cp & 0x3f);
      } else if (cp < 0x10000) {
        if (pos + 3 > end) break;
        buf[pos++] = 0xe0 | (cp >> 12);
        buf[pos++] = 0x80 | ((cp >> 6) & 0x3f);
        buf[pos++] = 0x80 | (cp & 0x3f);
      } else {
        if (pos + 4 > end) break;
        buf[pos++] = 0xf0 | (cp >> 18);
        buf[pos++] = 0x80 | ((cp >> 12) & 0x3f);
        buf[pos++] = 0x80 | ((cp >> 6) & 0x3f);
        buf[pos++] = 0x80 | (cp & 0x3f);
      }
    }
    return pos - offset;
  }

  // Lossy UTF-8 decode (U+FFFD for invalid), matching node's utf8Slice for
  // valid input.
  function utf8DecodeRange(buf, start, end) {
    const parts = [];
    let chunk = [];
    const flush = () => { if (chunk.length) { parts.push(String.fromCharCode(...chunk)); chunk = []; } };
    let i = start;
    while (i < end) {
      const b = buf[i];
      let cp, extra;
      if (b < 0x80) { cp = b; extra = 0; }
      else if (b >= 0xc2 && b < 0xe0) { cp = b & 0x1f; extra = 1; }
      else if (b >= 0xe0 && b < 0xf0) { cp = b & 0x0f; extra = 2; }
      else if (b >= 0xf0 && b < 0xf5) { cp = b & 0x07; extra = 3; }
      else { chunk.push(0xfffd); i++; continue; }
      let ok = true;
      let value = cp;
      for (let k = 1; k <= extra; k++) {
        if (i + k >= end) { ok = false; break; }
        const cb = buf[i + k];
        if ((cb & 0xc0) !== 0x80) { ok = false; break; }
        value = (value << 6) | (cb & 0x3f);
      }
      if (!ok ||
          (extra === 1 && value < 0x80) ||
          (extra === 2 && (value < 0x800 || (value >= 0xd800 && value < 0xe000))) ||
          (extra === 3 && (value < 0x10000 || value > 0x10ffff))) {
        chunk.push(0xfffd); i++; continue;
      }
      if (value >= 0x10000) {
        value -= 0x10000;
        chunk.push(0xd800 + (value >> 10), 0xdc00 + (value & 0x3ff));
      } else {
        chunk.push(value);
      }
      i += extra + 1;
      if (chunk.length > 4096) flush();
    }
    flush();
    return parts.join('');
  }

  function charSlice(buf, start, end, mask) {
    const parts = [];
    let chunk = [];
    for (let i = start; i < end; i++) {
      chunk.push(buf[i] & mask);
      if (chunk.length > 4096) { parts.push(String.fromCharCode(...chunk)); chunk = []; }
    }
    if (chunk.length) parts.push(String.fromCharCode(...chunk));
    return parts.join('');
  }

  function charWrite(buf, string, offset, length) {
    const n = Math.min(length, string.length, buf.length - offset);
    for (let i = 0; i < n; i++) buf[offset + i] = string.charCodeAt(i) & 0xff;
    return n;
  }

  function base64EncodeRange(buf, start, end, alphabet, pad) {
    let out = '';
    let i = start;
    for (; i + 2 < end; i += 3) {
      const n = (buf[i] << 16) | (buf[i + 1] << 8) | buf[i + 2];
      out += alphabet[(n >> 18) & 63] + alphabet[(n >> 12) & 63] + alphabet[(n >> 6) & 63] + alphabet[n & 63];
    }
    const rem = end - i;
    if (rem === 1) {
      const n = buf[i] << 16;
      out += alphabet[(n >> 18) & 63] + alphabet[(n >> 12) & 63] + (pad ? '==' : '');
    } else if (rem === 2) {
      const n = (buf[i] << 16) | (buf[i + 1] << 8);
      out += alphabet[(n >> 18) & 63] + alphabet[(n >> 12) & 63] + alphabet[(n >> 6) & 63] + (pad ? '=' : '');
    }
    return out;
  }

  // Forgiving base64 decode (both alphabets), stops at capacity; returns bytes
  // written — matches node's legacy-forgiving binding behavior for valid input.
  function base64WriteInto(buf, string, offset, length) {
    let bits = 0, acc = 0, pos = offset;
    const end = offset + length;
    for (let i = 0; i < string.length && pos < end; i++) {
      const v = B64_REV[string.charCodeAt(i)];
      if (v === -1) {
        if (string[i] === '=') break;
        continue; // skip whitespace/invalid, like node
      }
      acc = (acc << 6) | v;
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        buf[pos++] = (acc >> bits) & 0xff;
      }
    }
    return pos - offset;
  }

  function hexWriteInto(buf, string, offset, length) {
    const n = Math.min(length, string.length >>> 1, buf.length - offset);
    let written = 0;
    for (; written < n; written++) {
      const hi = parseInt(string[written * 2], 16);
      const lo = parseInt(string[written * 2 + 1], 16);
      if (Number.isNaN(hi) || Number.isNaN(lo)) break;
      buf[offset + written] = (hi << 4) | lo;
    }
    return written;
  }

  function hexSliceRange(buf, start, end) {
    let out = '';
    for (let i = start; i < end; i++) out += buf[i].toString(16).padStart(2, '0');
    return out;
  }

  function ucs2SliceRange(buf, start, end) {
    const parts = [];
    let chunk = [];
    for (let i = start; i + 1 < end; i += 2) {
      chunk.push(buf[i] | (buf[i + 1] << 8));
      if (chunk.length > 4096) { parts.push(String.fromCharCode(...chunk)); chunk = []; }
    }
    if (chunk.length) parts.push(String.fromCharCode(...chunk));
    return parts.join('');
  }

  function ucs2WriteInto(buf, string, offset, length) {
    const maxChars = Math.min(string.length, Math.floor(length / 2), Math.floor((buf.length - offset) / 2));
    for (let i = 0; i < maxChars; i++) {
      const c = string.charCodeAt(i);
      buf[offset + i * 2] = c & 0xff;
      buf[offset + i * 2 + 1] = c >> 8;
    }
    return maxChars * 2;
  }

  function normRange(buf, start, end) {
    const len = buf.length;
    start = start === undefined ? 0 : Math.max(0, Math.min(start, len));
    end = end === undefined ? len : Math.max(start, Math.min(end, len));
    return [start, end];
  }

  function encodeForSearch(val, encodingNum) {
    // encodingNum indexes `encodings` above.
    const enc = encodings[encodingNum] ?? 'utf8';
    let tmp;
    switch (enc) {
      case 'utf8': {
        tmp = new Uint8Array(utf8ByteLength(val));
        utf8WriteInto(tmp, val, 0, tmp.length);
        return tmp;
      }
      case 'utf16le': {
        tmp = new Uint8Array(val.length * 2);
        ucs2WriteInto(tmp, val, 0, tmp.length);
        return tmp;
      }
      case 'latin1':
      case 'ascii': {
        tmp = new Uint8Array(val.length);
        charWrite(tmp, val, 0, tmp.length);
        return tmp;
      }
      case 'hex': {
        tmp = new Uint8Array(val.length >>> 1);
        return tmp.subarray(0, hexWriteInto(tmp, val, 0, tmp.length));
      }
      case 'base64':
      case 'base64url': {
        tmp = new Uint8Array(Math.ceil(val.length * 3 / 4));
        return tmp.subarray(0, base64WriteInto(tmp, val, 0, tmp.length));
      }
      default:
        throw new Error(`POC: search encoding ${enc} unsupported`);
    }
  }

  function searchBytes(haystack, needle, byteOffset, forward) {
    if (needle.length === 0) return byteOffset > haystack.length ? haystack.length : byteOffset;
    if (forward) {
      outer:
      for (let i = Math.max(0, byteOffset); i + needle.length <= haystack.length; i++) {
        for (let j = 0; j < needle.length; j++) {
          if (haystack[i + j] !== needle[j]) continue outer;
        }
        return i;
      }
    } else {
      outer2:
      for (let i = Math.min(byteOffset, haystack.length - needle.length); i >= 0; i--) {
        for (let j = 0; j < needle.length; j++) {
          if (haystack[i + j] !== needle[j]) continue outer2;
        }
        return i;
      }
    }
    return -1;
  }

  const bufferBinding = withAutoStubs('buffer', {
    kMaxLength: 4294967296,
    kStringMaxLength: 536870888,
    byteLengthUtf8: (string) => utf8ByteLength(string),
    copy(source, target, targetStart, sourceStart, nb) {
      target.set(source.subarray(sourceStart, sourceStart + nb), targetStart);
      return nb;
    },
    compare(a, b) {
      const len = Math.min(a.length, b.length);
      for (let i = 0; i < len; i++) {
        if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
      }
      return a.length === b.length ? 0 : (a.length < b.length ? -1 : 1);
    },
    compareOffset(source, target, targetStart, sourceStart, targetEnd, sourceEnd) {
      const a = source.subarray(sourceStart, sourceEnd);
      const b = target.subarray(targetStart, targetEnd);
      return bufferBinding.compare(a, b);
    },
    fill(buf, value, offset, end, encoding) {
      let bytes;
      if (typeof value === 'string') {
        bytes = encodeForSearch(value, typeof encoding === 'number' ? encoding : 1);
        if (value.length > 0 && bytes.length === 0) return -1;
      } else {
        bytes = value; // Uint8Array
      }
      if (bytes.length === 0) return 0;
      for (let i = offset; i < end; i++) buf[i] = bytes[(i - offset) % bytes.length];
      return 0;
    },
    isAscii(input) {
      for (let i = 0; i < input.length; i++) if (input[i] > 0x7f) return false;
      return true;
    },
    isUtf8(input) {
      let i = 0;
      while (i < input.length) {
        const b = input[i];
        let extra;
        if (b < 0x80) { i++; continue; }
        else if (b >= 0xc2 && b < 0xe0) extra = 1;
        else if (b >= 0xe0 && b < 0xf0) extra = 2;
        else if (b >= 0xf0 && b < 0xf5) extra = 3;
        else return false;
        if (i + extra >= input.length) return false;
        let value = extra === 1 ? b & 0x1f : extra === 2 ? b & 0x0f : b & 0x07;
        for (let k = 1; k <= extra; k++) {
          if ((input[i + k] & 0xc0) !== 0x80) return false;
          value = (value << 6) | (input[i + k] & 0x3f);
        }
        if ((extra === 1 && value < 0x80) ||
            (extra === 2 && (value < 0x800 || (value >= 0xd800 && value < 0xe000))) ||
            (extra === 3 && (value < 0x10000 || value > 0x10ffff))) return false;
        i += extra + 1;
      }
      return true;
    },
    indexOfBuffer: (buf, val, byteOffset, _encoding, dir) => searchBytes(buf, val, byteOffset, dir),
    indexOfNumber(buf, val, byteOffset, dir) {
      return searchBytes(buf, Uint8Array.of(val & 0xff), byteOffset, dir);
    },
    indexOfString: (buf, val, byteOffset, encoding, dir) =>
      searchBytes(buf, encodeForSearch(val, encoding), byteOffset, dir),
    swap16(buf) {
      for (let i = 0; i + 1 < buf.length; i += 2) {
        const t = buf[i]; buf[i] = buf[i + 1]; buf[i + 1] = t;
      }
      return buf;
    },
    swap32(buf) {
      for (let i = 0; i + 3 < buf.length; i += 4) {
        let t = buf[i]; buf[i] = buf[i + 3]; buf[i + 3] = t;
        t = buf[i + 1]; buf[i + 1] = buf[i + 2]; buf[i + 2] = t;
      }
      return buf;
    },
    swap64(buf) {
      for (let i = 0; i + 7 < buf.length; i += 8) {
        for (let j = 0; j < 4; j++) {
          const t = buf[i + j]; buf[i + j] = buf[i + 7 - j]; buf[i + 7 - j] = t;
        }
      }
      return buf;
    },
    atob(input) {
      // Returns decoded latin1 string, or negative error codes like node.
      let clean = '';
      for (const ch of input) {
        if (' \t\n\f\r'.includes(ch)) continue;
        clean += ch;
      }
      if (clean.length % 4 === 1) return -1;
      let padStripped = clean;
      if (clean.endsWith('==')) padStripped = clean.slice(0, -2);
      else if (clean.endsWith('=')) padStripped = clean.slice(0, -1);
      for (let i = 0; i < padStripped.length; i++) {
        const v = B64_REV[padStripped.charCodeAt(i)];
        if (v === -1 || padStripped[i] === '-' || padStripped[i] === '_') return -2;
      }
      const tmp = new Uint8Array(Math.ceil(padStripped.length * 3 / 4));
      const n = base64WriteInto(tmp, padStripped, 0, tmp.length);
      return charSlice(tmp, 0, n, 0xff);
    },
    btoa(input) {
      const bytes = new Uint8Array(input.length);
      for (let i = 0; i < input.length; i++) {
        const c = input.charCodeAt(i);
        if (c > 0xff) return -1;
        bytes[i] = c;
      }
      return base64EncodeRange(bytes, 0, bytes.length, B64_STD, true);
    },
    asciiSlice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return charSlice(buf, s, e, 0x7f); },
    latin1Slice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return charSlice(buf, s, e, 0xff); },
    utf8Slice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return utf8DecodeRange(buf, s, e); },
    hexSlice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return hexSliceRange(buf, s, e); },
    ucs2Slice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return ucs2SliceRange(buf, s, e); },
    base64Slice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return base64EncodeRange(buf, s, e, B64_STD, true); },
    base64urlSlice: (buf, start, end) => { const [s, e] = normRange(buf, start, end); return base64EncodeRange(buf, s, e, B64_URL, false); },
    asciiWriteStatic: (buf, string, offset, length) => charWrite(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    latin1WriteStatic: (buf, string, offset, length) => charWrite(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    utf8WriteStatic: (buf, string, offset, length) => utf8WriteInto(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    base64Write: (buf, string, offset, length) => base64WriteInto(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    base64urlWrite: (buf, string, offset, length) => base64WriteInto(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    hexWrite: (buf, string, offset, length) => hexWriteInto(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    ucs2Write: (buf, string, offset, length) => ucs2WriteInto(buf, string, offset ?? 0, length ?? buf.length - (offset ?? 0)),
    createUnsafeArrayBuffer: (size) => new ArrayBuffer(size),
    setDetachKey() {},
  });

  // -- stage 2.5: back validation/length codecs with upstream simdutf compiled
  //    to wasm32-wasip1 against the agentOS patched sysroot, instantiated with
  //    V8's own WebAssembly engine inside this isolate. The hand-written JS
  //    implementations above stay as the fallback (and as the differential
  //    reference for checks.js). Injected by the harness as base64 in
  //    __pocSimdutfWasmBase64; absent means JS-only mode. --
  (function initSimdutfWasm() {
    const b64 = globalThis.__pocSimdutfWasmBase64;
    delete globalThis.__pocSimdutfWasmBase64;
    if (typeof b64 !== 'string' || b64.length === 0) return;
    const wasmBytes = new Uint8Array(Math.ceil(b64.length * 3 / 4));
    const wasmLen = base64WriteInto(wasmBytes, b64, 0, wasmBytes.length);
    const module = new WebAssembly.Module(wasmBytes.subarray(0, wasmLen));
    // Only malloc's abort paths import WASI; stub them, but fail loud on exit.
    const wasiStub = () => 0;
    const instance = new WebAssembly.Instance(module, {
      wasi_snapshot_preview1: {
        fd_close: wasiStub,
        fd_seek: wasiStub,
        fd_write: wasiStub,
        proc_exit: (code) => { throw new Error(`simdutf wasm called proc_exit(${code})`); },
      },
    });
    const exps = instance.exports;
    exps._initialize();

    function withBytes(input, fn) {
      const ptr = exps.poc_alloc(input.length);
      if (ptr === 0) throw new Error('simdutf wasm poc_alloc failed');
      try {
        // View created after alloc: memory.buffer may detach on growth.
        new Uint8Array(exps.memory.buffer, ptr, input.length).set(input);
        return fn(ptr, input.length);
      } finally {
        exps.poc_free(ptr);
      }
    }

    const jsFallback = {
      isUtf8: bufferBinding.isUtf8,
      isAscii: bufferBinding.isAscii,
      byteLengthUtf8: bufferBinding.byteLengthUtf8,
    };
    bufferBinding.isUtf8 = (input) =>
      input.length === 0 ? true : withBytes(input, (p, n) => exps.poc_is_utf8(p, n) === 1);
    bufferBinding.isAscii = (input) =>
      input.length === 0 ? true : withBytes(input, (p, n) => exps.poc_is_ascii(p, n) === 1);
    bufferBinding.byteLengthUtf8 = (string) => {
      // simdutf assumes well-formed UTF-16; node replaces lone surrogates with
      // U+FFFD. Keep the JS implementation for that (rare) case.
      if (string.length === 0) return 0;
      if (!string.isWellFormed()) return jsFallback.byteLengthUtf8(string);
      const u16 = new Uint8Array(string.length * 2);
      for (let i = 0; i < string.length; i++) {
        const c = string.charCodeAt(i);
        u16[2 * i] = c & 0xff;
        u16[2 * i + 1] = c >>> 8;
      }
      return withBytes(u16, (p) => exps.poc_utf8_len_from_utf16le(p, string.length));
    };
    globalThis.__pocSimdutfBacked = true;
    globalThis.__pocSimdutfJsFallback = jsFallback;
  })();

  // -- uv: errno surface used by internal/errors for error translation --
  const UV_ERRNOS = [
    ['EACCES', -13, 'permission denied'],
    ['EBADF', -9, 'bad file descriptor'],
    ['EEXIST', -17, 'file already exists'],
    ['EINVAL', -22, 'invalid argument'],
    ['EISDIR', -21, 'illegal operation on a directory'],
    ['ENOENT', -2, 'no such file or directory'],
    ['ENOTDIR', -20, 'not a directory'],
    ['ENOTEMPTY', -39, 'directory not empty'],
    ['EPERM', -1, 'operation not permitted'],
  ];
  const uvBinding = withAutoStubs('uv', {
    getErrorMap: () => new Map(UV_ERRNOS.map(([code, errno, msg]) => [errno, [code, msg]])),
    errname(errno) {
      const hit = UV_ERRNOS.find(([, num]) => num === errno);
      return hit ? hit[0] : `UNKNOWN(${errno})`;
    },
    ...Object.fromEntries(UV_ERRNOS.map(([code, errno]) => [`UV_${code}`, errno])),
  });

  // -- permission (permission model off) --
  const permissionBinding = withAutoStubs('permission', {
    isEnabled: () => false,
    has: () => true,
  });

  // ---- POC scheduler: macrotask FIFO + node-style nextTick queue + wiring
  //      for internal/timers (setImmediate/setTimeout). Substrate is a chained
  //      microtask (bare isolates expose no true macrotask API); FIFO order,
  //      run-after-current-synchronous-code, and nextTick-drain-after-each-
  //      completion match node. Exact libuv phase parity is NOT claimed. ----
  const nextTickQueue = [];
  const macroQueue = [];
  let macroPumpScheduled = false;
  let tickDrainScheduled = false;
  let processImmediateCb = null;
  let processTimersCb = null;
  let immediatePumpQueued = false;
  let libuvClock = 1;
  const immediateInfoArr = new Uint32Array(3); // kCount, kRefCount, kHasOutstanding
  const timeoutInfoArr = new Int32Array(1);

  function drainNextTicks() {
    while (nextTickQueue.length > 0) {
      const { fn, args } = nextTickQueue.shift();
      fn(...args);
    }
  }

  function pumpImmediatesIfNeeded() {
    if (immediateInfoArr[0] > 0 && processImmediateCb && !immediatePumpQueued) {
      immediatePumpQueued = true;
      macroQueue.push(() => {
        immediatePumpQueued = false;
        processImmediateCb();
      });
    }
  }

  function pumpMacro() {
    macroPumpScheduled = false;
    drainNextTicks(); // ticks queued in the triggering turn run before I/O completions
    const task = macroQueue.shift();
    if (task) {
      try {
        task();
      } finally {
        drainNextTicks();
        pumpImmediatesIfNeeded();
      }
    }
    if (macroQueue.length > 0) ensureMacroPump();
  }

  function ensureMacroPump() {
    if (macroPumpScheduled) return;
    macroPumpScheduled = true;
    // Two-hop deferral: plain microtasks queued in the same turn (promise
    // continuations, the nextTick fallback drain) run before I/O completions,
    // approximating node's nextTick/microtask-before-poll ordering.
    Promise.resolve().then(() => Promise.resolve().then(pumpMacro));
  }

  function scheduleMacro(task) {
    macroQueue.push(task);
    ensureMacroPump();
  }

  process.nextTick = function nextTick(fn, ...args) {
    nextTickQueue.push({ fn, args });
    if (!tickDrainScheduled) {
      tickDrainScheduled = true;
      // Fallback drain for ticks queued outside a scheduler task.
      Promise.resolve().then(() => {
        tickDrainScheduled = false;
        drainNextTicks();
        // Piggyback: pick up immediates queued from synchronous code.
        pumpImmediatesIfNeeded();
        if (macroQueue.length > 0) ensureMacroPump();
      });
    }
  };
  globalThis.__pocScheduleMacro = scheduleMacro; // for ordering probes in checks

  // ---- fs: node-shaped errors + a pluggable backend. The in-memory backend
  //      below is the POC stand-in; a kernel/VFS-bridge backend implements the
  //      same object surface. ----
  function uvError(code, syscall, path) {
    const entry = UV_ERRNOS.find(([c]) => c === code);
    const [, errno, msg] = entry ?? [code, -4094, 'unknown error'];
    let message = `${code}: ${msg}, ${syscall}`;
    if (path !== undefined) message += ` '${path}'`;
    const err = new Error(message);
    err.errno = errno;
    err.code = code;
    err.syscall = syscall;
    if (path !== undefined) err.path = path;
    return err;
  }

  // ---- async fs contract: node's lib passes either an FSReqCallback (fire
  //      req.oncomplete(err, result) later, `this` = req) or the kUsePromises
  //      sentinel (return a Promise). The req can sit MID-args (e.g.
  //      stat(path, bigint, req, throwIfNoEntry)), so dispatch scans args.
  class FSReqCallback {
    constructor(bigint) {
      this.bigint = Boolean(bigint);
      this.oncomplete = null;
      this.context = undefined;
    }
  }
  const kUsePromises = Symbol('fs.promises');

  function finalizeFsBinding(ops, asyncOverrides = {}) {
    const wrapped = {};
    for (const name of Object.keys(ops)) {
      const syncFn = ops[name];
      wrapped[name] = function pocFsOp(...args) {
        const reqIdx = args.findIndex(
          (a) => a === kUsePromises || a instanceof FSReqCallback,
        );
        if (reqIdx === -1) return syncFn(...args);
        const req = args[reqIdx];
        args[reqIdx] = undefined; // keep positional args (throwIfNoEntry etc.)
        const exec = asyncOverrides[name]
          ? () => asyncOverrides[name](...args)
          : () => syncFn(...args);
        if (req === kUsePromises) {
          return new Promise((resolve, reject) => {
            scheduleMacro(() => {
              try {
                resolve(exec()); // adopts promises from async overrides
              } catch (err) {
                reject(err);
              }
            });
          });
        }
        scheduleMacro(() => {
          let result;
          try {
            result = exec();
          } catch (err) {
            req.oncomplete.call(req, err);
            return;
          }
          if (result && typeof result.then === 'function') {
            result.then(
              (value) => scheduleMacro(() => req.oncomplete.call(req, null, value)),
              (err) => scheduleMacro(() => req.oncomplete.call(req, err)),
            );
          } else {
            req.oncomplete.call(req, null, result);
          }
        });
        return undefined;
      };
    }

    // fs.promises open(): binding.openFileHandle(path, flags, mode, kUsePromises)
    // resolves a handle exposing fd/close()/closeSync()/getAsyncId().
    wrapped.openFileHandle = function openFileHandle(path, flags, mode, req) {
      const open = () => wrapped.open(path, flags, mode, kUsePromises);
      const makeHandle = (fd) => ({
        fd,
        close: () => wrapped.close(fd, kUsePromises),
        closeSync: () => ops.close(fd),
        getAsyncId: () => 0,
      });
      if (req === kUsePromises) return open().then(makeHandle);
      throw new Error('POC: openFileHandle only supports kUsePromises');
    };

    wrapped.FSReqCallback = FSReqCallback;
    wrapped.kUsePromises = kUsePromises;
    return wrapped;
  }

  function makeMemFsBackend() {
    // path -> { type: 'file', data: Uint8Array, mode } | { type: 'dir', mode }
    const nodes = new Map();
    const t0 = 1750000000; // fixed epoch seconds; deterministic mtimes
    nodes.set('/', { type: 'dir', mode: 0o755, mtime: t0 });
    nodes.set('/tmp', { type: 'dir', mode: 0o777, mtime: t0 });

    function normalize(p) {
      const parts = String(p).split('/');
      const out = [];
      for (const part of parts) {
        if (part === '' || part === '.') continue;
        if (part === '..') out.pop();
        else out.push(part);
      }
      return '/' + out.join('/');
    }
    const parentOf = (p) => (p === '/' ? null : p.slice(0, p.lastIndexOf('/')) || '/');

    return {
      normalize,
      get(p) { return nodes.get(normalize(p)); },
      requireDirParent(p, syscall) {
        const parent = parentOf(normalize(p));
        const node = parent === null ? null : nodes.get(parent);
        if (!node) throw uvError('ENOENT', syscall, p);
        if (node.type !== 'dir') throw uvError('ENOTDIR', syscall, p);
        return parent;
      },
      set(p, node) { nodes.set(normalize(p), node); },
      delete(p) { nodes.delete(normalize(p)); },
      list(p) {
        const norm = normalize(p);
        const prefix = norm === '/' ? '/' : `${norm}/`;
        const names = [];
        for (const key of nodes.keys()) {
          if (key !== norm && key.startsWith(prefix) && !key.slice(prefix.length).includes('/')) {
            names.push(key.slice(prefix.length));
          }
        }
        return names.sort();
      },
      now() { return t0 + 1000; },
    };
  }

  function makeFsBinding(backend) {
    const O = constantsBinding.fs; // real linux O_*/S_* values (harvested)
    // fd table: fd -> { path, pos, readable, writable, append }
    const fds = new Map();
    let nextFd = 3;

    function resolveFile(path, syscall) {
      const node = backend.get(path);
      if (!node) throw uvError('ENOENT', syscall, path);
      return node;
    }

    function openFd(path, flags, mode, syscall = 'open') {
      const accMode = flags & (O.O_WRONLY | O.O_RDWR);
      const writable = accMode !== 0;
      let node = backend.get(path);
      if (node?.type === 'dir') {
        if (writable) throw uvError('EISDIR', syscall, path);
      } else if (!node) {
        if (!(flags & O.O_CREAT)) throw uvError('ENOENT', syscall, path);
        backend.requireDirParent(path, syscall);
        node = { type: 'file', data: new Uint8Array(0), mode: mode ?? 0o666, mtime: backend.now() };
        backend.set(path, node);
      } else if ((flags & O.O_CREAT) && (flags & O.O_EXCL)) {
        throw uvError('EEXIST', syscall, path);
      }
      if (node.type === 'file' && (flags & O.O_TRUNC) && writable) {
        node.data = new Uint8Array(0);
      }
      const fd = nextFd++;
      fds.set(fd, {
        path: backend.normalize(path),
        pos: 0,
        readable: accMode !== O.O_WRONLY,
        writable,
        append: Boolean(flags & O.O_APPEND),
      });
      return fd;
    }

    function fdEntry(fd, syscall) {
      const entry = fds.get(fd);
      if (!entry) throw uvError('EBADF', syscall);
      return entry;
    }

    function statArrayFor(node, useBigint) {
      const isDir = node.type === 'dir';
      const size = isDir ? 0 : node.data.length;
      const mode = (isDir ? 0o040000 : 0o100000) | (node.mode & 0o7777);
      const mtime = node.mtime ?? backend.now();
      const fields = [
        1, mode, 1, 1000, 1000, 0, 4096, 1, size,
        Math.ceil(size / 512),
        mtime, 0, mtime, 0, mtime, 0, mtime, 0,
      ];
      if (useBigint) return new BigInt64Array(fields.map(BigInt));
      return new Float64Array(fields);
    }

    function writeAt(entry, node, bytes, position) {
      const usesCursor = position === null || position === undefined || position < 0;
      const pos = entry.append ? node.data.length : (usesCursor ? entry.pos : position);
      const end = pos + bytes.length;
      if (end > node.data.length) {
        const grown = new Uint8Array(end);
        grown.set(node.data);
        node.data = grown;
      }
      node.data.set(bytes, pos);
      node.mtime = backend.now();
      if (usesCursor || entry.append) entry.pos = end;
      return bytes.length;
    }

    return withAutoStubs('fs', finalizeFsBinding({
      open: (path, flags, mode) => openFd(path, flags, mode),
      close(fd) { fdEntry(fd, 'close'); fds.delete(fd); },
      read(fd, buffer, offset, length, position) {
        const entry = fdEntry(fd, 'read');
        if (!entry.readable) throw uvError('EBADF', 'read');
        const node = resolveFile(entry.path, 'read');
        const usesCursor = position === null || position === undefined || position < 0;
        const pos = usesCursor ? entry.pos : position;
        const n = Math.max(0, Math.min(length, node.data.length - pos));
        buffer.set(node.data.subarray(pos, pos + n), offset);
        if (usesCursor) entry.pos = pos + n;
        return n;
      },
      writeBuffer(fd, buffer, offset, length, position) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        const node = resolveFile(entry.path, 'write');
        return writeAt(entry, node, buffer.subarray(offset, offset + length), position);
      },
      writeString(fd, string, position, _encoding) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        const node = resolveFile(entry.path, 'write');
        const bytes = new Uint8Array(utf8ByteLength(string));
        utf8WriteInto(bytes, string, 0, bytes.length);
        return writeAt(entry, node, bytes, position);
      },
      writeBuffers(fd, buffers, position) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        const node = resolveFile(entry.path, 'write');
        let total = 0;
        let pos = position;
        for (const buffer of buffers) {
          total += writeAt(entry, node, buffer, pos);
          if (pos !== null && pos !== undefined && pos >= 0) pos += buffer.length;
        }
        return total;
      },
      readFileUtf8(pathOrFd, _flags) {
        const path = typeof pathOrFd === 'number' ? fdEntry(pathOrFd, 'read').path : pathOrFd;
        const node = resolveFile(path, 'open');
        if (node.type === 'dir') throw uvError('EISDIR', 'read', path);
        return utf8DecodeRange(node.data, 0, node.data.length);
      },
      writeFileUtf8(path, data, flags, mode) {
        const fd = openFd(path, flags, mode);
        try {
          const entry = fds.get(fd);
          const node = resolveFile(entry.path, 'write');
          const bytes = new Uint8Array(utf8ByteLength(data));
          utf8WriteInto(bytes, data, 0, bytes.length);
          writeAt(entry, node, bytes, null);
        } finally {
          fds.delete(fd);
        }
      },
      existsSync(path) {
        try { return backend.get(path) !== undefined; } catch { return false; }
      },
      stat(path, useBigint, _req, throwIfNoEntry) {
        const node = backend.get(path);
        if (!node) {
          if (throwIfNoEntry === false) return undefined;
          throw uvError('ENOENT', 'stat', path);
        }
        return statArrayFor(node, useBigint);
      },
      lstat(path, useBigint, _req, throwIfNoEntry) {
        const node = backend.get(path);
        if (!node) {
          if (throwIfNoEntry === false) return undefined;
          throw uvError('ENOENT', 'lstat', path);
        }
        return statArrayFor(node, useBigint);
      },
      fstat(fd, useBigint, _req, shouldNotThrow) {
        const entry = fds.get(fd);
        const node = entry ? backend.get(entry.path) : undefined;
        if (!node) {
          if (shouldNotThrow) return undefined;
          throw uvError(entry ? 'ENOENT' : 'EBADF', 'fstat', entry?.path);
        }
        return statArrayFor(node, useBigint);
      },
      mkdir(path, mode, recursive) {
        const norm = backend.normalize(path);
        if (backend.get(norm)) {
          if (recursive) return undefined;
          throw uvError('EEXIST', 'mkdir', path);
        }
        if (recursive) {
          const parts = norm.split('/').filter(Boolean);
          let current = '';
          let first;
          for (const part of parts) {
            current += `/${part}`;
            if (!backend.get(current)) {
              backend.set(current, { type: 'dir', mode: mode ?? 0o777, mtime: backend.now() });
              first ??= current;
            }
          }
          return first;
        }
        backend.requireDirParent(norm, 'mkdir');
        backend.set(norm, { type: 'dir', mode: mode ?? 0o777, mtime: backend.now() });
        return undefined;
      },
      readdir(path, _encoding, withFileTypes) {
        const node = backend.get(path);
        if (!node) throw uvError('ENOENT', 'scandir', path);
        if (node.type !== 'dir') throw uvError('ENOTDIR', 'scandir', path);
        const names = backend.list(path);
        if (!withFileTypes) return names;
        const types = names.map((name) =>
          backend.get(`${backend.normalize(path)}/${name}`).type === 'dir' ? 2 : 1);
        return [names, types];
      },
      unlink(path) {
        const node = backend.get(path);
        if (!node) throw uvError('ENOENT', 'unlink', path);
        if (node.type === 'dir') throw uvError('EISDIR', 'unlink', path);
        backend.delete(path);
      },
      rmdir(path) {
        const node = backend.get(path);
        if (!node) throw uvError('ENOENT', 'rmdir', path);
        if (node.type !== 'dir') throw uvError('ENOTDIR', 'rmdir', path);
        if (backend.list(path).length > 0) throw uvError('ENOTEMPTY', 'rmdir', path);
        backend.delete(path);
      },
    }));
  }

  // Real-VFS mode: when the agentOS `_fs*` sync bridge globals are present
  // (inside a sidecar VM), implement the SAME internalBinding('fs') surface
  // over them, so node's real lib/fs.js talks to the actual kernel/VFS.
  // Bridge globals are node-API-shaped (fs.statSync, fs.readFileSync, ...).
  // fd-level ops are emulated client-side over whole-file reads/writes for the
  // POC; true fd mapping would use the fs.openSync/fs.readSync bridge globals.
  function makeBridgeFsBinding() {
    const g = globalThis;
    const fds = new Map();
    let nextFd = 3;
    const O = constantsBinding.fs;

    // Sidecar errors arrive as exceptions whose message leads with the code:
    // "ENOENT: no such file or directory, ...". Re-shape into node-style errors
    // with the syscall the caller expects.
    function translate(err, syscall, path) {
      const msg = err?.message ?? '';
      const match = /^([A-Z]+):/.exec(msg);
      if (match && UV_ERRNOS.some(([code]) => code === match[1])) {
        throw uvError(match[1], syscall, path);
      }
      // Some kernel errors carry only prose; map the common ones.
      if (/entry not found|no such file or directory|not found/i.test(msg)) {
        throw uvError('ENOENT', syscall, path);
      }
      if (/already exists/i.test(msg)) throw uvError('EEXIST', syscall, path);
      if (/permission denied/i.test(msg)) throw uvError('EACCES', syscall, path);
      if (/not a directory/i.test(msg)) throw uvError('ENOTDIR', syscall, path);
      if (/directory not empty/i.test(msg)) throw uvError('ENOTEMPTY', syscall, path);
      if (/is a directory/i.test(msg)) throw uvError('EISDIR', syscall, path);
      throw err;
    }
    // Complex bridge results may arrive as JSON strings.
    function decodeJson(value) {
      return typeof value === 'string' ? JSON.parse(value) : value;
    }
    function call(fn, syscall, path, ...args) {
      try {
        return fn(...args);
      } catch (err) {
        translate(err, syscall, path);
      }
    }

    // _fsReadFileBinary returns base64; binary writes go raw when
    // _fsWriteFileBinaryRaw exists, else base64-tagged.
    function toBytes(value) {
      if (value instanceof Uint8Array) return value;
      if (Array.isArray(value)) return Uint8Array.from(value);
      if (typeof value === 'string') {
        const out = new Uint8Array(Math.ceil(value.length * 3 / 4));
        return out.subarray(0, base64WriteInto(out, value, 0, out.length));
      }
      if (value && value.type === 'Buffer' && Array.isArray(value.data)) {
        return Uint8Array.from(value.data);
      }
      throw new Error(`POC: unexpected bridge byte payload ${Object.prototype.toString.call(value)}`);
    }
    function writeBytes(path, bytes) {
      if (typeof g._fsWriteFileBinaryRaw === 'function') {
        call(g._fsWriteFileBinaryRaw, 'write', path, path, bytes);
        return;
      }
      call(g._fsWriteFileBinary, 'write', path, path, {
        __agentOSType: 'bytes',
        base64: base64EncodeRange(bytes, 0, bytes.length, B64_STD, true),
      });
    }

    function statObj(path, syscall, lstat = false) {
      return decodeJson(call(lstat ? g._fsLstat : g._fsStat, syscall, path, path));
    }

    function statArrayFromObj(obj, useBigint, sizeOverride) {
      const size = sizeOverride ?? obj.size ?? 0;
      const mtimeMs = obj.mtimeMs ?? 0;
      const sec = Math.floor(mtimeMs / 1000);
      const nsec = Math.round((mtimeMs % 1000) * 1e6);
      const fields = [
        obj.dev ?? 1, obj.mode ?? 0o100644, obj.nlink ?? 1, obj.uid ?? 1000,
        obj.gid ?? 1000, obj.rdev ?? 0, obj.blksize ?? 4096, obj.ino ?? 1,
        size, obj.blocks ?? Math.ceil(size / 512),
        sec, nsec, sec, nsec, sec, nsec, sec, nsec,
      ];
      if (useBigint) return new BigInt64Array(fields.map((v) => BigInt(Math.trunc(v))));
      return new Float64Array(fields);
    }

    function readWholeFile(path, syscall) {
      return toBytes(call(g._fsReadFileBinary, syscall, path, path));
    }

    function openFd(path, flags, mode, syscall = 'open') {
      const accMode = flags & (O.O_WRONLY | O.O_RDWR);
      const writable = accMode !== 0;
      let data = null;
      if ((flags & O.O_CREAT) && (flags & O.O_EXCL) && g._fsExists(path)) {
        throw uvError('EEXIST', syscall, path);
      }
      if ((flags & O.O_TRUNC) && writable) {
        data = new Uint8Array(0);
      } else {
        try {
          data = readWholeFile(path, syscall);
        } catch (err) {
          if (err.code === 'ENOENT' && (flags & O.O_CREAT)) data = new Uint8Array(0);
          else throw err;
        }
      }
      const fd = nextFd++;
      fds.set(fd, {
        path, data,
        pos: (flags & O.O_APPEND) ? data.length : 0,
        readable: accMode !== O.O_WRONLY,
        writable,
        append: Boolean(flags & O.O_APPEND),
        dirty: writable && Boolean(flags & (O.O_CREAT | O.O_TRUNC)),
      });
      return fd;
    }

    function fdEntry(fd, syscall) {
      const entry = fds.get(fd);
      if (!entry) throw uvError('EBADF', syscall);
      return entry;
    }

    function writeAt(entry, bytes, position) {
      const usesCursor = position === null || position === undefined || position < 0;
      const pos = entry.append ? entry.data.length : (usesCursor ? entry.pos : position);
      const end = pos + bytes.length;
      if (end > entry.data.length) {
        const grown = new Uint8Array(end);
        grown.set(entry.data);
        entry.data = grown;
      }
      entry.data.set(bytes, pos);
      entry.dirty = true;
      if (usesCursor || entry.append) entry.pos = end;
      return bytes.length;
    }

    function normalizeReaddir(result, withFileTypes) {
      let entries;
      if (result instanceof Uint8Array) {
        // Raw payload: repeating [kind:u8][nameLen:u32le][nameBytes].
        entries = [];
        let offset = 0;
        while (offset < result.byteLength) {
          const kind = result[offset++];
          const nameLength = result[offset] | (result[offset + 1] << 8) |
            (result[offset + 2] << 16) | (result[offset + 3] << 24);
          offset += 4;
          entries.push({
            name: utf8DecodeRange(result, offset, offset + nameLength),
            isDirectory: kind === 1,
          });
          offset += nameLength;
        }
      } else {
        entries = decodeJson(result);
      }
      entries = entries.filter((e) => e.name !== '.' && e.name !== '..');
      const names = entries.map((e) => (typeof e === 'string' ? e : e.name));
      if (!withFileTypes) return names;
      return [names, entries.map((e) => (typeof e === 'object' && e.isDirectory ? 2 : 1))];
    }

    // Async overrides route through the real `_fs*Async` bridge globals, so
    // completions arrive via the session event stream (true async I/O), then
    // get re-shaped exactly like the sync paths.
    const asyncCall = (promise, syscall, path) =>
      promise.catch((err) => translate(err, syscall, path));

    const statAsync = (asyncFn, syscall) => (path, useBigint, _req, throwIfNoEntry) =>
      asyncFn(path).then(
        (v) => statArrayFromObj(decodeJson(v), useBigint),
        (err) => {
          try {
            translate(err, syscall, path);
          } catch (shaped) {
            if (shaped.code === 'ENOENT' && throwIfNoEntry === false) return undefined;
            throw shaped;
          }
        },
      );

    async function openFdAsync(path, flags, mode) {
      const accMode = flags & (O.O_WRONLY | O.O_RDWR);
      const writable = accMode !== 0;
      let data = null;
      if ((flags & O.O_CREAT) && (flags & O.O_EXCL)) {
        const exists = await g._fsStatAsync(path).then(() => true, () => false);
        if (exists) throw uvError('EEXIST', 'open', path);
      }
      if ((flags & O.O_TRUNC) && writable) {
        data = new Uint8Array(0);
      } else {
        try {
          data = toBytes(await asyncCall(g._fsReadFileBinaryAsync(path), 'open', path));
        } catch (err) {
          if (err.code === 'ENOENT' && (flags & O.O_CREAT)) data = new Uint8Array(0);
          else throw err;
        }
      }
      const fd = nextFd++;
      fds.set(fd, {
        path, data,
        pos: (flags & O.O_APPEND) ? data.length : 0,
        readable: accMode !== O.O_WRONLY,
        writable,
        append: Boolean(flags & O.O_APPEND),
        dirty: writable && Boolean(flags & (O.O_CREAT | O.O_TRUNC)),
      });
      return fd;
    }

    const bridgeAsyncOverrides = {
      open: (path, flags, mode) => openFdAsync(path, flags, mode),
      close: async (fd) => {
        const entry = fdEntry(fd, 'close');
        fds.delete(fd);
        if (entry.dirty) {
          await asyncCall(g._fsWriteFileBinaryAsync(entry.path, {
            __agentOSType: 'bytes',
            base64: base64EncodeRange(entry.data, 0, entry.data.length, B64_STD, true),
          }), 'write', entry.path);
        }
      },
      stat: statAsync((p) => g._fsStatAsync(p), 'stat'),
      lstat: statAsync((p) => g._fsLstatAsync(p), 'lstat'),
      unlink: (path) => asyncCall(g._fsUnlinkAsync(path), 'unlink', path).then(() => undefined),
      rmdir: (path) => asyncCall(g._fsRmdirAsync(path), 'rmdir', path).then(() => undefined),
      mkdir: (path, mode, recursive) =>
        asyncCall(g._fsMkdirAsync(path, { recursive: Boolean(recursive), mode }), 'mkdir', path)
          .then(() => undefined),
      readdir: (path, _encoding, withFileTypes) =>
        g._fsStatAsync(path).then(
          () => asyncCall(g._fsReadDirAsync(path), 'scandir', path),
          (err) => translate(err, 'scandir', path),
        ).then((result) => normalizeReaddir(result, withFileTypes)),
    };

    return withAutoStubs('fs', finalizeFsBinding({
      open: (path, flags, mode) => openFd(path, flags, mode),
      close(fd) {
        const entry = fdEntry(fd, 'close');
        if (entry.dirty) writeBytes(entry.path, entry.data);
        fds.delete(fd);
      },
      read(fd, buffer, offset, length, position) {
        const entry = fdEntry(fd, 'read');
        if (!entry.readable) throw uvError('EBADF', 'read');
        const usesCursor = position === null || position === undefined || position < 0;
        const pos = usesCursor ? entry.pos : position;
        const n = Math.max(0, Math.min(length, entry.data.length - pos));
        buffer.set(entry.data.subarray(pos, pos + n), offset);
        if (usesCursor) entry.pos = pos + n;
        return n;
      },
      writeBuffer(fd, buffer, offset, length, position) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        return writeAt(entry, buffer.subarray(offset, offset + length), position);
      },
      writeString(fd, string, position, _encoding) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        const bytes = new Uint8Array(utf8ByteLength(string));
        utf8WriteInto(bytes, string, 0, bytes.length);
        return writeAt(entry, bytes, position);
      },
      writeBuffers(fd, buffers, position) {
        const entry = fdEntry(fd, 'write');
        if (!entry.writable) throw uvError('EBADF', 'write');
        let total = 0;
        let pos = position;
        for (const buffer of buffers) {
          total += writeAt(entry, buffer, pos);
          if (pos !== null && pos !== undefined && pos >= 0) pos += buffer.length;
        }
        return total;
      },
      readFileUtf8(pathOrFd, _flags) {
        if (typeof pathOrFd === 'number') {
          const entry = fdEntry(pathOrFd, 'read');
          return utf8DecodeRange(entry.data, 0, entry.data.length);
        }
        return call(g._fsReadFile, 'open', pathOrFd, pathOrFd, 'utf8');
      },
      writeFileUtf8(path, data, flags, _mode) {
        if (flags & O.O_APPEND) {
          let existing = '';
          try {
            existing = call(g._fsReadFile, 'open', path, path, 'utf8');
          } catch (err) {
            if (err.code !== 'ENOENT') throw err;
          }
          call(g._fsWriteFile, 'write', path, path, existing + data);
          return;
        }
        call(g._fsWriteFile, 'write', path, path, data);
      },
      existsSync: (path) => Boolean(g._fsExists(path)),
      stat(path, useBigint, _req, throwIfNoEntry) {
        try {
          return statArrayFromObj(statObj(path, 'stat'), useBigint);
        } catch (err) {
          if (err.code === 'ENOENT' && throwIfNoEntry === false) return undefined;
          throw err;
        }
      },
      lstat(path, useBigint, _req, throwIfNoEntry) {
        try {
          return statArrayFromObj(statObj(path, 'lstat', true), useBigint);
        } catch (err) {
          if (err.code === 'ENOENT' && throwIfNoEntry === false) return undefined;
          throw err;
        }
      },
      fstat(fd, useBigint, _req, shouldNotThrow) {
        const entry = fds.get(fd);
        if (!entry) {
          if (shouldNotThrow) return undefined;
          throw uvError('EBADF', 'fstat');
        }
        try {
          const obj = entry.dirty
            ? { mode: 0o100644, mtimeMs: 0 }
            : statObj(entry.path, 'fstat');
          return statArrayFromObj(obj, useBigint, entry.data.length);
        } catch (err) {
          if (shouldNotThrow) return undefined;
          throw err;
        }
      },
      mkdir(path, mode, recursive) {
        call(g._fsMkdir, 'mkdir', path, path, { recursive: Boolean(recursive), mode });
        return undefined;
      },
      readdir(path, _encoding, withFileTypes) {
        // The kernel returns empty for a missing dir; node needs ENOENT.
        if (!g._fsExists(path)) throw uvError('ENOENT', 'scandir', path);
        const result = call(g._fsReadDir, 'scandir', path, path);
        return normalizeReaddir(result, withFileTypes);
      },
      unlink(path) { call(g._fsUnlink, 'unlink', path, path); },
      rmdir(path) { call(g._fsRmdir, 'rmdir', path, path); },
    }, bridgeAsyncOverrides));
  }

  // Explicit opt-in: bridge globals exist as unserviced stubs in light
  // harnesses too, so presence-sniffing would deadlock a sync call there.
  const fsBinding = globalThis.__pocUseBridgeFs === true
    ? makeBridgeFsBinding()
    : makeFsBinding(makeMemFsBackend());

  // -- builtins / module_wrap / errors: what the REAL realm.js loader needs --
  let requireBuiltin = null; // captured from realm.js via setInternalLoaders
  let realmInternalBinding = null;

  const builtinsBinding = {
    builtinIds: Object.keys(sources),
    compileFunction: (id) => compileBuiltin(id, [
      'exports', 'require', 'module', 'process', 'internalBinding', 'primordials',
    ]),
    // Node's C++ stores the realm loaders here; we do the same to obtain them.
    setInternalLoaders(binding, require) {
      realmInternalBinding = binding;
      requireBuiltin = require;
    },
  };
  class ModuleWrap {}
  const errorsBinding = withAutoStubs('errors', {
    setPrepareStackTraceCallback() {},
    setEnhanceStackForFatalException() {},
    setSourceMapsSupport() {},
    triggerUncaughtException(err) { throw err; },
    exitCodes: {
      kNoFailure: 0, kGenericUserError: 1, kInternalJSParseError: 3,
      kInternalJSEvaluationFailure: 4, kV8FatalError: 5, kInvalidFatalExceptionMonkeyPatching: 6,
      kExceptionInFatalExceptionHandler: 7, kInvalidCommandLineArgument: 9,
      kBootstrapFailure: 10, kInvalidCommandLineArgument2: 12, kUnsettledTopLevelAwait: 13,
      kUnCaughtException: 1,
    },
  });

  const bindings = {
    __proto__: null,
    constants: constantsBinding,
    config: configBinding,
    util: utilBinding,
    types: typesBinding,
    string_decoder: stringDecoderBinding,
    options: optionsBinding,
    messaging: messagingBinding,
    buffer: bufferBinding,
    builtins: builtinsBinding,
    module_wrap: { ModuleWrap },
    errors: errorsBinding,
    uv: uvBinding,
    permission: permissionBinding,
    fs: fsBinding,
    encoding_binding: (() => {
      const encodeIntoResults = new Uint32Array(2);
      return withAutoStubs('encoding_binding', {
        encodeIntoResults,
        encodeUtf8String(input) {
          const out = new Uint8Array(utf8ByteLength(input));
          utf8WriteInto(out, input, 0, out.length);
          return out;
        },
        encodeInto(input, dest) {
          const written = utf8WriteInto(dest, input, 0, dest.length);
          // POC: `read` is approximated as chars consumed for fully-written input.
          encodeIntoResults[0] = written >= utf8ByteLength(input) ? input.length : 0;
          encodeIntoResults[1] = written;
        },
        decodeUTF8: (input, _ignoreBom, _fatal) => utf8DecodeRange(input, 0, input.length),
        decodeLatin1: (input) => charSlice(input, 0, input.length, 0xff),
      });
    })(),
    fs_dir: withAutoStubs('fs_dir', {}),
    credentials: withAutoStubs('credentials', { getTempDir: () => '/tmp' }),
    performance: withAutoStubs('performance', {
      constants: {
        NODE_PERFORMANCE_MILESTONE_TIME_ORIGIN: 0,
        NODE_PERFORMANCE_MILESTONE_TIME_ORIGIN_TIMESTAMP: 1,
        NODE_PERFORMANCE_MILESTONE_ENVIRONMENT: 2,
        NODE_PERFORMANCE_MILESTONE_NODE_START: 3,
        NODE_PERFORMANCE_MILESTONE_V8_START: 4,
        NODE_PERFORMANCE_MILESTONE_LOOP_START: 5,
        NODE_PERFORMANCE_MILESTONE_LOOP_EXIT: 6,
      },
      milestones: new Float64Array(8),
      now: () => libuvClock,
      markMilestone() {},
    }),
    os: withAutoStubs('os', {
      getHostname: () => 'agentos-poc',
      getHomeDirectory: () => '/root',
      getOSInformation: () => ['Linux', 'agentos', '6.1.0', 'x86_64'],
      isBigEndian: false,
      getAvailableParallelism: () => 1,
      getFreeMem: () => 0,
      getTotalMem: () => 0,
      getUptime: () => 0,
      getLoadAvg: (arr) => { arr[0] = 0; arr[1] = 0; arr[2] = 0; },
      getCPUs: () => [],
    }),
    // fs.watch is out of POC scope; class must exist for load-time destructure.
    fs_event_wrap: withAutoStubs('fs_event_wrap', {
      FSEvent: class FSEvent {
        start() { throw new Error('POC: fs.watch not supported'); }
        close() {}
      },
    }),
    diagnostics_channel: (() => {
      const channelIndexes = new Map();
      return withAutoStubs('diagnostics_channel', {
        subscribers: [],
        linkNativeChannel() {},
        getOrCreateChannelIndex(name) {
          if (!channelIndexes.has(name)) channelIndexes.set(name, channelIndexes.size);
          return channelIndexes.get(name);
        },
      });
    })(),
    blob: withAutoStubs('blob', {}),
    url: withAutoStubs('url', {}),
    url_pattern: withAutoStubs('url_pattern', { URLPattern: class URLPattern {} }),
    symbols: perIsolateSymbols,
    // Indexes only need to be internally consistent: no C++ side reads them here.
    async_wrap: withAutoStubs('async_wrap', {
      setCallbackTrampoline() {},
      async_hook_fields: new Uint32Array(16),
      async_id_fields: new Float64Array(8),
      async_ids_stack: new Float64Array(4096),
      execution_async_resources: [],
      registerDestroyHook() {},
      queueDestroyAsyncId() {},
      constants: {
        kInit: 0, kBefore: 1, kAfter: 2, kDestroy: 3, kPromiseResolve: 4,
        kTotals: 5, kCheck: 6, kStackLength: 7, kUsesExecutionAsyncResource: 8,
        kExecutionAsyncId: 0, kTriggerAsyncId: 1, kAsyncIdCounter: 2,
        kDefaultTriggerAsyncId: 3,
      },
    }),
    async_context_frame: (() => {
      let frame;
      return withAutoStubs('async_context_frame', {
        getContinuationPreservedEmbedderData: () => frame,
        setContinuationPreservedEmbedderData(value) { frame = value; },
      });
    })(),
    trace_events: withAutoStubs('trace_events', {
      getCategoryEnabledBuffer: () => new Uint8Array(1),
      trace() {},
      isTraceCategoryEnabled: () => false,
    }),
    task_queue: withAutoStubs('task_queue', {
      enqueueMicrotask(fn) { Promise.resolve().then(fn); },
      setTickCallback() {},
      setPromiseRejectCallback() {},
    }),
    // Wired into the POC scheduler: internal/timers registers its
    // processImmediate/processTimers callbacks through setupTimers (exactly
    // what node's C++ stores), and scheduleTimer turns into a macrotask that
    // advances the fake libuv clock past the requested expiry.
    timers: withAutoStubs('timers', {
      immediateInfo: immediateInfoArr,
      timeoutInfo: timeoutInfoArr,
      getLibuvNow: () => libuvClock,
      setupTimers(processImmediate, processTimers) {
        processImmediateCb = processImmediate;
        processTimersCb = processTimers;
      },
      scheduleTimer(msecs) {
        scheduleMacro(() => {
          libuvClock += Math.max(1, msecs);
          if (processTimersCb) processTimersCb(libuvClock);
        });
      },
      toggleTimerRef() {},
      // Fires on immediate refcount 0->1 (first pending immediate): our cue to
      // pump, since there is no per-loop-iteration check phase here. NOTE the
      // Immediate constructor calls ref() BEFORE immediateInfo[kCount]++, so
      // the count must be checked from a scheduled task, not synchronously.
      toggleImmediateRef(on) {
        if (on) {
          scheduleMacro(() => {
            if (immediateInfoArr[0] > 0 && processImmediateCb) processImmediateCb();
          });
        }
      },
    }),
    process_methods: (() => {
      const hrtimeBuffer = new Uint32Array(3);
      return withAutoStubs('process_methods', {
        hrtimeBuffer,
        hrtime() {
          const ns = Date.now() * 1e6;
          const sec = Math.floor(ns / 1e9);
          hrtimeBuffer[0] = Math.floor(sec / 0x100000000);
          hrtimeBuffer[1] = sec >>> 0;
          hrtimeBuffer[2] = ns % 1e9;
        },
      });
    })(),
    mksnapshot: withAutoStubs('mksnapshot', {
      setSerializeCallback() {},
      setDeserializeCallback() {},
      setDeserializeMainFunction() {},
      isBuildingSnapshotBuffer: new Uint8Array([0]),
    }),
  };

  function getInternalBinding(name) {
    return bindings[name] ?? missingBinding(name);
  }
  function getLinkedBinding(name) {
    throw new Error(`POC: linked binding '${name}' not supported`);
  }

  // ---- run node's REAL realm bootstrap (lib/internal/bootstrap/realm.js) ----
  compileBuiltin('internal/bootstrap/realm', [
    'process', 'getLinkedBinding', 'getInternalBinding', 'primordials',
  ])(process, getLinkedBinding, getInternalBinding, primordials);

  if (typeof requireBuiltin !== 'function') {
    throw new Error('POC: realm.js did not hand back its loaders via setInternalLoaders');
  }

  // Wire node's REAL timer processing (normally done by bootstrap/node.js:
  // getTimerCallbacks(runNextTicks) + setupTimers). Our scheduler drives
  // processImmediate/processTimers instead of libuv's check/timer phases.
  {
    const internalTimers = requireBuiltin('internal/timers');
    const { processImmediate, processTimers } =
      internalTimers.getTimerCallbacks(drainNextTicks);
    bindings.timers.setupTimers(processImmediate, processTimers);
    // Normally done by internal/process/pre_execution.js.
    requireBuiltin('internal/util/debuglog').initializeDebugEnv(process.env.NODE_DEBUG);
  }

  globalThis.__poc = {
    requireBuiltin,
    internalBinding: realmInternalBinding,
    process,
    primordials,
  };
})();
