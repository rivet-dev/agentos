use crate::common::stable_hash64;
use crate::node_import_cache::{
    NodeImportCache, NodeImportCacheCleanup, NODE_IMPORT_CACHE_ASSET_ROOT_ENV,
};
use crate::runtime_support::{
    NODE_COMPILE_CACHE_ENV, NODE_DISABLE_COMPILE_CACHE_ENV, NODE_FROZEN_TIME_ENV,
    NODE_SANDBOX_ROOT_ENV,
};
use crate::signal::NodeSignalHandlerRegistration;
use crate::v8_host::{V8RuntimeHost, V8SessionHandle};
use crate::v8_ipc::BinaryFrame;
use crate::v8_runtime;
use agentos_bridge::queue_tracker::{register_queue, TrackedLimit, TrackedReceiver};
use agentos_v8_runtime::runtime_protocol::{RuntimeCommand, WarmSessionHint};
use getrandom::getrandom;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::fd::OwnedFd;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
    Arc, Condvar, Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{
    mpsc::{channel, error::TryRecvError as TokioTryRecvError, Receiver as TokioReceiver},
    Notify,
};
use tokio::time;

const NODE_ENTRYPOINT_ENV: &str = "AGENTOS_ENTRYPOINT";
const NODE_BOOTSTRAP_ENV: &str = "AGENTOS_BOOTSTRAP_MODULE";
const NODE_GUEST_ARGV_ENV: &str = "AGENTOS_GUEST_ARGV";
const NODE_PREWARM_IMPORTS_ENV: &str = "AGENTOS_NODE_PREWARM_IMPORTS";
const NODE_IMPORT_COMPILE_CACHE_NAMESPACE_VERSION: &str = "3";
const NODE_IMPORT_CACHE_LOADER_PATH_ENV: &str = "AGENTOS_NODE_IMPORT_CACHE_LOADER_PATH";
const NODE_IMPORT_CACHE_PATH_ENV: &str = "AGENTOS_NODE_IMPORT_CACHE_PATH";
const NODE_KEEP_STDIN_OPEN_ENV: &str = "AGENTOS_KEEP_STDIN_OPEN";
const NODE_GUEST_ENTRYPOINT_ENV: &str = "AGENTOS_GUEST_ENTRYPOINT";
const NODE_GUEST_ENTRYPOINT_MODULE_MODE_ENV: &str = "AGENTOS_GUEST_ENTRYPOINT_MODULE_MODE";
const NODE_GUEST_PATH_MAPPINGS_ENV: &str = "AGENTOS_GUEST_PATH_MAPPINGS";
const NODE_VIRTUAL_PROCESS_EXEC_PATH_ENV: &str = "AGENTOS_VIRTUAL_PROCESS_EXEC_PATH";
const NODE_VIRTUAL_PROCESS_PID_ENV: &str = "AGENTOS_VIRTUAL_PROCESS_PID";
const NODE_VIRTUAL_PROCESS_PPID_ENV: &str = "AGENTOS_VIRTUAL_PROCESS_PPID";
const NODE_VIRTUAL_PROCESS_UID_ENV: &str = "AGENTOS_VIRTUAL_PROCESS_UID";
const NODE_VIRTUAL_PROCESS_GID_ENV: &str = "AGENTOS_VIRTUAL_PROCESS_GID";
const NODE_PARENT_ALLOW_CHILD_PROCESS_ENV: &str = "AGENTOS_PARENT_NODE_ALLOW_CHILD_PROCESS";
const NODE_PARENT_ALLOW_WORKER_ENV: &str = "AGENTOS_PARENT_NODE_ALLOW_WORKER";
const NODE_EXTRA_FS_READ_PATHS_ENV: &str = "AGENTOS_EXTRA_FS_READ_PATHS";
const NODE_EXTRA_FS_WRITE_PATHS_ENV: &str = "AGENTOS_EXTRA_FS_WRITE_PATHS";
const NODE_ALLOWED_BUILTINS_ENV: &str = "AGENTOS_ALLOWED_NODE_BUILTINS";
const NODE_LOOPBACK_EXEMPT_PORTS_ENV: &str = "AGENTOS_LOOPBACK_EXEMPT_PORTS";
const NODE_SYNC_RPC_ENABLE_ENV: &str = "AGENTOS_NODE_SYNC_RPC_ENABLE";
const NODE_SYNC_RPC_REQUEST_FD_ENV: &str = "AGENTOS_NODE_SYNC_RPC_REQUEST_FD";
const NODE_SYNC_RPC_RESPONSE_FD_ENV: &str = "AGENTOS_NODE_SYNC_RPC_RESPONSE_FD";
const NODE_SYNC_RPC_DATA_BYTES_ENV: &str = "AGENTOS_NODE_SYNC_RPC_DATA_BYTES";
const NODE_SYNC_RPC_WAIT_TIMEOUT_MS_ENV: &str = "AGENTOS_NODE_SYNC_RPC_WAIT_TIMEOUT_MS";
static NEXT_V8_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static JAVASCRIPT_TIMER_WHEEL: OnceLock<Arc<TimerWheel>> = OnceLock::new();

#[derive(Default)]
struct JsStartPhaseStats {
    calls: u64,
    total_ns: u128,
    max_ns: u128,
}

static JS_START_PHASES: OnceLock<Mutex<BTreeMap<String, JsStartPhaseStats>>> = OnceLock::new();
static JS_EVENT_PHASES: OnceLock<Mutex<BTreeMap<String, JsStartPhaseStats>>> = OnceLock::new();

fn js_start_phases_enabled() -> bool {
    std::env::var("AGENTOS_JS_START_PHASES").as_deref() == Ok("1")
}

fn js_event_phases_enabled() -> bool {
    std::env::var("AGENTOS_JS_EVENT_PHASES").as_deref() == Ok("1")
}

fn record_js_start_phase(stage: &str, elapsed: Duration) {
    if !js_start_phases_enabled() {
        return;
    }
    record_js_phase_stats(
        &JS_START_PHASES,
        "AGENTOS_JS_START_PHASES_FILE",
        stage,
        elapsed,
    );
}

fn record_js_event_phase(stage: &str, elapsed: Duration) {
    if !js_event_phases_enabled() {
        return;
    }
    record_js_phase_stats(
        &JS_EVENT_PHASES,
        "AGENTOS_JS_EVENT_PHASES_FILE",
        stage,
        elapsed,
    );
}

fn record_js_phase_stats(
    phases: &OnceLock<Mutex<BTreeMap<String, JsStartPhaseStats>>>,
    path_env: &str,
    stage: &str,
    elapsed: Duration,
) {
    let phases = phases.get_or_init(|| Mutex::new(BTreeMap::new()));
    let Ok(mut phases) = phases.lock() else {
        return;
    };
    let stats = phases.entry(stage.to_string()).or_default();
    stats.calls += 1;
    let elapsed_ns = elapsed.as_nanos();
    stats.total_ns += elapsed_ns;
    stats.max_ns = stats.max_ns.max(elapsed_ns);

    let Some(path) = std::env::var_os(path_env) else {
        return;
    };
    let mut output = String::new();
    for (stage, stats) in phases.iter() {
        let total_us = stats.total_ns / 1_000;
        let avg_us = if stats.calls == 0 {
            0
        } else {
            total_us / u128::from(stats.calls)
        };
        let max_us = stats.max_ns / 1_000;
        output.push_str(&format!(
            "stage={stage} calls={} total_us={total_us} avg_us={avg_us} max_us={max_us}\n",
            stats.calls
        ));
    }
    let _ = fs::write(path, output);
}

const DEFAULT_V8_CPU_TIME_LIMIT_MS: u32 = 30_000;
const DEFAULT_V8_WALL_CLOCK_LIMIT_MS: u32 = 0;
const DEFAULT_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS: u64 = 30_000;
const NODE_SYNC_RPC_DEFAULT_DATA_BYTES: usize = 4 * 1024 * 1024;
const NODE_SYNC_RPC_DEFAULT_WAIT_TIMEOUT_MS: u64 = 30_000;
const NODE_SYNC_RPC_RESPONSE_QUEUE_CAPACITY: usize = 1;
const FORWARD_KERNEL_STDIN_RPC_ENV: &str = "AGENTOS_FORWARD_KERNEL_STDIN_RPC";
// Defense-in-depth headroom: a transient burst of guest events (e.g. a chatty
// tool/skill turn) should be absorbed by the buffer, so the producer only ever
// hits backpressure under a genuinely stuck consumer rather than on every spike.
const JAVASCRIPT_EVENT_CHANNEL_CAPACITY: usize = 512;
const JAVASCRIPT_EVENT_PAYLOAD_LIMIT_BYTES: usize = 1024 * 1024;
const JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const KERNEL_STDIN_BUFFER_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const NODE_WARMUP_MARKER_VERSION: &str = "1";
const NODE_WARMUP_SPECIFIERS: &[&str] = &[
    "secure-exec:builtin/path",
    "secure-exec:builtin/url",
    "secure-exec:builtin/fs-promises",
    "secure-exec:polyfill/path",
];

#[derive(Debug, Default, Clone)]
struct SyncBridgePhaseStats {
    calls: u64,
    total_us: u64,
    max_us: u64,
}

static SYNC_BRIDGE_PHASES: OnceLock<Mutex<BTreeMap<String, SyncBridgePhaseStats>>> =
    OnceLock::new();
static SYNC_BRIDGE_REQUEST_ENQUEUED: OnceLock<Mutex<HashMap<u64, (String, Instant)>>> =
    OnceLock::new();

fn sync_bridge_phases_enabled() -> bool {
    std::env::var("AGENTOS_SYNC_BRIDGE_PHASES").as_deref() == Ok("1")
}

fn record_sync_bridge_phase(method: &str, stage: &str, elapsed: Duration) {
    if !sync_bridge_phases_enabled() {
        return;
    }
    let stats = SYNC_BRIDGE_PHASES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let Ok(mut stats) = stats.lock() else {
        return;
    };
    let elapsed_us = elapsed.as_micros() as u64;
    let key = format!("{method}:{stage}");
    let entry = stats.entry(key).or_default();
    entry.calls += 1;
    entry.total_us = entry.total_us.wrapping_add(elapsed_us);
    entry.max_us = entry.max_us.max(elapsed_us);

    if let Ok(path) = std::env::var("AGENTOS_SYNC_BRIDGE_PHASES_FILE") {
        let mut lines = String::new();
        for (key, value) in stats.iter() {
            let Some((method, stage)) = key.split_once(':') else {
                continue;
            };
            let avg_us = value.total_us.checked_div(value.calls).unwrap_or(0);
            lines.push_str(&format!(
                "method={method} stage={stage} calls={} total_us={} avg_us={} max_us={}\n",
                value.calls, value.total_us, avg_us, value.max_us
            ));
        }
        let _ = fs::write(path, lines);
    }
}

pub fn record_sync_bridge_request_enqueued(call_id: u64, method: &str) {
    if !sync_bridge_phases_enabled() {
        return;
    }
    let requests = SYNC_BRIDGE_REQUEST_ENQUEUED.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut requests) = requests.lock() else {
        return;
    };
    if requests.len() > 4096 {
        requests.clear();
    }
    requests.insert(call_id, (method.to_owned(), Instant::now()));
}

pub fn record_sync_bridge_request_observed(call_id: u64, fallback_method: &str) {
    if !sync_bridge_phases_enabled() {
        return;
    }
    let Some(requests) = SYNC_BRIDGE_REQUEST_ENQUEUED.get() else {
        return;
    };
    let Ok(mut requests) = requests.lock() else {
        return;
    };
    let Some((method, started)) = requests.remove(&call_id) else {
        return;
    };
    let method = if method.is_empty() {
        fallback_method
    } else {
        method.as_str()
    };
    record_sync_bridge_phase(method, "request_service_observed", started.elapsed());
}
const CONTROLLED_STDERR_PREFIXES: &[&str] =
    &[crate::node_import_cache::NODE_IMPORT_CACHE_METRICS_PREFIX];
const RESERVED_NODE_ENV_KEYS: &[&str] = &[
    NODE_BOOTSTRAP_ENV,
    NODE_COMPILE_CACHE_ENV,
    NODE_DISABLE_COMPILE_CACHE_ENV,
    NODE_ENTRYPOINT_ENV,
    NODE_EXTRA_FS_READ_PATHS_ENV,
    NODE_EXTRA_FS_WRITE_PATHS_ENV,
    NODE_SANDBOX_ROOT_ENV,
    NODE_FROZEN_TIME_ENV,
    NODE_GUEST_ENTRYPOINT_ENV,
    NODE_GUEST_ENTRYPOINT_MODULE_MODE_ENV,
    NODE_GUEST_ARGV_ENV,
    NODE_GUEST_PATH_MAPPINGS_ENV,
    NODE_VIRTUAL_PROCESS_EXEC_PATH_ENV,
    NODE_VIRTUAL_PROCESS_PID_ENV,
    NODE_VIRTUAL_PROCESS_PPID_ENV,
    NODE_VIRTUAL_PROCESS_UID_ENV,
    NODE_VIRTUAL_PROCESS_GID_ENV,
    NODE_PARENT_ALLOW_CHILD_PROCESS_ENV,
    NODE_PARENT_ALLOW_WORKER_ENV,
    NODE_IMPORT_CACHE_ASSET_ROOT_ENV,
    NODE_IMPORT_CACHE_LOADER_PATH_ENV,
    NODE_IMPORT_CACHE_PATH_ENV,
    NODE_KEEP_STDIN_OPEN_ENV,
    NODE_ALLOWED_BUILTINS_ENV,
    NODE_LOOPBACK_EXEMPT_PORTS_ENV,
    NODE_SYNC_RPC_ENABLE_ENV,
    NODE_SYNC_RPC_REQUEST_FD_ENV,
    NODE_SYNC_RPC_RESPONSE_FD_ENV,
    NODE_SYNC_RPC_DATA_BYTES_ENV,
    NODE_SYNC_RPC_WAIT_TIMEOUT_MS_ENV,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NodeControlMessage {
    NodeImportCacheMetrics {
        metrics: serde_json::Value,
    },
    PythonExit {
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    SignalState {
        signal: u32,
        registration: NodeSignalHandlerRegistration,
    },
}

#[derive(Debug, Default)]
struct LinePrefixFilter {
    pending: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavascriptSyncRpcRequest {
    pub id: u64,
    pub method: String,
    pub args: Vec<Value>,
    pub raw_bytes_args: HashMap<usize, Vec<u8>>,
}

#[derive(Debug, Deserialize)]
struct JavascriptSyncRpcRequestWire {
    id: u64,
    method: String,
    #[serde(default)]
    args: Vec<Value>,
}

struct JavascriptSyncRpcChannels {
    parent_request_reader: File,
    parent_response_writer: File,
    child_request_writer: OwnedFd,
    child_response_reader: OwnedFd,
}

impl LinePrefixFilter {
    fn filter_chunk(&mut self, chunk: &[u8], prefixes: &[&str]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut filtered = Vec::new();

        while let Some(newline_index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = self.pending.drain(..=newline_index).collect::<Vec<_>>();
            if !has_control_prefix(&line, prefixes) {
                filtered.extend_from_slice(&line);
            }
        }

        filtered
    }
}

fn has_control_prefix(line: &[u8], prefixes: &[&str]) -> bool {
    let text = String::from_utf8_lossy(line);
    let trimmed = text.trim_end_matches(['\r', '\n']);
    prefixes.iter().any(|prefix| trimmed.starts_with(prefix))
}

#[derive(Debug)]
struct JavascriptSyncRpcResponseWriter {
    sender: SyncSender<Vec<u8>>,
    timeout: Duration,
}

impl JavascriptSyncRpcResponseWriter {
    fn new(writer: File, timeout: Duration) -> Self {
        let (sender, receiver) = mpsc::sync_channel(NODE_SYNC_RPC_RESPONSE_QUEUE_CAPACITY);
        spawn_javascript_sync_rpc_response_writer(writer, receiver);
        Self { sender, timeout }
    }

    fn send(&self, payload: Vec<u8>) -> Result<(), JavascriptExecutionError> {
        let started = Instant::now();
        let mut payload = Some(payload);

        loop {
            match self
                .sender
                .try_send(payload.take().expect("payload should be present"))
            {
                Ok(()) => return Ok(()),
                Err(TrySendError::Disconnected(_)) => {
                    return Err(JavascriptExecutionError::RpcResponse(String::from(
                        "JavaScript sync RPC response channel closed unexpectedly",
                    )));
                }
                Err(TrySendError::Full(returned_payload)) => {
                    if started.elapsed() >= self.timeout {
                        return Err(JavascriptExecutionError::RpcResponse(format!(
                            "timed out after {}ms while queueing JavaScript sync RPC response",
                            self.timeout.as_millis()
                        )));
                    }
                    payload = Some(returned_payload);
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

impl Clone for JavascriptSyncRpcResponseWriter {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            timeout: self.timeout,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingSyncRpcState {
    Pending(u64),
    TimedOut(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingSyncRpcResolution {
    Pending,
    TimedOut,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateJavascriptContextRequest {
    pub vm_id: String,
    pub bootstrap_module: Option<String>,
    pub compile_cache_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavascriptContext {
    pub context_id: String,
    pub vm_id: String,
    pub bootstrap_module: Option<String>,
    pub compile_cache_dir: Option<PathBuf>,
}

/// Per-execution JavaScript runtime limits, carried as typed fields on the
/// execution request rather than `AGENTOS_*` env vars. The sidecar populates
/// these from the per-VM `VmLimits` (which originate from `CreateVmConfig` on
/// the BARE wire); `None` selects the engine default. See the env-vs-wire rule
/// in `crates/sidecar/CLAUDE.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JavascriptExecutionLimits {
    /// V8 heap cap in MB. `None`/`Some(0)` keeps the engine default heap.
    pub v8_heap_limit_mb: Option<u32>,
    /// Sync-RPC blocking-wait ceiling in ms. `None` keeps the engine default.
    pub sync_rpc_wait_timeout_ms: Option<u64>,
    /// Active JavaScript CPU-time budget in ms. `None` keeps the engine default;
    /// `Some(0)` disables the CPU watchdog.
    pub cpu_time_limit_ms: Option<u32>,
    /// JavaScript wall-clock backstop in ms. `None` keeps the engine default;
    /// `Some(0)` disables the wall-clock watchdog.
    pub wall_clock_limit_ms: Option<u32>,
    /// Timeout for materializing the per-VM Node import cache.
    pub import_cache_materialize_timeout_ms: Option<u64>,
}

/// Per-execution guest-runtime config carried as typed fields rather than
/// `AGENTOS_*` env vars. The sidecar populates these from kernel state
/// (`user_profile()`, `resource_limits()`) and `CreateVmConfig`; the runtime
/// shim interpolates them into a `_processConfig` object the guest reads, so the
/// guest's virtual identity no longer rides the ambient env channel. `None`
/// keeps the guest-runtime default. See the env-vs-wire rule in
/// `crates/sidecar/CLAUDE.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuestRuntimeConfig {
    /// Virtual `process.pid`.
    pub virtual_pid: Option<u64>,
    /// Virtual `process.ppid`.
    pub virtual_ppid: Option<u64>,
    /// Virtual `process.uid` / `process.euid`.
    pub virtual_uid: Option<u64>,
    /// Virtual `process.gid` / `process.egid` / `process.groups`.
    pub virtual_gid: Option<u64>,
    /// Virtual `process.execPath`.
    pub virtual_exec_path: Option<String>,
    /// `os.cpus().length`.
    pub os_cpu_count: Option<u64>,
    /// `os.totalmem()` in bytes.
    pub os_totalmem: Option<u64>,
    /// `os.freemem()` in bytes.
    pub os_freemem: Option<u64>,
    /// `os.homedir()`.
    pub os_homedir: Option<String>,
    /// `os.hostname()`.
    pub os_hostname: Option<String>,
    /// `os.tmpdir()`.
    pub os_tmpdir: Option<String>,
    /// `os.type()`.
    pub os_type: Option<String>,
    /// `os.release()`.
    pub os_release: Option<String>,
    /// `os.version()`.
    pub os_version: Option<String>,
    /// `os.machine()`.
    pub os_machine: Option<String>,
    /// Default login shell.
    pub os_shell: Option<String>,
    /// `os.userInfo().username`.
    pub os_user: Option<String>,
    /// Opt-in high-resolution monotonic guest clock. Default false preserves
    /// the security-oriented coarse clock.
    pub high_resolution_time: bool,
    /// Optional agent-SDK bundle (esbuild IIFE) to evaluate into the per-sidecar
    /// V8 snapshot alongside the bridge, so the SDK is loaded once per sidecar and
    /// reused across sessions instead of re-imported on every execution. `None`
    /// keeps the bridge-only snapshot (unchanged behavior). The runtime caches the
    /// snapshot process-wide keyed by sha256(bridge_code + this bundle).
    pub snapshot_userland_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartJavascriptExecutionRequest {
    pub vm_id: String,
    pub context_id: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    /// Per-execution runtime limits (see [`JavascriptExecutionLimits`]).
    pub limits: JavascriptExecutionLimits,
    /// Per-execution guest-runtime config (see [`GuestRuntimeConfig`]).
    pub guest_runtime: GuestRuntimeConfig,
    /// Optional inline JavaScript code supplied by the sidecar.
    /// Eval entrypoints always execute this source directly. Module-mode file
    /// entrypoints may also use it so the isolate can evaluate the original
    /// source without re-reading through the host. CommonJS file entrypoints
    /// still go through the normal require() wrapper so Node-style globals such
    /// as __filename and __dirname are initialized correctly.
    pub inline_code: Option<String>,
    /// Optional raw WASM module bytes to expose to the runner isolate for this
    /// execution.
    pub wasm_module_bytes: Option<Arc<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavascriptExecutionEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    SyncRpcRequest(JavascriptSyncRpcRequest),
    SignalState {
        signal: u32,
        registration: NodeSignalHandlerRegistration,
    },
    Exited(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JavascriptProcessEvent {
    Stdout(Vec<u8>),
    RawStderr(Vec<u8>),
    SyncRpcRequest(JavascriptSyncRpcRequest),
    Control(NodeControlMessage),
    Exited(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavascriptExecutionResult {
    pub execution_id: String,
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GuestPathMapping {
    guest_path: String,
    host_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GuestPathMappingWire {
    #[serde(rename = "guestPath")]
    guest_path: String,
    #[serde(rename = "hostPath")]
    host_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleResolveMode {
    Require,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalResolvedModuleFormat {
    Module,
    Commonjs,
    Json,
}

impl LocalResolvedModuleFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Commonjs => "commonjs",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LocalModuleResolutionCache {
    resolve_results: HashMap<(String, String, ModuleResolveMode), Option<String>>,
    module_format_results: HashMap<String, Option<LocalResolvedModuleFormat>>,
    package_json_results: HashMap<String, Option<LocalPackageJson>>,
    exists_results: HashMap<String, bool>,
    stat_results: HashMap<String, Option<bool>>,
}

/// Read-only filesystem primitives the module resolver needs. The resolution
/// algorithm itself is pure path algebra over these four operations; pointing
/// it at a different backing store (host files vs. the kernel VFS) is purely a
/// matter of supplying a different `ModuleFsReader`.
///
/// All paths are guest paths (e.g. `/root/node_modules/foo/index.js`). Symlink
/// following is the reader's responsibility: `canonical_guest_path` must return
/// the fully-resolved guest path (realpath), and `path_is_dir`/`path_exists`
/// must follow symlinks the way real Node's `fs.stat`/`fs.existsSync` do.
pub trait ModuleFsReader {
    /// Realpath of `guest_path`, expressed as a guest path. `None` if the path
    /// does not resolve (does not exist / escapes the addressable tree).
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String>;

    /// Read the file at `guest_path` as a UTF-8 string, following symlinks.
    fn read_to_string(&mut self, guest_path: &str) -> Option<String>;

    /// `Some(true)` if `guest_path` is a directory, `Some(false)` if it exists
    /// but is not a directory, `None` if it does not exist. Follows symlinks.
    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool>;

    /// Whether `guest_path` exists, following symlinks.
    fn path_exists(&mut self, guest_path: &str) -> bool;
}

/// Guest JavaScript module-resolution mode (the `moduleResolution` axis of
/// `jsRuntime`). Defaults to full Node.js resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GuestModuleResolution {
    /// node_modules ancestor-walk + exports/conditions + realpath.
    #[default]
    Node,
    /// Relative/absolute ESM only; bare specifiers do not resolve.
    Relative,
    /// No resolution at all; every specifier (relative included) is denied.
    None,
}

impl GuestModuleResolution {
    fn from_env(env: &BTreeMap<String, String>) -> Self {
        match env.get("AGENTOS_JS_MODULE_RESOLUTION").map(String::as_str) {
            Some("relative") => Self::Relative,
            Some("none") => Self::None,
            _ => Self::Node,
        }
    }
}

#[derive(Default)]
struct LocalBridgeState {
    translator: GuestPathTranslator,
    resolution_cache: LocalModuleResolutionCache,
    /// jsRuntime module-resolution mode for this execution.
    module_resolution: GuestModuleResolution,
    handle_descriptions: HashMap<String, String>,
    next_timer_id: u64,
    timers: Arc<Mutex<HashMap<u64, LocalTimerEntry>>>,
    kernel_stdin: Arc<LocalKernelStdinBridge>,
    forward_kernel_stdin_rpc: bool,
    v8_session: Option<V8SessionHandle>,
    /// Optional read-only reader over the mounted `node_modules` VFS, supplied by
    /// the sidecar. When present, the bridge thread resolves module-resolution
    /// RPCs (`_resolveModule` / `_loadFile` / `_moduleFormat` /
    /// `_batchResolveModules`) inline against this reader, concurrently with the
    /// service loop — so a large cold-start module graph does not serialize
    /// behind / starve the ACP bootstrap on the single service-loop thread.
    /// `None` means "route module resolution to the service loop" (the kernel-VFS
    /// fallback for callers that supply no reader).
    module_reader: Option<Box<dyn ModuleFsReader + Send>>,
}

impl Drop for LocalBridgeState {
    /// Tear down all tracked timers when the bridge state is dropped (which
    /// happens when the event-bridge service loop exits on session termination —
    /// success, error, or shutdown). Clearing the shared `timers` map cancels both
    /// kernel and bridge timers: any in-flight wheel action that wakes afterwards
    /// finds its entry gone and suppresses its callback via `timer_should_fire`,
    /// so a destroyed session's timers do not fire after the fact.
    fn drop(&mut self) {
        if let Ok(mut timers) = self.timers.lock() {
            timers.clear();
        }
    }
}

#[derive(Debug, Default)]
struct LocalKernelStdinBridge {
    state: Mutex<LocalKernelStdinState>,
    ready: Condvar,
}

#[derive(Debug, Default)]
struct LocalKernelStdinState {
    bytes: VecDeque<u8>,
    closed: bool,
}

#[derive(Debug, Clone, Default)]
struct GuestPathTranslator {
    implicit_guest_cwd: String,
    implicit_host_cwd: PathBuf,
    sandbox_root: Option<PathBuf>,
    mappings: Vec<GuestPathMapping>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LocalPackageJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    package_type: Option<String>,
    #[serde(default)]
    exports: Option<Value>,
    #[serde(default)]
    imports: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalTimerEntry {
    delay_ms: u64,
    generation: u64,
    repeat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PolyfillSourceKind {
    NodeStdlibBrowser,
    CustomBridge,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolyfillRegistryGroup {
    source: PolyfillSourceKind,
    #[serde(default)]
    error_code: Option<String>,
    names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolyfillRegistry {
    version: u32,
    groups: Vec<PolyfillRegistryGroup>,
}

static POLYFILL_REGISTRY: OnceLock<PolyfillRegistry> = OnceLock::new();

fn polyfill_registry() -> &'static PolyfillRegistry {
    POLYFILL_REGISTRY.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/polyfill-registry.json"))
            .expect("polyfill-registry.json must be valid")
    })
}

#[derive(Debug, Clone, PartialEq)]
enum LocalBridgeCallResult {
    Immediate(Value),
    Deferred,
}

/// Upper bound on guest-supplied timer delays, matching the JS `TIMEOUT_MAX`
/// ceiling (`2**31 - 1` ms, ~24.8 days). Guest code can pass a delay up to
/// `u64::MAX` ms; clamping keeps the timer wheel's deadline math and session
/// handle lifetime within Node-compatible bounds.
const MAX_TIMER_DELAY_MS: u64 = 2_147_483_647;

fn timer_delay_ms(value: Option<&Value>) -> u64 {
    let delay = match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    };

    if !delay.is_finite() || delay <= 0.0 {
        0
    } else {
        delay.floor().min(MAX_TIMER_DELAY_MS as f64) as u64
    }
}

/// Decide whether a woken timer action should fire, and reclaim its tracking
/// entry. Returns `false` (suppressing the callback) when the timer is gone from
/// the map (cleared, or wiped on session teardown) or its generation no longer
/// matches the one captured at scheduling time (re-armed/cancelled). A one-shot
/// (`repeat == false`) timer that does fire is removed from the map so its id is
/// reclaimed. Shared by the kernel-timer and bridge-timer paths so both honor the
/// same cancellation semantics.
fn timer_should_fire(
    timers: &Arc<Mutex<HashMap<u64, LocalTimerEntry>>>,
    timer_id: u64,
    generation: u64,
) -> bool {
    timers
        .lock()
        .ok()
        .and_then(|mut timers| {
            let (current_generation, repeat) = timers
                .get(&timer_id)
                .map(|entry| (entry.generation, entry.repeat))?;
            if current_generation != generation {
                return Some(false);
            }
            if !repeat {
                timers.remove(&timer_id);
            }
            Some(true)
        })
        .unwrap_or(false)
}

struct TimerWheel {
    state: Mutex<TimerWheelState>,
    ready: Condvar,
}

#[derive(Default)]
struct TimerWheelState {
    heap: BinaryHeap<Reverse<(Instant, u64)>>,
    entries: HashMap<u64, TimerAction>,
    next_seq: u64,
}

enum TimerAction {
    StreamEvent {
        session: V8SessionHandle,
        timer_id: u64,
        generation: u64,
        timers: Arc<Mutex<HashMap<u64, LocalTimerEntry>>>,
    },
    BridgeResponse {
        session: V8SessionHandle,
        call_id: u64,
        timer_id: u64,
        generation: u64,
        timers: Arc<Mutex<HashMap<u64, LocalTimerEntry>>>,
    },
}

impl TimerAction {
    fn execute(self) {
        match self {
            Self::StreamEvent {
                session,
                timer_id,
                generation,
                timers,
            } => {
                if !timer_should_fire(&timers, timer_id, generation) {
                    return;
                }

                let payload =
                    v8_runtime::json_to_cbor_payload(&json!(timer_id)).unwrap_or_default();
                let _ = session.send_stream_event("timer", payload);
            }
            Self::BridgeResponse {
                session,
                call_id,
                timer_id,
                generation,
                timers,
            } => {
                if !timer_should_fire(&timers, timer_id, generation) {
                    return;
                }
                let _ = session.send_bridge_response(call_id, 0, Vec::new());
            }
        }
    }
}

impl TimerWheel {
    fn get() -> &'static Arc<Self> {
        JAVASCRIPT_TIMER_WHEEL.get_or_init(Self::start)
    }

    fn start() -> Arc<Self> {
        let wheel = Arc::new(Self {
            state: Mutex::new(TimerWheelState::default()),
            ready: Condvar::new(),
        });
        let worker = Arc::clone(&wheel);
        // Detached daemon: queued entries carry generation-checked session handles,
        // matching the previous fire-and-forget timer threads without one OS
        // thread per guest timer.
        if let Err(error) = thread::Builder::new()
            .name(String::from("secure-exec-js-timer-wheel"))
            .spawn(move || worker.run())
        {
            tracing::warn!(?error, "failed to start JavaScript timer wheel thread");
        }
        wheel
    }

    fn schedule(&self, delay_ms: u64, action: TimerAction) {
        let now = Instant::now();
        let deadline = now
            .checked_add(Duration::from_millis(delay_ms))
            .unwrap_or(now);
        let mut state = self.lock_state();
        let old_earliest = state.heap.peek().map(|Reverse((deadline, _))| *deadline);
        let seq = state.next_seq;
        state.next_seq = state.next_seq.wrapping_add(1);
        state.heap.push(Reverse((deadline, seq)));
        state.entries.insert(seq, action);
        if old_earliest.is_none_or(|old| deadline < old) {
            self.ready.notify_one();
        }
    }

    fn run(&self) {
        loop {
            let due = {
                let mut state = self.lock_state();
                loop {
                    match state.heap.peek().copied() {
                        Some(Reverse((deadline, _))) => {
                            let now = Instant::now();
                            if deadline <= now {
                                break;
                            }
                            let timeout = deadline.saturating_duration_since(now);
                            state = match self.ready.wait_timeout(state, timeout) {
                                Ok((state, _)) => state,
                                Err(poisoned) => poisoned.into_inner().0,
                            };
                        }
                        None => {
                            state = match self.ready.wait(state) {
                                Ok(state) => state,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                        }
                    }
                }

                let now = Instant::now();
                let mut due = Vec::new();
                while let Some(Reverse((deadline, seq))) = state.heap.peek().copied() {
                    if deadline > now {
                        break;
                    }
                    state.heap.pop();
                    if let Some(action) = state.entries.remove(&seq) {
                        due.push(action);
                    }
                }
                due
            };

            for action in due {
                if catch_unwind(AssertUnwindSafe(|| action.execute())).is_err() {
                    tracing::warn!("JavaScript timer wheel action panicked");
                }
            }
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, TimerWheelState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl GuestPathTranslator {
    fn from_host_context(
        env: &BTreeMap<String, String>,
        host_cwd: PathBuf,
        guest_cwd: String,
    ) -> Self {
        let mut mappings = parse_guest_path_mappings_from_env(env)
            .into_iter()
            .filter(|mapping| mapping.guest_path.starts_with('/'))
            .collect::<Vec<_>>();

        if !mappings
            .iter()
            .any(|mapping| mapping.guest_path == guest_cwd && mapping.host_path == host_cwd)
        {
            mappings.push(GuestPathMapping {
                guest_path: guest_cwd.clone(),
                host_path: host_cwd.clone(),
            });
        }

        sort_guest_path_mappings(&mut mappings);

        Self {
            implicit_guest_cwd: guest_cwd,
            implicit_host_cwd: host_cwd,
            sandbox_root: env
                .get(NODE_SANDBOX_ROOT_ENV)
                .filter(|value| Path::new(value.as_str()).is_absolute())
                .map(PathBuf::from),
            mappings,
        }
    }

    fn is_known_host_path(&self, host_path: &Path) -> bool {
        if host_path.starts_with(&self.implicit_host_cwd) {
            return true;
        }

        if let Some(sandbox_root) = &self.sandbox_root {
            if host_path.starts_with(sandbox_root) {
                return true;
            }
        }

        self.mappings.iter().any(|mapping| {
            host_path.starts_with(&mapping.host_path)
                || fs::canonicalize(&mapping.host_path)
                    .map(|real_path| host_path.starts_with(real_path))
                    .unwrap_or(false)
        })
    }

    fn from_request(request: &StartJavascriptExecutionRequest) -> Self {
        let implicit_guest_cwd = request
            .env
            .get("PWD")
            .filter(|value| value.starts_with('/'))
            .cloned()
            .or_else(|| {
                request
                    .env
                    .get("HOME")
                    .filter(|value| value.starts_with('/'))
                    .cloned()
            })
            .unwrap_or_else(|| String::from("/root"));
        let mut translator = Self::from_host_context(
            &request.env,
            request.cwd.clone(),
            implicit_guest_cwd.clone(),
        );
        translator.mappings.sort_by(|left, right| {
            let left_is_implicit =
                left.guest_path == implicit_guest_cwd && left.host_path == request.cwd;
            let right_is_implicit =
                right.guest_path == implicit_guest_cwd && right.host_path == request.cwd;
            right
                .guest_path
                .len()
                .cmp(&left.guest_path.len())
                .then_with(|| right_is_implicit.cmp(&left_is_implicit))
                .then_with(|| {
                    right
                        .host_path
                        .components()
                        .count()
                        .cmp(&left.host_path.components().count())
                })
        });
        translator
    }

    fn guest_cwd(&self) -> &str {
        &self.implicit_guest_cwd
    }

    fn resolve_host_entrypoint(&self, cwd: &Path, entrypoint: &str) -> PathBuf {
        if entrypoint == "-e" || entrypoint == "--eval" {
            return PathBuf::from(entrypoint);
        }

        let path = Path::new(entrypoint);
        if path.is_absolute() {
            if self.is_known_host_path(path) {
                return path.to_path_buf();
            }
            self.guest_to_host(entrypoint)
                .unwrap_or_else(|| path.to_path_buf())
        } else {
            cwd.join(path)
        }
    }

    fn host_to_guest_string(&self, host_path: &Path) -> String {
        if !host_path.is_absolute() {
            return normalize_guest_path(&host_path.to_string_lossy());
        }

        for mapping in &self.mappings {
            if let Ok(stripped) = host_path.strip_prefix(&mapping.host_path) {
                return join_guest_path(
                    &mapping.guest_path,
                    &stripped.to_string_lossy().replace('\\', "/"),
                );
            }
            if let Ok(real_mapping_path) = fs::canonicalize(&mapping.host_path) {
                if let Ok(stripped) = host_path.strip_prefix(&real_mapping_path) {
                    return join_guest_path(
                        &mapping.guest_path,
                        &stripped.to_string_lossy().replace('\\', "/"),
                    );
                }
            }
        }

        if let Ok(stripped) = host_path.strip_prefix(&self.implicit_host_cwd) {
            return join_guest_path(
                &self.implicit_guest_cwd,
                &stripped.to_string_lossy().replace('\\', "/"),
            );
        }

        if let Some(sandbox_root) = &self.sandbox_root {
            if let Ok(stripped) = host_path.strip_prefix(sandbox_root) {
                return join_guest_path("/", &stripped.to_string_lossy().replace('\\', "/"));
            }
        }

        let basename = host_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown");
        join_guest_path("/unknown", basename)
    }

    fn guest_to_host(&self, guest_path: &str) -> Option<PathBuf> {
        let normalized = normalize_guest_path(guest_path);
        let mut fallback_candidate = None;

        for mapping in &self.mappings {
            if let Some(suffix) = strip_guest_prefix(&normalized, &mapping.guest_path) {
                let candidate = join_host_path(&mapping.host_path, suffix);
                if candidate.exists() {
                    return self.confine_host_path(candidate);
                }
                if let Ok(real_mapping_path) = fs::canonicalize(&mapping.host_path) {
                    let real_candidate = join_host_path(&real_mapping_path, suffix);
                    if real_candidate.exists() {
                        return self.confine_host_path(real_candidate);
                    }
                    if let Some(sibling_candidate) =
                        resolve_pnpm_sibling_host_path(&real_mapping_path, suffix)
                    {
                        return self.confine_host_path(sibling_candidate);
                    }
                }
                fallback_candidate.get_or_insert(candidate);
            }
        }
        if let Some(suffix) = strip_guest_prefix(&normalized, &self.implicit_guest_cwd) {
            return self.confine_host_path(join_host_path(&self.implicit_host_cwd, suffix));
        }

        if let Some(candidate) = fallback_candidate {
            return self.confine_host_path(candidate);
        }

        if let Some(sandbox_root) = &self.sandbox_root {
            return self.confine_host_path(join_host_path(
                sandbox_root,
                normalized.trim_start_matches('/'),
            ));
        }

        None
    }

    fn confine_host_path(&self, host_path: PathBuf) -> Option<PathBuf> {
        let allowed_roots = self.allowed_canonical_host_roots();
        if allowed_roots.is_empty() {
            return None;
        }

        if let Ok(canonical_path) = fs::canonicalize(&host_path) {
            return canonical_path_is_allowed(&canonical_path, &allowed_roots).then_some(host_path);
        }

        let existing_ancestor = nearest_existing_host_ancestor(&host_path)?;
        let canonical_ancestor = fs::canonicalize(existing_ancestor).ok()?;
        canonical_path_is_allowed(&canonical_ancestor, &allowed_roots).then_some(host_path)
    }

    fn allowed_canonical_host_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for root in self
            .mappings
            .iter()
            .map(|mapping| mapping.host_path.as_path())
            .chain(std::iter::once(self.implicit_host_cwd.as_path()))
            .chain(self.sandbox_root.as_deref())
        {
            if let Ok(canonical_root) = fs::canonicalize(root) {
                if !roots.iter().any(|existing| existing == &canonical_root) {
                    roots.push(canonical_root);
                }
            }
        }
        roots
    }

    fn canonical_guest_path(&self, guest_path: &str) -> Option<String> {
        let host_path = self.guest_to_host(guest_path)?;
        let canonical = fs::canonicalize(host_path).ok()?;
        for mapping in &self.mappings {
            if strip_guest_prefix(guest_path, &mapping.guest_path).is_none() {
                continue;
            }
            if let Ok(stripped) = canonical.strip_prefix(&mapping.host_path) {
                return Some(join_guest_path(
                    &mapping.guest_path,
                    &stripped.to_string_lossy().replace('\\', "/"),
                ));
            }
            if let Ok(real_mapping_path) = fs::canonicalize(&mapping.host_path) {
                if let Ok(stripped) = canonical.strip_prefix(&real_mapping_path) {
                    return Some(join_guest_path(
                        &mapping.guest_path,
                        &stripped.to_string_lossy().replace('\\', "/"),
                    ));
                }
            }
        }
        if let Some(node_modules_root) = self
            .mappings
            .iter()
            .find(|mapping| mapping.guest_path == "/root/node_modules")
        {
            if let Ok(stripped) = canonical.strip_prefix(&node_modules_root.host_path) {
                return Some(join_guest_path(
                    &node_modules_root.guest_path,
                    &stripped.to_string_lossy().replace('\\', "/"),
                ));
            }
            if let Ok(real_root) = fs::canonicalize(&node_modules_root.host_path) {
                if let Ok(stripped) = canonical.strip_prefix(&real_root) {
                    return Some(join_guest_path(
                        &node_modules_root.guest_path,
                        &stripped.to_string_lossy().replace('\\', "/"),
                    ));
                }
            }
        }
        let guest = self.host_to_guest_string(&canonical);
        (!guest.starts_with("/unknown/")).then_some(normalize_guest_path(&guest))
    }
}

fn sort_guest_path_mappings(mappings: &mut [GuestPathMapping]) {
    mappings.sort_by(|left, right| {
        right
            .guest_path
            .len()
            .cmp(&left.guest_path.len())
            .then_with(|| {
                right
                    .host_path
                    .components()
                    .count()
                    .cmp(&left.host_path.components().count())
            })
    });
}

fn canonical_path_is_allowed(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    allowed_roots
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn nearest_existing_host_ancestor(path: &Path) -> Option<&Path> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        if fs::symlink_metadata(current).is_ok() {
            return Some(current);
        }
        candidate = current.parent();
    }
    None
}

#[doc(hidden)]
pub struct ModuleResolutionTestHarness {
    local_bridge: LocalBridgeState,
}

impl ModuleResolutionTestHarness {
    pub fn new(host_root: impl Into<PathBuf>) -> Self {
        let host_root = host_root.into();
        let mut mappings = vec![
            GuestPathMapping {
                guest_path: String::from("/root/node_modules"),
                host_path: host_root.join("node_modules"),
            },
            GuestPathMapping {
                guest_path: String::from("/root"),
                host_path: host_root.clone(),
            },
        ];
        sort_guest_path_mappings(&mut mappings);

        // Build via default + in-place assignment rather than `..default()`:
        // LocalBridgeState implements Drop (to cancel timers on session teardown),
        // and functional-record-update would move fields out of a Drop type (E0509).
        let mut local_bridge = LocalBridgeState::default();
        local_bridge.translator = GuestPathTranslator {
            implicit_guest_cwd: String::from("/root"),
            implicit_host_cwd: host_root,
            sandbox_root: None,
            mappings,
        };
        Self { local_bridge }
    }

    pub fn resolve_import(&mut self, specifier: &str, from_path: &str) -> Option<String> {
        self.local_bridge
            .resolve_module(specifier, from_path, ModuleResolveMode::Import)
    }

    pub fn resolve_require(&mut self, specifier: &str, from_path: &str) -> Option<String> {
        self.local_bridge
            .resolve_module(specifier, from_path, ModuleResolveMode::Require)
    }

    pub fn module_format(&mut self, path: &str) -> Option<&'static str> {
        self.local_bridge
            .module_format(path)
            .map(LocalResolvedModuleFormat::as_str)
    }
}

#[doc(hidden)]
pub fn handle_internal_bridge_call_from_host_context(
    host_cwd: &Path,
    guest_cwd: &str,
    env: &BTreeMap<String, String>,
    method: &str,
    args: &[Value],
) -> Option<Value> {
    // default + in-place assign: LocalBridgeState is Drop, so `..default()` (E0509)
    // is not allowed.
    let mut local_bridge = LocalBridgeState::default();
    local_bridge.translator =
        GuestPathTranslator::from_host_context(env, host_cwd.to_path_buf(), guest_cwd.to_owned());

    match local_bridge.handle_internal_bridge_call(0, method, args) {
        Some(LocalBridgeCallResult::Immediate(value)) => Some(value),
        _ => None,
    }
}

fn resolve_pnpm_sibling_host_path(real_mapping_path: &Path, suffix: &str) -> Option<PathBuf> {
    let trimmed = suffix.strip_prefix("node_modules/")?;
    let mut current = Some(real_mapping_path);
    while let Some(path) = current {
        if path.file_name().and_then(|name| name.to_str()) == Some("node_modules") {
            let candidate = join_host_path(path, trimmed);
            if candidate.exists() {
                return Some(candidate);
            }
            break;
        }
        current = path.parent();
    }
    None
}

fn parse_guest_path_mappings(request: &StartJavascriptExecutionRequest) -> Vec<GuestPathMapping> {
    parse_guest_path_mappings_from_env(&request.env)
}

fn parse_guest_path_mappings_from_env(env: &BTreeMap<String, String>) -> Vec<GuestPathMapping> {
    env.get(NODE_GUEST_PATH_MAPPINGS_ENV)
        .and_then(|value| serde_json::from_str::<Vec<GuestPathMappingWire>>(value).ok())
        .into_iter()
        .flatten()
        .map(|mapping| GuestPathMapping {
            guest_path: normalize_guest_path(&mapping.guest_path),
            host_path: PathBuf::from(mapping.host_path),
        })
        .collect()
}

fn normalize_guest_path(path: &str) -> String {
    let mut segments = Vec::new();
    let absolute = path.starts_with('/');
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if !absolute {
        return segments.join("/");
    }
    if segments.is_empty() {
        String::from("/")
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn join_guest_path(base: &str, suffix: &str) -> String {
    if suffix.is_empty() || suffix == "." {
        return normalize_guest_path(base);
    }
    let trimmed = suffix.trim_start_matches('/');
    normalize_guest_path(&format!("{}/{}", base.trim_end_matches('/'), trimmed))
}

fn strip_guest_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix == "/" {
        return path.strip_prefix('/');
    }
    if path == prefix {
        return Some("");
    }
    path.strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix('/'))
}

fn join_host_path(base: &Path, suffix: &str) -> PathBuf {
    if suffix.is_empty() {
        return base.to_path_buf();
    }
    let mut joined = base.to_path_buf();
    for segment in suffix.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            joined.pop();
        } else {
            joined.push(segment);
        }
    }
    joined
}

fn translate_v8_bridge_value_to_legacy(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(translate_v8_bridge_value_to_legacy)
                .collect(),
        ),
        Value::Object(map) if map.get("__type").and_then(Value::as_str) == Some("Buffer") => {
            json!({
                "__agentOSType": "bytes",
                "base64": map.get("data").cloned().unwrap_or(Value::String(String::new())),
            })
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), translate_v8_bridge_value_to_legacy(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn translate_request_args_for_legacy(method: &str, args: &[Value]) -> Vec<Value> {
    let mut translated = args
        .iter()
        .map(translate_v8_bridge_value_to_legacy)
        .collect::<Vec<_>>();

    if matches!(method, "fs.writeFileSync" | "fs.promises.writeFile") {
        if let Some(Value::String(data)) = translated.get(1) {
            translated[1] = json!({
                "__agentOSType": "bytes",
                "base64": v8_runtime::base64_encode_pub(data.as_bytes()),
            });
        }
    }

    translated
}

fn translate_legacy_bridge_value_to_v8(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(translate_legacy_bridge_value_to_v8)
                .collect(),
        ),
        Value::Object(map) if map.get("__agentOSType").and_then(Value::as_str) == Some("bytes") => {
            json!({
                "__type": "Buffer",
                "data": map.get("base64").cloned().unwrap_or(Value::String(String::new())),
            })
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), translate_legacy_bridge_value_to_v8(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn decode_bridge_output_arg(value: &Value) -> Vec<u8> {
    match value {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(map)
            if map.get("__type").and_then(Value::as_str) == Some("Buffer")
                || map.get("__agentOSType").and_then(Value::as_str) == Some("bytes") =>
        {
            let base64_value = map
                .get("data")
                .or_else(|| map.get("base64"))
                .and_then(Value::as_str);
            if let Some(base64_value) = base64_value {
                if let Some(bytes) = v8_runtime::base64_decode_pub(base64_value) {
                    return bytes;
                }
            }
            value.to_string().into_bytes()
        }
        other => other.to_string().into_bytes(),
    }
}

fn decode_bridge_output_args(args: &[Value]) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            output.push(b' ');
        }
        output.extend(decode_bridge_output_arg(arg));
    }
    output
}

#[derive(Debug)]
pub enum JavascriptExecutionError {
    EmptyArgv,
    MissingContext(String),
    VmMismatch { expected: String, found: String },
    PrepareImportCache(std::io::Error),
    Spawn(std::io::Error),
    PendingSyncRpcRequest(u64),
    ExpiredSyncRpcRequest(u64),
    RpcResponse(String),
    Terminate(std::io::Error),
    StdinClosed,
    Stdin(std::io::Error),
    OutputBufferExceeded { stream: &'static str, limit: usize },
    EventChannelClosed,
}

impl fmt::Display for JavascriptExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArgv => f.write_str("guest JavaScript execution requires argv[0]"),
            Self::MissingContext(context_id) => {
                write!(f, "unknown guest JavaScript context: {context_id}")
            }
            Self::VmMismatch { expected, found } => {
                write!(
                    f,
                    "guest JavaScript context belongs to vm {expected}, not {found}"
                )
            }
            Self::PrepareImportCache(err) => {
                write!(
                    f,
                    "failed to prepare sidecar-scoped Node import cache: {err}"
                )
            }
            Self::Spawn(err) => write!(f, "failed to start guest JavaScript runtime: {err}"),
            Self::PendingSyncRpcRequest(id) => {
                write!(
                    f,
                    "guest JavaScript execution requires servicing pending sync RPC request {id}"
                )
            }
            Self::ExpiredSyncRpcRequest(id) => {
                write!(f, "sync RPC request {id} is no longer pending")
            }
            Self::RpcResponse(message) => {
                write!(
                    f,
                    "failed to reply to guest JavaScript sync RPC request: {message}"
                )
            }
            Self::Terminate(err) => {
                write!(f, "failed to terminate guest JavaScript runtime: {err}")
            }
            Self::StdinClosed => f.write_str("guest JavaScript stdin is already closed"),
            Self::Stdin(err) => write!(f, "failed to write guest stdin: {err}"),
            Self::OutputBufferExceeded { stream, limit } => {
                write!(
                    f,
                    "guest JavaScript {stream} exceeded the captured output limit of {limit} bytes"
                )
            }
            Self::EventChannelClosed => {
                f.write_str("guest JavaScript event channel closed unexpectedly")
            }
        }
    }
}

impl std::error::Error for JavascriptExecutionError {}

#[derive(Debug)]
pub struct JavascriptExecution {
    execution_id: String,
    child_pid: u32,
    events: tokio::sync::Mutex<TokioReceiver<JavascriptExecutionEvent>>,
    pending_sync_rpc: Arc<Mutex<Option<PendingSyncRpcState>>>,
    kernel_stdin: Arc<LocalKernelStdinBridge>,
    _import_cache_guard: Arc<NodeImportCacheCleanup>,
    v8_session: V8SessionHandle,
    /// Host-direct module resolver state, used ONLY by the standalone `wait()`
    /// loop. The real VM runtime resolves modules against the kernel VFS on the
    /// sidecar service loop and never reaches this; but `wait()` runs without a
    /// kernel (dev/test harness), so it services module-resolution sync RPCs
    /// host-directly from the request's path translator.
    module_resolution: Mutex<(GuestPathTranslator, LocalModuleResolutionCache)>,
}

impl JavascriptExecution {
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub fn child_pid(&self) -> u32 {
        self.child_pid
    }

    pub fn v8_session_handle(&self) -> V8SessionHandle {
        self.v8_session.clone()
    }

    pub fn uses_shared_v8_runtime(&self) -> bool {
        true
    }

    pub fn write_stdin(&mut self, chunk: &[u8]) -> Result<(), JavascriptExecutionError> {
        self.kernel_stdin.write(chunk)?;
        let payload = v8_runtime::json_to_cbor_payload(&json!({
            "dataBase64": v8_runtime::base64_encode_pub(chunk),
        }))
        .map_err(JavascriptExecutionError::Stdin)?;
        self.v8_session
            .send_stream_event("stdin", payload)
            .map_err(JavascriptExecutionError::Stdin)
    }

    pub fn close_stdin(&mut self) -> Result<(), JavascriptExecutionError> {
        self.kernel_stdin.close();
        let _ = self.v8_session.send_stream_event("stdin_end", vec![]);
        Ok(())
    }

    pub(crate) fn write_kernel_stdin_only(
        &mut self,
        chunk: &[u8],
    ) -> Result<(), JavascriptExecutionError> {
        self.kernel_stdin.write(chunk)
    }

    pub(crate) fn close_kernel_stdin_only(&mut self) {
        self.kernel_stdin.close();
    }

    pub fn read_kernel_stdin_sync_rpc(
        &self,
        request: &JavascriptSyncRpcRequest,
    ) -> Result<Value, JavascriptExecutionError> {
        if request.method != "__kernel_stdin_read" {
            return Ok(Value::Null);
        }

        Ok(self.kernel_stdin.read(&request.args))
    }

    pub(crate) fn handle_kernel_stdin_sync_rpc(
        &mut self,
        request: &JavascriptSyncRpcRequest,
    ) -> Result<bool, JavascriptExecutionError> {
        if request.method != "__kernel_stdin_read" {
            return Ok(false);
        }

        let response = self.kernel_stdin.read(&request.args);
        self.respond_sync_rpc_success(request.id, response)?;
        Ok(true)
    }

    pub fn terminate(&self) -> Result<(), JavascriptExecutionError> {
        self.v8_session
            .terminate()
            .map_err(JavascriptExecutionError::Terminate)
    }

    pub fn send_stream_event(
        &self,
        event_type: &str,
        payload: Value,
    ) -> Result<(), JavascriptExecutionError> {
        let payload = v8_runtime::json_to_cbor_payload(&payload)
            .map_err(|error| JavascriptExecutionError::RpcResponse(error.to_string()))?;
        self.v8_session
            .send_stream_event(event_type, payload)
            .map_err(|error| JavascriptExecutionError::RpcResponse(error.to_string()))
    }

    pub fn respond_sync_rpc_success(
        &mut self,
        id: u64,
        result: Value,
    ) -> Result<(), JavascriptExecutionError> {
        let phase_start = Instant::now();
        match self.clear_pending_sync_rpc(id)? {
            PendingSyncRpcResolution::Pending => {}
            PendingSyncRpcResolution::TimedOut => {
                return Err(JavascriptExecutionError::ExpiredSyncRpcRequest(id));
            }
            PendingSyncRpcResolution::Missing => {}
        }
        record_sync_bridge_phase(
            "sync_rpc_response",
            "response_clear_pending",
            phase_start.elapsed(),
        );

        let phase_start = Instant::now();
        let payload = translate_legacy_bridge_value_to_v8(&result);
        record_sync_bridge_phase(
            "sync_rpc_response",
            "response_translate_value",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        let payload = v8_runtime::json_to_cbor_payload(&payload)
            .map_err(|e| JavascriptExecutionError::RpcResponse(e.to_string()))?;
        record_sync_bridge_phase(
            "sync_rpc_response",
            "response_encode_cbor",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        let result = self
            .v8_session
            .send_bridge_response(id, 0, payload)
            .map_err(|e| JavascriptExecutionError::RpcResponse(e.to_string()));
        record_sync_bridge_phase("sync_rpc_response", "response_send", phase_start.elapsed());
        result
    }

    pub fn respond_sync_rpc_raw_success(
        &mut self,
        id: u64,
        payload: Vec<u8>,
    ) -> Result<(), JavascriptExecutionError> {
        let phase_start = Instant::now();
        match self.clear_pending_sync_rpc(id)? {
            PendingSyncRpcResolution::Pending => {}
            PendingSyncRpcResolution::TimedOut => {
                return Err(JavascriptExecutionError::ExpiredSyncRpcRequest(id));
            }
            PendingSyncRpcResolution::Missing => {}
        }
        record_sync_bridge_phase(
            "sync_rpc_raw_response",
            "response_clear_pending",
            phase_start.elapsed(),
        );

        let phase_start = Instant::now();
        let result = self
            .v8_session
            .send_bridge_response(id, 2, payload)
            .map_err(|e| JavascriptExecutionError::RpcResponse(e.to_string()));
        record_sync_bridge_phase(
            "sync_rpc_raw_response",
            "response_send",
            phase_start.elapsed(),
        );
        result
    }

    pub fn respond_sync_rpc_error(
        &mut self,
        id: u64,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), JavascriptExecutionError> {
        match self.clear_pending_sync_rpc(id)? {
            PendingSyncRpcResolution::Pending => {}
            PendingSyncRpcResolution::TimedOut => {
                return Err(JavascriptExecutionError::ExpiredSyncRpcRequest(id));
            }
            PendingSyncRpcResolution::Missing => {}
        }

        let error_msg = format!("{}: {}", code.into(), message.into());
        self.v8_session
            .send_bridge_response(id, 1, error_msg.into_bytes())
            .map_err(|e| JavascriptExecutionError::RpcResponse(e.to_string()))
    }

    pub async fn poll_event(
        &self,
        timeout: Duration,
    ) -> Result<Option<JavascriptExecutionEvent>, JavascriptExecutionError> {
        if timeout.is_zero() {
            let mut events = self.events.lock().await;
            return match events.try_recv() {
                Ok(event) => Ok(Some(event)),
                Err(TokioTryRecvError::Empty) => Ok(None),
                Err(TokioTryRecvError::Disconnected) => {
                    Err(JavascriptExecutionError::EventChannelClosed)
                }
            };
        }

        let mut events = self.events.lock().await;
        match time::timeout(timeout, events.recv()).await {
            Ok(Some(event)) => Ok(Some(event)),
            Ok(None) => Err(JavascriptExecutionError::EventChannelClosed),
            Err(_) => Ok(None),
        }
    }

    pub fn poll_event_blocking(
        &self,
        timeout: Duration,
    ) -> Result<Option<JavascriptExecutionEvent>, JavascriptExecutionError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(mut events) = self.events.try_lock() {
                match events.try_recv() {
                    Ok(event) => return Ok(Some(event)),
                    Err(TokioTryRecvError::Disconnected) => {
                        return Err(JavascriptExecutionError::EventChannelClosed);
                    }
                    Err(TokioTryRecvError::Empty) => {
                        if Instant::now() >= deadline {
                            return Ok(None);
                        }
                    }
                }
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn wait(mut self) -> Result<JavascriptExecutionResult, JavascriptExecutionError> {
        self.close_stdin()?;
        let mut events = std::mem::replace(
            self.events.get_mut(),
            channel(JAVASCRIPT_EVENT_CHANNEL_CAPACITY).1,
        );
        let execution_id = std::mem::take(&mut self.execution_id);

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        loop {
            match events.blocking_recv() {
                Some(JavascriptExecutionEvent::Stdout(chunk)) => {
                    append_captured_output(&mut stdout, chunk, "stdout")?;
                }
                Some(JavascriptExecutionEvent::Stderr(chunk)) => {
                    append_captured_output(&mut stderr, chunk, "stderr")?;
                }
                Some(JavascriptExecutionEvent::SyncRpcRequest(request)) => {
                    // The standalone engine has no kernel/service loop. Service
                    // module-resolution RPCs host-directly (the only FS source
                    // available here) so `wait()` does not deadlock; everything
                    // else is unsupported off the VM path.
                    if self.try_service_standalone_module_sync_rpc(&request)? {
                        continue;
                    }
                    return Err(JavascriptExecutionError::PendingSyncRpcRequest(request.id));
                }
                Some(JavascriptExecutionEvent::SignalState { .. }) => {}
                Some(JavascriptExecutionEvent::Exited(exit_code)) => {
                    return Ok(JavascriptExecutionResult {
                        execution_id,
                        exit_code,
                        stdout,
                        stderr,
                    });
                }
                None => return Err(JavascriptExecutionError::EventChannelClosed),
            }
        }
    }

    /// Service a module-resolution sync RPC host-directly, for consumers that
    /// drive the V8 bridge without a kernel/service loop (the standalone
    /// `wait()` loop and the Python/WASM prewarm loops). Uses this execution's
    /// own path translator (captured at start, including any runtime path
    /// mappings) and a persistent cache. Returns `Ok(true)` if the request was a
    /// module method and was answered, `Ok(false)` if it should fall through.
    ///
    /// The real VM runtime resolves modules against the kernel VFS on the
    /// sidecar service loop and never calls this.
    pub fn try_service_standalone_module_sync_rpc(
        &mut self,
        request: &JavascriptSyncRpcRequest,
    ) -> Result<bool, JavascriptExecutionError> {
        let result = {
            let mut guard = self.module_resolution.lock().map_err(|_| {
                JavascriptExecutionError::RpcResponse(String::from(
                    "standalone module resolution state poisoned",
                ))
            })?;
            let (translator, cache) = &mut *guard;
            let mut resolver = ModuleResolver::new(translator, cache);
            match request.method.as_str() {
                "__resolve_module" | "_resolveModule" | "_resolveModuleSync" => {
                    let specifier = request.args.first().and_then(Value::as_str).unwrap_or("");
                    let parent = request.args.get(1).and_then(Value::as_str).unwrap_or("/");
                    let mode = match request.args.get(2).and_then(Value::as_str) {
                        Some("import") => ModuleResolveMode::Import,
                        Some("require") => ModuleResolveMode::Require,
                        _ if request.method == "_resolveModuleSync" => ModuleResolveMode::Require,
                        _ => ModuleResolveMode::Import,
                    };
                    resolver
                        .resolve_module(specifier, parent, mode)
                        .map(Value::String)
                        .unwrap_or(Value::Null)
                }
                "__load_file" | "_loadFile" | "_loadFileSync" => resolver
                    .load_file(request.args.first().and_then(Value::as_str).unwrap_or(""))
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                "__module_format" | "_moduleFormat" => resolver
                    .module_format(request.args.first().and_then(Value::as_str).unwrap_or(""))
                    .map(|format| Value::String(String::from(format.as_str())))
                    .unwrap_or(Value::Null),
                "__batch_resolve_modules" | "_batchResolveModules" => {
                    resolver.batch_resolve_modules(&request.args)
                }
                _ => return Ok(false),
            }
        };
        self.respond_sync_rpc_success(request.id, result)?;
        Ok(true)
    }

    fn clear_pending_sync_rpc(
        &self,
        id: u64,
    ) -> Result<PendingSyncRpcResolution, JavascriptExecutionError> {
        let mut pending = self.pending_sync_rpc.lock().map_err(|_| {
            JavascriptExecutionError::RpcResponse(String::from(
                "sync RPC pending-request state lock poisoned",
            ))
        })?;
        match *pending {
            Some(PendingSyncRpcState::Pending(current)) if current == id => {
                *pending = None;
                Ok(PendingSyncRpcResolution::Pending)
            }
            Some(PendingSyncRpcState::TimedOut(current)) if current == id => {
                Ok(PendingSyncRpcResolution::TimedOut)
            }
            _ => Ok(PendingSyncRpcResolution::Missing),
        }
    }
}

impl Drop for JavascriptExecution {
    fn drop(&mut self) {
        let _ = self.v8_session.destroy();
    }
}

fn append_captured_output(
    target: &mut Vec<u8>,
    chunk: Vec<u8>,
    stream: &'static str,
) -> Result<(), JavascriptExecutionError> {
    let next_len = target.len().checked_add(chunk.len()).ok_or(
        JavascriptExecutionError::OutputBufferExceeded {
            stream,
            limit: JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES,
        },
    )?;
    if next_len > JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES {
        return Err(JavascriptExecutionError::OutputBufferExceeded {
            stream,
            limit: JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES,
        });
    }

    target.extend(chunk);
    Ok(())
}

struct V8SessionRegistrationGuard<'a> {
    v8_host: &'a V8RuntimeHost,
    session_id: String,
    active: bool,
}

impl<'a> V8SessionRegistrationGuard<'a> {
    fn new(v8_host: &'a V8RuntimeHost, session_id: String) -> Self {
        Self {
            v8_host,
            session_id,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for V8SessionRegistrationGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            self.v8_host.unregister_session(&self.session_id);
        }
    }
}

struct PendingV8SessionRegistration<'a> {
    frame_receiver: TrackedReceiver<BinaryFrame>,
    registration_guard: V8SessionRegistrationGuard<'a>,
}

fn register_v8_session<F>(
    v8_host: &V8RuntimeHost,
    session_id: String,
    heap_limit_mb: u32,
    cpu_time_limit_ms: u32,
    wall_clock_limit_ms: u32,
    warm_hint: Option<WarmSessionHint>,
    create_session: F,
) -> Result<PendingV8SessionRegistration<'_>, JavascriptExecutionError>
where
    F: FnOnce(RuntimeCommand) -> std::io::Result<()>,
{
    let frame_receiver = v8_host
        .register_session(&session_id)
        .map_err(JavascriptExecutionError::Spawn)?;
    let registration_guard = V8SessionRegistrationGuard::new(v8_host, session_id.clone());

    create_session(RuntimeCommand::CreateSession {
        session_id,
        heap_limit_mb: (heap_limit_mb > 0).then_some(heap_limit_mb),
        cpu_time_limit_ms: (cpu_time_limit_ms > 0).then_some(cpu_time_limit_ms),
        wall_clock_limit_ms: (wall_clock_limit_ms > 0).then_some(wall_clock_limit_ms),
        warm_hint,
    })
    .map_err(JavascriptExecutionError::Spawn)?;

    Ok(PendingV8SessionRegistration {
        frame_receiver,
        registration_guard,
    })
}

#[derive(Default)]
pub struct JavascriptExecutionEngine {
    next_context_id: usize,
    next_execution_id: usize,
    contexts: BTreeMap<String, JavascriptContext>,
    import_caches: BTreeMap<String, NodeImportCache>,
    v8_host: Option<V8RuntimeHost>,
    event_notify: Option<Arc<Notify>>,
}

impl std::fmt::Debug for JavascriptExecutionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavascriptExecutionEngine")
            .field("next_context_id", &self.next_context_id)
            .field("next_execution_id", &self.next_execution_id)
            .field("contexts", &self.contexts)
            .field("v8_host", &self.v8_host.is_some())
            .finish()
    }
}

impl JavascriptExecutionEngine {
    #[doc(hidden)]
    pub fn set_event_notify(&mut self, notify: Option<Arc<Notify>>) {
        self.event_notify = notify;
    }

    #[doc(hidden)]
    pub fn set_import_cache_base_dir(&mut self, vm_id: impl Into<String>, base_dir: PathBuf) {
        self.import_caches
            .insert(vm_id.into(), NodeImportCache::new_in(base_dir));
    }

    pub fn create_context(&mut self, request: CreateJavascriptContextRequest) -> JavascriptContext {
        self.next_context_id += 1;
        self.import_caches.entry(request.vm_id.clone()).or_default();

        let context = JavascriptContext {
            context_id: format!("js-ctx-{}", self.next_context_id),
            vm_id: request.vm_id,
            bootstrap_module: request.bootstrap_module,
            compile_cache_dir: request
                .compile_cache_root
                .map(resolve_node_import_compile_cache_dir),
        };
        self.contexts
            .insert(context.context_id.clone(), context.clone());
        context
    }

    pub fn start_execution(
        &mut self,
        request: StartJavascriptExecutionRequest,
    ) -> Result<JavascriptExecution, JavascriptExecutionError> {
        self.start_execution_with_module_reader(request, None, None)
    }

    fn ensure_v8_host(&mut self) -> Result<(), JavascriptExecutionError> {
        let should_spawn_v8_host = match self.v8_host.as_mut() {
            Some(v8_host) => !v8_host
                .is_alive()
                .map_err(JavascriptExecutionError::Spawn)?,
            None => true,
        };
        if should_spawn_v8_host {
            self.v8_host = Some(V8RuntimeHost::spawn().map_err(JavascriptExecutionError::Spawn)?);
        }
        Ok(())
    }

    pub(crate) fn snapshot_userland_ready(
        &mut self,
        userland_code: &str,
    ) -> Result<bool, JavascriptExecutionError> {
        self.ensure_v8_host()?;
        Ok(self
            .v8_host
            .as_ref()
            .expect("V8 host initialized")
            .snapshot_ready(userland_code))
    }

    pub(crate) fn pre_warm_snapshot(
        &mut self,
        userland_code: &str,
    ) -> Result<(), JavascriptExecutionError> {
        self.ensure_v8_host()?;
        self.v8_host
            .as_ref()
            .expect("V8 host initialized")
            .pre_warm_snapshot(userland_code)
            .map_err(JavascriptExecutionError::Spawn)
    }

    pub(crate) fn pre_warm_workers(
        &mut self,
        userland_code: &str,
        heap_limit_mb: u32,
        count: usize,
    ) -> Result<(), JavascriptExecutionError> {
        self.ensure_v8_host()?;
        self.v8_host
            .as_ref()
            .expect("V8 host initialized")
            .pre_warm_workers(userland_code, heap_limit_mb, count);
        Ok(())
    }

    /// Like [`start_execution`](Self::start_execution) but with an optional
    /// read-only VFS reader over the mounted `node_modules` tree. When supplied,
    /// the bridge thread resolves module-resolution RPCs inline against this
    /// reader (off the service loop, concurrently with it) instead of routing
    /// them through the service loop. The reader must be `Send` because it is
    /// moved onto the bridge thread; it must read the same mount the guest sees.
    pub fn start_execution_with_module_reader(
        &mut self,
        request: StartJavascriptExecutionRequest,
        module_reader: Option<Box<dyn ModuleFsReader + Send>>,
        guest_reader: Option<Box<dyn agentos_v8_runtime::execution::GuestModuleReader>>,
    ) -> Result<JavascriptExecution, JavascriptExecutionError> {
        let context = self
            .contexts
            .get(&request.context_id)
            .cloned()
            .ok_or_else(|| JavascriptExecutionError::MissingContext(request.context_id.clone()))?;

        if context.vm_id != request.vm_id {
            return Err(JavascriptExecutionError::VmMismatch {
                expected: context.vm_id,
                found: request.vm_id,
            });
        }

        if request.argv.is_empty() {
            return Err(JavascriptExecutionError::EmptyArgv);
        }

        let phase_start = Instant::now();
        // Ensure import cache is materialized (still needed for module resolution)
        let import_cache = self.import_caches.entry(context.vm_id.clone()).or_default();
        import_cache
            .ensure_materialized_with_timeout(javascript_import_cache_materialize_timeout(&request))
            .map_err(JavascriptExecutionError::PrepareImportCache)?;
        let import_cache_guard = import_cache.cleanup_guard();
        record_js_start_phase("js_start_import_cache", phase_start.elapsed());

        self.next_execution_id += 1;
        let execution_id = format!("exec-{}", self.next_execution_id);
        let sync_rpc_timeout = javascript_sync_rpc_timeout(&request);

        let phase_start = Instant::now();
        self.ensure_v8_host()?;
        let v8_host = self.v8_host.as_ref().unwrap();
        record_js_start_phase("js_start_v8_host_ready", phase_start.elapsed());

        let phase_start = Instant::now();
        // Create a V8 session
        let session_id = format!(
            "v8-{execution_id}-{}",
            NEXT_V8_SESSION_ID.fetch_add(1, Ordering::Relaxed)
        );
        let heap_limit_mb = javascript_heap_limit_mb(&request);
        let cpu_time_limit_ms = javascript_cpu_time_limit_ms(&request);
        let wall_clock_limit_ms = javascript_wall_clock_limit_ms(&request);
        let snapshot_userland_code = request
            .guest_runtime
            .snapshot_userland_code
            .clone()
            .unwrap_or_default();
        let warm_hint = Some(WarmSessionHint {
            bridge_code: V8RuntimeHost::bridge_code().to_owned(),
            userland_code: snapshot_userland_code.clone(),
            heap_limit_mb: (heap_limit_mb > 0).then_some(heap_limit_mb),
        });
        if snapshot_userland_code.is_empty() && heap_limit_mb == 0 {
            v8_host.seed_default_warm_workers_async();
        }
        let PendingV8SessionRegistration {
            frame_receiver,
            mut registration_guard,
        } = register_v8_session(
            v8_host,
            session_id.clone(),
            heap_limit_mb,
            cpu_time_limit_ms,
            wall_clock_limit_ms,
            warm_hint,
            |command| v8_host.create_session_from_command(command),
        )?;
        record_js_start_phase("js_start_v8_session_register", phase_start.elapsed());

        let phase_start = Instant::now();
        // Build user code: prefer inline code, fall back to entrypoint-based
        let translator = GuestPathTranslator::from_request(&request);
        let host_entrypoint = translator.resolve_host_entrypoint(&request.cwd, &request.argv[0]);
        let guest_entrypoint = if request.argv[0] == "-e" || request.argv[0] == "--eval" {
            request.argv[0].clone()
        } else if let Some(explicit_guest_entrypoint) = request
            .env
            .get(NODE_GUEST_ENTRYPOINT_ENV)
            .filter(|value| value.starts_with('/'))
        {
            // Part B (guest-VFS adapter launch): the sidecar already resolved the
            // GUEST entrypoint path (AGENTOS_GUEST_ENTRYPOINT). Use it directly as
            // the sourceURL / module-resolution base instead of translating the
            // host entrypoint — `host_to_guest_string` misses for guest-native
            // mounts (`agentos_packages`, whose host staging dir is not in the
            // translation map) and falls back to `/unknown/<cmd>`, which then
            // poisons the adapter's own relative/bare imports. For host-backed
            // mounts the two values are equal, so this is a no-op there. Applies
            // to child launches too (they set AGENTOS_GUEST_ENTRYPOINT as well),
            // which is the child-process `/unknown` case.
            explicit_guest_entrypoint.clone()
        } else {
            translator.host_to_guest_string(&host_entrypoint)
        };
        let process_argv = if matches!(guest_entrypoint.as_str(), "-e" | "--eval") {
            std::iter::once(String::from("node"))
                .chain(request.argv.iter().skip(1).cloned())
                .collect::<Vec<_>>()
        } else {
            std::iter::once(String::from("node"))
                .chain(std::iter::once(guest_entrypoint.clone()))
                .chain(request.argv.iter().skip(1).cloned())
                .collect::<Vec<_>>()
        };
        let inline_code = request
            .inline_code
            .clone()
            .map(|inline_code| strip_javascript_hashbang(&inline_code));
        let use_module_mode = request
            .env
            .get(NODE_GUEST_ENTRYPOINT_MODULE_MODE_ENV)
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            || host_entrypoint_uses_module_mode(&host_entrypoint)
            || inline_code
                .as_deref()
                .is_some_and(inline_code_uses_module_mode);
        if !matches!(guest_entrypoint.as_str(), "-e" | "--eval") && !use_module_mode {
            if let Some(inline_code) = inline_code.as_ref() {
                if let Some(parent) = host_entrypoint.parent() {
                    fs::create_dir_all(parent)
                        .map_err(JavascriptExecutionError::PrepareImportCache)?;
                }
                fs::write(&host_entrypoint, inline_code)
                    .map_err(JavascriptExecutionError::PrepareImportCache)?;
            }
        }
        let user_code = if matches!(guest_entrypoint.as_str(), "-e" | "--eval") {
            inline_code.unwrap_or_else(|| build_v8_user_code(&guest_entrypoint, &request.env))
        } else if use_module_mode {
            if let Some(inline_code) = inline_code {
                format!("{inline_code}\n//# sourceURL={guest_entrypoint}")
            } else {
                strip_javascript_hashbang(&fs::read_to_string(&host_entrypoint).map_err(
                    |error| {
                        JavascriptExecutionError::PrepareImportCache(std::io::Error::new(
                            error.kind(),
                            format!(
                                "failed to read JavaScript entrypoint {}: {error}",
                                host_entrypoint.display()
                            ),
                        ))
                    },
                )?)
            }
        } else {
            build_v8_user_code(&guest_entrypoint, &request.env)
        };
        let user_code = prepend_v8_runtime_shim(
            user_code,
            &guest_entrypoint,
            &process_argv,
            translator.guest_cwd(),
            &request.env,
            heap_limit_mb,
            &request.guest_runtime,
        );
        record_js_start_phase("js_start_build_user_code", phase_start.elapsed());

        let phase_start = Instant::now();
        // Create session handle for sending bridge responses
        let v8_session = v8_host.session_handle(session_id.clone());

        // Start the event bridge before execution so early sync bridge calls
        // made during module instantiation/evaluation cannot deadlock waiting
        // for a response while no host thread is draining session frames yet.
        let pending_sync_rpc = Arc::new(Mutex::new(None));
        let kernel_stdin = Arc::new(LocalKernelStdinBridge::default());
        let standalone_translator = translator.clone();
        // default + in-place assign: LocalBridgeState is Drop, so `..Default::default()`
        // (E0509) is not allowed.
        let mut local_bridge = LocalBridgeState::default();
        local_bridge.translator = translator;
        local_bridge.kernel_stdin = kernel_stdin.clone();
        local_bridge.v8_session = Some(v8_session.clone());
        local_bridge.module_reader = module_reader;
        local_bridge.module_resolution = GuestModuleResolution::from_env(&request.env);
        local_bridge.forward_kernel_stdin_rpc = request
            .env
            .get(FORWARD_KERNEL_STDIN_RPC_ENV)
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        let events = spawn_v8_event_bridge(
            frame_receiver,
            pending_sync_rpc.clone(),
            sync_rpc_timeout,
            v8_session.clone(),
            local_bridge,
            self.event_notify.clone(),
        );
        record_js_start_phase("js_start_event_bridge", phase_start.elapsed());

        let phase_start = Instant::now();
        // Install the direct module reader on the session thread BEFORE the Execute
        // frame so the SetModuleReader command (routed through the same dispatch
        // queue) arrives first; module loads then read source directly on the V8
        // thread instead of round-tripping the bridge.
        if let Some(guest_reader) = guest_reader {
            v8_session
                .set_module_reader(guest_reader)
                .map_err(JavascriptExecutionError::Spawn)?;
        }
        record_js_start_phase("js_start_install_module_reader", phase_start.elapsed());

        let phase_start = Instant::now();
        // Execute bridge code + user code in the V8 isolate
        v8_session
            .execute(
                if use_module_mode { 1 } else { 0 },
                guest_entrypoint.clone(),
                V8RuntimeHost::bridge_code().to_owned(),
                String::new(),
                snapshot_userland_code,
                request.guest_runtime.high_resolution_time,
                user_code,
                request.wasm_module_bytes.clone(),
            )
            .map_err(JavascriptExecutionError::Spawn)?;
        registration_guard.disarm();
        record_js_start_phase("js_start_send_execute", phase_start.elapsed());

        Ok(JavascriptExecution {
            execution_id,
            child_pid: v8_host.child_pid(),
            events: tokio::sync::Mutex::new(events),
            pending_sync_rpc,
            kernel_stdin,
            _import_cache_guard: import_cache_guard,
            v8_session,
            module_resolution: Mutex::new((
                standalone_translator,
                LocalModuleResolutionCache::default(),
            )),
        })
    }

    pub fn dispose_vm(&mut self, vm_id: &str) {
        self.contexts.retain(|_, context| context.vm_id != vm_id);
        self.import_caches.remove(vm_id);
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn materialize_import_cache_for_vm(
        &mut self,
        vm_id: &str,
    ) -> Result<&std::path::Path, std::io::Error> {
        let import_cache = self.import_caches.entry(vm_id.to_owned()).or_default();
        import_cache.ensure_materialized()?;
        Ok(import_cache.cache_path())
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn import_cache_path_for_vm(&self, vm_id: &str) -> Option<&std::path::Path> {
        self.import_caches
            .get(vm_id)
            .map(NodeImportCache::cache_path)
    }
}

fn set_pending_sync_rpc_state(
    pending_sync_rpc: &Arc<Mutex<Option<PendingSyncRpcState>>>,
    id: u64,
) -> Result<(), JavascriptExecutionError> {
    let mut pending = pending_sync_rpc.lock().map_err(|_| {
        JavascriptExecutionError::RpcResponse(String::from(
            "sync RPC pending-request state lock poisoned",
        ))
    })?;
    *pending = Some(PendingSyncRpcState::Pending(id));
    Ok(())
}

fn resolve_node_import_compile_cache_dir(root_dir: PathBuf) -> PathBuf {
    root_dir.join(format!(
        "node-imports-v{NODE_IMPORT_COMPILE_CACHE_NAMESPACE_VERSION}-{:016x}",
        stable_compile_cache_namespace_hash()
    ))
}

fn stable_compile_cache_namespace_hash() -> u64 {
    stable_hash64(
        [
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            NODE_ENTRYPOINT_ENV,
            NODE_BOOTSTRAP_ENV,
            NODE_GUEST_ARGV_ENV,
            NODE_PREWARM_IMPORTS_ENV,
            NODE_WARMUP_MARKER_VERSION,
        ]
        .into_iter()
        .chain(NODE_WARMUP_SPECIFIERS.iter().copied())
        .collect::<Vec<_>>()
        .join("\n")
        .as_bytes(),
    )
}

fn javascript_sync_rpc_timeout(request: &StartJavascriptExecutionRequest) -> Duration {
    let timeout_ms = request
        .limits
        .sync_rpc_wait_timeout_ms
        .filter(|value| *value > 0)
        .unwrap_or(NODE_SYNC_RPC_DEFAULT_WAIT_TIMEOUT_MS);
    Duration::from_millis(timeout_ms)
}

fn javascript_heap_limit_mb(request: &StartJavascriptExecutionRequest) -> u32 {
    request
        .limits
        .v8_heap_limit_mb
        .filter(|value| *value > 0)
        .unwrap_or(0)
}

fn javascript_import_cache_materialize_timeout(
    request: &StartJavascriptExecutionRequest,
) -> Duration {
    let timeout_ms = request
        .limits
        .import_cache_materialize_timeout_ms
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS);
    Duration::from_millis(timeout_ms)
}

/// Resolve the TRUE CPU-time budget (ms) for a JavaScript execution.
///
/// Read from typed `limits.jsRuntime.cpuTimeLimitMs`, falling back to a bounded
/// default when unset. `0` remains an explicit trusted opt-out and is normalized
/// to `None` by the V8 session.
fn javascript_cpu_time_limit_ms(request: &StartJavascriptExecutionRequest) -> u32 {
    request
        .limits
        .cpu_time_limit_ms
        // Generous active-CPU budget: long-lived adapters are still not capped
        // on wall-clock, but CPU-bound runaways no longer pin a core forever by
        // default.
        .unwrap_or(DEFAULT_V8_CPU_TIME_LIMIT_MS)
}

/// Resolve the opt-in WALL-CLOCK backstop (ms) for a JavaScript execution.
///
/// Read from typed `limits.jsRuntime.wallClockLimitMs`, falling back to `0` (no
/// limit). `0` is normalized to `None` by the V8 session, so the wall-clock
/// `TimeoutGuard` is NOT armed and the guest runs without a wall-clock limit.
/// This is INDEPENDENT of the CPU-time budget: setting only one arms only that
/// guard.
fn javascript_wall_clock_limit_ms(request: &StartJavascriptExecutionRequest) -> u32 {
    request
        .limits
        .wall_clock_limit_ms
        .unwrap_or(DEFAULT_V8_WALL_CLOCK_LIMIT_MS)
}

fn spawn_javascript_sync_rpc_timeout(
    id: u64,
    timeout: Duration,
    pending_state: Arc<Mutex<Option<PendingSyncRpcState>>>,
    responses: Option<JavascriptSyncRpcResponseWriter>,
) {
    let Some(responses) = responses else {
        return;
    };

    thread::spawn(move || {
        thread::sleep(timeout);

        let should_timeout = match pending_state.lock() {
            Ok(mut guard) if *guard == Some(PendingSyncRpcState::Pending(id)) => {
                *guard = Some(PendingSyncRpcState::TimedOut(id));
                true
            }
            Ok(_) => false,
            Err(_) => false,
        };

        if !should_timeout {
            return;
        }

        let _ = write_javascript_sync_rpc_response(
            &responses,
            json!({
                "id": id,
                "ok": false,
                "error": {
                    "code": "ERR_AGENTOS_NODE_SYNC_RPC_TIMEOUT",
                    "message": format!(
                        "guest JavaScript sync RPC request {id} timed out after {}ms",
                        timeout.as_millis()
                    ),
                },
            }),
        );
    });
}

fn spawn_javascript_sync_rpc_reader(
    reader: File,
    sender: mpsc::Sender<JavascriptProcessEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match parse_javascript_sync_rpc_request(trimmed) {
                        Ok(request) => {
                            if sender
                                .send(JavascriptProcessEvent::SyncRpcRequest(request))
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(message) => {
                            if sender
                                .send(JavascriptProcessEvent::RawStderr(
                                    format!("{message}\n").into_bytes(),
                                ))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(JavascriptProcessEvent::RawStderr(
                        format!("failed to read JavaScript sync RPC request: {error}\n")
                            .into_bytes(),
                    ));
                    return;
                }
            }
        }
    })
}

fn parse_javascript_sync_rpc_request(line: &str) -> Result<JavascriptSyncRpcRequest, String> {
    let wire: JavascriptSyncRpcRequestWire =
        serde_json::from_str(line).map_err(|error| error.to_string())?;
    Ok(JavascriptSyncRpcRequest {
        id: wire.id,
        method: wire.method,
        args: wire.args,
        raw_bytes_args: HashMap::new(),
    })
}

fn write_javascript_sync_rpc_response(
    writer: &JavascriptSyncRpcResponseWriter,
    response: Value,
) -> Result<(), JavascriptExecutionError> {
    let mut payload = serde_json::to_vec(&response)
        .map_err(|error| JavascriptExecutionError::RpcResponse(error.to_string()))?;
    payload.push(b'\n');
    writer.send(payload)
}

fn spawn_javascript_sync_rpc_response_writer(
    writer: File,
    receiver: Receiver<Vec<u8>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut writer = BufWriter::new(writer);
        while let Ok(payload) = receiver.recv() {
            if writer
                .write_all(&payload)
                .and_then(|()| writer.flush())
                .is_err()
            {
                return;
            }
        }
    })
}

/// Build the user code wrapper for V8 execution.
/// This wraps the entrypoint in a way that the V8 bridge can execute it.
fn build_v8_user_code(entrypoint: &str, env: &BTreeMap<String, String>) -> String {
    // The bridge code (polyfills) sets up the module system and globals.
    // User code is executed after the bridge completes.
    // For file-based entrypoints, we load and execute them through the module system.
    // For inline code (-e flag), we execute directly.
    if entrypoint == "-e" || entrypoint == "--eval" {
        // Inline code from NODE_EVAL or similar
        env.get("AGENTOS_NODE_EVAL").cloned().unwrap_or_default()
    } else {
        // Module entrypoint - use require to load it
        format!(
            "require({});\n//# sourceURL={}",
            serde_json::to_string(entrypoint).unwrap_or_else(|_| format!("\"{}\"", entrypoint)),
            entrypoint
        )
    }
}

fn host_entrypoint_uses_module_mode(entrypoint: &Path) -> bool {
    // Agent adapters are launched via an extensionless `/opt/agentos/bin/<cmd>`
    // symlink into the packed package's `node_modules/<name>/<entry>`. Resolve it
    // to the real file so the extension check and the nearest-`package.json` walk
    // reflect the package (which carries `"type": "module"`), not the symlink farm
    // (which has neither an extension nor a package.json).
    let resolved = fs::canonicalize(entrypoint).unwrap_or_else(|_| entrypoint.to_path_buf());
    match resolved.extension().and_then(|ext| ext.to_str()) {
        Some("mjs" | "mts") => true,
        Some("js") => nearest_package_json_type(&resolved).as_deref() == Some("module"),
        _ => false,
    }
}

fn inline_code_uses_module_mode(source: &str) -> bool {
    let sanitized = strip_non_code_segments(source);
    let tokens = tokenize_inline_module_source(&sanitized);
    let has_commonjs_signal = tokens.windows(3).any(|window| {
        matches!(
            window,
            [
                InlineModuleToken::Identifier("module"),
                InlineModuleToken::Punct('.'),
                InlineModuleToken::Identifier("exports")
            ]
        )
    }) || tokens.windows(2).any(|window| {
        matches!(
            window,
            [
                InlineModuleToken::Identifier("exports"),
                InlineModuleToken::Punct('.' | '[')
            ] | [
                InlineModuleToken::Identifier("require"),
                InlineModuleToken::Punct('(')
            ]
        )
    });

    if has_commonjs_signal {
        return false;
    }

    tokens.windows(2).any(|window| match window {
        [InlineModuleToken::Identifier("import"), InlineModuleToken::Punct('.')] => true,
        [InlineModuleToken::Identifier("import"), InlineModuleToken::Punct('(' | ':')] => false,
        [InlineModuleToken::Identifier("import"), InlineModuleToken::Identifier(_)
        | InlineModuleToken::Punct('{')
        | InlineModuleToken::Punct('*')
        | InlineModuleToken::StringLiteral] => true,
        [InlineModuleToken::Identifier("export"), InlineModuleToken::Identifier(
            "default" | "const" | "let" | "var" | "function" | "class" | "async" | "enum" | "type"
            | "interface",
        )
        | InlineModuleToken::Punct('{')
        | InlineModuleToken::Punct('*')] => true,
        _ => false,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineModuleToken<'a> {
    Identifier(&'a str),
    StringLiteral,
    Punct(char),
}

const INLINE_MODULE_STRING_PLACEHOLDER: char = '\u{1F}';

fn strip_non_code_segments(source: &str) -> String {
    let mut sanitized = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    sanitize_javascript_code(bytes, &mut index, &mut sanitized, None);
    sanitized
}

fn sanitize_javascript_code(
    bytes: &[u8],
    index: &mut usize,
    output: &mut String,
    until_brace_depth: Option<usize>,
) {
    let mut brace_depth = 0usize;

    while *index < bytes.len() {
        let current = bytes[*index];

        if let Some(target_depth) = until_brace_depth {
            match current {
                b'{' => brace_depth += 1,
                b'}' => {
                    if brace_depth == target_depth {
                        output.push(' ');
                        *index += 1;
                        return;
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        match current {
            b'/' if bytes.get(*index + 1) == Some(&b'/') => {
                output.push(' ');
                output.push(' ');
                *index += 2;
                while *index < bytes.len() {
                    let comment_byte = bytes[*index];
                    *index += 1;
                    if comment_byte == b'\n' {
                        output.push('\n');
                        break;
                    }
                    output.push(' ');
                }
            }
            b'/' if bytes.get(*index + 1) == Some(&b'*') => {
                output.push(' ');
                output.push(' ');
                *index += 2;
                while *index < bytes.len() {
                    let comment_byte = bytes[*index];
                    if comment_byte == b'*' && bytes.get(*index + 1) == Some(&b'/') {
                        output.push(' ');
                        output.push(' ');
                        *index += 2;
                        break;
                    }
                    output.push(if comment_byte == b'\n' { '\n' } else { ' ' });
                    *index += 1;
                }
            }
            b'\'' | b'"' => sanitize_string_literal(bytes, index, output, current),
            b'`' => sanitize_template_literal(bytes, index, output),
            _ => {
                output.push(char::from(current));
                *index += 1;
            }
        }
    }
}

fn sanitize_string_literal(bytes: &[u8], index: &mut usize, output: &mut String, quote: u8) {
    output.push(INLINE_MODULE_STRING_PLACEHOLDER);
    *index += 1;

    while *index < bytes.len() {
        let current = bytes[*index];
        *index += 1;
        match current {
            b'\\' => {
                if *index < bytes.len() {
                    *index += 1;
                }
            }
            c if c == quote => break,
            _ => {}
        }
    }
}

fn sanitize_template_literal(bytes: &[u8], index: &mut usize, output: &mut String) {
    output.push(INLINE_MODULE_STRING_PLACEHOLDER);
    *index += 1;

    while *index < bytes.len() {
        let current = bytes[*index];
        match current {
            b'\\' => {
                *index += 1;
                if *index < bytes.len() {
                    *index += 1;
                }
            }
            b'`' => {
                *index += 1;
                break;
            }
            b'$' if bytes.get(*index + 1) == Some(&b'{') => {
                output.push(' ');
                output.push(' ');
                *index += 2;
                sanitize_javascript_code(bytes, index, output, Some(0));
                output.push(INLINE_MODULE_STRING_PLACEHOLDER);
            }
            b'\n' => {
                output.push('\n');
                *index += 1;
            }
            _ => {
                *index += 1;
            }
        }
    }
}

fn tokenize_inline_module_source(source: &str) -> Vec<InlineModuleToken<'_>> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let current = bytes[index];
        match current {
            b if b.is_ascii_whitespace() => index += 1,
            b if char::from(b) == INLINE_MODULE_STRING_PLACEHOLDER => {
                tokens.push(InlineModuleToken::StringLiteral);
                index += 1;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && matches!(bytes[index], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$')
                {
                    index += 1;
                }
                tokens.push(InlineModuleToken::Identifier(&source[start..index]));
            }
            _ => {
                tokens.push(InlineModuleToken::Punct(char::from(current)));
                index += 1;
            }
        }
    }

    tokens
}

fn nearest_package_json_type(entrypoint: &Path) -> Option<String> {
    let mut current = entrypoint.parent();
    while let Some(dir) = current {
        let package_json = dir.join("package.json");
        if let Ok(contents) = fs::read_to_string(&package_json) {
            if let Ok(pkg) = serde_json::from_str::<LocalPackageJson>(&contents) {
                return pkg.package_type;
            }
        }
        current = dir.parent();
    }
    None
}

fn resolve_v8_entrypoint(cwd: &Path, entrypoint: &str) -> String {
    if entrypoint == "-e" || entrypoint == "--eval" {
        return entrypoint.to_owned();
    }

    let path = Path::new(entrypoint);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    resolved.to_string_lossy().into_owned()
}

fn prepend_v8_runtime_shim(
    user_code: String,
    entrypoint: &str,
    argv: &[String],
    cwd: &str,
    env: &BTreeMap<String, String>,
    // V8 heap cap in MB (`0` = engine default). Threaded from the typed wire
    // limit and interpolated into the shim so guest heap-stats reporting no
    // longer depends on an `AGENTOS_V8_HEAP_LIMIT_MB` env var.
    heap_limit_mb: u32,
    // Typed guest-runtime identity, interpolated into the shim so virtual
    // `process.*` identity no longer rides `AGENTOS_VIRTUAL_PROCESS_*` env vars.
    guest_runtime: &GuestRuntimeConfig,
) -> String {
    let argv_json = serde_json::to_string(argv).unwrap_or_else(|_| String::from("[\"node\"]"));
    let entry_json =
        serde_json::to_string(entrypoint).unwrap_or_else(|_| String::from("\"/<entry>\""));
    let cwd_json = serde_json::to_string(cwd).unwrap_or_else(|_| String::from("\"/\""));
    let env_json = serde_json::to_string(env).unwrap_or_else(|_| String::from("{}"));
    // Virtual process identity object. `Option` fields serialize to `null`, which
    // the shim treats as "unset" (leaving the V8-baked default) — matching the
    // prior behavior when the env var was absent.
    let identity_json = serde_json::json!({
        "pid": guest_runtime.virtual_pid,
        "ppid": guest_runtime.virtual_ppid,
        "uid": guest_runtime.virtual_uid,
        "gid": guest_runtime.virtual_gid,
        "execPath": guest_runtime.virtual_exec_path,
    })
    .to_string();
    // Virtual OS identity (os.cpus/totalmem/freemem/homedir/userInfo/...). Read
    // by the bridge + node-import-cache os polyfill from the `__agentOSVirtualOs`
    // global instead of `AGENTOS_VIRTUAL_OS_*` env vars. Absent fields stay
    // `null`, so the consumers fall back to their built-in defaults.
    let virtual_os_json = serde_json::json!({
        "cpuCount": guest_runtime.os_cpu_count,
        "totalmem": guest_runtime.os_totalmem,
        "freemem": guest_runtime.os_freemem,
        "homedir": guest_runtime.os_homedir,
        "hostname": guest_runtime.os_hostname,
        "tmpdir": guest_runtime.os_tmpdir,
        "type": guest_runtime.os_type,
        "release": guest_runtime.os_release,
        "version": guest_runtime.os_version,
        "machine": guest_runtime.os_machine,
        "shell": guest_runtime.os_shell,
        "user": guest_runtime.os_user,
    })
    .to_string();
    let high_resolution_time = guest_runtime.high_resolution_time;

    format!(
        r#"(function () {{
  const __guestIdentity = {identity_json};
  Object.defineProperty(globalThis, "__agentOSVirtualOs", {{
    configurable: true,
    enumerable: false,
    value: {virtual_os_json},
    writable: true,
  }});
  const nextArgv = {argv_json};
  const entryFile = {entry_json};
  const nextCwd = {cwd_json};
  const nextEnv = {env_json};
  const nextHighResolutionTime = {high_resolution_time};
  try {{
    const previousProcessConfig =
      typeof globalThis._processConfig === "object" && globalThis._processConfig !== null
        ? globalThis._processConfig
        : {{}};
    Object.defineProperty(globalThis, "_processConfig", {{
      configurable: true,
      enumerable: false,
      value: Object.freeze({{
        ...previousProcessConfig,
        cwd: nextCwd,
        env: nextEnv,
        argv: nextArgv,
        high_resolution_time: nextHighResolutionTime,
      }}),
      writable: false,
    }});
  }} catch (_e) {{}}
  Object.defineProperty(globalThis, "__agentOSProcessConfigEnv", {{
    configurable: true,
    enumerable: false,
    value: nextEnv,
    writable: true,
  }});
  const visibleEnv = Object.fromEntries(
    Object.entries(nextEnv).filter(([key]) => !key.startsWith("AGENTOS_"))
  );

  if (typeof process !== "undefined") {{
    process.argv = nextArgv;
    process.argv0 = nextArgv[0] || "node";
    process.env = {{
      ...(process.env || {{}}),
      ...visibleEnv,
    }};
    const configuredHeapLimitMb = {heap_limit_mb};
    if (Number.isFinite(configuredHeapLimitMb) && configuredHeapLimitMb > 0) {{
      Object.defineProperty(globalThis, "__agentOSV8HeapLimitBytes", {{
        configurable: true,
        enumerable: false,
        value: configuredHeapLimitMb * 1024 * 1024,
        writable: true,
      }});
    }}
    if (nextEnv.AGENTOS_ALLOW_PROCESS_BINDINGS === "1" && typeof process.binding === "function") {{
      const originalProcessBinding = process.binding.bind(process);
      process.binding = (name) => {{
        const bindingName = String(name);
        if (
          bindingName === "constants" &&
          typeof __agentOSConstantsBinding !== "undefined"
        ) {{
          const constantsBinding =
            __agentOSConstantsBinding.default ?? __agentOSConstantsBinding;
          return {{
            fs: constantsBinding,
            crypto: constantsBinding,
            zlib: constantsBinding,
            trace: constantsBinding,
            internal: constantsBinding,
            os: {{
              UV_UDP_REUSEADDR: constantsBinding.UV_UDP_REUSEADDR,
              dlopen: constantsBinding.dlopen,
              errno: constantsBinding.errno,
              signals: constantsBinding.signals,
              priority: constantsBinding.priority,
            }},
          }};
        }}
        try {{
          return originalProcessBinding(name);
        }} catch (error) {{
          const originalMessage =
            error && typeof error === "object" && typeof error.message === "string"
              ? error.message
              : String(error);
          throw new Error(
            `process.binding(${{bindingName}}) failed: ${{originalMessage}}`
          );
        }}
      }};
    }}
    const nextPid = Number(__guestIdentity.pid);
    if (Number.isFinite(nextPid) && nextPid > 0) {{
      process.pid = nextPid;
    }}
    const nextPpid = Number(__guestIdentity.ppid);
    if (Number.isFinite(nextPpid) && nextPpid >= 0) {{
      process.ppid = nextPpid;
    }}
    const nextUid = Number(__guestIdentity.uid);
    if (Number.isFinite(nextUid) && nextUid >= 0) {{
      process.uid = nextUid;
      process.euid = nextUid;
    }}
    const nextGid = Number(__guestIdentity.gid);
    if (Number.isFinite(nextGid) && nextGid >= 0) {{
      process.gid = nextGid;
      process.egid = nextGid;
      process.groups = [nextGid];
    }}
    if (typeof __guestIdentity.execPath === "string" && __guestIdentity.execPath.length > 0) {{
      process.execPath = __guestIdentity.execPath;
    }}
    if (nextEnv.AGENTOS_NODE_IPC === "1" && typeof __runtimeInstallProcessIpcBridge === "function") {{
      process.connected = true;
      __runtimeInstallProcessIpcBridge();
    }}
    process.cwd = () => nextCwd;
    process._cwd = nextCwd;
    if (typeof process.getBuiltinModule !== "function") {{
      process.getBuiltinModule = function(specifier) {{
        return globalThis.require ? globalThis.require(specifier) : undefined;
      }};
    }}
  }}

  globalThis.__runtimeStreamStdin = nextEnv.AGENTOS_KEEP_STDIN_OPEN === "1";

  if (
    typeof globalThis.WebAssembly === "object" &&
    globalThis.WebAssembly !== null &&
    typeof globalThis.WebAssembly.instantiateStreaming !== "function"
  ) {{
    globalThis.WebAssembly.instantiateStreaming = async function instantiateStreaming(source, imports) {{
      const response = await source;
      if (response == null || typeof response.arrayBuffer !== "function") {{
        throw new TypeError(
          "WebAssembly.instantiateStreaming requires a Response or promise for one",
        );
      }}
      const bytes = new Uint8Array(await response.arrayBuffer());
      return globalThis.WebAssembly.instantiate(bytes, imports);
    }};
  }}

  if (
    typeof globalThis.require === "undefined" &&
    typeof globalThis._moduleModule?.createRequire === "function"
  ) {{
    const requireEntryFile =
      entryFile === "-e" || entryFile === "--eval"
        ? nextCwd === "/"
          ? "/__agentos_eval__.js"
          : `${{nextCwd.replace(/\/+$/, "")}}/__agentos_eval__.js`
        : entryFile;
    globalThis.require =
      globalThis._moduleModule.createRequire(requireEntryFile);
  }}

  // jsRuntime platform tiering: the guest JS host surface is baked into the
  // shared V8 snapshot, so non-node platforms are produced by subtractively
  // scrubbing baked globals here, per execution.
  const __jsPlatform = nextEnv.AGENTOS_JS_PLATFORM || "node";
  // Install the builtin allow-list gate consulted by the bridge's
  // rejectRestrictedBuiltinRequest (covers require + ESM builtin loads). Present
  // for non-node platforms (empty => deny all) and node + explicit allow-list;
  // absent => unrestricted (node default).
  {{
    const __builtinAllowRaw = nextEnv.AGENTOS_JS_BUILTIN_ALLOWLIST;
    if (typeof __builtinAllowRaw === "string") {{
      let __allowList = [];
      try {{ __allowList = JSON.parse(__builtinAllowRaw); }} catch (_e) {{ __allowList = []; }}
      if (typeof globalThis.__agentOSInitJsRuntime === "function") {{
        globalThis.__agentOSInitJsRuntime(Array.isArray(__allowList) ? __allowList : []);
      }}
      try {{ delete globalThis.__agentOSInitJsRuntime; }} catch (_e) {{}}
    }}
  }}
  if (__jsPlatform !== "node") {{
    const __dropGlobal = (name) => {{
      try {{ delete globalThis[name]; }} catch (_e) {{}}
      if (
        Object.prototype.hasOwnProperty.call(globalThis, name) ||
        typeof globalThis[name] !== "undefined"
      ) {{
        try {{ globalThis[name] = undefined; }} catch (_e) {{}}
        try {{
          Object.defineProperty(globalThis, name, {{
            value: undefined,
            configurable: true,
            writable: true,
          }});
        }} catch (_e) {{}}
        try {{ delete globalThis[name]; }} catch (_e) {{}}
      }}
    }};
    // Node host surface + identity channels — removed on every non-node platform.
    [
      "process", "Buffer", "require", "module", "exports",
      "__dirname", "__filename", "global",
      "_processConfig", "__agentOSProcessConfigEnv", "__agentOSVirtualOs",
    ].forEach(__dropGlobal);
    if (__jsPlatform === "browser") {{
      // Narrow `crypto` from the full node:crypto module to the WebCrypto object
      // (drops randomBytes/createHash/... while keeping subtle/getRandomValues).
      try {{
        const __wc = globalThis.crypto && globalThis.crypto.webcrypto;
        if (__wc) {{ globalThis.crypto = __wc; }}
      }} catch (_e) {{}}
    }}
    if (__jsPlatform === "neutral" || __jsPlatform === "bare") {{
      // Web-platform globals removed at neutral and below.
      [
        "fetch", "Headers", "Request", "Response", "FormData",
        "URL", "URLSearchParams", "Blob", "File", "crypto",
        "atob", "btoa", "structuredClone", "performance",
        "AbortController", "AbortSignal", "Event", "EventTarget",
        "MessageChannel", "MessagePort", "MessageEvent",
        "ReadableStream", "WritableStream", "TransformStream",
      ].forEach(__dropGlobal);
    }}
    if (__jsPlatform === "bare") {{
      // Universal host primitives removed only at the language-only tier.
      [
        "console", "queueMicrotask",
        "setTimeout", "clearTimeout", "setInterval", "clearInterval",
        "setImmediate", "clearImmediate",
      ].forEach(__dropGlobal);
    }}
  }}
}})();
{user_code}"#
    )
}

/// Spawn a V8 event bridge thread that converts V8 BinaryFrame messages
/// into JavascriptExecutionEvent for the sidecar event loop.
///
/// Internal bridge calls (module loading, logging, timers) are handled locally
/// by the event bridge. Kernel operations (fs, net, child_process, dns) are
/// forwarded to the sidecar via SyncRpcRequest events.
fn spawn_v8_event_bridge(
    frame_receiver: TrackedReceiver<BinaryFrame>,
    pending_sync_rpc: Arc<Mutex<Option<PendingSyncRpcState>>>,
    _sync_rpc_timeout: Duration,
    v8_session: V8SessionHandle,
    mut local_bridge: LocalBridgeState,
    event_notify: Option<Arc<Notify>>,
) -> TokioReceiver<JavascriptExecutionEvent> {
    let (sender, receiver) = channel(JAVASCRIPT_EVENT_CHANNEL_CAPACITY);
    let event_gauge = register_queue(
        TrackedLimit::JavascriptEventChannel,
        JAVASCRIPT_EVENT_CHANNEL_CAPACITY,
    );

    thread::spawn(move || {
        let mut emitted_exit = false;
        loop {
            let frame_recv_start = Instant::now();
            let Ok(frame) = frame_receiver.recv() else {
                break;
            };
            let frame_recv_wait = frame_recv_start.elapsed();
            let mut exit_frame_start = None;
            let event = match frame {
                BinaryFrame::BridgeCall {
                    call_id,
                    method,
                    payload,
                    ..
                } => {
                    // Convert CBOR payload to JSON args
                    let phase_start = Instant::now();
                    let args = v8_runtime::cbor_payload_to_json_args(&payload).unwrap_or_default();
                    record_sync_bridge_phase(&method, "event_decode_args", phase_start.elapsed());

                    // Module resolution / loading must read the mounted
                    // `node_modules` VFS, not host files directly. When the
                    // sidecar supplied a read-only VFS module reader, resolve
                    // these inline on this bridge thread (off the service loop) so
                    // a large cold-start module graph runs concurrently with — and
                    // never serializes behind / starves — the ACP bootstrap that
                    // is itself awaiting the adapter's `session/new` response on
                    // the single service-loop thread. Without a reader (no mount),
                    // they flow to the service loop as SyncRpcRequests (mapped to
                    // `__resolve_module` / `__load_file` / `__module_format` /
                    // `__batch_resolve_modules`) and resolve against `vm.kernel`.
                    let is_module_method = matches!(
                        method.as_str(),
                        "_resolveModule"
                            | "_resolveModuleSync"
                            | "_loadFile"
                            | "_loadFileSync"
                            | "_moduleFormat"
                            | "_batchResolveModules"
                    );
                    let resolve_on_service_loop =
                        is_module_method && !local_bridge.has_module_reader();

                    // Check if this is an internal bridge call we handle locally
                    if !resolve_on_service_loop {
                        if let Some(response) =
                            local_bridge.handle_internal_bridge_call(call_id, &method, &args)
                        {
                            if let LocalBridgeCallResult::Immediate(response) = response {
                                let cbor_payload =
                                    v8_runtime::json_to_cbor_payload(&response).unwrap_or_default();
                                let _ = v8_session.send_bridge_response(call_id, 0, cbor_payload);
                            }
                            continue;
                        }
                    }

                    // Handle logging locally (produce stdout/stderr events)
                    if method == "_log" || method == "_error" {
                        let output = decode_bridge_output_args(&args);
                        // Respond to the bridge call
                        let _ = v8_session.send_bridge_response(
                            call_id,
                            0,
                            v8_runtime::json_to_cbor_payload(&Value::Null).unwrap_or_default(),
                        );
                        if method == "_log" {
                            if !send_javascript_event(
                                &sender,
                                &v8_session,
                                &event_gauge,
                                event_notify.as_deref(),
                                JavascriptExecutionEvent::Stdout(output),
                            ) {
                                break;
                            }
                        } else {
                            if !send_javascript_event(
                                &sender,
                                &v8_session,
                                &event_gauge,
                                event_notify.as_deref(),
                                JavascriptExecutionEvent::Stderr(output),
                            ) {
                                break;
                            }
                        }
                        continue;
                    }

                    // Map the bridge method name to the sidecar sync RPC method name
                    let phase_start = Instant::now();
                    let (sidecar_method, _needs_translation) =
                        v8_runtime::map_bridge_method(&method);
                    record_sync_bridge_phase(&method, "event_map_method", phase_start.elapsed());

                    // Track pending sync RPC
                    let phase_start = Instant::now();
                    if let Ok(mut pending) = pending_sync_rpc.lock() {
                        *pending = Some(PendingSyncRpcState::Pending(call_id));
                    }
                    record_sync_bridge_phase(&method, "event_mark_pending", phase_start.elapsed());

                    let phase_start = Instant::now();
                    let request_args = translate_request_args_for_legacy(sidecar_method, &args);
                    let mut raw_bytes_args = HashMap::new();
                    if sidecar_method == "net.write"
                        || sidecar_method == "fs.writeSync"
                        || sidecar_method == "fs.writevSync"
                        || sidecar_method == "fs.writeFileSync"
                    {
                        if let Ok(Some(bytes)) = v8_runtime::cbor_payload_raw_byte_arg(&payload, 1)
                        {
                            raw_bytes_args.insert(1, bytes);
                        }
                    }
                    if method == "_fsReadRaw" {
                        raw_bytes_args.insert(usize::MAX, Vec::new());
                    }
                    record_sync_bridge_phase(
                        &method,
                        "event_translate_args",
                        phase_start.elapsed(),
                    );
                    Some(JavascriptExecutionEvent::SyncRpcRequest(
                        JavascriptSyncRpcRequest {
                            id: call_id,
                            method: sidecar_method.to_owned(),
                            args: request_args,
                            raw_bytes_args,
                        },
                    ))
                }
                BinaryFrame::Log {
                    channel, message, ..
                } => {
                    if channel == 0 {
                        Some(JavascriptExecutionEvent::Stdout(message.into_bytes()))
                    } else {
                        Some(JavascriptExecutionEvent::Stderr(message.into_bytes()))
                    }
                }
                BinaryFrame::ExecutionResult {
                    exit_code, error, ..
                } => {
                    let phase_start = Instant::now();
                    exit_frame_start = Some(phase_start);
                    record_js_event_phase("js_exit_frame_recv_wait", frame_recv_wait);
                    let is_process_exit_error = error.as_ref().is_some_and(|err| {
                        err.error_type == "ProcessExitError"
                            || err.message.starts_with("process.exit(")
                    });
                    let resolved_exit_code = error
                        .as_ref()
                        .and_then(|err| {
                            if is_process_exit_error {
                                parse_process_exit_code_message(&err.message)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(exit_code);
                    let should_emit_error = error.is_some() && !is_process_exit_error;
                    if should_emit_error {
                        let err = error.as_ref().expect("checked above");
                        let error_msg = if err.stack.is_empty() {
                            format!("{}: {}\n", err.error_type, err.message)
                        } else {
                            format!("{}\n", err.stack)
                        };
                        if !send_javascript_event(
                            &sender,
                            &v8_session,
                            &event_gauge,
                            event_notify.as_deref(),
                            JavascriptExecutionEvent::Stderr(error_msg.into_bytes()),
                        ) {
                            break;
                        }
                    }
                    emitted_exit = true;
                    record_js_event_phase(
                        "js_exit_frame_to_event_construct",
                        phase_start.elapsed(),
                    );
                    Some(JavascriptExecutionEvent::Exited(resolved_exit_code))
                }
                BinaryFrame::StreamCallback { .. } => None,
                _ => None,
            };

            if let Some(event) = event {
                let sync_rpc = match &event {
                    JavascriptExecutionEvent::SyncRpcRequest(request) => {
                        Some((request.id, request.method.clone()))
                    }
                    _ => None,
                };
                if let Some((call_id, method)) = sync_rpc.as_ref() {
                    record_sync_bridge_request_enqueued(*call_id, method);
                }
                let phase_start = sync_rpc.as_ref().map(|_| Instant::now());
                let exit_send_start = if matches!(&event, JavascriptExecutionEvent::Exited(_)) {
                    Some(Instant::now())
                } else {
                    None
                };
                if !send_javascript_event(
                    &sender,
                    &v8_session,
                    &event_gauge,
                    event_notify.as_deref(),
                    event,
                ) {
                    break;
                }
                if let (Some((_, method)), Some(start)) = (sync_rpc, phase_start) {
                    record_sync_bridge_phase(&method, "event_enqueue", start.elapsed());
                }
                if let Some(start) = exit_send_start {
                    record_js_event_phase("js_exit_event_send", start.elapsed());
                }
                if let Some(start) = exit_frame_start {
                    record_js_event_phase("js_exit_frame_to_event_sent", start.elapsed());
                }
            }
        }

        if !emitted_exit {
            let phase_start = Instant::now();
            let sent = send_javascript_event(
                &sender,
                &v8_session,
                &event_gauge,
                event_notify.as_deref(),
                JavascriptExecutionEvent::Exited(1),
            );
            if sent {
                record_js_event_phase("js_exit_fallback_event_send", phase_start.elapsed());
            }
        }
    });

    receiver
}

fn send_javascript_event(
    sender: &tokio::sync::mpsc::Sender<JavascriptExecutionEvent>,
    v8_session: &V8SessionHandle,
    gauge: &agentos_bridge::queue_tracker::QueueGauge,
    notify: Option<&Notify>,
    event: JavascriptExecutionEvent,
) -> bool {
    if javascript_event_payload_len(&event) > JAVASCRIPT_EVENT_PAYLOAD_LIMIT_BYTES {
        let _ = v8_session.destroy();
        return false;
    }

    // Apply backpressure instead of self-destructing when the host consumer is
    // slow. A burst of guest events that briefly outpaces the host draining this
    // channel is normal; previously a single `try_send` returning `Full` tore the
    // whole session down (`destroy()` -> Shutdown -> `Exited(1)`), turning a
    // transient backlog into a fatal crash. This bridge runs on its own dedicated
    // OS thread (never a Tokio runtime worker), so a blocking send is safe: it
    // parks this thread — and, in turn, the guest producing into it — until the
    // host drains capacity. The only terminal condition is the receiver being
    // dropped (host gone for good), which surfaces as `Closed` and stops the loop.
    match sender.blocking_send(event) {
        Ok(()) => {
            // Sample the live channel depth so the centralized queue tracker can
            // warn before the host consumer falls far enough behind to stall the
            // session (and surface the high-water mark for debugging).
            gauge.observe_depth(sender.max_capacity().saturating_sub(sender.capacity()));
            if let Some(notify) = notify {
                notify.notify_one();
            }
            true
        }
        Err(_closed) => false,
    }
}

fn javascript_event_payload_len(event: &JavascriptExecutionEvent) -> usize {
    match event {
        JavascriptExecutionEvent::Stdout(chunk) | JavascriptExecutionEvent::Stderr(chunk) => {
            chunk.len()
        }
        JavascriptExecutionEvent::SyncRpcRequest(_)
        | JavascriptExecutionEvent::SignalState { .. }
        | JavascriptExecutionEvent::Exited(_) => 0,
    }
}

/// Handle internal bridge calls that don't need to go to the sidecar.
/// Returns Some(response) if handled locally, None if it should be forwarded.
impl LocalBridgeState {
    fn handle_internal_bridge_call(
        &mut self,
        call_id: u64,
        method: &str,
        args: &[Value],
    ) -> Option<LocalBridgeCallResult> {
        match method {
            "_resolveModule" | "_resolveModuleSync" => {
                let specifier = args.first().and_then(Value::as_str).unwrap_or("");
                let parent = args.get(1).and_then(Value::as_str).unwrap_or("/");
                let mode = match args.get(2).and_then(Value::as_str) {
                    Some("import") => ModuleResolveMode::Import,
                    Some("require") => ModuleResolveMode::Require,
                    _ if method == "_resolveModule" => ModuleResolveMode::Import,
                    _ => ModuleResolveMode::Require,
                };
                if self.js_runtime_denies_specifier(specifier) {
                    return Some(LocalBridgeCallResult::Immediate(Value::Null));
                }
                let resolved = self.with_module_resolver(|resolver| {
                    resolver.resolve_module(specifier, parent, mode)
                });
                if resolved.is_none() && self.has_module_reader() {
                    return None;
                }
                Some(LocalBridgeCallResult::Immediate(
                    resolved.map(Value::String).unwrap_or(Value::Null),
                ))
            }
            "_moduleFormat" => {
                let format = self.module_format(args.first().and_then(Value::as_str).unwrap_or(""));
                if format.is_none() && self.has_module_reader() {
                    return None;
                }
                Some(LocalBridgeCallResult::Immediate(
                    format
                        .map(|format| Value::String(String::from(format.as_str())))
                        .unwrap_or(Value::Null),
                ))
            }
            "_loadFile" | "_loadFileSync" => {
                let source = self.load_file(args.first().and_then(Value::as_str).unwrap_or(""));
                if source.is_none() && self.has_module_reader() {
                    return None;
                }
                Some(LocalBridgeCallResult::Immediate(
                    source.map(Value::String).unwrap_or(Value::Null),
                ))
            }
            "_batchResolveModules" => {
                let resolved = self.batch_resolve_modules(args);
                if self.has_module_reader()
                    && resolved
                        .as_array()
                        .is_some_and(|items| items.iter().any(Value::is_null))
                {
                    return None;
                }
                Some(LocalBridgeCallResult::Immediate(resolved))
            }
            "_loadPolyfill" => Some(LocalBridgeCallResult::Immediate(
                self.handle_polyfill_dispatch(args),
            )),
            "_cryptoRandomFill" => {
                let size = args.first().and_then(Value::as_u64).unwrap_or(16) as usize;
                let mut bytes = vec![0u8; size];
                if getrandom(&mut bytes).is_err() {
                    return Some(LocalBridgeCallResult::Immediate(Value::Null));
                }
                Some(LocalBridgeCallResult::Immediate(Value::String(
                    v8_runtime::base64_encode_pub(&bytes),
                )))
            }
            "_cryptoRandomUUID" => {
                let mut bytes = [0u8; 16];
                if getrandom(&mut bytes).is_err() {
                    return Some(LocalBridgeCallResult::Immediate(Value::Null));
                }
                bytes[6] = (bytes[6] & 0x0f) | 0x40;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;

                Some(LocalBridgeCallResult::Immediate(Value::String(format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    bytes[0],
                    bytes[1],
                    bytes[2],
                    bytes[3],
                    bytes[4],
                    bytes[5],
                    bytes[6],
                    bytes[7],
                    bytes[8],
                    bytes[9],
                    bytes[10],
                    bytes[11],
                    bytes[12],
                    bytes[13],
                    bytes[14],
                    bytes[15],
                ))))
            }
            "_kernelStdinRead" | "_kernelStdinReadRaw" if self.forward_kernel_stdin_rpc => None,
            "_kernelStdinRead" | "_kernelStdinReadRaw" => Some(LocalBridgeCallResult::Immediate(
                self.kernel_stdin.read(args),
            )),
            "_pythonStdinRead" => Some(LocalBridgeCallResult::Immediate(
                self.kernel_stdin.read_python_raw(args),
            )),
            "_scheduleTimer" => {
                self.schedule_bridge_timer_response(call_id, timer_delay_ms(args.first()));
                Some(LocalBridgeCallResult::Deferred)
            }
            _ => None,
        }
    }

    fn handle_polyfill_dispatch(&mut self, args: &[Value]) -> Value {
        let Some(dispatch) = args.first().and_then(Value::as_str) else {
            return Value::Null;
        };
        if !dispatch.starts_with("__bd:") {
            return polyfill_expression(dispatch)
                .map(Value::String)
                .unwrap_or(Value::Null);
        }
        let (dispatch_method, payload_json) = dispatch
            .strip_prefix("__bd:")
            .and_then(|value| value.split_once(':'))
            .unwrap_or(("", "[]"));
        let payload = serde_json::from_str::<Value>(payload_json).unwrap_or_else(|_| json!([]));
        let args = payload.as_array().cloned().unwrap_or_default();
        let result = match dispatch_method {
            "kernelHandleRegister" => {
                if let (Some(id), Some(description)) = (
                    args.first().and_then(Value::as_str),
                    args.get(1).and_then(Value::as_str),
                ) {
                    self.handle_descriptions
                        .insert(id.to_owned(), description.to_owned());
                }
                Value::Null
            }
            "kernelHandleUnregister" => {
                if let Some(id) = args.first().and_then(Value::as_str) {
                    self.handle_descriptions.remove(id);
                }
                Value::Null
            }
            "kernelHandleList" => Value::Array(
                self.handle_descriptions
                    .iter()
                    .map(|(id, description)| {
                        json!({
                            "id": id,
                            "description": description,
                        })
                    })
                    .collect(),
            ),
            "kernelTimerCreate" => {
                let delay_ms = timer_delay_ms(args.first());
                let repeat = args.get(1).and_then(Value::as_bool).unwrap_or(false);
                json!(self.create_kernel_timer(delay_ms, repeat))
            }
            "kernelTimerArm" => {
                if let Some(timer_id) = args.first().and_then(Value::as_u64) {
                    self.arm_kernel_timer(timer_id);
                }
                Value::Null
            }
            "kernelTimerClear" => {
                if let Some(timer_id) = args.first().and_then(Value::as_u64) {
                    self.clear_kernel_timer(timer_id);
                }
                Value::Null
            }
            _ => json!({
                "__bd_error": {
                    "name": "Error",
                    "message": format!("No handler: {dispatch_method}"),
                }
            }),
        };

        if dispatch_method.starts_with("kernel") {
            Value::String(
                serde_json::to_string(&json!({ "__bd_result": result }))
                    .unwrap_or_else(|_| String::from("{\"__bd_result\":null}")),
            )
        } else {
            Value::String(
                serde_json::to_string(&json!({
                    "__bd_error": {
                        "name": "Error",
                        "message": format!("No handler: {dispatch_method}"),
                    }
                }))
                .unwrap_or_else(|_| {
                    String::from(
                        "{\"__bd_error\":{\"name\":\"Error\",\"message\":\"dispatch failed\"}}",
                    )
                }),
            )
        }
    }

    fn create_kernel_timer(&mut self, delay_ms: u64, repeat: bool) -> u64 {
        self.next_timer_id += 1;
        if let Ok(mut timers) = self.timers.lock() {
            timers.insert(
                self.next_timer_id,
                LocalTimerEntry {
                    delay_ms,
                    generation: 0,
                    repeat,
                },
            );
        }
        self.next_timer_id
    }

    /// Allocate a fresh timer id and register a one-shot (`repeat == false`)
    /// tracking entry at generation 0. Used by the bridge-timer path so the
    /// queued wheel action can be cancelled (its entry removed) on `clear`/teardown.
    fn register_oneshot_timer(&mut self, delay_ms: u64) -> u64 {
        self.next_timer_id += 1;
        let timer_id = self.next_timer_id;
        if let Ok(mut timers) = self.timers.lock() {
            timers.insert(
                timer_id,
                LocalTimerEntry {
                    delay_ms,
                    generation: 0,
                    repeat: false,
                },
            );
        }
        timer_id
    }

    fn arm_kernel_timer(&self, timer_id: u64) {
        let Some(session) = self.v8_session.clone() else {
            return;
        };

        let Some((delay_ms, generation, timers)) =
            self.timers.lock().ok().and_then(|mut timers| {
                let entry = timers.get_mut(&timer_id)?;
                entry.generation = entry.generation.wrapping_add(1);
                Some((entry.delay_ms, entry.generation, self.timers.clone()))
            })
        else {
            return;
        };

        TimerWheel::get().schedule(
            delay_ms,
            TimerAction::StreamEvent {
                session,
                timer_id,
                generation,
                timers,
            },
        );
    }

    fn clear_kernel_timer(&self, timer_id: u64) {
        if let Ok(mut timers) = self.timers.lock() {
            timers.remove(&timer_id);
        }
    }

    fn schedule_bridge_timer_response(&mut self, call_id: u64, delay_ms: u64) {
        let Some(session) = self.v8_session.clone() else {
            return;
        };

        // Register the bridge timer in the shared `timers` map with a generation,
        // mirroring the kernel-timer cancellation path. Tracking it means that
        // when `LocalBridgeState` is dropped on session teardown (which clears the
        // map) or the entry is otherwise removed, the timer wheel observes the
        // missing/mismatched generation via `timer_should_fire` and suppresses the
        // response instead of touching the torn-down session.
        let timer_id = self.register_oneshot_timer(delay_ms);
        let generation = 0;
        let timers = self.timers.clone();

        TimerWheel::get().schedule(
            delay_ms,
            TimerAction::BridgeResponse {
                session,
                call_id,
                timer_id,
                generation,
                timers,
            },
        );
    }

    fn has_module_reader(&self) -> bool {
        self.module_reader.is_some()
    }

    fn batch_resolve_modules(&mut self, args: &[Value]) -> Value {
        self.with_module_resolver(|resolver| resolver.batch_resolve_modules(args))
    }

    fn resolve_module(
        &mut self,
        specifier: &str,
        from_dir: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        if self.js_runtime_denies_specifier(specifier) {
            if std::env::var("AGENTOS_MODULE_READER_TRACE").is_ok() {
                eprintln!("resolve DENIED: {specifier} from {from_dir}");
            }
            return None;
        }
        let resolved =
            self.with_module_resolver(|resolver| resolver.resolve_module(specifier, from_dir, mode));
        if resolved.is_none() && std::env::var("AGENTOS_MODULE_READER_TRACE").is_ok() {
            eprintln!("resolve MISS: {specifier} from {from_dir} mode={mode:?}");
        }
        resolved
    }

    /// jsRuntime resolution gate. Denies builtin and bare/relative imports per the
    /// configured `moduleResolution` and builtin allow-list, before the resolver
    /// touches the VFS. This is the authoritative chokepoint for the live
    /// shared-V8 path (both `import`/`import()` and `require`/`createRequire`
    /// route through `_resolveModule` -> here).
    fn js_runtime_denies_specifier(&self, specifier: &str) -> bool {
        let is_local = specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier == "."
            || specifier == ".."
            || specifier.starts_with('/')
            || specifier.starts_with("file:");
        match self.module_resolution {
            GuestModuleResolution::Node => false,
            // Relative permits local files only; bare specifiers and package
            // imports (`#...`) do not resolve.
            GuestModuleResolution::Relative => !is_local,
            // None denies every specifier, local included.
            GuestModuleResolution::None => true,
        }
    }

    fn module_format(&mut self, path: &str) -> Option<LocalResolvedModuleFormat> {
        self.with_module_resolver(|resolver| resolver.module_format(path))
    }

    fn load_file(&mut self, path: &str) -> Option<String> {
        self.with_module_resolver(|resolver| resolver.load_file(path))
    }

    /// Run `f` against a resolver bound to this bridge's resolution cache, reading
    /// through the supplied VFS `module_reader` when present (the live VM path:
    /// resolution executes here on the bridge thread, reading the mounted
    /// `node_modules` filesystem in parallel with the service loop), or through
    /// the host-backed path translator otherwise (the legacy host-direct path
    /// used by `handle_internal_bridge_call_from_host_context`).
    fn with_module_resolver<T>(
        &mut self,
        f: impl FnOnce(&mut ModuleResolver<'_, &mut dyn ModuleFsReader>) -> T,
    ) -> T {
        let cache = &mut self.resolution_cache;
        if let Some(reader) = self.module_reader.as_deref_mut() {
            let reader: &mut dyn ModuleFsReader = reader;
            let mut resolver = ModuleResolver { reader, cache };
            f(&mut resolver)
        } else {
            let mut translator = &mut self.translator;
            let reader: &mut dyn ModuleFsReader = &mut translator;
            let mut resolver = ModuleResolver { reader, cache };
            f(&mut resolver)
        }
    }
}

impl ModuleFsReader for &mut dyn ModuleFsReader {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        (**self).canonical_guest_path(guest_path)
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        (**self).read_to_string(guest_path)
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        (**self).path_is_dir(guest_path)
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        (**self).path_exists(guest_path)
    }
}

impl ModuleFsReader for &mut GuestPathTranslator {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        GuestPathTranslator::canonical_guest_path(self, guest_path)
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        let host_path = self.guest_to_host(guest_path)?;
        fs::read_to_string(host_path).ok()
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        self.guest_to_host(guest_path)
            .and_then(|host_path| fs::metadata(host_path).ok())
            .map(|metadata| metadata.is_dir())
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        self.guest_to_host(guest_path)
            .map(|host_path| host_path.exists())
            .unwrap_or(false)
    }
}

/// Standard Node module resolution executed as pure path algebra over a
/// [`ModuleFsReader`]. The same algorithm backs both the legacy host-direct
/// path (reader = host path translator) and the live VM path (reader = kernel
/// VFS), guaranteeing they resolve identically.
pub struct ModuleResolver<'a, R: ModuleFsReader> {
    reader: R,
    cache: &'a mut LocalModuleResolutionCache,
}

impl<'a, R: ModuleFsReader> ModuleResolver<'a, R> {
    /// Construct a resolver over `reader`, reusing `cache` across calls. The
    /// cache must be persisted per-VM so cold-start resolution does not rebuild
    /// it on every dispatch.
    pub fn new(reader: R, cache: &'a mut LocalModuleResolutionCache) -> Self {
        Self { reader, cache }
    }

    pub fn batch_resolve_modules(&mut self, args: &[Value]) -> Value {
        let requests = args
            .first()
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Value::Array(
            requests
                .into_iter()
                .map(|request| {
                    let pair = request.as_array().cloned().unwrap_or_default();
                    let specifier = pair.first().and_then(Value::as_str).unwrap_or("");
                    let referrer = pair.get(1).and_then(Value::as_str).unwrap_or("/");
                    self.resolve_module(specifier, referrer, ModuleResolveMode::Import)
                        .and_then(|resolved| {
                            self.load_file(&resolved).map(|source| {
                                json!({
                                    "resolved": resolved,
                                    "source": source,
                                })
                            })
                        })
                        .unwrap_or(Value::Null)
                })
                .collect(),
        )
    }

    pub fn resolve_module(
        &mut self,
        specifier: &str,
        from_dir: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        let normalized_from_path = self
            .reader
            .canonical_guest_path(from_dir)
            .unwrap_or_else(|| normalize_guest_path(from_dir));
        let normalized_from = if self.cached_stat(&normalized_from_path) == Some(false) {
            dirname_guest_path(&normalized_from_path)
        } else {
            normalize_module_resolve_context(&normalized_from_path)
        };
        let cache_key = (specifier.to_owned(), normalized_from.clone(), mode);
        if let Some(cached) = self.cache.resolve_results.get(&cache_key) {
            return cached.clone();
        }

        let resolved = if let Some(builtin) = normalize_builtin_specifier(specifier) {
            Some(builtin)
        } else if specifier.starts_with("file:") {
            guest_path_from_file_url(specifier)
                .and_then(|file_path| self.resolve_path(&file_path, mode))
        } else if specifier.starts_with('/') {
            self.resolve_path(specifier, mode)
        } else if specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier == "."
            || specifier == ".."
        {
            self.resolve_path(&join_guest_path(&normalized_from, specifier), mode)
        } else if specifier.starts_with('#') {
            self.resolve_package_imports(specifier, &normalized_from, mode)
        } else {
            self.resolve_package_self_reference(specifier, &normalized_from, mode)
                .or_else(|| self.resolve_node_modules(specifier, &normalized_from, mode))
        };

        if resolved.is_some() || module_resolution_miss_is_stable(&normalized_from) {
            self.cache
                .resolve_results
                .insert(cache_key, resolved.clone());
        }
        resolved
    }

    pub fn load_file(&mut self, path: &str) -> Option<String> {
        let bare = path.trim_start_matches("node:");
        if is_builtin_specifier(path) {
            return Some(build_builtin_module_wrapper(bare));
        }

        let source = self.reader.read_to_string(path)?;
        Some(
            if matches!(
                Path::new(path).extension().and_then(|ext| ext.to_str()),
                Some("js" | "mjs" | "cjs")
            ) {
                strip_javascript_hashbang(&source)
            } else {
                source
            },
        )
    }

    pub fn module_format(&mut self, path: &str) -> Option<LocalResolvedModuleFormat> {
        if let Some(cached) = self.cache.module_format_results.get(path) {
            return *cached;
        }

        let format = self.detect_module_format(path);
        self.cache
            .module_format_results
            .insert(path.to_owned(), format);
        format
    }

    fn detect_module_format(&mut self, path: &str) -> Option<LocalResolvedModuleFormat> {
        if is_builtin_specifier(path) {
            return Some(LocalResolvedModuleFormat::Module);
        }

        let normalized = normalize_guest_path(path);
        match Path::new(&normalized)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some("mjs" | "mts") => Some(LocalResolvedModuleFormat::Module),
            Some("cjs" | "cts") => Some(LocalResolvedModuleFormat::Commonjs),
            Some("json") => Some(LocalResolvedModuleFormat::Json),
            Some("js") => Some(
                if self
                    .nearest_package_json_type_for_guest_path(&normalized)
                    .as_deref()
                    == Some("module")
                {
                    LocalResolvedModuleFormat::Module
                } else {
                    LocalResolvedModuleFormat::Commonjs
                },
            ),
            _ => None,
        }
    }

    fn nearest_package_json_type_for_guest_path(&mut self, guest_path: &str) -> Option<String> {
        let mut dir = dirname_guest_path(guest_path);
        loop {
            let package_json_path = join_guest_path(&dir, "package.json");
            if let Some(package_json) = self.read_package_json(&package_json_path) {
                return package_json.package_type;
            }
            if dir == "/" {
                break;
            }
            dir = dirname_guest_path(&dir);
        }
        None
    }

    fn resolve_package_imports(
        &mut self,
        request: &str,
        from_dir: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        let mut dir = normalize_guest_path(from_dir);
        loop {
            let pkg_json_path = join_guest_path(&dir, "package.json");
            if let Some(pkg_json) = self.read_package_json(&pkg_json_path) {
                if let Some(imports) = &pkg_json.imports {
                    if let Some(target) = resolve_imports_target(imports, request, mode) {
                        let target_path = if target.starts_with('/') {
                            target
                        } else {
                            join_guest_path(&dir, &target)
                        };
                        return self.resolve_path(&target_path, mode);
                    }
                    return None;
                }
            }
            if dir == "/" {
                break;
            }
            dir = dirname_guest_path(&dir);
        }
        None
    }

    fn resolve_package_self_reference(
        &mut self,
        request: &str,
        from_dir: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        let (package_name, subpath) = split_package_request(request)?;
        let mut dir = normalize_guest_path(from_dir);
        loop {
            let pkg_json_path = join_guest_path(&dir, "package.json");
            if let Some(pkg_json) = self.read_package_json(&pkg_json_path) {
                if pkg_json.name.as_deref() == Some(package_name) {
                    return self.resolve_package_entry_from_dir(&dir, subpath, mode);
                }
            }
            if dir == "/" {
                break;
            }
            dir = dirname_guest_path(&dir);
        }
        None
    }

    fn resolve_node_modules(
        &mut self,
        request: &str,
        from_dir: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        let (package_name, subpath) = split_package_request(request)?;

        // Standard Node resolution over the faithful VFS: walk ancestor
        // `node_modules` directories (following symlinks via the importer's
        // realpath). pnpm/yarn layouts resolve because the VFS exposes their
        // symlinks, not because the resolver understands package-manager
        // internals (see CLAUDE.md npm Compatibility).
        let mut dir = normalize_guest_path(from_dir);
        loop {
            for package_dir in node_modules_direct_candidate_dirs(&dir, package_name) {
                if let Some(entry) =
                    self.resolve_package_entry_from_dir(&package_dir, subpath, mode)
                {
                    return Some(entry);
                }
            }
            if dir == "/" {
                break;
            }
            dir = dirname_guest_path(&dir);
        }

        ["/root/node_modules", "/node_modules"]
            .into_iter()
            .find_map(|root| {
                self.resolve_package_entry_from_dir(
                    &join_guest_path(root, package_name),
                    subpath,
                    mode,
                )
            })
    }

    fn resolve_package_entry_from_dir(
        &mut self,
        package_dir: &str,
        subpath: &str,
        mode: ModuleResolveMode,
    ) -> Option<String> {
        let package_json_path = join_guest_path(package_dir, "package.json");
        let pkg_json = self.read_package_json(&package_json_path);
        if pkg_json.is_none() && !self.cached_exists(package_dir) {
            return None;
        }

        if let Some(pkg_json) = pkg_json.as_ref() {
            if let Some(exports) = &pkg_json.exports {
                let exports_subpath = if subpath.is_empty() {
                    String::from(".")
                } else {
                    format!("./{subpath}")
                };
                let exports_target = resolve_exports_target(exports, &exports_subpath, mode)?;
                let target_path = join_guest_path(package_dir, &exports_target);
                return self.resolve_path(&target_path, mode).or(Some(target_path));
            }
        }

        if !subpath.is_empty() {
            return self.resolve_path(&join_guest_path(package_dir, subpath), mode);
        }

        let entry_field = pkg_json
            .as_ref()
            .and_then(|pkg_json| pkg_json.main.as_deref())
            .unwrap_or("index.js");
        let entry_path = join_guest_path(package_dir, entry_field);
        self.resolve_path(&entry_path, mode)
            .or_else(|| self.resolve_path(&join_guest_path(package_dir, "index"), mode))
    }

    fn resolve_path(&mut self, base_path: &str, mode: ModuleResolveMode) -> Option<String> {
        if self.cached_stat(base_path) == Some(false) {
            return Some(normalize_guest_path(base_path));
        }

        for extension in [".js", ".json", ".mjs", ".cjs"] {
            let candidate = format!("{}{}", normalize_guest_path(base_path), extension);
            if self.cached_exists(&candidate) {
                return Some(candidate);
            }
        }

        if self.cached_stat(base_path) == Some(true) {
            let pkg_json_path = join_guest_path(base_path, "package.json");
            if let Some(pkg_json) = self.read_package_json(&pkg_json_path) {
                if let Some(main) = pkg_json.main.as_deref() {
                    let entry_path = join_guest_path(base_path, main);
                    if entry_path != normalize_guest_path(base_path) {
                        if let Some(entry) = self.resolve_path(&entry_path, mode) {
                            return Some(entry);
                        }
                    }
                }
                if mode == ModuleResolveMode::Import
                    && pkg_json.package_type.as_deref() == Some("module")
                    && self.cached_exists(&join_guest_path(base_path, "index.js"))
                {
                    return Some(join_guest_path(base_path, "index.js"));
                }
            }

            for extension in [".js", ".json", ".mjs", ".cjs"] {
                let index_path = join_guest_path(base_path, &format!("index{extension}"));
                if self.cached_exists(&index_path) {
                    return Some(index_path);
                }
            }
        }

        None
    }

    fn read_package_json(&mut self, guest_path: &str) -> Option<LocalPackageJson> {
        if let Some(cached) = self.cache.package_json_results.get(guest_path).cloned() {
            return cached;
        }

        let parsed = self
            .reader
            .read_to_string(guest_path)
            .and_then(|contents| serde_json::from_str::<LocalPackageJson>(&contents).ok());
        if parsed.is_some() || module_path_miss_is_stable(guest_path) {
            self.cache
                .package_json_results
                .insert(guest_path.to_owned(), parsed.clone());
        }
        parsed
    }

    fn cached_exists(&mut self, guest_path: &str) -> bool {
        if let Some(cached) = self.cache.exists_results.get(guest_path) {
            return *cached;
        }
        let exists = self.reader.path_exists(guest_path);
        if exists || module_path_miss_is_stable(guest_path) {
            self.cache
                .exists_results
                .insert(guest_path.to_owned(), exists);
        }
        exists
    }

    fn cached_stat(&mut self, guest_path: &str) -> Option<bool> {
        if let Some(cached) = self.cache.stat_results.get(guest_path) {
            return *cached;
        }
        let result = self.reader.path_is_dir(guest_path);
        if result.is_some() || module_path_miss_is_stable(guest_path) {
            self.cache
                .stat_results
                .insert(guest_path.to_owned(), result);
        }
        result
    }
}

fn module_resolution_miss_is_stable(from_dir: &str) -> bool {
    module_path_miss_is_stable(from_dir)
}

fn module_path_miss_is_stable(guest_path: &str) -> bool {
    guest_path == "/node_modules"
        || guest_path.ends_with("/node_modules")
        || guest_path.contains("/node_modules/")
}

fn guest_path_from_file_url(specifier: &str) -> Option<String> {
    if !specifier.starts_with("file:") {
        return None;
    }

    let mut pathname = if let Some(stripped) = specifier.strip_prefix("file://") {
        stripped
    } else {
        specifier.strip_prefix("file:")?
    };

    if let Some(terminator_index) = pathname.find(['?', '#']) {
        pathname = &pathname[..terminator_index];
    }

    if !pathname.starts_with('/') {
        let slash_index = pathname.find('/')?;
        let host = &pathname[..slash_index];
        if !host.is_empty() && host != "localhost" {
            return None;
        }
        pathname = &pathname[slash_index..];
    }

    Some(normalize_guest_path(&percent_decode(pathname)?))
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut index = 0;
    let mut decoded = Vec::with_capacity(bytes.len());
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
                {
                    let value = (high << 4) | low;
                    decoded.push(value);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl LocalKernelStdinBridge {
    fn write(&self, chunk: &[u8]) -> Result<(), JavascriptExecutionError> {
        let mut state = self.state.lock().expect("kernel stdin state poisoned");
        if state.closed {
            return Err(JavascriptExecutionError::StdinClosed);
        }
        let next_len = state.bytes.len().checked_add(chunk.len()).ok_or_else(|| {
            JavascriptExecutionError::Stdin(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("guest stdin buffer exceeded {KERNEL_STDIN_BUFFER_LIMIT_BYTES} bytes"),
            ))
        })?;
        if next_len > KERNEL_STDIN_BUFFER_LIMIT_BYTES {
            return Err(JavascriptExecutionError::Stdin(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("guest stdin buffer exceeded {KERNEL_STDIN_BUFFER_LIMIT_BYTES} bytes"),
            )));
        }

        state.bytes.extend(chunk.iter().copied());
        self.ready.notify_all();
        Ok(())
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("kernel stdin state poisoned");
        state.closed = true;
        self.ready.notify_all();
    }

    fn read(&self, args: &[Value]) -> Value {
        let max_bytes = args
            .first()
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, 64 * 1024) as usize)
            .unwrap_or(64 * 1024);
        let timeout = Duration::from_millis(args.get(1).and_then(Value::as_u64).unwrap_or(100));
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().expect("kernel stdin state poisoned");

        while state.bytes.is_empty() && !state.closed {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Value::Null;
            }
            let (next_state, wait_result) = self
                .ready
                .wait_timeout(state, remaining)
                .expect("kernel stdin wait poisoned");
            state = next_state;
            if wait_result.timed_out() && state.bytes.is_empty() && !state.closed {
                return Value::Null;
            }
        }

        if !state.bytes.is_empty() {
            let read_len = state.bytes.len().min(max_bytes);
            let bytes = state.bytes.drain(..read_len).collect::<Vec<_>>();
            return json!({
                "dataBase64": v8_runtime::base64_encode_pub(&bytes),
            });
        }

        json!({
            "done": true,
        })
    }

    fn read_python_raw(&self, args: &[Value]) -> Value {
        const PYTHON_STDIN_DONE_SENTINEL: &str = "__AGENTOS_PYTHON_STDIN_DONE__";

        let max_bytes = args
            .first()
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, 64 * 1024) as usize)
            .unwrap_or(64 * 1024);
        let timeout = Duration::from_millis(args.get(1).and_then(Value::as_u64).unwrap_or(100));
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().expect("kernel stdin state poisoned");

        while state.bytes.is_empty() && !state.closed {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Value::Null;
            }
            let (next_state, wait_result) = self
                .ready
                .wait_timeout(state, remaining)
                .expect("kernel stdin wait poisoned");
            state = next_state;
            if wait_result.timed_out() && state.bytes.is_empty() && !state.closed {
                return Value::Null;
            }
        }

        if !state.bytes.is_empty() {
            let read_len = state.bytes.len().min(max_bytes);
            let bytes = state.bytes.drain(..read_len).collect::<Vec<_>>();
            return Value::String(v8_runtime::base64_encode_pub(&bytes));
        }

        Value::String(String::from(PYTHON_STDIN_DONE_SENTINEL))
    }
}

fn normalize_module_resolve_context(path: &str) -> String {
    let normalized = normalize_guest_path(path);
    if normalized.ends_with(".js")
        || normalized.ends_with(".mjs")
        || normalized.ends_with(".cjs")
        || normalized.ends_with(".json")
        || normalized.ends_with(".ts")
        || normalized.ends_with(".mts")
        || normalized.ends_with(".cts")
    {
        dirname_guest_path(&normalized)
    } else {
        normalized
    }
}

fn strip_javascript_hashbang(source: &str) -> String {
    if let Some(stripped) = source.strip_prefix("#!") {
        match stripped.find('\n') {
            Some(index) => format!("\n{}", &stripped[index + 1..]),
            None => String::new(),
        }
    } else {
        source.to_owned()
    }
}

fn parse_process_exit_code_message(message: &str) -> Option<i32> {
    let code = message.strip_prefix("process.exit(")?.strip_suffix(')')?;
    code.parse::<i32>().ok()
}

fn dirname_guest_path(path: &str) -> String {
    let normalized = normalize_guest_path(path);
    if normalized == "/" {
        return normalized;
    }
    normalized
        .rsplit_once('/')
        .map(|(parent, _)| {
            if parent.is_empty() {
                String::from("/")
            } else {
                parent.to_owned()
            }
        })
        .unwrap_or_else(|| String::from("/"))
}

fn normalize_builtin_specifier(specifier: &str) -> Option<String> {
    let bare = specifier.trim_start_matches("node:");
    match bare {
        "assert"
        | "async_hooks"
        | "buffer"
        | "child_process"
        | "cluster"
        | "console"
        | "constants"
        | "crypto"
        | "dgram"
        | "diagnostics_channel"
        | "dns"
        | "dns/promises"
        | "events"
        | "fs"
        | "fs/promises"
        | "http"
        | "http2"
        | "https"
        | "inspector"
        | "module"
        | "net"
        | "os"
        | "path"
        | "path/posix"
        | "path/win32"
        | "perf_hooks"
        | "process"
        | "punycode"
        | "querystring"
        | "readline"
        | "repl"
        | "sqlite"
        | "stream"
        | "stream/consumers"
        | "stream/promises"
        | "stream/web"
        | "string_decoder"
        | "sys"
        | "timers"
        | "tls"
        | "timers/promises"
        | "trace_events"
        | "tty"
        | "url"
        | "util"
        | "util/types"
        | "domain"
        | "vm"
        | "v8"
        | "wasi"
        | "worker_threads"
        | "zlib" => Some(format!("node:{bare}")),
        _ => None,
    }
}

fn is_builtin_specifier(specifier: &str) -> bool {
    normalize_builtin_specifier(specifier).is_some()
}

fn polyfill_expression(request: &str) -> Option<String> {
    let normalized = request.trim_start_matches("node:");
    let entry = polyfill_registry()
        .groups
        .iter()
        .find(|group| group.names.iter().any(|name| name == normalized))?;

    Some(match entry.source {
        PolyfillSourceKind::NodeStdlibBrowser | PolyfillSourceKind::CustomBridge => format!(
            "globalThis._requireFrom({}, \"/\")",
            serde_json::to_string(&format!("node:{normalized}"))
                .unwrap_or_else(|_| format!("\"node:{normalized}\""))
        ),
        PolyfillSourceKind::Denied => {
            let error_code = entry.error_code.as_deref().unwrap_or("ERR_ACCESS_DENIED");
            format!(
                "(() => {{ const error = new Error({message}); error.code = {code}; throw error; }})()",
                message = serde_json::to_string(&format!(
                    "node:{normalized} is not available in the secure-exec guest runtime"
                ))
                .unwrap_or_else(|_| format!(
                    "\"node:{normalized} is not available in the secure-exec guest runtime\""
                )),
                code = serde_json::to_string(error_code)
                    .unwrap_or_else(|_| "\"ERR_ACCESS_DENIED\"".to_owned())
            )
        }
    })
}

fn build_builtin_module_wrapper(module_name: &str) -> String {
    if module_name == "assert" {
        return String::from(
            r#"class AssertionError extends Error {
  constructor(message = "Assertion failed") {
    super(message);
    this.name = "AssertionError";
  }
}

function fail(message) {
  throw new AssertionError(message);
}

function ok(value, message) {
  if (!value) fail(message);
}

function equal(actual, expected, message) {
  if (actual != expected) fail(message ?? `Expected ${actual} == ${expected}`);
}

function notEqual(actual, expected, message) {
  if (actual == expected) fail(message ?? `Expected ${actual} != ${expected}`);
}

function strictEqual(actual, expected, message) {
  if (actual !== expected) fail(message ?? `Expected ${actual} === ${expected}`);
}

function notStrictEqual(actual, expected, message) {
  if (actual === expected) fail(message ?? `Expected ${actual} !== ${expected}`);
}

function serialize(value) {
  return JSON.stringify(value);
}

function deepEqual(actual, expected, message) {
  if (serialize(actual) !== serialize(expected)) {
    fail(message ?? "Expected values to be deeply equal");
  }
}

function deepStrictEqual(actual, expected, message) {
  return deepEqual(actual, expected, message);
}

function match(actual, expected, message) {
  if (!(expected instanceof RegExp) || !expected.test(String(actual))) {
    fail(message ?? `Expected ${actual} to match ${expected}`);
  }
}

function matchesExpectedError(error, expected) {
  if (expected == null) return true;
  if (expected instanceof RegExp) {
    return expected.test(String(error?.message ?? error));
  }
  if (typeof expected === "function") {
    if (error instanceof expected) return true;
    return expected(error) === true;
  }
  if (typeof expected === "object") {
    return Object.entries(expected).every(([key, value]) => serialize(error?.[key]) === serialize(value));
  }
  return false;
}

function throws(fn, expected, message) {
  if (typeof fn !== "function") {
    fail(message ?? "assert.throws requires a function");
  }

  try {
    fn();
  } catch (error) {
    if (!matchesExpectedError(error, expected)) {
      throw error;
    }
    return error;
  }

  fail(message ?? "Missing expected exception");
}

async function rejects(promiseOrFn, expected, message) {
  let promise;
  if (typeof promiseOrFn === "function") {
    promise = promiseOrFn();
  } else {
    promise = promiseOrFn;
  }

  try {
    await promise;
  } catch (error) {
    if (!matchesExpectedError(error, expected)) {
      throw error;
    }
    return error;
  }

  fail(message ?? "Missing expected rejection");
}

function ifError(error) {
  if (error != null) {
    throw error;
  }
}

function assert(value, message) {
  ok(value, message);
}

Object.assign(assert, {
  AssertionError,
  deepEqual,
  deepStrictEqual,
  equal,
  fail,
  ifError,
  match,
  notEqual,
  notStrictEqual,
  ok,
  rejects,
  strict: assert,
  strictEqual,
  throws,
});

export {
  AssertionError,
  assert as default,
  deepEqual,
  deepStrictEqual,
  equal,
  fail,
  ifError,
  match,
  notEqual,
  notStrictEqual,
  ok,
  rejects,
  assert as strict,
  strictEqual,
  throws,
};
"#,
        );
    }

    if module_name == "path" || module_name == "path/posix" || module_name == "path/win32" {
        return String::from(
            r#"const sep = "/";
const delimiter = ":";

function normalizeSegments(parts) {
  const output = [];
  for (const part of parts) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (output.length > 0) output.pop();
      continue;
    }
    output.push(part);
  }
  return output;
}

function isAbsolute(path) {
  return String(path || "").startsWith(sep);
}

function join(...parts) {
  const absolute = parts.some((part, index) => index === 0 && isAbsolute(part));
  const normalized = normalizeSegments(parts.flatMap((part) => String(part || "").split(sep)));
  const joined = normalized.join(sep);
  if (!joined) return absolute ? sep : ".";
  return absolute ? `${sep}${joined}` : joined;
}

function dirname(path) {
  const normalized = String(path || "");
  if (!normalized || normalized === sep) return sep;
  const parts = normalizeSegments(normalized.split(sep));
  if (parts.length <= 1) return isAbsolute(normalized) ? sep : ".";
  const dir = parts.slice(0, -1).join(sep);
  return isAbsolute(normalized) ? `${sep}${dir}` : dir;
}

function basename(path) {
  const normalized = normalizeSegments(String(path || "").split(sep));
  return normalized.length === 0 ? "" : normalized[normalized.length - 1];
}

function extname(path) {
  const base = basename(path);
  const index = base.lastIndexOf(".");
  if (index <= 0) return "";
  return base.slice(index);
}

function resolve(...parts) {
  const absoluteParts = [];
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const part = String(parts[index] || "");
    if (!part) continue;
    absoluteParts.unshift(part);
    if (isAbsolute(part)) break;
  }
  if (absoluteParts.length === 0 || !isAbsolute(absoluteParts[0])) {
    absoluteParts.unshift(typeof process?.cwd === "function" ? process.cwd() : sep);
  }
  return join(...absoluteParts);
}

function relative(from, to) {
  const fromResolved = resolve(from);
  const toResolved = resolve(to);
  if (fromResolved === toResolved) return "";

  const fromParts = normalizeSegments(fromResolved.split(sep));
  const toParts = normalizeSegments(toResolved.split(sep));
  let shared = 0;
  while (
    shared < fromParts.length &&
    shared < toParts.length &&
    fromParts[shared] === toParts[shared]
  ) {
    shared += 1;
  }

  const up = new Array(fromParts.length - shared).fill("..");
  const down = toParts.slice(shared);
  const result = [...up, ...down].join(sep);
  return result || ".";
}

function parse(path) {
  const root = isAbsolute(path) ? sep : "";
  const dir = dirname(path);
  const base = basename(path);
  const ext = extname(path);
  const name = ext ? base.slice(0, -ext.length) : base;
  return { root, dir, base, ext, name };
}

function format(pathObject = {}) {
  const dir = pathObject.dir || pathObject.root || "";
  const base =
    pathObject.base ||
    `${pathObject.name || ""}${pathObject.ext || ""}`;
  if (!dir) return base;
  if (!base) return dir;
  return dir.endsWith(sep) ? `${dir}${base}` : `${dir}${sep}${base}`;
}

function normalize(path) {
  return join(String(path || ""));
}

const pathModule = {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
};
const posix = pathModule;
const win32 = pathModule;
pathModule.posix = posix;
pathModule.win32 = win32;

export { basename, delimiter, dirname, extname, format, isAbsolute, join, normalize, parse, posix, relative, resolve, sep, win32 };
export default pathModule;
"#,
        );
    }

    if module_name == "url" {
        return String::from(
            r#"const NativeURL = globalThis.URL;

function normalizeFilePath(value) {
  const path = String(value ?? "");
  if (path.length === 0) {
    return "/";
  }
  return path.startsWith("/") ? path : `/${path}`;
}

function encodeFilePath(path) {
  return path
    .split("/")
    .map((segment, index) =>
      index === 0
        ? ""
        : encodeURIComponent(segment).replace(/[!'()*]/g, (char) =>
            `%${char.charCodeAt(0).toString(16).toUpperCase()}`
          )
    )
    .join("/");
}

function buildFileUrlRecord(href, pathname) {
  const searchParams = new URLSearchParams();
  return {
    href,
    origin: "null",
    protocol: "file:",
    username: "",
    password: "",
    host: "",
    hostname: "",
    port: "",
    pathname,
    search: "",
    searchParams,
    hash: "",
    toString() {
      return href;
    },
    toJSON() {
      return href;
    },
    valueOf() {
      return href;
    },
    [Symbol.toPrimitive]() {
      return href;
    },
  };
}

function fileURLToPath(value) {
  const raw =
    typeof value === "string"
      ? value
      : value && typeof value.href === "string"
        ? value.href
        : String(value ?? "");
  if (raw.startsWith("/")) {
    return raw;
  }
  if (raw.startsWith("file:")) {
    let pathname = raw.startsWith("file://")
      ? raw.slice("file://".length)
      : raw.slice("file:".length);
    const terminatorIndex = pathname.search(/[?#]/);
    if (terminatorIndex >= 0) {
      pathname = pathname.slice(0, terminatorIndex);
    }
    if (!pathname.startsWith("/")) {
      const slashIndex = pathname.indexOf("/");
      if (slashIndex === -1) {
        return "/";
      }
      const host = pathname.slice(0, slashIndex);
      if (host && host !== "localhost") {
        throw new Error(`Expected file URL with an empty host, received ${host}`);
      }
      pathname = pathname.slice(slashIndex);
    }
    return decodeURIComponent(pathname || "/");
  }
  const url = value instanceof NativeURL ? value : new NativeURL(raw);
  if (url.protocol !== "file:") {
    throw new Error(`Expected file URL, received ${url.protocol}`);
  }
  return decodeURIComponent(url.pathname);
}

function pathToFileURL(path) {
  const absolute = normalizeFilePath(path);
  const pathname = encodeFilePath(absolute);
  const href = `file://${pathname}`;

  try {
    return new NativeURL(href);
  } catch {}

  return buildFileUrlRecord(href, pathname);
}

function parse(input, parseQueryString = false) {
  const parsed = new NativeURL(String(input ?? ""));
  const queryString = parsed.search.length > 0 ? parsed.search.slice(1) : null;
  const auth =
    parsed.username || parsed.password
      ? `${decodeURIComponent(parsed.username)}${parsed.password ? `:${decodeURIComponent(parsed.password)}` : ""}`
      : null;
  return {
    href: parsed.href,
    protocol: parsed.protocol,
    slashes: true,
    auth,
    host: parsed.host,
    port: parsed.port || null,
    hostname: parsed.hostname,
    hash: parsed.hash || null,
    search: parsed.search || null,
    query: parseQueryString ? Object.fromEntries(parsed.searchParams.entries()) : queryString,
    pathname: parsed.pathname,
    path: `${parsed.pathname}${parsed.search}`,
  };
}

function format(value) {
  if (value == null) return "";
  if (typeof value === "string") return value;
  if (typeof value.href === "string") return value.href;

  const protocol = typeof value.protocol === "string" ? value.protocol : "http:";
  const slashes = value.slashes === false ? "" : "//";
  const auth =
    typeof value.auth === "string" && value.auth.length > 0 ? `${value.auth}@` : "";
  const host =
    typeof value.host === "string" && value.host.length > 0
      ? value.host
      : `${value.hostname || ""}${value.port ? `:${value.port}` : ""}`;
  const pathname =
    typeof value.pathname === "string"
      ? value.pathname
      : typeof value.path === "string"
        ? value.path
        : "";

  let search = "";
  if (typeof value.search === "string") {
    search = value.search;
  } else if (typeof value.query === "string" && value.query.length > 0) {
    search = value.query.startsWith("?") ? value.query : `?${value.query}`;
  } else if (value.query && typeof value.query === "object") {
    const params = new URLSearchParams();
    for (const [key, entry] of Object.entries(value.query)) {
      if (Array.isArray(entry)) {
        for (const item of entry) {
          params.append(key, String(item));
        }
      } else if (entry != null) {
        params.append(key, String(entry));
      }
    }
    const encoded = params.toString();
    search = encoded ? `?${encoded}` : "";
  }

  const hash = typeof value.hash === "string" ? value.hash : "";
  return `${protocol}${slashes}${auth}${host}${pathname}${search}${hash}`;
}

export { NativeURL as URL, fileURLToPath, format, parse, pathToFileURL };
export default { URL: NativeURL, fileURLToPath, format, parse, pathToFileURL };
"#,
        );
    }

    if module_name == "readline" {
        return String::from(
            r#"class MiniEmitter {
  constructor() {
    this.listeners = new Map();
  }

  on(event, listener) {
    const listeners = this.listeners.get(event) ?? [];
    listeners.push(listener);
    this.listeners.set(event, listeners);
    return this;
  }

  addListener(event, listener) {
    return this.on(event, listener);
  }

  once(event, listener) {
    const wrapped = (...args) => {
      this.off(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }

  off(event, listener) {
    const listeners = this.listeners.get(event) ?? [];
    this.listeners.set(
      event,
      listeners.filter((candidate) => candidate !== listener),
    );
    return this;
  }

  removeListener(event, listener) {
    return this.off(event, listener);
  }

  emit(event, ...args) {
    const listeners = this.listeners.get(event) ?? [];
    for (const listener of listeners) {
      listener(...args);
    }
    return listeners.length > 0;
  }
}

export function createInterface(options = {}) {
  const input = options.input ?? null;
  const output = options.output ?? null;
  const emitter = new MiniEmitter();
  let buffer = "";
  let closed = false;
  let ended = false;
  const queuedLines = [];
  let pendingResolve = null;
  const pendingQuestionResolves = [];

  const enqueueLine = (line) => {
    if (pendingQuestionResolves.length > 0) {
      const resolve = pendingQuestionResolves.shift();
      resolve(line);
      return;
    }
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = null;
      resolve({ done: false, value: line });
      return;
    }
    queuedLines.push(line);
  };

  const flush = () => {
    if (buffer.length > 0) {
      emitter.emit("line", buffer);
      enqueueLine(buffer);
      buffer = "";
    }
  };

  const onData = (chunk) => {
    buffer += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8");
    while (true) {
      const index = buffer.indexOf("\n");
      if (index < 0) break;
      const line = buffer.slice(0, index).replace(/\r$/, "");
      buffer = buffer.slice(index + 1);
      emitter.emit("line", line);
      enqueueLine(line);
    }
  };

  const onEnd = () => {
    if (ended) return;
    ended = true;
    flush();
    emitter.emit("close");
    while (pendingQuestionResolves.length > 0) {
      const resolve = pendingQuestionResolves.shift();
      resolve("");
    }
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = null;
      resolve({ done: true, value: void 0 });
    }
  };

  if (input && typeof input.on === "function") {
    input.on("data", onData);
    input.on("end", onEnd);
    if (typeof input.resume === "function") {
      input.resume();
    }
  }

  emitter.close = () => {
    if (closed) return;
    closed = true;
    if (input && typeof input.off === "function") {
      input.off("data", onData);
      input.off("end", onEnd);
    }
    flush();
    emitter.emit("close");
    while (pendingQuestionResolves.length > 0) {
      const resolve = pendingQuestionResolves.shift();
      resolve("");
    }
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = null;
      resolve({ done: true, value: void 0 });
    }
  };

  emitter.question = (prompt, callback) => {
    if (output && typeof output.write === "function" && prompt) {
      output.write(String(prompt));
    }
    const readLine = () => {
      if (queuedLines.length > 0) {
        return Promise.resolve(queuedLines.shift());
      }
      if (closed || ended) {
        return Promise.resolve("");
      }
      return new Promise((resolve) => {
        pendingQuestionResolves.push(resolve);
      });
    };
    if (typeof callback === "function") {
      void readLine().then((line) => {
        callback(line);
      });
      return;
    }
    return readLine();
  };

  emitter[Symbol.asyncIterator] = () => ({
    next() {
      if (queuedLines.length > 0) {
        return Promise.resolve({ done: false, value: queuedLines.shift() });
      }
      if (closed || ended) {
        return Promise.resolve({ done: true, value: void 0 });
      }
      return new Promise((resolve) => {
        pendingResolve = resolve;
      });
    },
    return() {
      emitter.close();
      return Promise.resolve({ done: true, value: void 0 });
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  });

  return emitter;
}

export default { createInterface };
"#,
        );
    }

    if module_name == "stream" {
        return String::from(
            r#"class MiniEmitter {
  constructor() {
    this._listeners = new Map();
    this._onceListeners = new Map();
  }

  on(event, listener) {
    const listeners = this._listeners.get(event) ?? [];
    listeners.push(listener);
    this._listeners.set(event, listeners);
    return this;
  }

  once(event, listener) {
    const listeners = this._onceListeners.get(event) ?? [];
    listeners.push(listener);
    this._onceListeners.set(event, listeners);
    return this;
  }

  off(event, listener) {
    for (const map of [this._listeners, this._onceListeners]) {
      const listeners = map.get(event) ?? [];
      map.set(
        event,
        listeners.filter((candidate) => candidate !== listener),
      );
    }
    return this;
  }

  removeListener(event, listener) {
    return this.off(event, listener);
  }

  emit(event, ...args) {
    const persistent = [...(this._listeners.get(event) ?? [])];
    const once = [...(this._onceListeners.get(event) ?? [])];
    this._onceListeners.delete(event);
    for (const listener of persistent) {
      listener(...args);
    }
    for (const listener of once) {
      listener(...args);
    }
    return persistent.length + once.length > 0;
  }
}

function getCallback(encodingOrCallback, callback) {
  if (typeof encodingOrCallback === "function") return encodingOrCallback;
  if (typeof callback === "function") return callback;
  return null;
}

function queueResult(callback, error = null) {
  if (typeof callback !== "function") return;
  queueMicrotask(() => callback(error));
}

function createReadableAsyncIterator(stream) {
  const queuedChunks = [];
  let pendingResolve = null;
  let pendingReject = null;
  let done = stream?.readableEnded === true;
  let error = stream?.errored ?? null;

  const cleanup = () => {
    stream?.off?.("data", onData);
    stream?.off?.("end", onEnd);
    stream?.off?.("close", onEnd);
    stream?.off?.("error", onError);
  };

  const settlePending = (result) => {
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = null;
      pendingReject = null;
      resolve(result);
    }
  };

  const rejectPending = (reason) => {
    if (pendingReject) {
      const reject = pendingReject;
      pendingResolve = null;
      pendingReject = null;
      reject(reason);
    }
  };

  const onData = (chunk) => {
    if (pendingResolve) {
      settlePending({ done: false, value: chunk });
      return;
    }
    queuedChunks.push(chunk);
  };

  const onEnd = () => {
    if (done) return;
    done = true;
    cleanup();
    settlePending({ done: true, value: void 0 });
  };

  const onError = (reason) => {
    error = reason;
    done = true;
    cleanup();
    rejectPending(reason);
  };

  const pull = () => {
    if (done || typeof stream?._read !== "function") {
      return;
    }
    try {
      stream._read();
    } catch (reason) {
      stream.errored = reason;
      onError(reason);
    }
  };

  stream?.on?.("data", onData);
  stream?.on?.("end", onEnd);
  stream?.on?.("close", onEnd);
  stream?.on?.("error", onError);

  return {
    next() {
      if (error) {
        return Promise.reject(error);
      }
      if (queuedChunks.length > 0) {
        return Promise.resolve({ done: false, value: queuedChunks.shift() });
      }
      if (done) {
        return Promise.resolve({ done: true, value: void 0 });
      }
      pull();
      if (queuedChunks.length > 0) {
        return Promise.resolve({ done: false, value: queuedChunks.shift() });
      }
      if (done) {
        return Promise.resolve({ done: true, value: void 0 });
      }
      return new Promise((resolve, reject) => {
        pendingResolve = resolve;
        pendingReject = reject;
      });
    },
    return() {
      done = true;
      cleanup();
      stream?.destroy?.();
      return Promise.resolve({ done: true, value: void 0 });
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  };
}

class Stream extends MiniEmitter {
  pipe(destination) {
    this.on("data", (chunk) => destination.write(chunk));
    this.once("end", () => destination.end());
    return destination;
  }

  destroy(error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    if (error) {
      this.errored = error;
      queueMicrotask(() => this.emit("error", error));
    }
    queueMicrotask(() => this.emit("close"));
    return this;
  }
}

class Readable extends Stream {
  constructor() {
    super();
    this.readable = true;
    this.readableEnded = false;
    this.destroyed = false;
  }

  push(chunk) {
    if (chunk === null) {
      if (!this.readableEnded) {
        this.readableEnded = true;
        queueMicrotask(() => {
          this.emit("end");
          this.emit("close");
        });
      }
      return false;
    }
    this.emit("data", Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk ?? []));
    return true;
  }

  static fromWeb(stream) {
    if (!stream || typeof stream.getReader !== "function") {
      throw new TypeError("Readable.fromWeb expects a WHATWG ReadableStream");
    }
    return {
      async *[Symbol.asyncIterator]() {
        const reader = stream.getReader();
        try {
          while (true) {
            const { value, done } = await reader.read();
            if (done) break;
            yield Buffer.from(value ?? []);
          }
        } finally {
          reader.releaseLock?.();
        }
      },
    };
  }

  [Symbol.asyncIterator]() {
    return createReadableAsyncIterator(this);
  }
}

class Writable extends Stream {
  constructor(options = undefined) {
    super();
    this.writable = true;
    this.writableEnded = false;
    this.destroyed = false;
    this._writeOption =
      options && typeof options.write === "function" ? options.write : null;
    this._destroyOption =
      options && typeof options.destroy === "function" ? options.destroy : null;
  }

  write(chunk, encodingOrCallback, callback) {
    if (this.writableEnded) {
      const error = new Error("write after end");
      queueResult(getCallback(encodingOrCallback, callback), error);
      this.emit("error", error);
      return false;
    }
    const done = getCallback(encodingOrCallback, callback);
    this._write(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk ?? []), done);
    return true;
  }

  _write(_chunk, callback) {
    if (!this._writeOption) {
      queueResult(callback);
      return;
    }
    try {
      this._writeOption.call(this, _chunk, "buffer", callback);
    } catch (error) {
      queueResult(callback, error);
    }
  }

  _destroy(error, callback) {
    if (!this._destroyOption) {
      queueResult(callback, error);
      return;
    }
    try {
      this._destroyOption.call(this, error ?? null, callback);
    } catch (destroyError) {
      queueResult(callback, destroyError);
    }
  }

  destroy(error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this._destroy(error ?? null, (destroyError) => {
      const finalError = destroyError ?? error;
      if (finalError) {
        this.errored = finalError;
        this.emit("error", finalError);
      }
      this.emit("close");
    });
    return this;
  }

  end(chunk, encodingOrCallback, callback) {
    if (chunk !== undefined && chunk !== null) {
      this.write(chunk, encodingOrCallback);
    }
    if (this.writableEnded) {
      queueResult(getCallback(encodingOrCallback, callback));
      return this;
    }
    this.writableEnded = true;
    const done = getCallback(encodingOrCallback, callback);
    queueMicrotask(() => {
      queueResult(done);
      this.emit("finish");
      this.destroy();
    });
    return this;
  }
}

class Duplex extends Readable {
  constructor() {
    super();
    this.writable = true;
    this.writableEnded = false;
  }

  write(chunk, encodingOrCallback, callback) {
    return Writable.prototype.write.call(this, chunk, encodingOrCallback, callback);
  }

  _write(chunk, callback) {
    queueResult(callback);
  }

  end(chunk, encodingOrCallback, callback) {
    return Writable.prototype.end.call(this, chunk, encodingOrCallback, callback);
  }
}

class Transform extends Duplex {
  _write(chunk, callback) {
    try {
      this._transform(chunk, "buffer", (error, output) => {
        if (!error && output !== undefined && output !== null) {
          this.push(output);
        }
        queueResult(callback, error ?? null);
      });
    } catch (error) {
      queueResult(callback, error);
      this.emit("error", error);
    }
  }

  _transform(chunk, _encoding, callback) {
    callback(null, chunk);
  }

  end(chunk, encodingOrCallback, callback) {
    Writable.prototype.end.call(this, chunk, encodingOrCallback, callback);
    this.push(null);
    return this;
  }
}

class PassThrough extends Transform {}

function finished(stream, callback) {
  const done = (error = null) => {
    cleanup();
    if (typeof callback === "function") callback(error);
  };
  const onFinish = () => done();
  const onEnd = () => done();
  const onClose = () => done();
  const onError = (error) => done(error);
  const cleanup = () => {
    stream?.off?.("finish", onFinish);
    stream?.off?.("end", onEnd);
    stream?.off?.("close", onClose);
    stream?.off?.("error", onError);
  };
  stream?.once?.("finish", onFinish);
  stream?.once?.("end", onEnd);
  stream?.once?.("close", onClose);
  stream?.once?.("error", onError);
  return cleanup;
}

function pipeline(...streams) {
  const callback =
    streams.length > 0 && typeof streams[streams.length - 1] === "function"
      ? streams.pop()
      : null;
  if (streams.length < 2) {
    const error = new TypeError("pipeline requires at least two streams");
    callback?.(error);
    throw error;
  }
  for (let index = 0; index < streams.length - 1; index += 1) {
    streams[index].pipe(streams[index + 1]);
  }
  if (callback) {
    finished(streams[streams.length - 1], callback);
  }
  return streams[streams.length - 1];
}

function compose(...streams) {
  return pipeline(...streams);
}

function addAbortSignal(signal, stream) {
  if (signal?.aborted) {
    stream?.destroy?.(signal.reason);
    return stream;
  }
  signal?.addEventListener?.("abort", () => stream?.destroy?.(signal.reason), {
    once: true,
  });
  return stream;
}

function isReadable(stream) {
  return Boolean(stream && stream.readable && !stream.destroyed);
}

function isWritable(stream) {
  return Boolean(stream && stream.writable && !stream.destroyed);
}

function isErrored(stream) {
  return Boolean(stream && stream.errored);
}

function isDisturbed(stream) {
  return Boolean(
    stream && (stream.disturbed === true || stream.locked || stream.readableDidRead === true),
  );
}

const streamModule = Stream;
Object.assign(streamModule, {
  Duplex,
  PassThrough,
  Readable,
  Stream,
  Transform,
  Writable,
  addAbortSignal,
  compose,
  finished,
  isDisturbed,
  isErrored,
  isReadable,
  isWritable,
  pipeline,
});

export {
  Duplex,
  PassThrough,
  Readable,
  Stream,
  Transform,
  Writable,
  addAbortSignal,
  compose,
  finished,
  isDisturbed,
  isErrored,
  isReadable,
  isWritable,
  pipeline,
};
export default streamModule;
"#,
        );
    }

    if module_name == "stream/promises" {
        return String::from(
            r#"const _m = globalThis._requireFrom("node:stream/promises", "/");

export default _m;
export const finished = _m.finished;
export const pipeline = _m.pipeline;
"#,
        );
    }

    if module_name == "zlib" {
        return String::from(
            r#"const _m = globalThis._requireFrom("node:zlib", "/");
const zlibConstants =
  typeof _m.constants === "object" && _m.constants !== null
    ? _m.constants
    : Object.fromEntries(
        Object.entries(_m).filter(
          ([key, value]) => /^[A-Z0-9_]+$/.test(key) && typeof value === "number",
        ),
      );

if (typeof _m.constants === "undefined") {
  Object.defineProperty(_m, "constants", {
    configurable: true,
    enumerable: true,
    value: zlibConstants,
    writable: true,
  });
}

export default _m;
export const constants = _m.constants;
export const BrotliCompress = _m.BrotliCompress;
export const BrotliDecompress = _m.BrotliDecompress;
export const Deflate = _m.Deflate;
export const DeflateRaw = _m.DeflateRaw;
export const Gunzip = _m.Gunzip;
export const Gzip = _m.Gzip;
export const Inflate = _m.Inflate;
export const InflateRaw = _m.InflateRaw;
export const Unzip = _m.Unzip;
export const brotliCompress = _m.brotliCompress;
export const brotliCompressSync = _m.brotliCompressSync;
export const brotliDecompress = _m.brotliDecompress;
export const brotliDecompressSync = _m.brotliDecompressSync;
export const createBrotliCompress = _m.createBrotliCompress;
export const createBrotliDecompress = _m.createBrotliDecompress;
export const createDeflate = _m.createDeflate;
export const createDeflateRaw = _m.createDeflateRaw;
export const createGunzip = _m.createGunzip;
export const createGzip = _m.createGzip;
export const createInflate = _m.createInflate;
export const createInflateRaw = _m.createInflateRaw;
export const createUnzip = _m.createUnzip;
export const deflate = _m.deflate;
export const deflateRaw = _m.deflateRaw;
export const deflateRawSync = _m.deflateRawSync;
export const deflateSync = _m.deflateSync;
export const gunzip = _m.gunzip;
export const gunzipSync = _m.gunzipSync;
export const gzip = _m.gzip;
export const gzipSync = _m.gzipSync;
export const inflate = _m.inflate;
export const inflateRaw = _m.inflateRaw;
export const inflateRawSync = _m.inflateRawSync;
export const inflateSync = _m.inflateSync;
export const unzip = _m.unzip;
export const unzipSync = _m.unzipSync;
"#,
        );
    }

    if module_name == "stream/web" {
        return String::from(
            r#"export const ReadableStream = globalThis.ReadableStream;
export const WritableStream = globalThis.WritableStream;
export const TransformStream = globalThis.TransformStream;
export const TextEncoderStream = globalThis.TextEncoderStream;
export const TextDecoderStream = globalThis.TextDecoderStream;
export const CompressionStream = globalThis.CompressionStream;
export const DecompressionStream = globalThis.DecompressionStream;
export default {
  ReadableStream,
  WritableStream,
  TransformStream,
  TextEncoderStream,
  TextDecoderStream,
  CompressionStream,
  DecompressionStream,
};
"#,
        );
    }

    if module_name == "fs/promises" {
        return String::from(
            r#"const fsModule = globalThis._requireFrom("node:fs", "/");
const _m = fsModule.promises;

export default _m;
export const constants = fsModule.constants;
export const FileHandle = _m.FileHandle;
export const access = _m.access;
export const appendFile = _m.appendFile;
export const chmod = _m.chmod;
export const chown = _m.chown;
export const copyFile = _m.copyFile;
export const cp = _m.cp;
export const lchmod = _m.lchmod;
export const lchown = _m.lchown;
export const link = _m.link;
export const lstat = _m.lstat;
export const lutimes = _m.lutimes;
export const mkdir = _m.mkdir;
export const mkdtemp = _m.mkdtemp;
export const open = _m.open;
export const opendir = _m.opendir;
export const readFile = _m.readFile;
export const readdir = _m.readdir;
export const readlink = _m.readlink;
export const realpath = _m.realpath;
export const rename = _m.rename;
export const rm = _m.rm;
export const rmdir = _m.rmdir;
export const stat = _m.stat;
export const statfs = _m.statfs;
export const symlink = _m.symlink;
export const truncate = _m.truncate;
export const unlink = _m.unlink;
export const utimes = _m.utimes;
export const watch = _m.watch;
export const writeFile = _m.writeFile;
"#,
        );
    }

    if module_name == "readline" {
        return String::from(
            r#"const _m = globalThis._requireFrom("node:readline", "/");

function createInterface(...args) {
  const interfaceValue = _m.createInterface(...args);
  if (interfaceValue && typeof interfaceValue === "object") {
    if (interfaceValue.__agentOSReadlineWrapped === true) {
      return interfaceValue;
    }
    Object.defineProperty(interfaceValue, "__agentOSReadlineWrapped", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    const options = args[0] && typeof args[0] === "object" ? args[0] : {};
    const output = options.output ?? null;
    const originalOn = typeof interfaceValue.on === "function"
      ? interfaceValue.on.bind(interfaceValue)
      : null;
    const originalOff = typeof interfaceValue.off === "function"
      ? interfaceValue.off.bind(interfaceValue)
      : typeof interfaceValue.removeListener === "function"
        ? interfaceValue.removeListener.bind(interfaceValue)
        : null;
    const originalClose = typeof interfaceValue.close === "function"
      ? interfaceValue.close.bind(interfaceValue)
      : null;
    const queued = [];
    const pendingQuestionResolves = [];
    let pendingResolve = null;
    let done = false;
    const enqueue = (line) => {
      if (pendingQuestionResolves.length > 0) {
        const resolve = pendingQuestionResolves.shift();
        resolve(line);
        return;
      }
      if (pendingResolve) {
        const resolve = pendingResolve;
        pendingResolve = null;
        resolve({ done: false, value: line });
        return;
      }
      queued.push(line);
    };
    const finish = () => {
      if (done) {
        return;
      }
      done = true;
      while (pendingQuestionResolves.length > 0) {
        const resolve = pendingQuestionResolves.shift();
        resolve("");
      }
      if (pendingResolve) {
        const resolve = pendingResolve;
        pendingResolve = null;
        resolve({ done: true, value: void 0 });
      }
    };
    const readLine = () => {
      if (queued.length > 0) {
        return Promise.resolve(queued.shift());
      }
      if (done) {
        return Promise.resolve("");
      }
      return new Promise((resolve) => {
        pendingQuestionResolves.push(resolve);
      });
    };
    originalOn?.("line", enqueue);
    originalOn?.("close", finish);
    interfaceValue.question = (prompt, callback) => {
      if (output && typeof output.write === "function" && prompt) {
        output.write(String(prompt));
      }
      if (typeof callback === "function") {
        void readLine().then((line) => {
          callback(line);
        });
        return;
      }
      return readLine();
    };
    interfaceValue[Symbol.asyncIterator] = () => ({
      next() {
        if (queued.length > 0) {
          return Promise.resolve({ done: false, value: queued.shift() });
        }
        if (done) {
          return Promise.resolve({ done: true, value: void 0 });
        }
        return new Promise((resolve) => {
          pendingResolve = resolve;
        });
      },
      return() {
        originalOff?.("line", enqueue);
        originalOff?.("close", finish);
        originalClose?.();
        finish();
        return Promise.resolve({ done: true, value: void 0 });
      },
      [Symbol.asyncIterator]() {
        return this;
      },
    });
  }
  return interfaceValue;
}

export default _m;
export { createInterface };
"#,
        );
    }

    if module_name == "string_decoder" {
        return String::from(
            r#"class StringDecoder {
  constructor(encoding = "utf8") {
    this.encoding = encoding;
    this.decoder = new TextDecoder(encoding, { fatal: false });
  }

  write(input) {
    const buffer =
      typeof input === "string"
        ? Buffer.from(input, this.encoding)
        : Buffer.isBuffer(input)
          ? input
          : Buffer.from(input ?? []);
    return this.decoder.decode(buffer, { stream: true });
  }

  end(input) {
    let output = "";
    if (input !== undefined) {
      output += this.write(input);
    }
    output += this.decoder.decode();
    return output;
  }
}

export { StringDecoder };
export default { StringDecoder };
"#,
        );
    }

    if module_name == "v8" {
        return String::from(
            r#"function serialize(value) {
  return Buffer.from(JSON.stringify(value ?? null), "utf8");
}

function deserialize(value) {
  const buffer = Buffer.isBuffer(value) ? value : Buffer.from(value ?? []);
  return JSON.parse(buffer.toString("utf8"));
}

class Serializer {
  constructor() {
    this._value = null;
  }

  writeHeader() {}

  writeValue(value) {
    this._value = value;
  }

  releaseBuffer() {
    return serialize(this._value);
  }

  transferArrayBuffer() {}
}

class Deserializer {
  constructor(buffer) {
    this._buffer = buffer;
  }

  readHeader() {}

  readValue() {
    return deserialize(this._buffer);
  }

  transferArrayBuffer() {}
}

function cachedDataVersionTag() {
  return 0;
}

function getCppHeapStatistics() {
  return {
    committed_size_bytes: 0,
    resident_size_bytes: 0,
    used_size_bytes: 0,
    space_statistics: [],
  };
}

function getHeapCodeStatistics() {
  return {
    code_and_metadata_size: 0,
    bytecode_and_metadata_size: 0,
    external_script_source_size: 0,
    cpu_profiler_metadata_size: 0,
  };
}

function configuredHeapLimitBytes() {
  const configured = Number(globalThis.__agentOSV8HeapLimitBytes);
  if (!Number.isFinite(configured) || configured <= 0) {
    return 0;
  }
  return configured;
}

function getHeapStatistics() {
  const heapLimit = configuredHeapLimitBytes();
  return {
    total_heap_size: 0,
    total_heap_size_executable: 0,
    total_physical_size: 0,
    total_available_size: 0,
    used_heap_size: 0,
    heap_size_limit: heapLimit,
    malloced_memory: 0,
    peak_malloced_memory: 0,
    does_zap_garbage: 0,
    number_of_native_contexts: 0,
    number_of_detached_contexts: 0,
    total_global_handles_size: 0,
    used_global_handles_size: 0,
    external_memory: 0,
  };
}

function getHeapSpaceStatistics() {
  return [];
}

function getHeapSnapshot() {
  return Readable.fromWeb(
    new ReadableStream({
      start(controller) {
        controller.enqueue(Buffer.from("{}"));
        controller.close();
      },
    }),
  );
}

function isStringOneByteRepresentation(value) {
  return typeof value === "string" && !/[^\x00-\xff]/.test(value);
}

function queryObjects() {
  return [];
}

function setFlagsFromString() {}

function setHeapSnapshotNearHeapLimit() {
  return [];
}

function startCpuProfile() {
  return {
    stop() {
      return {};
    },
  };
}

function stopCoverage() {
  return [];
}

function takeCoverage() {
  return [];
}

function writeHeapSnapshot() {
  return "";
}

class GCProfiler {
  start() {}

  stop() {
    return [];
  }
}

const promiseHooks = {};
const startupSnapshot = {};

export {
  GCProfiler,
  cachedDataVersionTag,
  Deserializer,
  deserialize,
  getCppHeapStatistics,
  getHeapCodeStatistics,
  getHeapSnapshot,
  getHeapSpaceStatistics,
  getHeapStatistics,
  isStringOneByteRepresentation,
  promiseHooks,
  queryObjects,
  serialize,
  Serializer,
  setFlagsFromString,
  setHeapSnapshotNearHeapLimit,
  startCpuProfile,
  startupSnapshot,
  stopCoverage,
  takeCoverage,
  writeHeapSnapshot,
};
export {
  Deserializer as DefaultDeserializer,
  Serializer as DefaultSerializer,
};
export default {
  GCProfiler,
  cachedDataVersionTag,
  DefaultDeserializer: Deserializer,
  DefaultSerializer: Serializer,
  Deserializer,
  deserialize,
  getCppHeapStatistics,
  getHeapCodeStatistics,
  getHeapSnapshot,
  getHeapSpaceStatistics,
  getHeapStatistics,
  isStringOneByteRepresentation,
  promiseHooks,
  queryObjects,
  serialize,
  Serializer,
  setFlagsFromString,
  setHeapSnapshotNearHeapLimit,
  startCpuProfile,
  startupSnapshot,
  stopCoverage,
  takeCoverage,
  writeHeapSnapshot,
};
"#,
        );
    }

    if module_name == "vm" {
        return String::from(
            r#"const VM_CONTEXT_TAG = typeof Symbol === "function" ? Symbol.for("secure-exec.vm.context") : "__secure_exec_vm_context__";
const VM_CONTEXT_ID = typeof Symbol === "function" ? Symbol.for("secure-exec.vm.context.id") : "__secure_exec_vm_context_id__";

function createVmNotImplementedError(feature) {
  const error = new Error(`node:vm ${feature} is not implemented in the secure-exec guest runtime`);
  error.code = "ERR_NOT_IMPLEMENTED";
  return error;
}

function isVmContextCandidate(value) {
  return value !== null && (typeof value === "object" || typeof value === "function");
}

function normalizeVmOptions(options = undefined) {
  if (typeof options === "string") {
    return { filename: options };
  }
  if (!options || typeof options !== "object") {
    return {};
  }
  const normalized = {};
  if (typeof options.filename === "string") {
    normalized.filename = options.filename;
  }
  if (Number.isInteger(options.lineOffset)) {
    normalized.lineOffset = options.lineOffset;
  }
  if (Number.isInteger(options.columnOffset)) {
    normalized.columnOffset = options.columnOffset;
  }
  if (Number.isInteger(options.timeout) && options.timeout > 0) {
    normalized.timeout = options.timeout;
  }
  if (options.cachedData !== undefined) {
    normalized.cachedData = options.cachedData;
  }
  if (options.produceCachedData === true) {
    normalized.produceCachedData = true;
  }
  return normalized;
}

function mergeVmOptions(baseOptions, overrideOptions) {
  return { ...normalizeVmOptions(baseOptions), ...normalizeVmOptions(overrideOptions) };
}

function createContext(context = {}) {
  if (!isVmContextCandidate(context)) {
    throw new TypeError('The "object" argument must be of type object.');
  }
  if (context[VM_CONTEXT_TAG] === true && Number.isInteger(context[VM_CONTEXT_ID])) {
    return context;
  }
  const contextId = globalThis._vmCreateContext(context);
  Object.defineProperty(context, VM_CONTEXT_TAG, {
    value: true,
    configurable: true,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(context, VM_CONTEXT_ID, {
    value: contextId,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  return context;
}

function isContext(context) {
  return isVmContextCandidate(context) && context[VM_CONTEXT_TAG] === true && Number.isInteger(context[VM_CONTEXT_ID]);
}

function assertContext(context) {
  if (!isContext(context)) {
    throw new TypeError('The "contextifiedObject" argument must be a vm context.');
  }
  return context;
}

function runInThisContext(code, options = undefined) {
  return globalThis._vmRunInThisContext(String(code), normalizeVmOptions(options));
}

function runInContext(code, contextifiedObject, options = undefined) {
  const context = assertContext(contextifiedObject);
  return globalThis._vmRunInContext(context[VM_CONTEXT_ID], String(code), normalizeVmOptions(options), context);
}

function runInNewContext(code, contextOrOptions = {}, maybeOptions = undefined) {
  const hasExplicitContext = isVmContextCandidate(contextOrOptions);
  const context = hasExplicitContext ? contextOrOptions : {};
  const options = hasExplicitContext ? maybeOptions : contextOrOptions;
  return runInContext(code, createContext(context), options);
}

class Script {
  constructor(code, options = undefined) {
    this.code = String(code);
    this.options = normalizeVmOptions(options);
    this.filename = this.options.filename ?? "evalmachine.<anonymous>";
    this.lineOffset = this.options.lineOffset ?? 0;
    this.columnOffset = this.options.columnOffset ?? 0;
    this.cachedData = this.options.cachedData;
    this.cachedDataProduced = false;
    this.cachedDataRejected = false;
  }

  createCachedData() {
    return typeof Buffer === "function" ? Buffer.alloc(0) : new Uint8Array(0);
  }

  runInThisContext(options = undefined) {
    return runInThisContext(this.code, mergeVmOptions(this.options, options));
  }

  runInContext(contextifiedObject, options = undefined) {
    return runInContext(this.code, contextifiedObject, mergeVmOptions(this.options, options));
  }

  runInNewContext(context = {}, options = undefined) {
    return runInNewContext(this.code, context, mergeVmOptions(this.options, options));
  }
}

function compileFunction() {
  throw createVmNotImplementedError("compileFunction");
}

function measureMemory() {
  throw createVmNotImplementedError("measureMemory");
}

export { Script, compileFunction, createContext, isContext, measureMemory, runInContext, runInNewContext, runInThisContext };
export default { Script, compileFunction, createContext, isContext, measureMemory, runInContext, runInNewContext, runInThisContext };
"#,
        );
    }

    if module_name == "worker_threads" {
        return String::from(
            r#"function createNotImplementedError(feature) {
  const error = new Error(`node:worker_threads ${feature} is not available in the secure-exec guest runtime`);
  error.code = "ERR_NOT_IMPLEMENTED";
  return error;
}

class MessagePort {
  postMessage() {}
  start() {}
  close() {}
  unref() {
    return this;
  }
  ref() {
    return this;
  }
}

class MessageChannel {
  constructor() {
    this.port1 = new MessagePort();
    this.port2 = new MessagePort();
  }
}

class Worker {
  constructor() {
    throw createNotImplementedError("Worker");
  }
}

function getEnvironmentData() {
  return undefined;
}

function markAsUncloneable() {}

function markAsUntransferable() {}

function moveMessagePortToContext() {
  throw createNotImplementedError("moveMessagePortToContext");
}

function postMessageToThread() {
  throw createNotImplementedError("postMessageToThread");
}

function receiveMessageOnPort() {
  return undefined;
}

function setEnvironmentData() {}

export const BroadcastChannel = globalThis.BroadcastChannel;
export { MessageChannel, MessagePort, Worker, getEnvironmentData, markAsUncloneable, markAsUntransferable, moveMessagePortToContext, postMessageToThread, receiveMessageOnPort, setEnvironmentData };
export const SHARE_ENV = Symbol.for("secure-exec.worker_threads.SHARE_ENV");
export const isMainThread = true;
export const parentPort = null;
export const resourceLimits = {};
export const threadId = 0;
export const workerData = null;
export default {
  BroadcastChannel: globalThis.BroadcastChannel,
  MessageChannel,
  MessagePort,
  SHARE_ENV,
  Worker,
  getEnvironmentData,
  isMainThread,
  markAsUncloneable,
  markAsUntransferable,
  moveMessagePortToContext,
  parentPort,
  postMessageToThread,
  receiveMessageOnPort,
  resourceLimits,
  setEnvironmentData,
  threadId,
  workerData,
};
"#,
        );
    }

    let default_target = format!(
        "globalThis._requireFrom({}, \"/\")",
        serde_json::to_string(&format!("node:{module_name}"))
            .unwrap_or_else(|_| format!("\"node:{module_name}\""))
    );
    let mut exports = builtin_named_exports(module_name)
        .iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    exports.sort_unstable();

    let mut source = format!("const _m = {default_target};\nexport default _m;\n");
    for name in exports {
        source.push_str(&format!("export const {name} = _m[\"{name}\"];\n"));
    }
    source
}

fn builtin_named_exports(module_name: &str) -> &'static [&'static str] {
    match module_name {
        "async_hooks" => &[
            "AsyncLocalStorage",
            "AsyncResource",
            "createHook",
            "executionAsyncId",
            "triggerAsyncId",
        ],
        "buffer" => &[
            "Blob",
            "Buffer",
            "File",
            "INSPECT_MAX_BYTES",
            "SlowBuffer",
            "isUtf8",
        ],
        "child_process" => &[
            "ChildProcess",
            "exec",
            "execFile",
            "execFileSync",
            "execSync",
            "fork",
            "spawn",
            "spawnSync",
        ],
        "console" => &[
            "Console",
            "assert",
            "clear",
            "context",
            "count",
            "countReset",
            "createTask",
            "debug",
            "dir",
            "dirxml",
            "error",
            "group",
            "groupCollapsed",
            "groupEnd",
            "info",
            "log",
            "profile",
            "profileEnd",
            "table",
            "time",
            "timeEnd",
            "timeLog",
            "timeStamp",
            "trace",
            "warn",
        ],
        "crypto" => &[
            "createHash",
            "createPrivateKey",
            "getHashes",
            "getRandomValues",
            "randomBytes",
            "randomFillSync",
            "randomUUID",
            "subtle",
        ],
        "diagnostics_channel" => &[
            "Channel",
            "channel",
            "hasSubscribers",
            "subscribe",
            "tracingChannel",
            "unsubscribe",
        ],
        "events" => &[
            "EventEmitter",
            "addAbortListener",
            "defaultMaxListeners",
            "errorMonitor",
            "getEventListeners",
            "getMaxListeners",
            "on",
            "once",
            "setMaxListeners",
        ],
        "dns" => &[
            "Resolver",
            "getServers",
            "lookup",
            "promises",
            "resolve",
            "resolve4",
            "resolve6",
            "setServers",
        ],
        "dns/promises" => &[
            "Resolver",
            "lookup",
            "resolve",
            "resolve4",
            "resolve6",
            "resolveAny",
            "resolveMx",
            "resolveTxt",
            "resolveSrv",
            "resolveCname",
            "resolvePtr",
            "resolveNs",
            "resolveSoa",
            "resolveNaptr",
            "resolveCaa",
        ],
        "fs" => &[
            "access",
            "accessSync",
            "appendFile",
            "appendFileSync",
            "chmod",
            "chmodSync",
            "closeSync",
            "constants",
            "createReadStream",
            "createWriteStream",
            "existsSync",
            "fstat",
            "fstatSync",
            "fsyncSync",
            "lstat",
            "lstatSync",
            "mkdir",
            "mkdirSync",
            "openSync",
            "readFile",
            "promises",
            "readFileSync",
            "readdir",
            "readSync",
            "readdirSync",
            "readlink",
            "realpathSync",
            "rename",
            "readlinkSync",
            "renameSync",
            "rm",
            "rmSync",
            "stat",
            "statSync",
            "unlink",
            "unlinkSync",
            "watch",
            "watchFile",
            "unwatchFile",
            "writeFile",
            "writeFileSync",
            "writeSync",
        ],
        "fs/promises" => &[
            "access",
            "appendFile",
            "chmod",
            "chown",
            "constants",
            "copyFile",
            "cp",
            "glob",
            "lchown",
            "link",
            "lstat",
            "mkdir",
            "mkdtemp",
            "open",
            "opendir",
            "readFile",
            "readdir",
            "readlink",
            "realpath",
            "rename",
            "rm",
            "rmdir",
            "stat",
            "statfs",
            "symlink",
            "truncate",
            "unlink",
            "utimes",
            "writeFile",
        ],
        "http" => &[
            "Agent",
            "ClientRequest",
            "IncomingMessage",
            "METHODS",
            "Server",
            "ServerResponse",
            "STATUS_CODES",
            "_checkInvalidHeaderChar",
            "_checkIsHttpToken",
            "createServer",
            "get",
            "globalAgent",
            "maxHeaderSize",
            "request",
            "validateHeaderName",
            "validateHeaderValue",
        ],
        "http2" => &["connect", "createServer", "createSecureServer"],
        "https" => &[
            "Agent",
            "ClientRequest",
            "IncomingMessage",
            "Server",
            "ServerResponse",
            "_checkInvalidHeaderChar",
            "_checkIsHttpToken",
            "createServer",
            "get",
            "globalAgent",
            "maxHeaderSize",
            "request",
            "validateHeaderName",
            "validateHeaderValue",
        ],
        "module" => &[
            "Module",
            "_cache",
            "_extensions",
            "_resolveFilename",
            "builtinModules",
            "createRequire",
            "findSourceMap",
            "isBuiltin",
            "syncBuiltinESMExports",
            "wrap",
        ],
        "net" => &[
            "BlockList",
            "Socket",
            "SocketAddress",
            "Server",
            "Stream",
            "connect",
            "createConnection",
            "createServer",
            "getDefaultAutoSelectFamily",
            "getDefaultAutoSelectFamilyAttemptTimeout",
            "isIP",
            "isIPv4",
            "isIPv6",
            "setDefaultAutoSelectFamily",
            "setDefaultAutoSelectFamilyAttemptTimeout",
        ],
        "os" => &[
            "EOL",
            "arch",
            "availableParallelism",
            "constants",
            "cpus",
            "endianness",
            "freemem",
            "homedir",
            "hostname",
            "networkInterfaces",
            "platform",
            "release",
            "totalmem",
            "tmpdir",
            "type",
            "userInfo",
            "version",
        ],
        "path" | "path/posix" | "path/win32" => &[
            "basename",
            "delimiter",
            "dirname",
            "extname",
            "format",
            "isAbsolute",
            "join",
            "normalize",
            "parse",
            "posix",
            "relative",
            "resolve",
            "sep",
            "win32",
        ],
        "process" => &[
            "arch", "argv", "argv0", "cwd", "env", "execPath", "exit", "pid", "platform", "ppid",
            "stderr", "stdin", "stdout", "umask", "version", "versions",
        ],
        "perf_hooks" => &[
            "PerformanceObserver",
            "constants",
            "createHistogram",
            "performance",
        ],
        "readline" => &["createInterface"],
        "sqlite" => &["DatabaseSync", "StatementSync", "constants"],
        "stream" => &[
            "Duplex",
            "PassThrough",
            "Readable",
            "Stream",
            "Transform",
            "Writable",
            "addAbortSignal",
            "compose",
            "finished",
            "isDisturbed",
            "isErrored",
            "isReadable",
            "pipeline",
        ],
        "stream/consumers" => &["arrayBuffer", "blob", "buffer", "json", "text"],
        "sys" => &[
            "MIMEType",
            "MIMEParams",
            "TextDecoder",
            "TextEncoder",
            "callbackify",
            "debug",
            "debuglog",
            "deprecate",
            "format",
            "formatWithOptions",
            "inherits",
            "inspect",
            "parseArgs",
            "promisify",
            "stripVTControlCharacters",
            "types",
        ],
        "timers" => &[
            "clearImmediate",
            "clearInterval",
            "clearTimeout",
            "setImmediate",
            "setInterval",
            "setTimeout",
        ],
        "tty" => &["ReadStream", "WriteStream", "isatty"],
        "tls" => &[
            "TLSSocket",
            "Server",
            "connect",
            "createSecureContext",
            "createServer",
            "getCiphers",
        ],
        "stream/promises" => &["finished", "pipeline"],
        "timers/promises" => &["scheduler", "setImmediate", "setInterval", "setTimeout"],
        "url" => &["URL", "fileURLToPath", "format", "parse", "pathToFileURL"],
        "util" => &[
            "MIMEType",
            "MIMEParams",
            "TextDecoder",
            "TextEncoder",
            "callbackify",
            "debug",
            "debuglog",
            "deprecate",
            "format",
            "formatWithOptions",
            "inherits",
            "inspect",
            "isDeepStrictEqual",
            "parseArgs",
            "promisify",
            "stripVTControlCharacters",
            "types",
        ],
        "util/types" => &[
            "isAnyArrayBuffer",
            "isArgumentsObject",
            "isArrayBuffer",
            "isArrayBufferView",
            "isAsyncFunction",
            "isBigInt64Array",
            "isBigIntObject",
            "isBigUint64Array",
            "isBooleanObject",
            "isBoxedPrimitive",
            "isCryptoKey",
            "isDataView",
            "isDate",
            "isExternal",
            "isFloat16Array",
            "isFloat32Array",
            "isFloat64Array",
            "isGeneratorFunction",
            "isGeneratorObject",
            "isInt16Array",
            "isInt32Array",
            "isInt8Array",
            "isKeyObject",
            "isMap",
            "isMapIterator",
            "isModuleNamespaceObject",
            "isNativeError",
            "isNumberObject",
            "isPromise",
            "isProxy",
            "isRegExp",
            "isSet",
            "isSetIterator",
            "isSharedArrayBuffer",
            "isStringObject",
            "isSymbolObject",
            "isTypedArray",
            "isUint16Array",
            "isUint32Array",
            "isUint8Array",
            "isUint8ClampedArray",
            "isWeakMap",
            "isWeakSet",
        ],
        "vm" => &[
            "Script",
            "compileFunction",
            "createContext",
            "isContext",
            "measureMemory",
            "runInContext",
            "runInNewContext",
            "runInThisContext",
        ],
        "v8" => &[
            "cachedDataVersionTag",
            "DefaultDeserializer",
            "DefaultSerializer",
            "Deserializer",
            "GCProfiler",
            "Serializer",
            "deserialize",
            "getCppHeapStatistics",
            "getHeapCodeStatistics",
            "getHeapSnapshot",
            "getHeapSpaceStatistics",
            "getHeapStatistics",
            "isStringOneByteRepresentation",
            "promiseHooks",
            "queryObjects",
            "serialize",
            "setFlagsFromString",
            "setHeapSnapshotNearHeapLimit",
            "startCpuProfile",
            "startupSnapshot",
            "stopCoverage",
            "takeCoverage",
            "writeHeapSnapshot",
        ],
        "worker_threads" => &[
            "MessageChannel",
            "MessagePort",
            "Worker",
            "isMainThread",
            "parentPort",
            "workerData",
        ],
        "zlib" => &[
            "BrotliCompress",
            "BrotliDecompress",
            "Deflate",
            "DeflateRaw",
            "Gunzip",
            "Gzip",
            "Inflate",
            "InflateRaw",
            "Unzip",
            "brotliCompress",
            "brotliCompressSync",
            "brotliDecompress",
            "brotliDecompressSync",
            "constants",
            "createBrotliCompress",
            "createBrotliDecompress",
            "createDeflate",
            "createDeflateRaw",
            "createGunzip",
            "createGzip",
            "createInflate",
            "createInflateRaw",
            "createUnzip",
            "deflate",
            "deflateRaw",
            "deflateRawSync",
            "deflateSync",
            "gunzip",
            "gunzipSync",
            "gzip",
            "gzipSync",
            "inflate",
            "inflateRaw",
            "inflateRawSync",
            "inflateSync",
            "unzip",
            "unzipSync",
        ],
        _ => &[],
    }
}

fn split_package_request(request: &str) -> Option<(&str, &str)> {
    if request.starts_with('@') {
        let mut parts = request.splitn(3, '/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let package_name = &request[..scope.len() + 1 + name.len()];
        let subpath = parts.next().unwrap_or("");
        Some((package_name, subpath))
    } else {
        request.split_once('/').or(Some((request, "")))
    }
}

fn node_modules_direct_candidate_dirs(dir: &str, package_name: &str) -> Vec<String> {
    let mut candidates = HashSet::new();
    candidates.insert(join_guest_path(
        dir,
        &format!("node_modules/{package_name}"),
    ));
    if dir == "/node_modules" || dir.ends_with("/node_modules") {
        candidates.insert(join_guest_path(dir, package_name));
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn resolve_exports_target(
    exports_field: &Value,
    subpath: &str,
    mode: ModuleResolveMode,
) -> Option<String> {
    match exports_field {
        Value::String(value) => (subpath == ".").then(|| value.clone()),
        Value::Array(values) => values
            .iter()
            .find_map(|value| resolve_exports_target(value, subpath, mode)),
        Value::Object(record) => {
            if subpath == "."
                && !record.contains_key(".")
                && !record.keys().any(|key| key.starts_with("./"))
            {
                return resolve_conditional_target(record, mode);
            }
            if let Some(value) = record.get(subpath) {
                return resolve_exports_target(value, ".", mode);
            }
            for (key, value) in record {
                if let Some((prefix, suffix)) = key.split_once('*') {
                    if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                        let wildcard = &subpath[prefix.len()..subpath.len() - suffix.len()];
                        let resolved = resolve_exports_target(value, ".", mode)?;
                        return Some(resolved.replace('*', wildcard));
                    }
                }
            }
            if subpath == "." {
                record
                    .get(".")
                    .and_then(|value| resolve_exports_target(value, ".", mode))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn resolve_conditional_target(
    record: &serde_json::Map<String, Value>,
    mode: ModuleResolveMode,
) -> Option<String> {
    let order: &[&str] = match mode {
        ModuleResolveMode::Import => &["import", "node", "module", "default", "require"],
        ModuleResolveMode::Require => &["require", "node", "default", "import", "module"],
    };
    for key in order {
        if let Some(value) = record.get(*key) {
            if let Some(resolved) = resolve_exports_target(value, ".", mode) {
                return Some(resolved);
            }
        }
    }
    None
}

fn resolve_imports_target(
    imports_field: &Value,
    specifier: &str,
    mode: ModuleResolveMode,
) -> Option<String> {
    match imports_field {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => values
            .iter()
            .find_map(|value| resolve_imports_target(value, specifier, mode)),
        Value::Object(record) => {
            if let Some(value) = record.get(specifier) {
                return resolve_exports_target(value, ".", mode);
            }
            for (key, value) in record {
                if let Some((prefix, suffix)) = key.split_once('*') {
                    if specifier.starts_with(prefix) && specifier.ends_with(suffix) {
                        let wildcard = &specifier[prefix.len()..specifier.len() - suffix.len()];
                        let resolved = resolve_exports_target(value, ".", mode)?;
                        return Some(resolved.replace('*', wildcard));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::fcntl::OFlag;
    use nix::unistd::pipe2;
    use serde_json::Value;
    use std::io::BufRead;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;

    #[test]
    fn javascript_limits_are_read_from_typed_fields_and_env_is_inert() {
        // Misleading env values: a reader that still consulted `AGENTOS_*` would
        // observe these instead of the typed wire limits.
        let env = std::collections::BTreeMap::from([
            (
                String::from("AGENTOS_V8_HEAP_LIMIT_MB"),
                String::from("999999"),
            ),
            (
                String::from("AGENTOS_V8_CPU_TIME_LIMIT_MS"),
                String::from("999999"),
            ),
            (
                String::from("AGENTOS_V8_WALL_CLOCK_LIMIT_MS"),
                String::from("999999"),
            ),
            (
                String::from("AGENTOS_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS"),
                String::from("999999"),
            ),
            (
                String::from(NODE_SYNC_RPC_WAIT_TIMEOUT_MS_ENV),
                String::from("999999"),
            ),
        ]);
        let request = StartJavascriptExecutionRequest {
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js"),
            context_id: String::from("ctx-js"),
            argv: vec![String::from("/entry.mjs")],
            env,
            cwd: std::path::PathBuf::from("/tmp"),
            limits: JavascriptExecutionLimits {
                v8_heap_limit_mb: Some(64),
                sync_rpc_wait_timeout_ms: Some(2_000),
                cpu_time_limit_ms: Some(750),
                wall_clock_limit_ms: Some(500),
                import_cache_materialize_timeout_ms: Some(125),
            },
            wasm_module_bytes: None,
            inline_code: None,
        };

        assert_eq!(
            javascript_heap_limit_mb(&request),
            64,
            "heap must come from the typed wire limit, not AGENTOS_V8_HEAP_LIMIT_MB"
        );
        assert_eq!(
            javascript_sync_rpc_timeout(&request),
            std::time::Duration::from_millis(2_000),
            "sync-rpc wait must come from the typed wire limit, not env"
        );
        assert_eq!(
            javascript_cpu_time_limit_ms(&request),
            750,
            "CPU budget must come from the typed wire limit, not env"
        );
        assert_eq!(
            javascript_wall_clock_limit_ms(&request),
            500,
            "wall-clock budget must come from the typed wire limit, not env"
        );
        assert_eq!(
            javascript_import_cache_materialize_timeout(&request),
            std::time::Duration::from_millis(125),
            "import-cache timeout must come from the typed wire limit, not env"
        );
    }

    #[test]
    fn javascript_limits_fall_back_to_defaults_when_unset() {
        let request = StartJavascriptExecutionRequest {
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js"),
            context_id: String::from("ctx-js"),
            argv: vec![String::from("/entry.mjs")],
            env: std::collections::BTreeMap::new(),
            cwd: std::path::PathBuf::from("/tmp"),
            limits: JavascriptExecutionLimits::default(),
            wasm_module_bytes: None,
            inline_code: None,
        };

        assert_eq!(
            javascript_heap_limit_mb(&request),
            0,
            "0 selects the engine default heap"
        );
        assert_eq!(
            javascript_sync_rpc_timeout(&request),
            std::time::Duration::from_millis(NODE_SYNC_RPC_DEFAULT_WAIT_TIMEOUT_MS),
        );
        assert_eq!(
            javascript_cpu_time_limit_ms(&request),
            DEFAULT_V8_CPU_TIME_LIMIT_MS
        );
        assert_eq!(
            javascript_wall_clock_limit_ms(&request),
            DEFAULT_V8_WALL_CLOCK_LIMIT_MS
        );
        assert_eq!(
            javascript_import_cache_materialize_timeout(&request),
            std::time::Duration::from_millis(DEFAULT_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS)
        );
    }

    #[test]
    fn inline_code_module_detection_prefers_commonjs_when_import_only_appears_in_comment() {
        let source = "// import { x } from 'y';\nmodule.exports = { foo: 1 };";
        assert!(!inline_code_uses_module_mode(source));
    }

    #[test]
    fn inline_code_module_detection_ignores_import_inside_string_literal() {
        let source = "const msg = \"run: import x from 'y'\";\nmodule.exports.msg = msg;";
        assert!(!inline_code_uses_module_mode(source));
    }

    #[test]
    fn inline_code_module_detection_accepts_multiline_import_statements() {
        let source = "import\n  { default as foo }\nfrom 'bar';\nconsole.log(foo);";
        assert!(inline_code_uses_module_mode(source));
    }

    #[test]
    fn inline_code_module_detection_accepts_real_esm_source() {
        let source = "import { foo } from 'bar';\nexport const baz = 1;\nconsole.log(foo, baz);";
        assert!(inline_code_uses_module_mode(source));
    }

    #[test]
    fn inline_code_module_detection_is_deterministic_for_empty_comment_only_and_template_cases() {
        assert!(!inline_code_uses_module_mode(""));
        assert!(!inline_code_uses_module_mode(
            "// import x from 'y';\n/* export const z = 1; */"
        ));
        assert!(!inline_code_uses_module_mode(
            "const msg = `export const nope = 1;`;"
        ));
    }

    #[test]
    fn javascript_sync_rpc_timeout_writes_clear_error_response() {
        let (reader_fd, writer_fd) = pipe2(OFlag::O_CLOEXEC).expect("create pipe");
        let reader = File::from(reader_fd);
        let writer = File::from(writer_fd);
        let response_writer =
            JavascriptSyncRpcResponseWriter::new(writer, Duration::from_millis(50));
        let pending = Arc::new(Mutex::new(Some(PendingSyncRpcState::Pending(7))));

        spawn_javascript_sync_rpc_timeout(
            7,
            Duration::from_millis(20),
            pending.clone(),
            Some(response_writer),
        );

        let mut line = String::new();
        let mut reader = BufReader::new(reader);
        reader.read_line(&mut line).expect("read timeout response");

        let response: Value = serde_json::from_str(line.trim()).expect("parse timeout response");
        assert_eq!(response["id"], Value::from(7));
        assert_eq!(response["ok"], Value::from(false));
        assert_eq!(
            response["error"]["code"],
            Value::String(String::from("ERR_AGENTOS_NODE_SYNC_RPC_TIMEOUT"))
        );
        assert!(response["error"]["message"]
            .as_str()
            .expect("timeout message")
            .contains("timed out after 20ms"));
        assert_eq!(
            *pending.lock().expect("pending state lock"),
            Some(PendingSyncRpcState::TimedOut(7))
        );
    }

    #[test]
    fn javascript_sync_rpc_response_writer_times_out_when_queue_is_full() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let writer = JavascriptSyncRpcResponseWriter {
            sender,
            timeout: Duration::from_millis(30),
        };

        writer
            .send(b"first\n".to_vec())
            .expect("queue first response");

        let started = Instant::now();
        let error = writer
            .send(b"second\n".to_vec())
            .expect_err("full queue should time out");
        assert!(
            started.elapsed() >= Duration::from_millis(30),
            "send should wait for the configured timeout"
        );
        assert!(error
            .to_string()
            .contains("timed out after 30ms while queueing JavaScript sync RPC response"));
    }

    #[test]
    fn javascript_wait_capture_rejects_output_over_limit() {
        let mut stdout = vec![b'x'; JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES - 1];
        append_captured_output(&mut stdout, vec![b'y'], "stdout").expect("fill to limit");
        assert_eq!(stdout.len(), JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES);

        let error = append_captured_output(&mut stdout, vec![b'z'], "stdout")
            .expect_err("captured output over limit should fail");
        assert!(matches!(
            error,
            JavascriptExecutionError::OutputBufferExceeded {
                stream: "stdout",
                limit: JAVASCRIPT_CAPTURED_OUTPUT_LIMIT_BYTES,
            }
        ));
    }

    #[test]
    fn kernel_stdin_bridge_rejects_buffer_over_limit_and_closed_writes() {
        let bridge = LocalKernelStdinBridge::default();
        bridge
            .write(&vec![b'x'; KERNEL_STDIN_BUFFER_LIMIT_BYTES])
            .expect("fill stdin buffer to limit");

        let error = bridge
            .write(b"y")
            .expect_err("stdin buffer over limit should fail");
        assert!(matches!(error, JavascriptExecutionError::Stdin(_)));

        let bridge = LocalKernelStdinBridge::default();
        bridge.close();
        let error = bridge
            .write(b"x")
            .expect_err("write after stdin close should fail");
        assert!(matches!(error, JavascriptExecutionError::StdinClosed));
    }

    #[test]
    fn javascript_event_sender_reports_closed_receiver() {
        let (sender, receiver) = channel(1);
        drop(receiver);
        let host = V8RuntimeHost::spawn().expect("spawn V8 runtime host");
        let session = host.session_handle(String::from("closed-event-sender-test"));
        let gauge = register_queue(TrackedLimit::JavascriptEventChannel, 1);
        assert!(!send_javascript_event(
            &sender,
            &session,
            &gauge,
            None,
            JavascriptExecutionEvent::Exited(1)
        ));
    }

    // Regression: a full event channel must apply backpressure, not destroy the
    // session. The old code called `v8_session.destroy()` on the first `Full`,
    // truncating the stream and tearing the session down.
    #[test]
    fn javascript_event_sender_backpressures_instead_of_destroying_when_full() {
        let host = V8RuntimeHost::spawn().expect("spawn V8 runtime host");
        let session_id = format!(
            "event-overflow-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let _session_receiver = host
            .register_session(&session_id)
            .expect("register event overflow session");
        let session = host.session_handle(session_id.clone());
        let gauge = register_queue(TrackedLimit::JavascriptEventChannel, 1);
        let (sender, mut event_receiver) = channel(1);

        // Drain slowly on another thread so the producer is forced onto the
        // blocking-backpressure path the old destroy-on-full code never reached.
        let drainer = std::thread::spawn(move || {
            let mut drained = 0usize;
            while event_receiver.blocking_recv().is_some() {
                drained += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            drained
        });

        // Far more events than the 1-slot channel holds; every send must succeed.
        const SENDS: usize = 16;
        for _ in 0..SENDS {
            assert!(send_javascript_event(
                &sender,
                &session,
                &gauge,
                None,
                JavascriptExecutionEvent::Stdout(Vec::new())
            ));
        }
        drop(sender);
        let drained = drainer.join().expect("drainer thread panicked");
        assert_eq!(drained, SENDS, "every event must survive backpressure");

        // The session must NOT have been destroyed: it is still registered, so a
        // re-registration attempt fails.
        host.register_session(&session_id)
            .expect_err("session must survive backpressure (not be destroyed)");
        host.unregister_session(&session_id);
    }

    #[test]
    fn javascript_event_sender_destroys_session_when_event_is_oversized() {
        let host = V8RuntimeHost::spawn().expect("spawn V8 runtime host");
        let session_id = format!(
            "event-oversized-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let receiver = host
            .register_session(&session_id)
            .expect("register oversized event session");
        let session = host.session_handle(session_id.clone());
        let (sender, _event_receiver) = channel(JAVASCRIPT_EVENT_CHANNEL_CAPACITY);
        let gauge = register_queue(
            TrackedLimit::JavascriptEventChannel,
            JAVASCRIPT_EVENT_CHANNEL_CAPACITY,
        );

        assert!(!send_javascript_event(
            &sender,
            &session,
            &gauge,
            None,
            JavascriptExecutionEvent::Stdout(vec![0; JAVASCRIPT_EVENT_PAYLOAD_LIMIT_BYTES + 1])
        ));

        drop(receiver);
        let recovered = host
            .register_session(&session_id)
            .expect("oversized event should destroy and deregister the session");
        drop(recovered);
        host.unregister_session(&session_id);
    }

    #[test]
    fn internal_bridge_host_context_resolves_relative_module_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "secure-exec-module-bridge-{}-{unique}",
            std::process::id()
        ));
        let bin_dir = root.join("node_modules/next/dist/bin");
        let cli_dir = root.join("node_modules/next/dist/cli");
        fs::create_dir_all(&bin_dir).expect("create bin dir");
        fs::create_dir_all(&cli_dir).expect("create cli dir");
        fs::write(
            root.join("node_modules/next/package.json"),
            r#"{"name":"next"}"#,
        )
        .expect("write package.json");
        fs::write(bin_dir.join("next"), "#!/usr/bin/env node\n").expect("write next bin");
        fs::write(cli_dir.join("next-build.js"), "module.exports = 1;\n")
            .expect("write next-build.js");

        let env = BTreeMap::new();
        let result = handle_internal_bridge_call_from_host_context(
            &root,
            "/",
            &env,
            "_resolveModule",
            &[
                Value::String(String::from("../cli/next-build.js")),
                Value::String(String::from("/node_modules/next/dist/bin/next")),
                Value::String(String::from("import")),
            ],
        );

        assert_eq!(
            result,
            Some(Value::String(String::from(
                "/node_modules/next/dist/cli/next-build.js"
            )))
        );

        fs::remove_dir_all(&root).expect("remove temp module tree");
    }

    #[test]
    fn register_v8_session_deregisters_on_create_session_failure() {
        let host = V8RuntimeHost::spawn().expect("spawn V8 runtime host");
        let session_id = format!(
            "v8-register-failure-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );

        let error =
            match register_v8_session(&host, session_id.clone(), 0, 0, 0, None, |_command| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "simulated CreateSession send failure",
                ))
            }) {
                Ok(_) => panic!("register_v8_session should surface create-session send failures"),
                Err(error) => error,
            };

        match error {
            JavascriptExecutionError::Spawn(inner) => {
                assert_eq!(inner.kind(), std::io::ErrorKind::BrokenPipe);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        let receiver = host
            .register_session(&session_id)
            .expect("failed registration should not leak the session output receiver");
        drop(receiver);
        host.unregister_session(&session_id);
    }

    #[test]
    fn javascript_cpu_time_limit_defaults_to_bounded_value() {
        let request = StartJavascriptExecutionRequest {
            limits: Default::default(),
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js-default-cpu"),
            context_id: String::from("ctx-js-default-cpu"),
            argv: vec![String::from("./entry.mjs")],
            env: BTreeMap::new(),
            cwd: std::path::PathBuf::from("/tmp"),
            wasm_module_bytes: None,
            inline_code: None,
        };

        assert_eq!(
            javascript_cpu_time_limit_ms(&request),
            30_000,
            "unset JavaScript CPU budget must be bounded by default"
        );
    }

    #[test]
    fn javascript_execution_drop_keeps_normal_v8_session_cleanup() {
        let temp = tempdir().expect("create temp dir");
        let mut engine = JavascriptExecutionEngine::default();
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-drop-cleanup"),
            bootstrap_module: None,
            compile_cache_root: None,
        });

        let execution = engine
            .start_execution(StartJavascriptExecutionRequest {
                limits: Default::default(),
                guest_runtime: Default::default(),
                vm_id: String::from("vm-drop-cleanup"),
                context_id: context.context_id,
                argv: vec![String::from("./entry.mjs")],
                env: BTreeMap::new(),
                cwd: temp.path().to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some(String::from("globalThis.__agentOSDropCleanup = true;")),
            })
            .expect("start JavaScript execution");
        let session_id = execution.v8_session.session_id().to_owned();
        let host = engine.v8_host.as_ref().expect("shared V8 runtime host");

        drop(execution);

        let receiver = host
            .register_session(&session_id)
            .expect("execution drop should still destroy and deregister the session");
        drop(receiver);
        host.unregister_session(&session_id);
    }

    // --- Timer cancellation / cap regression tests (U4: H2 bridge timers, M3
    // kernel timers). These assert the *safeguards firing* (delay clamped, timer
    // entry reclaimed, callback suppressed) and never spawn unbounded threads. ---

    #[test]
    fn timer_delay_is_clamped_to_the_cap() {
        // A guest can pass an arbitrarily large delay (up to u64::MAX ms); without
        // a cap the timer wheel could retain a session Arc behind a deadline that
        // is effectively forever away. The cap bounds that lifetime.
        assert_eq!(
            timer_delay_ms(Some(&json!(u64::MAX))),
            MAX_TIMER_DELAY_MS,
            "a u64::MAX delay must be clamped to MAX_TIMER_DELAY_MS"
        );
        assert_eq!(
            timer_delay_ms(Some(&json!(1.0e308_f64))),
            MAX_TIMER_DELAY_MS,
            "an enormous float delay must be clamped to the cap"
        );
        assert_eq!(
            timer_delay_ms(Some(&json!(MAX_TIMER_DELAY_MS + 1))),
            MAX_TIMER_DELAY_MS,
            "a delay one past the cap must clamp down to the cap"
        );
        // Below-cap values pass through unchanged so normal timers are unaffected.
        assert_eq!(timer_delay_ms(Some(&json!(250))), 250);
        assert_eq!(timer_delay_ms(Some(&json!(0))), 0);
    }

    #[test]
    fn cleared_timer_is_suppressed_and_entry_reclaimed() {
        // Mirrors what a woken bridge/kernel timer action does after waiting: it
        // consults the shared map via `timer_should_fire`. When the entry has been
        // removed (clear or session teardown), the callback must be suppressed.
        let timers: Arc<Mutex<HashMap<u64, LocalTimerEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));
        timers.lock().unwrap().insert(
            7,
            LocalTimerEntry {
                delay_ms: 1_000,
                generation: 0,
                repeat: false,
            },
        );

        // Simulate `kernelTimerClear` / teardown removing the entry before the
        // action wakes.
        timers.lock().unwrap().remove(&7);

        assert!(
            !timer_should_fire(&timers, 7, 0),
            "a cleared timer must not fire"
        );
        assert!(
            timers.lock().unwrap().is_empty(),
            "tracking map stays empty after a cleared timer is evaluated"
        );
    }

    #[test]
    fn rearmed_timer_generation_mismatch_suppresses_stale_action() {
        // The bridge/kernel timer action captures the generation at schedule time.
        // If the timer is re-armed (generation bumped) before the stale action
        // wakes, the stale action must observe the mismatch and suppress, while the
        // entry survives for the live generation.
        let timers: Arc<Mutex<HashMap<u64, LocalTimerEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));
        timers.lock().unwrap().insert(
            3,
            LocalTimerEntry {
                delay_ms: 10,
                generation: 1,
                repeat: false,
            },
        );

        // Stale action captured generation 0; current entry is at generation 1.
        assert!(
            !timer_should_fire(&timers, 3, 0),
            "a stale generation must be suppressed"
        );
        assert!(
            timers.lock().unwrap().contains_key(&3),
            "the live entry must survive a stale-generation evaluation"
        );

        // The matching (current) generation fires and reclaims the one-shot entry.
        assert!(
            timer_should_fire(&timers, 3, 1),
            "the current generation must fire"
        );
        assert!(
            timers.lock().unwrap().is_empty(),
            "a fired one-shot timer must reclaim its id from the map"
        );
    }

    #[test]
    fn bridge_timer_registration_is_tracked_and_drop_clears_timers() {
        // H2: the bridge-timer path must register its timer (so it is cancellable)
        // before queuing a wheel action, and session teardown
        // (dropping LocalBridgeState) must wipe the tracking map so in-flight timer
        // actions are cancelled.
        let mut state = LocalBridgeState::default();
        // Observe the same map the queued actions would consult.
        let timers = state.timers.clone();

        let id_a = state.register_oneshot_timer(MAX_TIMER_DELAY_MS);
        let id_b = state.register_oneshot_timer(500);
        assert_ne!(id_a, id_b, "each bridge timer gets a fresh id");
        assert_eq!(
            timers.lock().unwrap().len(),
            2,
            "registered bridge timers are tracked in the shared map"
        );
        // A registered timer would fire for its captured generation (proving the
        // entry is real and consultable) ...
        assert!(timer_should_fire(&timers, id_a, 0));
        // ... and seeding a still-pending one before teardown:
        let id_c = state.register_oneshot_timer(1_000);

        // Session teardown: dropping the bridge state must clear every timer so any
        // queued action wakes to a missing entry and suppresses its callback.
        drop(state);

        assert!(
            timers.lock().unwrap().is_empty(),
            "dropping LocalBridgeState must clear the timers map on teardown"
        );
        assert!(
            !timer_should_fire(&timers, id_c, 0),
            "a pending bridge timer is suppressed after teardown"
        );
    }
}
