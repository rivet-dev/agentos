//! Return native heap memory to the OS after VM disposal.
//!
//! V8 isolates allocate through glibc malloc, which keeps freed arena pages
//! mapped after an isolate is destroyed. A disposed VM therefore leaves a high
//! RSS watermark unless the sidecar asks glibc to release free pages.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agentos_driver_tokio::{BlockingJobError, DriverHandle, TaskClass};

/// Coalescing window. Every disposal inside the window shares one trim, so VM
/// churn causes at most one trim per window across the whole process.
const TRIM_COALESCE_WINDOW: Duration = Duration::from_secs(1);

static TRIM_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Schedule one coalesced native heap trim on the shared blocking executor.
///
/// A no-op unless the sidecar runs on Linux with glibc.
pub(crate) fn schedule_trim_after_vm_disposal(runtime: &DriverHandle) {
    if !cfg!(all(target_os = "linux", target_env = "gnu")) {
        return;
    }
    if TRIM_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let blocking = runtime.blocking().clone();
    let spawned = runtime.spawn(TaskClass::Timer, async move {
        tokio::time::sleep(TRIM_COALESCE_WINDOW).await;
        // Clear before the trim so a disposal that finishes during the trim
        // schedules a later one instead of being absorbed by this one.
        TRIM_SCHEDULED.store(false, Ordering::Release);
        match blocking
            .run(
                0,
                agentos_executor_v8_runtime::native_heap::release_free_native_heap,
            )
            .await
        {
            Ok(_) => {}
            Err(BlockingJobError::ShuttingDown) => {
                tracing::debug!("skipped native heap trim: sidecar runtime is shutting down");
            }
            Err(error) => {
                tracing::warn!(%error, "ERR_AGENTOS_NATIVE_HEAP_TRIM: native heap trim failed");
            }
        }
    });
    if let Err(error) = spawned {
        TRIM_SCHEDULED.store(false, Ordering::Release);
        tracing::warn!(%error, "ERR_AGENTOS_NATIVE_HEAP_TRIM: failed to schedule native heap trim");
    }
}
