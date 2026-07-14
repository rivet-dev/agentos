//! Streaming / subscription primitives.
//!
//! Implements `spec.md` §5 / ADR-001 §5. The TypeScript `on*(id, handler) -> unsubscribe` pattern
//! becomes streams + a uniform RAII [`Subscription`] guard:
//!
//! - process stdout/stderr, shell data, session events, permission requests, cron events ->
//!   [`tokio::sync::broadcast`] (multi-subscriber; no replay).
//! - process exit -> [`tokio::sync::watch`] seeded `None` (already-exited branch fires immediately
//!   because the watch already holds `Some(code)`).
//! - permission responder + internal single-reply correlation -> [`tokio::sync::oneshot`].

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::broadcast;
use tokio_util::sync::ReusableBoxFuture;

use crate::error::ClientError;

#[derive(Debug, Clone)]
pub(crate) enum RoutedStreamEvent<T> {
    Data(T),
    Lagged { skipped: u64 },
    Closed { context: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamRouteFailure {
    Lagged { skipped: u64 },
    Closed { context: &'static str },
}

impl StreamRouteFailure {
    pub(crate) fn event<T>(self) -> RoutedStreamEvent<T> {
        match self {
            Self::Lagged { skipped } => RoutedStreamEvent::Lagged { skipped },
            Self::Closed { context } => RoutedStreamEvent::Closed { context },
        }
    }
}

type ByteRecvResult = Result<RoutedStreamEvent<Vec<u8>>, broadcast::error::RecvError>;
type ByteRecvState = (
    ByteRecvResult,
    broadcast::Receiver<RoutedStreamEvent<Vec<u8>>>,
);

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
/// Lag is returned once as a typed terminal error. Closing the sender ends the stream.
pub struct ByteStream {
    inner: ReusableBoxFuture<'static, ByteRecvState>,
    terminated: bool,
}

impl ByteStream {
    /// Wrap a broadcast receiver as a [`Stream`] of byte chunks.
    pub(crate) fn new(rx: broadcast::Receiver<RoutedStreamEvent<Vec<u8>>>) -> Self {
        Self {
            inner: ReusableBoxFuture::new(recv_bytes(rx)),
            terminated: false,
        }
    }

    pub(crate) fn failed(failure: RoutedStreamEvent<Vec<u8>>) -> Self {
        let (tx, rx) = broadcast::channel(1);
        tx.send(failure)
            .expect("fresh byte-stream failure receiver must be live");
        drop(tx);
        Self::new(rx)
    }
}

async fn recv_bytes(mut rx: broadcast::Receiver<RoutedStreamEvent<Vec<u8>>>) -> ByteRecvState {
    let result = rx.recv().await;
    (result, rx)
}

impl Stream for ByteStream {
    type Item = Result<Vec<u8>, ClientError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        let (result, rx) = match self.inner.poll(cx) {
            Poll::Ready(value) => value,
            Poll::Pending => return Poll::Pending,
        };
        self.inner.set(recv_bytes(rx));
        match result {
            Ok(RoutedStreamEvent::Data(bytes)) => Poll::Ready(Some(Ok(bytes))),
            Ok(RoutedStreamEvent::Lagged { skipped }) => {
                self.terminated = true;
                Poll::Ready(Some(Err(ClientError::EventStreamLagged { skipped })))
            }
            Ok(RoutedStreamEvent::Closed { context }) => {
                self.terminated = true;
                Poll::Ready(Some(Err(ClientError::EventStreamClosed { context })))
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                self.terminated = true;
                Poll::Ready(Some(Err(ClientError::EventStreamLagged { skipped })))
            }
            Err(broadcast::error::RecvError::Closed) => {
                self.terminated = true;
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteStream, RoutedStreamEvent};
    use crate::error::ClientError;
    use futures::StreamExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn byte_stream_surfaces_lag_with_the_skipped_count() {
        let (tx, rx) = broadcast::channel(1);
        let mut stream = ByteStream::new(rx);
        tx.send(RoutedStreamEvent::Data(vec![1]))
            .expect("first chunk");
        tx.send(RoutedStreamEvent::Data(vec![2]))
            .expect("second chunk");
        tx.send(RoutedStreamEvent::Data(vec![3]))
            .expect("third chunk");

        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 2 }))
        ));
        assert!(stream.next().await.is_none(), "lag terminates the stream");
    }

    #[tokio::test]
    async fn byte_stream_surfaces_upstream_route_lag() {
        let (tx, rx) = broadcast::channel(4);
        let mut stream = ByteStream::new(rx);
        tx.send(RoutedStreamEvent::Lagged { skipped: 7 })
            .expect("route failure");

        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 7 }))
        ));
        assert!(
            stream.next().await.is_none(),
            "route failure terminates the stream"
        );
    }
}
