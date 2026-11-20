//! Native process exit and synchronous callback failure are terminal outcomes,
//! never ordinary catchable Promise rejections. The first outcome reserves its
//! original value before error classification can invoke guest-controlled getters.
use crate::execution::{
    classify_terminal_exception, ExplicitMicrotaskScope, MicrotaskSuppressionStorage,
    TerminalExceptionKind,
};
use crate::ipc::ExecutionError;
use std::cell::RefCell;
use std::rc::Rc;

const GLOBAL: &str = "__agentOsReportTerminalCallback";
const EXIT_GLOBAL: &str = "__agentOsRequestProcessExit";
const DELIVERY_PRIVATE: &str = "agentos.native.terminal.entry";

// A private symbol is isolate-owned, but never owns a realm or function. The
// function itself is a context-private property serialized with that context.
struct DeliveryKey(Rc<v8::Global<v8::Private>>);

fn private_delivery_key<'s>(
    scope: &mut v8::HandleScope<'s, ()>,
) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, DELIVERY_PRIVATE)?;
    Some(v8::Private::for_api(scope, Some(name)))
}

#[derive(Clone, Default)]
pub(crate) struct TerminalCallbackOwner(Rc<RefCell<TerminalCallbackState>>);

#[derive(Default)]
struct TerminalCallbackState {
    generation: u64,
    settled: bool,
    stop_pending: bool,
    cleanup_failure_reported: bool,
    phase: TerminalCallbackPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalErrorDisposition {
    Diagnostic,
    SilentProcessExit,
}

#[derive(Default)]
enum TerminalCallbackPhase {
    #[default]
    Empty,
    Recording {
        original: v8::Global<v8::Value>,
        interrupted: bool,
    },
    Recorded {
        original: v8::Global<v8::Value>,
        result: (i32, ExecutionError),
        kind: TerminalExceptionKind,
        error_disposition: TerminalErrorDisposition,
        after_settle: bool,
    },
}

impl TerminalCallbackState {
    fn reset(&mut self, generation: u64) -> TerminalCallbackPhase {
        let previous = std::mem::take(&mut self.phase);
        self.generation = generation;
        self.settled = false;
        self.stop_pending = false;
        self.cleanup_failure_reported = false;
        previous
    }

    pub(crate) fn pending(&self) -> bool {
        self.stop_pending
    }

    fn occupied(&self) -> bool {
        !matches!(self.phase, TerminalCallbackPhase::Empty)
    }

    pub(crate) fn result(&self) -> Option<(i32, ExecutionError)> {
        if !self.stop_pending {
            return None;
        }
        match &self.phase {
            TerminalCallbackPhase::Recorded { result, .. } => Some(result.clone()),
            TerminalCallbackPhase::Empty | TerminalCallbackPhase::Recording { .. } => None,
        }
    }
}

pub(crate) fn classification_failure() -> (i32, ExecutionError) {
    (
        1,
        ExecutionError {
            error_type: "Error".into(),
            message: "terminal callback error classification was interrupted".into(),
            stack: String::new(),
            code: Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE".into()),
        },
    )
}

#[derive(Clone, Copy)]
enum TerminalInput {
    Callback,
    ProcessExit { code: i32, diagnostic: bool },
}

impl TerminalInput {
    fn error_disposition(self, kind: TerminalExceptionKind) -> TerminalErrorDisposition {
        match (self, kind) {
            (Self::Callback, TerminalExceptionKind::ProcessExit)
            | (
                Self::ProcessExit {
                    diagnostic: false, ..
                },
                _,
            ) => TerminalErrorDisposition::SilentProcessExit,
            _ => TerminalErrorDisposition::Diagnostic,
        }
    }

    fn classification_failure(self) -> (TerminalExceptionKind, (i32, ExecutionError)) {
        match self {
            Self::Callback => (TerminalExceptionKind::Other, classification_failure()),
            // The explicit primitive exit request is already committed. A
            // diagnostic getter cannot turn it back into a resumable call.
            Self::ProcessExit { code, .. } => (
                TerminalExceptionKind::ProcessExit,
                (code, classification_failure().1),
            ),
        }
    }
}

pub(crate) fn request_process_exit(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _return_value: v8::ReturnValue,
) {
    let value = args.get(0);
    // Preserve the existing native ProcessExitError code conversion without
    // invoking guest coercion or sentinel/error getters to recognize an exit.
    let code = if value.is_undefined() || value.is_null() {
        0
    } else if value.is_number() {
        value.int32_value(scope).unwrap_or(0)
    } else {
        1
    };
    let original = args.get(1);
    record_terminal(
        scope,
        original,
        TerminalInput::ProcessExit {
            code,
            diagnostic: args.length() > 1,
        },
    );
}

pub(crate) fn report_terminal_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _return_value: v8::ReturnValue,
) {
    record_terminal(scope, args.get(0), TerminalInput::Callback);
}

pub(crate) fn record_exception(scope: &mut v8::HandleScope, original: v8::Local<v8::Value>) {
    record_terminal(scope, original, TerminalInput::Callback);
}

fn record_terminal(
    scope: &mut v8::HandleScope,
    original: v8::Local<v8::Value>,
    input: TerminalInput,
) {
    let Some(owner) = scope.get_slot::<TerminalCallbackOwner>().cloned() else {
        eprintln!("ERR_AGENTOS_CALLBACK_TERMINAL_STATE: isolate terminal owner is not installed");
        scope.terminate_execution();
        return;
    };
    // Never hold the owner borrow across classification or other V8 operations.
    let (occupied, report_cleanup_failure) = {
        let mut state = owner.0.borrow_mut();
        let occupied = state.occupied();
        let report = occupied && state.settled && !state.cleanup_failure_reported;
        if occupied {
            state.stop_pending = true;
            if let TerminalCallbackPhase::Recording { interrupted, .. } = &mut state.phase {
                // V8 may not expose termination until after the getter returns.
                // Preserve native reentry independently of that delayed flag.
                *interrupted = true;
            }
        }
        if report {
            state.cleanup_failure_reported = true;
        }
        (occupied, report)
    };
    if occupied {
        if report_cleanup_failure {
            eprintln!("ERR_AGENTOS_CALLBACK_TERMINAL_STATE: additional callback failure during completed-call cleanup; original terminal result retained");
        }
        deliver_terminal_stop(scope);
        return;
    }
    let mut suppression_storage = std::pin::pin!(MicrotaskSuppressionStorage::new());
    let mut explicit = ExplicitMicrotaskScope::new(scope, suppression_storage.as_mut());
    let scope = &mut *explicit;
    let retained = v8::Global::new(scope, original);
    let (generation, after_settle) = {
        let mut state = owner.0.borrow_mut();
        state.stop_pending = true;
        let identity = (state.generation, state.settled);
        state.phase = TerminalCallbackPhase::Recording {
            original: retained,
            interrupted: false,
        };
        identity
    };
    // No mutable state borrow crosses this guest-observable classifier.
    let (kind, result, error_disposition) = {
        let tc = &mut v8::TryCatch::new(scope);
        let classified = match input {
            TerminalInput::Callback => classify_terminal_exception(tc, original),
            TerminalInput::ProcessExit { code, diagnostic } => (
                TerminalExceptionKind::ProcessExit,
                (
                    code,
                    if diagnostic {
                        crate::execution::extract_error_info(tc, original)
                    } else {
                        ExecutionError {
                            error_type: "ProcessExitError".into(),
                            message: format!("process.exit({code})"),
                            stack: String::new(),
                            code: None,
                        }
                    },
                ),
            ),
        };
        if tc.has_terminated() || tc.has_caught() {
            let (kind, result) = input.classification_failure();
            (kind, result, TerminalErrorDisposition::Diagnostic)
        } else {
            let (kind, result) = classified;
            (kind, result, input.error_disposition(kind))
        }
    };
    {
        let mut state = owner.0.borrow_mut();
        if state.generation == generation {
            let phase = std::mem::take(&mut state.phase);
            state.phase = match phase {
                TerminalCallbackPhase::Recording {
                    original,
                    interrupted,
                } => TerminalCallbackPhase::Recorded {
                    original,
                    result: if interrupted {
                        input.classification_failure().1
                    } else {
                        result
                    },
                    kind: if interrupted {
                        input.classification_failure().0
                    } else {
                        kind
                    },
                    error_disposition: if interrupted {
                        TerminalErrorDisposition::Diagnostic
                    } else {
                        error_disposition
                    },
                    after_settle,
                },
                other => other,
            };
        }
    }
    deliver_terminal_stop(scope);
}

fn initialize_delivery_key_in_scope(scope: &mut v8::HandleScope<'_, ()>) -> Result<(), String> {
    if scope.get_slot::<DeliveryKey>().is_none() {
        let key = private_delivery_key(scope).ok_or("terminal private key allocation failed")?;
        let key = DeliveryKey(Rc::new(v8::Global::new(scope, key)));
        scope.set_slot(key);
    }
    Ok(())
}

// Only use this wrapper before entering scopes. An active HandleScope must use
// the scope-preserving helper: reopening from Isolate tries to activate its root.
pub(crate) fn initialize_delivery_key(isolate: &mut v8::Isolate) {
    if isolate.get_slot::<DeliveryKey>().is_none() {
        let scope = &mut v8::HandleScope::new(&mut *isolate);
        initialize_delivery_key_in_scope(scope)
            .expect("terminal private key initialized before guest entry");
    }
}

pub(crate) fn release_delivery_key(isolate: &mut v8::Isolate) {
    // Snapshot serialization requires native Global cells to be reset while the
    // isolate is still live; dispose_annex invalidates handles before slots drop.
    drop(isolate.remove_slot::<DeliveryKey>());
}

pub(crate) fn validate_delivery(scope: &mut v8::HandleScope) -> Result<(), String> {
    let key = scope
        .get_slot::<DeliveryKey>()
        .map(|key| key.0.clone())
        .ok_or("native terminal private key is not initialized")?;
    let key = v8::Local::new(scope, &*key);
    let global = scope.get_current_context().global(scope);
    let function = global
        .get_private(scope, key)
        .ok_or("native terminal private entry lookup failed")?;
    if !function.is_function() {
        return Err("native terminal private entry is missing".into());
    }
    Ok(())
}

// Only a real JavaScript entry has the interrupt stack check needed to make
// the recorded native stop uncatchable before this callback returns. The trusted
// function is precompiled at context setup, not stored in FunctionTemplate data.
fn deliver_terminal_stop(scope: &mut v8::HandleScope) {
    if scope.is_execution_terminating() {
        return;
    }
    let mut suppression_storage = std::pin::pin!(MicrotaskSuppressionStorage::new());
    let mut explicit = ExplicitMicrotaskScope::new(scope, suppression_storage.as_mut());
    let tc = &mut v8::TryCatch::new(&mut *explicit);
    let key = tc.get_slot::<DeliveryKey>().map(|key| key.0.clone());
    let barrier = key.and_then(|key| {
        let key = v8::Local::new(tc, &*key);
        let global = tc.get_current_context().global(tc);
        global
            .get_private(tc, key)
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    });
    if tc.has_terminated() {
        assert!(tc.rethrow().is_some(), "caught termination can be rethrown");
        return;
    }
    let Some(barrier) = barrier else {
        // Setup rejects a missing barrier before guest execution. This visible
        // invariant failure must never be accepted as successful delivery.
        eprintln!(
            "ERR_AGENTOS_CALLBACK_TERMINAL_STATE: private terminal JavaScript entry unavailable"
        );
        tc.terminate_execution();
        return;
    };
    let receiver = v8::undefined(tc);
    let delivered = crate::isolate::with_terminal_stack_margin(tc, |scope| {
        scope.terminate_execution();
        barrier.call(scope, receiver.into(), &[]).is_some()
    });
    // The ordinary limit has already been restored before rethrowing, including
    // when the entry encountered a previously pending external termination.
    if tc.has_terminated() {
        assert!(tc.rethrow().is_some(), "caught termination can be rethrown");
        return;
    }
    match delivered {
        Ok(returned) => eprintln!("ERR_AGENTOS_CALLBACK_TERMINAL_STATE: JavaScript entry did not deliver requested termination (returned={returned})"),
        Err(error) => eprintln!("ERR_AGENTOS_CALLBACK_TERMINAL_STATE: terminal stack margin rejected: {error}"),
    }
    // This visible failure is never successful delivery. The finite prototype
    // owner must reject either diagnostic, even if another check later stops JS.
    tc.terminate_execution();
}

/// Install before bridge evaluation for both fresh and snapshot-created realms.
/// The bridge captures/deletes this native hook; snapshots use bridge::external_refs.
pub(crate) fn install(scope: &mut v8::HandleScope) -> Result<(), String> {
    initialize_delivery_key_in_scope(scope)?;
    if scope.get_slot::<TerminalCallbackOwner>().is_none() {
        scope.set_slot(TerminalCallbackOwner::default());
    }
    let key = private_delivery_key(scope).ok_or("terminal private key allocation failed")?;
    let source =
        v8::String::new(scope, "return;").ok_or("terminal entry source allocation failed")?;
    let mut source = v8::script_compiler::Source::new(source, None);
    let barrier = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &[],
        &[],
        v8::script_compiler::CompileOptions::EagerCompile,
        v8::script_compiler::NoCacheReason::NoReason,
    )
    .ok_or("terminal entry compilation failed")?;
    let global = scope.get_current_context().global(scope);
    if global.set_private(scope, key, barrier.into()) != Some(true) {
        return Err("terminal private entry installation failed".into());
    }
    let function =
        v8::Function::new(scope, report_terminal_callback).expect("terminal callback function");
    let name = v8::String::new(scope, GLOBAL).expect("terminal callback name");
    global.set(scope, name.into(), function.into());
    let exit_function =
        v8::Function::new(scope, request_process_exit).expect("native process exit");
    let exit_name = v8::String::new(scope, EXIT_GLOBAL).expect("native process exit name");
    global.set(scope, exit_name.into(), exit_function.into());
    Ok(())
}

/// Read the already-classified result without guest property access.
pub(crate) fn result(isolate: &v8::Isolate) -> Option<(i32, ExecutionError)> {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .and_then(|owner| owner.0.borrow().result())
}

/// Only a recorded typed silent exit may omit the error field on the result
/// frame. Diagnostic text and constructor names never select this disposition.
pub(crate) fn is_silent_process_exit(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .is_some_and(|owner| {
            let state = owner.0.borrow();
            state.stop_pending
                && matches!(
                    state.phase,
                    TerminalCallbackPhase::Recorded {
                        error_disposition: TerminalErrorDisposition::SilentProcessExit,
                        ..
                    }
                )
        })
}

/// Inspect the recorded classification without re-entering guest code.
pub(crate) fn is_process_exit(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .is_some_and(|owner| {
            let state = owner.0.borrow();
            state.stop_pending
                && matches!(
                    state.phase,
                    TerminalCallbackPhase::Recorded {
                        kind: TerminalExceptionKind::ProcessExit,
                        ..
                    }
                )
        })
}

pub(crate) fn pending(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .is_some_and(|owner| owner.0.borrow().pending())
}

pub(crate) fn mark_settled(isolate: &mut v8::Isolate) {
    if let Some(owner) = isolate.get_slot::<TerminalCallbackOwner>() {
        owner.0.borrow_mut().settled = true;
    }
}

pub(crate) fn after_settle(isolate: &v8::Isolate) -> bool {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .is_some_and(|owner| {
            matches!(
                owner.0.borrow().phase,
                TerminalCallbackPhase::Recorded {
                    after_settle: true,
                    ..
                }
            )
        })
}

pub(crate) fn acknowledge_stop(isolate: &mut v8::Isolate) {
    if let Some(owner) = isolate.get_slot::<TerminalCallbackOwner>() {
        owner.0.borrow_mut().stop_pending = false;
    }
}

fn release_phase(phase: TerminalCallbackPhase) {
    match phase {
        TerminalCallbackPhase::Recording { original, .. }
        | TerminalCallbackPhase::Recorded { original, .. } => drop(original),
        TerminalCallbackPhase::Empty => {}
    }
}

pub(crate) fn clear(isolate: &mut v8::Isolate) {
    if let Some(owner) = isolate.get_slot::<TerminalCallbackOwner>() {
        let previous = owner.0.borrow_mut().reset(0);
        release_phase(previous);
    }
}

pub(crate) struct ExecutionTerminalGuard(TerminalCallbackOwner);

impl Drop for ExecutionTerminalGuard {
    fn drop(&mut self) {
        let previous = self.0 .0.borrow_mut().reset(0);
        release_phase(previous);
    }
}

pub(crate) fn begin_call(isolate: &mut v8::Isolate, generation: u64) -> ExecutionTerminalGuard {
    initialize_delivery_key(isolate);
    let owner = isolate
        .get_slot::<TerminalCallbackOwner>()
        .expect("configured native terminal owner")
        .clone();
    let previous = owner.0.borrow_mut().reset(generation);
    release_phase(previous);
    ExecutionTerminalGuard(owner)
}

#[cfg(test)]
include!("../tests/support/terminal_callback_owner.rs");
