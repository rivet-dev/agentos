import { encodeChildProcessIpcFrame, splitChildProcessIpcFrames } from "./child-process.js";
import { UndiciHeaders, UndiciRequest, UndiciResponse } from "./undici.js";
import { BUFFER_CONSTANTS, BUFFER_MAX_LENGTH, BUFFER_MAX_STRING_LENGTH } from "./buffer-constants.js";
import { EventEmitter, once } from "./events.js";
import { _fs, _processCpuUsage, _processMemoryUsage, _processResourceUsage, _processUmask, _processVersions, decodeBridgeJson, normalizeModeArgument } from "./fs.js";
import { builtinPathStdlibModule } from "./builtin-modules.js";
import { fetch } from "./network.js";
import { getRuntimeGid, getRuntimeUid } from "./os.js";
import { URL2, installWhatwgUrlGlobals } from "./whatwg-url.js";
import { exposeCustomGlobal } from "../global-exposure.js";
import { CustomEvent, Event, EventTarget, TextDecoder, TextEncoder2 } from "../polyfills/index.js";
import { require_base64_js } from "../vendor/buffer.js";
import { Buffer3 } from "./buffer-runtime.js";
import { _stderr, _stdout, installBuiltinUtilFormatWithOptions } from "./console.js";
import { SandboxCrypto, SandboxCryptoKey, SandboxDOMException, SandboxSubtleCrypto, builtinCryptoModule } from "./crypto.js";
import { installSafeIntlFormatters } from "./misc-stubs.js";
import { _stdin, _stdinListeners, _stdinOnceListeners, resetLiveStdinState, setStdinDataValue, setStdinEnded, setStdinFlowMode, setStdinPosition, stdinDispatch, syncLiveStdinHandle } from "./stdin.js";
import { _nextTickQueue, _queueMicrotask, clearImmediate, clearInterval, clearTimeout2, runWithAsyncLocalStorageSnapshot, scheduleNextTickFlush, setImmediate, setInterval, setTimeout2, snapshotAsyncLocalStorageStores, wrapAsyncLocalStorageCallback } from "./timers.js";
import { _resolveRuntimeTtyConfig } from "./tty-config.js";

function readProcessConfig() {
  return {
    platform: typeof _processConfig !== "undefined" && _processConfig.platform || "linux",
    arch: typeof _processConfig !== "undefined" && _processConfig.arch || "x64",
    version: typeof _processConfig !== "undefined" && _processConfig.version || "v22.0.0",
    cwd: typeof _processConfig !== "undefined" && _processConfig.cwd || "/root",
    env: typeof _processConfig !== "undefined" && _processConfig.env || {},
    argv: typeof _processConfig !== "undefined" && _processConfig.argv || [
      "node",
      "script.js"
    ],
    argv0: typeof _processConfig !== "undefined" && typeof _processConfig.argv0 === "string"
      ? _processConfig.argv0
      : "node",
    execPath: typeof _processConfig !== "undefined" && _processConfig.execPath || "/usr/bin/node",
    pid: typeof _processConfig !== "undefined" && _processConfig.pid || 1,
    ppid: typeof _processConfig !== "undefined" && _processConfig.ppid || 0,
    uid: typeof _processConfig !== "undefined" && _processConfig.uid || 0,
    gid: typeof _processConfig !== "undefined" && _processConfig.gid || 0,
    stdin: typeof _processConfig !== "undefined" ? _processConfig.stdin : void 0,
    timingMitigation: typeof _processConfig !== "undefined" && _processConfig.timingMitigation || "off",
    frozenTimeMs: typeof _processConfig !== "undefined" ? _processConfig.frozenTimeMs : void 0,
    highResolutionTime: typeof _processConfig !== "undefined" && _processConfig.high_resolution_time === true
  };
}

var config2 = readProcessConfig();

var processClockFallbackNow = typeof performance !== "undefined" && performance && typeof performance.now === "function" ? performance.now.bind(performance) : Date.now;

var processClockNow = () => {
  if (typeof __secureExecHrNowUs === "function") {
    return __secureExecHrNowUs() / 1000;
  }
  return processClockFallbackNow();
};

function getNowMs() {
  if (config2.timingMitigation === "freeze" && typeof config2.frozenTimeMs === "number") {
    return config2.frozenTimeMs;
  }
  return processClockNow();
}

var _processStartTime = getNowMs();

var _exitCode = void 0;

var _exited = false;

var ProcessExitError = class extends Error {
  code;
  _isProcessExit;
  constructor(code) {
    super("process.exit(" + code + ")");
    this.name = "ProcessExitError";
    this.code = code;
    this._isProcessExit = true;
  }
};

exposeCustomGlobal("ProcessExitError", ProcessExitError);

var _signalNumbers = {
  SIGHUP: 1,
  SIGINT: 2,
  SIGQUIT: 3,
  SIGILL: 4,
  SIGTRAP: 5,
  SIGABRT: 6,
  SIGBUS: 7,
  SIGFPE: 8,
  SIGKILL: 9,
  SIGUSR1: 10,
  SIGSEGV: 11,
  SIGUSR2: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM: 15,
  SIGCHLD: 17,
  SIGCONT: 18,
  SIGSTOP: 19,
  SIGTSTP: 20,
  SIGTTIN: 21,
  SIGTTOU: 22,
  SIGURG: 23,
  SIGXCPU: 24,
  SIGXFSZ: 25,
  SIGVTALRM: 26,
  SIGPROF: 27,
  SIGWINCH: 28,
  SIGIO: 29,
  SIGPWR: 30,
  SIGSYS: 31
};

var _signalNamesByNumber = Object.fromEntries(
  Object.entries(_signalNumbers).map(([name, num]) => [num, name])
);

var _ignoredSelfSignals = /* @__PURE__ */ new Set(["SIGWINCH", "SIGCHLD", "SIGCONT", "SIGURG"]);

var _trackedProcessSignalEvents = /* @__PURE__ */ new Set([
  "SIGHUP",
  "SIGINT",
  "SIGUSR1",
  "SIGALRM",
  "SIGTERM",
  "SIGCHLD",
  "SIGCONT",
  "SIGWINCH"
]);

function _resolveSignal(signal) {
  if (signal === void 0 || signal === null) return 15;
  if (typeof signal === "number") return signal;
  const num = _signalNumbers[signal];
  if (num !== void 0) return num;
  throw new Error("Unknown signal: " + signal);
}

function _isTrackedProcessSignalEventName(eventName) {
  return typeof eventName === "string" && _trackedProcessSignalEvents.has(eventName);
}

var _processKillErrnoByCode = { ESRCH: 3, EPERM: 1, EINVAL: 22 };

function _createProcessKillError(error) {
  const message = String((error && error.message) || error || "");
  let code = null;
  if (error && typeof error.code === "string" && Object.prototype.hasOwnProperty.call(_processKillErrnoByCode, error.code)) {
    code = error.code;
  } else if (/\bESRCH\b/.test(message)) {
    code = "ESRCH";
  } else if (/\bEINVAL\b/.test(message)) {
    code = "EINVAL";
  } else if (/\bEPERM\b/.test(message) || /permission denied/i.test(message)) {
    code = "EPERM";
  }
  if (code === null) {
    return error instanceof Error ? error : new Error(message);
  }
  const err = new Error(`kill ${code}`);
  err.code = code;
  err.errno = -_processKillErrnoByCode[code];
  err.syscall = "kill";
  return err;
}

var _processListeners = {};

var _processOnceListeners = {};

var _processMaxListeners = 10;

var _processMaxListenersWarned = /* @__PURE__ */ new Set();

function _listenerCountForEvent(event) {
  return (_processListeners[event] || []).length + (_processOnceListeners[event] || []).length;
}

function _syncGuestProcessSignalState(eventName) {
  if (!_isTrackedProcessSignalEventName(eventName) || typeof _processSignalState === "undefined") {
    return;
  }
  const signal = _signalNumbers[eventName];
  if (typeof signal !== "number") {
    return;
  }
  const action = _listenerCountForEvent(eventName) > 0 ? "user" : "default";
  try {
    _processSignalState.applySyncPromise(void 0, [signal, action, JSON.stringify([]), 0]);
  } catch {
  }
}

function _syncAllGuestProcessSignalStates() {
  for (const eventName of _trackedProcessSignalEvents) {
    _syncGuestProcessSignalState(eventName);
  }
}

function _deliverProcessSignal(signal, action = "default") {
  const sigNum = _resolveSignal(signal);
  if (sigNum === 0) {
    return true;
  }
  const sigName = _signalNamesByNumber[sigNum] ?? `SIG${sigNum}`;
  if (action === "ignore") {
    return true;
  }
  if (_emit(sigName, sigName)) {
    return true;
  }
  if (_ignoredSelfSignals.has(sigName)) {
    return true;
  }
  return process2.exit(128 + sigNum);
}

function signalDispatch(eventType, payload) {
  if (eventType !== "signal" || payload === null || typeof payload !== "object") {
    return;
  }
  const signal = payload.signal ?? payload.number;
  const action = typeof payload.action === "string" ? payload.action : "default";
  _deliverProcessSignal(signal, action);
}

function _addListener(event, listener, once = false) {
  const target = once ? _processOnceListeners : _processListeners;
  if (!target[event]) {
    target[event] = [];
  }
  target[event].push(listener);
  if (_processMaxListeners > 0 && !_processMaxListenersWarned.has(event)) {
    const total = (_processListeners[event]?.length ?? 0) + (_processOnceListeners[event]?.length ?? 0);
    if (total > _processMaxListeners) {
      _processMaxListenersWarned.add(event);
      const warning = `MaxListenersExceededWarning: Possible EventEmitter memory leak detected. ${total} ${event} listeners added to [process]. MaxListeners is ${_processMaxListeners}. Use emitter.setMaxListeners() to increase limit`;
      if (typeof _error !== "undefined") {
        _error.applySync(void 0, [warning]);
      }
    }
  }
  _syncGuestProcessSignalState(event);
  return process2;
}

function _removeListener(event, listener) {
  if (_processListeners[event]) {
    const idx = _processListeners[event].indexOf(listener);
    if (idx !== -1) _processListeners[event].splice(idx, 1);
  }
  if (_processOnceListeners[event]) {
    const idx = _processOnceListeners[event].indexOf(listener);
    if (idx !== -1) _processOnceListeners[event].splice(idx, 1);
  }
  _syncGuestProcessSignalState(event);
  return process2;
}

function _emit(event, ...args) {
  let handled = false;
  if (_processListeners[event]) {
    for (const listener of _processListeners[event]) {
      listener.call(process2, ...args);
      handled = true;
    }
  }
  if (_processOnceListeners[event]) {
    const listeners = _processOnceListeners[event].slice();
    _processOnceListeners[event] = [];
    for (const listener of listeners) {
      listener.call(process2, ...args);
      handled = true;
    }
  }
  return handled;
}

function isProcessExitError(error) {
  return Boolean(
    error && typeof error === "object" && (error._isProcessExit === true || error.name === "ProcessExitError")
  );
}

function normalizeAsyncError(error) {
  return error instanceof Error ? error : new Error(String(error));
}

function routeAsyncCallbackError(error) {
  if (isProcessExitError(error)) {
    return { handled: false, rethrow: error };
  }
  const normalized = normalizeAsyncError(error);
  try {
    if (_emit("uncaughtException", normalized, "uncaughtException")) {
      return { handled: true, rethrow: null };
    }
  } catch (emitError) {
    return { handled: false, rethrow: emitError };
  }
  return { handled: false, rethrow: normalized };
}

function scheduleAsyncRethrow(error) {
  setTimeout2(() => {
    throw error;
  }, 0);
}

function dispatchCustomEmitterListeners(thisArg, listeners, args) {
  if (!listeners || listeners.length === 0) {
    return false;
  }
  for (const listener of listeners.slice()) {
    try {
      listener.call(thisArg, ...args);
    } catch (error) {
      const outcome = routeAsyncCallbackError(error);
      if (!outcome.handled && outcome.rethrow !== null) {
        throw outcome.rethrow;
      }
      return true;
    }
  }
  return true;
}

function _getStdinIsTTY() {
  return _resolveRuntimeTtyConfig().stdinIsTTY;
}

exposeCustomGlobal("_stdinDispatch", stdinDispatch);

exposeCustomGlobal("_signalDispatch", signalDispatch);

function hrtime(prev) {
  const now = getNowMs();
  const seconds = Math.floor(now / 1e3);
  const nanoseconds = Math.floor(now % 1e3 * 1e6);
  if (prev) {
    let diffSec = seconds - prev[0];
    let diffNano = nanoseconds - prev[1];
    if (diffNano < 0) {
      diffSec -= 1;
      diffNano += 1e9;
    }
    return [diffSec, diffNano];
  }
  return [seconds, nanoseconds];
}

hrtime.bigint = function() {
  const now = getNowMs();
  return BigInt(Math.floor(now * 1e6));
};

var _cwd = config2.cwd;

var _umask = 18;

var _processVersionsCache = {
  node: config2.version.replace(/^v/, ""),
  v8: "11.3.244.8",
  uv: "1.44.2",
  zlib: "1.2.13",
  brotli: "1.0.9",
  ares: "1.19.0",
  modules: "108",
  nghttp2: "1.52.0",
  napi: "8",
  llhttp: "8.1.0",
  openssl: "3.0.8",
  cldr: "42.0",
  icu: "72.1",
  tz: "2022g",
  unicode: "15.0"
};

function defaultProcessMemoryUsage() {
  return {
    rss: 50 * 1024 * 1024,
    heapTotal: 20 * 1024 * 1024,
    heapUsed: 10 * 1024 * 1024,
    external: 1 * 1024 * 1024,
    arrayBuffers: 500 * 1024
  };
}

function readLiveProcessMemoryUsage() {
  const fallback = defaultProcessMemoryUsage();
  const usage = _processMemoryUsage.applySyncPromise(void 0, []);
  if (!usage || typeof usage !== "object") {
    return fallback;
  }
  return {
    rss: Number.isFinite(usage.rss) ? Number(usage.rss) : fallback.rss,
    heapTotal: Number.isFinite(usage.heapTotal) ? Number(usage.heapTotal) : fallback.heapTotal,
    heapUsed: Number.isFinite(usage.heapUsed) ? Number(usage.heapUsed) : fallback.heapUsed,
    external: Number.isFinite(usage.external) ? Number(usage.external) : fallback.external,
    arrayBuffers: Number.isFinite(usage.arrayBuffers) ? Number(usage.arrayBuffers) : fallback.arrayBuffers
  };
}

function readLiveProcessCpuUsage(prev) {
  const usage = _processCpuUsage.applySyncPromise(void 0, [prev ?? null]);
  if (usage && typeof usage === "object") {
    return {
      user: Number.isFinite(usage.user) ? Number(usage.user) : 1e6,
      system: Number.isFinite(usage.system) ? Number(usage.system) : 5e5
    };
  }
  const fallback = {
    user: 1e6,
    system: 5e5
  };
  if (prev && typeof prev === "object") {
    return {
      user: fallback.user - Number(prev.user || 0),
      system: fallback.system - Number(prev.system || 0)
    };
  }
  return fallback;
}

function defaultProcessResourceUsage() {
  return {
    userCPUTime: 1e6,
    systemCPUTime: 5e5,
    maxRSS: 50 * 1024,
    sharedMemorySize: 0,
    unsharedDataSize: 0,
    unsharedStackSize: 0,
    minorPageFault: 0,
    majorPageFault: 0,
    swappedOut: 0,
    fsRead: 0,
    fsWrite: 0,
    ipcSent: 0,
    ipcReceived: 0,
    signalsCount: 0,
    voluntaryContextSwitches: 0,
    involuntaryContextSwitches: 0
  };
}

function readLiveProcessResourceUsage() {
  const fallback = defaultProcessResourceUsage();
  const usage = _processResourceUsage.applySyncPromise(void 0, []);
  if (!usage || typeof usage !== "object") {
    return fallback;
  }
  return {
    userCPUTime: Number.isFinite(usage.userCPUTime) ? Number(usage.userCPUTime) : fallback.userCPUTime,
    systemCPUTime: Number.isFinite(usage.systemCPUTime) ? Number(usage.systemCPUTime) : fallback.systemCPUTime,
    maxRSS: Number.isFinite(usage.maxRSS) ? Number(usage.maxRSS) : fallback.maxRSS,
    sharedMemorySize: Number.isFinite(usage.sharedMemorySize) ? Number(usage.sharedMemorySize) : fallback.sharedMemorySize,
    unsharedDataSize: Number.isFinite(usage.unsharedDataSize) ? Number(usage.unsharedDataSize) : fallback.unsharedDataSize,
    unsharedStackSize: Number.isFinite(usage.unsharedStackSize) ? Number(usage.unsharedStackSize) : fallback.unsharedStackSize,
    minorPageFault: Number.isFinite(usage.minorPageFault) ? Number(usage.minorPageFault) : fallback.minorPageFault,
    majorPageFault: Number.isFinite(usage.majorPageFault) ? Number(usage.majorPageFault) : fallback.majorPageFault,
    swappedOut: Number.isFinite(usage.swappedOut) ? Number(usage.swappedOut) : fallback.swappedOut,
    fsRead: Number.isFinite(usage.fsRead) ? Number(usage.fsRead) : fallback.fsRead,
    fsWrite: Number.isFinite(usage.fsWrite) ? Number(usage.fsWrite) : fallback.fsWrite,
    ipcSent: Number.isFinite(usage.ipcSent) ? Number(usage.ipcSent) : fallback.ipcSent,
    ipcReceived: Number.isFinite(usage.ipcReceived) ? Number(usage.ipcReceived) : fallback.ipcReceived,
    signalsCount: Number.isFinite(usage.signalsCount) ? Number(usage.signalsCount) : fallback.signalsCount,
    voluntaryContextSwitches: Number.isFinite(usage.voluntaryContextSwitches) ? Number(usage.voluntaryContextSwitches) : fallback.voluntaryContextSwitches,
    involuntaryContextSwitches: Number.isFinite(usage.involuntaryContextSwitches) ? Number(usage.involuntaryContextSwitches) : fallback.involuntaryContextSwitches
  };
}

function readLiveProcessVersions() {
  _processVersionsCache.node = config2.version.replace(/^v/, "");
  const versions = _processVersions.applySyncPromise(void 0, []);
  if (versions && typeof versions === "object") {
    Object.assign(_processVersionsCache, versions);
    _processVersionsCache.node = config2.version.replace(/^v/, "");
  }
  return _processVersionsCache;
}

var process2 = {
  // Static properties
  platform: config2.platform,
  arch: config2.arch,
  version: config2.version,
  get versions() {
    return readLiveProcessVersions();
  },
  pid: config2.pid,
  ppid: config2.ppid,
  execPath: config2.execPath,
  execArgv: [],
  argv: config2.argv,
  argv0: config2.argv0,
  title: "node",
  env: config2.env,
  // Config stubs
  config: {
    target_defaults: {
      cflags: [],
      default_configuration: "Release",
      defines: [],
      include_dirs: [],
      libraries: []
    },
    variables: {
      node_prefix: "/usr",
      node_shared_libuv: false
    }
  },
  release: {
    name: "node",
    sourceUrl: "https://nodejs.org/download/release/v20.0.0/node-v20.0.0.tar.gz",
    headersUrl: "https://nodejs.org/download/release/v20.0.0/node-v20.0.0-headers.tar.gz"
  },
  // Feature flags
  features: {
    inspector: false,
    debug: false,
    uv: true,
    ipv6: true,
    tls_alpn: true,
    tls_sni: true,
    tls_ocsp: true,
    tls: true
  },
  // Methods
  cwd() {
    return _cwd;
  },
  chdir(dir) {
    let statJson;
    try {
      statJson = _fs.stat.applySyncPromise(void 0, [dir]);
    } catch {
      const err = new Error(`ENOENT: no such file or directory, chdir '${dir}'`);
      err.code = "ENOENT";
      err.errno = -2;
      err.syscall = "chdir";
      err.path = dir;
      throw err;
    }
    const parsed = decodeBridgeJson(statJson);
    if (!parsed.isDirectory) {
      const err = new Error(`ENOTDIR: not a directory, chdir '${dir}'`);
      err.code = "ENOTDIR";
      err.errno = -20;
      err.syscall = "chdir";
      err.path = dir;
      throw err;
    }
    _cwd = dir;
  },
  get exitCode() {
    return _exitCode;
  },
  set exitCode(code) {
    _exitCode = code == null ? void 0 : code;
  },
  exit(code) {
    const exitCode = code !== void 0 ? code : _exitCode ?? 0;
    _exitCode = exitCode;
    _exited = true;
    try {
      _emit("exit", exitCode);
    } catch (_e) {
    }
    throw new ProcessExitError(exitCode);
  },
  abort() {
    return process2.kill(process2.pid, "SIGABRT");
  },
  nextTick(callback, ...args) {
    const asyncLocalStorageSnapshot = snapshotAsyncLocalStorageStores();
    _nextTickQueue.push({
      callback: wrapAsyncLocalStorageCallback(callback, asyncLocalStorageSnapshot),
      args
    });
    scheduleNextTickFlush();
  },
  hrtime,
  getuid() {
    return getRuntimeUid();
  },
  getgid() {
    return getRuntimeGid();
  },
  geteuid() {
    const value = globalThis.process?.euid;
    return Number.isFinite(value) ? value : getRuntimeUid();
  },
  getegid() {
    const value = globalThis.process?.egid;
    return Number.isFinite(value) ? value : getRuntimeGid();
  },
  getgroups() {
    return Array.isArray(globalThis.process?.groups) && globalThis.process.groups.length > 0 ? [...globalThis.process.groups] : [getRuntimeGid()];
  },
  setuid() {
  },
  setgid() {
  },
  seteuid() {
  },
  setegid() {
  },
  setgroups() {
  },
  umask(mask) {
    const normalizedMask = mask === void 0 ? void 0 : normalizeModeArgument(mask, "mask");
    const previousMask = Number(_processUmask.applySyncPromise(void 0, [normalizedMask ?? null]));
    if (Number.isFinite(previousMask)) {
      _umask = normalizedMask ?? previousMask;
      return previousMask;
    }
    const oldMask = _umask;
    if (normalizedMask !== void 0) {
      _umask = normalizedMask;
    }
    return oldMask;
  },
  uptime() {
    return (getNowMs() - _processStartTime) / 1e3;
  },
  memoryUsage() {
    return readLiveProcessMemoryUsage();
  },
  cpuUsage(prev) {
    return readLiveProcessCpuUsage(prev);
  },
  resourceUsage() {
    return readLiveProcessResourceUsage();
  },
  kill(pid, signal) {
    if (typeof pid !== "number" || !Number.isFinite(pid) || !Number.isInteger(pid)) {
      throw new TypeError(`The "pid" argument must be an integer. Received ${String(pid)}`);
    }
    const sigNum = _resolveSignal(signal);
    const sigName = _signalNamesByNumber[sigNum] ?? `SIG${sigNum}`;
    if (typeof _processKill !== "undefined") {
      let rawResult;
      try {
        rawResult = _processKill.applySyncPromise(void 0, [pid, sigName]);
      } catch (error) {
        throw _createProcessKillError(error);
      }
      let result = rawResult;
      if (typeof result === "string") {
        try {
          result = JSON.parse(result);
        } catch {
          result = null;
        }
      }
      if (result && typeof result === "object" && result.self === true) {
        const action = typeof result.action === "string" ? result.action : "default";
        return _deliverProcessSignal(sigNum, action);
      }
      return true;
    }
    if (pid !== process2.pid) {
      const err = new Error("Operation not permitted");
      err.code = "EPERM";
      err.errno = -1;
      err.syscall = "kill";
      throw err;
    }
    return _deliverProcessSignal(sigNum, "default");
  },
  // EventEmitter methods
  on(event, listener) {
    return _addListener(event, listener);
  },
  once(event, listener) {
    return _addListener(event, listener, true);
  },
  removeListener(event, listener) {
    return _removeListener(event, listener);
  },
  // off is an alias for removeListener (assigned below to be same reference)
  off: null,
  removeAllListeners(event) {
    if (event) {
      delete _processListeners[event];
      delete _processOnceListeners[event];
      _syncGuestProcessSignalState(event);
    } else {
      Object.keys(_processListeners).forEach((k) => delete _processListeners[k]);
      Object.keys(_processOnceListeners).forEach(
        (k) => delete _processOnceListeners[k]
      );
      _syncAllGuestProcessSignalStates();
    }
    return process2;
  },
  addListener(event, listener) {
    return _addListener(event, listener);
  },
  emit(event, ...args) {
    return _emit(event, ...args);
  },
  listeners(event) {
    return [
      ..._processListeners[event] || [],
      ..._processOnceListeners[event] || []
    ];
  },
  listenerCount(event) {
    return _listenerCountForEvent(event);
  },
  prependListener(event, listener) {
    if (!_processListeners[event]) {
      _processListeners[event] = [];
    }
    _processListeners[event].unshift(listener);
    _syncGuestProcessSignalState(event);
    return process2;
  },
  prependOnceListener(event, listener) {
    if (!_processOnceListeners[event]) {
      _processOnceListeners[event] = [];
    }
    _processOnceListeners[event].unshift(listener);
    _syncGuestProcessSignalState(event);
    return process2;
  },
  eventNames() {
    return [
      .../* @__PURE__ */ new Set([
        ...Object.keys(_processListeners),
        ...Object.keys(_processOnceListeners)
      ])
    ];
  },
  setMaxListeners(n) {
    _processMaxListeners = n;
    return process2;
  },
  getMaxListeners() {
    return _processMaxListeners;
  },
  rawListeners(event) {
    return process2.listeners(event);
  },
  // Stdio streams
  stdout: _stdout,
  stderr: _stderr,
  stdin: _stdin,
  // Process state
  connected: config2.env?.AGENTOS_NODE_IPC === "1",
  // Module info (will be set by createRequire)
  mainModule: void 0,
  // No-op methods for compatibility
  emitWarning(warning) {
    if (warning && typeof warning === "object") {
      if (typeof warning.message !== "string") {
        warning.message = String(warning.message ?? "");
      }
      if (typeof warning.name !== "string" || warning.name.length === 0) {
        warning.name = "Warning";
      }
      _emit("warning", warning);
      return;
    }
    _emit("warning", {
      message: String(warning ?? ""),
      name: "Warning"
    });
  },
  binding(_name) {
    const error = new Error("process.binding is not supported in sandbox");
    error.code = "ERR_ACCESS_DENIED";
    throw error;
  },
  _linkedBinding(_name) {
    const error = new Error("process._linkedBinding is not supported in sandbox");
    error.code = "ERR_ACCESS_DENIED";
    throw error;
  },
  dlopen() {
    throw new Error("process.dlopen is not supported");
  },
  hasUncaughtExceptionCaptureCallback() {
    return false;
  },
  setUncaughtExceptionCaptureCallback() {
  },
  send(message, sendHandleOrOptions, optionsOrCallback, maybeCallback) {
    const callback = typeof sendHandleOrOptions === "function" ? sendHandleOrOptions : typeof optionsOrCallback === "function" ? optionsOrCallback : maybeCallback;
    if (!process2.connected) {
      return false;
    }
    try {
      process2.stdout.write(encodeChildProcessIpcFrame(message));
      if (callback) {
        queueMicrotask(() => callback(null));
      }
      return true;
    } catch (error) {
      if (callback) {
        queueMicrotask(() => callback(error));
        return false;
      }
      throw error;
    }
  },
  disconnect() {
    if (!process2.connected) {
      return;
    }
    process2.connected = false;
    if (process2._agentOSIpcHandleId && typeof _unregisterHandle === "function") {
      _unregisterHandle(process2._agentOSIpcHandleId);
      process2._agentOSIpcHandleId = null;
    }
    _emit("disconnect");
  },
  // Report
  report: {
    directory: "",
    filename: "",
    compact: false,
    signal: "SIGUSR2",
    reportOnFatalError: false,
    reportOnSignal: false,
    reportOnUncaughtException: false,
    getReport() {
      return {};
    },
    writeReport() {
      return "";
    }
  },
  // Debug port
  debugPort: 9229,
  // Internal state
  _cwd: config2.cwd,
  _umask: 18
};

function installProcessIpcBridge() {
  const ipcEnabled = config2.env?.AGENTOS_NODE_IPC === "1" || globalThis.__agentOSProcessConfigEnv?.AGENTOS_NODE_IPC === "1";
  if (!ipcEnabled || process2._agentOSIpcInstalled) {
    return;
  }
  process2._agentOSIpcInstalled = true;
  process2.connected = true;
  if (!process2._agentOSIpcHandleId && typeof _registerHandle === "function") {
    process2._agentOSIpcHandleId = `process-ipc:${process2.pid}`;
    _registerHandle(process2._agentOSIpcHandleId, "child_process IPC channel");
  }
  let ipcInputBuffer = "";
  process2.stdin.on("data", (chunk) => {
    const parsed = splitChildProcessIpcFrames(ipcInputBuffer, chunk);
    ipcInputBuffer = parsed.buffer;
    for (const message of parsed.messages) {
      _emit("message", message, void 0);
    }
  });
}

function applyProcessConfig(nextConfig) {
  syncLiveStdinHandle(false);
  resetLiveStdinState(new TextDecoder());
  for (const key of Object.keys(_stdinListeners)) {
    _stdinListeners[key] = [];
  }
  for (const key of Object.keys(_stdinOnceListeners)) {
    _stdinOnceListeners[key] = [];
  }
  setStdinDataValue(nextConfig.stdin ?? "");
  setStdinPosition(0);
  setStdinEnded(false);
  setStdinFlowMode(false);
  config2 = nextConfig;
  _cwd = nextConfig.cwd;
  process2.platform = nextConfig.platform;
  process2.arch = nextConfig.arch;
  process2.version = nextConfig.version;
  process2.pid = nextConfig.pid;
  process2.ppid = nextConfig.ppid;
  process2.execPath = nextConfig.execPath;
  process2.argv = nextConfig.argv;
  process2.argv0 = nextConfig.argv0;
  process2.env = nextConfig.env;
  process2.connected = nextConfig.env?.AGENTOS_NODE_IPC === "1";
  process2._cwd = nextConfig.cwd;
  process2.stdin.paused = true;
  process2.stdin.encoding = null;
  process2.stdin.isRaw = false;
  _processVersionsCache.node = nextConfig.version.replace(/^v/, "");
}

exposeCustomGlobal("__runtimeRefreshProcessConfig", () => {
  applyProcessConfig(readProcessConfig());
});

process2.off = process2.removeListener;

exposeCustomGlobal("__runtimeInstallProcessIpcBridge", installProcessIpcBridge);

installProcessIpcBridge();

process2.memoryUsage.rss = function() {
  return readLiveProcessMemoryUsage().rss;
};

Object.defineProperty(process2, Symbol.toStringTag, {
  value: "process",
  writable: false,
  configurable: true,
  enumerable: false
});

var process_default = process2;

function encodeFilePathSegment(value) {
  return encodeURIComponent(String(value)).replace(/%2F/g, "/");
}

function pathToFileURL2(filePath) {
  const normalized = builtinPathStdlibModule.posix.resolve(String(filePath || "/"));
  const pathname = encodeFilePathSegment(normalized);
  return new URL2(`file://${pathname.startsWith("/") ? pathname : `/${pathname}`}`);
}

function fileURLToPath2(input) {
  const href = input instanceof URL2 ? input.href : String(input ?? "");
  if (!href.startsWith("file:")) {
    throw new TypeError("The URL must be of scheme file");
  }
  let pathname = href.slice("file:".length);
  if (pathname.startsWith("//")) {
    const authorityMatch = /^\/\/[^/]*(.*)$/.exec(pathname);
    pathname = authorityMatch?.[1] || "/";
  }
  pathname = pathname.split(/[?#]/, 1)[0] || "/";
  pathname = decodeURIComponent(pathname);
  if (!pathname.startsWith("/")) {
    pathname = `/${pathname}`;
  }
  return pathname;
}

function setupGlobals() {
  const g = globalThis;
  g.process = process2;
  g.setTimeout = setTimeout2;
  g.clearTimeout = clearTimeout2;
  g.setInterval = setInterval;
  g.clearInterval = clearInterval;
  g.setImmediate = setImmediate;
  g.clearImmediate = clearImmediate;
  const nativeQueueMicrotask = typeof g.queueMicrotask === "function" ? g.queueMicrotask.bind(g) : _queueMicrotask;
  g.queueMicrotask = (callback) => {
    const asyncLocalStorageSnapshot = snapshotAsyncLocalStorageStores();
    return nativeQueueMicrotask(() =>
      runWithAsyncLocalStorageSnapshot(
        asyncLocalStorageSnapshot,
        callback,
        g,
        []
      )
    );
  };
  installWhatwgUrlGlobals(g);
  g.TextEncoder = TextEncoder2;
  g.TextDecoder = TextDecoder;
  g.Event = Event;
  g.CustomEvent = CustomEvent;
  g.EventTarget = EventTarget;
  if (typeof g.Buffer === "undefined") {
    g.Buffer = Buffer3;
  }
  const globalBuffer = g.Buffer;
  if (typeof globalBuffer.kMaxLength !== "number") {
    globalBuffer.kMaxLength = BUFFER_MAX_LENGTH;
  }
  if (typeof globalBuffer.kStringMaxLength !== "number") {
    globalBuffer.kStringMaxLength = BUFFER_MAX_STRING_LENGTH;
  }
  if (typeof globalBuffer.constants !== "object" || globalBuffer.constants === null) {
    globalBuffer.constants = BUFFER_CONSTANTS;
  }
  const builtinUtilModule = globalThis.__secureExecBuiltinUtilModule;
  if (builtinUtilModule?.types) {
    builtinUtilModule.types.isProxy = () => false;
  }
  installBuiltinUtilFormatWithOptions(builtinUtilModule);
  if (typeof g.atob === "undefined" || typeof g.btoa === "undefined") {
    const base64 = require_base64_js();
    if (typeof g.atob === "undefined") {
      g.atob = (value) => {
        const bytes = base64.toByteArray(String(value));
        let decoded = "";
        for (const byte of bytes) {
          decoded += String.fromCharCode(byte);
        }
        return decoded;
      };
    }
    if (typeof g.btoa === "undefined") {
      g.btoa = (value) => {
        const input = String(value);
        const bytes = new Uint8Array(input.length);
        for (let index = 0; index < input.length; index += 1) {
          const code = input.charCodeAt(index);
          if (code > 255) {
            throw new TypeError("Invalid character");
          }
          bytes[index] = code;
        }
        return base64.fromByteArray(bytes);
      };
    }
  }
  if (typeof g.Crypto === "undefined") {
    g.Crypto = SandboxCrypto;
  }
  if (typeof g.SubtleCrypto === "undefined") {
    g.SubtleCrypto = SandboxSubtleCrypto;
  }
  if (typeof g.CryptoKey === "undefined") {
    g.CryptoKey = SandboxCryptoKey;
  }
  if (typeof g.DOMException === "undefined") {
    g.DOMException = SandboxDOMException;
  }
  if (typeof g.crypto === "undefined") {
    g.crypto = builtinCryptoModule;
  } else {
    const cryptoObj = g.crypto;
    for (const [name, value] of Object.entries(builtinCryptoModule)) {
      if (typeof cryptoObj[name] === "undefined") {
        cryptoObj[name] = value;
      }
    }
  }
  g.fetch = fetch;
  g.Headers = UndiciHeaders;
  g.Request = UndiciRequest;
  g.Response = UndiciResponse;
  installSafeIntlFormatters(g);
}
export { ProcessExitError, _addListener, _createProcessKillError, _cwd, _deliverProcessSignal, _emit, _exitCode, _exited, _getStdinIsTTY, _ignoredSelfSignals, _isTrackedProcessSignalEventName, _listenerCountForEvent, _processKillErrnoByCode, _processListeners, _processMaxListeners, _processMaxListenersWarned, _processOnceListeners, _processStartTime, _processVersionsCache, _removeListener, _resolveSignal, _signalNamesByNumber, _signalNumbers, _syncAllGuestProcessSignalStates, _syncGuestProcessSignalState, _trackedProcessSignalEvents, _umask, applyProcessConfig, config2, defaultProcessMemoryUsage, defaultProcessResourceUsage, dispatchCustomEmitterListeners, encodeFilePathSegment, fileURLToPath2, getNowMs, hrtime, installProcessIpcBridge, isProcessExitError, normalizeAsyncError, pathToFileURL2, process2, processClockNow, process_default, readLiveProcessCpuUsage, readLiveProcessMemoryUsage, readLiveProcessResourceUsage, readLiveProcessVersions, readProcessConfig, routeAsyncCallbackError, scheduleAsyncRethrow, setupGlobals, signalDispatch };
