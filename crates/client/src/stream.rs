//! Streaming / subscription primitives.
//!
//! Implements `spec.md` §5 / ADR-001 §5. The TypeScript `on*(id, handler) -> unsubscribe` pattern
//! becomes streams + a uniform RAII [`Subscription`] guard:
//!
//! - process stdout/stderr, shell data, and cron events ->
//!   [`tokio::sync::broadcast`] (multi-subscriber; no replay).
//! - process exit -> [`tokio::sync::watch`] seeded `None` (already-exited branch fires immediately
//!   because the watch already holds `Some(code)`).
//! - permission responder + internal single-reply correlation -> [`tokio::sync::oneshot`].

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::broadcast;
use tokio_util::sync::ReusableBoxFuture;

type ByteRecvResult = Result<Vec<u8>, broadcast::error::RecvError>;
type ByteRecvState = (ByteRecvResult, broadcast::Receiver<Vec<u8>>);

/// RAII guard returned by `on_*` register methods. Dropping it deregisters the subscription.
///
/// For broadcast/watch-backed subscriptions, dropping the returned stream/receiver is itself the
/// unsubscribe; this guard wraps an optional deregistration closure for the cases (idempotent
/// handler removal) that need explicit cleanup.
#[must_use = "dropping the Subscription immediately unsubscribes"]
pub struct Subscription {
    on_drop: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Subscription {
    /// Create a subscription guard whose `Drop` runs `on_drop`.
    pub fn new(on_drop: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self {
            on_drop: Some(Box::new(on_drop)),
        }
    }

    /// Create a no-op subscription guard (used when dropping the returned stream is the unsubscribe).
    pub fn noop() -> Self {
        Self { on_drop: None }
    }

    /// Detach the guard so dropping it no longer deregisters (subscription becomes permanent).
    pub fn detach(mut self) {
        self.on_drop = None;
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("active", &self.on_drop.is_some())
            .finish()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(on_drop) = self.on_drop.take() {
            on_drop();
        }
    }
}

/// A byte stream over a broadcast channel (process stdout/stderr, shell data).
///
/// Lagged messages are skipped. Closing the sender ends the stream.
pub struct ByteStream {
    inner: ReusableBoxFuture<'static, ByteRecvState>,
}

impl ByteStream {
    /// Wrap a broadcast receiver as a [`Stream`] of byte chunks.
    pub fn new(rx: broadcast::Receiver<Vec<u8>>) -> Self {
        Self {
            inner: ReusableBoxFuture::new(recv_bytes(rx)),
        }
    }
}

async fn recv_bytes(mut rx: broadcast::Receiver<Vec<u8>>) -> ByteRecvState {
    let result = rx.recv().await;
    (result, rx)
}

impl Stream for ByteStream {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let (result, rx) = match self.inner.poll(cx) {
                Poll::Ready(value) => value,
                Poll::Pending => return Poll::Pending,
            };
            self.inner.set(recv_bytes(rx));
            match result {
                Ok(bytes) => return Poll::Ready(Some(bytes)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return Poll::Ready(None),
            }
        }
    }
}
