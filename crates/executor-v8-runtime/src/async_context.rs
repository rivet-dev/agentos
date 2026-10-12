//! Engine primitives behind the guest's `AsyncLocalStorage`.
//!
//! V8 saves the isolate's continuation-preserved embedder data (CPED) with every
//! promise reaction and queued microtask, and puts it back when that continuation
//! runs. The bridge keeps its async context frame in that slot
//! (`bridge-src/builtins/async-context.ts`).
//!
//! V8 does not manage the slot for host entry points, so every host execution and
//! event starts with no context ([`clear_async_context`]). The bridge then enters the
//! frame of the resource that owns the event. A path that forgets to do so delivers no
//! store, never a store left by an earlier event.

const GET_GLOBAL: &str = "__agentOsGetAsyncContext";
const SET_GLOBAL: &str = "__agentOsSetAsyncContext";

/// Installs the functions the bridge takes at load time. The bridge deletes these
/// globals, so guest code never sees them. They are V8 builtins from the extras
/// binding: TurboFan inlines them, and a snapshot can hold them with no external
/// reference.
pub fn install_async_context_globals(scope: &mut v8::HandleScope) {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let binding = context.get_extras_binding_object(scope);
    for (name, builtin) in [
        (GET_GLOBAL, "getContinuationPreservedEmbedderData"),
        (SET_GLOBAL, "setContinuationPreservedEmbedderData"),
    ] {
        let builtin_key = v8::String::new(scope, builtin).expect("static V8 string");
        let function = binding
            .get(scope, builtin_key.into())
            .expect("V8 extras binding has the async context builtins");
        let key = v8::String::new(scope, name).expect("static V8 string");
        global.set(scope, key.into(), function);
    }
}

/// Starts a host execution or event with no async context.
pub fn clear_async_context(scope: &mut v8::HandleScope) {
    let undefined = v8::undefined(scope);
    scope.set_continuation_preserved_embedder_data(undefined.into());
}
