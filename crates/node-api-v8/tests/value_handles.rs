use agentos_node_api_v8::{CapabilityError, CapabilityKind, V8NodeApiEnvironment};

#[test]
fn v8_values_are_opaque_scope_owned_and_stale_after_scope_close() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    let mut isolate = v8::Isolate::new(v8::CreateParams::default());
    let context;
    let mut environment = V8NodeApiEnvironment::new(16).unwrap();
    let stale;
    {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let local_context = v8::Context::new(scope, v8::ContextOptions::default());
        context = v8::Global::new(scope, local_context);
        let context_local = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context_local);
        let handle_scope = environment.open_handle_scope().unwrap();
        let value: v8::Local<v8::Value> = v8::String::new(scope, "opaque").unwrap().into();
        stale = environment.add_value(scope, handle_scope, value).unwrap();
        assert_eq!(
            environment
                .value(scope, stale)
                .unwrap()
                .to_string(scope)
                .unwrap()
                .to_rust_string_lossy(scope),
            "opaque"
        );
        environment.close_handle_scope(handle_scope).unwrap();
        assert!(matches!(
            environment.value(scope, stale),
            Err(CapabilityError::InvalidHandle { .. })
        ));
        assert_eq!(environment.live_capabilities(), 0);
        environment.teardown();
    }
    drop(context);
    drop(isolate);
}

#[test]
fn capability_kind_is_not_guest_forgeable() {
    assert_ne!(CapabilityKind::Value, CapabilityKind::Reference);
}
