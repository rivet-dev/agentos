//! Session realm-family microtasks. Generic embedder contexts keep V8's default
//! queue. Weak membership survives bridge registry resets without retaining realms.
use std::cell::RefCell;
use std::rc::Rc;

// The existing node:vm ceiling plus the session root and current process root.
const MAX_FAMILY_CONTEXTS: usize = 1026;

struct Family {
    queue: v8::MicrotaskQueueHandle,
    default_queue: *const v8::MicrotaskQueue,
    contexts: Vec<v8::Weak<v8::Context>>,
    policy: v8::MicrotasksPolicy,
    near_limit_warned: bool,
}

#[derive(Clone)]
struct FamilySlot(Rc<RefCell<Family>>);

/// Ensures custom queues are detached before the underlying isolate is disposed,
/// including early returns and Rust unwinding. No realm-valued native root is added.
pub(crate) struct SessionIsolate(Option<crate::isolate::GuestIsolate>);

impl SessionIsolate {
    pub(crate) fn new(isolate: crate::isolate::GuestIsolate) -> Self {
        Self(Some(isolate))
    }
}

impl std::ops::Deref for SessionIsolate {
    type Target = v8::Isolate;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("live session isolate")
    }
}

impl std::ops::DerefMut for SessionIsolate {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("live session isolate")
    }
}

impl Drop for SessionIsolate {
    fn drop(&mut self) {
        if let Some(mut isolate) = self.0.take() {
            crate::bridge::release_vm_context_registry(&mut isolate);
            if let Some(slot) = isolate.remove_slot::<FamilySlot>() {
                let mut family = slot.0.borrow_mut();
                assert!(!family.queue.is_running_microtasks());
                assert_eq!(family.queue.get_microtasks_scope_depth(), 0);
                v8::scope!(scope, &mut *isolate);
                for weak in &family.contexts {
                    if let Some(context) = weak
                        .to_local(scope)
                        .filter(|c| c.get_microtask_queue().is_some())
                    {
                        // Saved from the first context; the default queue belongs to
                        // this still-live isolate and outlives every custom queue.
                        context.set_microtask_queue(unsafe { &*family.default_queue });
                    }
                }
                family.contexts.clear();
                drop(family);
                drop(slot);
            }
            crate::isolate::drop_isolate(Some(isolate));
        }
    }
}

#[derive(Debug)]
pub(crate) enum ContextAdmissionError {
    Limit(String),
    Interrupted,
}
impl std::fmt::Display for ContextAdmissionError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Limit(message) => output.write_str(message),
            Self::Interrupted => output.write_str("ERR_AGENTOS_VM_CONTEXT_ADMISSION_INTERRUPTED: realm collection encountered an existing termination or heap stop"),
        }
    }
}

fn room(isolate: &mut v8::Isolate, slot: &FamilySlot) -> Result<(), ContextAdmissionError> {
    {
        let mut family = slot.0.borrow_mut();
        family.contexts.retain(|context| !context.is_empty());
        if family.contexts.len() < MAX_FAMILY_CONTEXTS * 4 / 5 {
            family.near_limit_warned = false;
        } else if !family.near_limit_warned {
            family.near_limit_warned = true;
            eprintln!("WARN_AGENTOS_VM_CONTEXT_LIMIT: retained realm family has {} weak entries, capacity {MAX_FAMILY_CONTEXTS}", family.contexts.len());
        }
        if family.contexts.len() < MAX_FAMILY_CONTEXTS {
            return Ok(());
        }
    }
    // Weak entries can be unreachable but not collected yet. At saturation only,
    // collect once without executing guest jobs, then retry the bounded admission.
    eprintln!("WARN_AGENTOS_VM_CONTEXT_LIMIT: realm family reached {MAX_FAMILY_CONTEXTS} weak entries; collecting before admission");
    crate::execution::collect_without_microtasks(isolate);
    if isolate.is_execution_terminating() || crate::isolate::heap_limit_exceeded(isolate) {
        return Err(ContextAdmissionError::Interrupted);
    }
    let mut family = slot.0.borrow_mut();
    family.contexts.retain(|context| !context.is_empty());
    if family.contexts.len() >= MAX_FAMILY_CONTEXTS {
        return Err(ContextAdmissionError::Limit(format!("ERR_AGENTOS_VM_CONTEXT_LIMIT: realm family retained {MAX_FAMILY_CONTEXTS} contexts after collection; release retained realms before creating more")));
    }
    Ok(())
}

pub(crate) fn create_root(
    isolate: &mut v8::Isolate,
) -> Result<v8::Global<v8::Context>, ContextAdmissionError> {
    let existing = isolate.get_slot::<FamilySlot>().cloned();
    if let Some(slot) = &existing {
        room(isolate, slot)?;
    }
    v8::scope!(scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let slot = existing.unwrap_or_else(|| {
        let policy = scope.get_microtasks_policy();
        let default_queue = std::ptr::from_ref(
            context
                .get_microtask_queue()
                .expect("a new root context is attached to its isolate's default queue"),
        );
        let slot = FamilySlot(Rc::new(RefCell::new(Family {
            queue: v8::MicrotaskQueue::new(scope, policy),
            default_queue,
            contexts: Vec::new(),
            policy,
            near_limit_warned: false,
        })));
        scope.set_slot(slot.clone());
        slot
    });
    let mut family = slot.0.borrow_mut();
    // This factory is called only outside entered ContextScopes.
    context.set_microtask_queue(&family.queue);
    family.contexts.push(v8::Weak::new(scope, context));
    Ok(v8::Global::new(scope, context))
}

/// Called by the sole node:vm native context factory before guest code can run.
pub(crate) fn create_child<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut options: v8::ContextOptions<'s>,
) -> Result<v8::Local<'s, v8::Context>, ContextAdmissionError> {
    let Some(slot) = scope.get_slot::<FamilySlot>().cloned() else {
        return Ok(v8::Context::new(scope, options));
    };
    let parent = scope.get_current_context();
    if !parent
        .get_microtask_queue()
        .is_some_and(|queue| std::ptr::eq(queue, &*slot.0.borrow().queue))
    {
        return Ok(v8::Context::new(scope, options));
    }
    room(scope, &slot)?;
    let mut family = slot.0.borrow_mut();
    let queue = std::ptr::from_ref(&*family.queue).cast_mut();
    options.microtask_queue = Some(queue);
    let context = v8::Context::new(scope, options);
    family.contexts.push(v8::Weak::new(scope, context));
    Ok(context)
}

pub(crate) fn is_owned(scope: &mut v8::PinScope<'_, '_>) -> bool {
    let Some(slot) = scope.get_slot::<FamilySlot>().cloned() else {
        return false;
    };
    let context = scope.get_current_context();
    let owned = context
        .get_microtask_queue()
        .is_some_and(|queue| std::ptr::eq(queue, &*slot.0.borrow().queue));
    owned
}

pub(crate) fn checkpoint(scope: &mut v8::PinScope<'_, '_>) {
    // A pending terminal owner must reach outer disposal before any queued job.
    // In particular, V8 forbids entering RunMicrotasks with delivered termination.
    if crate::terminal_callback::pending(scope) || scope.is_execution_terminating() {
        return;
    }
    crate::next_ticks::checkpoint(scope);
}

/// All ContextScopes/callbacks have unwound. Never executes a checkpoint or guest
/// cleanup, and never cancels termination. The caller retains its abort fence.
pub(crate) fn discard_failed_call(isolate: &mut v8::Isolate) {
    let Some(slot) = isolate.get_slot::<FamilySlot>().cloned() else {
        return;
    };
    let mut family = slot.0.borrow_mut();
    assert!(!family.queue.is_running_microtasks());
    assert_eq!(family.queue.get_microtasks_scope_depth(), 0);
    let replacement = v8::MicrotaskQueue::new(isolate, family.policy);
    {
        v8::scope!(scope, isolate);
        family.contexts.retain(|context| !context.is_empty());
        for weak in &family.contexts {
            if let Some(context) = weak
                .to_local(scope)
                .filter(|c| c.get_microtask_queue().is_some())
            {
                context.set_microtask_queue(&replacement);
                let scope = &mut v8::ContextScope::new(scope, context);
                crate::next_ticks::discard(scope);
            }
        }
    }
    // The old queue now has no live associated realm; its destructor discards
    // queued jobs without invoking them. Promise/global state stays in each realm.
    family.queue = replacement;
}

#[cfg(test)]
#[path = "../tests/support/owned_microtasks.rs"]
pub(crate) mod tests;
