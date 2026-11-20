//! Node's nextTick phase belongs to the context, separately from Promise jobs.
//! Private V8 properties keep the hooks in their own realm without native roots.
use crate::execution::{ExplicitMicrotaskScope, MicrotaskSuppressionStorage};

const BOOTSTRAP: &str = "__agentOSNextTickHooks";
const HOOKS: &str = "agentos.nextTick.hooks";
const PENDING: &str = "agentos.nextTick.pending";
const BEFORE: &str = "agentos.nextTick.beforeCheckpoint";
const ENTRY: &str = "agentos.nextTick.entryIsModule";

fn key<'s>(scope: &mut v8::HandleScope<'s>, name: &str) -> v8::Local<'s, v8::Private> {
    let text = v8::String::new(scope, name).expect("nextTick private key");
    v8::Private::for_api(scope, Some(text))
}
fn get<'s>(scope: &mut v8::HandleScope<'s>, name: &str) -> Option<v8::Local<'s, v8::Value>> {
    let global = scope.get_current_context().global(scope);
    let key = key(scope, name);
    global.get_private(scope, key)
}
fn set(scope: &mut v8::HandleScope, name: &str, value: bool) {
    let global = scope.get_current_context().global(scope);
    let key = key(scope, name);
    let value = v8::Boolean::new(scope, value);
    assert_eq!(global.set_private(scope, key, value.into()), Some(true));
}
fn clear(scope: &mut v8::HandleScope, name: &str) {
    let global = scope.get_current_context().global(scope);
    let key = key(scope, name);
    assert_eq!(global.delete_private(scope, key), Some(true));
}
fn field<'s>(
    scope: &mut v8::HandleScope<'s>,
    object: v8::Local<v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    let key = v8::String::new(scope, name)?;
    object.get(scope, key.into())
}

pub(crate) fn capture(scope: &mut v8::HandleScope) -> Result<(), String> {
    let global = scope.get_current_context().global(scope);
    let name = v8::String::new(scope, BOOTSTRAP).ok_or("nextTick bootstrap key allocation")?;
    let value = global
        .get(scope, name.into())
        .ok_or("nextTick bootstrap lookup")?;
    if value.is_undefined() {
        return Ok(());
    }
    let hooks =
        v8::Local::<v8::Object>::try_from(value).map_err(|_| "invalid nextTick bootstrap")?;
    if !field(scope, hooks, "flush").is_some_and(|value| value.is_function())
        || !field(scope, hooks, "state").is_some_and(|value| value.is_object())
    {
        return Err("invalid nextTick bootstrap hooks".into());
    }
    let private = key(scope, HOOKS);
    if global.set_private(scope, private, hooks.into()) != Some(true)
        || global.delete(scope, name.into()) != Some(true)
    {
        return Err("cannot capture private nextTick hooks".into());
    }
    Ok(())
}

pub(crate) fn begin_entry(scope: &mut v8::HandleScope, module: bool) {
    set(scope, ENTRY, module);
}

pub(crate) fn scheduled(scope: &mut v8::HandleScope) -> Result<(), String> {
    if !get(scope, HOOKS).is_some_and(|value| value.is_object()) {
        return Err("nextTick hooks are not installed".into());
    }
    let deferred = match get(scope, ENTRY) {
        Some(value) if value.is_boolean() => value.boolean_value(scope),
        _ => scope
            .get_current_context()
            .get_microtask_queue()
            .is_running_microtasks(),
    };
    set(scope, PENDING, true);
    if !deferred {
        set(scope, BEFORE, true);
    }
    Ok(())
}

pub(crate) fn pending(scope: &mut v8::HandleScope) -> bool {
    get(scope, PENDING).is_some_and(|value| value.is_true())
}

fn stopped(scope: &mut v8::HandleScope) -> bool {
    crate::terminal_callback::pending(scope) || scope.is_execution_terminating()
}
fn flush(scope: &mut v8::HandleScope) {
    if stopped(scope) || !pending(scope) {
        return;
    }
    let mut storage = std::pin::pin!(MicrotaskSuppressionStorage::new());
    let mut explicit = ExplicitMicrotaskScope::new(scope, storage.as_mut());
    let tc = &mut v8::TryCatch::new(&mut *explicit);
    let hooks = get(tc, HOOKS)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .expect("scheduled nextTick hooks");
    let function = field(tc, hooks, "flush")
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .expect("captured nextTick flush");
    set(tc, PENDING, false);
    set(tc, BEFORE, false);
    match function.call(tc, hooks.into(), &[]) {
        Some(result) => {
            let remaining = result
                .uint32_value(tc)
                .expect("internal nextTick queue length");
            set(tc, PENDING, remaining > 0);
            set(tc, BEFORE, remaining > 0);
        }
        None if tc.has_terminated() => {}
        None => {
            let error = tc.exception().expect("nextTick flush exception");
            tc.reset();
            crate::terminal_callback::record_exception(tc, error);
        }
    }
}

pub(crate) fn checkpoint(scope: &mut v8::HandleScope) {
    if stopped(scope) {
        return;
    }
    clear(scope, ENTRY);
    if get(scope, BEFORE).is_some_and(|value| value.is_true()) {
        flush(scope);
    }
    loop {
        if stopped(scope) {
            return;
        }
        let context = scope.get_current_context();
        context.get_microtask_queue().perform_checkpoint(scope);
        if stopped(scope) || !pending(scope) {
            return;
        }
        flush(scope);
    }
}

/// Replaces runtime-owned queue storage without invoking callbacks or setters
/// supplied by guest code. The private state object never leaves the runtime.
pub(crate) fn discard(scope: &mut v8::HandleScope) {
    if let Some(hooks) =
        get(scope, HOOKS).and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    {
        let state = field(scope, hooks, "state")
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            .expect("captured nextTick state");
        let queue = v8::Array::new(scope, 0);
        let name = v8::String::new(scope, "queue").expect("nextTick queue key");
        assert_eq!(state.set(scope, name.into(), queue.into()), Some(true));
    }
    clear(scope, ENTRY);
    set(scope, PENDING, false);
    set(scope, BEFORE, false);
}
