use std::future::Future;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;

/// Poll `waiter` on its own thread with no runtime. A waiter that never returns
/// `Pending` while capacity is full would hold a single runtime worker forever;
/// here it fails the test instead of hanging it. After the first poll parks,
/// `release` frees capacity and the next poll must admit.
pub(crate) fn assert_parks_until_released<F, T>(waiter: F, release: impl FnOnce()) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (polled_tx, polled_rx) = mpsc::channel();
    let (released_tx, released_rx) = mpsc::channel::<()>();
    let poller = thread::spawn(move || {
        let mut context = Context::from_waker(Waker::noop());
        let mut waiter = Box::pin(waiter);
        let parked = waiter.as_mut().poll(&mut context).is_pending();
        polled_tx.send(parked).expect("test thread is waiting");
        released_rx.recv().expect("test thread releases capacity");
        match waiter.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("a released capacity must admit the waiter"),
        }
    });
    let parked = polled_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the first poll must return instead of retrying its own failed admission");
    assert!(parked, "the waiter must park while capacity is full");
    release();
    released_tx.send(()).expect("poller thread is waiting");
    poller.join().expect("poller thread")
}
