'use strict';

// M0 bootstraps Node's real realm and compiles every public builtin while the
// native bindings are still inert. Later milestones replace these property-
// readable adapters with real HOST/BRIDGE/WASM providers one binding at a time.
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
  const util = {
    constants: { ALL_PROPERTIES: 0, ONLY_WRITABLE: 1, ONLY_ENUMERABLE: 2, ONLY_CONFIGURABLE: 4,
      SKIP_STRINGS: 8, SKIP_SYMBOLS: 16 },
    privateSymbols,
    getOwnNonIndexProperties: (value) => Object.getOwnPropertyNames(value),
    isInsideNodeModules: () => false,
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
    fs: numericConstants(),
    os: numericConstants(),
    crypto: numericConstants(),
    zlib: numericConstants({ BROTLI_PARAM_MODE: 0 }),
  };

  const concreteBindings = {
    builtins,
    module_wrap: { ModuleWrap },
    errors,
    config,
    options,
    symbols: perIsolateSymbols,
    types,
    buffer: bufferBinding,
    util,
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
  if (global.__agentOSNodeLoadAll === true) {
    requireBuiltin('internal/modules/cjs/loader').initializeCJS();
  }

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

  // M0's sanctioned dual-loader window must not let Node's realm bootstrap
  // replace the legacy bridge's live process methods. The real builtin loader
  // retains its closure over this process object, while the guest-facing
  // process surface is restored byte-for-byte for legacy execution.
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
