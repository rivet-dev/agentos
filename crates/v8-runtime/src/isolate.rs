// V8 isolate lifecycle: platform init, create, configure, destroy

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, Once};

use crate::ipc::ExecutionError;
use agentos_bridge::queue_tracker::{warn_limit_exhausted, TrackedLimit};

static V8_INIT: Once = Once::new();
static V8_ISOLATE_LIFECYCLE: Mutex<()> = Mutex::new(());
const MAX_UNHANDLED_PROMISE_REJECTIONS: usize = 1024;

#[repr(align(16))]
struct AlignedBytes<const N: usize>([u8; N]);

static ICU_COMMON_DATA: AlignedBytes<
    { include_bytes!(concat!(env!("OUT_DIR"), "/icudtl.dat")).len() },
> = AlignedBytes(*include_bytes!(concat!(env!("OUT_DIR"), "/icudtl.dat")));

#[derive(Default)]
pub struct PromiseRejectState {
    pub unhandled: HashMap<i32, ExecutionError>,
    overflow_count: usize,
}

impl PromiseRejectState {
    fn record_unhandled(&mut self, promise_id: i32, error: ExecutionError) {
        use std::collections::hash_map::Entry;
        // Cache the length before taking the entry, since `Entry` borrows the
        // map mutably and we cannot read `len()` while it is held.
        let under_limit = self.unhandled.len() < MAX_UNHANDLED_PROMISE_REJECTIONS;
        match self.unhandled.entry(promise_id) {
            // Existing rejection for this promise — overwrite with latest error.
            Entry::Occupied(mut entry) => {
                entry.insert(error);
            }
            // New rejection: store it if under the cap, otherwise count overflow.
            Entry::Vacant(entry) => {
                if under_limit {
                    entry.insert(error);
                } else {
                    self.overflow_count = self.overflow_count.saturating_add(1);
                }
            }
        }
    }

    fn mark_handled(&mut self, promise_id: i32) {
        if self.unhandled.remove(&promise_id).is_none() && self.overflow_count > 0 {
            self.overflow_count -= 1;
        }
    }

    pub fn take_next_unhandled(&mut self) -> Option<ExecutionError> {
        if self.overflow_count > 0 {
            self.overflow_count = 0;
            self.unhandled.clear();
            return Some(ExecutionError {
                error_type: "Error".into(),
                message: format!(
                    "unhandled promise rejection registry exceeded limit of {MAX_UNHANDLED_PROMISE_REJECTIONS} rejections"
                ),
                stack: String::new(),
                code: Some("ERR_AGENTOS_UNHANDLED_REJECTION_LIMIT".into()),
            });
        }
        self.unhandled.drain().next().map(|(_, err)| err)
    }
}

extern "C" fn promise_reject_callback(msg: v8::PromiseRejectMessage) {
    let scope = &mut unsafe { v8::CallbackScope::new(&msg) };
    let promise_id = msg.get_promise().get_identity_hash().get();
    match msg.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let error = {
                let scope = &mut v8::HandleScope::new(scope);
                let value = msg
                    .get_value()
                    .unwrap_or_else(|| v8::undefined(scope).into());
                crate::execution::extract_error_info(scope, value)
            };
            if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
                state.record_unhandled(promise_id, error);
            }
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
                state.mark_handled(promise_id);
            }
        }
        _ => {}
    }
}

pub fn configure_isolate(isolate: &mut v8::OwnedIsolate) {
    isolate.set_slot(PromiseRejectState::default());
    isolate.set_promise_reject_callback(promise_reject_callback);
}

/// Initialize the V8 platform (once per process).
/// Safe to call multiple times; only the first call takes effect.
pub fn init_v8_platform() {
    V8_INIT.call_once(|| {
        v8::icu::set_common_data_74(&ICU_COMMON_DATA.0)
            .expect("failed to initialize V8 ICU common data");
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

// Headroom granted to V8 when the near-heap-limit callback fires. V8 fatal-aborts
// the whole process (SIGTRAP) if the callback does not raise the limit, so we must
// hand back a larger limit to give the engine room to unwind. Termination has
// already been requested, so this extra budget only covers propagation of the
// uncatchable termination exception, not continued guest allocation.
const NEAR_HEAP_LIMIT_HEADROOM_BYTES: usize = 16 * 1024 * 1024;

/// Default per-isolate heap cap applied when the caller passes no explicit limit.
///
/// Resource limits must be bounded by default (never unbounded for memory): a
/// guest with no configured `heap_limit_mb` must NOT be able to grow the heap until
/// V8 fatal-aborts the process-global runtime and takes down every co-tenant
/// isolate. 128 MiB matches the Cloudflare Workers per-isolate budget we mirror for
/// isolation semantics; operators may raise it via the configured limit.
pub const DEFAULT_HEAP_LIMIT_MB: u32 = 128;

/// Invoked by V8 when heap usage approaches the configured limit. Instead of
/// letting V8 fatal-abort the (process-global) runtime, request termination of the
/// offending isolate and return a raised limit so V8 can propagate the uncatchable
/// termination exception cleanly. `data` is a leaked `Box<v8::IsolateHandle>` for
/// the isolate this callback was registered on.
extern "C" fn near_heap_limit_callback(
    data: *mut c_void,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> usize {
    if !data.is_null() {
        // Safety: `data` is the pointer produced by `Box::into_raw` in
        // `install_heap_limit_guard` and lives for the entire lifetime of the
        // isolate.
        let handle = unsafe { &*(data as *const v8::IsolateHandle) };
        // Terminate any JS currently running on this isolate. This unwinds the
        // guest with an uncatchable exception rather than crashing the process.
        handle.terminate_execution();
    }
    warn_limit_exhausted(
        TrackedLimit::V8HeapBytes,
        current_heap_limit,
        initial_heap_limit.max(1),
    );
    // Grant headroom so V8 does not immediately fatal-abort before the termination
    // takes effect. We never shrink below the current limit.
    current_heap_limit
        .max(initial_heap_limit)
        .saturating_add(NEAR_HEAP_LIMIT_HEADROOM_BYTES)
}

/// Register the near-heap-limit OOM guard on an isolate that was created with a
/// configured heap cap. Without this guard, V8 fatal-aborts the whole (process-
/// global) runtime with a SIGTRAP when the cap is reached, taking down every
/// concurrent tenant; with it, the offending isolate is terminated instead.
///
/// Must be called for every isolate created with a non-`None` heap limit,
/// regardless of whether it was built fresh or restored from a snapshot.
pub fn install_heap_limit_guard(isolate: &mut v8::OwnedIsolate) {
    // The callback needs a thread-safe handle to request termination of this very
    // isolate. The handle is leaked so it outlives the callback registration; the
    // number of isolates per process is bounded, so this is not an unbounded leak,
    // and the memory is reclaimed when the process exits.
    let handle = Box::new(isolate.thread_safe_handle());
    let data = Box::into_raw(handle) as *mut c_void;
    isolate.add_near_heap_limit_callback(near_heap_limit_callback, data);
}

/// Create a new V8 isolate with an optional heap limit in MB. `None` applies the
/// bounded-by-default cap (`DEFAULT_HEAP_LIMIT_MB`) — an isolate is NEVER created
/// with an unbounded heap, so a guest heap bomb terminates its own isolate rather
/// than fatal-aborting the shared process.
pub fn create_isolate(heap_limit_mb: Option<u32>) -> v8::OwnedIsolate {
    let limit = heap_limit_mb.unwrap_or(DEFAULT_HEAP_LIMIT_MB);
    let mut params = v8::CreateParams::default();
    let limit_bytes = (limit as usize) * 1024 * 1024;
    params = params.heap_limits(0, limit_bytes);
    let mut isolate = with_isolate_lifecycle_lock(|| v8::Isolate::new(params));
    configure_isolate(&mut isolate);
    install_heap_limit_guard(&mut isolate);
    isolate
}

/// Run V8 isolate create/drop work under a process-wide lifecycle lock.
///
/// rusty_v8 130.0.7 embeds a V8 13.0-era process-wide WebAssembly code pointer
/// table. Isolate construction allocates wasm builtin handles from that table and
/// isolate destruction frees them again, so create/drop must not overlap across
/// session threads.
pub fn with_isolate_lifecycle_lock<T>(f: impl FnOnce() -> T) -> T {
    let _guard = V8_ISOLATE_LIFECYCLE
        .lock()
        .expect("V8 isolate lifecycle lock poisoned");
    f()
}

pub fn drop_isolate(isolate: Option<v8::OwnedIsolate>) {
    if let Some(isolate) = isolate {
        with_isolate_lifecycle_lock(|| drop(isolate));
    }
}

/// Create a new V8 context on the given isolate.
/// Returns a Global handle so the context can be reused across scopes.
pub fn create_context(isolate: &mut v8::OwnedIsolate) -> v8::Global<v8::Context> {
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(scope, Default::default());
    v8::Global::new(scope, context)
}

// V8 lifecycle tests are consolidated in execution::tests to avoid
// inter-test SIGSEGV from V8 global state issues.
