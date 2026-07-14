//! Shared state types used across sidecar domain modules.
//!
//! Contains VM state, session state, configuration types, active process/socket
//! types, and other shared data structures extracted from service.rs.

use crate::protocol::{
    GuestRuntimeKind, MountDescriptor, ProjectedModuleDescriptor, RegisterHostCallbacksRequest,
    SidecarRequestFrame, SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload,
    SignalHandlerRegistration, SoftwareDescriptor, WasmPermissionTier,
};
use crate::wire::DEFAULT_MAX_FRAME_BYTES;
use agentos_bridge::{BridgeTypes, FilesystemSnapshot};
use agentos_execution::{
    v8_host::V8SessionHandle, JavascriptExecution, JavascriptSyncRpcRequest, PythonExecution,
    PythonVfsRpcRequest, WasmExecution,
};
use agentos_kernel::kernel::{KernelProcessHandle, KernelVm};
use agentos_kernel::mount_table::MountTable;
use agentos_kernel::pty::Termios;
use agentos_kernel::root_fs::RootFilesystemMode;
use agentos_kernel::socket_table::SocketId;
use agentos_native_sidecar_core::VmLayerStore;
use agentos_vm_config as vm_config;
use agentos_vm_config::PermissionsPolicy;
use rusqlite::Connection;
use rustls::{ClientConnection, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

pub(crate) type BridgeError<B> = <B as BridgeTypes>::Error;
pub(crate) type SidecarKernel = KernelVm<MountTable>;
pub(crate) type KernelSocketReadinessRegistry =
    Arc<Mutex<BTreeMap<SocketId, KernelSocketReadinessTarget>>>;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub(crate) const EXECUTION_DRIVER_NAME: &str = "agentos-native-sidecar-execution";
pub(crate) const JAVASCRIPT_COMMAND: &str = "node";
pub(crate) const PYTHON_COMMAND: &str = "python";
pub(crate) const WASM_COMMAND: &str = "wasm";
// The Python runtime addresses the whole guest VFS (the kernel enforces fs
// permissions and mount-confinement on every op, identical to what the JS/WASM
// runtimes and `vm.readFile()` see), so the VFS-RPC root is `/`, not a single
// workspace dir.
pub(crate) const PYTHON_VFS_RPC_GUEST_ROOT: &str = "/";
pub(crate) const EXECUTION_SANDBOX_ROOT_ENV: &str = "AGENTOS_SANDBOX_ROOT";
pub(crate) const WASM_STDIO_SYNC_RPC_ENV: &str = "AGENTOS_WASI_STDIO_SYNC_RPC";
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const HOST_REALPATH_MAX_SYMLINK_DEPTH: usize = 40;
pub(crate) const DISPOSE_VM_SIGTERM_GRACE: std::time::Duration =
    std::time::Duration::from_millis(100);
pub(crate) const DISPOSE_VM_SIGKILL_GRACE: std::time::Duration =
    std::time::Duration::from_millis(100);
pub(crate) const VM_DNS_SERVERS_METADATA_KEY: &str = "network.dns.servers";
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const VM_LISTEN_PORT_MIN_METADATA_KEY: &str = "network.listen.port_min";
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const VM_LISTEN_PORT_MAX_METADATA_KEY: &str = "network.listen.port_max";
pub(crate) const VM_LISTEN_ALLOW_PRIVILEGED_METADATA_KEY: &str = "network.listen.allow_privileged";
pub(crate) const DEFAULT_JAVASCRIPT_NET_BACKLOG: u32 = 511;
pub(crate) const LOOPBACK_EXEMPT_PORTS_ENV: &str = "AGENTOS_LOOPBACK_EXEMPT_PORTS";
pub(crate) const TOOL_DRIVER_NAME: &str = "secure-exec-host-callbacks";
pub(crate) const MAPPED_HOST_FD_START: u32 = 1_000_000_000;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NativeSidecarConfig {
    pub sidecar_id: String,
    pub max_frame_bytes: usize,
    pub compile_cache_root: Option<PathBuf>,
    pub expected_auth_token: Option<String>,
    pub acp_termination_grace: Duration,
}

impl Default for NativeSidecarConfig {
    fn default() -> Self {
        Self {
            sidecar_id: String::from("agentos-native-sidecar"),
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            compile_cache_root: None,
            expected_auth_token: None,
            acp_termination_grace: Duration::from_secs(3),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarError {
    InvalidState(String),
    ProtocolVersionMismatch(String),
    BridgeVersionMismatch(String),
    Conflict(String),
    Unauthorized(String),
    Unsupported(String),
    FrameTooLarge(String),
    Kernel(String),
    Plugin(String),
    Execution(String),
    Bridge(String),
    Io(String),
}

impl fmt::Display for SidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(message)
            | Self::ProtocolVersionMismatch(message)
            | Self::BridgeVersionMismatch(message)
            | Self::Conflict(message)
            | Self::Unauthorized(message)
            | Self::Unsupported(message)
            | Self::FrameTooLarge(message)
            | Self::Kernel(message)
            | Self::Plugin(message)
            | Self::Execution(message)
            | Self::Bridge(message)
            | Self::Io(message) => f.write_str(message),
        }
    }
}

impl Error for SidecarError {}

pub trait SidecarRequestTransport: Send + Sync {
    fn send_request(
        &self,
        request: SidecarRequestFrame,
        timeout: Duration,
    ) -> Result<SidecarResponseFrame, SidecarError>;
}

#[derive(Clone)]
pub(crate) struct SharedSidecarRequestClient {
    transport: Option<Arc<dyn SidecarRequestTransport>>,
    next_request_id: Arc<AtomicI64>,
}

impl Default for SharedSidecarRequestClient {
    fn default() -> Self {
        Self {
            transport: None,
            next_request_id: Arc::new(AtomicI64::new(-1)),
        }
    }
}

impl SharedSidecarRequestClient {
    pub(crate) fn set_transport(&mut self, transport: Arc<dyn SidecarRequestTransport>) {
        self.transport = Some(transport);
    }

    pub(crate) fn invoke(
        &self,
        ownership: crate::protocol::OwnershipScope,
        payload: SidecarRequestPayload,
        timeout: Duration,
    ) -> Result<SidecarResponsePayload, SidecarError> {
        let transport = self.transport.as_ref().ok_or_else(|| {
            SidecarError::Unsupported(String::from("sidecar request transport is not configured"))
        })?;
        let request_id = self.next_request_id.fetch_sub(1, Ordering::Relaxed);
        let request = SidecarRequestFrame::new(request_id, ownership.clone(), payload);
        let response = transport.send_request(request, timeout)?;
        if response.request_id != request_id {
            return Err(SidecarError::InvalidState(format!(
                "sidecar response {} did not match request {request_id}",
                response.request_id
            )));
        }
        if response.ownership != ownership {
            return Err(SidecarError::InvalidState(String::from(
                "sidecar response ownership did not match request ownership",
            )));
        }
        Ok(response.payload)
    }
}

/// Fire-and-forget live event sink. Lets an extension emit a `session/update`
/// (or any other) event frame to the host *mid-dispatch*, instead of having to
/// return it from the dispatch and wait for the whole request to resolve before
/// the stdio loop flushes it. Mirrors `SidecarRequestTransport`, but events have
/// no response, no request id, and no timeout — they are written to the same
/// outbound stdout channel the batch path uses.
pub trait EventSinkTransport: Send + Sync {
    fn emit_event(&self, event: crate::wire::EventFrame) -> Result<(), SidecarError>;
}

#[derive(Clone, Default)]
pub(crate) struct SharedEventSink {
    transport: Option<Arc<dyn EventSinkTransport>>,
}

impl SharedEventSink {
    pub(crate) fn set_transport(&mut self, transport: Arc<dyn EventSinkTransport>) {
        self.transport = Some(transport);
    }

    /// Emit `event` live if a transport is configured (the stdio path). Returns
    /// `Ok(None)` when the event was handed to the live transport, or
    /// `Ok(Some(event))` when no transport is configured (e.g. an in-process
    /// `NativeSidecar` with no stdout loop) so the caller can fall back to the
    /// batch path and still deliver the event when the dispatch resolves.
    pub(crate) fn try_emit(
        &self,
        event: crate::wire::EventFrame,
    ) -> Result<Option<crate::wire::EventFrame>, SidecarError> {
        match self.transport.as_ref() {
            Some(transport) => {
                transport.emit_event(event)?;
                Ok(None)
            }
            None => Ok(Some(event)),
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge wrapper
// ---------------------------------------------------------------------------

pub(crate) struct SharedBridge<B> {
    pub(crate) inner: Arc<Mutex<B>>,
    pub(crate) permissions: Arc<Mutex<BTreeMap<String, PermissionsPolicy>>>,
    #[cfg(test)]
    pub(crate) set_vm_permissions_outcomes: Arc<Mutex<VecDeque<Option<SidecarError>>>>,
}

impl<B> Clone for SharedBridge<B> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            permissions: Arc::clone(&self.permissions),
            #[cfg(test)]
            set_vm_permissions_outcomes: Arc::clone(&self.set_vm_permissions_outcomes),
        }
    }
}

// ---------------------------------------------------------------------------
// Connection / session / VM state
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ConnectionState {
    pub(crate) auth_token: String,
    pub(crate) sessions: BTreeSet<String>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct SessionState {
    pub(crate) connection_id: String,
    pub(crate) placement: crate::protocol::SidecarPlacement,
    pub(crate) metadata: BTreeMap<String, String>,
    pub(crate) vm_ids: BTreeSet<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct VmConfiguration {
    pub(crate) mounts: Vec<MountDescriptor>,
    pub(crate) software: Vec<SoftwareDescriptor>,
    pub(crate) permissions: PermissionsPolicy,
    pub(crate) module_access_cwd: Option<String>,
    pub(crate) instructions: Vec<String>,
    pub(crate) projected_modules: Vec<ProjectedModuleDescriptor>,
    pub(crate) command_permissions: BTreeMap<String, WasmPermissionTier>,
    pub(crate) provided_commands: BTreeMap<String, Vec<String>>,
    /// Guest JavaScript host-environment config (platform / module resolution /
    /// builtin allow-list). Set at `create_vm` from `CreateVmConfig.jsRuntime`
    /// and preserved across `configure_vm`. `None` => full Node.js emulation.
    pub(crate) js_runtime: Option<vm_config::JsRuntimeConfig>,
    /// Agent SDK bundle read by the sidecar from the configured package dir and
    /// evaluated into the shared V8 startup snapshot.
    pub(crate) snapshot_userland_code: Option<String>,
    pub(crate) loopback_exempt_ports: Vec<u16>,
}

impl Default for VmConfiguration {
    fn default() -> Self {
        Self {
            mounts: Vec::new(),
            software: Vec::new(),
            permissions: agentos_native_sidecar_core::permissions::deny_all_policy(),
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: BTreeMap::new(),
            provided_commands: BTreeMap::new(),
            js_runtime: None,
            snapshot_userland_code: None,
            loopback_exempt_ports: Vec::new(),
        }
    }
}

#[allow(dead_code)]
pub(crate) struct VmState {
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    /// Operator-tunable VM-scoped runtime limits. Immutable for the VM's lifetime;
    /// `ConfigureVm` does not mutate limits.
    pub(crate) limits: crate::limits::VmLimits,
    pub(crate) dns: VmDnsConfig,
    pub(crate) listen_policy: VmListenPolicy,
    pub(crate) create_loopback_exempt_ports: BTreeSet<u16>,
    pub(crate) guest_env: BTreeMap<String, String>,
    pub(crate) requested_runtime: GuestRuntimeKind,
    pub(crate) root_filesystem_mode: RootFilesystemMode,
    pub(crate) guest_cwd: String,
    pub(crate) cwd: PathBuf,
    pub(crate) host_cwd: PathBuf,
    pub(crate) kernel: SidecarKernel,
    pub(crate) kernel_socket_readiness: KernelSocketReadinessRegistry,
    pub(crate) loaded_snapshot: Option<FilesystemSnapshot>,
    pub(crate) configuration: VmConfiguration,
    pub(crate) layers: VmLayerStore,
    pub(crate) command_guest_paths: BTreeMap<String, String>,
    pub(crate) provided_commands: BTreeMap<String, Vec<String>>,
    pub(crate) command_permissions: BTreeMap<String, WasmPermissionTier>,
    pub(crate) toolkits: BTreeMap<String, RegisterHostCallbacksRequest>,
    pub(crate) active_processes: BTreeMap<String, ActiveProcess>,
    pub(crate) exited_process_snapshots: VecDeque<ExitedProcessSnapshot>,
    pub(crate) detached_child_processes: BTreeSet<String>,
    pub(crate) signal_states: BTreeMap<String, BTreeMap<u32, SignalHandlerRegistration>>,
    /// Legacy staging root slot retained for same-version internal state shape.
    /// The current `/opt/agentos` projection mounts package tars and synthetic
    /// symlink leaves directly, so this remains `None`.
    pub(crate) packages_staging_root: Option<PathBuf>,
    /// Projected agent launch surface, keyed by agent id. Sourced from the
    /// packed vbare manifests at `ConfigureVm`/`LinkPackage` time — packed
    /// packages ship no `agentos-package.json`, so agent enumeration and
    /// resolution read this instead of the guest filesystem.
    pub(crate) projected_agent_launch: BTreeMap<String, ProjectedAgentLaunch>,
}

/// Launch parameters for one projected agent package.
#[derive(Debug, Clone)]
pub(crate) struct ProjectedAgentLaunch {
    pub(crate) acp_entrypoint: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) launch_args: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExitedProcessSnapshot {
    pub(crate) captured_at: Instant,
    pub(crate) process: crate::protocol::ProcessSnapshotEntry,
}

// ---------------------------------------------------------------------------
// DNS configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct VmDnsConfig {
    pub(crate) name_servers: Vec<SocketAddr>,
    pub(crate) overrides: BTreeMap<String, Vec<IpAddr>>,
}

#[derive(Debug, Clone)]
pub(crate) struct JavascriptSocketPathContext {
    pub(crate) sandbox_root: PathBuf,
    pub(crate) mounts: Vec<MountDescriptor>,
    pub(crate) listen_policy: VmListenPolicy,
    pub(crate) loopback_exempt_ports: BTreeSet<u16>,
    pub(crate) tcp_loopback_guest_to_host_ports: BTreeMap<(JavascriptSocketFamily, u16), u16>,
    pub(crate) http_loopback_targets:
        BTreeMap<(JavascriptSocketFamily, u16), JavascriptHttpLoopbackTarget>,
    pub(crate) udp_loopback_guest_to_host_ports: BTreeMap<(JavascriptSocketFamily, u16), u16>,
    pub(crate) udp_loopback_host_to_guest_ports: BTreeMap<(JavascriptSocketFamily, u16), u16>,
    pub(crate) used_tcp_guest_ports: BTreeMap<JavascriptSocketFamily, BTreeSet<u16>>,
    pub(crate) used_udp_guest_ports: BTreeMap<JavascriptSocketFamily, BTreeSet<u16>>,
}

#[derive(Debug, Clone)]
pub(crate) struct JavascriptHttpLoopbackTarget {
    pub(crate) process_id: String,
    pub(crate) server_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum JavascriptSocketFamily {
    Ipv4,
    Ipv6,
}

impl JavascriptSocketFamily {
    pub(crate) fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::Ipv4,
            IpAddr::V6(_) => Self::Ipv6,
        }
    }
}

impl From<JavascriptUdpFamily> for JavascriptSocketFamily {
    fn from(value: JavascriptUdpFamily) -> Self {
        match value {
            JavascriptUdpFamily::Ipv4 => Self::Ipv4,
            JavascriptUdpFamily::Ipv6 => Self::Ipv6,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VmListenPolicy {
    pub(crate) port_min: u16,
    pub(crate) port_max: u16,
    pub(crate) allow_privileged: bool,
}

impl Default for VmListenPolicy {
    fn default() -> Self {
        Self {
            port_min: 1,
            port_max: u16::MAX,
            allow_privileged: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Active process state
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(crate) struct ActiveProcess {
    pub(crate) kernel_pid: u32,
    pub(crate) kernel_handle: KernelProcessHandle,
    pub(crate) kernel_stdin_writer_fd: Option<u32>,
    /// For a TTY (PTY-backed) process, the master-end fd whose output buffer
    /// carries cooked-mode echo plus ONLCR-processed guest output. When set,
    /// this master output is the single ordered output stream surfaced to the
    /// host (instead of the raw guest stdout/stderr execution events).
    pub(crate) tty_master_fd: Option<u32>,
    pub(crate) runtime: GuestRuntimeKind,
    pub(crate) detached: bool,
    pub(crate) execution: ActiveExecution,
    pub(crate) guest_cwd: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) host_cwd: PathBuf,
    pub(crate) host_write_dirty: bool,
    pub(crate) mapped_host_fds: BTreeMap<u32, ActiveMappedHostFd>,
    pub(crate) next_mapped_host_fd: u32,
    pub(crate) pending_execution_events: VecDeque<ActiveExecutionEvent>,
    pub(crate) pending_self_signal_exit: Option<i32>,
    pub(crate) child_processes: BTreeMap<String, ActiveProcess>,
    pub(crate) next_child_process_id: usize,
    pub(crate) http_servers: BTreeMap<u64, ActiveHttpServer>,
    pub(crate) pending_http_requests: BTreeMap<(u64, u64), Option<String>>,
    pub(crate) http2: ActiveHttp2State,
    pub(crate) tcp_listeners: BTreeMap<String, ActiveTcpListener>,
    pub(crate) next_tcp_listener_id: usize,
    pub(crate) tcp_sockets: BTreeMap<String, ActiveTcpSocket>,
    pub(crate) next_tcp_socket_id: usize,
    pub(crate) tcp_port_reservations: BTreeMap<String, (JavascriptSocketFamily, u16)>,
    pub(crate) next_tcp_port_reservation_id: usize,
    pub(crate) unix_listeners: BTreeMap<String, ActiveUnixListener>,
    pub(crate) next_unix_listener_id: usize,
    pub(crate) unix_sockets: BTreeMap<String, ActiveUnixSocket>,
    pub(crate) next_unix_socket_id: usize,
    pub(crate) udp_sockets: BTreeMap<String, ActiveUdpSocket>,
    pub(crate) next_udp_socket_id: usize,
    /// Synchronous host sockets opened by the guest Python `socket` bridge,
    /// keyed by the handle returned to the runner. Distinct from the JS
    /// runtime's event-driven `tcp_sockets`/`udp_sockets` above.
    pub(crate) python_sockets: BTreeMap<u64, PythonHostSocket>,
    pub(crate) next_python_socket_id: u64,
    pub(crate) cipher_sessions: BTreeMap<u64, ActiveCipherSession>,
    pub(crate) next_cipher_session_id: u64,
    pub(crate) diffie_hellman_sessions: BTreeMap<u64, ActiveDiffieHellmanSession>,
    pub(crate) next_diffie_hellman_session_id: u64,
    pub(crate) sqlite_databases: BTreeMap<u64, ActiveSqliteDatabase>,
    pub(crate) next_sqlite_database_id: u64,
    pub(crate) sqlite_statements: BTreeMap<u64, ActiveSqliteStatement>,
    pub(crate) next_sqlite_statement_id: u64,
    /// For a child process whose stdio is the SHARED terminal (its kernel fd 1
    /// is the same PTY slave as the shell's), the `(kernel pid, master fd)` of
    /// the process that owns the host-facing PTY master. Set at spawn. Such a
    /// child's stdio writes surface ONLY through master drains attributed to
    /// the owner — never as child stdout events — exactly like a native
    /// terminal reading the PTY master (a shell never relays its child's tty
    /// output).
    pub(crate) tty_master_owner: Option<(u32, u32)>,
    /// Terminal attributes inherited at spawn by a child sharing an ancestor's
    /// PTY. Restored when the child is reaped so a full-screen command cannot
    /// strand its caller in raw mode. This is the exact inherited state rather
    /// than an assumed cooked configuration, so nested raw-mode callers remain
    /// raw after their child exits.
    pub(crate) tty_restore_termios: Option<Termios>,
    /// A parked `__kernel_stdin_read` / `__kernel_poll` sync RPC awaiting
    /// kernel readiness (reply-by-token deferral so servicing never blocks the
    /// dispatch loop). At most one per process: the guest thread is blocked in
    /// this RPC, so it cannot issue another. `(request, absolute deadline)`.
    pub(crate) deferred_kernel_wait_rpc: Option<(JavascriptSyncRpcRequest, Instant)>,
    /// Per-process module resolution cache, persisted across module sync-RPCs
    /// (`__resolve_module` / `__load_file` / `__module_format` /
    /// `__batch_resolve_modules`) for the lifetime of this process so cold-start
    /// resolution does not rebuild it on every dispatch. The resolver reads the
    /// kernel VFS; the node_modules tree is mounted read-only, so cached
    /// stat/exists/package.json results under it stay valid for the process run.
    pub(crate) module_resolution_cache: agentos_execution::LocalModuleResolutionCache,
}

pub(crate) struct ActiveMappedHostFd {
    pub(crate) file: File,
    pub(crate) path: PathBuf,
    pub(crate) guest_path: Option<String>,
}

pub(crate) struct ActiveCipherSession {
    pub(crate) context: crate::crypto_cipher::StreamCipherSession,
}

pub(crate) struct ActiveSqliteDatabase {
    pub(crate) connection: Connection,
    pub(crate) host_path: Option<PathBuf>,
    pub(crate) vm_path: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) transaction_depth: usize,
    pub(crate) read_only: bool,
}

#[derive(Clone)]
pub(crate) struct ActiveSqliteStatement {
    pub(crate) database_id: u64,
    pub(crate) sql: String,
    pub(crate) return_arrays: bool,
    pub(crate) read_bigints: bool,
    pub(crate) allow_bare_named_parameters: bool,
    pub(crate) allow_unknown_named_parameters: bool,
}

pub(crate) enum ActiveDiffieHellmanSession {
    Dh(ActiveDhSession),
    Ecdh(ActiveEcdhSession),
}

pub(crate) struct ActiveDhSession {
    pub(crate) params: openssl::dh::Dh<openssl::pkey::Params>,
    pub(crate) key_pair: Option<openssl::dh::Dh<openssl::pkey::Private>>,
}

pub(crate) struct ActiveEcdhSession {
    pub(crate) curve: String,
    pub(crate) key_pair: Option<openssl::ec::EcKey<openssl::pkey::Private>>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NetworkResourceCounts {
    pub(crate) sockets: usize,
    pub(crate) connections: usize,
}

#[derive(Debug)]
pub(crate) struct ActiveHttpServer {
    pub(crate) listener: TcpListener,
    pub(crate) guest_local_addr: SocketAddr,
    pub(crate) next_request_id: u64,
}

#[derive(Clone, Default)]
pub(crate) struct ActiveHttp2State {
    pub(crate) shared: Arc<Mutex<Http2SharedState>>,
}

#[derive(Default)]
pub(crate) struct Http2SharedState {
    pub(crate) next_session_id: u64,
    pub(crate) next_stream_id: u64,
    pub(crate) ready: Arc<Condvar>,
    pub(crate) event_session: Option<V8SessionHandle>,
    pub(crate) servers: BTreeMap<u64, ActiveHttp2Server>,
    pub(crate) sessions: BTreeMap<u64, ActiveHttp2Session>,
    pub(crate) streams: BTreeMap<u64, ActiveHttp2Stream>,
    pub(crate) server_events: BTreeMap<u64, VecDeque<Http2BridgeEvent>>,
    pub(crate) session_events: BTreeMap<u64, VecDeque<Http2BridgeEvent>>,
}

#[derive(Debug)]
pub(crate) struct ActiveHttp2Server {
    pub(crate) actual_local_addr: SocketAddr,
    pub(crate) guest_local_addr: SocketAddr,
    pub(crate) secure: bool,
    pub(crate) tls: Option<JavascriptTlsBridgeOptions>,
    pub(crate) closed: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveHttp2Session {
    pub(crate) command_tx: UnboundedSender<Http2SessionCommand>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveHttp2Stream {
    pub(crate) session_id: u64,
    pub(crate) paused: Arc<AtomicBool>,
    pub(crate) resume_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Http2SocketSnapshot {
    pub(crate) encrypted: bool,
    pub(crate) allow_half_open: bool,
    pub(crate) local_address: Option<String>,
    pub(crate) local_port: Option<u16>,
    pub(crate) local_family: Option<String>,
    pub(crate) remote_address: Option<String>,
    pub(crate) remote_port: Option<u16>,
    pub(crate) remote_family: Option<String>,
    pub(crate) servername: Option<String>,
    pub(crate) alpn_protocol: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Http2RuntimeSnapshot {
    pub(crate) effective_local_window_size: u32,
    pub(crate) local_window_size: u32,
    pub(crate) remote_window_size: u32,
    pub(crate) next_stream_id: u32,
    pub(crate) outbound_queue_size: u32,
    pub(crate) deflate_dynamic_table_size: u32,
    pub(crate) inflate_dynamic_table_size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Http2SessionSnapshot {
    pub(crate) encrypted: bool,
    pub(crate) alpn_protocol: Option<String>,
    pub(crate) origin_set: Vec<String>,
    pub(crate) local_settings: BTreeMap<String, Value>,
    pub(crate) remote_settings: BTreeMap<String, Value>,
    pub(crate) state: Http2RuntimeSnapshot,
    pub(crate) socket: Http2SocketSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Http2BridgeEvent {
    pub(crate) kind: String,
    pub(crate) id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extra: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extra_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extra_headers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) flags: Option<u64>,
}

pub(crate) enum Http2SessionCommand {
    Request {
        headers_json: String,
        options_json: String,
        respond_to: Sender<Result<Value, String>>,
    },
    Settings {
        settings_json: String,
        respond_to: Sender<Result<Value, String>>,
    },
    SetLocalWindowSize {
        size: u32,
        respond_to: Sender<Result<Value, String>>,
    },
    Goaway {
        error_code: u32,
        last_stream_id: u32,
        opaque_data: Option<Vec<u8>>,
        respond_to: Sender<Result<Value, String>>,
    },
    Close {
        abrupt: bool,
        respond_to: Sender<Result<Value, String>>,
    },
    StreamRespond {
        stream_id: u64,
        headers_json: String,
        respond_to: Sender<Result<Value, String>>,
    },
    StreamPush {
        stream_id: u64,
        headers_json: String,
        respond_to: Sender<Result<Value, String>>,
    },
    StreamWrite {
        stream_id: u64,
        chunk: Vec<u8>,
        end_stream: bool,
        respond_to: Sender<Result<Value, String>>,
    },
    StreamClose {
        stream_id: u64,
        error_code: Option<u32>,
        respond_to: Sender<Result<Value, String>>,
    },
    StreamRespondWithFile {
        stream_id: u64,
        body: Vec<u8>,
        headers_json: String,
        options_json: String,
        respond_to: Sender<Result<Value, String>>,
    },
}

// ---------------------------------------------------------------------------
// TCP types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum JavascriptTcpListenerEvent {
    Connection(PendingTcpSocket),
    Error {
        code: Option<String>,
        message: String,
    },
}

#[derive(Debug)]
pub(crate) struct PendingTcpSocket {
    pub(crate) stream: Option<TcpStream>,
    pub(crate) kernel_socket_id: Option<SocketId>,
    pub(crate) preallocated: bool,
    pub(crate) guest_local_addr: SocketAddr,
    pub(crate) guest_remote_addr: SocketAddr,
}

#[derive(Debug)]
pub(crate) enum JavascriptTcpSocketEvent {
    Data(Vec<u8>),
    End,
    Close {
        had_error: bool,
    },
    Error {
        code: Option<String>,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct JavascriptSocketEventPusher {
    pub(crate) session: V8SessionHandle,
    pub(crate) socket_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KernelSocketReadinessEvent {
    Data,
    Datagram,
    Accept,
}

#[derive(Clone, Debug)]
pub(crate) struct KernelSocketReadinessTarget {
    pub(crate) session: V8SessionHandle,
    pub(crate) target_id: String,
    pub(crate) event: KernelSocketReadinessEvent,
}

#[derive(Debug)]
pub(crate) struct ActiveTcpSocket {
    pub(crate) stream: Option<Arc<Mutex<TcpStream>>>,
    pub(crate) pending_read_stream: Option<Arc<Mutex<Option<TcpStream>>>>,
    pub(crate) events: Option<Receiver<JavascriptTcpSocketEvent>>,
    pub(crate) event_sender: Option<Sender<JavascriptTcpSocketEvent>>,
    pub(crate) event_pusher: Arc<Mutex<Option<JavascriptSocketEventPusher>>>,
    pub(crate) kernel_socket_id: Option<SocketId>,
    pub(crate) no_delay: bool,
    pub(crate) keep_alive: bool,
    pub(crate) keep_alive_initial_delay_secs: Option<u64>,
    pub(crate) guest_local_addr: SocketAddr,
    pub(crate) guest_remote_addr: SocketAddr,
    pub(crate) listener_id: Option<String>,
    pub(crate) tls_mode: Arc<AtomicBool>,
    pub(crate) tls_stream: Arc<Mutex<Option<ActiveTlsStream>>>,
    pub(crate) tls_state: Arc<Mutex<Option<ActiveTlsState>>>,
    pub(crate) loopback_tls_pending_write: Arc<Mutex<Option<LoopbackTlsPendingWriteHandle>>>,
    pub(crate) saw_local_shutdown: Arc<AtomicBool>,
    pub(crate) saw_remote_end: Arc<AtomicBool>,
    pub(crate) close_notified: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct LoopbackTlsTransportPair {
    pub(crate) state: Mutex<LoopbackTlsTransportPairState>,
    pub(crate) ready: Condvar,
}

#[derive(Debug, Default)]
pub(crate) struct LoopbackTlsTransportPairState {
    pub(crate) lower_to_higher: VecDeque<u8>,
    pub(crate) higher_to_lower: VecDeque<u8>,
    pub(crate) lower_write_closed: bool,
    pub(crate) higher_write_closed: bool,
    pub(crate) lower_closed: bool,
    pub(crate) higher_closed: bool,
    pub(crate) lower_read_interrupt: bool,
    pub(crate) higher_read_interrupt: bool,
}

pub(crate) struct LoopbackTlsEndpoint {
    pub(crate) pair: Arc<LoopbackTlsTransportPair>,
    pub(crate) is_lower_socket: bool,
    pub(crate) poll_timeout: Duration,
    /// Registry key (`vm_id:lower:higher`) under which this endpoint's transport
    /// pair is registered in the loopback-TLS transport registry. Stored so the
    /// endpoint's `Drop` can eagerly prune its own registry entry once it is the
    /// last owner of the pair, instead of leaking a dead `Weak` entry until the
    /// next lazy `retain()` in `loopback_tls_endpoint()`. `None` means the
    /// endpoint was not registered (e.g. test-constructed) and Drop skips pruning.
    pub(crate) registry_key: Option<String>,
}

#[derive(Debug)]
pub(crate) struct LoopbackTlsPendingWriteState {
    pub(crate) buffer: Vec<u8>,
    pub(crate) warned_near_cap: bool,
    pub(crate) flushing: bool,
    pub(crate) defer_shutdown_write: bool,
    pub(crate) failure_message: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoopbackTlsPendingWriteHandle {
    pub(crate) state: Arc<Mutex<LoopbackTlsPendingWriteState>>,
    pub(crate) tls_handshake_complete: Arc<AtomicBool>,
    pub(crate) failed: Arc<AtomicBool>,
    pub(crate) pair: Arc<LoopbackTlsTransportPair>,
    pub(crate) is_lower_socket: bool,
    pub(crate) handshake_started_at: Instant,
}

impl fmt::Debug for LoopbackTlsEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackTlsEndpoint")
            .field("is_lower_socket", &self.is_lower_socket)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum ActiveTlsStream {
    Client(StreamOwned<ClientConnection, TcpStream>),
    Server(StreamOwned<ServerConnection, TcpStream>),
    LoopbackClient(StreamOwned<ClientConnection, LoopbackTlsEndpoint>),
    LoopbackServer(StreamOwned<ServerConnection, LoopbackTlsEndpoint>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct JavascriptTlsClientHello {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) servername: Option<String>,
    #[serde(
        rename = "ALPNProtocols",
        alias = "ALPNProtocols",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) alpn_protocols: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct JavascriptTlsBridgeOptions {
    pub(crate) is_server: bool,
    pub(crate) servername: Option<String>,
    pub(crate) reject_unauthorized: Option<bool>,
    pub(crate) request_cert: Option<bool>,
    pub(crate) session: Option<String>,
    pub(crate) key: Option<JavascriptTlsMaterial>,
    pub(crate) cert: Option<JavascriptTlsMaterial>,
    pub(crate) ca: Option<JavascriptTlsMaterial>,
    pub(crate) passphrase: Option<String>,
    pub(crate) ciphers: Option<String>,
    #[serde(alias = "ALPNProtocols")]
    pub(crate) alpn_protocols: Option<Vec<String>>,
    pub(crate) min_version: Option<String>,
    pub(crate) max_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum JavascriptTlsMaterial {
    Single(JavascriptTlsDataValue),
    Many(Vec<JavascriptTlsDataValue>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum JavascriptTlsDataValue {
    Buffer { data: String },
    String { data: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveTlsState {
    pub(crate) client_hello: Option<JavascriptTlsClientHello>,
    pub(crate) local_certificates: Vec<Vec<u8>>,
    pub(crate) session_reused: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedTcpConnectAddr {
    pub(crate) actual_addr: SocketAddr,
    pub(crate) guest_remote_addr: SocketAddr,
    pub(crate) use_kernel_loopback: bool,
}

#[derive(Debug)]
pub(crate) struct ActiveTcpListener {
    pub(crate) listener: Option<TcpListener>,
    pub(crate) kernel_socket_id: Option<SocketId>,
    pub(crate) local_addr: Option<SocketAddr>,
    pub(crate) guest_local_addr: SocketAddr,
    pub(crate) backlog: usize,
    pub(crate) active_connection_ids: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Unix socket types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum JavascriptUnixListenerEvent {
    Connection(PendingUnixSocket),
    Error {
        code: Option<String>,
        message: String,
    },
}

#[derive(Debug)]
pub(crate) struct PendingUnixSocket {
    pub(crate) stream: UnixStream,
    pub(crate) local_path: Option<String>,
    pub(crate) remote_path: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ActiveUnixSocket {
    pub(crate) stream: Arc<Mutex<UnixStream>>,
    pub(crate) events: Receiver<JavascriptTcpSocketEvent>,
    pub(crate) event_sender: Sender<JavascriptTcpSocketEvent>,
    pub(crate) event_pusher: Arc<Mutex<Option<JavascriptSocketEventPusher>>>,
    pub(crate) listener_id: Option<String>,
    pub(crate) local_path: Option<String>,
    pub(crate) remote_path: Option<String>,
    pub(crate) saw_local_shutdown: Arc<AtomicBool>,
    pub(crate) saw_remote_end: Arc<AtomicBool>,
    pub(crate) close_notified: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct ActiveUnixListener {
    pub(crate) listener: UnixListener,
    pub(crate) path: String,
    pub(crate) backlog: usize,
    pub(crate) active_connection_ids: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// UDP types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JavascriptUdpFamily {
    Ipv4,
    Ipv6,
}

impl JavascriptUdpFamily {
    pub(crate) fn from_socket_type(value: &str) -> Result<Self, SidecarError> {
        match value {
            "udp4" => Ok(Self::Ipv4),
            "udp6" => Ok(Self::Ipv6),
            other => Err(SidecarError::InvalidState(format!(
                "unsupported dgram socket type {other}"
            ))),
        }
    }

    pub(crate) fn socket_type(self) -> &'static str {
        match self {
            Self::Ipv4 => "udp4",
            Self::Ipv6 => "udp6",
        }
    }

    pub(crate) fn matches_addr(self, addr: &SocketAddr) -> bool {
        matches!(
            (self, addr),
            (Self::Ipv4, SocketAddr::V4(_)) | (Self::Ipv6, SocketAddr::V6(_))
        )
    }
}

#[derive(Debug)]
pub(crate) enum JavascriptUdpSocketEvent {
    Message {
        data: Vec<u8>,
        remote_addr: SocketAddr,
    },
    Error {
        code: Option<String>,
        message: String,
    },
}

/// A blocking host socket backing one guest Python socket. Reads use a short
/// timeout (set on the socket) so a `recv`/`recvfrom` RPC never stalls the
/// shared sidecar event loop; the Python shim re-polls to emulate blocking.
#[derive(Debug)]
pub(crate) enum PythonHostSocket {
    Tcp(TcpStream),
    Udp(UdpSocket),
}

#[derive(Debug)]
pub(crate) struct ActiveUdpSocket {
    pub(crate) family: JavascriptUdpFamily,
    pub(crate) socket: Option<UdpSocket>,
    pub(crate) kernel_socket_id: Option<SocketId>,
    pub(crate) guest_local_addr: Option<SocketAddr>,
    pub(crate) recv_buffer_size: usize,
    pub(crate) send_buffer_size: usize,
}

// ---------------------------------------------------------------------------
// Execution types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum ActiveExecution {
    Javascript(JavascriptExecution),
    Python(PythonExecution),
    Wasm(Box<WasmExecution>),
    Tool(ToolExecution),
}

#[derive(Debug, Clone)]
pub(crate) struct ToolExecution {
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) pending_events: Arc<Mutex<VecDeque<ActiveExecutionEvent>>>,
    pub(crate) events_overflowed: Arc<AtomicBool>,
}

impl Default for ToolExecution {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            pending_events: Arc::new(Mutex::new(VecDeque::new())),
            events_overflowed: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Debug)]
pub(crate) enum ActiveExecutionEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    JavascriptSyncRpcRequest(JavascriptSyncRpcRequest),
    PythonVfsRpcRequest(Box<PythonVfsRpcRequest>),
    SignalState {
        signal: u32,
        registration: SignalHandlerRegistration,
    },
    Exited(i32),
}

#[derive(Debug)]
pub(crate) struct ProcessEventEnvelope {
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    pub(crate) vm_id: String,
    pub(crate) process_id: String,
    pub(crate) event: ActiveExecutionEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SocketQueryKind {
    TcpListener,
    UdpBound,
}

// ---------------------------------------------------------------------------
// Command resolution
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct ResolvedChildProcessExecution {
    pub(crate) command: String,
    pub(crate) process_args: Vec<String>,
    pub(crate) runtime: GuestRuntimeKind,
    pub(crate) entrypoint: String,
    pub(crate) execution_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) guest_cwd: String,
    pub(crate) host_cwd: PathBuf,
    pub(crate) wasm_permission_tier: Option<WasmPermissionTier>,
    pub(crate) tool_command: bool,
}

// ---------------------------------------------------------------------------
// Utility types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct ProcNetEntry {
    pub(crate) local_host: String,
    pub(crate) local_port: u16,
    pub(crate) state: String,
    pub(crate) inode: u64,
}
