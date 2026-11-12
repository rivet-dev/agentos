use super::*;
use std::future::Future;

// Only this test installs this path-keyed gate; unrelated materializers proceed.
type MaterializationGate = (
    PathBuf,
    Box<dyn FnOnce() -> MaterializationCompletion + Send>,
);
static MATERIALIZATION_GATE: Mutex<Option<MaterializationGate>> = Mutex::new(None);

pub(super) struct MaterializationCompletion(Option<std::sync::mpsc::SyncSender<()>>);
impl Drop for MaterializationCompletion {
    fn drop(&mut self) {
        if let Some(done) = self.0.take() {
            done.send(())
                .expect("test still owns materializer completion receiver");
        }
    }
}

pub(super) fn before_materialize(path: &Path) -> MaterializationCompletion {
    let gate = {
        let mut slot = MATERIALIZATION_GATE
            .lock()
            .expect("materialization test gate");
        if slot.as_ref().is_some_and(|(expected, _)| expected == path) {
            slot.take().map(|(_, gate)| gate)
        } else {
            None
        }
    };
    gate.map_or(MaterializationCompletion(None), |gate| gate())
}

#[test]
fn cancelled_startup_retains_root_until_its_actual_materializer_retires() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = NodeImportCache::new_in(temporary.path().to_path_buf());
    let root = cache.root_dir.clone();
    let retirement = cache.cleanup_guard().prepare_retirement();
    retirement.arm();
    let (entered_tx, mut entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    *MATERIALIZATION_GATE.lock().unwrap() = Some((
        root.clone(),
        Box::new(move || {
            entered_tx.send(()).expect("announce actual materializer");
            // Sender drop releases the worker too, including assertion unwinding.
            match release_rx.recv() {
                Ok(()) | Err(_) => {}
            }
            MaterializationCompletion(Some(done_tx))
        }),
    ));
    let runtime = crate::test_runtime_context();
    let waiter = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let entered = waiter.block_on(async {
        let mut startup =
            Box::pin(cache.ensure_materialized_with_timeout_and_runtime_async(
                &runtime,
                Duration::from_secs(10),
            ));
        let entered = time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| {
                if let std::task::Poll::Ready(result) = startup.as_mut().poll(cx) {
                    return std::task::Poll::Ready(Err(format!(
                        "startup completed before writer gate: {result:?}"
                    )));
                }
                match std::pin::Pin::new(&mut entered_rx).poll(cx) {
                    std::task::Poll::Ready(result) => {
                        std::task::Poll::Ready(result.map_err(|error| error.to_string()))
                    }
                    std::task::Poll::Pending => std::task::Poll::Pending,
                }
            }),
        )
        .await;
        drop(startup); // Cancel the real startup while its admitted worker is writing.
        entered
    });
    drop(cache);
    let retained_while_writer_live = root.exists()
        && !retirement.0.released.load(Ordering::Acquire)
        && retirement.remove_released().is_err();
    // Drain the actual writer independently of the retirement notification, so
    // even a broken early notification cannot leave the RED worker running.
    drop(release_tx);
    let worker_done = done_rx.recv_timeout(Duration::from_secs(5));
    let released = waiter.block_on(async {
        time::timeout(Duration::from_secs(5), retirement.wait_released()).await
    });
    let removed = runtime.blocking().run_sync(1024, Duration::from_secs(5), {
        let retirement = retirement.clone();
        move || retirement.remove_released()
    });
    *MATERIALIZATION_GATE.lock().unwrap() = None;
    entered
        .expect("actual writer enters before cancellation deadline")
        .expect("actual writer gate");
    assert!(
        retained_while_writer_live,
        "cancelled startup must not retire a live writer's root"
    );
    worker_done.expect("actual materializer has stopped writing");
    released.expect("actual materializer releases its lease");
    removed
        .expect("bounded deletion worker")
        .expect("physical deletion");
    assert!(
        !root.exists(),
        "no writer remains to recreate a successfully retired root"
    );
}

#[test]
fn staged_unarmed_retirement_preserves_standalone_drop_cleanup() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = NodeImportCache::new_in(temporary.path().to_path_buf());
    fs::create_dir_all(&cache.root_dir).unwrap();
    let record = cache.cleanup_guard().prepare_retirement();
    let root = record.root_path().to_path_buf();
    // Models failed lifecycle admission: staging alone never transfers authority.
    drop(record);
    assert!(root.exists());
    drop(cache);
    assert!(!root.exists());
}
