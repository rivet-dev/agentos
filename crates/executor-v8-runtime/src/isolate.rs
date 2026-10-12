// V8 isolate lifecycle: platform init, create, configure, destroy

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once};
use std::thread;

use crate::ipc::ExecutionError;
use agentos_resource_accounting::queue_tracker::{
    register_queue, warn_limit_exhausted, QueueGauge, TrackedLimit,
};

static V8_INIT: Once = Once::new();
static V8_ISOLATE_LIFECYCLE: Mutex<()> = Mutex::new(());
pub(crate) const MAX_UNHANDLED_PROMISE_REJECTIONS: usize = 1024;

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

/// Pending events own original values only until the current checkpoint dispatches
/// them. Reported promises carry a V8-private marker instead of a strong registry.
pub(crate) enum PendingPromiseRejection {
    /// **When**
    ///
    /// V8 reports a rejected Promise without a handler and queue admission succeeds.
    ///
    /// **State**
    ///
    /// Owns the original Promise, reason and rejection frame until cancellation or reporting.
    ///
    /// **Effects**
    ///
    /// The checkpoint dispatcher emits unhandledRejection outside the queue borrow.
    Unhandled {
        promise: v8::Global<v8::Promise>,
        reason: v8::Global<v8::Value>,
        frame: v8::Global<v8::Value>,
    },
    /// **When**
    ///
    /// V8 reports a handler attached after notification in the current execution generation.
    ///
    /// **State**
    ///
    /// Owns only the original Promise; its private reported marker is already cleared.
    ///
    /// **Effects**
    ///
    /// The checkpoint dispatcher emits rejectionHandled in the cleared host event frame.
    Handled { promise: v8::Global<v8::Promise> },
}

impl PendingPromiseRejection {
    fn promise(&self) -> &v8::Global<v8::Promise> {
        match self {
            Self::Unhandled { promise, .. } | Self::Handled { promise } => promise,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectionBatchKind {
    /// Late-handler events drain, including reentrant additions, before new reports.
    Handled,
    /// The current unhandled snapshot is sealed against listener-side cancellation.
    Unhandled,
}

#[derive(Default, Clone, Copy)]
enum RejectionDrainPhase {
    /// All queued entries may still be cancelled before their checkpoint report.
    #[default]
    Idle,
    /// The leading `remaining` entries are the current sealed reporting batch.
    Reporting { remaining: usize },
    /// A drain is active between snapshots; nested checkpoints leave it to its owner.
    BetweenBatches,
}

pub struct PromiseRejectState {
    pending: VecDeque<PendingPromiseRejection>,
    phase: RejectionDrainPhase,
    overflow_count: usize,
    bookkeeping_error: Option<ExecutionError>,
    generation: u64,
    gauge: Arc<QueueGauge>,
    dispatch_work: usize,
}

impl Default for PromiseRejectState {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            phase: RejectionDrainPhase::Idle,
            overflow_count: 0,
            bookkeeping_error: None,
            generation: 1,
            dispatch_work: 0,
            gauge: register_queue(
                TrackedLimit::V8PromiseRejections,
                MAX_UNHANDLED_PROMISE_REJECTIONS,
            ),
        }
    }
}

impl PromiseRejectState {
    fn enqueue(&mut self, event: PendingPromiseRejection) -> bool {
        if self.pending.len() >= MAX_UNHANDLED_PROMISE_REJECTIONS {
            self.overflow_count = self.overflow_count.saturating_add(1);
            false
        } else {
            // Node reports already-handled rejections before new unhandled
            // ones at the next checkpoint. Never reorder the sealed prefix.
            let index = if matches!(event, PendingPromiseRejection::Handled { .. }) {
                let sealed = self.sealed_len();
                sealed
                    + self
                        .pending
                        .iter()
                        .skip(sealed)
                        .take_while(|queued| {
                            matches!(queued, PendingPromiseRejection::Handled { .. })
                        })
                        .count()
            } else {
                self.pending.len()
            };
            self.pending.insert(index, event);
            self.gauge
                .observe_depth(self.pending.len().max(self.dispatch_work));
            true
        }
    }

    fn sealed_len(&self) -> usize {
        match self.phase {
            RejectionDrainPhase::Idle | RejectionDrainPhase::BetweenBatches => 0,
            RejectionDrainPhase::Reporting { remaining } => remaining,
        }
    }

    fn cancel_pending(&mut self, promise: v8::Local<v8::Promise>) -> bool {
        let sealed = self.sealed_len();
        let index = self
            .pending
            .iter()
            .enumerate()
            .skip(sealed)
            .find_map(|(index, event)| {
                matches!(event, PendingPromiseRejection::Unhandled { .. })
                    .then(|| event.promise().eq(&promise))
                    .filter(|same| *same)
                    .map(|_| index)
            });
        if let Some(index) = index {
            self.pending.remove(index);
            self.gauge
                .observe_depth(self.pending.len().max(self.dispatch_work));
            true
        } else {
            false
        }
    }

    pub(crate) fn begin_drain(&mut self) -> bool {
        if !matches!(self.phase, RejectionDrainPhase::Idle) {
            return false;
        }
        self.phase = RejectionDrainPhase::BetweenBatches;
        self.dispatch_work = 0;
        true
    }

    pub(crate) fn finish_drain(&mut self) {
        self.phase = RejectionDrainPhase::Idle;
        self.dispatch_work = 0;
        self.gauge.observe_depth(self.pending.len());
    }

    pub(crate) fn begin_batch(&mut self) -> Option<(RejectionBatchKind, usize)> {
        if self.pending.is_empty() {
            self.phase = RejectionDrainPhase::BetweenBatches;
            return None;
        }
        let (kind, remaining) = if matches!(
            self.pending.front(),
            Some(PendingPromiseRejection::Handled { .. })
        ) {
            (
                RejectionBatchKind::Handled,
                self.pending
                    .iter()
                    .take_while(|event| matches!(event, PendingPromiseRejection::Handled { .. }))
                    .count(),
            )
        } else {
            (RejectionBatchKind::Unhandled, self.pending.len())
        };
        self.phase = RejectionDrainPhase::Reporting { remaining };
        Some((kind, remaining))
    }

    pub(crate) fn pop_reporting(&mut self) -> Option<PendingPromiseRejection> {
        let remaining = match self.phase {
            RejectionDrainPhase::Idle | RejectionDrainPhase::BetweenBatches => return None,
            RejectionDrainPhase::Reporting { remaining } => remaining,
        };
        self.phase = if remaining == 1 {
            RejectionDrainPhase::BetweenBatches
        } else {
            RejectionDrainPhase::Reporting {
                remaining: remaining - 1,
            }
        };
        let event = self.pending.pop_front();
        self.gauge
            .observe_depth(self.pending.len().max(self.dispatch_work));
        event
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn take_failure(&mut self) -> Option<ExecutionError> {
        let error = self
            .bookkeeping_error
            .take()
            .or_else(|| (self.overflow_count > 0).then(rejection_limit_error));
        if error.is_some() {
            self.clear_pending();
        }
        error
    }

    /// A guest rejection listener threw and uncaughtException consumed it.
    /// Dispose only the rest of that selected unhandled snapshot; newly queued
    /// events remain behind its prefix and keep their own delivery/handles.
    pub(crate) fn discard_unhandled_snapshot(&mut self) -> Result<(), ExecutionError> {
        while self.sealed_len() != 0 {
            if !matches!(
                self.pending.front(),
                Some(PendingPromiseRejection::Unhandled { .. })
            ) {
                return Err(ExecutionError {
                    error_type: "Error".into(),
                    message:
                        "promise rejection owner attempted to discard a non-unhandled snapshot"
                            .into(),
                    stack: String::new(),
                    code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
                });
            }
            if self.pop_reporting().is_none() {
                return Err(ExecutionError {
                    error_type: "Error".into(),
                    message: "promise rejection owner lost its selected snapshot entry".into(),
                    stack: String::new(),
                    code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn yield_reporting(&mut self) {
        self.phase = RejectionDrainPhase::BetweenBatches;
    }

    pub(crate) fn admit_dispatch(&mut self) -> Result<(), ExecutionError> {
        if self.dispatch_work == MAX_UNHANDLED_PROMISE_REJECTIONS {
            return Err(rejection_limit_error());
        }
        self.dispatch_work += 1;
        self.gauge
            .observe_depth(self.pending.len().max(self.dispatch_work));
        Ok(())
    }

    /// Called once after prior-call cleanup, before entering a new execution.
    pub(crate) fn reset_execution(&mut self) {
        self.clear_pending();
        match self.generation.checked_add(1) {
            Some(generation) => self.generation = generation,
            None => {
                self.bookkeeping_error = Some(ExecutionError {
                    error_type: "Error".into(),
                    message:
                        "V8 promise rejection execution generation exhausted; recreate the context"
                            .into(),
                    stack: String::new(),
                    code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
                })
            }
        }
    }

    pub(crate) fn clear_pending(&mut self) {
        self.pending.clear();
        self.gauge.observe_depth(0);
        self.dispatch_work = 0;
        self.phase = RejectionDrainPhase::Idle;
        self.overflow_count = 0;
        self.bookkeeping_error = None;
    }
}

pub(crate) fn rejection_limit_error() -> ExecutionError {
    warn_limit_exhausted(
        TrackedLimit::V8PromiseRejections,
        MAX_UNHANDLED_PROMISE_REJECTIONS + 1,
        MAX_UNHANDLED_PROMISE_REJECTIONS,
    );
    ExecutionError { error_type: "Error".into(), message: format!(
        "ERR_AGENTOS_UNHANDLED_REJECTION_LIMIT: unhandled promise rejection registry exceeded limit of {MAX_UNHANDLED_PROMISE_REJECTIONS} rejections; reduce pending/recursive rejection events; no runtime adjustment is available (rebuild to change the fixed capacity)"),
        stack: String::new(), code: Some("ERR_AGENTOS_UNHANDLED_REJECTION_LIMIT".into()) }
}

// The fixed private key cannot be read/forged from guest JavaScript. Only a
// reported/overflow marker survives the pending owner, never a strong handle.
fn rejection_marker_key<'s>(scope: &mut v8::HandleScope<'s>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "agentos#promise-rejection-state")?;
    Some(v8::Private::for_api(scope, Some(name)))
}

pub(crate) fn set_rejection_marker(
    scope: &mut v8::HandleScope,
    promise: v8::Local<v8::Promise>,
    marker: i32,
) -> bool {
    let Some(generation) = scope
        .get_slot::<PromiseRejectState>()
        .map(|state| state.generation)
    else {
        return false;
    };
    let Some(key) = rejection_marker_key(scope) else {
        if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
            state.bookkeeping_error = Some(ExecutionError {
                error_type: "Error".into(),
                message: "could not allocate V8 promise rejection owner key".into(),
                stack: String::new(),
                code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
            });
        }
        return false;
    };
    // Opaque native BigInt words cannot invoke guest Array/Object prototype
    // setters while V8 is inside its rejection hook. Both words are fixed-size.
    let Some(value) = v8::BigInt::new_from_words(scope, false, &[marker as u64, generation]) else {
        if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
            state.bookkeeping_error = Some(ExecutionError {
                error_type: "Error".into(),
                message: "could not allocate V8 promise rejection owner marker".into(),
                stack: String::new(),
                code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
            });
        }
        return false;
    };
    let written = promise.set_private(scope, key, value.into()) == Some(true);
    if !written {
        if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
            state.bookkeeping_error = Some(ExecutionError {
                error_type: "Error".into(),
                message: "failed to update V8 promise rejection owner state".into(),
                stack: String::new(),
                code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
            });
        }
    }
    written
}

fn rejection_marker(
    scope: &mut v8::HandleScope,
    promise: v8::Local<v8::Promise>,
) -> Option<(i32, u64)> {
    let key = rejection_marker_key(scope)?;
    let value = v8::Local::<v8::BigInt>::try_from(promise.get_private(scope, key)?).ok()?;
    if value.word_count() != 2 {
        return None;
    }
    let mut storage = [0; 2];
    let (negative, words) = value.to_words_array(&mut storage);
    if negative {
        return None;
    }
    Some((i32::try_from(words[0]).ok()?, words[1]))
}

fn current_rejection_marker(
    scope: &mut v8::HandleScope,
    promise: v8::Local<v8::Promise>,
) -> Option<i32> {
    let (marker, generation) = rejection_marker(scope, promise)?;
    (generation == scope.get_slot::<PromiseRejectState>()?.generation).then_some(marker)
}

// The module loader already retains at most one evaluation Promise. This
// non-retaining lifetime marker transfers reporting to that existing owner.
// After cancellation/reset, late settlement still belongs to the ended module
// operation, never to ordinary Promise reporting in the next Execute.
const MODULE_EVALUATION_REJECTION_OWNER: i32 = 3;

pub(crate) fn claim_module_evaluation_rejection(
    scope: &mut v8::HandleScope,
    promise: v8::Local<v8::Promise>,
) -> Result<(), ExecutionError> {
    let failed = || ExecutionError {
        error_type: "Error".into(),
        message: "could not transfer module evaluation rejection ownership".into(),
        stack: String::new(),
        code: Some("ERR_AGENTOS_PROMISE_REJECTION_STATE".into()),
    };
    let marker = current_rejection_marker(scope, promise);
    let state = scope
        .get_slot_mut::<PromiseRejectState>()
        .ok_or_else(failed)?;
    // Never take an event out of a report snapshot currently being delivered.
    if state
        .pending
        .iter()
        .take(state.sealed_len())
        .any(|event| event.promise().eq(&promise))
    {
        return Err(failed());
    }
    state.cancel_pending(promise);
    if marker == Some(2) {
        state.overflow_count = state.overflow_count.checked_sub(1).ok_or_else(failed)?;
    }
    if !set_rejection_marker(scope, promise, MODULE_EVALUATION_REJECTION_OWNER) {
        return Err(scope
            .get_slot_mut::<PromiseRejectState>()
            .and_then(PromiseRejectState::take_failure)
            .unwrap_or_else(failed));
    }
    Ok(())
}

extern "C" fn promise_reject_callback(msg: v8::PromiseRejectMessage) {
    let scope = &mut unsafe { v8::CallbackScope::new(&msg) };
    let promise = msg.get_promise();
    match msg.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            if rejection_marker(scope, promise)
                .is_some_and(|(marker, _)| marker == MODULE_EVALUATION_REJECTION_OWNER)
            {
                return;
            }
            let reason = msg
                .get_value()
                .unwrap_or_else(|| v8::undefined(scope).into());
            let frame = scope.get_continuation_preserved_embedder_data();
            let event = PendingPromiseRejection::Unhandled {
                promise: v8::Global::new(scope, promise),
                reason: v8::Global::new(scope, reason),
                frame: v8::Global::new(scope, frame),
            };
            let admitted = scope
                .get_slot_mut::<PromiseRejectState>()
                .is_some_and(|state| state.enqueue(event));
            if !admitted {
                set_rejection_marker(scope, promise, 2);
            }
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            if scope
                .get_slot_mut::<PromiseRejectState>()
                .is_some_and(|state| state.cancel_pending(promise))
            {
                return;
            }
            let marker = current_rejection_marker(scope, promise).unwrap_or(0);
            if marker == 2 {
                if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
                    state.overflow_count = state.overflow_count.saturating_sub(1);
                }
                set_rejection_marker(scope, promise, 0);
            } else if marker == 1 && set_rejection_marker(scope, promise, 0) {
                let event = PendingPromiseRejection::Handled {
                    promise: v8::Global::new(scope, promise),
                };
                if let Some(state) = scope.get_slot_mut::<PromiseRejectState>() {
                    state.enqueue(event);
                }
            }
        }
        _ => {}
    }
}

pub fn configure_isolate(isolate: &mut v8::OwnedIsolate) {
    isolate.set_slot(PromiseRejectState::default());
    isolate.set_slot(crate::terminal_callback::TerminalCallbackOwner::default());
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
                // LLVM 19 emits the legacy Phase-3 WebAssembly exception
                // encoding and our toolchain translates it to the finalized
                // exnref form required by Wasmtime. V8 130 keeps that finalized
                // form behind this process-global flag. The shared AgentOS
                // validator still controls which guest proposals are accepted.
                v8::V8::set_flags_from_string("--experimental-wasm-exnref");
                // Native lifetime tests use V8's deterministic full-GC test API.
                #[cfg(test)]
                v8::V8::set_flags_from_string("--expose_gc");
                let platform =
                    v8::new_default_platform(V8_PLATFORM_WORKER_THREADS, false).make_shared();
                // Deno's embedding layer also owns cppgc initialization and
                // records whether V8 has started. Initialize it on this same
                // process-lifetime owner before either kind of isolate runs.
                #[cfg(feature = "obscura-embedding")]
                deno_core::JsRuntime::init_platform(Some(platform), false);
                #[cfg(not(feature = "obscura-embedding"))]
                {
                    v8::V8::initialize_platform(platform);
                    v8::V8::initialize();
                }
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

struct HeapLimitGuardRegistration {
    guard: Box<HeapLimitGuard>,
}

impl Drop for HeapLimitGuardRegistration {
    fn drop(&mut self) {
        // rusty_v8 clears slots before native Isolate::Dispose. The original
        // isolate is still alive, but its annex/IsolateHandle is already
        // invalidated. Remove the callback directly before freeing its data.
        // A zero limit avoids changing the heap limit during teardown.
        unsafe { &mut *self.guard.isolate }
            .remove_near_heap_limit_callback(near_heap_limit_callback, 0);
    }
}

/// Isolate slot that records whether the near-heap-limit callback terminated the
/// running script, so the session can report the heap limit as the cause.
struct HeapLimitExceeded(Arc<AtomicBool>);

pub(crate) fn heap_limit_exceeded(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<HeapLimitExceeded>()
        .is_some_and(|flag| flag.0.load(Ordering::Acquire))
}

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
/// termination exception cleanly. The isolate's registration owns `data` until
/// this callback is unregistered.
extern "C" fn near_heap_limit_callback(
    data: *mut c_void,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> usize {
    let mut heap_size = 0;
    if !data.is_null() {
        // Safety: `data` points into the stable Box owned by the registration.
        // V8 runs this callback on the isolate's own thread; the registration
        // unregisters it before freeing the Box.
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
    if isolate.get_slot::<HeapLimitGuardRegistration>().is_some() {
        return;
    }
    let exceeded = Arc::new(AtomicBool::new(false));
    isolate.set_slot(HeapLimitExceeded(Arc::clone(&exceeded)));
    let mut guard = Box::new(HeapLimitGuard {
        handle: isolate.thread_safe_handle(),
        isolate: &mut **isolate,
        exceeded,
    });
    let data = guard.as_mut() as *mut HeapLimitGuard as *mut c_void;
    isolate.add_near_heap_limit_callback(near_heap_limit_callback, data);
    isolate.set_slot(HeapLimitGuardRegistration { guard });
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

struct ExternalMemoryGuardRegistration {
    isolate: *mut v8::Isolate,
    guard: Box<ExternalMemoryGuard>,
}

impl Drop for ExternalMemoryGuardRegistration {
    fn drop(&mut self) {
        let data = self.guard.as_mut() as *mut ExternalMemoryGuard as *mut c_void;
        // As with the heap guard, slot teardown precedes native V8 disposal.
        // Keep the callback's Box alive until removal of this exact data pointer.
        unsafe { &mut *self.isolate }
            .remove_gc_prologue_callback(external_memory_gc_callback, data);
    }
}

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
    if isolate
        .get_slot::<ExternalMemoryGuardRegistration>()
        .is_some()
    {
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
    let mut guard = Box::new(ExternalMemoryGuard {
        budget,
        exceeded,
        warned: AtomicBool::new(false),
    });
    let data = guard.as_mut() as *mut ExternalMemoryGuard as *mut c_void;
    isolate.add_gc_prologue_callback(external_memory_gc_callback, data, v8::GCType::ALL);
    let registration = ExternalMemoryGuardRegistration {
        isolate: &mut **isolate,
        guard,
    };
    isolate.set_slot(registration);
}

extern "C" fn external_memory_gc_callback(
    isolate: *mut v8::Isolate,
    _gc_type: v8::GCType,
    _flags: v8::GCCallbackFlags,
    data: *mut c_void,
) {
    // Safety: the registration owns this stable Box and unregisters the callback
    // before freeing it. V8 runs GC prologue callbacks on the isolate's thread.
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

#[cfg(test)]
mod memory_budget_tests {
    use super::*;

    #[test]
    fn isolate_disposal_releases_heap_callback_ownership() {
        for disposal in 0..3 {
            let exceeded = {
                let isolate = create_isolate(Some(8));
                let exceeded = Arc::downgrade(
                    &isolate
                        .get_slot::<HeapLimitExceeded>()
                        .expect("heap guard installed")
                        .0,
                );
                match disposal {
                    0 => drop_isolate(Some(isolate)),
                    1 => with_isolate_lifecycle_lock(|| drop(isolate)),
                    _ => with_isolate_lifecycle_lock(move || {
                        // Implicit drop inside the lifecycle lock, without the helper.
                        let _owned_isolate = isolate;
                    }),
                }
                exceeded
            };
            assert!(
                exceeded.upgrade().is_none(),
                "disposed isolate must release its heap callback state (mode {disposal})"
            );
        }
    }

    #[test]
    fn isolate_disposal_releases_external_callback_ownership() {
        for disposal in 0..3 {
            let budget = {
                let mut isolate = create_isolate(Some(8));
                install_external_memory_guard(&mut isolate);
                // Repeated installation must not create an additional callback owner.
                install_external_memory_guard(&mut isolate);
                let budget = Arc::downgrade(
                    isolate
                        .get_slot::<Arc<ArrayBufferBudget>>()
                        .expect("array buffer budget installed"),
                );
                match disposal {
                    0 => drop_isolate(Some(isolate)),
                    1 => with_isolate_lifecycle_lock(|| drop(isolate)),
                    _ => with_isolate_lifecycle_lock(move || {
                        let _owned_isolate = isolate;
                    }),
                }
                budget
            };
            assert!(
                budget.upgrade().is_none(),
                "disposed isolate must release its external callback budget (mode {disposal})"
            );
        }
    }

    #[test]
    fn repeated_heap_guard_installation_preserves_enforcement() {
        let mut isolate = create_isolate(Some(8));
        let exceeded = Arc::downgrade(&isolate.get_slot::<HeapLimitExceeded>().unwrap().0);
        install_heap_limit_guard(&mut isolate);
        assert!(std::ptr::eq(
            exceeded.as_ptr(),
            Arc::as_ptr(&isolate.get_slot::<HeapLimitExceeded>().unwrap().0),
        ));
        let data = isolate
            .get_slot::<HeapLimitGuardRegistration>()
            .unwrap()
            .guard
            .as_ref() as *const HeapLimitGuard as *mut c_void;
        // Exercise the registered callback without allocating to the heap limit.
        let limit = 8 * 1024 * 1024;
        let (headroom, rejected, terminated) = {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let scope = &mut v8::TryCatch::new(scope);
            let code = v8::String::new(scope, "for (let i = 0; i < 4; i++) {} 42").unwrap();
            let script = v8::Script::compile(scope, code, None).unwrap();
            let headroom = near_heap_limit_callback(data, limit, limit);
            // TerminateExecution requests an interrupt. Running JS services it
            // and exposes the termination exception through TryCatch.
            let rejected = script.run(scope).is_none();
            (headroom, rejected, scope.has_terminated())
        };
        let reported = take_heap_limit_exceeded(&isolate);
        isolate.cancel_terminate_execution();
        with_isolate_lifecycle_lock(|| drop(isolate));
        assert!(
            reported,
            "heap callback did not report its limit: terminated={terminated}, headroom={headroom}"
        );
        assert!(
            rejected && terminated,
            "heap callback did not terminate JS: rejected={rejected}, terminated={terminated}, reported={reported}, headroom={headroom}"
        );
        assert!(headroom >= limit + NEAR_HEAP_LIMIT_HEADROOM_BYTES);
        assert!(exceeded.upgrade().is_none());
    }

    #[test]
    fn external_guard_enforces_backing_memory_after_repeated_installation() {
        let mut isolate = create_isolate(Some(8));
        install_external_memory_guard(&mut isolate);
        install_external_memory_guard(&mut isolate);
        let (rejected, terminated, external_bytes) = {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let scope = &mut v8::TryCatch::new(scope);
            let code = v8::String::new(scope, "for (let i = 0; i < 4; i++) {} 42").unwrap();
            let script = v8::Script::compile(scope, code, None).unwrap();
            // Host-provided backing stores are outside the custom allocator's
            // budget. Keep this finite 9MiB buffer rooted while the 8MiB guard
            // runs; HeapStatistics reports backing bytes, not manual counters.
            let backing = v8::ArrayBuffer::new_backing_store_from_bytes(vec![0u8; 9 * 1024 * 1024]);
            let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing.make_shared());
            scope.low_memory_notification();
            let mut statistics = v8::HeapStatistics::default();
            scope.get_heap_statistics(&mut statistics);
            let rejected = script.run(scope).is_none();
            let terminated = scope.has_terminated();
            // Referencing the handle after GC makes its intended lifetime clear.
            std::hint::black_box(buffer);
            (rejected, terminated, statistics.external_memory())
        };
        let reported = take_heap_limit_exceeded(&isolate);
        let warned = isolate
            .get_slot::<ExternalMemoryGuardRegistration>()
            .unwrap()
            .guard
            .warned
            .load(Ordering::Relaxed);
        isolate.cancel_terminate_execution();
        with_isolate_lifecycle_lock(|| drop(isolate));
        assert!(reported && warned, "GC callback did not report its limit: rejected={rejected}, terminated={terminated}, reported={reported}, warned={warned}, external_bytes={external_bytes}");
        assert!(rejected && terminated, "GC callback did not terminate JS: rejected={rejected}, terminated={terminated}, external_bytes={external_bytes}");
    }

    #[test]
    fn callback_retirement_preserves_exported_backing_store_allocator() {
        let mut isolate = create_isolate(Some(8));
        install_external_memory_guard(&mut isolate);
        let budget = Arc::downgrade(isolate.get_slot::<Arc<ArrayBufferBudget>>().unwrap());
        let backing = v8::ArrayBuffer::new_backing_store(&mut isolate, 32).make_shared();
        with_isolate_lifecycle_lock(|| drop(isolate));
        assert!(
            budget.upgrade().is_some(),
            "live backing store owns its allocator"
        );
        assert_eq!(backing.byte_length(), 32);
        drop(backing);
        assert!(
            budget.upgrade().is_none(),
            "last backing store releases allocator"
        );
    }

    #[test]
    fn guest_array_buffer_over_budget_throws_without_killing_isolate() {
        let mut isolate = create_isolate(Some(8));
        {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let source = v8::String::new(
                scope,
                r#"
                (() => {
                    try { new ArrayBuffer(9 * 1024 * 1024); }
                    catch (error) {
                        return error instanceof RangeError && new Uint8Array(32).length === 32;
                    }
                    return false;
                })()
            "#,
            )
            .unwrap();
            let script = v8::Script::compile(scope, source, None).unwrap();
            let result = script
                .run(scope)
                .expect("over-budget allocation must not terminate the isolate");
            assert!(
                result.is_true(),
                "guest allocation must fail as RangeError and permit later work"
            );
        }
        drop_isolate(Some(isolate));
    }

    #[test]
    fn allocator_rejected_growth_preserves_existing_allocation_and_budget() {
        let budget = ArrayBufferBudget {
            used: AtomicUsize::new(0),
            limit: 32,
            warned: AtomicBool::new(false),
        };
        unsafe {
            let allocation = budget_allocate(&budget, 16);
            assert!(!allocation.is_null());
            assert!(budget_reallocate(&budget, allocation, 16, 33).is_null());
            assert_eq!(budget.used.load(Ordering::Acquire), 16);
            let allocation = budget_reallocate(&budget, allocation, 16, 8);
            assert!(!allocation.is_null());
            assert_eq!(budget.used.load(Ordering::Acquire), 8);
            budget_free(&budget, allocation, 8);
            assert_eq!(budget.used.load(Ordering::Acquire), 0);
        }
    }
}
