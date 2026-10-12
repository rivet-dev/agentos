//! A synchronous callback failure is a terminal execution outcome, never an
//! ordinary Promise rejection. The first failure reserves its original value
//! before error classification can invoke guest-controlled getters.
use crate::execution::{exception_to_result, ExplicitMicrotaskScope};
use crate::ipc::ExecutionError;
use std::cell::RefCell;
use std::rc::Rc;

const GLOBAL: &str = "__agentOsReportTerminalCallback";

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

pub(crate) fn report_terminal_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _return_value: v8::ReturnValue,
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
        scope.terminate_execution();
        return;
    }
    let mut explicit = ExplicitMicrotaskScope::new(scope);
    let scope = &mut *explicit;
    let original = args.get(0);
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
    let result = {
        let tc = &mut v8::TryCatch::new(scope);
        let classified = exception_to_result(tc, original);
        if tc.has_terminated() || tc.has_caught() {
            classification_failure()
        } else {
            classified
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
                        classification_failure()
                    } else {
                        result
                    },
                    after_settle,
                },
                other => other,
            };
        }
    }
    scope.terminate_execution();
}

/// Install before bridge evaluation for both fresh and snapshot-created realms.
/// The bridge captures/deletes this native hook; snapshots use bridge::external_refs.
pub(crate) fn install(scope: &mut v8::HandleScope) {
    if scope.get_slot::<TerminalCallbackOwner>().is_none() {
        scope.set_slot(TerminalCallbackOwner::default());
    }
    let global = scope.get_current_context().global(scope);
    let function =
        v8::Function::new(scope, report_terminal_callback).expect("terminal callback function");
    let name = v8::String::new(scope, GLOBAL).expect("terminal callback name");
    global.set(scope, name.into(), function.into());
}

/// Read the already-classified result without guest property access.
pub(crate) fn result(isolate: &v8::Isolate) -> Option<(i32, ExecutionError)> {
    isolate
        .get_slot::<TerminalCallbackOwner>()
        .and_then(|owner| owner.0.borrow().result())
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
