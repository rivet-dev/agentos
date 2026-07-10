'use strict';

// Bootstrap Node's pinned realm in the guest context. Bindings not yet owned by
// the active migration milestone remain property-readable inert adapters; M1's
// filesystem and CJS-loader dependencies below are live bridge/HOST providers.
(function installAgentOSNodeRealm(global) {
  try {
  const globalDescriptors = Object.getOwnPropertyDescriptors(global);
  const sources = global.__agentOSNodeSources;
  if (!sources || typeof sources !== 'object') {
    const error = new Error('AgentOS Node sources were not injected');
    error.code = 'ERR_NODE_STDLIB_SOURCES_MISSING';
    throw error;
  }

  function compileBuiltin(id, params) {
    const source = sources[id];
    if (typeof source !== 'string') {
      const error = new Error(`Missing pinned Node builtin: ${id}`);
      error.code = 'ERR_NODE_STDLIB_BUILTIN_MISSING';
      throw error;
    }
    const compiled = new Function(...params, `${source}\n//# sourceURL=node:${id}`);
    return function agentOSBuiltinEntry(...args) {
      try {
        return Reflect.apply(compiled, this, args);
      } catch (error) {
        if (error && typeof error === 'object' && error.nodeBuiltin === undefined) {
          error.nodeBuiltin = id;
          error.message = `[node:${id}] ${error.message}`;
        }
        throw error;
      }
    };
  }

  function lazySymbols(prefix) {
    const values = Object.create(null);
    return new Proxy(values, {
      get(_target, property) {
        if (typeof property !== 'string') return undefined;
        return values[property] ??= Symbol(`${prefix}:${property}`);
      },
    });
  }

  const privateSymbols = lazySymbols('private');
  const perIsolateSymbols = lazySymbols('per-isolate');
  const errorPrepareStackTrace = Object.getOwnPropertyDescriptor(Error, 'prepareStackTrace');
  const primordials = { __proto__: null };
  compileBuiltin('internal/per_context/primordials', [
    'exports',
    'primordials',
    'privateSymbols',
    'perIsolateSymbols',
  ])({}, primordials, privateSymbols, perIsolateSymbols);

  const process = global.process;
  const processPrototype = Object.getPrototypeOf(process);
  const processDescriptors = Object.getOwnPropertyDescriptors(process);
  const hrtime = (previous) => {
    const now = BigInt(Date.now()) * 1_000_000n;
    const value = [Number(now / 1_000_000_000n), Number(now % 1_000_000_000n)];
    if (!previous) return value;
    let seconds = value[0] - previous[0];
    let nanoseconds = value[1] - previous[1];
    if (nanoseconds < 0) { seconds--; nanoseconds += 1_000_000_000; }
    return [seconds, nanoseconds];
  };
  hrtime.bigint = () => BigInt(Date.now()) * 1_000_000n;
  Object.assign(process, {
    argv: Array.isArray(process.argv) ? process.argv : [],
    execArgv: Array.isArray(process.execArgv) ? process.execArgv : [],
    pid: 1,
    ppid: 0,
    version: 'v24.15.0',
    moduleLoadList: [],
    features: Object.assign(Object.create(null), process.features, {
      inspector: false,
      debug: false,
      uv: true,
      ipv6: true,
      tls_alpn: true,
      tls_sni: true,
      tls_ocsp: true,
      tls: true,
    }),
    emitWarning() {},
    hasUncaughtExceptionCaptureCallback() { return false; },
    setUncaughtExceptionCaptureCallback() {},
    cwd() { return '/'; },
    hrtime,
    umask() { return 0o22; },
    nextTick(callback, ...args) {
      Promise.resolve().then(() => callback(...args));
    },
    memoryUsage() {
      return { rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0 };
    },
  });

  function makeInert() {
    let proxy;
    const properties = new Map();
    const target = function agentOSInertNative() { return proxy; };
    proxy = new Proxy(target, {
      apply() { return makeInert(); },
      construct() { return makeInert(); },
      get(innerTarget, property, receiver) {
        if (property === Symbol.toPrimitive) return () => 0;
        if (property === Symbol.iterator) return function* emptyIterator() {};
        if (property === 'then') return undefined;
        if (property === 'toJSON') return () => Object.create(null);
        if (property === 'prototype') return innerTarget.prototype;
        const existing = Reflect.get(innerTarget, property, receiver);
        if (existing !== undefined) return existing;
        if (!properties.has(property)) properties.set(property, makeInert());
        return properties.get(property);
      },
      set() { return true; },
    });
    return proxy;
  }
  const inert = makeInert();

  let requireBuiltin;
  let internalBinding;
  const bindingCache = new Map();
  const builtinIds = Object.keys(sources);
  const builtins = {
    builtinIds,
    compileFunction(id) {
      return compileBuiltin(id, [
        'exports', 'require', 'module', 'process', 'internalBinding', 'primordials',
      ]);
    },
    setInternalLoaders(binding, require) {
      internalBinding = binding;
      requireBuiltin = require;
    },
  };
  class ModuleWrap {
    instantiate() {}
    evaluate() {}
    setExport() {}
  }
  const errors = {
    setPrepareStackTraceCallback() {},
    setEnhanceStackForFatalException() {},
    setSourceMapsSupport() {},
    triggerUncaughtException(error) { throw error; },
    exitCodes: {
      kNoFailure: 0,
      kGenericUserError: 1,
      kInternalJSParseError: 3,
      kInternalJSEvaluationFailure: 4,
      kV8FatalError: 5,
      kBootstrapFailure: 10,
      kUnCaughtException: 1,
    },
  };
  const config = {
    hasIntl: false,
    hasSmallICU: false,
    hasOpenSSL: true,
    hasQuic: false,
    // Public inspector modules must load inertly in M0 even though calls fail.
    hasInspector: true,
    hasTracing: true,
    hasSQLite: false,
    noBrowserGlobals: true,
  };
  const cliOptions = Object.assign(Object.create(null), {
    '--conditions': [],
    '--no-addons': false,
    '--require-module': false,
    '--experimental-quic': false,
    '--preserve-symlinks': false,
    '--preserve-symlinks-main': false,
    '--pending-deprecation': false,
    '--network-family-autoselection': true,
    '--network-family-autoselection-attempt-timeout': 250,
    '--experimental-detect-module': false,
    '--inspect-brk': false,
    '--inspect-wait': false,
    '--experimental-transform-types': false,
    '--experimental-strip-types': false,
    '--experimental-import-meta-resolve': false,
  });
  const options = {
    getCLIOptionsValues: () => cliOptions,
    getCLIOptionsInfo: () => ({ options: new Map(), aliases: new Map() }),
    getOptionsAsFlags: () => [],
    getEmbedderOptions: () => ({
      shouldNotRegisterESMLoader: true,
      noGlobalSearchPaths: true,
      noBrowserGlobals: true,
      hasEmbedderPreload: false,
    }),
  };
  const asyncWrap = {
    async_hook_fields: new Uint32Array(16),
    async_id_fields: new Float64Array(8),
    async_ids_stack: new Float64Array(4096),
    execution_async_resources: [],
    constants: {
      kInit: 0, kBefore: 1, kAfter: 2, kDestroy: 3, kPromiseResolve: 4,
      kTotals: 5, kCheck: 6, kStackLength: 7, kUsesExecutionAsyncResource: 8,
      kExecutionAsyncId: 0, kTriggerAsyncId: 1, kAsyncIdCounter: 2,
      kDefaultTriggerAsyncId: 3,
    },
  };
  const brand = (value) => Object.prototype.toString.call(value);
  const types = {
    isAnyArrayBuffer: (value) =>
      brand(value) === '[object ArrayBuffer]' || brand(value) === '[object SharedArrayBuffer]',
    isArrayBuffer: (value) => brand(value) === '[object ArrayBuffer]',
    isSharedArrayBuffer: (value) => brand(value) === '[object SharedArrayBuffer]',
    isDataView: (value) => brand(value) === '[object DataView]',
    isDate: (value) => brand(value) === '[object Date]',
    isMap: (value) => brand(value) === '[object Map]',
    isSet: (value) => brand(value) === '[object Set]',
    isWeakMap: (value) => brand(value) === '[object WeakMap]',
    isWeakSet: (value) => brand(value) === '[object WeakSet]',
    isRegExp: (value) => brand(value) === '[object RegExp]',
    isNativeError: (value) => value instanceof Error,
    isPromise: (value) => brand(value) === '[object Promise]',
    isProxy: () => false,
    isExternal: () => false,
    isModuleNamespaceObject: (value) => brand(value) === '[object Module]',
    isArgumentsObject: (value) => brand(value) === '[object Arguments]',
    isBoxedPrimitive: (value) =>
      typeof value === 'object' && /\[(?:object )?(?:Number|String|Boolean|BigInt|Symbol)\]/.test(brand(value)),
    isCryptoKey: () => false,
    isKeyObject: () => false,
  };
  const utf8Encode = (value) => {
    const bytes = [];
    for (let index = 0; index < value.length; index++) {
      let code = value.charCodeAt(index);
      if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
        const low = value.charCodeAt(index + 1);
        if (low >= 0xdc00 && low <= 0xdfff) {
          code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
          index++;
        } else {
          code = 0xfffd;
        }
      } else if (code >= 0xdc00 && code <= 0xdfff) {
        code = 0xfffd;
      }
      if (code < 0x80) bytes.push(code);
      else if (code < 0x800) bytes.push(0xc0 | (code >>> 6), 0x80 | (code & 63));
      else if (code < 0x10000) bytes.push(0xe0 | (code >>> 12), 0x80 | ((code >>> 6) & 63), 0x80 | (code & 63));
      else bytes.push(0xf0 | (code >>> 18), 0x80 | ((code >>> 12) & 63),
        0x80 | ((code >>> 6) & 63), 0x80 | (code & 63));
    }
    return Uint8Array.from(bytes);
  };
  const utf8Decode = (buffer) => {
    let output = '';
    for (let index = 0; index < buffer.length;) {
      const first = buffer[index++];
      if (first < 0x80) { output += String.fromCharCode(first); continue; }
      const count = first < 0xe0 ? 1 : first < 0xf0 ? 2 : 3;
      let code = first & (count === 1 ? 0x1f : count === 2 ? 0x0f : 0x07);
      let valid = true;
      for (let part = 0; part < count; part++) {
        const next = buffer[index++];
        if ((next & 0xc0) !== 0x80) { valid = false; break; }
        code = (code << 6) | (next & 63);
      }
      if (!valid || code > 0x10ffff) { output += '\ufffd'; continue; }
      if (code < 0x10000) output += String.fromCharCode(code);
      else {
        code -= 0x10000;
        output += String.fromCharCode(0xd800 + (code >>> 10), 0xdc00 + (code & 0x3ff));
      }
    }
    return output;
  };
  const writeBytes = (buffer, bytes, offset = 0, length = buffer.length - offset) => {
    const written = Math.min(bytes.length, length, buffer.length - offset);
    buffer.set(bytes.subarray(0, written), offset);
    return written;
  };
  const writeChars = (buffer, value, offset = 0, length = buffer.length - offset, mask = 0xff) => {
    const written = Math.min(value.length, length, buffer.length - offset);
    for (let index = 0; index < written; index++) {
      buffer[offset + index] = value.charCodeAt(index) & mask;
    }
    return written;
  };
  const sliceChars = (buffer, start = 0, end = buffer.length, mask = 0xff) => {
    let output = '';
    for (let index = start; index < Math.min(end, buffer.length); index++) {
      output += String.fromCharCode(buffer[index] & mask);
    }
    return output;
  };
  const hexSlice = (buffer, start = 0, end = buffer.length) => {
    let output = '';
    for (let index = start; index < Math.min(end, buffer.length); index++) {
      output += buffer[index].toString(16).padStart(2, '0');
    }
    return output;
  };
  const hexWrite = (buffer, value, offset = 0, length = buffer.length - offset) => {
    const written = Math.min(value.length >>> 1, length, buffer.length - offset);
    let index = 0;
    for (; index < written; index++) {
      const byte = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
      if (Number.isNaN(byte)) break;
      buffer[offset + index] = byte;
    }
    return index;
  };
  const base64Alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const base64Slice = (buffer, start = 0, end = buffer.length, url = false) => {
    let output = '';
    let index = start;
    for (; index + 2 < end; index += 3) {
      const value = (buffer[index] << 16) | (buffer[index + 1] << 8) | buffer[index + 2];
      output += base64Alphabet[(value >>> 18) & 63] + base64Alphabet[(value >>> 12) & 63] +
        base64Alphabet[(value >>> 6) & 63] + base64Alphabet[value & 63];
    }
    if (index < end) {
      const value = (buffer[index] << 16) | ((buffer[index + 1] ?? 0) << 8);
      output += base64Alphabet[(value >>> 18) & 63] + base64Alphabet[(value >>> 12) & 63];
      output += index + 1 < end ? base64Alphabet[(value >>> 6) & 63] + '=' : '==';
    }
    return url ? output.replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '') : output;
  };
  const bufferBinding = {
    kMaxLength: 0xffffffff,
    kStringMaxLength: 0x1fffffe8,
    byteLengthUtf8: (value) => utf8Encode(value).length,
    createUnsafeArrayBuffer: (size) => new ArrayBuffer(size),
    setDetachKey() {},
    utf8WriteStatic: (buffer, value, offset, length) =>
      writeBytes(buffer, utf8Encode(value), offset, length),
    asciiWriteStatic: (buffer, value, offset, length) =>
      writeChars(buffer, value, offset, length, 0x7f),
    latin1WriteStatic: (buffer, value, offset, length) =>
      writeChars(buffer, value, offset, length),
    utf8Slice: (buffer, start, end) => utf8Decode(buffer.subarray(start, end)),
    asciiSlice: (buffer, start, end) => sliceChars(buffer, start, end, 0x7f),
    latin1Slice: (buffer, start, end) => sliceChars(buffer, start, end),
    hexSlice,
    hexWrite,
    base64Slice: (buffer, start, end) => base64Slice(buffer, start, end),
    base64urlSlice: (buffer, start, end) => base64Slice(buffer, start, end, true),
    ucs2Slice: (buffer, start = 0, end = buffer.length) => {
      let output = '';
      for (let index = start; index + 1 < end; index += 2) {
        output += String.fromCharCode(buffer[index] | (buffer[index + 1] << 8));
      }
      return output;
    },
    ucs2Write(buffer, value, offset = 0, length = buffer.length - offset) {
      const chars = Math.min(value.length, Math.floor(length / 2));
      for (let index = 0; index < chars; index++) {
        const code = value.charCodeAt(index);
        buffer[offset + index * 2] = code & 0xff;
        buffer[offset + index * 2 + 1] = code >>> 8;
      }
      return chars * 2;
    },
    copy(source, target, targetStart, sourceStart, length) {
      target.set(source.subarray(sourceStart, sourceStart + length), targetStart);
      return length;
    },
    compare(left, right) {
      const length = Math.min(left.length, right.length);
      for (let index = 0; index < length; index++) {
        if (left[index] !== right[index]) return left[index] < right[index] ? -1 : 1;
      }
      return Math.sign(left.length - right.length);
    },
    fill(buffer, value, start, end) {
      const bytes = typeof value === 'string' ? utf8Encode(value) : value;
      if (!bytes?.length) return 0;
      for (let index = start; index < end; index++) buffer[index] = bytes[(index - start) % bytes.length];
      return 0;
    },
    isAscii: (input) => input.every((byte) => byte < 0x80),
    isUtf8: () => true,
  };
  const decoderState = new WeakMap();
  const stringDecoderBinding = {
    encodings: ['ascii', 'utf8', 'base64', 'utf16le', 'latin1', 'hex', 'buffer', 'base64url'],
    kIncompleteCharactersStart: 0,
    kIncompleteCharactersEnd: 4,
    kMissingBytes: 4,
    kBufferedBytes: 5,
    kEncodingField: 6,
    kNumFields: 7,
    kSize: 7,
    decode(state, input) {
      const encoding = state[6];
      if (encoding === 1) {
        let decoder = decoderState.get(state);
        if (!decoder) {
          decoder = new TextDecoder('utf-8', { fatal: false });
          decoderState.set(state, decoder);
        }
        return decoder.decode(input, { stream: true });
      }
      if (encoding === 0) return bufferBinding.asciiSlice(input, 0, input.length);
      if (encoding === 2) return bufferBinding.base64Slice(input, 0, input.length);
      if (encoding === 3) return bufferBinding.ucs2Slice(input, 0, input.length);
      if (encoding === 4) return bufferBinding.latin1Slice(input, 0, input.length);
      if (encoding === 5) return bufferBinding.hexSlice(input, 0, input.length);
      if (encoding === 7) return bufferBinding.base64urlSlice(input, 0, input.length);
      return bufferBinding.utf8Slice(input, 0, input.length);
    },
    flush(state) {
      const decoder = decoderState.get(state);
      decoderState.delete(state);
      return decoder ? decoder.decode() : '';
    },
  };
  const util = {
    constants: { ALL_PROPERTIES: 0, ONLY_WRITABLE: 1, ONLY_ENUMERABLE: 2, ONLY_CONFIGURABLE: 4,
      SKIP_STRINGS: 8, SKIP_SYMBOLS: 16, kPending: 0, kRejected: 1 },
    privateSymbols,
    getOwnNonIndexProperties(value, filter = 0) {
      const isArrayIndex = (property) => {
        if (typeof property !== 'string' || property === '') return false;
        const number = Number(property);
        return Number.isInteger(number) && number >= 0 && number < 0xffff_ffff && String(number) === property;
      };
      return Reflect.ownKeys(value).filter((property) => {
        if (isArrayIndex(property) || property === 'length') return false;
        if (typeof property === 'string' && (filter & 8) !== 0) return false;
        if (typeof property === 'symbol' && (filter & 16) !== 0) return false;
        const descriptor = Object.getOwnPropertyDescriptor(value, property);
        if (!descriptor) return false;
        if ((filter & 1) !== 0 && descriptor.writable !== true) return false;
        if ((filter & 2) !== 0 && descriptor.enumerable !== true) return false;
        if ((filter & 4) !== 0 && descriptor.configurable !== true) return false;
        return true;
      });
    },
    isInsideNodeModules: () => false,
    getPromiseDetails: () => undefined,
    getProxyDetails: () => undefined,
    previewEntries: () => [[], false],
    getConstructorName: (value) => value?.constructor?.name,
    getExternalValue: () => undefined,
    guessHandleType: () => 'UNKNOWN',
    sleep() {},
    getCallerLocation: () => undefined,
    getCallSites: (frameCount = 10) => Array.from({ length: frameCount }, () => ({
      functionName: '',
      scriptName: '<agentos>',
      lineNumber: 0,
      columnNumber: 0,
      isAsync: false,
      isEval: false,
      isNative: false,
      isConstructor: false,
    })),
    constructSharedArrayBuffer: (size) => new SharedArrayBuffer(size),
    defineLazyProperties(target, id, keys, enumerable = true) {
      for (const key of keys) Object.defineProperty(target, key, {
        configurable: true, enumerable, get: () => requireBuiltin(id)[key],
      });
      return target;
    },
  };
  class Serializer {
    writeHeader() {}
    writeValue() { return true; }
    writeUint32() {}
    writeUint64() {}
    writeDouble() {}
    writeRawBytes() {}
    transferArrayBuffer() {}
    _setTreatArrayBufferViewsAsHostObjects() {}
    releaseBuffer() { return new Uint8Array(0); }
  }
  class Deserializer {
    constructor(buffer = new Uint8Array(0)) { this.buffer = buffer; }
    readHeader() { return true; }
    readValue() { return undefined; }
    readUint32() { return 0; }
    readUint64() { return [0, 0]; }
    readDouble() { return 0; }
    transferArrayBuffer() {}
    _readRawBytes() { return 0; }
  }
  const numericConstants = (seed = {}) => new Proxy(seed, {
    get(target, property) { return Reflect.has(target, property) ? target[property] : 0; },
  });
  const constants = {
    fs: global._fsModule?.constants ?? numericConstants(),
    os: numericConstants(),
    crypto: numericConstants(),
    zlib: numericConstants({ BROTLI_PARAM_MODE: 0 }),
  };

  const UV_ERRNO = Object.freeze({
    EACCES: -13, EBADF: -9, EEXIST: -17, EINVAL: -22, EISDIR: -21,
    ENOENT: -2, ENOTDIR: -20, ENOTEMPTY: -39, EPERM: -1,
  });

  function statArray(value, bigint = false) {
    const milliseconds = (name) => Number(value[`${name}Ms`] ?? value[name]?.getTime?.() ?? 0);
    const time = (name) => {
      const ms = milliseconds(name);
      return [Math.trunc(ms / 1000), Math.trunc((ms % 1000) * 1_000_000)];
    };
    const fields = [
      value.dev ?? 0, value.mode ?? 0, value.nlink ?? 0, value.uid ?? 0,
      value.gid ?? 0, value.rdev ?? 0, value.blksize ?? 4096, value.ino ?? 0,
      value.size ?? 0, value.blocks ?? Math.ceil(Number(value.size ?? 0) / 512),
      ...time('atime'), ...time('mtime'), ...time('ctime'), ...time('birthtime'),
    ];
    return bigint
      ? BigInt64Array.from(fields, (entry) => BigInt(Math.trunc(Number(entry))))
      : Float64Array.from(fields, Number);
  }

  function statFsArray(value, bigint = false) {
    const fields = [
      value.type ?? 0, value.bsize ?? 4096, value.blocks ?? 0,
      value.bfree ?? 0, value.bavail ?? 0, value.files ?? 0, value.ffree ?? 0,
    ];
    return bigint
      ? BigInt64Array.from(fields, (entry) => BigInt(Math.trunc(Number(entry))))
      : Float64Array.from(fields, Number);
  }

  class FSReqCallback {
    constructor(bigint = false) {
      this.bigint = Boolean(bigint);
      this.oncomplete = null;
      this.context = undefined;
    }
  }
  const kUsePromises = Symbol('agentos.fs.promises');

  function wrapFsOperations(operations) {
    const binding = Object.create(null);
    for (const [name, operation] of Object.entries(operations)) {
      binding[name] = function agentOSFsBindingOperation(...args) {
        const requestIndex = args.findIndex(
          (value) => value === kUsePromises || value instanceof FSReqCallback,
        );
        if (requestIndex === -1) return operation(...args);
        const request = args[requestIndex];
        args[requestIndex] = undefined;
        if (request === kUsePromises) {
          return new Promise((resolve, reject) => queueMicrotask(() => {
            try { resolve(operation(...args)); } catch (error) { reject(error); }
          }));
        }
        queueMicrotask(() => {
          try {
            const result = operation(...args);
            if (result === undefined) request.oncomplete.call(request, null);
            else request.oncomplete.call(request, null, result);
          }
          catch (error) { request.oncomplete.call(request, error); }
        });
        return undefined;
      };
    }
    binding.FSReqCallback = FSReqCallback;
    binding.kUsePromises = kUsePromises;
    binding.statValues = new Float64Array(36);
    binding.bigintStatValues = new BigInt64Array(36);
    return binding;
  }

  function makeFsBinding() {
    const fs = global._fsModule;
    if (!fs) return Object.create(makeInert());
    const fdValue = (fd) => {
      if (typeof fd === 'number') return fd;
      let received;
      if (fd == null) received = ` Received ${fd}`;
      else if (typeof fd === 'object') received = ` Received an instance of ${fd.constructor?.name ?? 'Object'}`;
      else {
        const inspected = typeof fd === 'string' ? `'${fd}'` : String(fd);
        received = ` Received type ${typeof fd} (${inspected})`;
      }
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    };
    const operations = {
      access: (path, mode) => fs.accessSync(path, mode),
      chmod: (path, mode) => fs.chmodSync(path, mode),
      chown: (path, uid, gid) => fs.chownSync(path, uid, gid),
      close: (fd) => fs.closeSync(fdValue(fd)),
      copyFile: (source, destination, mode) => fs.copyFileSync(source, destination, mode),
      existsSync: (path) => fs.existsSync(path),
      fchmod: (fd, mode) => fs.fchmodSync(fdValue(fd), mode),
      fchown: (fd, uid, gid) => fs.fchownSync(fdValue(fd), uid, gid),
      fdatasync: (fd) => fs.fdatasyncSync(fdValue(fd)),
      fstat: (fd, bigint) => statArray(fs.fstatSync(fdValue(fd)), bigint),
      fsync: (fd) => fs.fsyncSync(fdValue(fd)),
      ftruncate: (fd, length) => fs.ftruncateSync(fdValue(fd), length),
      futimes: (fd, atime, mtime) => fs.futimesSync(fdValue(fd), atime, mtime),
      lchown: (path, uid, gid) => fs.lchownSync(path, uid, gid),
      link: (existingPath, newPath) => fs.linkSync(existingPath, newPath),
      lstat: (path, bigint, _request, throwIfNoEntry = true) => {
        try { return statArray(fs.lstatSync(path), bigint); }
        catch (error) { if (!throwIfNoEntry && error?.code === 'ENOENT') return undefined; throw error; }
      },
      lutimes: (path, atime, mtime) => fs.lutimesSync(path, atime, mtime),
      mkdir: (path, mode, recursive) => fs.mkdirSync(path, { mode, recursive: Boolean(recursive) }),
      mkdtemp: (prefix, encoding) => fs.mkdtempSync(prefix, { encoding: encoding || 'utf8' }),
      open: (path, flags, mode) => fs.openSync(path, flags, mode),
      read: (fd, buffer, offset, length, position) => fs.readSync(fdValue(fd), buffer, offset, length, position),
      readBuffers: (fd, buffers, position) => fs.readvSync(fdValue(fd), buffers, position),
      readFileUtf8: (pathOrFd, flags) => fs.readFileSync(pathOrFd, { encoding: 'utf8', flag: flags }),
      readdir: (path, encoding, withFileTypes) => {
        const entries = fs.readdirSync(path, { encoding: encoding || 'utf8', withFileTypes: true });
        const names = entries.map((entry) => entry.name);
        if (!withFileTypes) return names;
        return [names, entries.map((entry) => entry.isDirectory() ? 2 : entry.isSymbolicLink() ? 3 : 1)];
      },
      readlink: (path, encoding) => fs.readlinkSync(path, { encoding: encoding || 'utf8' }),
      realpath: (path, encoding) => fs.realpathSync(path, { encoding: encoding || 'utf8' }),
      rename: (oldPath, newPath) => fs.renameSync(oldPath, newPath),
      rmSync: (path, maxRetries, recursive, retryDelay) =>
        fs.rmSync(path, { force: true, maxRetries, recursive: Boolean(recursive), retryDelay }),
      rmdir: (path) => fs.rmdirSync(path),
      stat: (path, bigint, _request, throwIfNoEntry = true) => {
        try { return statArray(fs.statSync(path), bigint); }
        catch (error) { if (!throwIfNoEntry && error?.code === 'ENOENT') return undefined; throw error; }
      },
      statfs: (path, bigint) => statFsArray(fs.statfsSync(path), bigint),
      symlink: (target, path, type) => fs.symlinkSync(target, path, type),
      unlink: (path) => fs.unlinkSync(path),
      utimes: (path, atime, mtime) => fs.utimesSync(path, atime, mtime),
      writeBuffer: (fd, buffer, offset, length, position) =>
        fs.writeSync(fdValue(fd), buffer, offset, length, position),
      writeBuffers: (fd, buffers, position) => fs.writevSync(fdValue(fd), buffers, position),
      writeFileUtf8: (pathOrFd, data, flags, mode) =>
        fs.writeFileSync(pathOrFd, data, { encoding: 'utf8', flag: flags, mode }),
      writeString: (fd, value, position, encoding) => fs.writeSync(fdValue(fd), value, position, encoding),
      internalModuleStat(path) {
        try { return fs.statSync(path).isDirectory() ? 1 : 0; }
        catch (error) { return UV_ERRNO[error?.code] ?? -4094; }
      },
      internalModuleReadJSON(path) {
        try { return [fs.readFileSync(path, 'utf8'), false]; }
        catch { return []; }
      },
    };
    const binding = wrapFsOperations(operations);
    const close = binding.close;
    binding.close = (fd, request) => {
      fdValue(fd);
      return close(fd, request);
    };
    binding.openFileHandle = (path, flags, mode, request) => {
      if (request !== kUsePromises) throw new TypeError('openFileHandle requires kUsePromises');
      return binding.open(path, flags, mode, request).then((fd) => ({
        fd,
        close: () => binding.close(fd, kUsePromises),
        closeSync: () => operations.close(fd),
        getAsyncId: () => 0,
      }));
    };
    return binding;
  }

  function makeFsDirBinding() {
    const fs = global._fsModule;
    if (!fs) return Object.create(makeInert());
    const opendirSync = (path) => {
      const entries = fs.readdirSync(path, { withFileTypes: true });
      let offset = 0;
      let closed = false;
      return {
        read(_encoding, bufferSize, request) {
          const operation = () => {
            if (closed || offset >= entries.length) return null;
            const result = [];
            for (const entry of entries.slice(offset, offset + bufferSize)) {
              result.push(entry.name, entry.isDirectory() ? 2 : entry.isSymbolicLink() ? 3 : 1);
            }
            offset += bufferSize;
            return result;
          };
          if (!(request instanceof FSReqCallback)) return operation();
          queueMicrotask(() => {
            try { request.oncomplete.call(request, null, operation()); }
            catch (error) { request.oncomplete.call(request, error); }
          });
          return undefined;
        },
        close(request) {
          const operation = () => { closed = true; };
          if (!(request instanceof FSReqCallback)) return operation();
          queueMicrotask(() => { operation(); request.oncomplete.call(request, null); });
          return undefined;
        },
      };
    };
    return {
      opendirSync,
      opendir(path, _encoding, request) {
        queueMicrotask(() => {
          try { request.oncomplete.call(request, null, opendirSync(path)); }
          catch (error) { request.oncomplete.call(request, error); }
        });
      },
    };
  }

  const fsBinding = makeFsBinding();
  const fsDirBinding = makeFsDirBinding();

  function makeModulesBinding() {
    const fs = global._fsModule;
    const filePath = (value) => {
      const string = String(value);
      if (!string.startsWith('file:')) return string;
      return decodeURIComponent(new URL(string).pathname);
    };
    const dirname = (value) => {
      const normalized = filePath(value).replace(/\/+$/, '');
      const slash = normalized.lastIndexOf('/');
      return slash <= 0 ? '/' : normalized.slice(0, slash);
    };
    const serializePackage = (path) => {
      if (!fs?.existsSync(path)) return undefined;
      let data;
      try { data = JSON.parse(fs.readFileSync(path, 'utf8')); }
      catch { return undefined; }
      const serializeMap = (value) => {
        if (value === undefined) return null;
        return typeof value === 'string' ? value : JSON.stringify(value);
      };
      return [
        data.name ?? null,
        data.main ?? null,
        data.type ?? null,
        serializeMap(data.imports),
        serializeMap(data.exports),
        path,
      ];
    };
    const nearest = (value) => {
      let current = dirname(value);
      while (true) {
        const path = `${current === '/' ? '' : current}/package.json`;
        const packageData = serializePackage(path);
        if (packageData !== undefined) return packageData;
        if (current === '/') return undefined;
        current = dirname(current);
      }
    };
    return {
      readPackageJSON: (path) => serializePackage(filePath(path)),
      getNearestParentPackageJSON: nearest,
      getNearestParentPackageJSONType: (path) => nearest(path)?.[2] ?? 'none',
      getPackageScopeConfig: nearest,
      getPackageType: (url) => nearest(url)?.[2] ?? 'none',
      enableCompileCache: () => 0,
      getCompileCacheDir: () => undefined,
      compileCacheStatus: Object.freeze({ FAILED: 0, ENABLED: 1, ALREADY_ENABLED: 2, DISABLED: 3 }),
      flushCompileCache() {},
      getCompileCacheEntry: () => undefined,
      saveCompileCacheEntry() {},
      cachedCodeTypes: Object.freeze({
        kStrippedTypeScript: 0,
        kTransformedTypeScript: 1,
        kTransformedTypeScriptWithSourceMaps: 2,
      }),
      moduleFormats: Object.freeze(['builtin', 'commonjs', 'json', 'module', 'wasm']),
      setLazyPathHelpers() {},
    };
  }

  const modulesBinding = makeModulesBinding();

  const moduleWrapEvaluated = Symbol('module_wrap.kEvaluated');
  const concreteBindings = {
    builtins,
    module_wrap: {
      ModuleWrap,
      kEvaluated: moduleWrapEvaluated,
      createRequiredModuleFacade: (namespace) => namespace,
    },
    errors,
    config,
    options,
    symbols: perIsolateSymbols,
    types,
    buffer: bufferBinding,
    string_decoder: stringDecoderBinding,
    util,
    fs: fsBinding,
    fs_dir: fsDirBinding,
    uv: {
      getErrorMap: () => new Map(Object.entries(UV_ERRNO).map(([code, errno]) => [errno, [code, code]])),
      errname: (errno) => Object.entries(UV_ERRNO).find(([, value]) => value === errno)?.[0] ?? `UNKNOWN(${errno})`,
      ...Object.fromEntries(Object.entries(UV_ERRNO).map(([code, errno]) => [`UV_${code}`, errno])),
    },
    permission: { isEnabled: () => false, has: () => true },
    credentials: { safeGetenv: (name) => process.env[name], getTempDir: () => process.env.TMPDIR || '/tmp' },
    contextify: (() => {
      const contexts = new WeakSet();
      class ContextifyScript {
        constructor(source, filename = 'evalmachine.<anonymous>') {
          this.source = String(source);
          this.filename = String(filename);
          this.sourceMapURL = undefined;
          this.sourceURL = undefined;
        }
        runInContext() {
          return (0, eval)(`${this.source}\n//# sourceURL=${this.filename}`);
        }
        createCachedData() { return new Uint8Array(0); }
      }
      const compileFunctionForCJSLoader = (source, filename, isSeaMain, shouldDetectModule) =>
        typeof global._compileFunctionForCJSLoader === 'function'
          ? global._compileFunctionForCJSLoader(source, filename, isSeaMain, shouldDetectModule)
          : ({
              function: new Function(
                'exports', 'require', 'module', '__filename', '__dirname',
                `${source}\n//# sourceURL=${filename}`,
              ),
              sourceMapURL: undefined,
              sourceURL: undefined,
              canParseAsESM: false,
            });
      return {
      ContextifyScript,
      compileFunction: (source, _filename, _lineOffset, _columnOffset, _cachedData,
        _produceCachedData, _parsingContext, _contextExtensions, params = []) => ({
        function: new Function(...params, source),
      }),
      containsModuleSyntax: () => false,
      compileFunctionForCJSLoader,
      makeContext: (context) => {
        contexts.add(context);
        context[privateSymbols.contextify_context_private_symbol] = context;
        return context;
      },
      isContext: (context) => contexts.has(context),
      constants: Object.freeze({
        measureMemory: Object.freeze({
          mode: Object.freeze({ SUMMARY: 0, DETAILED: 1 }),
          execution: Object.freeze({ DEFAULT: 0, EAGER: 1 }),
        }),
      }),
      measureMemory: () => Promise.resolve({
        total: { jsMemoryEstimate: 0, jsMemoryRange: [0, 0] },
      }),
      };
    })(),
    modules: modulesBinding,
    serdes: { Serializer, Deserializer },
    constants,
    async_wrap: asyncWrap,
    task_queue: {
      tickInfo: new Uint8Array(2),
      promiseRejectEvents: {
        kPromiseRejectWithNoHandler: 0,
        kPromiseHandlerAddedAfterReject: 1,
        kPromiseRejectAfterResolved: 2,
        kPromiseResolveAfterResolved: 3,
      },
      runMicrotasks() {},
      enqueueMicrotask(callback) { Promise.resolve().then(callback); },
      setTickCallback() {},
      setPromiseRejectCallback() {},
    },
    mksnapshot: {
      setSerializeCallback() {},
      setDeserializeCallback() {},
      setDeserializeMainFunction() {},
      isBuildingSnapshotBuffer: new Uint8Array([0]),
    },
  };

  function getInternalBinding(name) {
    if (bindingCache.has(name)) return bindingCache.get(name);
    const binding = concreteBindings[name] ?? Object.create(makeInert());
    bindingCache.set(name, binding);
    return binding;
  }

  compileBuiltin('internal/bootstrap/realm', [
    'process', 'getLinkedBinding', 'getInternalBinding', 'primordials',
  ])(process, () => Object.create(makeInert()), getInternalBinding, primordials);

  if (typeof requireBuiltin !== 'function' || typeof internalBinding !== 'function') {
    const error = new Error('Pinned Node realm did not install its loaders');
    error.code = 'ERR_NODE_STDLIB_REALM_BOOTSTRAP';
    throw error;
  }

  const EventEmitter = requireBuiltin('events');
  if (typeof process.on !== 'function') {
    Object.setPrototypeOf(process, EventEmitter.prototype);
    EventEmitter.call(process);
  }
  requireBuiltin('internal/util/debuglog').initializeDebugEnv(process.env.NODE_DEBUG);
  requireBuiltin('internal/modules/cjs/loader').initializeCJS();

  const publicFs = requireBuiltin('fs');
  const upstreamReadFileSync = publicFs.readFileSync;
  const NodeBuffer = requireBuiltin('buffer').Buffer;
  publicFs.readFileSync = function agentOSReadFileSync(path, options) {
    const flag = options && typeof options === 'object' ? options.flag : undefined;
    const encoding = typeof options === 'string' ? options : options?.encoding;
    if (process.env.AGENTOS_BENCH_FS_READFILE_FAST_PATH === '0' ||
        typeof path !== 'string' || path.includes('\0') ||
        (flag !== undefined && flag !== 'r') || encoding != null) {
      return upstreamReadFileSync.call(this, path, options);
    }
    const bytes = global._fsReadFileRaw(path);
    return NodeBuffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  };

  const excluded = new Set(['quic', 'sqlite']);
  const publicIds = builtinIds.filter((id) => !id.startsWith('internal/') && !excluded.has(id));
  const eagerIds = ['events', 'buffer', 'util', 'stream', 'fs', 'path', 'timers'];
  const loadIds = global.__agentOSNodeLoadAll === true ? publicIds : eagerIds;
  const loaded = [];
  for (const id of loadIds) {
    try {
      requireBuiltin(id);
      loaded.push(id);
    } catch (cause) {
      const error = new Error(`Pinned Node public builtin failed inert load: ${id}: ${cause?.message ?? cause}`);
      error.code = 'ERR_NODE_STDLIB_INERT_LOAD';
      error.builtin = id;
      error.cause = cause;
      throw error;
    }
  }

  // Node's realm bootstrap must not replace the bridge's live process methods.
  // The builtin loader retains its closure over this process object, while the
  // guest-facing process surface is restored byte-for-byte after installation.
  for (const key of Reflect.ownKeys(process)) {
    if (!Object.prototype.hasOwnProperty.call(processDescriptors, key)) {
      Reflect.deleteProperty(process, key);
    }
  }
  Object.defineProperties(process, processDescriptors);
  Object.setPrototypeOf(process, processPrototype);
  if (errorPrepareStackTrace) {
    Object.defineProperty(Error, 'prepareStackTrace', errorPrepareStackTrace);
  } else {
    Reflect.deleteProperty(Error, 'prepareStackTrace');
  }
  for (const key of Reflect.ownKeys(global)) {
    if (!Object.prototype.hasOwnProperty.call(globalDescriptors, key)) {
      Reflect.deleteProperty(global, key);
    }
  }
  Object.defineProperties(global, globalDescriptors);

  Object.defineProperty(global, '__agentOSNodeStdlib', {
    configurable: false,
    enumerable: false,
    value: Object.freeze({
      requireBuiltin,
      internalBinding,
      primordials,
      publicIds: Object.freeze(publicIds),
      loaded: Object.freeze(loaded),
    }),
  });
  } catch (error) {
    if (error && typeof error === 'object' && !String(error.message).startsWith('[agentos-node-realm]')) {
      error.message = `[agentos-node-realm] ${error.message}`;
    }
    throw error;
  }
})(globalThis);
