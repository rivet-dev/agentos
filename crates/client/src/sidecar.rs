//! `AgentOsSidecar` (public transport handle) + placement/description + the process-global shared
//! pool + internal lease accounting.
//!
//! Ported from `packages/core/src/agent-os.ts` (`AgentOsSidecar`). The shared-sidecar pool is a
//! process-global map scoped to the caller Tokio runtime (default pool `"default"`).

#[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use once_cell::sync::OnceCell;
use scc::HashMap as SccHashMap;
use serde::Serialize;
use uuid::Uuid;

use agentos_sidecar_client::wire;

use crate::agent_os::AgentOs;
use crate::error::ClientError;
use crate::transport::SidecarProcess;

/// Maximum shared sidecar pool entries retained process-wide.
const SHARED_SIDECAR_POOL_LIMIT: usize = 1024;

/// Env var that overrides the Agent OS wrapper sidecar binary path.
const AGENTOS_SIDECAR_BIN_ENV: &str = "AGENTOS_SIDECAR_BIN";

static SHARED_SIDECAR_PACKAGE_CACHE: OnceCell<crate::software::ProcessPackageCacheOptions> =
    OnceCell::new();

/// Configure the package cache of child sidecars before opening any shared
/// connection. The hosted worker uses this once at process startup.
#[cfg(feature = "sidecar-internals")]
pub fn configure_shared_sidecar_package_cache(
    options: crate::software::ProcessPackageCacheOptions,
) -> Result<(), ClientError> {
    options.validate()?;
    if let Some(existing) = SHARED_SIDECAR_PACKAGE_CACHE.get() {
        if existing == &options {
            return Ok(());
        }
        return Err(ClientError::PackageCacheConfiguration(
            "shared sidecar package cache is already configured differently".into(),
        ));
    }
    match SHARED_SIDECAR_PACKAGE_CACHE.set(options.clone()) {
        Ok(()) => Ok(()),
        Err(_) => configure_shared_sidecar_package_cache(options),
    }
}

fn sidecar_spawn_args() -> Result<Vec<String>, ClientError> {
    let Some(options) = SHARED_SIDECAR_PACKAGE_CACHE.get() else {
        return Ok(Vec::new());
    };
    let mut args = vec!["sidecar".to_owned()];
    if let Some(path) = &options.root {
        let path = path.to_str().ok_or_else(|| {
            ClientError::PackageCacheConfiguration(
                "package cache directory is not valid UTF-8 for sidecar startup".into(),
            )
        })?;
        args.extend(["--package-cache-dir".into(), path.into()]);
    }
    for (name, value) in [
        ("--package-cache-min-free-bytes", options.min_free_bytes),
        ("--package-cache-max-bytes", options.max_bytes),
        ("--package-cache-max-entries", options.max_entries as u64),
        (
            "--package-cache-max-concurrent-acquisitions",
            options.max_concurrent_acquisitions as u64,
        ),
        (
            "--package-cache-max-pending-acquisitions",
            options.max_pending_acquisitions as u64,
        ),
        (
            "--package-cache-acquisition-timeout-ms",
            options.acquisition_timeout_ms,
        ),
        (
            "--package-cache-max-source-entries",
            options.max_source_entries as u64,
        ),
        ("--package-cache-source-ttl-ms", options.source_ttl_ms),
    ] {
        args.extend([name.into(), value.to_string()]);
    }
    Ok(args)
}

/// The lazily-established shared sidecar process + authenticated connection. Multiple VMs in the same
/// (shared) sidecar reuse this single process/connection, each opening its own session + VM on it.
pub(crate) struct SharedConnection {
    pub(crate) transport: Arc<SidecarProcess>,
    pub(crate) connection_id: String,
    #[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
    package_session_id: Option<String>,
    authenticated: bool,
}

/// Sidecar lifecycle state, encoded as a `u8` for `AtomicU8`.
///
/// Parity: TypeScript `describe()` returns a JSON-serializable description whose `state` is exactly
/// `"ready" | "disposing" | "disposed"`. The `#[serde(rename_all = "lowercase")]` attribute and the
/// matching [`SidecarState::as_str`] reproduce that wire string so [`AgentOsSidecarDescription`]
/// serializes to the same JSON shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SidecarState {
    Ready,
    Disposing,
    Disposed,
}

impl SidecarState {
    /// The TypeScript wire string for this state (`"ready" | "disposing" | "disposed"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            SidecarState::Ready => "ready",
            SidecarState::Disposing => "disposing",
            SidecarState::Disposed => "disposed",
        }
    }

    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            SidecarState::Ready => 0,
            SidecarState::Disposing => 1,
            SidecarState::Disposed => 2,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Self {
        match value {
            0 => SidecarState::Ready,
            1 => SidecarState::Disposing,
            2 => SidecarState::Disposed,
            // Any other bit pattern is unreachable; the field is only written via `as_u8`.
            _ => SidecarState::Disposed,
        }
    }
}

/// Where a sidecar lives.
///
/// Parity: TypeScript `AgentOsSidecarPlacement` is `{ kind: "shared"; pool?: string }` or
/// `{ kind: "explicit"; sidecarId: string }`. The serde `tag`/`rename` attributes reproduce that
/// JSON shape, including omitting `pool` when it is `None` (matching the `...(pool ? { pool } : {})`
/// spread in `getSharedAgentOsSidecarInternal`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AgentOsSidecarPlacement {
    Shared {
        #[serde(skip_serializing_if = "Option::is_none")]
        pool: Option<String>,
    },
    Explicit {
        #[serde(rename = "sidecarId")]
        sidecar_id: String,
    },
}

/// A sync, deep-clone snapshot of a sidecar's state.
///
/// Parity: serializes to the TypeScript `AgentOsSidecarDescription` JSON shape
/// (`{ sidecarId, placement, state, activeVmCount }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOsSidecarDescription {
    pub sidecar_id: String,
    pub placement: AgentOsSidecarPlacement,
    pub state: SidecarState,
    pub active_vm_count: u32,
}

/// Public transport handle for a (possibly shared) sidecar process hosting VMs.
pub struct AgentOsSidecar {
    pub(crate) sidecar_id: String,
    pub(crate) placement: AgentOsSidecarPlacement,
    pub(crate) shared_pool: Option<String>,
    runtime_id: Option<tokio::runtime::Id>,
    pub(crate) state: AtomicU8,
    pub(crate) active_vm_count: AtomicU32,
    pub(crate) active_package_sessions: AtomicU32,
    lifecycle: tokio::sync::Mutex<()>,
    disposal_result: parking_lot::Mutex<Option<Result<(), ClientError>>>,
    /// Absolute path to the `agentos-sidecar` binary, threaded from `AgentOsConfig` when present.
    /// Otherwise `ensure_connection` resolves the Agent OS env fallback and passes an explicit path
    /// to the generic transport.
    pub(crate) sidecar_binary_path: Option<String>,
    /// The shared sidecar process + authenticated connection, established on the first VM `create`
    /// against this sidecar and reused by every subsequent VM in the same (shared) sidecar.
    pub(crate) connection: tokio::sync::Mutex<Option<SharedConnection>>,
    /// The transport in `connection`, readable without waiting for that lock, so a health check
    /// sees a lost process even while another caller holds the connection.
    live_transport: parking_lot::Mutex<Option<Arc<SidecarProcess>>>,
}

impl AgentOsSidecar {
    #[cfg(test)]
    pub(crate) async fn attach_test_transport(&self, transport: Arc<SidecarProcess>) {
        let mut connection = self.connection.lock().await;
        *self.live_transport.lock() = Some(transport.clone());
        *connection = Some(SharedConnection {
            transport,
            connection_id: "connection".into(),
            package_session_id: None,
            authenticated: true,
        });
    }

    /// Construct a sidecar handle.
    pub(crate) fn new(
        sidecar_id: impl Into<String>,
        placement: AgentOsSidecarPlacement,
        shared_pool: Option<String>,
        sidecar_binary_path: Option<String>,
    ) -> Self {
        Self {
            sidecar_id: sidecar_id.into(),
            placement,
            shared_pool,
            runtime_id: tokio::runtime::Handle::try_current()
                .ok()
                .map(|handle| handle.id()),
            state: AtomicU8::new(SidecarState::Ready.as_u8()),
            active_vm_count: AtomicU32::new(0),
            active_package_sessions: AtomicU32::new(0),
            lifecycle: tokio::sync::Mutex::new(()),
            disposal_result: parking_lot::Mutex::new(None),
            sidecar_binary_path,
            connection: tokio::sync::Mutex::new(None),
            live_transport: parking_lot::Mutex::new(None),
        }
    }

    /// Get (or lazily establish) the shared sidecar process + authenticated connection. The first
    /// caller spawns the native sidecar child and runs the `Authenticate` handshake; subsequent
    /// callers reuse the same transport + connection id. This is what makes a shared sidecar host
    /// multiple VMs in one process.
    pub(crate) async fn ensure_connection(
        &self,
    ) -> Result<(Arc<SidecarProcess>, String, usize), ClientError> {
        let mut guard = self.connection.lock().await;
        if self.state.load(Ordering::SeqCst) != SidecarState::Ready.as_u8() {
            return Err(ClientError::Sidecar(
                "sidecar is disposing or disposed".into(),
            ));
        }
        if let Some(existing) = guard.as_ref() {
            if existing.transport.is_disconnected() {
                return Err(ClientError::SidecarExited(
                    "its VMs are gone; create a new VM".into(),
                ));
            }
            if !existing.authenticated {
                return Err(ClientError::Sidecar(
                    "sidecar authentication did not complete; dispose this sidecar before retrying"
                        .into(),
                ));
            }
            let max_frame = existing.transport.max_frame_bytes();
            return Ok((
                existing.transport.clone(),
                existing.connection_id.clone(),
                max_frame,
            ));
        }

        let transport = SidecarProcess::spawn_with_args(
            Some(self.resolved_sidecar_binary_path()),
            sidecar_spawn_args()?,
        )
        .await?;
        // Retain ownership before the authentication await, so errors or
        // cancellation cannot orphan a child that holds the cache directory.
        *self.live_transport.lock() = Some(transport.clone());
        *guard = Some(SharedConnection {
            transport: transport.clone(),
            connection_id: String::new(),
            #[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
            package_session_id: None,
            authenticated: false,
        });
        let authed = match transport
            .request_wire(
                wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                    connection_id: "client-hint".to_string(),
                }),
                wire::RequestPayload::AuthenticateRequest(wire::AuthenticateRequest {
                    client_name: "agentos-client".to_string(),
                    auth_token: "agentos-client".to_string(),
                    protocol_version: wire::PROTOCOL_VERSION,
                    bridge_version: agentos_vm_host_interface::bridge_contract().version,
                }),
            )
            .await?
        {
            wire::ResponsePayload::AuthenticatedResponse(authed) => authed,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(ClientError::Kernel {
                    code: rejected.code,
                    message: rejected.message,
                });
            }
            _ => {
                return Err(ClientError::Sidecar(
                    "unexpected authenticate response".to_string(),
                ));
            }
        };
        let max_frame = authed.max_frame_bytes as usize;
        transport.set_max_frame_bytes(max_frame);

        *guard = Some(SharedConnection {
            transport: transport.clone(),
            connection_id: authed.connection_id.clone(),
            #[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
            package_session_id: None,
            authenticated: true,
        });
        Ok((transport, authed.connection_id, max_frame))
    }

    /// Kill the shared sidecar child process if a connection was established. Used when the last VM
    /// on a shared sidecar shuts down, so the sidecar process does not leak (process-global pool
    /// entries are never dropped, so `kill_on_drop` alone would not fire at process exit).
    pub(crate) async fn kill_connection(&self) -> Result<(), ClientError> {
        let mut connection = self.connection.lock().await;
        let result = if let Some(connection) = connection.as_ref() {
            let result = connection.transport.terminate_child().await;
            if !connection.transport.has_confirmed_child_exit() {
                return result.map_err(ClientError::from);
            }
            crate::agent_os::release_transport_callback_routes(connection.transport.instance_id());
            result.map_err(ClientError::from)
        } else {
            Ok(())
        };
        // A nonzero exit is a terminal error with confirmed ownership release.
        // Unconfirmed failure/cancellation returns above, retaining the owner.
        connection.take();
        self.live_transport.lock().take();
        result
    }

    /// Serialize the final publication step with explicit owner disposal.
    pub(crate) async fn vm_publication_guard(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, ClientError> {
        let guard = self.lifecycle.lock().await;
        if self.current_state() != SidecarState::Ready {
            return Err(ClientError::Sidecar(
                "sidecar disposal began during VM creation".into(),
            ));
        }
        Ok(guard)
    }

    pub(crate) async fn acquire_vm_lease(
        self: &Arc<Self>,
    ) -> Result<AgentOsSidecarVmLease, ClientError> {
        self.acquire_lease(false).await
    }

    async fn acquire_lease(
        self: &Arc<Self>,
        package: bool,
    ) -> Result<AgentOsSidecarVmLease, ClientError> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.process_lost() {
            return Err(ClientError::SidecarExited(
                "its VMs are gone; create a new VM".into(),
            ));
        }
        if self.current_state() != SidecarState::Ready {
            return Err(ClientError::Sidecar(
                "sidecar is disposing or disposed".into(),
            ));
        }
        let count = if package {
            &self.active_package_sessions
        } else {
            &self.active_vm_count
        };
        count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_add(1)
            })
            .map_err(|_| {
                ClientError::Sidecar("limit_exceeded: sidecar lease count overflow".into())
            })?;
        Ok(AgentOsSidecarVmLease {
            sidecar: self.clone(),
            package,
            released: AtomicBool::new(false),
        })
    }

    pub(crate) async fn dispose_if_unused(&self) -> Result<(), ClientError> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.active_vm_count.load(Ordering::SeqCst) == 0
            && self.active_package_sessions.load(Ordering::SeqCst) == 0
        {
            self.dispose_locked().await?;
        }
        Ok(())
    }

    /// Open a sidecar-owned package acquisition session before any VM exists.
    #[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
    pub async fn open_package_session(
        self: &Arc<Self>,
    ) -> Result<AgentOsPackageSession, ClientError> {
        // Reserve before the first I/O await: last-VM shutdown must not kill
        // the child while this session is still opening.
        let lease = self.acquire_lease(true).await?;
        let result = async {
            let (transport, connection_id, _) = self.ensure_connection().await?;
            let mut connection = self.connection.lock().await;
            let current = connection.as_mut().ok_or_else(|| {
                ClientError::Sidecar(
                    "sidecar connection closed while opening package session".into(),
                )
            })?;
            // The wire has no CloseSession operation. Reuse one acquisition
            // session per connection so repeated open/close does not exhaust the
            // bounded sidecar session table while sibling VMs remain alive.
            let session_id = if let Some(session_id) = &current.package_session_id {
                session_id.clone()
            } else {
                let response = transport
                    .request_wire(
                        wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                            connection_id: connection_id.clone(),
                        }),
                        wire::RequestPayload::OpenSessionRequest(wire::OpenSessionRequest {
                            placement: crate::agent_os::sidecar_wire_placement(self),
                            metadata: HashMap::new(),
                        }),
                    )
                    .await?;
                let wire::ResponsePayload::SessionOpenedResponse(opened) = response else {
                    return Err(match response {
                        wire::ResponsePayload::RejectedResponse(rejected) => {
                            ClientError::from_rejection(rejected)
                        }
                        other => ClientError::Sidecar(format!(
                            "unexpected package session response: {other:?}"
                        )),
                    });
                };
                current.package_session_id = Some(opened.session_id.clone());
                opened.session_id
            };
            drop(connection);
            Ok((transport, connection_id, session_id))
        }
        .await;
        let (transport, connection_id, session_id) = match result {
            Ok(opened) => opened,
            Err(error) => {
                lease.release();
                if let Err(cleanup_error) = self.dispose_if_unused().await {
                    tracing::error!(%cleanup_error, "failed to stop sidecar after package session open failed");
                }
                return Err(error);
            }
        };
        Ok(AgentOsPackageSession {
            sidecar: self.clone(),
            transport,
            connection_id,
            session_id,
            closed: AtomicBool::new(false),
            lease,
        })
    }

    fn resolved_sidecar_binary_path(&self) -> String {
        self.sidecar_binary_path
            .clone()
            .or_else(|| std::env::var(AGENTOS_SIDECAR_BIN_ENV).ok())
            .unwrap_or_else(|| "agentos-sidecar".to_string())
    }

    /// Snapshot the sidecar's current state. SYNC.
    ///
    /// Parity: TypeScript `describe()` returns a deep clone of the internal description so callers
    /// cannot mutate sidecar state through the returned value. The Rust struct derives `Clone`, so
    /// constructing a fresh [`AgentOsSidecarDescription`] from the current atomics produces the same
    /// snapshot semantics.
    pub fn describe(&self) -> AgentOsSidecarDescription {
        AgentOsSidecarDescription {
            sidecar_id: self.sidecar_id.clone(),
            placement: self.placement.clone(),
            state: self.current_state(),
            active_vm_count: self.active_vm_count.load(Ordering::SeqCst),
        }
    }

    /// A disconnected transport fences admission as `Disposing` until cleanup
    /// confirms child reap. Shared-pool replacement performs that cleanup first.
    fn current_state(&self) -> SidecarState {
        let state = SidecarState::from_u8(self.state.load(Ordering::SeqCst));
        if state == SidecarState::Ready && self.process_lost() {
            SidecarState::Disposing
        } else {
            state
        }
    }

    fn process_lost(&self) -> bool {
        self.live_transport
            .lock()
            .as_ref()
            .is_some_and(|transport| transport.is_disconnected())
    }

    /// Stop and reap the owned sidecar, including failed VM leases.
    ///
    /// Concurrent callers serialize. Confirmed cleanup releases the shared-pool
    /// owner and preserves its terminal result, including a nonzero-exit error,
    /// for repeated calls. Unconfirmed failure or cancellation retains ownership
    /// so explicit disposal can retry. The whole sidecar is the recovery boundary
    /// when an individual VM's teardown result is unconfirmed.
    pub async fn dispose(&self) -> Result<(), ClientError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.dispose_locked().await
    }

    async fn dispose_locked(&self) -> Result<(), ClientError> {
        if SidecarState::from_u8(self.state.load(Ordering::SeqCst)) == SidecarState::Disposed {
            self.forget_shared_owner();
            return self.disposal_result.lock().clone().unwrap_or(Ok(()));
        }

        self.state
            .store(SidecarState::Disposing.as_u8(), Ordering::SeqCst);

        // Explicit sidecar disposal owns all its VMs. Reap the process before
        // reporting disposal, even if a failed VM still retained a lease.
        // On cancellation/error, remain Disposing and retain the connection
        // so a later call can finish instead of reporting false success.
        let result = self.kill_connection().await;
        if self.live_transport.lock().is_some() {
            return result;
        }
        self.active_vm_count.store(0, Ordering::SeqCst);
        self.active_package_sessions.store(0, Ordering::SeqCst);
        self.state
            .store(SidecarState::Disposed.as_u8(), Ordering::SeqCst);

        *self.disposal_result.lock() = Some(result.clone());
        self.forget_shared_owner();
        result
    }

    fn forget_shared_owner(&self) {
        if let Some(pool) = self.shared_pool.as_deref() {
            let _admission = shared_sidecar_pool_lock().lock();
            #[cfg(any(feature = "actor-internals", test))]
            let mut _admission = _admission;
            let self_ptr = self as *const AgentOsSidecar;
            let _ = shared_sidecars().remove_if(&(self.runtime_id, pool.to_owned()), |cached| {
                std::ptr::eq(Arc::as_ptr(cached), self_ptr)
            });
            #[cfg(any(feature = "actor-internals", test))]
            if let Some(runtime) = self.runtime_id {
                _admission.forget(runtime, self);
            }
        }
    }
}

/// A process-scoped sidecar session for package acquisition without a VM.
#[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
pub struct AgentOsPackageSession {
    sidecar: Arc<AgentOsSidecar>,
    transport: Arc<SidecarProcess>,
    connection_id: String,
    session_id: String,
    closed: AtomicBool,
    lease: AgentOsSidecarVmLease,
}

#[cfg(any(feature = "actor-internals", feature = "sidecar-internals", test))]
impl AgentOsPackageSession {
    pub async fn cache_stats(
        &self,
    ) -> Result<crate::software::ProcessPackageCacheStats, ClientError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ClientError::Sidecar("package session is closed".into()));
        }
        let response = self
            .transport
            .request_wire(
                wire::OwnershipScope::SessionOwnership(wire::SessionOwnership {
                    connection_id: self.connection_id.clone(),
                    session_id: self.session_id.clone(),
                }),
                wire::RequestPayload::GetPackageCacheStatsRequest,
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageCacheStatsResponse(stats) => {
                let count = |value: u64| usize::try_from(value).unwrap_or(usize::MAX);
                Ok(crate::software::ProcessPackageCacheStats {
                    entries: count(stats.entries),
                    source_entries: count(stats.source_entries),
                    bytes: stats.bytes,
                    pinned_entries: count(stats.pinned_entries),
                    pending_acquisitions: count(stats.pending_acquisitions),
                    hits: stats.hits,
                    misses: stats.misses,
                    coalesced_waiters: stats.coalesced_waiters,
                    acquisitions: stats.acquisitions,
                    evictions: stats.evictions,
                    capacity_failures: stats.capacity_failures,
                    cancelled_acquisitions: stats.cancelled_acquisitions,
                })
            }
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "unexpected package cache stats response: {other:?}"
            ))),
        }
    }

    pub async fn acquire(
        &self,
        source: crate::software::PackageSource,
        advisory: bool,
        timeout_ms: Option<u64>,
        options: Option<&crate::software::PackageResolverOptions>,
    ) -> Result<crate::software::InstalledSoftware, ClientError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ClientError::Sidecar("package session is closed".into()));
        }
        let source = match source {
            crate::software::PackageSource::Url {
                url,
                expected_digest,
            } => wire::PackageAcquisitionSource::PackageUrlSource(wire::PackageUrlSource {
                url,
                expected_digest,
            }),
            crate::software::PackageSource::Path {
                path,
                expected_digest,
            } => wire::PackageAcquisitionSource::PackagePathSource(wire::PackagePathSource {
                path,
                expected_digest,
            }),
        };
        let response = self
            .transport
            .request_wire(
                wire::OwnershipScope::SessionOwnership(wire::SessionOwnership {
                    connection_id: self.connection_id.clone(),
                    session_id: self.session_id.clone(),
                }),
                wire::RequestPayload::AcquirePackageRequest(wire::AcquirePackageRequest {
                    source,
                    advisory,
                    timeout_ms,
                    max_package_bytes: options.map(|options| options.max_package_bytes),
                    max_source_url_bytes: options
                        .map(|options| {
                            u64::try_from(options.max_source_url_bytes).map_err(|_| {
                                ClientError::InvalidPackageSource(
                                    "max_source_url_bytes exceeds u64 representation".into(),
                                )
                            })
                        })
                        .transpose()?,
                    download_timeout_ms: options.map(|options| options.download_timeout_ms),
                    connect_timeout_ms: options.map(|options| options.connect_timeout_ms),
                    max_redirects: options.map(|options| options.max_redirects as u32),
                    allow_insecure_local_http: options
                        .is_some_and(|options| options.allow_insecure_local_http),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageAcquiredResponse(package) => {
                Ok(crate::software::InstalledSoftware {
                    package_id: package.package_id,
                    digest: package.digest,
                    size_bytes: package.size,
                    package_name: package.package_name,
                    version: package.version,
                    commands: package.commands,
                })
            }
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "unexpected acquire_package response: {other:?}"
            ))),
        }
    }

    pub async fn close(&self) -> Result<(), ClientError> {
        self.closed.store(true, Ordering::SeqCst);
        self.lease.release();
        // Retry unfinished disposal even when an earlier close was cancelled.
        self.sidecar.dispose_if_unused().await
    }

    #[cfg(feature = "actor-internals")]
    pub(crate) async fn shutdown_worker(&self) -> Result<(), ClientError> {
        self.closed.store(true, Ordering::SeqCst);
        self.lease.release();
        self.sidecar.dispose().await
    }
}

/// A lease over a VM; released on `AgentOs` dispose.
pub(crate) struct AgentOsSidecarVmLease {
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    package: bool,
    released: AtomicBool,
}

impl AgentOsSidecarVmLease {
    /// An initialization error has no VM handle left to retry disposal. Keep
    /// that ownership counted until the explicit worker owner stops the child.
    pub(crate) fn retain_until_worker_shutdown(self) {
        self.released.store(true, Ordering::SeqCst);
    }

    /// Release the lease.
    ///
    /// Parity with the TypeScript lease `dispose()`: it is idempotent, removes itself from the
    /// owning sidecar's active-lease set, recomputes `activeVmCount`, and disposes the underlying
    /// session transport client. Consuming `self` here gives the idempotence for free (the lease
    /// cannot be disposed twice). The active-vm count is decremented (saturating at 0) to mirror
    /// `state.description.activeVmCount = state.activeLeases.size`.
    ///
    pub(crate) async fn dispose(self) -> Result<(), ClientError> {
        self.release();
        Ok(())
    }

    fn release(&self) -> bool {
        if self.released.swap(true, Ordering::SeqCst) {
            return false;
        }
        let count = if self.package {
            &self.sidecar.active_package_sessions
        } else {
            &self.sidecar.active_vm_count
        };
        count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                Some(count.saturating_sub(1))
            })
            .expect("saturating lease decrement");
        true
    }
}

impl Drop for AgentOsSidecarVmLease {
    fn drop(&mut self) {
        if self.release()
            && self.sidecar.state.load(Ordering::SeqCst) == SidecarState::Ready.as_u8()
        {
            tracing::warn!(sidecar_id = %self.sidecar.sidecar_id, "sidecar lease dropped without explicit cleanup; ownership released, call sidecar.dispose() to confirm child teardown");
        }
    }
}

/// Transport I/O belongs to the caller's Tokio reactor. Sharing across runtimes
/// would abort another runtime's active VMs when the first runtime shuts down.
/// Keep the cache process-wide and bounded, but include the owning runtime in its key.
type SharedSidecarKey = (Option<tokio::runtime::Id>, String);
static SHARED_SIDECARS: OnceCell<SccHashMap<SharedSidecarKey, Arc<AgentOsSidecar>>> =
    OnceCell::new();
static SHARED_SIDECAR_POOL_LOCK: OnceCell<parking_lot::Mutex<SharedPoolLifecycle>> =
    OnceCell::new();

#[derive(Default)]
struct SharedPoolLifecycle {
    #[cfg(any(feature = "actor-internals", test))]
    runtimes: HashMap<tokio::runtime::Id, RuntimeSharedOwners>,
    #[cfg(any(feature = "actor-internals", test))]
    owner_count: usize,
}

#[cfg(any(feature = "actor-internals", test))]
#[derive(Default)]
struct RuntimeSharedOwners {
    closed: bool,
    // Cached replacements do not relinquish older children before confirmed reap.
    owners: Vec<Arc<AgentOsSidecar>>,
}

#[cfg(any(feature = "actor-internals", test))]
impl SharedPoolLifecycle {
    fn admit(&self, runtime: tokio::runtime::Id) -> Result<(), ClientError> {
        if self
            .runtimes
            .get(&runtime)
            .is_some_and(|state| state.closed)
        {
            return Err(ClientError::Sidecar(String::from(
                "ERR_AGENTOS_SHARED_SIDECAR_ADMISSION_CLOSED: hosted worker shutdown permanently closed shared-sidecar admission for this runtime",
            )));
        }
        Ok(())
    }

    fn register(
        &mut self,
        runtime: tokio::runtime::Id,
        sidecar: Arc<AgentOsSidecar>,
    ) -> Result<(), ClientError> {
        self.admit(runtime)?;
        self.ensure_runtime_capacity(runtime)?;
        if self.owner_count >= SHARED_SIDECAR_POOL_LIMIT {
            return Err(shared_owner_capacity_error(
                "sharedSidecarOwners",
                self.owner_count,
            ));
        }
        self.runtimes
            .entry(runtime)
            .or_default()
            .owners
            .push(sidecar);
        if self.runtimes.len() * 5 >= SHARED_SIDECAR_POOL_LIMIT * 4 {
            tracing::warn!(
                limit = "sharedSidecarRuntimeOwners",
                observed = self.runtimes.len(),
                capacity = SHARED_SIDECAR_POOL_LIMIT,
                "shared runtime ownership records approaching capacity; restart terminal workers"
            );
        }
        self.owner_count += 1;
        if self.owner_count * 5 >= SHARED_SIDECAR_POOL_LIMIT * 4 {
            tracing::warn!(
                limit = "sharedSidecarOwners",
                observed = self.owner_count,
                capacity = SHARED_SIDECAR_POOL_LIMIT,
                "shared sidecar owners approaching capacity; dispose idle children"
            );
        }
        Ok(())
    }

    fn ensure_runtime_capacity(&self, runtime: tokio::runtime::Id) -> Result<(), ClientError> {
        if !self.runtimes.contains_key(&runtime) && self.runtimes.len() >= SHARED_SIDECAR_POOL_LIMIT
        {
            return Err(shared_owner_capacity_error(
                "sharedSidecarRuntimeOwners",
                self.runtimes.len(),
            ));
        }
        Ok(())
    }

    fn close(
        &mut self,
        runtime: tokio::runtime::Id,
    ) -> Result<Vec<Arc<AgentOsSidecar>>, ClientError> {
        self.ensure_runtime_capacity(runtime)?;
        let state = self.runtimes.entry(runtime).or_default();
        state.closed = true;
        // Fence every retained handle before polling the bounded disposal batch.
        // Existing handles must not open a fifth child while four owners drain.
        for owner in &state.owners {
            // The encoded lifecycle is monotonic: Ready=0, Disposing=1, Disposed=2.
            owner
                .state
                .fetch_max(SidecarState::Disposing.as_u8(), Ordering::SeqCst);
        }
        let owners = state.owners.clone();
        if self.runtimes.len() * 5 >= SHARED_SIDECAR_POOL_LIMIT * 4 {
            tracing::warn!(
                limit = "sharedSidecarRuntimeOwners",
                observed = self.runtimes.len(),
                capacity = SHARED_SIDECAR_POOL_LIMIT,
                "terminal shared runtime records approaching capacity; restart terminal workers"
            );
        }
        Ok(owners)
    }

    fn forget(&mut self, runtime: tokio::runtime::Id, sidecar: &AgentOsSidecar) {
        let remove_runtime = if let Some(state) = self.runtimes.get_mut(&runtime) {
            let before = state.owners.len();
            state
                .owners
                .retain(|owner| !std::ptr::eq(Arc::as_ptr(owner), sidecar));
            self.owner_count -= before - state.owners.len();
            state.owners.is_empty() && !state.closed
        } else {
            false
        };
        if remove_runtime {
            self.runtimes.remove(&runtime);
        }
    }
}

#[cfg(any(feature = "actor-internals", test))]
fn shared_owner_capacity_error(name: &str, current: usize) -> ClientError {
    ClientError::ResourceLimit {
        code: String::from("limit_exceeded"),
        message: format!("{name} reached {SHARED_SIDECAR_POOL_LIMIT}; dispose idle children or raise SHARED_SIDECAR_POOL_LIMIT in the worker build"),
        details: Box::new(crate::ResourceLimitDetails {
            limit_name: Some(name.into()), configured_limit: Some(SHARED_SIDECAR_POOL_LIMIT as u64),
            current_usage: Some(current as u64), requested: Some(1), unit: Some("owners".into()),
            operation: Some("shared_sidecar.admit".into()),
            configuration_path: Some("SHARED_SIDECAR_POOL_LIMIT".into()), ..Default::default()
        }),
    }
}

/// Terminal host-process ownership boundary, not VM or guest policy.
#[cfg(any(feature = "actor-internals", test))]
pub(crate) async fn shutdown_runtime_shared_sidecars(
    deadline: std::time::Duration,
) -> Result<(), ClientError> {
    use futures::StreamExt;
    if deadline.is_zero() {
        return Err(ClientError::InvalidConfig(
            "shared-sidecar shutdown deadline must be positive".into(),
        ));
    }
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|error| {
            ClientError::Sidecar(format!(
                "shared sidecar shutdown requires its owning Tokio runtime: {error}"
            ))
        })?
        .id();
    // Creation/replacement registration has no await and uses this same lock.
    // Closing and snapshotting therefore includes every previously admitted owner.
    let owners = shared_sidecar_pool_lock().lock().close(runtime)?;
    let drain = async {
        let mut pending = futures::stream::iter(owners)
            .map(|owner| async move {
                let result = owner.dispose().await;
                (owner.sidecar_id.clone(), result)
            })
            .buffer_unordered(4);
        let mut errors = Vec::new();
        while let Some((sidecar_id, result)) = pending.next().await {
            if let Err(error) = result {
                tracing::error!(%sidecar_id, %error, "shared sidecar worker shutdown reported a cleanup failure");
                errors.push(format!(
                    "{sidecar_id}: {}",
                    error.to_string().chars().take(2048).collect::<String>()
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ClientError::Sidecar(format!(
                "ERR_AGENTOS_SHARED_SIDECAR_SHUTDOWN_FAILED: {} owner(s): {}",
                errors.len(),
                errors.join("; ")
            )))
        }
    };
    tokio::time::timeout(deadline, drain).await.map_err(|_| {
        ClientError::OperationTimedOut {
            message: format!("shared-sidecar shutdown exceeded {}ms; child reap is unconfirmed; increase the worker shutdown deadline and retry before removing its cache", deadline.as_millis()),
            details: Box::new(crate::ResourceLimitDetails {
                limit_name: Some("sharedSidecarShutdownDeadlineMs".into()),
                configured_limit: Some(u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX)),
                unit: Some("ms".into()), operation: Some("shared_sidecar.shutdown".into()),
                configuration_path: Some("--worker-shutdown-timeout-ms".into()), retryable: Some(true), ..Default::default()
            }),
        }
    })?
}

/// Access (initializing on first use) the process-global shared-sidecar pool.
pub(crate) fn shared_sidecars() -> &'static SccHashMap<SharedSidecarKey, Arc<AgentOsSidecar>> {
    SHARED_SIDECARS.get_or_init(SccHashMap::new)
}

fn shared_sidecar_pool_lock() -> &'static parking_lot::Mutex<SharedPoolLifecycle> {
    SHARED_SIDECAR_POOL_LOCK.get_or_init(parking_lot::Mutex::default)
}

fn shared_sidecar_pool_len(cache: &SccHashMap<SharedSidecarKey, Arc<AgentOsSidecar>>) -> usize {
    let mut len = 0;
    cache.scan(|_, _| {
        len += 1;
    });
    len
}

fn prune_disposed_shared_sidecars(cache: &SccHashMap<SharedSidecarKey, Arc<AgentOsSidecar>>) {
    let mut disposed_pools = Vec::new();
    cache.scan(|pool, sidecar| {
        if sidecar.describe().state == SidecarState::Disposed {
            disposed_pools.push(pool.clone());
        }
    });
    for pool in disposed_pools {
        let _ = cache.remove_if(&pool, |sidecar| {
            sidecar.describe().state == SidecarState::Disposed
        });
    }
}

#[cfg(test)]
fn ensure_shared_sidecar_pool_capacity(
    cache: &SccHashMap<SharedSidecarKey, Arc<AgentOsSidecar>>,
) -> Result<(), ClientError> {
    if shared_sidecar_pool_len(cache) >= SHARED_SIDECAR_POOL_LIMIT {
        return Err(shared_sidecar_pool_limit_error());
    }
    Ok(())
}

fn shared_sidecar_pool_limit_error() -> ClientError {
    ClientError::Sidecar(format!(
        "shared sidecar pool limit exceeded: at most {SHARED_SIDECAR_POOL_LIMIT} pools can be cached"
    ))
}

impl AgentOs {
    /// Create an explicit sidecar handle. `sidecar_id` defaults to `agentos-sidecar-<uuid>`.
    ///
    /// Parity with TypeScript `createAgentOsSidecarInternal`: the explicit handle carries an
    /// `Explicit` placement whose `sidecar_id` echoes the resolved id and has no shared pool.
    pub async fn create_sidecar(
        sidecar_id: Option<String>,
    ) -> Result<Arc<AgentOsSidecar>, ClientError> {
        let sidecar_id =
            sidecar_id.unwrap_or_else(|| format!("agentos-sidecar-{}", Uuid::new_v4()));
        let placement = AgentOsSidecarPlacement::Explicit {
            sidecar_id: sidecar_id.clone(),
        };
        Ok(Arc::new(AgentOsSidecar::new(
            sidecar_id, placement, None, None,
        )))
    }

    /// Get (or create) a pooled shared sidecar. Pool defaults to `"default"`. Uses the process-global
    /// cache, partitioned by Tokio runtime so every transport shares its owner reactor lifetime.
    ///
    /// Parity with TypeScript `getSharedAgentOsSidecarInternal`: return the cached sidecar for the
    /// pool when it exists and is not disposed; otherwise build a fresh handle
    /// (`agentos-shared-sidecar:<pool>`, `Shared` placement) and cache it. Because the cache is a
    /// process-global concurrent map rather than a synchronously-checked `Map`, the insert is done
    /// atomically with `entry`/`insert` so two racing callers converge on a single live handle.
    pub async fn get_shared_sidecar(
        pool: Option<String>,
        sidecar_binary_path: Option<String>,
    ) -> Result<Arc<AgentOsSidecar>, ClientError> {
        let pool = pool.unwrap_or_else(|| "default".to_string());
        let runtime_id = tokio::runtime::Handle::try_current()
            .map_err(|error| {
                ClientError::Sidecar(format!("shared sidecar requires a Tokio runtime: {error}"))
            })?
            .id();
        let key = (Some(runtime_id), pool.clone());
        let cache = shared_sidecars();
        // Reap an unusable generation before replacing its cached owner. Do not
        // hold the synchronous pool lock across process teardown.
        if let Some(existing) = cache.read(&key, |_, sidecar| sidecar.clone()) {
            if existing.process_lost() {
                if let Err(error) = existing.dispose().await {
                    if existing.current_state() != SidecarState::Disposed {
                        return Err(error);
                    }
                    tracing::warn!(%error, "confirmed dead shared sidecar removed before replacement");
                }
            }
        }
        let _admission = shared_sidecar_pool_lock().lock();
        #[cfg(any(feature = "actor-internals", test))]
        let mut _admission = _admission;
        #[cfg(any(feature = "actor-internals", test))]
        _admission.admit(runtime_id)?;

        // Fast path: reuse a cached, non-disposed sidecar for this pool.
        if let Some(existing) = cache.read(&key, |_, sidecar| sidecar.clone()) {
            if existing.describe().state != SidecarState::Disposed {
                return Ok(existing);
            }
        }
        prune_disposed_shared_sidecars(cache);

        // Parity: TypeScript builds placement `{ kind: "shared", ...(pool ? { pool } : {}) }`, so an
        // empty-string pool (a non-nullish value that survives `?? "default"`) is OMITTED from the
        // placement. The `sharedPool` field used for cache cleanup still carries the raw pool value.
        let placement_pool = if pool.is_empty() {
            None
        } else {
            Some(pool.clone())
        };
        let sidecar = Arc::new(AgentOsSidecar::new(
            format!("agentos-shared-sidecar:{pool}"),
            AgentOsSidecarPlacement::Shared {
                pool: placement_pool,
            },
            Some(pool.clone()),
            sidecar_binary_path,
        ));

        // Insert atomically, replacing a stale (disposed) entry but yielding to a live one that a
        // concurrent caller may have just installed.
        let cache_len = shared_sidecar_pool_len(cache);
        match cache.entry(key) {
            scc::hash_map::Entry::Occupied(mut occupied) => {
                if occupied.get().describe().state == SidecarState::Disposed {
                    #[cfg(any(feature = "actor-internals", test))]
                    _admission.register(runtime_id, sidecar.clone())?;
                    *occupied.get_mut() = sidecar.clone();
                    Ok(sidecar)
                } else {
                    Ok(occupied.get().clone())
                }
            }
            scc::hash_map::Entry::Vacant(vacant) => {
                if cache_len >= SHARED_SIDECAR_POOL_LIMIT {
                    return Err(shared_sidecar_pool_limit_error());
                }
                #[cfg(any(feature = "actor-internals", test))]
                _admission.register(runtime_id, sidecar.clone())?;
                vacant.insert_entry(sidecar.clone());
                Ok(sidecar)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn disconnected_shared_owner_is_reaped_before_pool_replacement() {
        let pool = format!("reap-before-replace-{}", Uuid::new_v4());
        let old = AgentOs::get_shared_sidecar(Some(pool.clone()), None)
            .await
            .unwrap();
        let transport = SidecarProcess::spawn_with_args(
            Some("/bin/sh".into()),
            vec!["-c".into(), "exit 17".into()],
        )
        .await
        .unwrap();
        *old.live_transport.lock() = Some(transport.clone());
        *old.connection.lock().await = Some(SharedConnection {
            transport: transport.clone(),
            connection_id: "test".into(),
            package_session_id: None,
            authenticated: true,
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !transport.is_disconnected() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let fresh = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            AgentOs::get_shared_sidecar(Some(pool), None),
        )
        .await
        .unwrap()
        .unwrap();
        let confirmed = transport.has_confirmed_child_exit();
        let retained = old.connection.lock().await.is_some();
        fresh.dispose().await.unwrap();
        let old_result = old.dispose().await;
        assert!(!Arc::ptr_eq(&old, &fresh));
        assert!(confirmed);
        assert!(!retained);
        assert!(old_result.unwrap_err().to_string().contains("17"));
    }

    #[tokio::test]
    async fn disconnected_owner_is_not_disposed_before_reap_confirmation() {
        let sidecar = shared("unreaped-disconnect", SidecarState::Ready);
        let transport = SidecarProcess::spawn_with_args(
            Some("/bin/sh".into()),
            vec!["-c".into(), "exit 17".into()],
        )
        .await
        .unwrap();
        *sidecar.live_transport.lock() = Some(transport.clone());
        *sidecar.connection.lock().await = Some(SharedConnection {
            transport: transport.clone(),
            connection_id: "test".into(),
            package_session_id: None,
            authenticated: true,
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !transport.is_disconnected() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let state = sidecar.describe().state;
        let cleanup = tokio::time::timeout(std::time::Duration::from_secs(2), sidecar.dispose())
            .await
            .unwrap();
        assert_ne!(
            state,
            SidecarState::Disposed,
            "disconnect is not confirmed reap"
        );
        assert!(
            cleanup.is_err(),
            "confirmed natural exit17 must remain visible"
        );
        assert!(sidecar.connection.lock().await.is_none());
        assert_eq!(sidecar.describe().state, SidecarState::Disposed);
        assert!(
            sidecar.dispose().await.is_err(),
            "terminal error must not become success"
        );
    }

    #[tokio::test]
    async fn opening_leases_prevent_last_vm_from_disposing_the_child() {
        let sidecar = shared("opening-leases", SidecarState::Ready);
        let vm = sidecar.acquire_vm_lease().await.unwrap();
        let package = sidecar.acquire_lease(true).await.unwrap();
        vm.dispose().await.unwrap();
        sidecar.dispose_if_unused().await.unwrap();
        assert_eq!(sidecar.describe().state, SidecarState::Ready);
        assert_eq!(sidecar.active_package_sessions.load(Ordering::SeqCst), 1);
        package.release();
        sidecar.dispose_if_unused().await.unwrap();
        assert_eq!(sidecar.describe().state, SidecarState::Disposed);
        assert!(sidecar.acquire_vm_lease().await.is_err());
        assert!(sidecar.ensure_connection().await.is_err());
        drop(package);
        assert_eq!(sidecar.active_package_sessions.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unconfirmed_vm_cleanup_keeps_ownership_until_force_disposal() {
        let sidecar = shared("unconfirmed-cleanup", SidecarState::Ready);
        sidecar
            .acquire_vm_lease()
            .await
            .unwrap()
            .retain_until_worker_shutdown();
        sidecar.dispose_if_unused().await.unwrap();
        assert_eq!(sidecar.describe().active_vm_count, 1);
        assert_eq!(sidecar.describe().state, SidecarState::Ready);
        sidecar.dispose().await.unwrap();
        assert_eq!(sidecar.describe().active_vm_count, 0);
        assert_eq!(sidecar.describe().state, SidecarState::Disposed);
    }

    #[tokio::test]
    async fn failed_package_session_open_releases_its_reserved_lease() {
        let sidecar = Arc::new(AgentOsSidecar::new(
            "missing-binary",
            AgentOsSidecarPlacement::Explicit {
                sidecar_id: "missing-binary".into(),
            },
            None,
            Some("/agentos-missing-test-binary".into()),
        ));
        assert!(sidecar.open_package_session().await.is_err());
        assert_eq!(sidecar.active_package_sessions.load(Ordering::SeqCst), 0);
        assert_eq!(sidecar.describe().state, SidecarState::Disposed);
    }

    #[tokio::test]
    async fn failed_vm_creation_releases_its_reserved_lease() {
        let sidecar = Arc::new(AgentOsSidecar::new(
            "missing-vm-binary",
            AgentOsSidecarPlacement::Explicit {
                sidecar_id: "missing-vm-binary".into(),
            },
            None,
            Some("/agentos-missing-test-binary".into()),
        ));
        let result = AgentOs::create(crate::AgentOsConfig {
            sidecar: Some(crate::AgentOsSidecarConfig::Explicit {
                handle: sidecar.clone(),
            }),
            ..Default::default()
        })
        .await;
        assert!(result.is_err());
        assert_eq!(sidecar.describe().active_vm_count, 0);
        assert_eq!(sidecar.describe().state, SidecarState::Disposed);
    }

    fn shared(pool: &str, state: SidecarState) -> Arc<AgentOsSidecar> {
        let sidecar = Arc::new(AgentOsSidecar::new(
            format!("agentos-shared-sidecar:{pool}"),
            AgentOsSidecarPlacement::Shared {
                pool: Some(pool.to_string()),
            },
            Some(pool.to_string()),
            None,
        ));
        sidecar.state.store(state.as_u8(), Ordering::SeqCst);
        sidecar
    }

    #[test]
    fn sidecar_binary_path_prefers_explicit_wrapper_path() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(AGENTOS_SIDECAR_BIN_ENV).ok();
        std::env::set_var(AGENTOS_SIDECAR_BIN_ENV, "/tmp/from-env");
        let sidecar = AgentOsSidecar::new(
            "explicit-test",
            AgentOsSidecarPlacement::Explicit {
                sidecar_id: "explicit-test".to_string(),
            },
            None,
            Some("/tmp/from-config".to_string()),
        );

        assert_eq!(sidecar.resolved_sidecar_binary_path(), "/tmp/from-config");

        restore_env(AGENTOS_SIDECAR_BIN_ENV, previous);
    }

    #[test]
    fn sidecar_binary_path_uses_agent_os_env_fallback() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(AGENTOS_SIDECAR_BIN_ENV).ok();
        std::env::set_var(AGENTOS_SIDECAR_BIN_ENV, "/tmp/agentos-sidecar");
        let sidecar = shared("env-test", SidecarState::Ready);

        assert_eq!(
            sidecar.resolved_sidecar_binary_path(),
            "/tmp/agentos-sidecar"
        );

        restore_env(AGENTOS_SIDECAR_BIN_ENV, previous);
    }

    #[test]
    fn sidecar_binary_path_defaults_to_agent_os_wrapper() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(AGENTOS_SIDECAR_BIN_ENV).ok();
        std::env::remove_var(AGENTOS_SIDECAR_BIN_ENV);
        let sidecar = shared("default-test", SidecarState::Ready);

        assert_eq!(sidecar.resolved_sidecar_binary_path(), "agentos-sidecar");

        restore_env(AGENTOS_SIDECAR_BIN_ENV, previous);
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn prune_disposed_shared_sidecars_keeps_live_entries() {
        let cache = SccHashMap::new();
        let _ = cache.insert(
            (None, "live".to_string()),
            shared("live", SidecarState::Ready),
        );
        let _ = cache.insert(
            (None, "disposed".to_string()),
            shared("disposed", SidecarState::Disposed),
        );

        prune_disposed_shared_sidecars(&cache);

        assert_eq!(shared_sidecar_pool_len(&cache), 1);
        assert!(cache.read(&(None, "live".to_string()), |_, _| ()).is_some());
        assert!(cache
            .read(&(None, "disposed".to_string()), |_, _| ())
            .is_none());
    }

    #[test]
    fn shared_sidecar_pool_capacity_rejects_full_live_cache() {
        let cache = SccHashMap::new();
        for index in 0..SHARED_SIDECAR_POOL_LIMIT {
            let pool = format!("pool-{index}");
            let _ = cache.insert((None, pool.clone()), shared(&pool, SidecarState::Ready));
        }

        let error =
            ensure_shared_sidecar_pool_capacity(&cache).expect_err("full cache should reject");

        assert!(
            error
                .to_string()
                .contains("shared sidecar pool limit exceeded"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn shared_sidecar_pool_capacity_allows_after_pruning_disposed_entries() {
        let cache = SccHashMap::new();
        for index in 0..SHARED_SIDECAR_POOL_LIMIT {
            let pool = format!("pool-{index}");
            let state = if index == 0 {
                SidecarState::Disposed
            } else {
                SidecarState::Ready
            };
            let _ = cache.insert((None, pool.clone()), shared(&pool, state));
        }

        prune_disposed_shared_sidecars(&cache);

        ensure_shared_sidecar_pool_capacity(&cache).expect("pruned cache should admit one entry");
        assert_eq!(
            shared_sidecar_pool_len(&cache),
            SHARED_SIDECAR_POOL_LIMIT - 1
        );
    }

    #[tokio::test]
    async fn get_shared_sidecar_inserts_vacant_pool_without_reentrant_scan() {
        let pool = format!("unit-{}", Uuid::new_v4());
        let sidecar = AgentOs::get_shared_sidecar(Some(pool.clone()), None)
            .await
            .expect("shared sidecar");

        assert_eq!(sidecar.shared_pool.as_deref(), Some(pool.as_str()));
        let reused = AgentOs::get_shared_sidecar(Some(pool), None)
            .await
            .expect("same-runtime sidecar");
        assert!(Arc::ptr_eq(&sidecar, &reused));

        sidecar.dispose().await.expect("dispose shared sidecar");
    }

    #[test]
    fn dispose_removes_only_same_shared_sidecar_instance() {
        let pool = format!("dispose-race-{}", Uuid::new_v4());
        let old = shared(&pool, SidecarState::Ready);
        let replacement = shared(&pool, SidecarState::Ready);
        let cache = shared_sidecars();
        let _guard = shared_sidecar_pool_lock().lock();

        let _ = cache.insert((None, pool.clone()), replacement.clone());
        old.state
            .store(SidecarState::Disposing.as_u8(), Ordering::SeqCst);
        old.active_vm_count.store(0, Ordering::SeqCst);
        old.state
            .store(SidecarState::Disposed.as_u8(), Ordering::SeqCst);
        let old_ptr = Arc::as_ptr(&old);
        let _ = cache.remove_if(&(None, pool.clone()), |cached| {
            std::ptr::eq(Arc::as_ptr(cached), old_ptr)
        });

        let cached = cache
            .read(&(None, pool.clone()), |_, cached| cached.clone())
            .expect("replacement should remain cached");
        assert!(Arc::ptr_eq(&cached, &replacement));

        let replacement_ptr = Arc::as_ptr(&replacement);
        let _ = cache.remove_if(&(None, pool.clone()), |cached| {
            std::ptr::eq(Arc::as_ptr(cached), replacement_ptr)
        });
    }
    #[tokio::test]
    async fn worker_shutdown_reaps_cache_retired_and_current_owners_without_preload() {
        let pool = format!("worker-owner-{}", Uuid::new_v4());
        let runtime = tokio::runtime::Handle::current().id();
        let old = AgentOs::get_shared_sidecar(Some(pool.clone()), None)
            .await
            .unwrap();
        let weak_old = Arc::downgrade(&old);
        // Cache replacement must not discard ownership of a not-yet-reaped child.
        // No native child is started in this deterministic ownership test.
        shared_sidecars()
            .remove(&(Some(runtime), pool.clone()))
            .unwrap();
        let current = AgentOs::get_shared_sidecar(Some(pool), None).await.unwrap();
        assert!(!Arc::ptr_eq(&old, &current));
        drop(old);
        assert!(
            weak_old.upgrade().is_some(),
            "retired owner escaped worker tracking"
        );
        shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1))
            .await
            .unwrap();
        assert!(weak_old.upgrade().is_none());
        assert_eq!(current.describe().state, SidecarState::Disposed);
        assert!(
            AgentOs::get_shared_sidecar(Some("after-terminal-close".into()), None)
                .await
                .is_err()
        );
        shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn worker_shutdown_waits_for_inflight_connection_and_rejects_new_leases() {
        let owner =
            AgentOs::get_shared_sidecar(Some(format!("worker-inflight-{}", Uuid::new_v4())), None)
                .await
                .unwrap();
        let connection = owner.connection.lock().await;
        let drain = shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1));
        tokio::pin!(drain);
        assert!(futures::poll!(&mut drain).is_pending());
        assert_eq!(owner.describe().state, SidecarState::Disposing);
        assert!(
            AgentOs::get_shared_sidecar(Some("during-terminal-close".into()), None)
                .await
                .is_err()
        );
        drop(connection);
        drain.await.unwrap();
        assert!(owner.acquire_vm_lease().await.is_err());
        assert!(owner.ensure_connection().await.is_err());
    }

    #[tokio::test]
    async fn worker_shutdown_timeout_retains_retryable_ownership() {
        let owner =
            AgentOs::get_shared_sidecar(Some(format!("worker-timeout-{}", Uuid::new_v4())), None)
                .await
                .unwrap();
        let connection = owner.connection.lock().await;
        let error = shutdown_runtime_shared_sidecars(std::time::Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(matches!(error, ClientError::OperationTimedOut { .. }));
        assert_eq!(owner.describe().state, SidecarState::Disposing);
        assert!(
            AgentOs::get_shared_sidecar(Some("after-deadline".into()), None)
                .await
                .is_err()
        );
        drop(connection);
        shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(owner.describe().state, SidecarState::Disposed);
    }

    #[tokio::test]
    async fn worker_shutdown_cancellation_preserves_terminal_fence_and_retry() {
        let owner =
            AgentOs::get_shared_sidecar(Some(format!("worker-cancel-{}", Uuid::new_v4())), None)
                .await
                .unwrap();
        let connection = owner.connection.lock().await;
        {
            let drain = shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1));
            tokio::pin!(drain);
            assert!(futures::poll!(&mut drain).is_pending());
        }
        assert!(
            AgentOs::get_shared_sidecar(Some("after-cancel".into()), None)
                .await
                .is_err()
        );
        drop(connection);
        shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(owner.describe().state, SidecarState::Disposed);
    }

    #[test]
    fn worker_shutdown_is_scoped_to_the_calling_runtime() {
        let first = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let second = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let first_owner = first
            .block_on(AgentOs::get_shared_sidecar(Some("isolation".into()), None))
            .unwrap();
        let second_owner = second
            .block_on(AgentOs::get_shared_sidecar(Some("isolation".into()), None))
            .unwrap();
        first
            .block_on(shutdown_runtime_shared_sidecars(
                std::time::Duration::from_secs(1),
            ))
            .unwrap();
        assert_eq!(first_owner.describe().state, SidecarState::Disposed);
        assert_eq!(second_owner.describe().state, SidecarState::Ready);
        let reused = second
            .block_on(AgentOs::get_shared_sidecar(Some("isolation".into()), None))
            .unwrap();
        assert!(Arc::ptr_eq(&reused, &second_owner));
        second
            .block_on(shutdown_runtime_shared_sidecars(
                std::time::Duration::from_secs(1),
            ))
            .unwrap();
    }
    #[tokio::test]
    async fn worker_cache_retirement_preserves_unreaped_owner() {
        let runtime = tokio::runtime::Handle::current().id();
        let pool = format!("worker-retired-{}", Uuid::new_v4());
        let old = AgentOs::get_shared_sidecar(Some(pool.clone()), None)
            .await
            .unwrap();
        let weak = Arc::downgrade(&old);
        shared_sidecars().remove(&(Some(runtime), pool)).unwrap();
        drop(old);
        let retained = weak
            .upgrade()
            .expect("cache retirement must not abandon an unreaped worker sidecar owner");
        retained.dispose().await.unwrap();
    }
    #[tokio::test]
    async fn worker_shutdown_fences_every_existing_owner_before_bounded_drain() {
        let mut owners = Vec::new();
        for _ in 0..5 {
            owners.push(
                AgentOs::get_shared_sidecar(Some(format!("worker-batch-{}", Uuid::new_v4())), None)
                    .await
                    .unwrap(),
            );
        }
        let mut blocked = Vec::new();
        for owner in owners.iter().take(4) {
            blocked.push(owner.connection.lock().await);
        }
        let drain = shutdown_runtime_shared_sidecars(std::time::Duration::from_secs(1));
        tokio::pin!(drain);
        assert!(futures::poll!(&mut drain).is_pending());
        assert_eq!(owners[4].describe().state, SidecarState::Disposing);
        assert!(
            owners[4].acquire_vm_lease().await.is_err(),
            "unpolled fifth owner admitted a VM lease after terminal close"
        );
        assert!(
            owners[4].ensure_connection().await.is_err(),
            "unpolled fifth owner opened a new child after terminal close"
        );
        drop(blocked);
        drain.await.unwrap();
        assert!(owners
            .iter()
            .all(|owner| owner.describe().state == SidecarState::Disposed));
    }
}
