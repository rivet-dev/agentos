// V8 isolate lifecycle: platform init, create, configure, destroy

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once};
use std::thread;

use crate::ipc::ExecutionError;
use agentos_bridge::queue_tracker::{warn_limit_exhausted, TrackedLimit};

static V8_INIT: Once = Once::new();
static V8_ISOLATE_LIFECYCLE: Mutex<()> = Mutex::new(());
const MAX_UNHANDLED_PROMISE_REJECTIONS: usize = 1024;

unsafe extern "C" {
    // rusty_v8 130 does not expose these public V8 embedder hooks in Rust. The
    // build-script C++ shim is compiled against the pinned V8 headers so this
    // boundary does not depend on platform-specific C++ mangled names.
    fn agentos_v8_initialize_sandbox_hardware_before_thread_creation();
    fn agentos_v8_set_default_thread_isolation_permissions();
}

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

/// V8's process-global background worker pool is constant topology, not an
/// implicit function of host CPU count. Four preserves useful parallelism for
/// background compilation while keeping the trusted thread census bounded.
const V8_PLATFORM_WORKER_THREADS: u32 = 4;

/// Initialize the V8 platform (once per process).
/// Safe to call multiple times; only the first call takes effect.
pub fn init_v8_platform() {
    V8_INIT.call_once(|| {
        // V8 requires sandbox hardware keys to be allocated before any thread
        // which may access its sandbox is created. This call precedes both the
        // platform owner below and V8's own default-platform workers.
        unsafe { agentos_v8_initialize_sandbox_hardware_before_thread_creation() };
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        // V8 binds process-global isolate-group state to its initialization
        // thread. A library caller can be a short-lived Rust test, request, or
        // maintenance thread, so initializing inline leaves later executor
        // threads using torn-down process-global WebAssembly tables. Keep one
        // constant owner alive for the process lifetime instead.
        // AGENTOS_THREAD_SITE: constant-v8-platform-owner
        thread::Builder::new()
            .name(String::from("agentos-v8-platform"))
            .spawn(move || {
                v8::icu::set_common_data_74(&ICU_COMMON_DATA.0)
                    .expect("failed to initialize V8 ICU common data");
                let platform =
                    v8::new_default_platform(V8_PLATFORM_WORKER_THREADS, false).make_shared();
                v8::V8::initialize_platform(platform);
                v8::V8::initialize();
                ready_tx
                    .send(())
                    .expect("V8 platform initializer lost its caller");

                // V8 is intentionally process-global and is never disposed
                // while the sidecar is alive. Parking preserves the thread-local
                // isolate-group owner without consuming CPU or a Tokio worker.
                loop {
                    thread::park();
                }
            })
            .expect("failed to spawn V8 platform owner");
        ready_rx
            .recv()
            .expect("V8 platform owner exited during initialization");
    });
}

/// Restore V8's read-only protection-key defaults on the current thread.
///
/// Executor and maintenance threads may descend from fixed host workers that
/// existed before V8 allocated its pkeys. Linux preserves each parent's PKRU
/// value across clone, so such descendants can otherwise fault merely reading
/// V8's process-wide code-pointer tables. Call this after platform init and
/// before the thread first enters V8.
pub fn prepare_current_thread() {
    init_v8_platform();
    unsafe { agentos_v8_set_default_thread_isolation_permissions() };
}

// Headroom granted to V8 above the current heap size when the near-heap-limit
// callback fires. V8 fatal-aborts the whole process (SIGTRAP) if the returned
// limit is still below the heap size, so we must hand back a larger limit to give
// the engine room to unwind. Termination has already been requested, so this
// extra budget only covers propagation of the uncatchable termination exception,
// not continued guest allocation.
const NEAR_HEAP_LIMIT_HEADROOM_BYTES: usize = 16 * 1024 * 1024;

/// State the near-heap-limit callback needs for one isolate.
struct HeapLimitGuard {
    handle: v8::IsolateHandle,
    isolate: *mut v8::Isolate,
    exceeded: Arc<AtomicBool>,
}

/// Isolate slot that records whether the near-heap-limit callback terminated the
/// running script, so the session can report the heap limit as the cause.
struct HeapLimitExceeded(Arc<AtomicBool>);

/// Returns true once if the near-heap-limit callback terminated the script that
/// last ran on this isolate, and clears the flag for the next execution.
pub fn take_heap_limit_exceeded(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<HeapLimitExceeded>()
        .is_some_and(|flag| flag.0.swap(false, Ordering::AcqRel))
}

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
/// termination exception cleanly. `data` is a leaked `Box<HeapLimitGuard>` for the
/// isolate this callback was registered on.
extern "C" fn near_heap_limit_callback(
    data: *mut c_void,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> usize {
    let mut heap_size = 0;
    if !data.is_null() {
        // Safety: `data` is the pointer produced by `Box::into_raw` in
        // `install_heap_limit_guard` and lives for the entire lifetime of the
        // isolate. V8 runs this callback on the isolate's own thread.
        let guard = unsafe { &*(data as *const HeapLimitGuard) };
        // Terminate any JS currently running on this isolate. This unwinds the
        // guest with an uncatchable exception rather than crashing the process.
        guard.handle.terminate_execution();
        guard.exceeded.store(true, Ordering::Release);
        // One large allocation (for example a 256 MiB string) can take the heap
        // far past the limit before this callback runs, so size the new limit
        // from the heap, not from the old limit.
        let mut statistics = v8::HeapStatistics::default();
        unsafe { (*guard.isolate).get_heap_statistics(&mut statistics) };
        heap_size = statistics.total_heap_size();
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
        .max(heap_size)
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
    // isolate and the isolate itself to read its heap size. The guard is leaked so
    // it outlives the callback registration; the number of isolates per process is
    // bounded, so this is not an unbounded leak, and the memory is reclaimed when
    // the process exits.
    let exceeded = Arc::new(AtomicBool::new(false));
    isolate.set_slot(HeapLimitExceeded(Arc::clone(&exceeded)));
    let guard = Box::new(HeapLimitGuard {
        handle: isolate.thread_safe_handle(),
        isolate: &mut **isolate,
        exceeded,
    });
    let data = Box::into_raw(guard) as *mut c_void;
    isolate.add_near_heap_limit_callback(near_heap_limit_callback, data);
}

/// Byte budget for one isolate's ArrayBuffer backing stores.
///
/// `ArrayBuffer`, typed array, `Buffer`, and `SharedArrayBuffer` memory lives
/// outside the V8 heap, so `heap_limits` does not bound it. Without a budget one
/// guest can grow the shared sidecar until the host OOM-kills every VM on it.
/// Past the budget the allocator returns null; V8 then collects garbage,
/// retries, and finally throws `RangeError: Array buffer allocation failed`, the
/// same error Node reports when an allocation fails.
///
/// V8 allocates WebAssembly memory and resizable buffers through its page
/// allocator instead, so this budget does not count them.
struct ArrayBufferBudget {
    used: AtomicUsize,
    limit: usize,
    warned: AtomicBool,
}

impl ArrayBufferBudget {
    fn reserve(&self, len: usize) -> bool {
        let reserved = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(len).filter(|next| *next <= self.limit)
            })
            .is_ok();
        // V8 retries a refused allocation after each GC, so warn once per
        // isolate instead of once per attempt.
        if !reserved && !self.warned.swap(true, Ordering::Relaxed) {
            warn_limit_exhausted(
                TrackedLimit::V8ArrayBufferBytes,
                self.used.load(Ordering::Relaxed).saturating_add(len),
                self.limit,
            );
        }
        reserved
    }

    fn release(&self, len: usize) {
        self.used.fetch_sub(len, Ordering::AcqRel);
    }
}

// V8 calls these from the isolate thread and from its GC threads. They must not
// call back into V8.
unsafe extern "C" fn budget_allocate(budget: &ArrayBufferBudget, len: usize) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    let data = unsafe { libc::calloc(len.max(1), 1) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn budget_allocate_uninitialized(
    budget: &ArrayBufferBudget,
    len: usize,
) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    let data = unsafe { libc::malloc(len.max(1)) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn budget_free(budget: &ArrayBufferBudget, data: *mut c_void, len: usize) {
    unsafe { libc::free(data) };
    budget.release(len);
}

unsafe extern "C" fn budget_reallocate(
    budget: &ArrayBufferBudget,
    data: *mut c_void,
    old_len: usize,
    new_len: usize,
) -> *mut c_void {
    if new_len > old_len && !budget.reserve(new_len - old_len) {
        return std::ptr::null_mut();
    }
    let resized = unsafe { libc::realloc(data, new_len.max(1)) };
    if resized.is_null() {
        if new_len > old_len {
            budget.release(new_len - old_len);
        }
    } else if new_len < old_len {
        budget.release(old_len - new_len);
    }
    resized
}

unsafe extern "C" fn budget_drop(budget: *const ArrayBufferBudget) {
    drop(unsafe { Arc::from_raw(budget) });
}

static ARRAY_BUFFER_BUDGET_VTABLE: v8::RustAllocatorVtable<ArrayBufferBudget> =
    v8::RustAllocatorVtable {
        allocate: budget_allocate,
        allocate_uninitialized: budget_allocate_uninitialized,
        free: budget_free,
        reallocate: budget_reallocate,
        drop: budget_drop,
    };

/// Create a guest isolate, fresh or snapshot-restored (`customize` adds the
/// snapshot parameters). The heap limit in MB (`None` applies
/// `DEFAULT_HEAP_LIMIT_MB`) caps the V8 heap and, separately, the isolate's
/// ArrayBuffer memory.
pub(crate) fn create_guest_isolate(
    heap_limit_mb: Option<u32>,
    customize: impl FnOnce(v8::CreateParams) -> v8::CreateParams,
) -> v8::OwnedIsolate {
    let limit = heap_limit_mb.unwrap_or(DEFAULT_HEAP_LIMIT_MB);
    let limit_bytes = (limit as usize) * 1024 * 1024;
    let budget = Arc::new(ArrayBufferBudget {
        used: AtomicUsize::new(0),
        limit: limit_bytes,
        warned: AtomicBool::new(false),
    });
    // Safety: the allocator owns this Arc reference and releases it through
    // `budget_drop`; the vtable expects exactly an `ArrayBufferBudget`.
    let allocator = unsafe {
        v8::new_rust_allocator(
            Arc::into_raw(Arc::clone(&budget)),
            &ARRAY_BUFFER_BUDGET_VTABLE,
        )
    };
    let params = customize(
        v8::CreateParams::default()
            .heap_limits(0, limit_bytes)
            .array_buffer_allocator(allocator.make_shared()),
    );
    let mut isolate = with_isolate_lifecycle_lock(|| v8::Isolate::new(params));
    configure_isolate(&mut isolate);
    install_heap_limit_guard(&mut isolate);
    isolate.set_slot(budget);
    isolate
}

/// Create a new V8 isolate with an optional heap limit in MB. `None` applies the
/// bounded-by-default cap (`DEFAULT_HEAP_LIMIT_MB`) — an isolate is NEVER created
/// with an unbounded heap, so a guest heap bomb terminates its own isolate rather
/// than fatal-aborting the shared process.
pub fn create_isolate(heap_limit_mb: Option<u32>) -> v8::OwnedIsolate {
    prepare_current_thread();
    create_guest_isolate(heap_limit_mb, |params| params)
}

/// State the external-memory GC check needs for one isolate.
struct ExternalMemoryGuard {
    budget: Arc<ArrayBufferBudget>,
    exceeded: Arc<AtomicBool>,
    warned: AtomicBool,
}

/// Isolate slot that marks the external-memory guard as installed.
struct ExternalMemoryGuardInstalled;

/// Stop guest JavaScript whose memory outside the ArrayBuffer budget passes the
/// heap limit.
///
/// V8 allocates WebAssembly memory through its page allocator, so the
/// ArrayBuffer budget never sees it, and host-provided buffers are created
/// outside that budget on purpose. V8 counts both as external memory and
/// collects garbage as it grows, so a GC prologue check bounds them. Past the
/// limit the script is terminated and reported as a heap-limit failure.
///
/// Install this only for guest JavaScript sessions. Python (Pyodide) and the
/// WASM runner keep their own program memory in WebAssembly and bound it with
/// their own limits. Calling it again on the same isolate does nothing.
pub fn install_external_memory_guard(isolate: &mut v8::OwnedIsolate) {
    if isolate.get_slot::<ExternalMemoryGuardInstalled>().is_some() {
        return;
    }
    let budget = Arc::clone(
        isolate
            .get_slot::<Arc<ArrayBufferBudget>>()
            .expect("guest isolates carry an ArrayBuffer budget"),
    );
    let exceeded = Arc::clone(
        &isolate
            .get_slot::<HeapLimitExceeded>()
            .expect("guest isolates carry a heap-limit guard")
            .0,
    );
    // Leaked like `HeapLimitGuard`: V8 can call the callback until the isolate
    // is disposed, after its slots are gone.
    let guard = Box::into_raw(Box::new(ExternalMemoryGuard {
        budget,
        exceeded,
        warned: AtomicBool::new(false),
    }));
    isolate.add_gc_prologue_callback(
        external_memory_gc_callback,
        guard as *mut c_void,
        v8::GCType::ALL,
    );
    isolate.set_slot(ExternalMemoryGuardInstalled);
}

extern "C" fn external_memory_gc_callback(
    isolate: *mut v8::Isolate,
    _gc_type: v8::GCType,
    _flags: v8::GCCallbackFlags,
    data: *mut c_void,
) {
    // Safety: `data` is the guard leaked by `install_external_memory_guard`, and
    // V8 runs GC prologue callbacks on the isolate's own thread.
    let guard = unsafe { &*(data as *const ExternalMemoryGuard) };
    let isolate = unsafe { &mut *isolate };
    let mut statistics = v8::HeapStatistics::default();
    isolate.get_heap_statistics(&mut statistics);
    // External memory also holds the non-shared ArrayBuffers the budget already
    // counts, so only the remainder is checked here.
    let outside_budget = statistics
        .external_memory()
        .saturating_sub(guard.budget.used.load(Ordering::Acquire));
    let limit = guard.budget.limit;
    if outside_budget <= limit {
        return;
    }
    isolate.terminate_execution();
    guard.exceeded.store(true, Ordering::Release);
    if !guard.warned.swap(true, Ordering::Relaxed) {
        warn_limit_exhausted(TrackedLimit::V8ExternalMemoryBytes, outside_budget, limit);
    }
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
