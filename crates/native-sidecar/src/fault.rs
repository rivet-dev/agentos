//! Scope-based fault handling for the sidecar's inline serve path.
//!
//! The sidecar is the trusted enforcement point shared by every VM in the
//! process — and, on Rivet Compute, by every co-located actor on the runner.
//! Historically any error raised while servicing guest-driven work propagated
//! with `?` out of the serve loop to `main`, which exited(1): one guest's errno
//! killed every co-located tenant (LT-011).
//!
//! The failure was not that the wrong *kind* of error escaped. It was that the
//! error type carried no notion of **what is actually broken**, so `?` — the
//! shortest, most idiomatic thing to write — implicitly meant "kill the
//! process". Blame ("was this the guest's fault?") is the wrong axis: a host
//! error that only damaged one VM's state should still take down only that VM.
//! Scope is the right axis.
//!
//! So the default direction is inverted here. [`Fault`] converts from ordinary
//! errors as [`FaultScope::Isolated`], meaning "confine this to the current unit
//! of work". Escalating to [`FaultScope::Fatal`] must be written out explicitly
//! at the call site, so the failure mode of carelessness is "one VM died"
//! rather than "the runner died".
//!
//! Ownership/scope reuses [`TaskOwner`] from `agentos-runtime` rather than
//! introducing a second scope enum, so supervised spawned tasks and inline
//! serve-loop work share one ownership model.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use agentos_runtime::{TaskOwner, TaskTerminalReason};

/// How far a fault must propagate before the system is consistent again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaultScope {
    /// Confine to the current unit of work: fail the request, tear down the
    /// owning VM/session if its state may be inconsistent, keep serving
    /// everyone else. This is the default for any converted error.
    Isolated,
    /// The substrate shared by ALL work is unusable, so continuing would serve
    /// wrong answers rather than fewer answers. Reserved for a deliberately
    /// short list: the host transport is gone, framing on the shared channel is
    /// corrupt, or allocation failed. "A host-side error occurred" is NOT on
    /// its own a reason to be fatal.
    Fatal,
}

/// An abnormal end to a unit of sidecar work, tagged with how far it must
/// propagate.
#[derive(Clone, Debug)]
pub(crate) struct Fault {
    pub(crate) scope: FaultScope,
    pub(crate) reason: TaskTerminalReason,
    pub(crate) cause: String,
}

impl Fault {
    /// Confine to the current unit of work. Prefer the `From` conversion; this
    /// exists for call sites that build a cause string directly.
    pub(crate) fn isolated(cause: impl Into<String>) -> Self {
        Self {
            scope: FaultScope::Isolated,
            reason: TaskTerminalReason::Failed,
            cause: cause.into(),
        }
    }

    /// Take down the process. Only for shared-substrate failures — see
    /// [`FaultScope::Fatal`]. Every use should be obvious on inspection.
    pub(crate) fn fatal(cause: impl Into<String>) -> Self {
        Self {
            scope: FaultScope::Fatal,
            reason: TaskTerminalReason::Failed,
            cause: cause.into(),
        }
    }

    /// A caught panic. Always isolated, never fatal — but the caller MUST reap
    /// the owning fault domain rather than resuming into it (see
    /// [`catch_faults`]).
    pub(crate) fn panicked(payload: &(dyn Any + Send)) -> Self {
        Self {
            scope: FaultScope::Isolated,
            reason: TaskTerminalReason::Panicked,
            cause: panic_message(payload),
        }
    }

    pub(crate) fn is_fatal(&self) -> bool {
        matches!(self.scope, FaultScope::Fatal)
    }

    /// True when the owning fault domain's state may be torn and must be
    /// destroyed rather than reused. A panic can unwind out of a half-finished
    /// mutation, so resuming would serve from inconsistent state.
    pub(crate) fn requires_teardown(&self) -> bool {
        matches!(self.reason, TaskTerminalReason::Panicked)
    }

    /// Escalate an isolated fault after a circuit breaker trips.
    pub(crate) fn escalated(mut self) -> Self {
        self.scope = FaultScope::Fatal;
        self
    }
}

/// Isolation is the default: any ordinary error confined to the current unit of
/// work. This is the inversion that makes `?` safe in the serve loop.
impl<E> From<E> for Fault
where
    E: std::fmt::Display,
{
    fn from(error: E) -> Self {
        Self::isolated(error.to_string())
    }
}

/// Run `body` with panics converted into isolated [`Fault`]s.
///
/// A caught panic must never simply resume: it can unwind out of a partially
/// applied mutation, so the owning domain's state may be torn. Callers are
/// expected to check [`Fault::requires_teardown`] and destroy `owner` rather
/// than continuing to serve it. That teardown is also what makes the
/// `AssertUnwindSafe` here honest — the possibly-inconsistent state is
/// discarded, not reused.
pub(crate) fn catch_faults<T>(
    owner: TaskOwner,
    body: impl FnOnce() -> Result<T, Fault>,
) -> Result<T, Fault> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(payload) => {
            let fault = Fault::panicked(payload.as_ref());
            tracing::error!(
                owner = %owner,
                cause = %fault.cause,
                "panic escaped sidecar work; isolating and reaping the owning scope"
            );
            Err(fault)
        }
    }
}

/// Guards against per-VM isolation silently masking a genuinely broken host.
///
/// Isolating every fault is right until the host itself is the problem — then
/// the sidecar would cheerfully fail every VM in a loop forever. After
/// `threshold` consecutive faults sharing a cause, escalate to fatal so the
/// supervisor can restart the process instead.
pub(crate) struct FaultBreaker {
    threshold: u32,
    consecutive: u32,
    last_cause: Option<String>,
}

impl FaultBreaker {
    pub(crate) fn new(threshold: u32) -> Self {
        Self {
            threshold: threshold.max(1),
            consecutive: 0,
            last_cause: None,
        }
    }

    /// Record a fault; returns the fault, escalated to fatal if this cause has
    /// now repeated `threshold` times without an intervening success.
    pub(crate) fn record(&mut self, fault: Fault) -> Fault {
        if self.last_cause.as_deref() == Some(fault.cause.as_str()) {
            self.consecutive = self.consecutive.saturating_add(1);
        } else {
            self.last_cause = Some(fault.cause.clone());
            self.consecutive = 1;
        }
        if self.consecutive >= self.threshold && !fault.is_fatal() {
            tracing::error!(
                cause = %fault.cause,
                consecutive = self.consecutive,
                "same fault repeated across units of work; escalating to fatal"
            );
            return fault.escalated();
        }
        fault
    }

    /// Any successful unit of work clears the run.
    pub(crate) fn record_success(&mut self) {
        self.consecutive = 0;
        self.last_cause = None;
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        String::from("panic with non-string payload")
    }
}
