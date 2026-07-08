const fsModule =
  typeof globalThis._requireFrom === 'function'
    ? globalThis._requireFrom('node:fs', '/')
    : __agentOSRequireBuiltin('node:fs');
const fs = fsModule.promises;
const { readSync, writeSync } = fsModule;
const path =
  typeof globalThis._requireFrom === 'function'
    ? globalThis._requireFrom('node:path', '/')
    : __agentOSRequireBuiltin('node:path');
const { WASI } = globalThis.__agentOSWasiModule;
const HOST_CWD =
  typeof process?.env?.AGENTOS_WASM_HOST_CWD === 'string' &&
  process.env.AGENTOS_WASM_HOST_CWD.length > 0
    ? path.resolve(process.env.AGENTOS_WASM_HOST_CWD)
    : path.resolve('.');

const WASI_ERRNO_SUCCESS = 0;
const WASI_ERRNO_ACCES = 2;
const WASI_ERRNO_AGAIN = 6;
const WASI_ERRNO_BADF = 8;
const WASI_ERRNO_CHILD = 10;
const WASI_ERRNO_INVAL = 28;
const WASI_ERRNO_NOENT = 44;
const WASI_ERRNO_PIPE = 64;
const WASI_ERRNO_ROFS = 69;
const WASI_ERRNO_SPIPE = 70;
const WASI_ERRNO_SRCH = 71;
const WASI_ERRNO_FAULT = 21;
const WASI_RIGHT_FD_WRITE = 64n;
const WASI_FILETYPE_UNKNOWN = 0;
const WASI_FILETYPE_CHARACTER_DEVICE = 2;
const WASI_FILETYPE_REGULAR_FILE = 4;
const WASI_OFLAGS_CREAT = 1;
const WASI_OFLAGS_DIRECTORY = 2;
const WASI_OFLAGS_EXCL = 4;
const WASI_OFLAGS_TRUNC = 8;
const WASI_FDFLAGS_APPEND = 1;
const WASI_WHENCE_SET = 0;
const WASI_WHENCE_CUR = 1;
const WASI_WHENCE_END = 2;
const WASM_PAGE_BYTES = 65536;
const DEFAULT_VIRTUAL_PID = 1;
const DEFAULT_VIRTUAL_PPID = 0;
const DEFAULT_VIRTUAL_UID = 0;
const DEFAULT_VIRTUAL_GID = 0;
const DEFAULT_VIRTUAL_OS_USER = 'root';
const DEFAULT_VIRTUAL_OS_HOMEDIR = '/root';
const DEFAULT_VIRTUAL_OS_SHELL = '/bin/sh';

function parseVirtualProcessNumber(value, fallback) {
  if (typeof value !== 'string' || value.trim() === '') {
    return fallback;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isInteger(parsed) && parsed >= 0 ? parsed : fallback;
}

function parseVirtualProcessString(value, fallback) {
  return typeof value === 'string' && value.length > 0 ? value : fallback;
}

function resolveVirtualPath(value, fallback) {
  const resolved = parseVirtualProcessString(value, fallback);
  return resolved.startsWith('/') ? path.posix.normalize(resolved) : fallback;
}

function isPathLike(specifier) {
  return specifier.startsWith('.') || specifier.startsWith('/') || specifier.startsWith('file:');
}

function resolveModuleGuestPathToHostPath(guestPath) {
  return resolveModuleGuestPathToHostMapping(guestPath)?.hostPath ?? null;
}

function resolveModuleGuestPathToHostMapping(guestPath) {
  if (typeof guestPath !== 'string') {
    return null;
  }

  const normalized = path.posix.normalize(guestPath);
  for (const mapping of GUEST_PATH_MAPPINGS) {
    if (mapping.guestPath === '/') {
      const suffix = normalized.replace(/^\/+/, '');
      return {
        hostPath: suffix ? path.join(mapping.hostPath, suffix) : mapping.hostPath,
        readOnly: mapping.readOnly === true,
      };
    }

    if (
      normalized !== mapping.guestPath &&
      !normalized.startsWith(`${mapping.guestPath}/`)
    ) {
      continue;
    }

    const suffix =
      normalized === mapping.guestPath
        ? ''
        : normalized.slice(mapping.guestPath.length + 1);
    return {
      hostPath: suffix ? path.join(mapping.hostPath, ...suffix.split('/')) : mapping.hostPath,
      readOnly: mapping.readOnly === true,
    };
  }

  return null;
}

function resolveModulePath(specifier) {
  if (specifier.startsWith('file:')) {
    const guestPath = guestFilePathFromUrl(specifier);
    if (guestPath) {
      return resolveModuleGuestPathToHostPath(guestPath) ?? new URL(specifier);
    }
    return new URL(specifier);
  }
  if (isPathLike(specifier)) {
    if (specifier.startsWith('/')) {
      return resolveModuleGuestPathToHostPath(specifier) ?? path.resolve(process.cwd(), specifier);
    }
    return path.resolve(process.cwd(), specifier);
  }
  return specifier;
}

function parseGuestPathMappings(value) {
  if (typeof value !== 'string' || value.length === 0) {
    return [];
  }
  try {
    return JSON.parse(value)
      .map((entry) => {
        const guestPath =
          entry && typeof entry.guestPath === 'string'
            ? path.posix.normalize(entry.guestPath)
            : null;
        const hostPath =
          entry && typeof entry.hostPath === 'string'
            ? path.resolve(entry.hostPath)
            : null;
        return guestPath && hostPath
          ? { guestPath, hostPath, readOnly: entry.readOnly === true }
          : null;
      })
      .filter(Boolean)
      .sort((left, right) => right.guestPath.length - left.guestPath.length);
  } catch {
    return [];
  }
}

const modulePath = process.env.AGENTOS_WASM_MODULE_PATH;
if (!modulePath) {
  throw new Error('AGENTOS_WASM_MODULE_PATH is required');
}
const moduleBase64 = process.env.AGENTOS_WASM_MODULE_BASE64;
const __agentOSWasmPhaseDebug = process.env.AGENTOS_WASM_WARMUP_DEBUG === '1';
const __agentOSWasmPhaseTimings = [];
const __agentOSWasiSyscallPhasesEnabled = process.env.AGENTOS_WASI_SYSCALL_PHASES === '1';
const __agentOSWasiSyscallMetrics = new Map();
const __agentOSWasiSyncRpcMetrics = new Map();

function __agentOSWasmNowNs() {
  return process.hrtime.bigint();
}

function __agentOSWasmRecordPhase(name, startedNs) {
  if (!__agentOSWasmPhaseDebug) {
    return;
  }
  const elapsedNs = __agentOSWasmNowNs() - startedNs;
  __agentOSWasmPhaseTimings.push({
    name,
    ms: Number(elapsedNs) / 1000000,
  });
}

function __agentOSWasmMeasurePhase(name, run) {
  const startedNs = __agentOSWasmNowNs();
  try {
    return run();
  } finally {
    __agentOSWasmRecordPhase(name, startedNs);
  }
}

function __agentOSWasiMetricFor(map, key, factory) {
  let metric = map.get(key);
  if (!metric) {
    metric = factory();
    map.set(key, metric);
  }
  return metric;
}

function __agentOSWasiSyscallMetric(name) {
  return __agentOSWasiMetricFor(__agentOSWasiSyscallMetrics, name, () => ({
    name,
    count: 0,
    totalNs: 0n,
    phases: new Map(),
  }));
}

function __agentOSWasiRecordSyscall(name, startedNs) {
  if (!__agentOSWasiSyscallPhasesEnabled) {
    return;
  }
  const metric = __agentOSWasiSyscallMetric(name);
  metric.count += 1;
  metric.totalNs += __agentOSWasmNowNs() - startedNs;
}

function __agentOSWasiRecordPhase(syscallName, phaseName, startedNs) {
  if (!__agentOSWasiSyscallPhasesEnabled) {
    return;
  }
  const syscall = __agentOSWasiSyscallMetric(syscallName);
  const phase = __agentOSWasiMetricFor(syscall.phases, phaseName, () => ({
    name: phaseName,
    count: 0,
    totalNs: 0n,
  }));
  phase.count += 1;
  phase.totalNs += __agentOSWasmNowNs() - startedNs;
}

function __agentOSWasiMeasurePhase(syscallName, phaseName, run) {
  if (!__agentOSWasiSyscallPhasesEnabled) {
    return run();
  }
  const startedNs = __agentOSWasmNowNs();
  try {
    return run();
  } finally {
    __agentOSWasiRecordPhase(syscallName, phaseName, startedNs);
  }
}

function __agentOSWasiRecordSyncRpc(method, route, startedNs) {
  if (!__agentOSWasiSyscallPhasesEnabled) {
    return;
  }
  const key = `${route}:${method}`;
  const metric = __agentOSWasiMetricFor(__agentOSWasiSyncRpcMetrics, key, () => ({
    method,
    route,
    count: 0,
    totalNs: 0n,
  }));
  metric.count += 1;
  metric.totalNs += __agentOSWasmNowNs() - startedNs;
}

function __agentOSWasiNsToUs(ns) {
  return Number(ns) / 1000;
}

function __agentOSWasiMetricSummary(metric) {
  const totalUs = __agentOSWasiNsToUs(metric.totalNs);
  return {
    name: metric.name,
    count: metric.count,
    totalUs,
    avgUs: metric.count > 0 ? totalUs / metric.count : 0,
    phases: Array.from(metric.phases.values())
      .map((phase) => {
        const phaseTotalUs = __agentOSWasiNsToUs(phase.totalNs);
        return {
          name: phase.name,
          count: phase.count,
          totalUs: phaseTotalUs,
          avgUs: phase.count > 0 ? phaseTotalUs / phase.count : 0,
        };
      })
      .sort((left, right) => right.totalUs - left.totalUs),
  };
}

function __agentOSWasiEmitSyscallPhaseMetrics() {
  if (!__agentOSWasiSyscallPhasesEnabled || typeof process?.stderr?.write !== 'function') {
    return;
  }
  try {
    process.stderr.write(`__AGENTOS_WASI_SYSCALL_PHASE_METRICS__:${JSON.stringify({
      modulePath,
      syscalls: Array.from(__agentOSWasiSyscallMetrics.values())
        .map(__agentOSWasiMetricSummary)
        .sort((left, right) => right.totalUs - left.totalUs),
      syncRpc: Array.from(__agentOSWasiSyncRpcMetrics.values())
        .map((metric) => {
          const totalUs = __agentOSWasiNsToUs(metric.totalNs);
          return {
            method: metric.method,
            route: metric.route,
            count: metric.count,
            totalUs,
            avgUs: metric.count > 0 ? totalUs / metric.count : 0,
          };
        })
        .sort((left, right) => right.totalUs - left.totalUs),
    })}\n`);
  } catch {
    // Diagnostics must never change command behavior.
  }
}

if (__agentOSWasiSyscallPhasesEnabled && typeof process?.on === 'function') {
  process.on('exit', () => {
    __agentOSWasiEmitSyscallPhaseMetrics();
  });
}

function __agentOSWasmEmitPhaseMetrics(reason, extra = {}) {
  if (!__agentOSWasmPhaseDebug || typeof process?.stderr?.write !== 'function') {
    return;
  }
  try {
    process.stderr.write(`__AGENTOS_WASM_PHASE_METRICS__:${JSON.stringify({
      reason,
      modulePath,
      moduleBytes: typeof moduleBinary !== 'undefined' ? moduleBinary.byteLength : null,
      phases: __agentOSWasmPhaseTimings,
      ...extra,
    })}\n`);
  } catch {
    // Diagnostics must never change command behavior.
  }
}

const guestArgv = JSON.parse(process.env.AGENTOS_GUEST_ARGV ?? '[]');
const guestEnv = JSON.parse(process.env.AGENTOS_GUEST_ENV ?? '{}');
const GUEST_PATH_MAPPINGS = parseGuestPathMappings(process.env.AGENTOS_GUEST_PATH_MAPPINGS);
const permissionTier = process.env.AGENTOS_WASM_PERMISSION_TIER ?? 'full';
const prewarmOnly = process.env.AGENTOS_WASM_PREWARM_ONLY === '1';
const maxMemoryBytesValue = Number(process.env.AGENTOS_WASM_MAX_MEMORY_BYTES);
const maxMemoryPages = Number.isFinite(maxMemoryBytesValue)
  ? Math.max(0, Math.floor(maxMemoryBytesValue / WASM_PAGE_BYTES))
  : null;
const maxStackBytesValue = Number(process.env.AGENTOS_WASM_MAX_STACK_BYTES);
const maxStackBytes =
  Number.isFinite(maxStackBytesValue) && maxStackBytesValue > 0
    ? Math.floor(maxStackBytesValue)
    : null;

// A guest can drive WebAssembly into never-returning recursion. V8's default
// native stack guard already traps that as a generic `RangeError`, but the
// operator-configured `AGENTOS_WASM_MAX_STACK_BYTES` budget was previously
// never consulted, so the cap was dead. When a stack byte budget is set, treat
// a stack-exhaustion trap as enforcement of THAT budget: terminate the guest
// nonzero and attribute the failure to the configured limit instead of leaking
// the engine's generic default-guard message.
function isWasmStackExhaustionTrap(error) {
  const message = typeof error?.message === 'string' ? error.message : '';
  // V8 raises `RangeError: Maximum call stack size exceeded` when its native
  // stack guard fires on runaway recursion (the WebAssembly call stack is
  // mapped onto V8's). Match that explicitly rather than treating every
  // `RangeError` as stack exhaustion, so unrelated range failures still
  // surface with their own message.
  return /maximum call stack size exceeded/i.test(message);
}

function reportConfiguredStackLimitExceeded(error) {
  const detail = typeof error?.message === 'string' && error.message.length > 0
    ? ` (${error.message})`
    : '';
  if (typeof process?.stderr?.write === 'function') {
    process.stderr.write(
      `WebAssembly guest exceeded the configured stack byte limit of ${maxStackBytes} bytes${detail}\n`,
    );
  }
}
const frozenTimeValue = Number(process.env.AGENTOS_FROZEN_TIME_MS);
const frozenTimeMs = Number.isFinite(frozenTimeValue) ? Math.trunc(frozenTimeValue) : Date.now();
const frozenTimeNs = BigInt(frozenTimeMs) * 1000000n;
const VIRTUAL_UID = parseVirtualProcessNumber(
  process.env.AGENTOS_VIRTUAL_PROCESS_UID,
  DEFAULT_VIRTUAL_UID,
);
const VIRTUAL_GID = parseVirtualProcessNumber(
  process.env.AGENTOS_VIRTUAL_PROCESS_GID,
  DEFAULT_VIRTUAL_GID,
);
const VIRTUAL_PID = parseVirtualProcessNumber(
  process.env.AGENTOS_VIRTUAL_PROCESS_PID,
  DEFAULT_VIRTUAL_PID,
);
const VIRTUAL_PPID = parseVirtualProcessNumber(
  process.env.AGENTOS_VIRTUAL_PROCESS_PPID,
  DEFAULT_VIRTUAL_PPID,
);
const VIRTUAL_OS_USER = parseVirtualProcessString(
  (globalThis.__agentOSVirtualOs||{}).user,
  DEFAULT_VIRTUAL_OS_USER,
);
const VIRTUAL_OS_HOMEDIR = resolveVirtualPath(
  (globalThis.__agentOSVirtualOs||{}).homedir,
  DEFAULT_VIRTUAL_OS_HOMEDIR,
);
const VIRTUAL_OS_SHELL = resolveVirtualPath(
  (globalThis.__agentOSVirtualOs||{}).shell,
  DEFAULT_VIRTUAL_OS_SHELL,
);
const CONTROL_PIPE_FD = parseControlPipeFd(process.env.AGENTOS_CONTROL_PIPE_FD);
const NODE_SYNC_RPC_ENABLE = process.env.AGENTOS_NODE_SYNC_RPC_ENABLE === '1';
const NODE_SYNC_RPC_REQUEST_FD = parseControlPipeFd(process.env.AGENTOS_NODE_SYNC_RPC_REQUEST_FD);
const NODE_SYNC_RPC_RESPONSE_FD = parseControlPipeFd(process.env.AGENTOS_NODE_SYNC_RPC_RESPONSE_FD);
const KERNEL_STDIO_SYNC_RPC = process.env.AGENTOS_WASI_STDIO_SYNC_RPC === '1';
const SIDECAR_MANAGED_PROCESS =
  typeof process?.env?.AGENTOS_SANDBOX_ROOT === 'string' &&
  process.env.AGENTOS_SANDBOX_ROOT.length > 0;
let nextSyncRpcId = 1;
let syncRpcResponseBuffer = '';
const spawnedChildren = new Map();
const spawnedChildrenById = new Map();
let nextSyntheticChildPid = 0x40000000;
const syntheticFdEntries = new Map();
const delegateManagedFdRefCounts = new Map();
const closedPassthroughFds = new Set();
globalThis.__agentOSWasiDelegateFdRefCount = (fd) =>
  delegateManagedFdRefCounts.get(Number(fd) >>> 0) ?? 0;
const passthroughHandles = new Map([
  [0, { kind: 'passthrough', targetFd: 0, displayFd: 0, refCount: 0, open: true }],
  [1, { kind: 'passthrough', targetFd: 1, displayFd: 1, refCount: 0, open: true }],
  [2, { kind: 'passthrough', targetFd: 2, displayFd: 2, refCount: 0, open: true }],
]);
const retainedSyntheticHandlesByDisplayFd = new Map();
const retainedSpawnOutputHandlesByFd = new Map();
let nextSyntheticFd = 64;
let nextSyntheticPipeId = 1;
const syntheticWaitArray = new Int32Array(new SharedArrayBuffer(4));
let delegateWriteScratch = { base: 0, capacity: 0 };

function traceHostProcess(event, details) {
  const enabled =
    (typeof TRACE_HOST_PROCESS === 'boolean' && TRACE_HOST_PROCESS) ||
    (typeof HOST_PROCESS_ENV !== 'undefined' &&
      HOST_PROCESS_ENV?.AGENTOS_TRACE_HOST_PROCESS === '1') ||
    (typeof process !== 'undefined' && process?.env?.AGENTOS_TRACE_HOST_PROCESS === '1');
  if (!enabled) {
    return;
  }
  try {
    process.stderr.write(`[agent-os-host-process] ${event} ${JSON.stringify(details)}\n`);
  } catch {
    // Ignore tracing failures.
  }
}

const WASI_RIGHT_FD_DATASYNC = 1n << 0n;
const WASI_RIGHT_FD_READ = 1n << 1n;
const WASI_RIGHT_FD_SEEK = 1n << 2n;
const WASI_RIGHT_FD_FDSTAT_SET_FLAGS = 1n << 3n;
const WASI_RIGHT_FD_SYNC = 1n << 4n;
const WASI_RIGHT_FD_TELL = 1n << 5n;
const WASI_RIGHT_FD_ADVISE = 1n << 7n;
const WASI_RIGHT_FD_ALLOCATE = 1n << 8n;
const WASI_RIGHT_PATH_CREATE_DIRECTORY = 1n << 9n;
const WASI_RIGHT_PATH_LINK_SOURCE = 1n << 10n;
const WASI_RIGHT_PATH_LINK_TARGET = 1n << 11n;
const WASI_RIGHT_PATH_OPEN = 1n << 13n;
const WASI_RIGHT_FD_READDIR = 1n << 14n;
const WASI_RIGHT_PATH_READLINK = 1n << 15n;
const WASI_RIGHT_PATH_RENAME_SOURCE = 1n << 16n;
const WASI_RIGHT_PATH_RENAME_TARGET = 1n << 17n;
const WASI_RIGHT_PATH_FILESTAT_GET = 1n << 18n;
const WASI_RIGHT_PATH_FILESTAT_SET_SIZE = 1n << 19n;
const WASI_RIGHT_PATH_FILESTAT_SET_TIMES = 1n << 20n;
const WASI_RIGHT_FD_FILESTAT_GET = 1n << 21n;
const WASI_RIGHT_FD_FILESTAT_SET_SIZE = 1n << 22n;
const WASI_RIGHT_FD_FILESTAT_SET_TIMES = 1n << 23n;
const WASI_RIGHT_PATH_SYMLINK = 1n << 24n;
const WASI_RIGHT_PATH_REMOVE_DIRECTORY = 1n << 25n;
const WASI_RIGHT_PATH_UNLINK_FILE = 1n << 26n;
const WASI_RIGHT_POLL_FD_READWRITE = 1n << 27n;

const READ_ONLY_PREOPEN_RIGHTS_BASE =
  WASI_RIGHT_FD_READ |
  WASI_RIGHT_FD_SEEK |
  WASI_RIGHT_FD_FDSTAT_SET_FLAGS |
  WASI_RIGHT_FD_TELL |
  WASI_RIGHT_PATH_OPEN |
  WASI_RIGHT_FD_READDIR |
  WASI_RIGHT_PATH_READLINK |
  WASI_RIGHT_PATH_FILESTAT_GET |
  WASI_RIGHT_FD_FILESTAT_GET |
  WASI_RIGHT_POLL_FD_READWRITE;
const READ_ONLY_PREOPEN_RIGHTS_INHERITING =
  WASI_RIGHT_FD_READ |
  WASI_RIGHT_FD_SEEK |
  WASI_RIGHT_FD_FDSTAT_SET_FLAGS |
  WASI_RIGHT_FD_TELL |
  WASI_RIGHT_FD_FILESTAT_GET |
  WASI_RIGHT_POLL_FD_READWRITE;
const READ_WRITE_PREOPEN_RIGHTS_BASE =
  READ_ONLY_PREOPEN_RIGHTS_BASE |
  WASI_RIGHT_FD_DATASYNC |
  WASI_RIGHT_FD_SYNC |
  WASI_RIGHT_FD_WRITE |
  WASI_RIGHT_FD_ADVISE |
  WASI_RIGHT_FD_ALLOCATE |
  WASI_RIGHT_PATH_CREATE_DIRECTORY |
  WASI_RIGHT_PATH_FILESTAT_SET_SIZE |
  WASI_RIGHT_PATH_FILESTAT_SET_TIMES |
  WASI_RIGHT_FD_FILESTAT_SET_SIZE |
  WASI_RIGHT_FD_FILESTAT_SET_TIMES;
const READ_WRITE_PREOPEN_RIGHTS_INHERITING =
  READ_ONLY_PREOPEN_RIGHTS_INHERITING |
  WASI_RIGHT_FD_DATASYNC |
  WASI_RIGHT_FD_SYNC |
  WASI_RIGHT_FD_WRITE |
  WASI_RIGHT_FD_ADVISE |
  WASI_RIGHT_FD_ALLOCATE |
  WASI_RIGHT_FD_FILESTAT_SET_SIZE |
  WASI_RIGHT_FD_FILESTAT_SET_TIMES;
const FULL_PREOPEN_RIGHTS_BASE =
  READ_WRITE_PREOPEN_RIGHTS_BASE |
  WASI_RIGHT_PATH_LINK_SOURCE |
  WASI_RIGHT_PATH_LINK_TARGET |
  WASI_RIGHT_PATH_RENAME_SOURCE |
  WASI_RIGHT_PATH_RENAME_TARGET |
  WASI_RIGHT_PATH_SYMLINK |
  WASI_RIGHT_PATH_REMOVE_DIRECTORY |
  WASI_RIGHT_PATH_UNLINK_FILE;
const FULL_PREOPEN_RIGHTS_INHERITING = READ_WRITE_PREOPEN_RIGHTS_INHERITING;

function buildPreopenRights() {
  switch (permissionTier) {
    case 'read-only':
      return {
        rightsBase: READ_ONLY_PREOPEN_RIGHTS_BASE,
        rightsInheriting: READ_ONLY_PREOPEN_RIGHTS_INHERITING,
      };
    case 'read-write':
      return {
        rightsBase: READ_WRITE_PREOPEN_RIGHTS_BASE,
        rightsInheriting: READ_WRITE_PREOPEN_RIGHTS_INHERITING,
      };
    case 'full':
    default:
      return {
        rightsBase: FULL_PREOPEN_RIGHTS_BASE,
        rightsInheriting: FULL_PREOPEN_RIGHTS_INHERITING,
      };
  }
}

function createPreopen(hostPath, readOnly = false) {
  const rights =
    readOnly === true
      ? {
          rightsBase: READ_ONLY_PREOPEN_RIGHTS_BASE,
          rightsInheriting: READ_ONLY_PREOPEN_RIGHTS_INHERITING,
        }
      : buildPreopenRights();
  return {
    hostPath,
    readOnly: readOnly === true,
    rightsBase: rights.rightsBase,
    rightsInheriting: rights.rightsInheriting,
  };
}

function mappingContainsGuestPath(mapping, guestPath) {
  if (!mapping || typeof mapping.guestPath !== 'string' || typeof guestPath !== 'string') {
    return false;
  }
  const normalized = path.posix.normalize(guestPath);
  return (
    normalized === mapping.guestPath ||
    mapping.guestPath === '/' ||
    normalized.startsWith(`${mapping.guestPath}/`)
  );
}

function mappingContainsHostPath(mapping, hostPath) {
  if (!mapping || typeof mapping.hostPath !== 'string' || typeof hostPath !== 'string') {
    return false;
  }
  const normalized = path.resolve(hostPath);
  const root = path.resolve(mapping.hostPath);
  return normalized === root || normalized.startsWith(`${root}${path.sep}`);
}

function readOnlyForCwd(guestCwd) {
  for (const mapping of GUEST_PATH_MAPPINGS) {
    if (
      mapping?.readOnly === true &&
      (mappingContainsGuestPath(mapping, guestCwd) ||
        mappingContainsHostPath(mapping, HOST_CWD))
    ) {
      return true;
    }
  }
  return false;
}

function buildPreopens() {
  switch (permissionTier) {
    case 'isolated':
      return {};
    case 'read-only':
    case 'read-write':
    case 'full':
    default:
      const guestCwd =
        typeof guestEnv?.PWD === 'string' && guestEnv.PWD.startsWith('/')
          ? path.posix.normalize(guestEnv.PWD)
          : typeof process.env.PWD === 'string' && process.env.PWD.startsWith('/')
            ? path.posix.normalize(process.env.PWD)
            : null;
      const preopens = {};
      const seen = new Set();
      const cwdReadOnly = readOnlyForCwd(guestCwd);
      const cwdMount = guestCwd || '/workspace';
      preopens[cwdMount] = createPreopen(HOST_CWD, cwdReadOnly);
      seen.add(cwdMount);
      const rootMapping = GUEST_PATH_MAPPINGS.find(
        (mapping) => mapping && mapping.guestPath === '/',
      );
      if (rootMapping && !seen.has('/')) {
        preopens['/'] = createPreopen(rootMapping.hostPath, rootMapping.readOnly);
        seen.add('/');
      }
      for (const mapping of GUEST_PATH_MAPPINGS) {
        if (!mapping || typeof mapping.guestPath !== 'string' || typeof mapping.hostPath !== 'string') {
          continue;
        }
        const guestPath = path.posix.normalize(mapping.guestPath);
        if (
          !path.posix.isAbsolute(guestPath) ||
          seen.has(guestPath) ||
          guestPath === guestCwd
        ) {
          continue;
        }
        preopens[guestPath] = createPreopen(mapping.hostPath, mapping.readOnly);
        seen.add(guestPath);
      }
      if (cwdMount !== '/workspace' && !seen.has('/workspace')) {
        preopens['/workspace'] = createPreopen(HOST_CWD, cwdReadOnly);
        seen.add('/workspace');
      }
      return preopens;
  }
}

function readVarUint(bytes, offset, label) {
  let value = 0;
  let shift = 0;
  let cursor = offset;
  for (let count = 0; count < 10; count += 1) {
    if (cursor >= bytes.length) {
      throw new Error(`WebAssembly ${label} truncated`);
    }
    const byte = bytes[cursor];
    cursor += 1;
    value += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) {
      return { value, offset: cursor };
    }
    shift += 7;
  }
  throw new Error(`WebAssembly ${label} exceeds varuint limit`);
}

function encodeVarUint(value) {
  const encoded = [];
  let remaining = Math.trunc(value);
  do {
    let byte = remaining & 0x7f;
    remaining = Math.floor(remaining / 128);
    if (remaining > 0) {
      byte |= 0x80;
    }
    encoded.push(byte);
  } while (remaining > 0);
  return encoded;
}

function appendBytes(out, bytes) {
  for (let i = 0; i < bytes.length; i += 1) {
    out.push(bytes[i]);
  }
}

function rewriteMemorySection(sectionBytes, limitPages) {
  let offset = 0;
  const countResult = readVarUint(sectionBytes, offset, 'memory count');
  const count = countResult.value;
  offset = countResult.offset;
  const rewritten = [];
  appendBytes(rewritten, encodeVarUint(count));

  for (let index = 0; index < count; index += 1) {
    const flagsResult = readVarUint(sectionBytes, offset, 'memory flags');
    const flags = flagsResult.value;
    offset = flagsResult.offset;

    if ((flags & ~1) !== 0) {
      throw new Error(
        `configured WebAssembly memory limit does not support memory flags ${flags}`,
      );
    }

    const initialResult = readVarUint(sectionBytes, offset, 'memory minimum');
    const initialPages = initialResult.value;
    offset = initialResult.offset;

    let maximumPages = null;
    if ((flags & 1) !== 0) {
      const maximumResult = readVarUint(sectionBytes, offset, 'memory maximum');
      maximumPages = maximumResult.value;
      offset = maximumResult.offset;
    }

    if (initialPages > limitPages) {
      throw new Error(
        `initial WebAssembly memory of ${initialPages * WASM_PAGE_BYTES} bytes exceeds the configured limit of ${limitPages * WASM_PAGE_BYTES} bytes`,
      );
    }

    const cappedMaximumPages =
      maximumPages == null ? limitPages : Math.min(maximumPages, limitPages);
    appendBytes(rewritten, encodeVarUint(1));
    appendBytes(rewritten, encodeVarUint(initialPages));
    appendBytes(rewritten, encodeVarUint(cappedMaximumPages));
  }

  if (offset !== sectionBytes.length) {
    throw new Error('memory section parsing did not consume the full section');
  }

  return rewritten;
}

function enforceMemoryLimit(moduleBytes, limitPages) {
  if (!Number.isInteger(limitPages)) {
    return moduleBytes;
  }

  const bytes = moduleBytes instanceof Uint8Array ? moduleBytes : new Uint8Array(moduleBytes);
  if (bytes.length < 8 || bytes[0] !== 0 || bytes[1] !== 0x61 || bytes[2] !== 0x73 || bytes[3] !== 0x6d) {
    throw new Error('module is not a valid WebAssembly binary');
  }

  const rewritten = Array.from(bytes.slice(0, 8));
  let offset = 8;

  while (offset < bytes.length) {
    const sectionStart = offset;
    const sectionId = bytes[offset];
    offset += 1;
    const sectionSizeResult = readVarUint(bytes, offset, 'section size');
    const sectionSize = sectionSizeResult.value;
    offset = sectionSizeResult.offset;
    const sectionEnd = offset + sectionSize;
    if (sectionEnd > bytes.length) {
      throw new Error('section extends past end of module');
    }

    if (sectionId !== 5) {
      appendBytes(rewritten, bytes.slice(sectionStart, sectionEnd));
      offset = sectionEnd;
      continue;
    }

    const rewrittenSection = rewriteMemorySection(bytes.slice(offset, sectionEnd), limitPages);
    rewritten.push(sectionId);
    appendBytes(rewritten, encodeVarUint(rewrittenSection.length));
    appendBytes(rewritten, rewrittenSection);
    offset = sectionEnd;
  }

  return Buffer.from(rewritten);
}

function decodeBase64ToUint8Array(value) {
  return Buffer.from(value, 'base64');
}

// Memoized kernel-PTY probe for the guest's stdio fds. Rust's
// `stdin().is_terminal()` (and wasi-libc `isatty`) ask `fd_fdstat_get` for a
// CHARACTER_DEVICE filetype; the runner-process fds are pipes, so a delegated
// answer hides the kernel PTY and interactive guests (e.g. brush's prompt)
// believe stdin is not a terminal. Ask the sidecar's kernel instead.
const stdioTtyCache = new Map();
function stdioFdIsKernelTty(fd) {
  const descriptor = Number(fd) >>> 0;
  if (descriptor > 2) return false;
  // Even in kernel-stdio sync-RPC mode the answer must come from the kernel:
  // that mode is on for EVERY sidecar wasm execution, including piped
  // vm.exec() runs whose stdio is NOT a PTY. Hardcoding true here made
  // non-interactive shells think they had a terminal (fd_fdstat_get reported
  // CHARACTER_DEVICE, host_tty.isatty said 1), so they enabled raw mode and
  // the kernel's truthful "not a PTY end" refusal trapped the guest with
  // exit 1 after otherwise-successful commands.
  if (stdioTtyCache.has(descriptor)) return stdioTtyCache.get(descriptor);
  let isTty = false;
  try {
    isTty = callSyncRpc('__kernel_isatty', [descriptor]) === true;
  } catch {
    isTty = false;
  }
  stdioTtyCache.set(descriptor, isTty);
  return isTty;
}

// Long event-driven in-RPC waits: the sidecar services __kernel_stdin_read /
// __kernel_poll by parking the RPC and replying when kernel poll state changes
// (reply-by-token), so a long wait costs no dispatch-loop time and near-zero
// CPU (the guest thread blocks in Atomics.wait inside callSync). Keep each
// slice under the 30s guest sync-RPC deadline.
const KERNEL_WAIT_SLICE_MS = 10_000;

function readKernelStdinChunk(maxBytes) {
  const requestedLength = Math.max(1, Number(maxBytes) >>> 0);
  while (true) {
    const response = callSyncRpc('__kernel_stdin_read', [
      requestedLength,
      KERNEL_WAIT_SLICE_MS,
    ]);
    if (response && typeof response.dataBase64 === 'string') {
      return Buffer.from(response.dataBase64, 'base64');
    }
    if (response && response.done === true) {
      return null;
    }
  }
}

const rawModuleBytes = globalThis.__agentOSWasmModuleBytes;
const moduleSource =
  rawModuleBytes instanceof Uint8Array
    ? Buffer.from(rawModuleBytes.buffer, rawModuleBytes.byteOffset, rawModuleBytes.byteLength)
    : typeof moduleBase64 === 'string' && moduleBase64.length > 0
    ? moduleBase64
    : fsModule.readFileSync(resolveModulePath(modulePath));
const moduleBytes =
  typeof moduleSource === 'string'
    ? __agentOSWasmMeasurePhase('decodeBase64ToUint8Array', () => decodeBase64ToUint8Array(moduleSource))
    : moduleSource;
const moduleBinary = __agentOSWasmMeasurePhase('enforceMemoryLimit', () => enforceMemoryLimit(moduleBytes, maxMemoryPages));
const module = __agentOSWasmMeasurePhase('WebAssembly.Module', () => new WebAssembly.Module(moduleBinary));

if (prewarmOnly) {
  __agentOSWasmEmitPhaseMetrics('prewarm');
  process.exit(0);
}

const WASI_PREOPENS = buildPreopens();
const WASI_PREOPEN_FD_BASE = 3;
const WASI_PREOPEN_ENTRIES = Object.entries(WASI_PREOPENS);

const wasi = new WASI({
  version: 'preview1',
  args: guestArgv,
  env: guestEnv,
  preopens: WASI_PREOPENS,
  returnOnExit: true,
});

let instanceMemory = null;
const wasiImport = { ...wasi.wasiImport };
// node:wasi omits sock_shutdown; guest socket teardown happens via fd_close + host_net, so a
// success no-op is sufficient (needed for the cross-compiled X server / X clients).
if (typeof wasiImport.sock_shutdown !== 'function') {
  wasiImport.sock_shutdown = () => 0;
}
const delegateClockTimeGet =
  typeof wasi.wasiImport.clock_time_get === 'function'
    ? wasi.wasiImport.clock_time_get.bind(wasi.wasiImport)
    : null;
const delegateClockResGet =
  typeof wasi.wasiImport.clock_res_get === 'function'
    ? wasi.wasiImport.clock_res_get.bind(wasi.wasiImport)
    : null;
const delegatePathOpen =
  typeof wasi.wasiImport.path_open === 'function'
    ? wasi.wasiImport.path_open.bind(wasi.wasiImport)
    : null;
const delegateFdWrite =
  typeof wasi.wasiImport.fd_write === 'function'
    ? wasi.wasiImport.fd_write.bind(wasi.wasiImport)
    : null;
const delegateFdPread =
  typeof wasi.wasiImport.fd_pread === 'function'
    ? wasi.wasiImport.fd_pread.bind(wasi.wasiImport)
    : null;
const delegateFdPwrite =
  typeof wasi.wasiImport.fd_pwrite === 'function'
    ? wasi.wasiImport.fd_pwrite.bind(wasi.wasiImport)
    : null;
const delegateFdSync =
  typeof wasi.wasiImport.fd_sync === 'function'
    ? wasi.wasiImport.fd_sync.bind(wasi.wasiImport)
    : null;

function decodeSignalMask(maskLo, maskHi) {
  const values = [];
  const lo = Number(maskLo) >>> 0;
  const hi = Number(maskHi) >>> 0;
  for (let bit = 0; bit < 32; bit += 1) {
    if (((lo >>> bit) & 1) === 1) {
      values.push(bit + 1);
    }
  }
  for (let bit = 0; bit < 32; bit += 1) {
    if (((hi >>> bit) & 1) === 1) {
      values.push(bit + 33);
    }
  }
  return values;
}

function parseControlPipeFd(value) {
  if (typeof value !== 'string' || value.trim() === '') {
    return null;
  }

  const parsed = Number.parseInt(value, 10);
  return Number.isInteger(parsed) && parsed >= 3 ? parsed : null;
}

function emitControlMessage(message) {
  const emitSignalStateFallback = () => {
    if (
      message?.type === 'signal_state' &&
      typeof process?.stdout?.write === 'function'
    ) {
      try {
        process.stdout.write(`__AGENTOS_WASM_SIGNAL_STATE__:${JSON.stringify(message)}\n`);
      } catch {
        // Ignore signal-state bridge failures during teardown.
      }
    }
  };

  if (CONTROL_PIPE_FD == null) {
    emitSignalStateFallback();
    return;
  }

  try {
    writeSync(CONTROL_PIPE_FD, `${JSON.stringify(message)}\n`);
  } catch {
    emitSignalStateFallback();
  }
}

function isWorkspaceReadOnly() {
  return permissionTier === 'read-only' || permissionTier === 'isolated';
}

function hasWriteRights(rights) {
  try {
    return (BigInt(rights) & WASI_RIGHT_FD_WRITE) !== 0n;
  } catch {
    return true;
  }
}

function hasReadRights(rights) {
  try {
    return (BigInt(rights) & WASI_RIGHT_FD_READ) !== 0n;
  } catch {
    return true;
  }
}

function hasMutationOpenFlags(oflags) {
  const normalized = Number(oflags) >>> 0;
  return (
    (normalized & WASI_OFLAGS_CREAT) !== 0 ||
    (normalized & WASI_OFLAGS_EXCL) !== 0 ||
    (normalized & WASI_OFLAGS_TRUNC) !== 0
  );
}

function denyReadOnlyMutation() {
  return WASI_ERRNO_ROFS;
}

function guestPathForPreopenKey(key) {
  if (key === '.') {
    return HOST_FS_GUEST_CWD;
  }
  return path.posix.normalize(key);
}

function resolvePathOpenGuestPath(fd, pathPtr, pathLen) {
  const target = readGuestString(pathPtr, pathLen);
  if (target.startsWith('/')) {
    return path.posix.normalize(target);
  }

  const handle = lookupFdHandle(fd);
  if (handle && typeof handle.guestPath === 'string') {
    return path.posix.resolve(handle.guestPath, target);
  }

  const preopenIndex = (Number(fd) >>> 0) - WASI_PREOPEN_FD_BASE;
  const preopen = WASI_PREOPEN_ENTRIES[preopenIndex];
  if (preopen) {
    return path.posix.resolve(guestPathForPreopenKey(preopen[0]), target);
  }

  return null;
}

function guestPathIsReadOnly(guestPath) {
  return GUEST_PATH_MAPPINGS.some(
    (mapping) => mapping?.readOnly === true && mappingContainsGuestPath(mapping, guestPath),
  );
}

function resolvedGuestPathIsReadOnly(fd, pathPtr, pathLen) {
  try {
    const guestPath = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
    return typeof guestPath === 'string' && guestPathIsReadOnly(guestPath);
  } catch {
    return false;
  }
}

// Guest path recorded for a managed (path_open passthrough) fd, if known.
function guestPathForManagedFd(fd) {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'passthrough' && typeof handle.guestPath === 'string') {
    return handle.guestPath;
  }
  return null;
}

// WASI fstflags bits for *_filestat_set_times.
const WASI_FSTFLAGS_ATIM = 1;
const WASI_FSTFLAGS_ATIM_NOW = 2;
const WASI_FSTFLAGS_MTIM = 4;
const WASI_FSTFLAGS_MTIM_NOW = 8;
const WASI_LOOKUPFLAGS_SYMLINK_FOLLOW = 1;

// Resolve the (atime, mtime) seconds to apply for a *_filestat_set_times call:
// explicit nanosecond args, "now", or preserve-current (the OMIT case; node's
// utimes has no omit, so re-apply the current value from stat).
function resolveSetTimes(currentStat, atimNs, mtimNs, fstFlags) {
  const flags = Number(fstFlags) >>> 0;
  const nowSec = Date.now() / 1000;
  let atime = currentStat.atimeMs / 1000;
  let mtime = currentStat.mtimeMs / 1000;
  if (flags & WASI_FSTFLAGS_ATIM_NOW) {
    atime = nowSec;
  } else if (flags & WASI_FSTFLAGS_ATIM) {
    atime = Number(BigInt(atimNs)) / 1e9;
  }
  if (flags & WASI_FSTFLAGS_MTIM_NOW) {
    mtime = nowSec;
  } else if (flags & WASI_FSTFLAGS_MTIM) {
    mtime = Number(BigInt(mtimNs)) / 1e9;
  }
  return { atime, mtime };
}

// The embedded WASI shim omits the *_filestat_set_times ops entirely, which
// makes instantiation of any module importing them fail with a LinkError
// (vim uses path_filestat_set_times to restore file timestamps). Implement
// them against the bridge fs (the kernel VFS) when the base has no delegate.
if (typeof wasiImport.path_filestat_set_times !== 'function') {
  wasiImport.path_filestat_set_times = (fd, flags, pathPtr, pathLen, atimNs, mtimNs, fstFlags) => {
    try {
      const guestPath = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
      if (typeof guestPath !== 'string') {
        return WASI_ERRNO_BADF;
      }
      const follow = (Number(flags) >>> 0) & WASI_LOOKUPFLAGS_SYMLINK_FOLLOW;
      const stat = follow ? fsModule.statSync(guestPath) : fsModule.lstatSync(guestPath);
      const { atime, mtime } = resolveSetTimes(stat, atimNs, mtimNs, fstFlags);
      if (follow || typeof fsModule.lutimesSync !== 'function') {
        fsModule.utimesSync(guestPath, atime, mtime);
      } else {
        fsModule.lutimesSync(guestPath, atime, mtime);
      }
      return WASI_ERRNO_SUCCESS;
    } catch (error) {
      return error?.code === 'ENOENT' ? WASI_ERRNO_NOENT : WASI_ERRNO_INVAL;
    }
  };
}

if (typeof wasiImport.fd_filestat_set_times !== 'function') {
  wasiImport.fd_filestat_set_times = (fd, atimNs, mtimNs, fstFlags) => {
    try {
      const guestPath = guestPathForManagedFd(fd);
      if (typeof guestPath !== 'string') {
        return WASI_ERRNO_BADF;
      }
      const stat = fsModule.statSync(guestPath);
      const { atime, mtime } = resolveSetTimes(stat, atimNs, mtimNs, fstFlags);
      fsModule.utimesSync(guestPath, atime, mtime);
      return WASI_ERRNO_SUCCESS;
    } catch (error) {
      return error?.code === 'ENOENT' ? WASI_ERRNO_NOENT : WASI_ERRNO_INVAL;
    }
  };
}

function pathOpenMayCreateTarget(oflags, rightsBase, fdflags) {
  const normalizedOflags = Number(oflags) >>> 0;
  const normalizedFdflags = Number(fdflags) >>> 0;
  return (
    (normalizedOflags & WASI_OFLAGS_CREAT) !== 0 ||
    ((normalizedFdflags & WASI_FDFLAGS_APPEND) !== 0 && hasWriteRights(rightsBase))
  );
}

function precreatePathOpenTarget(fd, pathPtr, pathLen, oflags, rightsBase, fdflags) {
  if (!pathOpenMayCreateTarget(oflags, rightsBase, fdflags)) {
    return null;
  }

  const guestPath = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
  if (typeof guestPath !== 'string') {
    return null;
  }

  if (!fsModule.existsSync(guestPath)) {
    fsModule.writeFileSync(guestPath, Buffer.alloc(0));
  }
  return guestPath;
}

function fsOpenFlagForPathOpen(oflags, rightsBase, fdflags) {
  const normalizedOflags = Number(oflags) >>> 0;
  const normalizedFdflags = Number(fdflags) >>> 0;
  const wantsRead = hasReadRights(rightsBase);
  const wantsWrite = hasWriteRights(rightsBase);
  const wantsExclusive = (normalizedOflags & WASI_OFLAGS_EXCL) !== 0;
  const wantsAppend = (normalizedFdflags & WASI_FDFLAGS_APPEND) !== 0;
  const wantsTruncate = (normalizedOflags & WASI_OFLAGS_TRUNC) !== 0;

  if (!wantsWrite) {
    return 'r';
  }

  if (wantsAppend) {
    if (wantsExclusive) {
      return wantsRead ? 'ax+' : 'ax';
    }
    return wantsRead ? 'a+' : 'a';
  }

  if (wantsTruncate) {
    if (wantsExclusive) {
      return wantsRead ? 'wx+' : 'wx';
    }
    return wantsRead ? 'w+' : 'w';
  }

  return 'r+';
}

function allocateSyntheticFd() {
  let fd = nextSyntheticFd;
  while (
    syntheticFdEntries.has(fd) ||
    passthroughHandles.has(fd) ||
    delegateManagedFdRefCounts.has(fd)
  ) {
    fd += 1;
  }
  nextSyntheticFd = fd + 1;
  return fd;
}

function openGuestFileForPathOpen(fd, pathPtr, pathLen, oflags, rightsBase, fdflags, openedFdPtr) {
  if (!pathOpenMayCreateTarget(oflags, rightsBase, fdflags)) {
    return null;
  }

  const normalizedOflags = Number(oflags) >>> 0;
  const normalizedFdflags = Number(fdflags) >>> 0;

  const guestPath = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
  if (typeof guestPath !== 'string') {
    return null;
  }

  const append = (normalizedFdflags & WASI_FDFLAGS_APPEND) !== 0;
  const exclusive = (normalizedOflags & WASI_OFLAGS_EXCL) !== 0;
  const truncate = (normalizedOflags & WASI_OFLAGS_TRUNC) !== 0;
  if (!append && !exclusive && !truncate && !fsModule.existsSync(guestPath)) {
    fsModule.writeFileSync(guestPath, Buffer.alloc(0));
  }
  const targetFd = fsModule.openSync(
    guestPath,
    fsOpenFlagForPathOpen(oflags, rightsBase, fdflags),
    0o666,
  );
  const openedFd = allocateSyntheticFd();
  syntheticFdEntries.set(openedFd, {
    kind: 'guest-file',
    targetFd,
    displayFd: openedFd,
    refCount: 1,
    open: true,
    guestPath,
    position: append ? Number(fsModule.fstatSync(targetFd).size ?? 0) : 0,
    append,
  });
  return writeGuestUint32(openedFdPtr, openedFd);
}

function fsOpenNumericFlagsForManagedPath(rightsBase, fdflags) {
  const wantsRead = hasReadRights(rightsBase);
  const wantsWrite = hasWriteRights(rightsBase);
  let flags = wantsWrite ? (wantsRead ? 0o2 : 0o1) : 0;
  if ((Number(fdflags) & WASI_FDFLAGS_APPEND) !== 0) {
    flags |= 0o2000;
  }
  return flags;
}

function openManagedPathIoFd(guestPath, rightsBase, fdflags) {
  if (typeof guestPath !== 'string' || guestPath === '/dev/null') {
    return null;
  }
  try {
    return fsModule.openSync(
      guestPath,
      fsOpenNumericFlagsForManagedPath(rightsBase, fdflags),
      0o666,
    );
  } catch {
    return null;
  }
}

function retainPathOpenDelegateFd(openedFdPtr, guestPath, fdflags, rightsBase) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_SUCCESS;
  }

  try {
    const openedFd = new DataView(instanceMemory.buffer).getUint32(Number(openedFdPtr), true);
    const append = (Number(fdflags) & WASI_FDFLAGS_APPEND) !== 0;
    retainDelegateFd(openedFd);
    if (openedFd > 2 && !passthroughHandles.has(openedFd)) {
      const ioFd = openManagedPathIoFd(guestPath, rightsBase, fdflags);
      closedPassthroughFds.delete(openedFd);
      passthroughHandles.set(openedFd, {
        kind: 'passthrough',
        targetFd: openedFd,
        ioFd,
        displayFd: openedFd,
        refCount: 0,
        open: true,
        readOnly:
          typeof guestPath === 'string' &&
          resolveModuleGuestPathToHostMapping(guestPath)?.readOnly === true,
        append,
        position: append && typeof ioFd === 'number'
          ? Number(fsModule.fstatSync(ioFd).size ?? 0)
          : 0,
        ...(typeof guestPath === 'string' ? { guestPath } : {}),
      });
    }
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

function writeGuestUint32(ptr, value) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_FAULT;
  }

  try {
    new DataView(instanceMemory.buffer).setUint32(Number(ptr), Number(value) >>> 0, true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

function readGuestUint32(ptr) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    throw new Error('WebAssembly memory is unavailable');
  }
  return new DataView(instanceMemory.buffer).getUint32(Number(ptr), true);
}

function writeGuestUint64(ptr, value) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_FAULT;
  }

  try {
    new DataView(instanceMemory.buffer).setBigUint64(Number(ptr), BigInt(value), true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

function statTimestampNs(value) {
  const numeric = Number(value);
  return BigInt(Math.trunc((Number.isFinite(numeric) ? numeric : 0) * 1000000));
}

function writeGuestFilestat(ptr, stats, filetype = WASI_FILETYPE_REGULAR_FILE) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_FAULT;
  }

  try {
    const view = new DataView(instanceMemory.buffer);
    const offset = Number(ptr) >>> 0;
    view.setBigUint64(offset, 0n, true);
    view.setBigUint64(offset + 8, BigInt(stats?.ino ?? 0), true);
    view.setUint8(offset + 16, Number(filetype) >>> 0);
    view.setBigUint64(offset + 24, BigInt(stats?.nlink ?? 1), true);
    view.setBigUint64(offset + 32, BigInt(stats?.size ?? 0), true);
    view.setBigUint64(offset + 40, statTimestampNs(stats?.atimeMs), true);
    view.setBigUint64(offset + 48, statTimestampNs(stats?.mtimeMs), true);
    view.setBigUint64(offset + 56, statTimestampNs(stats?.ctimeMs), true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

function writeGuestFdstat(ptr, filetype, flags, rightsBase, rightsInheriting) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_FAULT;
  }

  try {
    const view = new DataView(instanceMemory.buffer);
    const offset = Number(ptr) >>> 0;
    view.setUint8(offset, Number(filetype) >>> 0);
    view.setUint16(offset + 2, Number(flags) >>> 0, true);
    view.setBigUint64(offset + 8, BigInt(rightsBase), true);
    view.setBigUint64(offset + 16, BigInt(rightsInheriting), true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

function mapSyntheticFsError(error) {
  switch (error?.code) {
    case 'EBADF':
      return WASI_ERRNO_BADF;
    case 'EACCES':
    case 'EPERM':
      return WASI_ERRNO_ACCES;
    case 'EINVAL':
      return WASI_ERRNO_INVAL;
    case 'EROFS':
      return WASI_ERRNO_ROFS;
    default:
      return WASI_ERRNO_FAULT;
  }
}

function seekGuestFileHandle(handle, offset, whence) {
  const numericWhence = Number(whence) >>> 0;
  let base;
  if (numericWhence === WASI_WHENCE_SET) {
    base = 0n;
  } else if (numericWhence === WASI_WHENCE_CUR) {
    base = BigInt(handle.position ?? 0);
  } else if (numericWhence === WASI_WHENCE_END) {
    base = BigInt(Number(fsModule.fstatSync(handle.targetFd).size ?? 0));
  } else {
    return null;
  }

  const next = base + BigInt(offset);
  if (next < 0n || next > BigInt(Number.MAX_SAFE_INTEGER)) {
    return null;
  }

  handle.position = Number(next);
  return next;
}

function createPipeHandle(kind, pipe, displayFd) {
  if (kind === 'pipe-read') {
    pipe.readHandleCount += 1;
  } else if (kind === 'pipe-write') {
    pipe.writeHandleCount += 1;
  }

  return {
    kind,
    pipe,
    displayFd: Number(displayFd) >>> 0,
    refCount: 1,
    open: true,
  };
}

function retainDelegateFd(fd) {
  const numericFd = Number(fd) >>> 0;
  delegateManagedFdRefCounts.set(numericFd, (delegateManagedFdRefCounts.get(numericFd) ?? 0) + 1);
}

function releaseDelegateFd(fd) {
  const numericFd = Number(fd) >>> 0;
  const current = delegateManagedFdRefCounts.get(numericFd);
  if (current == null) {
    return false;
  }
  if (current <= 1) {
    delegateManagedFdRefCounts.delete(numericFd);
    return true;
  }
  delegateManagedFdRefCounts.set(numericFd, current - 1);
  return false;
}

function lookupFdHandle(fd) {
  const numericFd = Number(fd) >>> 0;
  return (
    syntheticFdEntries.get(numericFd) ??
    retainedSpawnOutputHandlesByFd.get(numericFd)?.handle ??
    passthroughHandles.get(numericFd) ??
    null
  );
}

function lookupSyntheticHandleByDisplayFd(fd, expectedKind = null) {
  const numericFd = Number(fd) >>> 0;
  for (const handle of syntheticFdEntries.values()) {
    if (!handle || handle.displayFd !== numericFd) {
      continue;
    }
    if (expectedKind && handle.kind !== expectedKind) {
      continue;
    }
    return handle;
  }

  const retainedHandle = retainedSyntheticHandlesByDisplayFd.get(numericFd) ?? null;
  if (
    retainedHandle &&
    (!expectedKind || retainedHandle.kind === expectedKind)
  ) {
    return retainedHandle;
  }

  return null;
}

function retainSyntheticHandleByDisplayFd(handle) {
  if (
    handle &&
    (handle.kind === 'pipe-read' || handle.kind === 'pipe-write')
  ) {
    retainedSyntheticHandlesByDisplayFd.set(handle.displayFd >>> 0, handle);
  }
}

function releaseRetainedSyntheticHandleByDisplayFd(handle) {
  if (!handle) {
    return;
  }

  const displayFd = handle.displayFd >>> 0;
  if (retainedSyntheticHandlesByDisplayFd.get(displayFd) === handle) {
    retainedSyntheticHandlesByDisplayFd.delete(displayFd);
  }
}

function cloneFdHandle(fd) {
  const handle = lookupFdHandle(fd);
  if (!handle) {
    return null;
  }
  handle.refCount += 1;
  return handle;
}

function passthroughHandleHasCanonicalMapping(handle) {
  for (const current of passthroughHandles.values()) {
    if (current === handle) {
      return true;
    }
  }
  return false;
}

function releaseFdHandle(handle) {
  if (!handle) {
    return;
  }

  if (handle.kind === 'passthrough') {
    handle.refCount = Math.max(0, handle.refCount - 1);
    if (
      handle.refCount === 0 &&
      handle.open &&
      handle.targetFd > 2 &&
      !passthroughHandleHasCanonicalMapping(handle) &&
      releaseDelegateFd(handle.targetFd) &&
      typeof delegateManagedFdClose === 'function'
    ) {
      delegateManagedFdClose(handle.targetFd);
    }
    if (handle.refCount === 0 && handle.open && typeof handle.ioFd === 'number') {
      try {
        fsModule.closeSync(handle.ioFd);
      } catch {}
      handle.ioFd = null;
    }
    return;
  }

  if (handle.kind === 'guest-file') {
    handle.refCount = Math.max(0, handle.refCount - 1);
    if (handle.refCount === 0 && handle.open) {
      handle.open = false;
      fsModule.closeSync(handle.targetFd);
    }
    return;
  }

  handle.refCount = Math.max(0, handle.refCount - 1);
  if (handle.refCount > 0 || !handle.open) {
    return;
  }

  handle.open = false;
  if (handle.kind === 'pipe-read') {
    handle.pipe.readHandleCount = Math.max(0, handle.pipe.readHandleCount - 1);
  } else if (handle.kind === 'pipe-write') {
    handle.pipe.writeHandleCount = Math.max(0, handle.pipe.writeHandleCount - 1);
    if (handle.pipe.writeHandleCount === 0 && (handle.pipe.producers?.size ?? 0) === 0) {
      closePipeConsumers(handle.pipe);
    }
  }
}

function closeSyntheticFd(fd) {
  const numericFd = Number(fd) >>> 0;
  const handle = syntheticFdEntries.get(numericFd);
  if (!handle) {
    return false;
  }

  const shouldRetainMapping =
    ((handle.kind === 'pipe-write' && (handle.pipe.producers?.size ?? 0) > 0) ||
      (handle.kind === 'pipe-read' && (handle.pipe.consumers?.size ?? 0) > 0));
  if (shouldRetainMapping) {
    retainSyntheticHandleByDisplayFd(handle);
  }
  if (!shouldRetainMapping) {
    syntheticFdEntries.delete(numericFd);
  }
  releaseFdHandle(handle);
  if (shouldRetainMapping) {
    collectInactivePipeHandles(handle.pipe);
  }
  return true;
}

function closePassthroughFd(fd) {
  const numericFd = Number(fd) >>> 0;
  const handle = passthroughHandles.get(numericFd);
  if (!handle) {
    return false;
  }

  passthroughHandles.delete(numericFd);
  closedPassthroughFds.add(numericFd);
  if ((handle.refCount ?? 0) === 0) {
    releaseFdHandle(handle);
  }
  return true;
}

function rejectClosedPassthroughFd(fd) {
  return closedPassthroughFds.has(Number(fd) >>> 0);
}

function collectInactivePipeHandles(pipe) {
  if (!pipe) {
    return;
  }

  if (
    (pipe.readHandleCount ?? 0) > 0 ||
    (pipe.writeHandleCount ?? 0) > 0 ||
    (pipe.producers?.size ?? 0) > 0 ||
    (pipe.consumers?.size ?? 0) > 0
  ) {
    return;
  }

  for (const [fd, handle] of Array.from(syntheticFdEntries.entries())) {
    if (
      (handle.kind === 'pipe-read' || handle.kind === 'pipe-write') &&
      handle.pipe === pipe &&
      !handle.open &&
      handle.refCount === 0
    ) {
      syntheticFdEntries.delete(fd);
    }
  }

  for (const [displayFd, handle] of Array.from(retainedSyntheticHandlesByDisplayFd.entries())) {
    if (
      (handle.kind === 'pipe-read' || handle.kind === 'pipe-write') &&
      handle.pipe === pipe &&
      !handle.open &&
      handle.refCount === 0
    ) {
      retainedSyntheticHandlesByDisplayFd.delete(displayFd);
    }
  }
}

function resolveSpawnFd(fd) {
  const numericFd = Number(fd) >>> 0;
  const handle = lookupFdHandle(fd);
  if (!handle) {
    return numericFd;
  }
  if (handle.kind === 'passthrough') {
    return handle.targetFd >>> 0;
  }
  if (handle.kind === 'guest-file') {
    return numericFd;
  }
  return handle.displayFd >>> 0;
}

function spawnStdinFdIsSyntheticPipe(fd) {
  const handle =
    lookupFdHandle(fd) ?? lookupSyntheticHandleByDisplayFd(fd, 'pipe-read');
  return handle?.kind === 'pipe-read';
}

// Shell input redirects (`cmd < file`) reach proc_spawn as a plain file fd in
// stdin_fd. The child cannot share that descriptor across the spawn boundary,
// so the remaining file contents are materialized and written to the child's
// stdin pipe, exactly like POSIX children reading an inherited file fd to EOF.
// Returns null when the fd is not a readable file-backed handle so callers can
// fail loudly instead of leaving the child hanging on an open stdin pipe.
function readSpawnStdinRedirectBytes(fd) {
  const numericFd = Number(fd) >>> 0;
  const handle = lookupFdHandle(numericFd);
  if (!handle) {
    return null;
  }

  if (handle.kind === 'guest-file') {
    const chunks = [];
    let position = handle.position ?? 0;
    for (;;) {
      const buffer = Buffer.alloc(65536);
      const bytesRead = fsModule.readSync(
        handle.targetFd,
        buffer,
        0,
        buffer.length,
        position,
      );
      if (bytesRead <= 0) {
        break;
      }
      chunks.push(buffer.subarray(0, bytesRead));
      position += bytesRead;
    }
    handle.position = position;
    return Buffer.concat(chunks);
  }

  if (handle.kind === 'passthrough' && typeof handle.guestPath === 'string') {
    if (handle.guestPath === '/dev/null') {
      return Buffer.alloc(0);
    }
    const stats = fsModule.statSync(handle.guestPath);
    if (!stats.isFile()) {
      return null;
    }
    return Buffer.from(fsModule.readFileSync(handle.guestPath));
  }

  return null;
}

function retainSpawnOutputHandle(fd) {
  const numericFd = Number(fd) >>> 0;
  if (numericFd <= 2) {
    return null;
  }

  const retained = retainedSpawnOutputHandlesByFd.get(numericFd);
  if (retained) {
    retained.refCount += 1;
    retained.handle.refCount += 1;
    return { fd: numericFd, handle: retained.handle };
  }

  const handle = lookupFdHandle(numericFd);
  if (handle?.kind !== 'guest-file' && handle?.kind !== 'passthrough') {
    return null;
  }

  handle.refCount += 1;
  retainedSpawnOutputHandlesByFd.set(numericFd, { handle, refCount: 1 });
  return { fd: numericFd, handle };
}

function releaseSpawnOutputHandles(retainedHandles) {
  for (const retained of retainedHandles ?? []) {
    if (!retained || typeof retained.fd !== 'number' || !retained.handle) {
      continue;
    }
    const retainedEntry = retainedSpawnOutputHandlesByFd.get(retained.fd);
    if (retainedEntry?.handle === retained.handle) {
      retainedEntry.refCount -= 1;
      if (retainedEntry.refCount <= 0) {
        retainedSpawnOutputHandlesByFd.delete(retained.fd);
      }
    }
    releaseFdHandle(retained.handle);
  }
}

function collectGuestIovBytes(iovs, iovsLen) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    throw new Error('WebAssembly memory is not available');
  }

  const chunks = [];
  let totalLength = 0;

  for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
    const view = new DataView(instanceMemory.buffer);
    const entryOffset = (Number(iovs) >>> 0) + index * 8;
    const ptr = view.getUint32(entryOffset, true);
    const len = view.getUint32(entryOffset + 4, true);
    const chunk = readGuestBytes(ptr, len);
    chunks.push(chunk);
    totalLength += chunk.length;
  }

  return Buffer.concat(chunks, totalLength);
}

function writeBytesToGuestIovs(iovs, iovsLen, bytes) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    throw new Error('WebAssembly memory is not available');
  }

  const source = Buffer.from(bytes ?? []);
  let written = 0;

  for (let index = 0; index < (Number(iovsLen) >>> 0) && written < source.length; index += 1) {
    const view = new DataView(instanceMemory.buffer);
    const memory = new Uint8Array(instanceMemory.buffer);
    const entryOffset = (Number(iovs) >>> 0) + index * 8;
    const ptr = view.getUint32(entryOffset, true);
    const len = view.getUint32(entryOffset + 4, true);
    const remaining = source.length - written;
    const chunkLength = Math.min(len >>> 0, remaining);
    memory.set(source.subarray(written, written + chunkLength), ptr >>> 0);
    written += chunkLength;
  }

  return written >>> 0;
}

function dequeuePipeBytes(pipe, maxBytes) {
  const requested = Math.max(0, Number(maxBytes) >>> 0);
  if (requested === 0 || pipe.chunks.length === 0) {
    return Buffer.alloc(0);
  }

  const parts = [];
  let remaining = requested;
  while (remaining > 0 && pipe.chunks.length > 0) {
    const chunk = pipe.chunks[0];
    if (chunk.length <= remaining) {
      parts.push(chunk);
      pipe.chunks.shift();
      remaining -= chunk.length;
      continue;
    }

    parts.push(chunk.subarray(0, remaining));
    pipe.chunks[0] = chunk.subarray(remaining);
    remaining = 0;
  }

  return Buffer.concat(parts);
}

function enqueuePipeBytes(pipe, bytes) {
  const chunk = Buffer.from(bytes ?? []);
  if (chunk.length === 0) {
    return;
  }
  pipe.chunks.push(chunk);
}

function pipeHasReaders(pipe) {
  return (
    (pipe?.readHandleCount ?? 0) > 0 ||
    (pipe?.consumers?.size ?? 0) > 0
  );
}

function unregisterPipeProducer(pipe, producerKey) {
  if (!pipe || typeof pipe.producers?.delete !== 'function') {
    return;
  }
  pipe.producers.delete(producerKey);
  if (pipe.producers.size === 0 && (pipe.writeHandleCount ?? 0) === 0) {
    closePipeConsumers(pipe);
  }
  collectInactivePipeHandles(pipe);
}

function unregisterPipeConsumer(pipe, consumerKey) {
  if (!pipe || typeof pipe.consumers?.delete !== 'function') {
    return;
  }
  pipe.consumers.delete(consumerKey);
  collectInactivePipeHandles(pipe);
}

function unregisterChildPipeProducers(record) {
  if (!record || !record.childId) {
    return;
  }

  for (const [stream, fd, pipe] of [
    ['stdout', record.stdoutFd, record.stdoutPipe],
    ['stderr', record.stderrFd, record.stderrPipe],
  ]) {
    const outputPipe =
      pipe ??
      (() => {
        const handle =
          lookupFdHandle(fd) ?? lookupSyntheticHandleByDisplayFd(fd, 'pipe-write');
        return handle?.kind === 'pipe-write' ? handle.pipe : null;
      })();
    if (outputPipe) {
      unregisterPipeProducer(outputPipe, `${record.childId}:${stream}`);
    }
  }
}

function unregisterChildPipeConsumers(record) {
  if (!record || !record.childId) {
    return;
  }

  const inputPipe = resolveChildInputPipe(record);
  if (inputPipe) {
    unregisterPipeConsumer(inputPipe, `${record.childId}:stdin`);
  }
}

function resolveChildInputPipe(record) {
  if (!record) {
    return null;
  }

  return (
    record.stdinPipe ??
    (() => {
      const handle =
        lookupFdHandle(record.stdinFd) ??
        lookupSyntheticHandleByDisplayFd(record.stdinFd, 'pipe-read');
      return handle?.kind === 'pipe-read' ? handle.pipe : null;
    })()
  );
}

function registerPipeProducer(fd, childId, stream) {
  const handle =
    lookupFdHandle(fd) ?? lookupSyntheticHandleByDisplayFd(fd, 'pipe-write');
  if (handle?.kind !== 'pipe-write') {
    return null;
  }
  handle.pipe.producers.set(`${childId}:${stream}`, { childId, stream });
  traceHostProcess('register-producer', { fd: Number(fd) >>> 0, childId, stream, pipeId: handle.pipe.id });
  return handle.pipe;
}

function registerPipeConsumer(fd, childId, stream) {
  const handle =
    lookupFdHandle(fd) ?? lookupSyntheticHandleByDisplayFd(fd, 'pipe-read');
  if (handle?.kind !== 'pipe-read') {
    return null;
  }
  handle.pipe.consumers.set(`${childId}:${stream}`, { childId, stream });
  const shouldDeferInitialDelivery =
    stream === 'stdin' && !spawnedChildrenById.has(childId);
  traceHostProcess('register-consumer', {
    fd: Number(fd) >>> 0,
    childId,
    stream,
    pipeId: handle.pipe.id,
    deferred: shouldDeferInitialDelivery,
  });
  if (!shouldDeferInitialDelivery) {
    if (handle.pipe.chunks.length > 0) {
      flushPipeConsumers(handle.pipe);
    }
    if (handle.pipe.producers.size === 0 && (handle.pipe.writeHandleCount ?? 0) === 0) {
      closePipeConsumers(handle.pipe);
    }
  }
  return handle.pipe;
}

function flushPipeConsumers(pipe) {
  if (
    !pipe ||
    typeof pipe.consumers?.size !== 'number' ||
    !Array.isArray(pipe.chunks) ||
    pipe.consumers.size === 0 ||
    pipe.chunks.length === 0
  ) {
    return false;
  }

  let flushed = false;
  while (pipe.chunks.length > 0) {
    const chunk = pipe.chunks[0];
    if (!chunk || chunk.length === 0) {
      pipe.chunks.shift();
      continue;
    }
    let shouldRetryChunk = false;
    for (const [consumerKey, consumer] of Array.from(pipe.consumers.entries())) {
      try {
        callSyncRpc('child_process.write_stdin', [consumer.childId, chunk]);
        traceHostProcess('flush-consumer-write', {
          pipeId: pipe.id,
          childId: consumer.childId,
          bytes: chunk.length,
        });
        flushed = true;
      } catch (error) {
        if (spawnedChildrenById.has(consumer?.childId) && isChildProcessGoneError(error)) {
          shouldRetryChunk = true;
          continue;
        }
        traceHostProcess('flush-consumer-write-failed', {
          pipeId: pipe.id,
          childId: consumer?.childId ?? null,
        });
        pipe.consumers.delete(consumerKey);
      }
    }
    if (shouldRetryChunk) {
      break;
    }
    pipe.chunks.shift();
  }

  return flushed;
}

function closePipeConsumers(pipe) {
  if (!pipe || typeof pipe.consumers?.size !== 'number' || pipe.consumers.size === 0) {
    return false;
  }

  let closed = false;
  for (const [consumerKey, consumer] of Array.from(pipe.consumers.entries())) {
    try {
      callSyncRpc('child_process.close_stdin', [consumer.childId]);
      traceHostProcess('close-consumer-stdin', {
        pipeId: pipe.id,
        childId: consumer.childId,
      });
      closed = true;
    } catch (error) {
      if (spawnedChildrenById.has(consumer?.childId) && isChildProcessGoneError(error)) {
        continue;
      }
      traceHostProcess('close-consumer-stdin-failed', {
        pipeId: pipe.id,
        childId: consumer?.childId ?? null,
      });
      // Ignore close errors during teardown.
    }
    pipe.consumers.delete(consumerKey);
  }

  collectInactivePipeHandles(pipe);
  return closed;
}

function consumeSpawnOutputFd(fd) {
  const numericFd = Number(fd) >>> 0;
  const handle = syntheticFdEntries.get(numericFd);
  if (handle?.kind === 'pipe-write' && handle.open) {
    // Release the guest-owned write handle but retain the fd mapping so later
    // child stdout/stderr events can still route into the synthetic pipe.
    releaseFdHandle(handle);
  }
}

function routeChunkToFd(fd, bytes) {
  const numericFd = Number(fd) >>> 0;
  const handle =
    lookupFdHandle(numericFd) ??
    lookupSyntheticHandleByDisplayFd(numericFd) ??
    (typeof globalThis.lookupFdHandle === 'function'
      ? globalThis.lookupFdHandle(numericFd)
      : null);
  traceHostProcess('route-chunk', {
    fd: numericFd,
    handleKind: handle?.kind ?? null,
    bytes: Buffer.from(bytes ?? []).length,
  });
  if (!handle) {
    if (isStdioFd(numericFd) && routeChunkToDelegateFd(numericFd, bytes)) {
      return;
    }
    if (isStdioFd(numericFd)) {
      writeToStdioFd(numericFd, Buffer.from(bytes ?? []));
      return;
    }
    if (numericFd > 2 && routeChunkToDelegateFd(numericFd, bytes)) {
      return;
    }
    writeSync(numericFd, bytes);
    return;
  }

  if (handle.kind === 'passthrough') {
    if (handle.append === true && typeof handle.guestPath === 'string') {
      fsModule.appendFileSync(handle.guestPath, Buffer.from(bytes ?? []));
      return;
    }
    if (routeChunkToDelegateFd(handle.targetFd, bytes)) {
      return;
    }
    if (isStdioFd(handle.targetFd)) {
      writeToStdioFd(handle.targetFd, Buffer.from(bytes ?? []));
      return;
    }
    writeSync(handle.targetFd, bytes);
    return;
  }

  if (handle.kind === 'host-passthrough') {
    if (routeChunkToDelegateFd(handle.displayFd ?? numericFd, bytes)) {
      return;
    }
    if (routeChunkToDelegateFd(handle.targetFd, bytes)) {
      return;
    }
    if (isStdioFd(handle.targetFd)) {
      writeToStdioFd(handle.targetFd, Buffer.from(bytes ?? []));
      return;
    }
    writeSync(handle.targetFd, bytes);
    return;
  }

  if (handle.kind === 'pipe-write') {
    enqueuePipeBytes(handle.pipe, bytes);
    flushPipeConsumers(handle.pipe);
    return;
  }

  if (handle.kind === 'guest-file') {
    writeBytesToGuestFileHandle(handle, Buffer.from(bytes ?? []));
    return;
  }

  throw new Error(`bad file descriptor ${numericFd}`);
}

function writeBytesToGuestFileHandle(handle, bytes) {
  const chunk = Buffer.from(bytes ?? []);
  const position = handle.append ? null : (handle.position ?? 0);
  const written = fsModule.writeSync(
    handle.targetFd,
    chunk,
    0,
    chunk.length,
    position,
  );
  if (handle.append) {
    handle.position = Number(fsModule.fstatSync(handle.targetFd).size ?? 0);
  } else {
    handle.position = (handle.position ?? 0) + written;
  }
  return written;
}

function routeChunkToDelegateFd(fd, bytes) {
  if (!(instanceMemory instanceof WebAssembly.Memory) || typeof delegateManagedFdWrite !== 'function') {
    return false;
  }

  const chunk = Buffer.from(bytes ?? []);
  const needed = 8 + chunk.length + 4;
  if (
    delegateWriteScratch.capacity < needed ||
    delegateWriteScratch.base + needed > instanceMemory.buffer.byteLength
  ) {
    const pages = Math.max(1, Math.ceil(needed / 65536));
    const basePage = instanceMemory.grow(pages);
    delegateWriteScratch = {
      base: basePage * 65536,
      capacity: pages * 65536,
    };
  }

  try {
    const iovsPtr = delegateWriteScratch.base;
    const dataPtr = iovsPtr + 8;
    const nwrittenPtr = dataPtr + chunk.length;
    const memory = new Uint8Array(instanceMemory.buffer);
    const view = new DataView(instanceMemory.buffer);
    memory.set(chunk, dataPtr);
    view.setUint32(iovsPtr, dataPtr, true);
    view.setUint32(iovsPtr + 4, chunk.length, true);
    const result = delegateManagedFdWrite(fd, iovsPtr, 1, nwrittenPtr);
    traceHostProcess('route-chunk-delegate', {
      fd: Number(fd) >>> 0,
      bytes: chunk.length,
      result,
    });
    return result === WASI_ERRNO_SUCCESS;
  } catch (error) {
    traceHostProcess('route-chunk-delegate-error', {
      fd: Number(fd) >>> 0,
      bytes: chunk.length,
      message: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
}

function finalizeChildExit(record, exitCode, signal) {
  const status =
    signal == null
      ? (Number(exitCode ?? 1) & 0xff)
      : 128 + (signalNumberFromName(signal) & 0x7f);
  record.exitStatus = status;
  for (const fd of record.delegateRetainedFds ?? []) {
    if (releaseDelegateFd(fd) && typeof delegateManagedFdClose === 'function') {
      delegateManagedFdClose(fd);
    }
  }
  releaseSpawnOutputHandles(record.retainedSpawnOutputHandles);
  unregisterChildPipeProducers(record);
  unregisterChildPipeConsumers(record);
  return status;
}

function pollChildEvent(record, waitMs) {
  if (Array.isArray(record?.pendingEvents) && record.pendingEvents.length > 0) {
    return record.pendingEvents.shift() ?? null;
  }
  if (record?.synthetic) {
    return null;
  }
  return callSyncRpc('child_process.poll', [record.childId, waitMs]);
}

function isChildProcessGoneError(error) {
  return (
    (error instanceof Error && error.code === 'ECHILD') ||
    (error instanceof Error &&
      typeof error.message === 'string' &&
      error.message.startsWith('ECHILD:'))
  );
}

function resolveSyntheticGuestPath(value, fromGuestDir = '/') {
  if (typeof value !== 'string') {
    return value;
  }
  if (value.startsWith('file:')) {
    try {
      return path.posix.normalize(new URL(value).pathname);
    } catch {
      return value;
    }
  }
  if (value.startsWith('/')) {
    return path.posix.normalize(value);
  }
  if (value.startsWith('./') || value.startsWith('../')) {
    return path.posix.normalize(path.posix.join(fromGuestDir, value));
  }
  return value;
}

function resolveSyntheticHostPath(value, fromGuestDir = '/') {
  const mapping = resolveSyntheticHostMapping(value, fromGuestDir);
  return mapping?.hostPath ?? null;
}

function resolveSyntheticHostMapping(value, fromGuestDir = '/') {
  const guestPath = resolveSyntheticGuestPath(value, fromGuestDir);
  if (typeof guestPath !== 'string') {
    return null;
  }
  return resolveModuleGuestPathToHostMapping(guestPath);
}

function chmodMappedGuestPath(guestPath, hostPath, mode) {
  fsModule.chmodSync(hostPath, mode);
  try {
    if (typeof guestPath === 'string' && guestPath.length > 0) {
      fsModule.chmodSync(guestPath, mode);
    }
  } catch {
    // Best effort: host-mapped paths may not also exist as direct kernel paths.
  }
}

function maybeCreateSyntheticCommandResult(command, args, cwd) {
  const basename = path.posix.basename(String(command || ''));

  if (basename === 'chmod') {
    if (args.length < 2 || !args.every((arg) => typeof arg === 'string')) {
      return null;
    }
    const modeArg = args[0];
    if (!/^[0-7]{3,4}$/.test(modeArg)) {
      return null;
    }
    const mode = Number.parseInt(modeArg, 8) >>> 0;
    try {
      for (const targetArg of args.slice(1)) {
        const guestPath = resolveSyntheticGuestPath(targetArg, cwd || '/');
        const mapping = resolveSyntheticHostMapping(targetArg, cwd || '/');
        if (!mapping || typeof mapping.hostPath !== 'string') {
          throw new Error(`No such file or directory: ${targetArg}`);
        }
        if (mapping.readOnly) {
          const error = new Error(`Read-only file system: ${targetArg}`);
          error.code = 'EROFS';
          throw error;
        }
        chmodMappedGuestPath(guestPath, mapping.hostPath, mode);
      }
      return { exitCode: 0, stdout: '', stderr: '' };
    } catch (error) {
      return {
        exitCode: 1,
        stdout: '',
        stderr: `chmod: ${error instanceof Error ? error.message : String(error)}\n`,
      };
    }
  }

  if (basename === 'stat') {
    if (
      args.length === 3 &&
      args[0] === '-c' &&
      (args[1] === '%a' || args[1] === '"%a"') &&
      typeof args[2] === 'string'
    ) {
      try {
        const hostPath = resolveSyntheticHostPath(args[2], cwd || '/');
        if (typeof hostPath !== 'string') {
          return null;
        }
        const stat = fsModule.statSync(hostPath);
        const mode = Number(stat?.mode) >>> 0;
        return {
          exitCode: 0,
          stdout: `${(mode & 0o777).toString(8)}\n`,
          stderr: '',
        };
      } catch {
        return null;
      }
    }
    return null;
  }

  return null;
}

function createSyntheticChildRecord(result, stdinTarget, stdoutTarget, stderrTarget) {
  const pid = nextSyntheticChildPid++;
  const childId = `synthetic-child-${pid}`;
  const pendingEvents = [{
    type: 'exit',
    exitCode: Number(result?.exitCode ?? 1) >>> 0,
    signal: null,
  }];

  return {
    childId,
    pid,
    stdinFd: stdinTarget,
    stdoutFd: stdoutTarget,
    stderrFd: stderrTarget,
    stdinPipe: null,
    stdoutPipe: null,
    stderrPipe: null,
    delegateRetainedFds: [],
    exitStatus: null,
    pendingEvents,
    synthetic: true,
  };
}

function emitSyntheticCommandOutput(record, stdoutFd, stderrFd, result) {
  const syntheticOutputs = [
    ['stdout', stdoutFd, record.stdoutFd, result?.stdout],
    ['stderr', stderrFd, record.stderrFd, result?.stderr],
  ];

  for (const [stream, rawFd, targetFd, value] of syntheticOutputs) {
    const text = typeof value === 'string' ? value : '';
    const pipe = registerPipeProducer(targetFd, record.childId, stream);
    consumeSpawnOutputFd(rawFd);
    if (text.length > 0 && targetFd !== 0xffffffff) {
      routeChunkToFd(targetFd, Buffer.from(text, 'utf8'));
    }
    if (pipe) {
      unregisterPipeProducer(pipe, `${record.childId}:${stream}`);
    }
  }
}

function reapSpawnedChild(record) {
  if (!record) {
    return;
  }

  spawnedChildren.delete(record.pid);
  if (typeof record.childId === 'string' && record.childId.length > 0) {
    spawnedChildrenById.delete(record.childId);
  }
}

function processChildEvent(record, event) {
  if (!event) {
    return false;
  }
  traceHostProcess('child-event', {
    childId: record?.childId ?? null,
    pid: record?.pid ?? null,
    type: event.type,
    exitCode: event.exitCode ?? null,
    signal: event.signal ?? null,
  });

  if (event.type === 'stdout' && record.stdoutFd !== 0xffffffff) {
    const chunk = decodeSyncRpcValue(event.data);
    if (chunk?.length > 0) {
      routeChunkToFd(record.stdoutFd, chunk);
    }
    return true;
  }

  if (event.type === 'stderr' && record.stderrFd !== 0xffffffff) {
    const chunk = decodeSyncRpcValue(event.data);
    if (chunk?.length > 0) {
      routeChunkToFd(record.stderrFd, chunk);
    }
    return true;
  }

  if (event.type === 'signal') {
    dispatchWasmSignal(
      typeof event.number === 'number' ? event.number : signalNumberFromName(event.signal),
    );
    return true;
  }

  if (event.type === 'exit') {
    const exitCode =
      typeof event.exitCode === 'number' ? Math.trunc(event.exitCode) : null;
    const signal =
      typeof event.signal === 'string' ? event.signal : null;
    while (true) {
      let trailingEvent = null;
      try {
        trailingEvent = pollChildEvent(record, 0);
      } catch (error) {
        if (isChildProcessGoneError(error)) {
          break;
        }
        throw error;
      }
      if (!trailingEvent) {
        break;
      }
      if (!processChildEvent(record, trailingEvent)) {
        break;
      }
    }
    finalizeChildExit(record, exitCode, signal);
    return true;
  }

  return false;
}

function pumpPipeProducers(pipe, waitMs) {
  let processed = false;
  for (const [producerKey, producer] of Array.from(pipe.producers.entries())) {
    const record = spawnedChildrenById.get(producer.childId);
    if (!record) {
      unregisterPipeProducer(pipe, producerKey);
      continue;
    }
    if (typeof record.exitStatus === 'number') {
      unregisterPipeProducer(pipe, producerKey);
      continue;
    }

    processed = pumpChildInputPipe(record, 0) || processed;

    const event = pollChildEvent(record, waitMs);
    if (!event) {
      continue;
    }

    processed = true;
    processChildEvent(record, event);
  }

  return processed;
}

function pumpChildInputPipe(record, waitMs) {
  const inputPipe = resolveChildInputPipe(record);
  if (!inputPipe) {
    traceHostProcess('pump-child-input-skip-no-pipe', {
      childId: record?.childId ?? null,
    });
    return false;
  }
  if (record.pumpingInputPipe === true) {
    return false;
  }
  record.pumpingInputPipe = true;
  try {
    const stdinReadyAt = Number(record?.stdinReadyAtMs) || 0;
    if (stdinReadyAt > Date.now()) {
      traceHostProcess('pump-child-input-deferred', {
        childId: record?.childId ?? null,
        waitMs: Number(waitMs) >>> 0,
        stdinReadyAt,
        now: Date.now(),
        chunkCount: inputPipe.chunks.length,
        writeHandleCount: inputPipe.writeHandleCount ?? null,
        producerCount: inputPipe.producers?.size ?? null,
      });
      return false;
    }

    let progressed = false;
    traceHostProcess('pump-child-input-begin', {
      childId: record?.childId ?? null,
      waitMs: Number(waitMs) >>> 0,
      chunkCount: inputPipe.chunks.length,
      writeHandleCount: inputPipe.writeHandleCount ?? null,
      producerCount: inputPipe.producers?.size ?? null,
    });
    if (inputPipe.chunks.length > 0) {
      progressed = flushPipeConsumers(inputPipe) || progressed;
    }

    if (inputPipe.producers.size === 0 && (inputPipe.writeHandleCount ?? 0) === 0) {
      return closePipeConsumers(inputPipe) || progressed;
    }

    const pumped = pumpPipeProducers(inputPipe, waitMs);
    progressed = pumped || progressed;
    if (inputPipe.chunks.length > 0) {
      progressed = flushPipeConsumers(inputPipe) || progressed;
    }
    if (inputPipe.producers.size === 0 && (inputPipe.writeHandleCount ?? 0) === 0) {
      progressed = closePipeConsumers(inputPipe) || progressed;
    }

    return progressed;
  } finally {
    record.pumpingInputPipe = false;
  }
}

function pumpSpawnedChildren(waitMs) {
  let progressed = false;
  for (const record of Array.from(spawnedChildren.values())) {
    if (!record || typeof record.exitStatus === 'number') {
      continue;
    }
    try {
      const event = pollChildEvent(record, waitMs);
      if (event) {
        processChildEvent(record, event);
        progressed = true;
      }
      progressed = pumpChildInputPipe(record, 0) || progressed;
    } catch (error) {
      if (!isChildProcessGoneError(error)) {
        throw error;
      }
    }
  }
  return progressed;
}

function encodeGuestBytes(value) {
  return new TextEncoder().encode(String(value));
}

function readGuestBytes(ptr, len) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    throw new Error('WebAssembly memory is not available');
  }

  const start = Number(ptr) >>> 0;
  const length = Number(len) >>> 0;
  return Buffer.from(new Uint8Array(instanceMemory.buffer, start, length));
}

function readGuestString(ptr, len) {
  return readGuestBytes(ptr, len).toString('utf8');
}

function decodeNullSeparatedStrings(buffer) {
  if (!buffer || buffer.length === 0) {
    return [];
  }

  return buffer
    .toString('utf8')
    .split('\0')
    .filter((entry) => entry.length > 0);
}

function parseSerializedEnv(buffer) {
  const env = {};
  for (const entry of decodeNullSeparatedStrings(buffer)) {
    const delimiter = entry.indexOf('=');
    if (delimiter <= 0) {
      continue;
    }
    env[entry.slice(0, delimiter)] = entry.slice(delimiter + 1);
  }
  return env;
}

function encodeSyncRpcValue(value) {
  if (
    value == null ||
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean'
  ) {
    return value;
  }

  if (typeof Buffer === 'function' && Buffer.isBuffer(value)) {
    return {
      __agentOSType: 'bytes',
      base64: value.toString('base64'),
    };
  }

  if (ArrayBuffer.isView(value)) {
    return {
      __agentOSType: 'bytes',
      base64: Buffer.from(value.buffer, value.byteOffset, value.byteLength).toString('base64'),
    };
  }

  if (value instanceof ArrayBuffer) {
    return {
      __agentOSType: 'bytes',
      base64: Buffer.from(value).toString('base64'),
    };
  }

  if (Array.isArray(value)) {
    return value.map((entry) => encodeSyncRpcValue(entry));
  }

  if (typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, encodeSyncRpcValue(entry)]),
    );
  }

  return String(value);
}

function decodeSyncRpcValue(value) {
  if (Array.isArray(value)) {
    return value.map((entry) => decodeSyncRpcValue(entry));
  }

  if (Buffer.isBuffer(value)) {
    return value;
  }

  if (ArrayBuffer.isView(value)) {
    return Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  }

  if (value && typeof value === 'object') {
    if (value.__type === 'Buffer' && typeof value.data === 'string') {
      return Buffer.from(value.data, 'base64');
    }

    if (value.__agentOSType === 'bytes' && typeof value.base64 === 'string') {
      return Buffer.from(value.base64, 'base64');
    }

    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, decodeSyncRpcValue(entry)]),
    );
  }

  return value;
}

function readSyncRpcLine() {
  while (true) {
    const newlineIndex = syncRpcResponseBuffer.indexOf('\n');
    if (newlineIndex >= 0) {
      const line = syncRpcResponseBuffer.slice(0, newlineIndex);
      syncRpcResponseBuffer = syncRpcResponseBuffer.slice(newlineIndex + 1);
      return line;
    }

    const chunk = Buffer.alloc(4096);
    const bytesRead = readSync(NODE_SYNC_RPC_RESPONSE_FD, chunk, 0, chunk.length, null);
    if (bytesRead === 0) {
      throw new Error('secure-exec WASM sync RPC response channel closed unexpectedly');
    }
    syncRpcResponseBuffer += chunk.subarray(0, bytesRead).toString('utf8');
  }
}

const pendingWasmSignals = [];

function callSyncRpc(method, args = []) {
  if (
    globalThis.__agentOSSyncRpc &&
    typeof globalThis.__agentOSSyncRpc.callSync === 'function'
  ) {
    const startedNs = __agentOSWasmNowNs();
    try {
      return globalThis.__agentOSSyncRpc.callSync(method, args);
    } finally {
      __agentOSWasiRecordSyncRpc(method, 'glue', startedNs);
    }
  }

  if (!NODE_SYNC_RPC_ENABLE || NODE_SYNC_RPC_REQUEST_FD == null || NODE_SYNC_RPC_RESPONSE_FD == null) {
    const error = new Error(`secure-exec WASM sync RPC is unavailable for ${method}`);
    error.code = 'ERR_AGENTOS_WASM_SYNC_RPC_UNAVAILABLE';
    throw error;
  }

  const startedNs = __agentOSWasmNowNs();
  try {
    const payload = JSON.stringify({
      id: nextSyncRpcId++,
      method,
      args: encodeSyncRpcValue(args),
    });
    writeSync(NODE_SYNC_RPC_REQUEST_FD, `${payload}\n`);

    const response = JSON.parse(readSyncRpcLine());
    if (response?.ok) {
      return decodeSyncRpcValue(response.result);
    }

    const error = new Error(
      response?.error?.message || `secure-exec WASM sync RPC ${method} failed`,
    );
    if (typeof response?.error?.code === 'string') {
      error.code = response.error.code;
    }
    throw error;
  } finally {
    __agentOSWasiRecordSyncRpc(method, 'pipe', startedNs);
  }
}

const hostNetSockets = new Map();
let nextHostNetSocketFd = 0x40000000;
const HOST_NET_TIMEOUT_SENTINEL = '__agentos_net_timeout__';

function getHostNetSocket(fd) {
  return hostNetSockets.get(Number(fd) >>> 0) ?? null;
}

function dequeueHostNetBytes(socket, maxBytes) {
  const requested = Math.max(0, Number(maxBytes) >>> 0);
  if (requested === 0 || socket.readChunks.length === 0) {
    return Buffer.alloc(0);
  }

  const parts = [];
  let remaining = requested;
  while (remaining > 0 && socket.readChunks.length > 0) {
    const chunk = socket.readChunks[0];
    if (chunk.length <= remaining) {
      parts.push(chunk);
      socket.readChunks.shift();
      remaining -= chunk.length;
      continue;
    }

    parts.push(chunk.subarray(0, remaining));
    socket.readChunks[0] = chunk.subarray(remaining);
    remaining = 0;
  }

  return Buffer.concat(parts);
}

function pollHostNetSocket(socket, waitMs) {
  if (!socket?.socketId || socket.closed) {
    return null;
  }

  const event = callSyncRpc('net.poll', [socket.socketId, Math.max(0, Number(waitMs) >>> 0)]);
  if (!event) {
    return null;
  }

  if (event.type === 'data') {
    const chunk = decodeSyncRpcValue(event.data);
    if (chunk?.length > 0) {
      socket.readChunks.push(Buffer.from(chunk));
    }
    return event;
  }

  if (event.type === 'end' || event.type === 'close') {
    socket.readableEnded = true;
    if (event.type === 'close') {
      socket.closed = true;
      socket.socketId = null;
    }
    return event;
  }

  if (event.type === 'error') {
    socket.lastError = String(event.message || event.code || 'socket error');
    socket.closed = true;
    socket.socketId = null;
    return event;
  }

  return event;
}

function parseHostNetAddress(raw) {
  const value = String(raw ?? '').trim();
  if (!value) {
    throw new Error('host_net address is required');
  }

  if (value.startsWith('[')) {
    const end = value.indexOf(']');
    if (end < 0 || value.charCodeAt(end + 1) !== 58) {
      throw new Error(`invalid host_net address ${value}`);
    }
    return {
      host: value.slice(1, end),
      port: Number.parseInt(value.slice(end + 2), 10),
    };
  }

  const separator = value.lastIndexOf(':');
  if (separator <= 0 || separator === value.length - 1) {
    throw new Error(`invalid host_net address ${value}`);
  }

  return {
    host: value.slice(0, separator),
    port: Number.parseInt(value.slice(separator + 1), 10),
  };
}

function parseHostNetListenAddress(raw) {
  const value = String(raw ?? '').trim();
  if (!value) {
    throw new Error('host_net listen address is required');
  }
  if (value.startsWith('/')) {
    return { path: value };
  }
  const address = parseHostNetAddress(value);
  return { host: address.host, port: address.port };
}

function normalizeHostNetAddressInfo(address, port) {
  const host = String(address ?? '');
  const numericPort = Number(port);
  if (!host || !Number.isInteger(numericPort) || numericPort < 0 || numericPort > 65535) {
    return null;
  }
  return { address: host, port: numericPort };
}

function formatHostNetAddressInfo(info) {
  const address = String(info?.address ?? '');
  const port = Number(info?.port);
  if (!address || !Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error('host_net socket address is incomplete');
  }
  return `${address}:${port}`;
}

const HOST_NET_AF_INET = 2;
const HOST_NET_AF_INET6 = 10;
const HOST_NET_SOCK_DGRAM = 5;
const HOST_NET_SOCKET_TYPE_MASK = 0xf;
const HOST_NET_SOL_SOCKET = 1;
const HOST_NET_WASI_SOL_SOCKET = 0x7fffffff;
const HOST_NET_SO_ERROR = 4;
const HOST_NET_SO_RCVTIMEO_64 = 20;
const HOST_NET_SO_RCVTIMEO_32 = 66;
const HOST_NET_TIMEVAL_BYTES = 16;

function hostNetSocketBaseType(socket) {
  return Number(socket?.sockType ?? 0) & HOST_NET_SOCKET_TYPE_MASK;
}

function hostNetSockoptKind(level, optname, optvalLen) {
  const normalizedLevel = Number(level) >>> 0;
  const normalizedOptname = Number(optname) >>> 0;
  const normalizedOptvalLen = Number(optvalLen) >>> 0;
  if (
    normalizedLevel !== HOST_NET_SOL_SOCKET &&
    normalizedLevel !== HOST_NET_WASI_SOL_SOCKET
  ) {
    return null;
  }
  if (normalizedOptvalLen !== HOST_NET_TIMEVAL_BYTES) {
    return null;
  }
  if (
    normalizedOptname === HOST_NET_SO_RCVTIMEO_64 ||
    normalizedOptname === HOST_NET_SO_RCVTIMEO_32
  ) {
    return 'recv-timeout';
  }
  return null;
}

function parseHostNetTimevalMs(bytes) {
  if (bytes.byteLength !== HOST_NET_TIMEVAL_BYTES) {
    return null;
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const seconds = view.getBigInt64(0, true);
  const microseconds = view.getBigInt64(8, true);
  if (seconds < 0n || microseconds < 0n || microseconds > 999999n) {
    return null;
  }
  if (seconds === 0n && microseconds === 0n) {
    return null;
  }
  const milliseconds = seconds * 1000n + (microseconds + 999n) / 1000n;
  if (milliseconds > BigInt(Number.MAX_SAFE_INTEGER)) {
    return null;
  }
  return Number(milliseconds);
}

function ensureHostNetUdpSocket(socket) {
  if (!socket || socket.closed || hostNetSocketBaseType(socket) !== HOST_NET_SOCK_DGRAM) {
    return null;
  }
  if (socket.udpSocketId) {
    return socket.udpSocketId;
  }

  const type = socket.domain === HOST_NET_AF_INET6 ? 'udp6' : 'udp4';
  const result = callSyncRpc('dgram.createSocket', [{ type }]);
  if (!result || typeof result.socketId !== 'string') {
    throw new Error('host_net dgram socket creation failed');
  }
  socket.udpSocketId = result.socketId;
  return socket.udpSocketId;
}

function signalNumberFromName(signal) {
  const mapped = LINUX_SIGNAL_NAMES.indexOf(String(signal));
  if (mapped > 0) {
    return mapped;
  }
  if (String(signal).startsWith('SIG')) {
    const numeric = Number.parseInt(String(signal).slice(3), 10);
    return Number.isInteger(numeric) ? numeric : 15;
  }
  return 15;
}

function signalNameFromNumber(signal) {
  const numeric = Number(signal) >>> 0;
  return LINUX_SIGNAL_NAMES[numeric] ?? `SIG${numeric}`;
}

const LINUX_SIGNAL_NAMES = [
  null,
  'SIGHUP',
  'SIGINT',
  'SIGQUIT',
  'SIGILL',
  'SIGTRAP',
  'SIGABRT',
  'SIGBUS',
  'SIGFPE',
  'SIGKILL',
  'SIGUSR1',
  'SIGSEGV',
  'SIGUSR2',
  'SIGPIPE',
  'SIGALRM',
  'SIGTERM',
  null,
  'SIGCHLD',
  'SIGCONT',
  'SIGSTOP',
  'SIGTSTP',
  'SIGTTIN',
  'SIGTTOU',
  'SIGURG',
  'SIGXCPU',
  'SIGXFSZ',
  'SIGVTALRM',
  'SIGPROF',
  'SIGWINCH',
  'SIGIO',
  'SIGPWR',
  'SIGSYS',
];

function writeGuestBytes(ptr, maxLen, bytes, actualLenPtr) {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return WASI_ERRNO_FAULT;
  }

  try {
    const requestedLength = Number(maxLen) >>> 0;
    const memory = new Uint8Array(instanceMemory.buffer);
    const written = Math.min(requestedLength, bytes.byteLength);
    memory.set(bytes.subarray(0, written), Number(ptr));
    return writeGuestUint32(actualLenPtr, written);
  } catch {
    return WASI_ERRNO_FAULT;
  }
}

// Perform a single NON-BLOCKING accept on a listening host_net socket. On success it
// registers the accepted connection as a new host_net socket and returns
// { acceptedFd, address } (address is a Buffer: "host:port" for TCP, the peer path for
// AF_UNIX). Returns null when no connection is currently pending. Used by both net_poll
// (to report accurate listener readiness) and net_accept (non-blocking semantics) so the
// server never blocks inside accept() and starves already-connected clients.
function tryHostNetAcceptOnce(socket) {
  let result = callSyncRpc('net.server_accept', [socket.serverId]);
  if (!result || result === HOST_NET_TIMEOUT_SENTINEL) {
    return null;
  }
  if (typeof result === 'string') {
    result = JSON.parse(result);
  }
  if (!result || typeof result.socketId !== 'string') {
    return null;
  }

  const acceptedFd = nextHostNetSocketFd++;
  hostNetSockets.set(acceptedFd, {
    domain: socket.domain,
    sockType: socket.sockType,
    protocol: socket.protocol,
    bindOptions: null,
    localInfo: normalizeHostNetAddressInfo(result.info?.localAddress, result.info?.localPort),
    localReservation: null,
    remoteInfo: normalizeHostNetAddressInfo(result.info?.remoteAddress, result.info?.remotePort),
    serverId: null,
    socketId: result.socketId,
    udpSocketId: null,
    recvTimeoutMs: socket.recvTimeoutMs,
    readChunks: [],
    readableEnded: false,
    closed: false,
    lastError: null,
  });

  let address;
  if (result.info?.remoteAddress != null && result.info?.remotePort != null) {
    address = Buffer.from(formatHostNetAddressInfo({
      address: result.info.remoteAddress,
      port: result.info.remotePort,
    }), 'utf8');
  } else {
    address = Buffer.from(String(result.info?.remotePath ?? ''), 'utf8');
  }
  return { acceptedFd, address };
}

const hostNetImport = {
  // Poll an array of pollfd entries (8 bytes each: i32 fd, i16 events, i16 revents).
  // Connected sockets report POLLIN when data is queued; listening sockets report POLLIN
  // only when a connection is actually pending (a buffered non-blocking accept), so the
  // server's WaitForSomething does not spin forever inside a blocking accept().
  // POLLOUT is always writable.
  net_poll(fdsPtr, nfds, timeoutMs, retReadyPtr) {
    const n = Number(nfds) >>> 0;
    const base0 = Number(fdsPtr) >>> 0;
    // The patched wasi sysroot's effective poll bits (bits/poll.h): POLLIN=POLLRDNORM=0x1,
    // POLLOUT=POLLWRNORM=0x2 (NOT the 0x004 in legacy poll.h). Guests (X server + libxcb) use
    // these, so net_poll must match or POLLOUT readiness is never reported and writers block.
    const POLLIN = 0x001;
    const POLLOUT = 0x002;
    const POLLERR = 0x008;
    const POLLHUP = 0x010;
    const POLLNVAL = 0x020;
    const t = Number(timeoutMs) | 0;
    const deadline = t < 0 ? null : Date.now() + Math.max(0, t);
    const kernelManagedStdio =
      KERNEL_STDIO_SYNC_RPC ||
      (typeof process?.env?.AGENTOS_SANDBOX_ROOT === 'string' &&
        process.env.AGENTOS_SANDBOX_ROOT.length > 0);
    try {
      while (true) {
        dispatchPendingWasmSignals();
        const view = new DataView(instanceMemory.buffer);
        let ready = 0;
        // fds the kernel owns (PTY/pipe stdio in sidecar-managed mode): their readiness
        // comes from a batched __kernel_poll below, which doubles as the wait slice.
        const kernelTargets = [];
        const kernelEntries = [];
        for (let i = 0; i < n; i++) {
          const base = base0 + i * 8;
          const fd = view.getInt32(base, true);
          const events = view.getUint16(base + 4, true);
          let revents = 0;
          const socket = getHostNetSocket(fd);
          const handle = fd >= 0 ? lookupFdHandle(fd >>> 0) : undefined;
          if (socket && !socket.closed) {
            if (socket.serverId) {
              if (events & POLLIN) {
                // Report the listener readable only when a connection is actually pending.
                if (!socket.pendingAccepts) socket.pendingAccepts = [];
                if (socket.pendingAccepts.length === 0) {
                  const accepted = tryHostNetAcceptOnce(socket);
                  if (accepted) socket.pendingAccepts.push(accepted);
                }
                if (socket.pendingAccepts.length > 0) revents |= POLLIN;
              }
            } else if (socket.socketId) {
              if (events & POLLIN && socket.readChunks && socket.readChunks.length > 0) {
                revents |= POLLIN;
              }
              if (events & POLLOUT) revents |= POLLOUT;
            }
          } else if (handle?.kind === 'pipe-read') {
            if (events & POLLIN) {
              pumpPipeProducers(handle.pipe, 0);
              if (handle.pipe.chunks.length > 0) {
                revents |= POLLIN;
              } else if (
                handle.pipe.writeHandleCount === 0 &&
                handle.pipe.producers.size === 0
              ) {
                revents |= POLLHUP;
              }
            }
          } else if (handle?.kind === 'pipe-write') {
            if (events & POLLOUT) revents |= POLLOUT;
          } else if (
            fd >= 0 &&
            fd <= 2 &&
            kernelManagedStdio &&
            (!handle || (handle.kind === 'passthrough' && handle.targetFd === fd))
          ) {
            // Kernel-managed stdio (PTY slave / stdio pipes): ask the kernel, like a
            // native poll(2) on the terminal fd.
            kernelTargets.push({
              fd,
              events:
                ((events & POLLIN) !== 0 ? KERNEL_POLLIN : 0) |
                ((events & POLLOUT) !== 0 ? KERNEL_POLLOUT : 0),
            });
            kernelEntries.push({ base, fd, events });
          } else if (handle) {
            // Regular files / other VFS-backed fds: always ready, as on Linux.
            revents |= events & (POLLIN | POLLOUT);
          } else if (fd >= 0 && fd <= 2) {
            // Non-kernel-managed stdio (plain runner stdio): report requested
            // readiness rather than blocking a guest forever on fds we cannot wait on.
            revents |= events & (POLLIN | POLLOUT);
          } else if (fd >= 0) {
            revents |= POLLNVAL;
          }
          view.setUint16(base + 6, revents, true);
          if (revents) ready++;
        }

        if (kernelTargets.length > 0) {
          // If something is already ready (or this is a non-blocking poll), probe the
          // kernel without waiting; otherwise let the kernel wait one slice for us.
          const remaining = deadline == null ? Infinity : deadline - Date.now();
          const sliceMs =
            ready > 0 || t === 0
              ? 0
              : Math.max(0, Math.min(KERNEL_WAIT_SLICE_MS, remaining));
          let response = null;
          try {
            response = callSyncRpc('__kernel_poll', [kernelTargets, sliceMs]);
          } catch {
            response = null;
          }
          const responseEntries = Array.isArray(response?.fds) ? response.fds : [];
          for (const entry of kernelEntries) {
            const responseEntry = responseEntries.find(
              (item) => (Number(item?.fd) >>> 0) === (entry.fd >>> 0),
            );
            const kernelRevents = Number(responseEntry?.revents) >>> 0;
            let revents = 0;
            if (kernelRevents & KERNEL_POLLIN) revents |= POLLIN & entry.events;
            if (kernelRevents & KERNEL_POLLOUT) revents |= POLLOUT & entry.events;
            if (kernelRevents & KERNEL_POLLERR) revents |= POLLERR;
            if (kernelRevents & KERNEL_POLLHUP) revents |= POLLHUP;
            new DataView(instanceMemory.buffer).setUint16(entry.base + 6, revents, true);
            if (revents) ready++;
          }
        }

        if (ready > 0 || t === 0 || (deadline != null && Date.now() >= deadline)) {
          dispatchPendingWasmSignals();
          new DataView(instanceMemory.buffer).setUint32(Number(retReadyPtr) >>> 0, ready >>> 0, true);
          return 0;
        }
        let pumpedSocket = false;
        const v2 = new DataView(instanceMemory.buffer);
        for (let i = 0; i < n; i++) {
          const fd = v2.getInt32(base0 + i * 8, true);
          const s = getHostNetSocket(fd);
          if (s && s.socketId && !s.serverId) {
            pollHostNetSocket(s, 10);
            pumpedSocket = true;
          }
        }
        if (kernelTargets.length === 0 && !pumpedSocket) {
          // Nothing to wait on except time: sleep a slice instead of hot-spinning.
          const remaining = deadline == null ? Infinity : deadline - Date.now();
          Atomics.wait(syntheticWaitArray, 0, 0, Math.max(1, Math.min(10, remaining)));
        }
      }
    } catch (_e) {
      return WASI_ERRNO_FAULT;
    }
  },
  net_socket(domain, sockType, protocol, retFdPtr) {
    try {
      const numericDomain = Number(domain) >>> 0;
      const numericType = Number(sockType) >>> 0;
      const numericProtocol = Number(protocol) >>> 0;

      const fd = nextHostNetSocketFd++;
      hostNetSockets.set(fd, {
        domain: numericDomain,
        sockType: numericType,
        protocol: numericProtocol,
        bindOptions: null,
        localInfo: null,
        localReservation: null,
        remoteInfo: null,
        serverId: null,
        socketId: null,
        udpSocketId: null,
        recvTimeoutMs: null,
        readChunks: [],
        readableEnded: false,
        closed: false,
        lastError: null,
      });
      return writeGuestUint32(retFdPtr, fd);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  // Mark a host_net socket non-blocking (O_NONBLOCK). The patched wasi-libc fcntl cannot reach
  // host_net fds, so libxcb calls this directly. Non-blocking recv returns EAGAIN on no data.
  net_set_nonblock(fd, enable) {
    const socket = getHostNetSocket(fd);
    if (!socket) return WASI_ERRNO_BADF;
    socket.nonblock = (Number(enable) >>> 0) !== 0;
    return WASI_ERRNO_SUCCESS;
  },
  net_connect(fd, addrPtr, addrLen) {
    const socket = getHostNetSocket(fd);
    if (!socket) {
      return WASI_ERRNO_BADF;
    }

    try {
      let rawAddr = String(readGuestString(addrPtr, addrLen) ?? '');
      // A sockaddr_un serialized from sizeof(struct sockaddr_un) carries trailing NUL
      // padding; cut at the first NUL so the unix path is clean before classification.
      const nulAt = rawAddr.indexOf(String.fromCharCode(0));
      if (nulAt >= 0) rawAddr = rawAddr.slice(0, nulAt);
      rawAddr = rawAddr.trim();
      // AF_UNIX path connect (e.g. X11 /tmp/.X11-unix/X0): the C library passes the
      // raw sun_path, the same '/'-prefixed string net_bind/net_listen receive. Route it
      // to the sidecar's path-based net.connect, which dials the host-backed unix socket
      // the listener bound under the VM sandbox root, so two guests in one VM can talk.
      if (rawAddr.startsWith('/')) {
        let result;
        try {
          result = callSyncRpc('net.connect', [{ path: rawAddr }]);
        } catch (e) {
          try { process.stderr.write('[host_net] connect ' + rawAddr + ' failed: ' + (e && e.message ? e.message : String(e)) + '\n'); } catch (_) {}
          return WASI_ERRNO_FAULT;
        }
        if (!result || typeof result.socketId !== 'string') {
          try { process.stderr.write('[host_net] ' + rawAddr + ' returned no socketId\n'); } catch (_) {}
          return WASI_ERRNO_FAULT;
        }
        socket.socketId = result.socketId;
        socket.localInfo = null;
        socket.localReservation = null;
        socket.remoteInfo = null;
        socket.readChunks.length = 0;
        socket.readableEnded = false;
        socket.closed = false;
        socket.lastError = null;
        return WASI_ERRNO_SUCCESS;
      }
      const { host, port } = parseHostNetAddress(rawAddr);
      if (!Number.isInteger(port) || port < 0 || port > 65535) {
        return WASI_ERRNO_FAULT;
      }

      const request = { host, port };
      if (socket.bindOptions?.host != null) {
        request.localAddress = socket.bindOptions.host;
      }
      if (socket.bindOptions?.port != null) {
        request.localPort = socket.bindOptions.port;
      }
      if (socket.localReservation != null) {
        request.localReservation = socket.localReservation;
      }

      const result = callSyncRpc('net.connect', [request]);
      if (!result || typeof result.socketId !== 'string') {
        return WASI_ERRNO_FAULT;
      }

      socket.socketId = result.socketId;
      socket.localInfo = normalizeHostNetAddressInfo(result.localAddress, result.localPort);
      socket.localReservation = null;
      socket.remoteInfo = normalizeHostNetAddressInfo(result.remoteAddress, result.remotePort);
      socket.readChunks.length = 0;
      socket.readableEnded = false;
      socket.closed = false;
      socket.lastError = null;
      return WASI_ERRNO_SUCCESS;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_getaddrinfo(hostPtr, hostLen, portPtr, portLen, family, retAddrPtr, retAddrLenPtr) {
    try {
      const hostname = readGuestString(hostPtr, hostLen);
      const numericFamily = Number(family) >>> 0;
      const lookupOptions = { hostname, all: true };
      if (numericFamily === 4) {
        lookupOptions.family = 4;
      } else if (numericFamily === 6) {
        lookupOptions.family = 6;
      } else if (numericFamily !== 0) {
        return WASI_ERRNO_INVAL;
      }

      const records = callSyncRpc('dns.lookup', [lookupOptions]);
      if (!Array.isArray(records)) {
        return WASI_ERRNO_FAULT;
      }
      const payload = records.map((record) => {
        const family = Number(record?.family);
        if (family !== 4 && family !== 6) {
          throw new Error('host_net dns record family is unsupported');
        }
        return {
          addr: String(record?.address ?? ''),
          family,
        };
      });
      const encoded = Buffer.from(JSON.stringify(payload), 'utf8');
      return writeGuestBytes(
        retAddrPtr,
        readGuestUint32(retAddrLenPtr),
        encoded,
        retAddrLenPtr,
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_bind(fd, addrPtr, addrLen) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      if (socket.localReservation != null) {
        callSyncRpc('net.release_tcp_port', [socket.localReservation]);
        socket.localReservation = null;
      }

      socket.bindOptions = parseHostNetListenAddress(readGuestString(addrPtr, addrLen));
      if (hostNetSocketBaseType(socket) === HOST_NET_SOCK_DGRAM) {
        if (socket.bindOptions.path != null) {
          return WASI_ERRNO_FAULT;
        }
        const udpSocketId = ensureHostNetUdpSocket(socket);
        if (!udpSocketId) {
          return WASI_ERRNO_FAULT;
        }
        const result = callSyncRpc('dgram.bind', [
          udpSocketId,
          {
            address: socket.bindOptions.host,
            port: socket.bindOptions.port,
          },
        ]);
        socket.localInfo = normalizeHostNetAddressInfo(result?.localAddress, result?.localPort);
        return socket.localInfo ? WASI_ERRNO_SUCCESS : WASI_ERRNO_FAULT;
      }

      if (socket.bindOptions.path == null) {
        const reservation = callSyncRpc('net.reserve_tcp_port', [socket.bindOptions]);
        if (
          !reservation ||
          typeof reservation.reservationId !== 'string' ||
          !Number.isInteger(Number(reservation.localPort))
        ) {
          return WASI_ERRNO_FAULT;
        }
        socket.localReservation = reservation.reservationId;
        socket.bindOptions = {
          ...socket.bindOptions,
          host: reservation.localAddress ?? socket.bindOptions.host,
          port: Number(reservation.localPort),
        };
        socket.localInfo = normalizeHostNetAddressInfo(
          socket.bindOptions.host ?? '127.0.0.1',
          socket.bindOptions.port,
        );
      } else {
        socket.localInfo = null;
      }
      return WASI_ERRNO_SUCCESS;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_listen(fd, backlog) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }
    if (socket.serverId || !socket.bindOptions) {
      return WASI_ERRNO_FAULT;
    }

    try {
      const request = {
        ...socket.bindOptions,
        backlog: Math.max(0, Number(backlog) >>> 0),
      };
      if (socket.localReservation != null) {
        request.localReservation = socket.localReservation;
      }

      const result = callSyncRpc('net.listen', [request]);
      if (!result || typeof result.serverId !== 'string') {
        return WASI_ERRNO_FAULT;
      }
      socket.serverId = result.serverId;
      socket.localReservation = null;
      socket.localInfo = normalizeHostNetAddressInfo(result.localAddress, result.localPort);
      return WASI_ERRNO_SUCCESS;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_accept(fd, retFdPtr, retAddrPtr, retAddrLenPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket?.serverId || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      // First drain a connection already buffered by net_poll's readiness probe; otherwise block
      // until one arrives (POSIX blocking-accept semantics, for guests that accept() without polling
      // first). This no longer starves connected clients: net_poll now reports the listener readable
      // only when a connection is actually pending, so the X server only reaches accept() when there
      // is one to take, and otherwise services connected client fds instead.
      if (!socket.pendingAccepts) socket.pendingAccepts = [];
      let accepted = socket.pendingAccepts.shift();
      while (!accepted) {
        accepted = tryHostNetAcceptOnce(socket);
        if (!accepted) {
          pumpSpawnedChildren(10);
        }
      }
      if (writeGuestUint32(retFdPtr, accepted.acceptedFd) !== WASI_ERRNO_SUCCESS) {
        return WASI_ERRNO_FAULT;
      }
      return writeGuestBytes(retAddrPtr, readGuestUint32(retAddrLenPtr), accepted.address, retAddrLenPtr);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_getsockname(fd, addrPtr, addrLenPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }
    if (!socket.localInfo) {
      return WASI_ERRNO_INVAL;
    }

    try {
      const address = Buffer.from(formatHostNetAddressInfo(socket.localInfo), 'utf8');
      return writeGuestBytes(addrPtr, readGuestUint32(addrLenPtr), address, addrLenPtr);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_getpeername(fd, addrPtr, addrLenPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }
    if (!socket.remoteInfo) {
      return WASI_ERRNO_INVAL;
    }

    try {
      const address = Buffer.from(formatHostNetAddressInfo(socket.remoteInfo), 'utf8');
      return writeGuestBytes(addrPtr, readGuestUint32(addrLenPtr), address, addrLenPtr);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_send(fd, bufPtr, bufLen, flags, retSentPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket?.socketId || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      const chunk = readGuestBytes(bufPtr, bufLen);
      if ((Number(flags) >>> 0) !== 0) {
        // Non-zero send flags are currently ignored in the WASM host_net shim.
      }
      const written = Number(callSyncRpc('net.write', [socket.socketId, chunk])) >>> 0;
      return writeGuestUint32(retSentPtr, written);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_recv(fd, bufPtr, bufLen, flags, retReceivedPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket) {
      return WASI_ERRNO_BADF;
    }

    try {
      if ((Number(flags) >>> 0) !== 0) {
        // Non-zero recv flags are currently ignored in the WASM host_net shim.
      }

      // Non-blocking sockets (O_NONBLOCK via net_set_nonblock, used by libxcb's poll_for_*):
      // pull whatever is queued, do ONE short readiness probe, and return EAGAIN if still empty
      // instead of blocking. libxcb assumes its "poll" reads never block on an empty socket.
      if (socket.nonblock) {
        let queued = dequeueHostNetBytes(socket, bufLen);
        if (queued.length > 0) {
          return writeGuestBytes(bufPtr, bufLen, queued, retReceivedPtr);
        }
        if (socket.lastError) return WASI_ERRNO_FAULT;
        if (socket.readableEnded || socket.closed || !socket.socketId) {
          return writeGuestUint32(retReceivedPtr, 0);
        }
        pollHostNetSocket(socket, 0);
        queued = dequeueHostNetBytes(socket, bufLen);
        if (queued.length > 0) {
          return writeGuestBytes(bufPtr, bufLen, queued, retReceivedPtr);
        }
        if (socket.readableEnded || socket.closed || !socket.socketId) {
          return writeGuestUint32(retReceivedPtr, 0);
        }
        return WASI_ERRNO_AGAIN;
      }

      const deadline =
        socket.recvTimeoutMs == null ? null : Date.now() + Math.max(0, socket.recvTimeoutMs);
      while (true) {
        const queued = dequeueHostNetBytes(socket, bufLen);
        if (queued.length > 0) {
          return writeGuestBytes(bufPtr, bufLen, queued, retReceivedPtr);
        }

        if (socket.lastError) {
          return WASI_ERRNO_FAULT;
        }

        if (socket.readableEnded || socket.closed || !socket.socketId) {
          return writeGuestUint32(retReceivedPtr, 0);
        }

        const pollWaitMs =
          deadline == null ? 50 : Math.max(0, Math.min(50, deadline - Date.now()));
        if (deadline != null && pollWaitMs === 0) {
          return WASI_ERRNO_AGAIN;
        }
        pollHostNetSocket(socket, pollWaitMs);
        if (deadline != null && Date.now() >= deadline) {
          return WASI_ERRNO_AGAIN;
        }
      }
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_sendto(fd, bufPtr, bufLen, flags, addrPtr, addrLen, retSentPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      if ((Number(flags) >>> 0) !== 0) {
        return WASI_ERRNO_INVAL;
      }
      const udpSocketId = ensureHostNetUdpSocket(socket);
      if (!udpSocketId) {
        return WASI_ERRNO_FAULT;
      }

      const { host, port } = parseHostNetAddress(readGuestString(addrPtr, addrLen));
      const chunk = readGuestBytes(bufPtr, bufLen);
      const result = callSyncRpc('dgram.send', [
        udpSocketId,
        chunk,
        { address: host, port },
      ]);
      socket.localInfo = normalizeHostNetAddressInfo(result?.localAddress, result?.localPort);
      const written = Number(result?.bytes) >>> 0;
      return writeGuestUint32(retSentPtr, written);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_recvfrom(fd, bufPtr, bufLen, flags, retReceivedPtr, retAddrPtr, retAddrLenPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      if ((Number(flags) >>> 0) !== 0) {
        return WASI_ERRNO_INVAL;
      }
      const udpSocketId = ensureHostNetUdpSocket(socket);
      if (!udpSocketId) {
        return WASI_ERRNO_FAULT;
      }

      const deadline =
        socket.recvTimeoutMs == null ? null : Date.now() + Math.max(0, socket.recvTimeoutMs);
      while (true) {
        const pollWaitMs =
          deadline == null ? 50 : Math.max(0, Math.min(50, deadline - Date.now()));
        if (deadline != null && pollWaitMs === 0) {
          return WASI_ERRNO_AGAIN;
        }
        const event = callSyncRpc('dgram.poll', [udpSocketId, pollWaitMs]);
        if (!event) {
          if (deadline != null && Date.now() >= deadline) {
            return WASI_ERRNO_AGAIN;
          }
          continue;
        }
        if (event.type === 'error') {
          return WASI_ERRNO_FAULT;
        }
        if (event.type !== 'message') {
          continue;
        }

        let bytes;
        if (event.data && typeof event.data === 'object' && typeof event.data.base64 === 'string') {
          bytes = Buffer.from(event.data.base64, 'base64');
        } else {
          try {
            bytes = decodeFsBytesPayload(event.data, 'host_net recvfrom data');
          } catch {
            return WASI_ERRNO_FAULT;
          }
        }
        const dataResult = writeGuestBytes(bufPtr, bufLen, bytes, retReceivedPtr);
        if (dataResult !== WASI_ERRNO_SUCCESS) {
          return dataResult;
        }
        if (!event.remoteAddress || !Number.isInteger(Number(event.remotePort))) {
          return WASI_ERRNO_BADF;
        }
        let address;
        try {
          address = Buffer.from(formatHostNetAddressInfo({
            address: event.remoteAddress,
            port: event.remotePort,
          }), 'utf8');
        } catch {
          return WASI_ERRNO_INVAL;
        }
        let addressCapacity;
        try {
          addressCapacity = readGuestUint32(retAddrLenPtr);
        } catch {
          return WASI_ERRNO_FAULT;
        }
        const addressResult = writeGuestBytes(retAddrPtr, addressCapacity, address, retAddrLenPtr);
        return addressResult;
      }
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_setsockopt(fd, level, optname, optvalPtr, optvalLen) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }
    const sockoptKind = hostNetSockoptKind(level, optname, optvalLen);
    if (sockoptKind == null) {
      return WASI_ERRNO_INVAL;
    }
    try {
      const timeoutMs = parseHostNetTimevalMs(readGuestBytes(optvalPtr, optvalLen));
      if (timeoutMs == null && readGuestBytes(optvalPtr, optvalLen).some((byte) => byte !== 0)) {
        return WASI_ERRNO_INVAL;
      }
      if (sockoptKind === 'recv-timeout') {
        socket.recvTimeoutMs = timeoutMs;
      }
    } catch {
      return WASI_ERRNO_FAULT;
    }
    return WASI_ERRNO_SUCCESS;
  },
  net_getsockopt(fd, level, optname, optvalPtr, optvalLenPtr) {
    const socket = getHostNetSocket(fd);
    if (!socket || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      const optvalLen = readGuestUint32(optvalLenPtr);
      const normalizedLevel = Number(level) >>> 0;
      const normalizedOptname = Number(optname) >>> 0;
      if (
        (normalizedLevel === HOST_NET_SOL_SOCKET ||
          normalizedLevel === HOST_NET_WASI_SOL_SOCKET) &&
        normalizedOptname === HOST_NET_SO_ERROR
      ) {
        if (optvalLen < 4) {
          return WASI_ERRNO_INVAL;
        }
        new DataView(instanceMemory.buffer).setInt32(Number(optvalPtr) >>> 0, 0, true);
        return writeGuestUint32(optvalLenPtr, 4);
      }
      return WASI_ERRNO_INVAL;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_close(fd) {
    const numericFd = Number(fd) >>> 0;
    const socket = hostNetSockets.get(numericFd);
    if (!socket) {
      return WASI_ERRNO_BADF;
    }

    hostNetSockets.delete(numericFd);
    try {
      if (socket.localReservation != null) {
        callSyncRpc('net.release_tcp_port', [socket.localReservation]);
      }
      if (socket.socketId && !socket.closed) {
        callSyncRpc('net.destroy', [socket.socketId]);
      }
      if (socket.udpSocketId) {
        callSyncRpc('dgram.close', [socket.udpSocketId]);
      }
      return WASI_ERRNO_SUCCESS;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
  net_tls_connect(fd, hostnamePtr, hostnameLen, flags = 0) {
    const socket = getHostNetSocket(fd);
    if (!socket?.socketId || socket.closed) {
      return WASI_ERRNO_BADF;
    }

    try {
      const servername = readGuestString(hostnamePtr, hostnameLen);
      const tlsOptions = { servername };
      if ((Number(flags) & 1) === 1 || guestEnv.NODE_TLS_REJECT_UNAUTHORIZED === '0') {
        tlsOptions.rejectUnauthorized = false;
      }
      callSyncRpc('net.socket_upgrade_tls', [
        socket.socketId,
        JSON.stringify(tlsOptions),
      ]);
      return WASI_ERRNO_SUCCESS;
    } catch {
      return WASI_ERRNO_FAULT;
    }
  },
};

const hostProcessImport = {
        proc_spawn(
          argvPtr,
          argvLen,
          envpPtr,
          envpLen,
          stdinFd,
          stdoutFd,
          stderrFd,
          cwdPtr,
          cwdLen,
          retPidPtr,
        ) {
          if (permissionTier !== 'full') {
            return WASI_ERRNO_FAULT;
          }
          try {
            const argv = decodeNullSeparatedStrings(readGuestBytes(argvPtr, argvLen));
            if (argv.length === 0) {
              return WASI_ERRNO_FAULT;
            }

            const [command, ...args] = argv;
            const env = parseSerializedEnv(readGuestBytes(envpPtr, envpLen));
            const cwd =
              Number(cwdLen) > 0 ? readGuestString(cwdPtr, cwdLen) : undefined;
            const stdinTarget = resolveSpawnFd(stdinFd);
            const stdoutTarget = resolveSpawnFd(stdoutFd);
            const stderrTarget = resolveSpawnFd(stderrFd);
            const syntheticResult = maybeCreateSyntheticCommandResult(command, args, cwd);
            if (syntheticResult) {
              const record = createSyntheticChildRecord(
                syntheticResult,
                stdinTarget,
                stdoutTarget,
                stderrTarget,
              );
              spawnedChildren.set(record.pid, record);
              spawnedChildrenById.set(record.childId, record);
              traceHostProcess('proc-spawn-synthetic', {
                command,
                childId: record.childId,
                pid: record.pid,
                exitCode: syntheticResult.exitCode,
              });
              emitSyntheticCommandOutput(record, stdoutFd, stderrFd, syntheticResult);
              return writeGuestUint32(retPidPtr, record.pid);
            }
            traceHostProcess('proc-spawn-begin', {
              command,
              args,
              cwd: cwd ?? null,
              stdinFd: Number(stdinFd) >>> 0,
              stdoutFd: Number(stdoutFd) >>> 0,
              stderrFd: Number(stderrFd) >>> 0,
              stdinTarget,
              stdoutTarget,
              stderrTarget,
            });
            let stdinRedirectBytes = null;
            if (
              stdinTarget > 2 &&
              stdinTarget !== 0xffffffff &&
              !spawnStdinFdIsSyntheticPipe(stdinTarget)
            ) {
              stdinRedirectBytes = readSpawnStdinRedirectBytes(stdinTarget);
              if (stdinRedirectBytes == null) {
                traceHostProcess('proc-spawn-stdin-redirect-unreadable', {
                  command,
                  stdinFd: stdinTarget,
                });
                return WASI_ERRNO_FAULT;
              }
            }
            const result = callSyncRpc('child_process.spawn', [
              {
                command,
                args,
                options: {
                  cwd,
                  env,
                  internalBootstrapEnv: {},
                  shell: false,
                  stdio: [
                    stdinTarget === 0
                      ? 'inherit'
                      : stdinTarget === 0xffffffff
                        ? 'ignore'
                        : 'pipe',
                    stdoutTarget === 1
                      ? 'inherit'
                      : stdoutTarget === 0xffffffff
                        ? 'ignore'
                        : 'pipe',
                    stderrTarget === 2
                      ? 'inherit'
                      : stderrTarget === 0xffffffff
                        ? 'ignore'
                        : 'pipe',
                  ],
                },
              },
            ]);
            const pid = Number(result?.pid) >>> 0;
            if (!Number.isInteger(pid) || pid === 0 || typeof result?.childId !== 'string') {
              return WASI_ERRNO_FAULT;
            }

            const stdinPipe = registerPipeConsumer(stdinTarget, result.childId, 'stdin');
            const stdoutPipe = registerPipeProducer(stdoutTarget, result.childId, 'stdout');
            const stderrPipe = registerPipeProducer(stderrTarget, result.childId, 'stderr');
            const retainedSpawnOutputHandles = [stdoutTarget, stderrTarget]
              .filter((fd, index, values) => values.indexOf(fd) === index)
              .map((fd) => retainSpawnOutputHandle(fd))
              .filter(Boolean);
            const delegateRetainedFds = [stdinTarget, stdoutTarget, stderrTarget].filter(
              (fd, index, values) =>
                fd > 2 &&
                delegateManagedFdRefCounts.has(fd) &&
                values.indexOf(fd) === index,
            );
            for (const fd of delegateRetainedFds) {
              retainDelegateFd(fd);
            }
            const record = {
              childId: result.childId,
              pid,
              stdinFd: stdinTarget,
              stdoutFd: stdoutTarget,
              stderrFd: stderrTarget,
              stdinPipe,
              stdoutPipe,
              stderrPipe,
              stdinReadyAtMs: Date.now() + 100,
              delegateRetainedFds,
              retainedSpawnOutputHandles,
              exitStatus: null,
      };
            spawnedChildren.set(pid, record);
            spawnedChildrenById.set(result.childId, record);
            traceHostProcess('proc-spawn-ready', {
              command,
              childId: result.childId,
              pid,
            });
            if (stdinRedirectBytes != null) {
              if (stdinRedirectBytes.length > 0) {
                callSyncRpc('child_process.write_stdin', [
                  result.childId,
                  stdinRedirectBytes,
                ]);
              }
              callSyncRpc('child_process.close_stdin', [result.childId]);
            }
            consumeSpawnOutputFd(stdoutFd);
            consumeSpawnOutputFd(stderrFd);
            return writeGuestUint32(retPidPtr, pid);
          } catch (error) {
            traceHostProcess('proc-spawn-fault', {
              message: error instanceof Error ? error.message : String(error),
            });
            return WASI_ERRNO_FAULT;
          }
        },
        proc_waitpid(pid, options, retStatusPtr, retPidPtr) {
          const requestedPid = Number(pid) >>> 0;
          if (permissionTier !== 'full') {
            return requestedPid === 0xffffffff ? WASI_ERRNO_CHILD : WASI_ERRNO_SRCH;
          }
          const record =
            requestedPid === 0xffffffff
              ? spawnedChildren.values().next().value
              : spawnedChildren.get(requestedPid);
          if (!record) {
            return requestedPid === 0xffffffff ? WASI_ERRNO_CHILD : WASI_ERRNO_SRCH;
          }

          try {
            const nonBlocking = (Number(options) >>> 0) !== 0;
            traceHostProcess('proc-waitpid-begin', {
              requestedPid,
              childId: record.childId,
              pid: record.pid,
            });
            if (typeof record.exitStatus === 'number') {
              if (writeGuestUint32(retStatusPtr, record.exitStatus) !== WASI_ERRNO_SUCCESS) {
                return WASI_ERRNO_FAULT;
              }
              const writePidResult = writeGuestUint32(retPidPtr, record.pid);
              if (writePidResult !== WASI_ERRNO_SUCCESS) {
                return writePidResult;
              }
              reapSpawnedChild(record);
              return writePidResult;
            }

            while (true) {
              const event = pollChildEvent(
                record,
                nonBlocking ? 0 : 10,
              );
              if (!event) {
                if (!pumpChildInputPipe(record, nonBlocking ? 0 : 10)) {
                  if (nonBlocking) {
                    return writeGuestUint32(retPidPtr, 0);
                  }
                }
                continue;
              }
              traceHostProcess('proc-waitpid-poll', {
                requestedPid,
                childId: record.childId,
                type: event.type,
              });

              if (event.type === 'stdout' && record.stdoutFd !== 0xffffffff) {
                const chunk = decodeSyncRpcValue(event.data);
                if (chunk?.length > 0) {
                  routeChunkToFd(record.stdoutFd, chunk);
                }
                continue;
              }

              if (event.type === 'stderr' && record.stderrFd !== 0xffffffff) {
                const chunk = decodeSyncRpcValue(event.data);
                if (chunk?.length > 0) {
                  routeChunkToFd(record.stderrFd, chunk);
                }
                continue;
              }

              if (event.type === 'signal') {
                processChildEvent(record, event);
                continue;
              }

              if (event.type === 'exit') {
                processChildEvent(record, event);
                if (writeGuestUint32(retStatusPtr, record.exitStatus ?? 1) !== WASI_ERRNO_SUCCESS) {
                  return WASI_ERRNO_FAULT;
                }
                const writePidResult = writeGuestUint32(retPidPtr, record.pid);
                if (writePidResult !== WASI_ERRNO_SUCCESS) {
                  return writePidResult;
                }
                reapSpawnedChild(record);
                return writePidResult;
              }
            }
          } catch {
            traceHostProcess('proc-waitpid-fault', {
              requestedPid,
              childId: record.childId,
              pid: record.pid,
            });
            return WASI_ERRNO_FAULT;
          }
        },
        proc_kill(pid, signal) {
          if (permissionTier !== 'full') {
            return WASI_ERRNO_SRCH;
          }
          const targetPid = Number(pid) >>> 0;
          const signalName = signalNameFromNumber(signal);

          try {
            if (targetPid === VIRTUAL_PID) {
              callSyncRpc('process.kill', [VIRTUAL_PID, signalName]);
              if (
                Number(signal) > 0 &&
                typeof instance?.exports?.__wasi_signal_trampoline === 'function'
              ) {
                instance.exports.__wasi_signal_trampoline(Number(signal) | 0);
              }
              return WASI_ERRNO_SUCCESS;
            }

            const record = spawnedChildren.get(targetPid);
            if (record) {
              callSyncRpc('child_process.kill', [record.childId, signalName]);
              return WASI_ERRNO_SUCCESS;
            }

            callSyncRpc('process.kill', [targetPid, signalName]);
            return WASI_ERRNO_SUCCESS;
          } catch (error) {
            if (error?.code === 'ESRCH') {
              return WASI_ERRNO_SRCH;
            }
            return WASI_ERRNO_FAULT;
          }
        },
        proc_getpid(retPidPtr) {
          return writeGuestUint32(retPidPtr, VIRTUAL_PID);
        },
        proc_getppid(retPidPtr) {
          return writeGuestUint32(retPidPtr, VIRTUAL_PPID);
        },
        fd_pipe(retReadFdPtr, retWriteFdPtr) {
          try {
            const pipe = {
              id: nextSyntheticPipeId++,
              chunks: [],
              consumers: new Map(),
              producers: new Map(),
              readHandleCount: 0,
              writeHandleCount: 0,
            };
            const readFd = nextSyntheticFd++;
            const writeFd = nextSyntheticFd++;
            syntheticFdEntries.set(readFd, createPipeHandle('pipe-read', pipe, readFd));
            syntheticFdEntries.set(writeFd, createPipeHandle('pipe-write', pipe, writeFd));
            if (writeGuestUint32(retReadFdPtr, readFd) !== WASI_ERRNO_SUCCESS) {
              return WASI_ERRNO_FAULT;
            }
            return writeGuestUint32(retWriteFdPtr, writeFd);
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
        fd_dup(fd, retNewFdPtr) {
          try {
            const handle = cloneFdHandle(fd);
            if (!handle) {
              return WASI_ERRNO_BADF;
            }
            let duplicatedFd = 0;
            while (
              duplicatedFd <= 2 &&
              (
                syntheticFdEntries.has(duplicatedFd) ||
                passthroughHandles.has(duplicatedFd) ||
                delegateManagedFdRefCounts.has(duplicatedFd)
              )
            ) {
              duplicatedFd += 1;
            }
            if (duplicatedFd > 2) {
              duplicatedFd = nextSyntheticFd++;
            }
            syntheticFdEntries.set(duplicatedFd, handle);
            traceHostProcess('fd-dup', {
              fd: Number(fd) >>> 0,
              duplicatedFd,
              handleKind: handle.kind,
              targetFd: handle.targetFd ?? null,
              displayFd: handle.displayFd ?? null,
            });
            return writeGuestUint32(retNewFdPtr, duplicatedFd);
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
        fd_dup2(oldFd, newFd) {
          try {
            const sourceFd = Number(oldFd) >>> 0;
            const targetFd = Number(newFd) >>> 0;
            if (sourceFd === targetFd) {
              if (!lookupFdHandle(sourceFd)) {
                return WASI_ERRNO_BADF;
              }
              traceHostProcess('fd-dup2-same-fd', {
                oldFd: sourceFd,
                newFd: targetFd,
              });
              return WASI_ERRNO_SUCCESS;
            }

            const sourceHandle = cloneFdHandle(sourceFd);
            if (!sourceHandle) {
              return WASI_ERRNO_BADF;
            }

            traceHostProcess('fd-dup2-begin', {
              oldFd: sourceFd,
              newFd: targetFd,
              sourceKind: sourceHandle.kind,
              sourceTargetFd: sourceHandle.targetFd ?? null,
              sourceDisplayFd: sourceHandle.displayFd ?? null,
              existingKind: syntheticFdEntries.get(targetFd)?.kind ?? passthroughHandles.get(targetFd)?.kind ?? null,
            });

            closeSyntheticFd(targetFd);
            closePassthroughFd(targetFd);
            syntheticFdEntries.set(targetFd, sourceHandle);
            traceHostProcess('fd-dup2-installed', {
              oldFd: sourceFd,
              newFd: targetFd,
              sourceKind: sourceHandle.kind,
            });
            return WASI_ERRNO_SUCCESS;
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
        fd_dup_min(fd, minFd, retNewFdPtr) {
          try {
            const sourceFd = Number(fd);
            const minimumFdNumber = Number(minFd);
            if (!Number.isInteger(sourceFd) || sourceFd < 0) {
              return WASI_ERRNO_BADF;
            }
            if (!Number.isInteger(minimumFdNumber) || minimumFdNumber < 0) {
              return WASI_ERRNO_INVAL;
            }

            const handle = cloneFdHandle(sourceFd);
            if (!handle) {
              return WASI_ERRNO_BADF;
            }

            let duplicatedFd = minimumFdNumber >>> 0;
            while (
              syntheticFdEntries.has(duplicatedFd) ||
              passthroughHandles.has(duplicatedFd) ||
              delegateManagedFdRefCounts.has(duplicatedFd)
            ) {
              duplicatedFd += 1;
            }
            nextSyntheticFd = Math.max(nextSyntheticFd, duplicatedFd + 1);

            syntheticFdEntries.set(duplicatedFd, handle);
            traceHostProcess('fd-dup-min', {
              fd: sourceFd >>> 0,
              minimumFd: minimumFdNumber >>> 0,
              duplicatedFd,
              handleKind: handle.kind,
              targetFd: handle.targetFd ?? null,
              displayFd: handle.displayFd ?? null,
            });
            return writeGuestUint32(retNewFdPtr, duplicatedFd);
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
        sleep_ms(milliseconds) {
          try {
            const waitArray = new Int32Array(new SharedArrayBuffer(4));
            const deadline = Date.now() + (Number(milliseconds) >>> 0);
            while (Date.now() < deadline) {
              // Keep guest sleeps interruptible by V8 termination during SIGTERM,
              // SIGKILL, and VM disposal. Also drain handled Wasm signals at
              // syscall boundaries so cooperative handlers run during sleeps.
              dispatchPendingWasmSignals();
              Atomics.wait(waitArray, 0, 0, Math.max(1, Math.min(10, deadline - Date.now())));
            }
            dispatchPendingWasmSignals();
            return WASI_ERRNO_SUCCESS;
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
        pty_open(retMasterFdPtr, retSlaveFdPtr) {
          return WASI_ERRNO_FAULT;
        },
        proc_sigaction(signal, action, maskLo, maskHi, flags) {
          if (permissionTier !== 'full') {
            return WASI_ERRNO_FAULT;
          }
          try {
            const registration = {
              action: action === 0 ? 'default' : action === 1 ? 'ignore' : 'user',
              mask: decodeSignalMask(maskLo, maskHi),
              flags: Number(flags) >>> 0,
            };
            callSyncRpc('process.signal_state', [
              Number(signal) >>> 0,
              registration.action,
              JSON.stringify(registration.mask),
              registration.flags,
            ]);
            return WASI_ERRNO_SUCCESS;
          } catch {
            return WASI_ERRNO_FAULT;
          }
        },
};

const limitedHostProcessImport = {
  fd_dup_min: hostProcessImport.fd_dup_min,
};

const hostUserImport = {
  getuid(retUidPtr) {
    return writeGuestUint32(retUidPtr, VIRTUAL_UID);
  },
  getgid(retGidPtr) {
    return writeGuestUint32(retGidPtr, VIRTUAL_GID);
  },
  geteuid(retUidPtr) {
    return writeGuestUint32(retUidPtr, VIRTUAL_UID);
  },
  getegid(retGidPtr) {
    return writeGuestUint32(retGidPtr, VIRTUAL_GID);
  },
  isatty(fd, retBoolPtr) {
    const descriptor = Number(fd) >>> 0;
    const isTerminal = descriptor <= 2 && stdioFdIsKernelTty(descriptor) ? 1 : 0;
    return writeGuestUint32(retBoolPtr, isTerminal);
  },
  getpwuid(uid, bufPtr, bufLen, retLenPtr) {
    const numericUid = Number(uid) >>> 0;
    const passwdEntry =
      numericUid === VIRTUAL_UID
        ? `${VIRTUAL_OS_USER}:x:${VIRTUAL_UID}:${VIRTUAL_GID}::${VIRTUAL_OS_HOMEDIR}:${VIRTUAL_OS_SHELL}`
        : `user${numericUid}:x:${numericUid}:${numericUid}::/home/user${numericUid}:/bin/sh`;
    return writeGuestBytes(bufPtr, bufLen, encodeGuestBytes(passwdEntry), retLenPtr);
  },
};

const HOST_FS_MODE_REGULAR = 0o100644;
const HOST_FS_MODE_CHARACTER = 0o020666;
const HOST_FS_MODE_FIFO = 0o010600;
const HOST_FS_GUEST_CWD =
  typeof guestEnv?.PWD === 'string' && guestEnv.PWD.startsWith('/')
    ? path.posix.normalize(guestEnv.PWD)
    : '/';

for (let index = 0; index < WASI_PREOPEN_ENTRIES.length; index += 1) {
  const fd = WASI_PREOPEN_FD_BASE + index;
  const [guestPath, preopenSpec] = WASI_PREOPEN_ENTRIES[index];
  if (!passthroughHandles.has(fd)) {
    retainDelegateFd(fd);
    closedPassthroughFds.delete(fd);
    passthroughHandles.set(fd, {
      kind: 'passthrough',
      targetFd: fd,
      displayFd: fd,
      refCount: 0,
      open: true,
      guestPath: guestPathForPreopenKey(guestPath),
      readOnly: preopenSpec?.readOnly === true,
    });
  }
}

function hostFsModeFromStat(stat) {
  const mode = Number(stat?.mode);
  return Number.isInteger(mode) && mode > 0 ? mode >>> 0 : 0;
}

const hostFsSizeByGuestPath = new Map();
// Bound the per-path size cache so a guest truncating many distinct paths cannot
// grow it without limit. Entries are insertion-ordered, so evicting the oldest
// key is a cheap LRU-ish bound.
const HOST_FS_SIZE_CACHE_MAX_ENTRIES = 4096;
let hostFsSizeCacheEvictionWarned = false;

function forgetHostFsSize(guestPath) {
  if (typeof guestPath !== 'string') {
    return;
  }
  hostFsSizeByGuestPath.delete(path.posix.normalize(guestPath));
}

function rememberHostFsSize(guestPath, size) {
  if (typeof guestPath !== 'string') {
    return;
  }
  const normalized = path.posix.normalize(guestPath);
  if (!Number.isFinite(size) || size < 0) {
    hostFsSizeByGuestPath.delete(normalized);
    return;
  }
  if (
    !hostFsSizeByGuestPath.has(normalized) &&
    hostFsSizeByGuestPath.size >= HOST_FS_SIZE_CACHE_MAX_ENTRIES
  ) {
    const oldest = hostFsSizeByGuestPath.keys().next().value;
    if (oldest !== undefined) {
      hostFsSizeByGuestPath.delete(oldest);
    }
    if (!hostFsSizeCacheEvictionWarned) {
      hostFsSizeCacheEvictionWarned = true;
      traceHostProcess('host-fs-size-cache-evict', {
        max: HOST_FS_SIZE_CACHE_MAX_ENTRIES,
      });
    }
  }
  hostFsSizeByGuestPath.set(normalized, BigInt(Math.trunc(size)));
}

function rememberedHostFsSize(guestPath) {
  if (typeof guestPath !== 'string') {
    return null;
  }
  return hostFsSizeByGuestPath.get(path.posix.normalize(guestPath)) ?? null;
}

function resolveHostFsPath(value, fromGuestDir = HOST_FS_GUEST_CWD) {
  return resolveHostFsMapping(value, fromGuestDir)?.hostPath ?? null;
}

function resolveHostFsMapping(value, fromGuestDir = HOST_FS_GUEST_CWD) {
  const guestPath = resolveSyntheticGuestPath(value, fromGuestDir);
  if (typeof guestPath !== 'string') {
    return null;
  }
  return resolveModuleGuestPathToHostMapping(guestPath);
}

const hostFsImport = {
  fd_mode(fd) {
    const descriptor = Number(fd) >>> 0;
    if (descriptor <= 2) {
      return HOST_FS_MODE_CHARACTER;
    }

    const handle = lookupFdHandle(descriptor);
    if (handle?.kind === 'pipe-read' || handle?.kind === 'pipe-write') {
      return HOST_FS_MODE_FIFO;
    }

    try {
      const targetFd =
        typeof handle?.ioFd === 'number'
          ? Number(handle.ioFd) >>> 0
          : typeof handle?.targetFd === 'number'
            ? Number(handle.targetFd) >>> 0
            : descriptor;
      return hostFsModeFromStat(fsModule.fstatSync(targetFd)) || HOST_FS_MODE_REGULAR;
    } catch {
      return HOST_FS_MODE_REGULAR;
    }
  },
  fd_size(fd) {
    const descriptor = Number(fd) >>> 0;
    try {
      const handle = lookupFdHandle(descriptor);
      const rememberedSize = rememberedHostFsSize(handle?.guestPath);
      if (rememberedSize != null) {
        return rememberedSize;
      }
      if (typeof handle?.ioFd === 'number') {
        return BigInt(fsModule.fstatSync(Number(handle.ioFd) >>> 0).size ?? -1);
      }
      if (typeof handle?.guestPath === 'string') {
        const hostPath = resolveHostFsPath(handle.guestPath);
        if (typeof hostPath === 'string') {
          return BigInt(fsModule.statSync(hostPath).size ?? -1);
        }
        return BigInt(fsModule.statSync(handle.guestPath).size ?? -1);
      }
      const targetFd = typeof handle?.targetFd === 'number'
        ? Number(handle.targetFd) >>> 0
        : descriptor;
      return BigInt(fsModule.fstatSync(targetFd).size ?? -1);
    } catch {
      return (1n << 64n) - 1n;
    }
  },
  path_mode(fd, pathPtr, pathLen, followSymlinks) {
    try {
      const target = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
      if (typeof target !== 'string') {
        return 0;
      }
      const hostPath = resolveHostFsPath(target);
      if (typeof hostPath !== 'string') {
        return 0;
      }
      const stat =
        Number(followSymlinks) === 0
          ? fsModule.lstatSync(hostPath)
          : fsModule.statSync(hostPath);
      const mode = hostFsModeFromStat(stat);
      traceHostProcess('host-fs-path-mode', {
        target,
        hostPath,
        followSymlinks: Number(followSymlinks) >>> 0,
        mode,
      });
      return mode;
    } catch {
      traceHostProcess('host-fs-path-mode-fault', {});
      return 0;
    }
  },
  path_size(fd, pathPtr, pathLen, followSymlinks) {
    const target = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
    if (typeof target !== 'string') {
      return (1n << 64n) - 1n;
    }
    const rememberedSize = rememberedHostFsSize(target);
    if (rememberedSize != null) {
      return rememberedSize;
    }

    try {
      const hostPath = resolveHostFsPath(target);
      if (typeof hostPath === 'string') {
        const stat =
          Number(followSymlinks) === 0
            ? fsModule.lstatSync(hostPath)
            : fsModule.statSync(hostPath);
        return BigInt(stat?.size ?? -1);
      }
      const guestStat =
        Number(followSymlinks) === 0
          ? fsModule.lstatSync(target)
          : fsModule.statSync(target);
      return BigInt(guestStat?.size ?? -1);
    } catch {
      return (1n << 64n) - 1n;
    }
  },
  chmod(fd, pathPtr, pathLen, mode) {
    try {
      const target = resolvePathOpenGuestPath(fd, pathPtr, pathLen);
      if (typeof target !== 'string') {
        return 1;
      }
      const mapping = resolveHostFsMapping(target);
      if (!mapping || typeof mapping.hostPath !== 'string') {
        return 1;
      }
      if (mapping.readOnly) {
        return 1;
      }
      traceHostProcess('host-fs-chmod', {
        target,
        hostPath: mapping.hostPath,
        mode: Number(mode) >>> 0,
      });
      chmodMappedGuestPath(target, mapping.hostPath, Number(mode) >>> 0);
      return 0;
    } catch {
      traceHostProcess('host-fs-chmod-fault', {});
      return 1;
    }
  },
  fchmod(fd, mode) {
    try {
      const descriptor = Number(fd) >>> 0;
      const handle = lookupFdHandle(descriptor);
      if (handle?.readOnly === true) {
        return 1;
      }
      if (typeof handle?.guestPath === 'string') {
        const mapping = resolveHostFsMapping(handle.guestPath);
        if (!mapping || typeof mapping.hostPath !== 'string' || mapping.readOnly) {
          return 1;
        }
        chmodMappedGuestPath(handle.guestPath, mapping.hostPath, Number(mode) >>> 0);
        return 0;
      }
      const targetFd =
        typeof handle?.targetFd === 'number' ? Number(handle.targetFd) >>> 0 : descriptor;
      fsModule.fchmodSync(targetFd, Number(mode) >>> 0);
      return 0;
    } catch {
      traceHostProcess('host-fs-fchmod-fault', {});
      return 1;
    }
  },
  ftruncate(fd, length) {
    try {
      const descriptor = Number(fd) >>> 0;
      const nextSize = Number(length);
      if (!Number.isFinite(nextSize) || nextSize < 0) {
        return 1;
      }
      const handle = lookupFdHandle(descriptor);
      if (handle?.readOnly === true) {
        return 1;
      }
      if (typeof handle?.ioFd === 'number') {
        fsModule.ftruncateSync(handle.ioFd, nextSize);
        if ((handle.position ?? 0) > nextSize) {
          handle.position = nextSize;
        }
        rememberHostFsSize(handle.guestPath, nextSize);
        return 0;
      }
      if (typeof handle?.guestPath === 'string') {
        const pathFd = fsModule.openSync(handle.guestPath, 0o1, 0o666);
        try {
          fsModule.ftruncateSync(pathFd, nextSize);
          if ((handle.position ?? 0) > nextSize) {
            handle.position = nextSize;
          }
          rememberHostFsSize(handle.guestPath, nextSize);
        } finally {
          fsModule.closeSync(pathFd);
        }
        return 0;
      }
      return 1;
    } catch {
      return 1;
    }
  },
};

wasiImport.clock_time_get = (clockId, precision, resultPtr) => {
  const numericClockId = Number(clockId) >>> 0;
  if (numericClockId !== 0 && delegateClockTimeGet) {
    return delegateClockTimeGet(clockId, precision, resultPtr);
  }
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return delegateClockTimeGet
      ? delegateClockTimeGet(clockId, precision, resultPtr)
      : WASI_ERRNO_FAULT;
  }

  try {
    const view = new DataView(instanceMemory.buffer);
    view.setBigUint64(Number(resultPtr), frozenTimeNs, true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
};

wasiImport.clock_res_get = (clockId, resultPtr) => {
  const numericClockId = Number(clockId) >>> 0;
  if (numericClockId !== 0 && delegateClockResGet) {
    return delegateClockResGet(clockId, resultPtr);
  }
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return delegateClockResGet
      ? delegateClockResGet(clockId, resultPtr)
      : WASI_ERRNO_FAULT;
  }

  try {
    const view = new DataView(instanceMemory.buffer);
    view.setBigUint64(Number(resultPtr), 1000000n, true);
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
};

if (delegatePathOpen) {
  wasiImport.path_open = (
    fd,
    dirflags,
    pathPtr,
    pathLen,
    oflags,
    rightsBase,
    rightsInheriting,
    fdflags,
    openedFdPtr,
  ) => {
    const workspaceReadOnlyDenied = __agentOSWasiMeasurePhase('path_open', 'readonly_policy', () =>
      isWorkspaceReadOnly() &&
      (hasMutationOpenFlags(oflags) || hasWriteRights(rightsBase))
    );
    if (workspaceReadOnlyDenied) {
      return denyReadOnlyMutation();
    }

    const passthroughDirHandle = __agentOSWasiMeasurePhase('path_open', 'lookup_handle', () =>
      lookupFdHandle(fd)
    );
    if (passthroughDirHandle && passthroughDirHandle.kind !== 'passthrough') {
      return WASI_ERRNO_BADF;
    }
    if (!passthroughDirHandle && rejectClosedPassthroughFd(fd)) {
      return WASI_ERRNO_BADF;
    }

    const delegateDirFd =
      passthroughDirHandle?.kind === 'passthrough'
        ? passthroughDirHandle.targetFd
        : fd;
    const guestPath = __agentOSWasiMeasurePhase('path_open', 'path_resolution', () =>
      resolvePathOpenGuestPath(fd, pathPtr, pathLen)
    );
    const guestReadOnlyDenied = __agentOSWasiMeasurePhase('path_open', 'readonly_policy', () =>
      guestPathIsReadOnly(guestPath) &&
      (hasMutationOpenFlags(oflags) || hasWriteRights(rightsBase))
    );
    if (guestReadOnlyDenied) {
      return denyReadOnlyMutation();
    }
    const mayCreateTarget = pathOpenMayCreateTarget(oflags, rightsBase, fdflags);
    if (!SIDECAR_MANAGED_PROCESS && mayCreateTarget) {
      try {
        const syntheticResult = __agentOSWasiMeasurePhase(
          'path_open',
          'synthetic_open',
          () => openGuestFileForPathOpen(
            fd,
            pathPtr,
            pathLen,
            oflags,
            rightsBase,
            fdflags,
            openedFdPtr,
          ),
        );
        if (syntheticResult != null) {
          return syntheticResult;
        }
      } catch {
        return WASI_ERRNO_FAULT;
      }
    }

    let result = __agentOSWasiMeasurePhase(
      'path_open',
      'delegate_call',
      () => delegatePathOpen(
        delegateDirFd,
        dirflags,
        pathPtr,
        pathLen,
        oflags,
        rightsBase,
        rightsInheriting,
        fdflags,
        openedFdPtr,
      ),
    );

    // Precreate-and-retry exists for creatable targets the delegate reports
    // as missing (e.g. `>>` append redirect targets). Only retry on NOENT:
    // retrying on permission errors would mask the real errno and attempt to
    // create a file the kernel just denied.
    if (
      result === WASI_ERRNO_NOENT &&
      mayCreateTarget
    ) {
      try {
        __agentOSWasiMeasurePhase('path_open', 'synthetic_precreate', () =>
          precreatePathOpenTarget(fd, pathPtr, pathLen, oflags, rightsBase, fdflags)
        );
        result = __agentOSWasiMeasurePhase(
          'path_open',
          'delegate_call',
          () => delegatePathOpen(
            delegateDirFd,
            dirflags,
            pathPtr,
            pathLen,
            oflags,
            rightsBase,
            rightsInheriting,
            fdflags,
            openedFdPtr,
          ),
        );
        if (!SIDECAR_MANAGED_PROCESS && result !== WASI_ERRNO_SUCCESS) {
          const fallbackResult = __agentOSWasiMeasurePhase(
            'path_open',
            'synthetic_open',
            () => openGuestFileForPathOpen(
              fd,
              pathPtr,
              pathLen,
              oflags,
              rightsBase,
              fdflags,
              openedFdPtr,
            ),
          );
          if (fallbackResult != null) {
            return fallbackResult;
          }
        }
      } catch {
        return WASI_ERRNO_FAULT;
      }
    }

    if (result === WASI_ERRNO_SUCCESS) {
      return __agentOSWasiMeasurePhase('path_open', 'fd_bookkeeping', () =>
        retainPathOpenDelegateFd(openedFdPtr, guestPath, fdflags, rightsBase)
      );
    }
    return result;
  };
}

function wrapReadOnlyPathMutation(name, shouldDeny) {
  const delegate = typeof wasiImport[name] === 'function' ? wasiImport[name].bind(wasiImport) : null;
  if (!delegate) {
    return;
  }
  wasiImport[name] = (...args) => {
    if (shouldDeny(...args)) {
      return denyReadOnlyMutation();
    }
    return delegate(...args);
  };
}

wrapReadOnlyPathMutation('path_create_directory', (fd, pathPtr, pathLen) =>
  resolvedGuestPathIsReadOnly(fd, pathPtr, pathLen),
);
wrapReadOnlyPathMutation('path_filestat_set_times', (fd, _flags, pathPtr, pathLen) =>
  resolvedGuestPathIsReadOnly(fd, pathPtr, pathLen),
);
wrapReadOnlyPathMutation(
  'path_link',
  (oldFd, _oldFlags, oldPathPtr, oldPathLen, newFd, newPathPtr, newPathLen) =>
    resolvedGuestPathIsReadOnly(oldFd, oldPathPtr, oldPathLen) ||
    resolvedGuestPathIsReadOnly(newFd, newPathPtr, newPathLen),
);
wrapReadOnlyPathMutation('path_remove_directory', (fd, pathPtr, pathLen) =>
  resolvedGuestPathIsReadOnly(fd, pathPtr, pathLen),
);
wrapReadOnlyPathMutation(
  'path_rename',
  (oldFd, oldPathPtr, oldPathLen, newFd, newPathPtr, newPathLen) =>
    resolvedGuestPathIsReadOnly(oldFd, oldPathPtr, oldPathLen) ||
    resolvedGuestPathIsReadOnly(newFd, newPathPtr, newPathLen),
);
wrapReadOnlyPathMutation('path_symlink', (_oldPathPtr, _oldPathLen, fd, newPathPtr, newPathLen) =>
  resolvedGuestPathIsReadOnly(fd, newPathPtr, newPathLen),
);
wrapReadOnlyPathMutation('path_unlink_file', (fd, pathPtr, pathLen) =>
  resolvedGuestPathIsReadOnly(fd, pathPtr, pathLen),
);

if (isWorkspaceReadOnly()) {

  wasiImport.fd_write = (fd, iovs, iovsLen, nwrittenPtr) => {
    if (Number(fd) > 2) {
      return denyReadOnlyMutation();
    }

    return delegateFdWrite ? delegateFdWrite(fd, iovs, iovsLen, nwrittenPtr) : WASI_ERRNO_FAULT;
  };

  wasiImport.fd_pwrite = (fd, iovs, iovsLen, offset, nwrittenPtr) => {
    if (Number(fd) > 2) {
      return denyReadOnlyMutation();
    }

    return delegateFdPwrite
      ? delegateFdPwrite(fd, iovs, iovsLen, offset, nwrittenPtr)
      : WASI_ERRNO_FAULT;
  };

  for (const name of [
    'fd_allocate',
    'fd_filestat_set_size',
    'fd_filestat_set_times',
    'path_create_directory',
    'path_filestat_set_times',
    'path_link',
    'path_remove_directory',
    'path_rename',
    'path_symlink',
    'path_unlink_file',
  ]) {
    if (typeof wasiImport[name] === 'function') {
      wasiImport[name] = () => denyReadOnlyMutation();
    }
  }
}

const delegateManagedFdRead =
  typeof wasiImport.fd_read === 'function'
    ? wasiImport.fd_read.bind(wasiImport)
    : null;
const delegateManagedFdWrite =
  typeof wasiImport.fd_write === 'function'
    ? wasiImport.fd_write.bind(wasiImport)
    : null;
const delegateManagedFdPwrite =
  typeof wasiImport.fd_pwrite === 'function'
    ? wasiImport.fd_pwrite.bind(wasiImport)
    : null;
const delegateManagedFdSeek =
  typeof wasiImport.fd_seek === 'function'
    ? wasiImport.fd_seek.bind(wasiImport)
    : null;
const delegateManagedFdTell =
  typeof wasiImport.fd_tell === 'function'
    ? wasiImport.fd_tell.bind(wasiImport)
    : null;
const delegateManagedFdFdstatGet =
  typeof wasiImport.fd_fdstat_get === 'function'
    ? wasiImport.fd_fdstat_get.bind(wasiImport)
    : null;
const delegateManagedFdFdstatSetFlags =
  typeof wasiImport.fd_fdstat_set_flags === 'function'
    ? wasiImport.fd_fdstat_set_flags.bind(wasiImport)
    : null;
const delegateManagedFdFilestatGet =
  typeof wasiImport.fd_filestat_get === 'function'
    ? wasiImport.fd_filestat_get.bind(wasiImport)
    : null;
const delegateManagedFdFilestatSetSize =
  typeof wasiImport.fd_filestat_set_size === 'function'
    ? wasiImport.fd_filestat_set_size.bind(wasiImport)
    : null;
const delegateManagedFdClose =
  typeof wasiImport.fd_close === 'function'
    ? wasiImport.fd_close.bind(wasiImport)
    : null;
const delegateManagedFdRenumber =
  typeof wasiImport.fd_renumber === 'function'
    ? wasiImport.fd_renumber.bind(wasiImport)
    : null;
const delegateManagedFdPrestatGet =
  typeof wasiImport.fd_prestat_get === 'function'
    ? wasiImport.fd_prestat_get.bind(wasiImport)
    : null;
const delegateManagedFdPrestatDirName =
  typeof wasiImport.fd_prestat_dir_name === 'function'
    ? wasiImport.fd_prestat_dir_name.bind(wasiImport)
    : null;
const delegateManagedPollOneoff =
  typeof wasiImport.poll_oneoff === 'function'
    ? wasiImport.poll_oneoff.bind(wasiImport)
    : null;
const KERNEL_POLLIN = 0x0001;
const KERNEL_POLLOUT = 0x0004;
const KERNEL_POLLERR = 0x0008;
const KERNEL_POLLHUP = 0x0010;

wasiImport.fd_read = (fd, iovs, iovsLen, nreadPtr) => {
  const numericFd = Number(fd) >>> 0;
  const handle = __agentOSWasiMeasurePhase('fd_read', 'lookup_handle', () =>
    lookupFdHandle(numericFd)
  );
  if (handle?.kind === 'pipe-read') {
    try {
      const requestedLength = __agentOSWasiMeasurePhase('fd_read', 'iov_scan', () => {
        if (!(instanceMemory instanceof WebAssembly.Memory)) {
          return 0;
        }
        const view = new DataView(instanceMemory.buffer);
        let total = 0;
        for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
          const entryOffset = (Number(iovs) >>> 0) + index * 8;
          total += view.getUint32(entryOffset + 4, true);
        }
        return total >>> 0;
      });

      const pipeClosed = __agentOSWasiMeasurePhase('fd_read', 'pipe_wait', () => {
        while (handle.pipe.chunks.length === 0) {
          if (handle.pipe.writeHandleCount === 0 && handle.pipe.producers.size === 0) {
            return true;
          }

          const pumped = pumpPipeProducers(handle.pipe, 10);
          if (!pumped) {
            Atomics.wait(syntheticWaitArray, 0, 0, 10);
          }
        }
        return false;
      });
      if (pipeClosed) {
        return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
          writeGuestUint32(nreadPtr, 0)
        );
      }

      const chunk = __agentOSWasiMeasurePhase('fd_read', 'host_io', () =>
        dequeuePipeBytes(handle.pipe, requestedLength)
      );
      const written = __agentOSWasiMeasurePhase('fd_read', 'guest_iov_write', () =>
        writeBytesToGuestIovs(iovs, iovsLen, chunk)
      );
      return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
        writeGuestUint32(nreadPtr, written)
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'guest-file') {
    try {
      const requestedLength = __agentOSWasiMeasurePhase('fd_read', 'iov_scan', () => {
        if (!(instanceMemory instanceof WebAssembly.Memory)) {
          return 0;
        }
        const view = new DataView(instanceMemory.buffer);
        let total = 0;
        for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
          const entryOffset = (Number(iovs) >>> 0) + index * 8;
          total += view.getUint32(entryOffset + 4, true);
        }
        return total >>> 0;
      });
      const buffer = Buffer.alloc(requestedLength);
      const bytesRead = __agentOSWasiMeasurePhase('fd_read', 'host_io', () =>
        fsModule.readSync(
          handle.targetFd,
          buffer,
          0,
          requestedLength,
          handle.position ?? 0,
        )
      );
      handle.position = (handle.position ?? 0) + bytesRead;
      const written = __agentOSWasiMeasurePhase('fd_read', 'guest_iov_write', () =>
        writeBytesToGuestIovs(iovs, iovsLen, buffer.subarray(0, bytesRead))
      );
      return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
        writeGuestUint32(nreadPtr, written)
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'passthrough' && typeof handle.ioFd === 'number') {
    try {
      const requestedLength = __agentOSWasiMeasurePhase('fd_read', 'iov_scan', () => {
        if (!(instanceMemory instanceof WebAssembly.Memory)) {
          return 0;
        }
        const view = new DataView(instanceMemory.buffer);
        let total = 0;
        for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
          const entryOffset = (Number(iovs) >>> 0) + index * 8;
          total += view.getUint32(entryOffset + 4, true);
        }
        return total >>> 0;
      });
      const buffer = Buffer.alloc(requestedLength);
      const bytesRead = __agentOSWasiMeasurePhase('fd_read', 'host_io', () =>
        fsModule.readSync(
          handle.ioFd,
          buffer,
          0,
          requestedLength,
          handle.position ?? 0,
        )
      );
      handle.position = (handle.position ?? 0) + bytesRead;
      const written = __agentOSWasiMeasurePhase('fd_read', 'guest_iov_write', () =>
        writeBytesToGuestIovs(iovs, iovsLen, buffer.subarray(0, bytesRead))
      );
      return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
        writeGuestUint32(nreadPtr, written)
      );
    } catch (error) {
      return mapSyntheticFsError(error);
    }
  }

  if (
    numericFd === 0 &&
    handle?.kind === 'passthrough' &&
    handle.targetFd === 0 &&
    passthroughHandles.get(0) === handle
  ) {
    const sidecarManagedProcess =
      typeof process?.env?.AGENTOS_SANDBOX_ROOT === 'string' &&
      process.env.AGENTOS_SANDBOX_ROOT.length > 0;
    if (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC) {
      try {
        const requestedLength = __agentOSWasiMeasurePhase('fd_read', 'iov_scan', () => {
          if (!(instanceMemory instanceof WebAssembly.Memory)) {
            return 0;
          }
          const view = new DataView(instanceMemory.buffer);
          let total = 0;
          for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
            const entryOffset = (Number(iovs) >>> 0) + index * 8;
            total += view.getUint32(entryOffset + 4, true);
          }
          return total >>> 0;
        });
        const chunk = __agentOSWasiMeasurePhase('fd_read', 'kernel_stdin_read', () =>
          readKernelStdinChunk(requestedLength)
        );
        if (!chunk || chunk.length === 0) {
          return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
            writeGuestUint32(nreadPtr, 0)
          );
        }
        const written = __agentOSWasiMeasurePhase('fd_read', 'guest_iov_write', () =>
          writeBytesToGuestIovs(iovs, iovsLen, chunk)
        );
        return __agentOSWasiMeasurePhase('fd_read', 'result_marshal', () =>
          writeGuestUint32(nreadPtr, written)
        );
      } catch {
        return WASI_ERRNO_FAULT;
      }
    }
  }

  if (!handle && numericFd <= 2) {
    return WASI_ERRNO_BADF;
  }

  if (handle?.kind === 'passthrough') {
    return delegateManagedFdRead
      ? __agentOSWasiMeasurePhase('fd_read', 'delegate_call', () =>
          delegateManagedFdRead(handle.targetFd, iovs, iovsLen, nreadPtr)
        )
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(numericFd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdRead
    ? __agentOSWasiMeasurePhase('fd_read', 'delegate_call', () =>
        delegateManagedFdRead(numericFd, iovs, iovsLen, nreadPtr)
      )
    : WASI_ERRNO_BADF;
};

wasiImport.fd_pread = (fd, iovs, iovsLen, offset, nreadPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    try {
      const requestedLength = (() => {
        if (!(instanceMemory instanceof WebAssembly.Memory)) {
          return 0;
        }
        const view = new DataView(instanceMemory.buffer);
        let total = 0;
        for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
          const entryOffset = (Number(iovs) >>> 0) + index * 8;
          total += view.getUint32(entryOffset + 4, true);
        }
        return total >>> 0;
      })();
      const buffer = Buffer.alloc(requestedLength);
      const bytesRead = fsModule.readSync(
        handle.targetFd,
        buffer,
        0,
        requestedLength,
        Number(offset),
      );
      const written = writeBytesToGuestIovs(iovs, iovsLen, buffer.subarray(0, bytesRead));
      return writeGuestUint32(nreadPtr, written);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'passthrough') {
    if (typeof handle.ioFd === 'number') {
      try {
        const requestedLength = (() => {
          if (!(instanceMemory instanceof WebAssembly.Memory)) {
            return 0;
          }
          const view = new DataView(instanceMemory.buffer);
          let total = 0;
          for (let index = 0; index < (Number(iovsLen) >>> 0); index += 1) {
            const entryOffset = (Number(iovs) >>> 0) + index * 8;
            total += view.getUint32(entryOffset + 4, true);
          }
          return total >>> 0;
        })();
        const buffer = Buffer.alloc(requestedLength);
        const bytesRead = fsModule.readSync(
          handle.ioFd,
          buffer,
          0,
          requestedLength,
          Number(offset),
        );
        const written = writeBytesToGuestIovs(iovs, iovsLen, buffer.subarray(0, bytesRead));
        return writeGuestUint32(nreadPtr, written);
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    return delegateFdPread
      ? delegateFdPread(handle.targetFd, iovs, iovsLen, offset, nreadPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateFdPread
    ? delegateFdPread(fd, iovs, iovsLen, offset, nreadPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_pwrite = (fd, iovs, iovsLen, offset, nwrittenPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    try {
      const bytes = collectGuestIovBytes(iovs, iovsLen);
      const written = fsModule.writeSync(
        handle.targetFd,
        bytes,
        0,
        bytes.length,
        Number(offset),
      );
      return writeGuestUint32(nwrittenPtr, written);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'passthrough') {
    if (handle.readOnly === true) {
      return WASI_ERRNO_ROFS;
    }
    if (typeof handle.ioFd === 'number') {
      try {
        const bytes = collectGuestIovBytes(iovs, iovsLen);
        const written = fsModule.writeSync(
          handle.ioFd,
          bytes,
          0,
          bytes.length,
          Number(offset),
        );
        // A positioned write can grow the file past a size remembered from a
        // prior truncate; drop the stale entry so fd_size/path_size fall
        // through to the authoritative fstat.
        forgetHostFsSize(handle.guestPath);
        return writeGuestUint32(nwrittenPtr, written);
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    return delegateManagedFdPwrite
      ? delegateManagedFdPwrite(handle.targetFd, iovs, iovsLen, offset, nwrittenPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdPwrite
    ? delegateManagedFdPwrite(fd, iovs, iovsLen, offset, nwrittenPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_sync = (fd) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    return WASI_ERRNO_SUCCESS;
  }

  if (handle?.kind === 'passthrough') {
    return delegateFdSync ? delegateFdSync(handle.targetFd) : WASI_ERRNO_SUCCESS;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateFdSync ? delegateFdSync(fd) : WASI_ERRNO_SUCCESS;
};

wasiImport.fd_seek = (fd, offset, whence, newOffsetPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    try {
      const next = seekGuestFileHandle(handle, offset, whence);
      if (next == null) {
        return WASI_ERRNO_INVAL;
      }
      return writeGuestUint64(newOffsetPtr, next);
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_SPIPE;
  }

  if (handle?.kind === 'passthrough') {
    if (typeof handle.ioFd === 'number') {
      try {
        const next = seekGuestFileHandle(handle, offset, whence);
        if (next == null) {
          return WASI_ERRNO_INVAL;
        }
        return writeGuestUint64(newOffsetPtr, next);
      } catch {
        return WASI_ERRNO_FAULT;
      }
    }
    return delegateManagedFdSeek
      ? delegateManagedFdSeek(handle.targetFd, offset, whence, newOffsetPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdSeek
    ? delegateManagedFdSeek(fd, offset, whence, newOffsetPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_tell = (fd, offsetPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    return writeGuestUint64(offsetPtr, BigInt(handle.position ?? 0));
  }

  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_SPIPE;
  }

  if (handle?.kind === 'passthrough') {
    if (typeof handle.ioFd === 'number') {
      return writeGuestUint64(offsetPtr, BigInt(handle.position ?? 0));
    }
    return delegateManagedFdTell
      ? delegateManagedFdTell(handle.targetFd, offsetPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdTell
    ? delegateManagedFdTell(fd, offsetPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_fdstat_get = (fd, statPtr) => {
  const handle = __agentOSWasiMeasurePhase('fd_fdstat_get', 'lookup_handle', () =>
    lookupFdHandle(fd)
  );
  // Kernel-PTY stdio must report CHARACTER_DEVICE so guest is_terminal()/
  // isatty() see the TTY (the runner-process fds behind the delegate are
  // pipes). Resolve dup'd passthrough handles to their target fd first.
  {
    const stdioFd =
      handle?.kind === 'passthrough' ? Number(handle.targetFd) >>> 0 : Number(fd) >>> 0;
    if ((handle == null || handle.kind === 'passthrough') && stdioFd <= 2 &&
        stdioFdIsKernelTty(stdioFd)) {
      return __agentOSWasiMeasurePhase(
        'fd_fdstat_get',
        'marshal_fdstat',
        () => writeGuestFdstat(
          statPtr,
          WASI_FILETYPE_CHARACTER_DEVICE,
          0,
          WASI_RIGHT_FD_READ |
            WASI_RIGHT_FD_WRITE |
            WASI_RIGHT_FD_FDSTAT_SET_FLAGS |
            WASI_RIGHT_FD_FILESTAT_GET |
            WASI_RIGHT_POLL_FD_READWRITE,
          0n,
        ),
      );
    }
  }
  if (handle?.kind === 'pipe-read') {
    return __agentOSWasiMeasurePhase(
      'fd_fdstat_get',
      'marshal_fdstat',
      () => writeGuestFdstat(
        statPtr,
        WASI_FILETYPE_UNKNOWN,
        0,
        WASI_RIGHT_FD_READ |
          WASI_RIGHT_FD_FDSTAT_SET_FLAGS |
          WASI_RIGHT_FD_FILESTAT_GET |
          WASI_RIGHT_POLL_FD_READWRITE,
        0n,
      ),
    );
  }

  if (handle?.kind === 'pipe-write') {
    return __agentOSWasiMeasurePhase(
      'fd_fdstat_get',
      'marshal_fdstat',
      () => writeGuestFdstat(
        statPtr,
        WASI_FILETYPE_UNKNOWN,
        0,
        WASI_RIGHT_FD_WRITE |
          WASI_RIGHT_FD_FDSTAT_SET_FLAGS |
          WASI_RIGHT_FD_FILESTAT_GET |
          WASI_RIGHT_POLL_FD_READWRITE,
        0n,
      ),
    );
  }

  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_BADF;
  }

  if (handle?.kind === 'passthrough') {
    return delegateManagedFdFdstatGet
      ? __agentOSWasiMeasurePhase('fd_fdstat_get', 'delegate_call', () =>
          delegateManagedFdFdstatGet(handle.targetFd, statPtr)
        )
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdFdstatGet
    ? __agentOSWasiMeasurePhase('fd_fdstat_get', 'delegate_call', () =>
        delegateManagedFdFdstatGet(fd, statPtr)
      )
    : WASI_ERRNO_BADF;
};

wasiImport.fd_fdstat_set_flags = (fd, flags) => {
  const handle = lookupFdHandle(fd);
  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_BADF;
  }

  if (handle?.kind === 'passthrough') {
    return delegateManagedFdFdstatSetFlags
      ? delegateManagedFdFdstatSetFlags(handle.targetFd, flags)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdFdstatSetFlags
    ? delegateManagedFdFdstatSetFlags(fd, flags)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_filestat_get = (fd, statPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    try {
      return writeGuestFilestat(statPtr, fsModule.fstatSync(handle.targetFd));
    } catch (error) {
      return mapSyntheticFsError(error);
    }
  }

  if (handle?.kind === 'passthrough') {
    if (typeof handle.ioFd === 'number') {
      try {
        return writeGuestFilestat(statPtr, fsModule.fstatSync(handle.ioFd));
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    return delegateManagedFdFilestatGet
      ? delegateManagedFdFilestatGet(handle.targetFd, statPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdFilestatGet
    ? delegateManagedFdFilestatGet(fd, statPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_filestat_set_size = (fd, size) => {
  const handle = lookupFdHandle(fd);
  if (handle?.kind === 'guest-file') {
    try {
      const nextSize = Number(size);
      fsModule.ftruncateSync(handle.targetFd, nextSize);
      if ((handle.position ?? 0) > nextSize) {
        handle.position = nextSize;
      }
      rememberHostFsSize(handle.guestPath, nextSize);
      return WASI_ERRNO_SUCCESS;
    } catch (error) {
      return mapSyntheticFsError(error);
    }
  }

  if (handle?.kind === 'passthrough') {
    if (handle.readOnly === true) {
      return WASI_ERRNO_ROFS;
    }
    if (typeof handle.ioFd === 'number') {
      try {
        const nextSize = Number(size);
        fsModule.ftruncateSync(handle.ioFd, nextSize);
        if ((handle.position ?? 0) > nextSize) {
          handle.position = nextSize;
        }
        rememberHostFsSize(handle.guestPath, nextSize);
        return WASI_ERRNO_SUCCESS;
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    if (typeof handle.guestPath === 'string') {
      try {
        const nextSize = Number(size);
        const pathFd = fsModule.openSync(handle.guestPath, 0o1, 0o666);
        try {
          fsModule.ftruncateSync(pathFd, nextSize);
          if ((handle.position ?? 0) > nextSize) {
            handle.position = nextSize;
          }
          rememberHostFsSize(handle.guestPath, nextSize);
        } finally {
          fsModule.closeSync(pathFd);
        }
        return WASI_ERRNO_SUCCESS;
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    return delegateManagedFdFilestatSetSize
      ? delegateManagedFdFilestatSetSize(handle.targetFd, size)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdFilestatSetSize
    ? delegateManagedFdFilestatSetSize(fd, size)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_prestat_get = (fd, prestatPtr) => {
  const handle = lookupFdHandle(fd);
  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_BADF;
  }

  if (handle?.kind === 'passthrough') {
    return delegateManagedFdPrestatGet
      ? delegateManagedFdPrestatGet(handle.targetFd, prestatPtr)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdPrestatGet
    ? delegateManagedFdPrestatGet(fd, prestatPtr)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_prestat_dir_name = (fd, pathPtr, pathLen) => {
  const handle = lookupFdHandle(fd);
  if (handle && handle.kind !== 'passthrough') {
    return WASI_ERRNO_BADF;
  }

  if (handle?.kind === 'passthrough') {
    return delegateManagedFdPrestatDirName
      ? delegateManagedFdPrestatDirName(handle.targetFd, pathPtr, pathLen)
      : WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdPrestatDirName
    ? delegateManagedFdPrestatDirName(fd, pathPtr, pathLen)
    : WASI_ERRNO_BADF;
};

wasiImport.fd_write = (fd, iovs, iovsLen, nwrittenPtr) => {
  const handle = __agentOSWasiMeasurePhase('fd_write', 'lookup_handle', () =>
    lookupFdHandle(fd)
  );
  const numericFd = Number(fd) >>> 0;
  if (handle?.kind === 'pipe-write') {
    try {
      const bytes = __agentOSWasiMeasurePhase('fd_write', 'guest_iov_collect', () =>
        collectGuestIovBytes(iovs, iovsLen)
      );
      if (bytes.length > 0 && !pipeHasReaders(handle.pipe)) {
        return WASI_ERRNO_PIPE;
      }
      __agentOSWasiMeasurePhase('fd_write', 'host_io', () => {
        enqueuePipeBytes(handle.pipe, bytes);
        flushPipeConsumers(handle.pipe);
      });
      return __agentOSWasiMeasurePhase('fd_write', 'result_marshal', () =>
        writeGuestUint32(nwrittenPtr, bytes.length)
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'guest-file') {
    try {
      const bytes = __agentOSWasiMeasurePhase('fd_write', 'guest_iov_collect', () =>
        collectGuestIovBytes(iovs, iovsLen)
      );
      const written = __agentOSWasiMeasurePhase('fd_write', 'host_io', () =>
        writeBytesToGuestFileHandle(handle, bytes)
      );
      return __agentOSWasiMeasurePhase('fd_write', 'result_marshal', () =>
        writeGuestUint32(nwrittenPtr, written)
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (numericFd === 1 || numericFd === 2) {
    try {
      const bytes = __agentOSWasiMeasurePhase('fd_write', 'guest_iov_collect', () =>
        collectGuestIovBytes(iovs, iovsLen)
      );
      const sidecarManagedProcess =
        typeof process?.env?.AGENTOS_SANDBOX_ROOT === 'string' &&
        process.env.AGENTOS_SANDBOX_ROOT.length > 0;
      if (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC) {
        const written = __agentOSWasiMeasurePhase('fd_write', 'sync_rpc', () =>
          Number(callSyncRpc('__kernel_stdio_write', [numericFd, bytes])) >>> 0
        );
        return __agentOSWasiMeasurePhase('fd_write', 'result_marshal', () =>
          writeGuestUint32(nwrittenPtr, written)
        );
      }
      __agentOSWasiMeasurePhase('fd_write', 'host_io', () =>
        (numericFd === 1 ? process.stdout : process.stderr).write(bytes)
      );
      return __agentOSWasiMeasurePhase('fd_write', 'result_marshal', () =>
        writeGuestUint32(nwrittenPtr, bytes.length)
      );
    } catch {
      return WASI_ERRNO_FAULT;
    }
  }

  if (handle?.kind === 'passthrough') {
    if (handle.readOnly === true) {
      return WASI_ERRNO_ROFS;
    }
    if (typeof handle.ioFd === 'number') {
      try {
        const bytes = __agentOSWasiMeasurePhase('fd_write', 'guest_iov_collect', () =>
          collectGuestIovBytes(iovs, iovsLen)
        );
        const written = __agentOSWasiMeasurePhase('fd_write', 'host_io', () =>
          writeBytesToGuestFileHandle({ ...handle, targetFd: handle.ioFd }, bytes)
        );
        if (handle.append) {
          handle.position = Number(fsModule.fstatSync(handle.ioFd).size ?? 0);
        } else {
          handle.position = (handle.position ?? 0) + written;
        }
        // The write grew/changed the file; a size remembered from a prior
        // truncate is now stale. Drop it so fd_size/path_size fall through to
        // the authoritative fstat rather than reporting the old length.
        forgetHostFsSize(handle.guestPath);
        return __agentOSWasiMeasurePhase('fd_write', 'result_marshal', () =>
          writeGuestUint32(nwrittenPtr, written)
        );
      } catch (error) {
        return mapSyntheticFsError(error);
      }
    }
    return delegateManagedFdWrite
      ? __agentOSWasiMeasurePhase('fd_write', 'delegate_call', () =>
          delegateManagedFdWrite(handle.targetFd, iovs, iovsLen, nwrittenPtr)
        )
      : WASI_ERRNO_BADF;
  }

  if (!handle && numericFd <= 2) {
    return WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  return delegateManagedFdWrite
    ? __agentOSWasiMeasurePhase('fd_write', 'delegate_call', () =>
        delegateManagedFdWrite(fd, iovs, iovsLen, nwrittenPtr)
      )
    : WASI_ERRNO_BADF;
};

wasiImport.fd_close = (fd) => {
  traceHostProcess('fd-close-begin', {
    fd: Number(fd) >>> 0,
    syntheticKind: syntheticFdEntries.get(Number(fd) >>> 0)?.kind ?? null,
    passthroughKind: passthroughHandles.get(Number(fd) >>> 0)?.kind ?? null,
  });
  if (__agentOSWasiMeasurePhase('fd_close', 'synthetic_close', () => closeSyntheticFd(fd))) {
    traceHostProcess('fd-close-synthetic', { fd: Number(fd) >>> 0 });
    return WASI_ERRNO_SUCCESS;
  }

  const handle = __agentOSWasiMeasurePhase('fd_close', 'lookup_handle', () =>
    lookupFdHandle(fd)
  );
  if (handle?.kind === 'passthrough') {
    traceHostProcess('fd-close-passthrough', {
      fd: Number(fd) >>> 0,
      targetFd: handle.targetFd ?? null,
    });
    __agentOSWasiMeasurePhase('fd_close', 'fd_bookkeeping', () => closePassthroughFd(fd));
    return WASI_ERRNO_SUCCESS;
  }

  if (!handle && Number(fd) >>> 0 <= 2) {
    return WASI_ERRNO_BADF;
  }

  if (rejectClosedPassthroughFd(fd)) {
    return WASI_ERRNO_BADF;
  }

  if (delegateManagedFdRefCounts.has(Number(fd) >>> 0)) {
    const shouldDelegateClose = __agentOSWasiMeasurePhase('fd_close', 'fd_bookkeeping', () =>
      releaseDelegateFd(fd)
    );
    traceHostProcess('fd-close-delegate-tracked', {
      fd: Number(fd) >>> 0,
      shouldDelegateClose,
      remainingRefs: delegateManagedFdRefCounts.get(Number(fd) >>> 0) ?? 0,
    });
    if (!shouldDelegateClose) {
      return WASI_ERRNO_SUCCESS;
    }
    passthroughHandles.delete(Number(fd) >>> 0);
  }

  traceHostProcess('fd-close-delegate', { fd: Number(fd) >>> 0 });
  return delegateManagedFdClose
    ? __agentOSWasiMeasurePhase('fd_close', 'delegate_call', () =>
        delegateManagedFdClose(fd)
      )
    : WASI_ERRNO_BADF;
};

wasiImport.fd_renumber = (from, to) => {
  try {
    const sourceFd = Number(from) >>> 0;
    const targetFd = Number(to) >>> 0;
    if (sourceFd === targetFd) {
      return lookupFdHandle(sourceFd) || delegateManagedFdRefCounts.has(sourceFd)
        ? WASI_ERRNO_SUCCESS
        : WASI_ERRNO_BADF;
    }

    const syntheticHandle = syntheticFdEntries.get(sourceFd);
    const passthroughHandle = passthroughHandles.get(sourceFd);
    const retainedSpawnOutputHandle = retainedSpawnOutputHandlesByFd.get(sourceFd);
    if (!syntheticHandle && !passthroughHandle && !retainedSpawnOutputHandle) {
      if (rejectClosedPassthroughFd(sourceFd)) {
        return WASI_ERRNO_BADF;
      }
      return delegateManagedFdRenumber
        ? delegateManagedFdRenumber(sourceFd, targetFd)
        : WASI_ERRNO_BADF;
    }

    if (
      syntheticFdEntries.has(targetFd) ||
      passthroughHandles.has(targetFd) ||
      retainedSpawnOutputHandlesByFd.has(targetFd) ||
      delegateManagedFdRefCounts.has(targetFd)
    ) {
      const closeResult = wasiImport.fd_close(targetFd);
      if (closeResult !== WASI_ERRNO_SUCCESS) {
        return closeResult;
      }
    }

    if (syntheticHandle) {
      syntheticFdEntries.delete(sourceFd);
      syntheticFdEntries.set(targetFd, syntheticHandle);
    } else if (passthroughHandle) {
      passthroughHandles.delete(sourceFd);
      passthroughHandles.set(targetFd, passthroughHandle);
      closedPassthroughFds.add(sourceFd);
      closedPassthroughFds.delete(targetFd);
    } else {
      retainedSpawnOutputHandlesByFd.delete(sourceFd);
      retainedSpawnOutputHandlesByFd.set(targetFd, retainedSpawnOutputHandle);
    }

    nextSyntheticFd = Math.max(nextSyntheticFd, targetFd + 1);
    traceHostProcess('fd-renumber', {
      from: sourceFd,
      to: targetFd,
      syntheticKind: syntheticHandle?.kind ?? null,
      passthroughKind: passthroughHandle?.kind ?? null,
    });
    return WASI_ERRNO_SUCCESS;
  } catch {
    return WASI_ERRNO_FAULT;
  }
};

wasiImport.poll_oneoff = (inPtr, outPtr, nsubscriptions, neventsPtr) => {
  if (!(instanceMemory instanceof WebAssembly.Memory)) {
    return delegateManagedPollOneoff
      ? delegateManagedPollOneoff(inPtr, outPtr, nsubscriptions, neventsPtr)
      : WASI_ERRNO_FAULT;
  }

  const subscriptionCount = Number(nsubscriptions) >>> 0;
  if (subscriptionCount === 0) {
    return writeGuestUint32(neventsPtr, 0);
  }

  const subscriptionSize = 48;
  const eventSize = 32;
  const view = new DataView(instanceMemory.buffer);
  const memory = new Uint8Array(instanceMemory.buffer);
  const subscriptions = [];
  let hasSyntheticSubscription = false;
  let hasRemappedPassthroughSubscription = false;
  const sidecarManagedProcess =
    typeof process?.env?.AGENTOS_SANDBOX_ROOT === 'string' &&
    process.env.AGENTOS_SANDBOX_ROOT.length > 0;
  let timeoutMs = null;

  for (let index = 0; index < subscriptionCount; index += 1) {
    const base = (Number(inPtr) >>> 0) + index * subscriptionSize;
    const tag = view.getUint8(base + 8);
    const userdata = memory.slice(base, base + 8);
    if (tag === 0) {
      const timeoutNs = view.getBigUint64(base + 24, true);
      const relativeTimeoutMs = Number(timeoutNs / 1000000n);
      timeoutMs =
        timeoutMs == null ? relativeTimeoutMs : Math.min(timeoutMs, relativeTimeoutMs);
      subscriptions.push({ kind: 'clock', userdata });
      continue;
    }

    if (tag !== 1 && tag !== 2) {
      subscriptions.push({ kind: 'unsupported', userdata });
      continue;
    }

    const fd = view.getUint32(base + 16, true);
    const handle = lookupFdHandle(fd);
    if (!handle && rejectClosedPassthroughFd(fd)) {
      hasSyntheticSubscription = true;
      subscriptions.push({
        kind: tag === 1 ? 'fd_read' : 'fd_write',
        fd,
        handle,
        userdata,
        error: WASI_ERRNO_BADF,
      });
      continue;
    }
    if (handle && handle.kind !== 'passthrough') {
      hasSyntheticSubscription = true;
    } else if (handle?.kind === 'passthrough') {
      const targetFd = Number(handle.targetFd) >>> 0;
      if (
        targetFd !== fd ||
        (fd === 0 && (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC))
      ) {
        hasRemappedPassthroughSubscription = true;
      }
    } else if (!handle && fd <= 2 && (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC)) {
      // Kernel-managed stdio with no local handle: fd 0/1/2 map straight to
      // the kernel PTY/pipes, so readiness must come from __kernel_poll — the
      // delegate's fds are the runner process's own stdio and never fire for
      // guest terminal input (vim's RealWaitForChar polls fd 0 this way).
      hasRemappedPassthroughSubscription = true;
    }
    subscriptions.push({
      kind: tag === 1 ? 'fd_read' : 'fd_write',
      fd,
      handle,
      userdata,
    });
  }

  const hasClockSubscription = subscriptions.some((subscription) => subscription.kind === 'clock');

  if (!hasSyntheticSubscription && !hasRemappedPassthroughSubscription && !hasClockSubscription) {
    return delegateManagedPollOneoff
      ? delegateManagedPollOneoff(inPtr, outPtr, nsubscriptions, neventsPtr)
      : WASI_ERRNO_BADF;
  }

  const deadline = timeoutMs == null ? null : Date.now() + Math.max(0, timeoutMs);
  const readyEvents = [];

  function collectKernelReadyEvents(waitMs) {
    if (!hasRemappedPassthroughSubscription) {
      return [];
    }

    const kernelPollFdFor = (subscription) => {
      if (subscription.kind !== 'fd_read' && subscription.kind !== 'fd_write') {
        return null;
      }
      const fd = Number(subscription.fd) >>> 0;
      if (subscription.handle?.kind === 'passthrough') {
        const targetFd = Number(subscription.handle.targetFd) >>> 0;
        if (targetFd !== fd || (fd === 0 && (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC))) {
          return targetFd;
        }
        return null;
      }
      if (!subscription.handle && fd <= 2 && (sidecarManagedProcess || KERNEL_STDIO_SYNC_RPC)) {
        return fd;
      }
      return null;
    };
    const pollTargets = subscriptions
      .filter((subscription) => kernelPollFdFor(subscription) != null)
      .map((subscription) => ({
        fd: kernelPollFdFor(subscription),
        events: subscription.kind === 'fd_read' ? KERNEL_POLLIN : KERNEL_POLLOUT,
      }));
    if (pollTargets.length === 0) {
      return [];
    }

    let response;
    try {
      response = callSyncRpc('__kernel_poll', [
        pollTargets,
        Math.max(0, Number(waitMs) >>> 0),
      ]);
    } catch {
      return [];
    }

    const responseEntries = Array.isArray(response?.fds) ? response.fds : [];
    const ready = [];
    for (const subscription of subscriptions) {
      const kernelFd = kernelPollFdFor(subscription);
      if (kernelFd == null) {
        continue;
      }

      const targetFd = kernelFd;
      const responseEntry = responseEntries.find(
        (entry) => (Number(entry?.fd) >>> 0) === targetFd
      );
      const revents = Number(responseEntry?.revents) >>> 0;
      const interested =
        subscription.kind === 'fd_read'
          ? KERNEL_POLLIN | KERNEL_POLLERR | KERNEL_POLLHUP
          : KERNEL_POLLOUT | KERNEL_POLLERR | KERNEL_POLLHUP;
      if ((revents & interested) === 0) {
        continue;
      }

      ready.push({
        userdata: subscription.userdata,
        error: WASI_ERRNO_SUCCESS,
        type: subscription.kind === 'fd_read' ? 1 : 2,
        nbytes: subscription.kind === 'fd_read' ? 1 : 65536,
        flags: 0,
      });
    }
    return ready;
  }

  while (readyEvents.length === 0) {
    dispatchPendingWasmSignals();
    for (const subscription of subscriptions) {
      if (subscription.error != null) {
        readyEvents.push({
          userdata: subscription.userdata,
          error: subscription.error,
          type: subscription.kind === 'fd_read' ? 1 : 2,
          nbytes: 0,
          flags: 0,
        });
        continue;
      }

      if (subscription.kind === 'fd_read' && subscription.handle?.kind === 'pipe-read') {
        const pipe = subscription.handle.pipe;
        if (pipe.chunks.length > 0 || (pipe.writeHandleCount === 0 && pipe.producers.size === 0)) {
          readyEvents.push({
            userdata: subscription.userdata,
            error: WASI_ERRNO_SUCCESS,
            type: 1,
            nbytes: pipe.chunks[0]?.length ?? 0,
            flags: 0,
          });
        }
        continue;
      }

      if (subscription.kind === 'fd_write' && subscription.handle?.kind === 'pipe-write') {
        readyEvents.push({
          userdata: subscription.userdata,
          error: WASI_ERRNO_SUCCESS,
          type: 2,
          nbytes: 65536,
          flags: 0,
        });
        continue;
      }
    }

    if (readyEvents.length > 0) {
      break;
    }

    if (hasRemappedPassthroughSubscription) {
      // Kernel fds wait event-driven in the sidecar (parked RPC), so the slice
      // can be long. Synthetic (pipe) subscriptions still need short local
      // pumping, so only use the long slice when every subscription is
      // kernel-backed.
      const kernelWaitMs = hasSyntheticSubscription
        ? deadline == null
          ? 10
          : Math.max(0, Math.min(10, deadline - Date.now()))
        : deadline == null
          ? KERNEL_WAIT_SLICE_MS
          : Math.max(0, Math.min(KERNEL_WAIT_SLICE_MS, deadline - Date.now()));
      readyEvents.push(...collectKernelReadyEvents(kernelWaitMs));
      if (readyEvents.length > 0) {
        break;
      }
    }

    let pumped = false;
    for (const subscription of subscriptions) {
      if (subscription.kind === 'fd_read' && subscription.handle?.kind === 'pipe-read') {
        pumped = pumpPipeProducers(subscription.handle.pipe, 10) || pumped;
      }
    }

    if (pumped) {
      continue;
    }

    if (deadline != null && Date.now() >= deadline) {
      break;
    }

    Atomics.wait(
      syntheticWaitArray,
      0,
      0,
      deadline == null ? 10 : Math.max(0, Math.min(10, deadline - Date.now())),
    );
  }

    if (readyEvents.length === 0 && hasClockSubscription) {
    const clockSubscription = subscriptions.find((subscription) => subscription.kind === 'clock');
    readyEvents.push({
      userdata: clockSubscription.userdata,
      error: WASI_ERRNO_SUCCESS,
      type: 0,
      nbytes: 0,
      flags: 0,
    });
  }

  for (let index = 0; index < readyEvents.length; index += 1) {
    const base = (Number(outPtr) >>> 0) + index * eventSize;
    const event = readyEvents[index];
    memory.set(event.userdata, base);
    view.setUint16(base + 8, event.error, true);
    view.setUint8(base + 10, event.type);
    view.setBigUint64(base + 16, BigInt(event.nbytes), true);
    view.setUint16(base + 24, event.flags, true);
  }

  return writeGuestUint32(neventsPtr, readyEvents.length);
};

// Terminal event source for crossterm-based guests (brush shell, reedline).
// The patched crossterm WasiEventSource reads keystrokes through this import:
//   read(ptr, len, timeout_ms) -> usize
// It performs a timed read of the guest's stdin (the kernel PTY/pipe) and copies
// the bytes into guest memory, returning the count (0 on timeout / EOF). usize::MAX
// (-1 as i32) means block until input. Backed by the same __kernel_stdin_read RPC
// the wasi fd_read path uses, so it works identically under native and browser.
const hostTtyImport = {
  // Long event-driven waits: the sidecar parks __kernel_stdin_read and replies
  // when the PTY becomes readable (reply-by-token), so a host->guest write the
  // caller is waiting for (e.g. the CPR reply to crossterm's cursor-position
  // query) lands immediately — no polling slices, no self-deadlock.
  read(ptr, len, timeoutMs) {
    const cap = Number(len) >>> 0;
    if (cap === 0) return 0;
    const blocking = (timeoutMs >>> 0) === 0xffffffff;
    const deadline = blocking ? Infinity : Date.now() + (Number(timeoutMs) >>> 0);
    while (true) {
      const remaining = deadline - Date.now();
      if (remaining <= 0) return 0;
      const response = callSyncRpc('__kernel_stdin_read', [
        cap,
        Math.min(remaining, KERNEL_WAIT_SLICE_MS),
      ]);
      if (response && typeof response.dataBase64 === 'string') {
        const bytes = Buffer.from(response.dataBase64, 'base64');
        const n = Math.min(bytes.length, cap);
        if (n > 0) {
          new Uint8Array(instanceMemory.buffer).set(bytes.subarray(0, n), ptr >>> 0);
          return n;
        }
      }
      if (response && response.done === true) return 0;
    }
  },
  // `host_tty.isatty(fd)` -> 1 if the guest fd is a kernel PTY, else 0.
  isatty(fd) {
    const descriptor = Number(fd) >>> 0;
    return descriptor <= 2 && stdioFdIsKernelTty(descriptor) ? 1 : 0;
  },
  // `host_tty.get_size(fd, colsPtr, rowsPtr)` -> writes the PTY window size as two
  // little-endian u16s and returns 0; non-zero (ENOTTY) if fd is not a PTY.
  get_size(fd, colsPtr, rowsPtr) {
    const size = callSyncRpc('__kernel_tty_size', [fd >>> 0]);
    if (!size || typeof size.cols !== 'number' || typeof size.rows !== 'number') {
      return 25; // ENOTTY
    }
    const view = new DataView(instanceMemory.buffer);
    view.setUint16(colsPtr >>> 0, size.cols & 0xffff, true);
    view.setUint16(rowsPtr >>> 0, size.rows & 0xffff, true);
    return 0;
  },
  // Toggle terminal raw mode on the guest's PTY. crossterm/pty_probe/vim call this
  // instead of tcsetattr; route it to the kernel so the guest gets raw keystrokes.
  set_raw_mode(enabled) {
    if (!stdioFdIsKernelTty(0)) {
      return 25; // ENOTTY
    }
    callSyncRpc('__pty_set_raw_mode', [(enabled >>> 0) !== 0]);
    return 0;
  },
};

function __agentOSWasiWrapImport(name, delegate) {
  if (!__agentOSWasiSyscallPhasesEnabled || typeof delegate !== 'function') {
    return delegate;
  }
  return (...args) => {
    const startedNs = __agentOSWasmNowNs();
    try {
      return delegate(...args);
    } finally {
      __agentOSWasiRecordSyscall(name, startedNs);
    }
  };
}

if (__agentOSWasiSyscallPhasesEnabled) {
  for (const [name, delegate] of Object.entries(wasiImport)) {
    if (typeof delegate === 'function') {
      wasiImport[name] = __agentOSWasiWrapImport(name, delegate.bind(wasiImport));
    }
  }
}

const instance = __agentOSWasmMeasurePhase('WebAssembly.Instance', () => new WebAssembly.Instance(module, {
  wasi_snapshot_preview1: wasiImport,
  wasi_unstable: wasiImport,
  host_tty: hostTtyImport,
  // Read-write commands like DuckDB need fd_dup_min from the patched
  // wasi-libc surface, but broader host_process capabilities stay
  // reserved for the full tier.
  host_process:
    permissionTier === 'full'
      ? hostProcessImport
      : permissionTier === 'isolated'
        ? undefined
        : limitedHostProcessImport,
  host_net: permissionTier === 'full' ? hostNetImport : undefined,
  host_user: hostUserImport,
  host_fs: hostFsImport,
}));

if (instance.exports.memory instanceof WebAssembly.Memory) {
  instanceMemory = instance.exports.memory;
}

function dispatchWasmSignal(signal) {
  const numeric = Number(signal) | 0;
  if (
    numeric > 0 &&
    typeof instance?.exports?.__wasi_signal_trampoline === 'function'
  ) {
    instance.exports.__wasi_signal_trampoline(numeric);
  }
}

function dispatchPendingWasmSignals() {
  while (pendingWasmSignals.length > 0) {
    dispatchWasmSignal(pendingWasmSignals.shift());
  }
  while (true) {
    let signal;
    try {
      signal = callSyncRpc('process.take_signal', []);
    } catch (error) {
      if (error?.code === 'ERR_AGENTOS_WASM_SYNC_RPC_UNAVAILABLE') {
        return;
      }
      throw error;
    }
    if (typeof signal !== 'number') {
      return;
    }
    dispatchWasmSignal(signal);
  }
}

Object.defineProperty(globalThis, '__secureExecWasmSignalDispatch', {
  configurable: true,
  writable: true,
  value: (_eventType, payload) => {
    const signal =
      typeof payload?.number === 'number'
        ? payload.number
        : signalNumberFromName(payload?.signal);
    if (signal > 0) {
      pendingWasmSignals.push(signal);
    }
  },
});

if (typeof instance.exports._start === 'function') {
  // The `RuntimeError: unreachable` reports that used to point at
  // `WASI.start()` were caused by the host shim around guest startup, not by
  // V8 itself. Standalone runs must keep ordinary stdio on local process
  // streams unless kernel stdio sync-RPC is explicitly enabled, while
  // `poll_oneoff` still routes readiness probes through `__kernel_poll`.
  // That preserves the expected startup ordering so guest `_start` checks can
  // observe the ready event before we exit the runner.
  let exitCode;
  try {
    exitCode = __agentOSWasmMeasurePhase('wasi.start', () => wasi.start(instance));
  } catch (error) {
    __agentOSWasmEmitPhaseMetrics('wasi.start.error', {
      error: error && typeof error === 'object' && 'message' in error ? String(error.message) : String(error),
    });
    if (maxStackBytes !== null && isWasmStackExhaustionTrap(error)) {
      reportConfiguredStackLimitExceeded(error);
      process.exit(1);
    }
    throw error;
  }
  __agentOSWasmEmitPhaseMetrics('complete', { exitCode });
  process.exit(typeof exitCode === 'number' ? exitCode : 0);
} else if (typeof instance.exports.run === 'function') {
  const result = await instance.exports.run();
  if (typeof result !== 'undefined') {
    console.log(String(result));
  }
} else {
  throw new Error('WebAssembly module must export _start or run');
}
