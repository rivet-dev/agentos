//! `AgentOsSidecar` (public transport handle) + placement/description + the process-global shared
//! pool + internal lease/vm-admin.
//!
//! Ported from `packages/core/src/agent-os.ts` (`AgentOsSidecar`) and the internal vm-admin layer.
//! The shared-sidecar pool is a process-global map (default pool `"default"`); `create_vm` /
//! `get_vm_admin` / `dispose_vm` are internal and never public on `AgentOs`.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use once_cell::sync::OnceCell;
use scc::HashMap as SccHashMap;
use uuid::Uuid;

use crate::agent_os::AgentOs;
use crate::error::ClientError;

/// Sidecar lifecycle state, encoded as a `u8` for `AtomicU8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarState {
    Ready,
    Disposing,
    Disposed,
}

impl SidecarState {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOsSidecarPlacement {
    Shared { pool: Option<String> },
    Explicit { sidecar_id: String },
}

/// A sync, deep-clone snapshot of a sidecar's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOsSidecarDescription {
    pub sidecar_id: String,
    pub placement: AgentOsSidecarPlacement,
    pub state: SidecarState,
    pub active_vm_count: u32,
}

/// Public transport handle for a (possibly shared) native sidecar process hosting VMs.
pub struct AgentOsSidecar {
    pub(crate) sidecar_id: String,
    pub(crate) placement: AgentOsSidecarPlacement,
    pub(crate) shared_pool: Option<String>,
    pub(crate) state: AtomicU8,
    pub(crate) active_vm_count: AtomicU32,
    // TODO(parity: hold the transport handle / vm-admin registry once create_vm lands).
}

impl AgentOsSidecar {
    /// Construct a sidecar handle.
    pub(crate) fn new(
        sidecar_id: impl Into<String>,
        placement: AgentOsSidecarPlacement,
        shared_pool: Option<String>,
    ) -> Self {
        Self {
            sidecar_id: sidecar_id.into(),
            placement,
            shared_pool,
            state: AtomicU8::new(SidecarState::Ready.as_u8()),
            active_vm_count: AtomicU32::new(0),
        }
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
            state: SidecarState::from_u8(self.state.load(Ordering::SeqCst)),
            active_vm_count: self.active_vm_count.load(Ordering::SeqCst),
        }
    }

    /// Dispose the sidecar. Idempotent; disposes active leases and aggregates errors.
    ///
    /// Parity with TypeScript `AgentOsSidecar.dispose()`:
    /// 1. If already `disposed`, return immediately (idempotent).
    /// 2. Transition to `disposing`.
    /// 3. Dispose every active lease, collecting (not short-circuiting on) errors.
    /// 4. Reset `active_vm_count` to 0 and transition to `disposed`.
    /// 5. If this sidecar is the cached shared sidecar for its pool, remove it from the pool.
    /// 6. If any lease disposal failed, return an aggregated error.
    pub async fn dispose(&self) -> Result<(), ClientError> {
        if SidecarState::from_u8(self.state.load(Ordering::SeqCst)) == SidecarState::Disposed {
            return Ok(());
        }

        self.state
            .store(SidecarState::Disposing.as_u8(), Ordering::SeqCst);

        let errors: Vec<String> = Vec::new();

        // Parity note: TypeScript iterates `state.activeLeases` here and aggregates per-lease
        // disposal errors. Active leases are owned by `AgentOs` (via
        // `AgentOsInner.sidecar_lease`) and are released through `AgentOsSidecarVmLease::dispose`
        // during `AgentOs::shutdown`. The shared active-lease registry is part of the
        // create_vm / vm-admin transport layer, which is not yet wired (see the `SidecarVmAdmin`
        // TODO above). Once that lands, drain it here and push any disposal errors into `errors`.
        self.active_vm_count.store(0, Ordering::SeqCst);
        self.state
            .store(SidecarState::Disposed.as_u8(), Ordering::SeqCst);

        if let Some(pool) = self.shared_pool.as_deref() {
            // Only remove the cached entry if it still points at this exact sidecar instance.
            let self_id = self.sidecar_id.as_str();
            let _ = shared_sidecars().remove_if(pool, |cached| cached.sidecar_id == self_id);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ClientError::Sidecar(errors.join("; ")))
        }
    }
}

/// Internal VM admin held behind a lease. Not public.
pub(crate) trait SidecarVmAdmin: Send + Sync {
    // TODO(parity: model the vm-admin surface: kernel/rootView/mounts/sidecar session, etc.).
}

/// A lease over a VM admin; released on `AgentOs` dispose.
pub(crate) struct AgentOsSidecarVmLease {
    pub(crate) vm_id: String,
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    // TODO(parity: hold the admin + release wiring).
}

impl AgentOsSidecarVmLease {
    /// Release the lease.
    ///
    /// Parity with the TypeScript lease `dispose()`: it is idempotent, removes itself from the
    /// owning sidecar's active-lease set, recomputes `activeVmCount`, and disposes the underlying
    /// session transport client. Consuming `self` here gives the idempotence for free (the lease
    /// cannot be disposed twice). The active-vm count is decremented (saturating at 0) to mirror
    /// `state.description.activeVmCount = state.activeLeases.size`.
    ///
    /// Parity note: the underlying session/transport `client.dispose()` is part of the create_vm /
    /// vm-admin transport layer, which is not yet wired (see the `SidecarVmAdmin` TODO above). Once
    /// that lands, dispose the held admin/client here and surface any error.
    pub(crate) async fn dispose(self) -> Result<(), ClientError> {
        let sidecar = self.sidecar;
        // Mirror `activeVmCount = activeLeases.size` by decrementing, never underflowing past 0.
        let mut current = sidecar.active_vm_count.load(Ordering::SeqCst);
        loop {
            let next = current.saturating_sub(1);
            match sidecar.active_vm_count.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
        Ok(())
    }
}

/// Process-global shared-sidecar pool, keyed by pool name (default `"default"`).
static SHARED_SIDECARS: OnceCell<SccHashMap<String, Arc<AgentOsSidecar>>> = OnceCell::new();

/// Access (initializing on first use) the process-global shared-sidecar pool.
pub(crate) fn shared_sidecars() -> &'static SccHashMap<String, Arc<AgentOsSidecar>> {
    SHARED_SIDECARS.get_or_init(SccHashMap::new)
}

impl AgentOs {
    /// Create an explicit sidecar handle. `sidecar_id` defaults to `agent-os-sidecar-<uuid>`.
    ///
    /// Parity with TypeScript `createAgentOsSidecarInternal`: the explicit handle carries an
    /// `Explicit` placement whose `sidecar_id` echoes the resolved id and has no shared pool.
    pub async fn create_sidecar(
        sidecar_id: Option<String>,
    ) -> Result<Arc<AgentOsSidecar>, ClientError> {
        let sidecar_id = sidecar_id.unwrap_or_else(|| format!("agent-os-sidecar-{}", Uuid::new_v4()));
        let placement = AgentOsSidecarPlacement::Explicit {
            sidecar_id: sidecar_id.clone(),
        };
        Ok(Arc::new(AgentOsSidecar::new(sidecar_id, placement, None)))
    }

    /// Get (or create) a pooled shared sidecar. Pool defaults to `"default"`. Uses the process-global
    /// cache.
    ///
    /// Parity with TypeScript `getSharedAgentOsSidecarInternal`: return the cached sidecar for the
    /// pool when it exists and is not disposed; otherwise build a fresh handle
    /// (`agent-os-shared-sidecar:<pool>`, `Shared` placement) and cache it. Because the cache is a
    /// process-global concurrent map rather than a synchronously-checked `Map`, the insert is done
    /// atomically with `entry`/`insert` so two racing callers converge on a single live handle.
    pub async fn get_shared_sidecar(
        pool: Option<String>,
    ) -> Result<Arc<AgentOsSidecar>, ClientError> {
        let pool = pool.unwrap_or_else(|| "default".to_string());
        let cache = shared_sidecars();

        // Fast path: reuse a cached, non-disposed sidecar for this pool.
        if let Some(existing) = cache.read(&pool, |_, sidecar| sidecar.clone()) {
            if existing.describe().state != SidecarState::Disposed {
                return Ok(existing);
            }
        }

        let sidecar = Arc::new(AgentOsSidecar::new(
            format!("agent-os-shared-sidecar:{pool}"),
            AgentOsSidecarPlacement::Shared {
                pool: Some(pool.clone()),
            },
            Some(pool.clone()),
        ));

        // Insert atomically, replacing a stale (disposed) entry but yielding to a live one that a
        // concurrent caller may have just installed.
        match cache.entry(pool) {
            scc::hash_map::Entry::Occupied(mut occupied) => {
                if occupied.get().describe().state == SidecarState::Disposed {
                    *occupied.get_mut() = sidecar.clone();
                    Ok(sidecar)
                } else {
                    Ok(occupied.get().clone())
                }
            }
            scc::hash_map::Entry::Vacant(vacant) => {
                vacant.insert_entry(sidecar.clone());
                Ok(sidecar)
            }
        }
    }
}
