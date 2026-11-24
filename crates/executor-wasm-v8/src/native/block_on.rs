//! A re-entrant `block_on` for the guest thread.
//!
//! A host import runs its shared async handler to completion before returning
//! to V8. A handler can call back into the guest (a signal handler), and the
//! guest can make another import from there, so blocking nests. Tokio's and
//! `futures`' executors both refuse to nest; this one parks the thread and
//! tracks its own wake flag per level, so an outer wake that lands while an
//! inner level is parked is never lost.

use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

struct ThreadWaker {
    thread: Thread,
    woken: AtomicBool,
}

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

/// Drive `future` to completion on the current thread. The caller must have
/// entered the process Tokio runtime so timers and I/O resources resolve.
pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    let waker_state = Arc::new(ThreadWaker {
        thread: std::thread::current(),
        woken: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&waker_state));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        // Parks can return spuriously or because an inner or outer level was
        // woken; only this level's flag means this future can progress.
        while !waker_state.woken.swap(false, Ordering::AcqRel) {
            std::thread::park();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::block_on;

    #[test]
    fn nested_levels_complete() {
        let value = block_on(async {
            let inner = block_on(async { 2 });
            inner + 1
        });
        assert_eq!(value, 3);
    }

    #[test]
    fn cross_thread_wake_completes() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            sender.send(7).expect("receiver alive");
        });
        assert_eq!(block_on(receiver).expect("sent"), 7);
        thread.join().expect("sender thread");
    }
}
