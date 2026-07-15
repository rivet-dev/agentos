//! Pending permission requests for the inspector's permission banner: the
//! plugin-side buffer behind the observe-only `listPendingPermissions` backfill
//! action. The `permissionRequest` broadcast alone is live-only — a request
//! raised while no inspector iframe was open would stay invisible until the
//! runtime's auto-reject timeout — so the permission pump also records each
//! request here until it is answered (`respondPermission`) or expires.
//!
//! [`PendingPermissions`] lives INSIDE [`Vars`] (unlike
//! [`super::health::HealthBuffers`], which deliberately survives sleep):
//! the client-side reply slots die with the VM, so a pending entry that
//! outlived its VM would advertise a request that can no longer be answered.
//! `Vars::clear()` on sleep/destroy/run-loop exit drops the buffer with the
//! rest of the per-VM state.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_client::PERMISSION_TIMEOUT_MS;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// Bounded buffer cap: past this the oldest entry is dropped with a
/// host-visible warning (see [`PendingPermissions::insert`]). Requests expire
/// server-side after `PERMISSION_TIMEOUT_MS` anyway, so more than this many
/// simultaneously-pending requests means something is already wrong.
pub const PENDING_PERMISSIONS_CAP: usize = 64;

/// One row of `listPendingPermissions` (TS `PendingPermissionInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionDto {
    /// Client-facing EXTERNAL session id — same key as the
    /// `permissionRequest` broadcast, so the inspector can dedupe on
    /// `sessionId:permissionId`.
    pub session_id: String,
    pub permission_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Raw ACP request params, forwarded verbatim like the broadcast.
    pub params: JsonValue,
    /// Epoch milliseconds when the runtime observed the request.
    pub requested_at: f64,
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Shared, bounded pending-permission buffer. Cloning shares the underlying
/// storage (`Arc`), so the permission pump writes into the same buffer the
/// action dispatchers read from.
#[derive(Clone, Default)]
pub struct PendingPermissions {
    inner: Arc<Mutex<VecDeque<PendingPermissionDto>>>,
}

impl PendingPermissions {
    /// Lock the buffer, recovering from poisoning: the guarded sections are
    /// pure pushes/clones/retains, so a poisoned lock means a panic elsewhere
    /// at worst — permission bookkeeping must not cascade it into more panics.
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<PendingPermissionDto>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Record a pending request. At [`PENDING_PERMISSIONS_CAP`] the oldest
    /// entry is dropped and returned so the caller can surface a host-visible
    /// warning (a `tracing::warn!` fires here regardless — the drop is never
    /// silent). Re-inserting an existing `sessionId:permissionId` replaces the
    /// old entry instead of duplicating it.
    pub fn insert(
        &self,
        session_id: &str,
        permission_id: &str,
        description: Option<&str>,
        params: &JsonValue,
    ) -> Option<PendingPermissionDto> {
        self.insert_at(session_id, permission_id, description, params, now_ms())
    }

    fn insert_at(
        &self,
        session_id: &str,
        permission_id: &str,
        description: Option<&str>,
        params: &JsonValue,
        now: f64,
    ) -> Option<PendingPermissionDto> {
        let mut buf = self.lock();
        expire_in_place(&mut buf, now);
        buf.retain(|entry| {
            entry.session_id != session_id || entry.permission_id != permission_id
        });
        buf.push_back(PendingPermissionDto {
            session_id: session_id.to_owned(),
            permission_id: permission_id.to_owned(),
            description: description.map(str::to_owned),
            params: params.clone(),
            requested_at: now,
        });
        if buf.len() > PENDING_PERMISSIONS_CAP {
            let dropped = buf.pop_front();
            if let Some(dropped) = &dropped {
                tracing::warn!(
                    session_id = %dropped.session_id,
                    permission_id = %dropped.permission_id,
                    cap = PENDING_PERMISSIONS_CAP,
                    "pending permission buffer full; dropped the oldest request",
                );
            }
            return dropped;
        }
        None
    }

    /// Snapshot the still-pending requests, oldest first (expired entries are
    /// swept before the snapshot).
    pub fn list(&self) -> Vec<PendingPermissionDto> {
        self.list_at(now_ms())
    }

    fn list_at(&self, now: f64) -> Vec<PendingPermissionDto> {
        let mut buf = self.lock();
        expire_in_place(&mut buf, now);
        buf.iter().cloned().collect()
    }

    /// True when `sessionId:permissionId` is still pending (expired entries
    /// are swept first).
    pub fn contains(&self, session_id: &str, permission_id: &str) -> bool {
        self.contains_at(session_id, permission_id, now_ms())
    }

    fn contains_at(&self, session_id: &str, permission_id: &str, now: f64) -> bool {
        let mut buf = self.lock();
        expire_in_place(&mut buf, now);
        buf.iter()
            .any(|entry| entry.session_id == session_id && entry.permission_id == permission_id)
    }

    /// Remove an answered request; `false` when it was absent (already
    /// answered, expired, or never buffered).
    pub fn remove(&self, session_id: &str, permission_id: &str) -> bool {
        self.remove_at(session_id, permission_id, now_ms())
    }

    fn remove_at(&self, session_id: &str, permission_id: &str, now: f64) -> bool {
        let mut buf = self.lock();
        expire_in_place(&mut buf, now);
        let before = buf.len();
        buf.retain(|entry| {
            entry.session_id != session_id || entry.permission_id != permission_id
        });
        buf.len() != before
    }

    /// Drop everything. Called from `Vars::clear()` on VM teardown: the reply
    /// slots the entries point at die with the VM.
    pub fn clear(&self) {
        self.lock().clear();
    }
}

/// Sweep entries older than the runtime's `PERMISSION_TIMEOUT_MS`: past that
/// the client auto-rejected the request, so the reply slot is gone and the
/// entry only advertises an unanswerable card. Debug-logged, not warned — the
/// auto-reject is the runtime's documented behavior, not a fault here.
fn expire_in_place(buf: &mut VecDeque<PendingPermissionDto>, now: f64) {
    let timeout = PERMISSION_TIMEOUT_MS as f64;
    let before = buf.len();
    buf.retain(|entry| now - entry.requested_at < timeout);
    let expired = before - buf.len();
    if expired > 0 {
        tracing::debug!(expired, "swept expired pending permission requests");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn insert_n(pending: &PendingPermissions, count: usize, at: f64) {
        for i in 0..count {
            pending.insert_at("session-1", &format!("perm-{i}"), None, &json!({}), at);
        }
    }

    #[test]
    fn insert_drops_oldest_at_cap_and_returns_it_for_the_warning() {
        let pending = PendingPermissions::default();
        insert_n(&pending, PENDING_PERMISSIONS_CAP, 0.0);
        // The cap'th insert must evict the oldest and hand it back so the
        // pump can log the host-visible warning.
        let dropped = pending
            .insert_at("session-1", "perm-overflow", None, &json!({}), 0.0)
            .expect("insert past the cap returns the dropped entry");
        assert_eq!(dropped.permission_id, "perm-0", "oldest entry dropped");

        let rows = pending.list_at(0.0);
        assert_eq!(rows.len(), PENDING_PERMISSIONS_CAP);
        assert_eq!(rows[0].permission_id, "perm-1");
        assert_eq!(
            rows.last().unwrap().permission_id,
            "perm-overflow",
            "newest entry kept"
        );
    }

    #[test]
    fn expiry_sweep_drops_stale_entries_on_access() {
        let pending = PendingPermissions::default();
        pending.insert_at("session-1", "perm-old", None, &json!({}), 0.0);
        pending.insert_at("session-1", "perm-new", None, &json!({}), 1_000.0);

        // At exactly the timeout the old entry's reply slot has auto-rejected.
        let now = PERMISSION_TIMEOUT_MS as f64;
        let rows = pending.list_at(now);
        assert_eq!(rows.len(), 1, "expired entry swept on list");
        assert_eq!(rows[0].permission_id, "perm-new");
        assert!(!pending.contains_at("session-1", "perm-old", now));
        assert!(!pending.remove_at("session-1", "perm-old", now));
    }

    #[test]
    fn insert_remove_lifecycle_round_trips() {
        let pending = PendingPermissions::default();
        assert!(pending
            .insert_at(
                "session-1",
                "perm-1",
                Some("run a command"),
                &json!({ "toolCall": { "title": "Bash" } }),
                0.0,
            )
            .is_none());
        assert!(pending.contains_at("session-1", "perm-1", 0.0));

        assert!(pending.remove_at("session-1", "perm-1", 0.0));
        assert!(!pending.contains_at("session-1", "perm-1", 0.0));
        assert!(
            !pending.remove_at("session-1", "perm-1", 0.0),
            "second remove reports the entry as gone"
        );
        assert!(pending.list_at(0.0).is_empty());
    }

    #[test]
    fn reinserting_the_same_key_replaces_instead_of_duplicating() {
        let pending = PendingPermissions::default();
        pending.insert_at("session-1", "perm-1", Some("first"), &json!({}), 0.0);
        pending.insert_at("session-1", "perm-1", Some("second"), &json!({}), 1.0);

        let rows = pending.list_at(1.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].description.as_deref(), Some("second"));
    }
}
