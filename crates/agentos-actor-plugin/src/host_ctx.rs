//! Plugin-side host bridge — the inverse of the RivetKit host vtable impl.
//!
//! Wraps the `HostVtable` the host hands to `rivet_actor_run` and exposes safe
//! async/sync methods the actor run loop calls: durable storage (`db_*`), event
//! pull (`next_event`), replies, broadcast. Each async op is a sync submit +
//! completion callback bridged to an `await` via a oneshot (spec §5.4), driven
//! on the plugin runtime. Depends only on `rivet-actor-plugin-abi` + `tokio`
//! (no `agent-os-client`), so it builds independently of the secure-exec layer.

#![allow(dead_code)]

use std::ffi::c_void;

use rivet_actor_plugin_abi as abi;
use tokio::sync::oneshot;

/// `Send` wrapper so an `AbiResult` (which holds raw pointers) can travel
/// through the oneshot channel and the spawned actor future stays `Send`.
struct SendResult(abi::AbiResult);
unsafe impl Send for SendResult {}

/// Refcounted handle to the host actor context. `Clone` bumps the host refcount
/// (`ctx_clone`) so detached tasks can hold it; `Drop` releases it.
pub(crate) struct HostCtx {
    vtable: abi::HostVtable,
}

unsafe impl Send for HostCtx {}
unsafe impl Sync for HostCtx {}

impl Clone for HostCtx {
    fn clone(&self) -> Self {
        let mut v = self.vtable;
        v.ctx = (self.vtable.ctx_clone)(self.vtable.ctx);
        Self { vtable: v }
    }
}

impl Drop for HostCtx {
    fn drop(&mut self) {
        (self.vtable.ctx_release)(self.vtable.ctx);
    }
}

/// Completion callback the host invokes when an async op finishes: reclaims the
/// boxed oneshot sender and delivers the result. Panic-firewalled.
extern "C" fn complete(user_data: *mut c_void, result: abi::AbiResult) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let tx = Box::from_raw(user_data as *mut oneshot::Sender<SendResult>);
        let _ = tx.send(SendResult(result));
    }));
}

fn decode_result(result: abi::AbiResult) -> Result<Vec<u8>, String> {
    match result.status {
        abi::AbiStatus::Ok => Ok(unsafe { result.payload.into_vec() }),
        _ => {
            let bytes = unsafe { result.payload.into_vec() };
            Err(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// Decode a `next_event` payload: `[tag u32 LE][reply_token u64 LE][bytes]`.
fn decode_event(bytes: &[u8]) -> Option<(u32, u64, Vec<u8>)> {
    if bytes.len() < 12 {
        return None;
    }
    let tag = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let token = u64::from_le_bytes(bytes[4..12].try_into().ok()?);
    Some((tag, token, bytes[12..].to_vec()))
}

impl HostCtx {
    /// Adopt a strong ref to the host ctx handed in via `rivet_actor_run`'s
    /// `HostVtable`. The host retains and releases its own ref independently
    /// after run cleanup, so the plugin clone balances this handle's `Drop`.
    pub(crate) fn from_vtable(vtable: abi::HostVtable) -> Self {
        (vtable.ctx_clone)(vtable.ctx);
        Self { vtable }
    }

    fn ctx(&self) -> *const c_void {
        self.vtable.ctx
    }

    pub(crate) async fn db_exec(&self, sql: Vec<u8>) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel::<SendResult>();
        let ud = Box::into_raw(Box::new(tx)) as *mut c_void;
        (self.vtable.db_exec)(self.ctx(), abi::OwnedBuf::from_vec(sql), complete, ud);
        decode_result(
            rx.await
                .map(|r| r.0)
                .unwrap_or_else(|_| abi::AbiResult::channel_closed()),
        )
    }

    pub(crate) async fn db_query(&self, sql: Vec<u8>, params: Vec<u8>) -> Result<Vec<u8>, String> {
        self.submit_sql(self.vtable.db_query, sql, params).await
    }

    pub(crate) async fn db_run(&self, sql: Vec<u8>, params: Vec<u8>) -> Result<Vec<u8>, String> {
        self.submit_sql(self.vtable.db_run, sql, params).await
    }

    async fn submit_sql(
        &self,
        f: abi::DbSqlFn,
        sql: Vec<u8>,
        params: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel::<SendResult>();
        let ud = Box::into_raw(Box::new(tx)) as *mut c_void;
        f(
            self.ctx(),
            abi::OwnedBuf::from_vec(sql),
            abi::OwnedBuf::from_vec(params),
            complete,
            ud,
        );
        decode_result(
            rx.await
                .map(|r| r.0)
                .unwrap_or_else(|_| abi::AbiResult::channel_closed()),
        )
    }

    /// Pull the next lifecycle event, or `None` when the stream is closed.
    pub(crate) async fn next_event(&self) -> Option<(u32, u64, Vec<u8>)> {
        let (tx, rx) = oneshot::channel::<SendResult>();
        let ud = Box::into_raw(Box::new(tx)) as *mut c_void;
        (self.vtable.next_event)(self.ctx(), complete, ud);
        let result = rx
            .await
            .map(|r| r.0)
            .unwrap_or_else(|_| abi::AbiResult::channel_closed());
        match result.status {
            abi::AbiStatus::Ok => {
                let bytes = unsafe { result.payload.into_vec() };
                decode_event(&bytes)
            }
            _ => {
                unsafe { result.payload.free_self() };
                None
            }
        }
    }

    pub(crate) fn sql_is_enabled(&self) -> bool {
        (self.vtable.sql_is_enabled)(self.ctx()) != 0
    }

    /// Signal actor startup to the host (required: the native-plugin factory is
    /// built with manual startup-ready, so the host's `start()` caller awaits
    /// this before the actor is considered live). `ok = false` reports a fatal
    /// startup error with `msg`.
    pub(crate) fn startup_ready(&self, ok: bool, msg: &str) {
        let err = abi::BorrowedBuf::from_slice(msg.as_bytes());
        (self.vtable.startup_ready)(self.ctx(), u8::from(ok), err);
    }

    pub(crate) fn reply_ok(&self, token: u64, payload: Vec<u8>) -> abi::AbiStatus {
        (self.vtable.reply_ok)(self.ctx(), token, abi::OwnedBuf::from_vec(payload))
    }

    pub(crate) fn reply_err(&self, token: u64, msg: &str) -> abi::AbiStatus {
        (self.vtable.reply_err)(
            self.ctx(),
            token,
            abi::OwnedBuf::from_vec(msg.as_bytes().to_vec()),
        )
    }

    pub(crate) fn broadcast(&self, name: Vec<u8>, payload: Vec<u8>) -> abi::AbiStatus {
        (self.vtable.broadcast)(
            self.ctx(),
            abi::OwnedBuf::from_vec(name),
            abi::OwnedBuf::from_vec(payload),
        )
    }

    pub(crate) fn log_warn(&self, msg: &str) {
        (self.vtable.log)(self.ctx(), 3, abi::BorrowedBuf::from_slice(msg.as_bytes()));
    }
}
