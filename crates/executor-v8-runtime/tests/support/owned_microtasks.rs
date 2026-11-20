// Invoked by the existing consolidated native owner controls.
use super::*;

fn run(scope: &mut v8::HandleScope, source: &str) -> Option<v8::Global<v8::Value>> {
    let source = v8::String::new(scope, source).unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    script.run(scope).map(|value| v8::Global::new(scope, value))
}

fn native_child_exit_control() {
    let mut owner = SessionIsolate::new(crate::isolate::create_isolate(None));
    let isolate = &mut *owner;
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    let root = create_root(isolate).unwrap();
    let _call = crate::terminal_callback::begin_call(isolate, 1);
    {
        let scope = &mut v8::HandleScope::new(&mut *isolate);
        let context = v8::Local::new(scope, &root);
        let scope = &mut v8::ContextScope::new(scope, context);
        crate::terminal_callback::install(scope).unwrap();
        let bridge = crate::host_call::BridgeCallContext::new(
            Box::new(Vec::new()),
            Box::new(std::io::Cursor::new(Vec::new())),
            "owned-child".into(),
        );
        let _functions = crate::bridge::register_sync_bridge_fns(
            scope,
            &bridge,
            &["_vmCreateContext", "_vmRunInContext"],
        );
        let tc = &mut v8::TryCatch::new(scope);
        assert!(run(tc, "globalThis.box={stop:__agentOsRequestProcessExit,shared:{continued:false}};globalThis.childId=_vmCreateContext(box);try{_vmRunInContext(childId, 'stop(9);shared.continued=true', {}, box);globalThis.BAD_PARENT=true}catch{globalThis.BAD_CATCH=true}finally{globalThis.BAD_FINALLY=true}").is_none());
        assert!(tc.has_terminated());
        assert_eq!(crate::terminal_callback::result(tc).unwrap().0, 9);
    }
    crate::bridge::clear_vm_context_registry_for_test(isolate);
    discard_failed_call(isolate);
    isolate.cancel_terminate_execution();
    crate::terminal_callback::acknowledge_stop(isolate);
    {
        let scope = &mut v8::HandleScope::new(&mut *isolate);
        let context = v8::Local::new(scope, &root);
        let scope = &mut v8::ContextScope::new(scope, context);
        let value = run(scope, "typeof BAD_PARENT==='undefined' && typeof BAD_CATCH==='undefined' && typeof BAD_FINALLY==='undefined' && box.shared.continued===false").unwrap();
        assert!(v8::Local::new(scope, &value).is_true());
    }
}

// Exercise the real owner rather than manually sweeping a test registry.
fn registry_lifecycle_eval(
    isolate: &mut v8::OwnedIsolate,
    root: &v8::Global<v8::Context>,
    source: &str,
) -> bool {
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Local::new(scope, root);
    let scope = &mut v8::ContextScope::new(scope, context);
    let bridge = crate::host_call::BridgeCallContext::new(
        Box::new(Vec::new()),
        Box::new(std::io::Cursor::new(Vec::new())),
        "registry-retirement".into(),
    );
    let _functions = crate::bridge::register_sync_bridge_fns(
        scope,
        &bridge,
        &["_vmCreateContext", "_vmRunInContext"],
    );
    let result = run(scope, source).expect("registry lifecycle evaluation");
    v8::Local::new(scope, &result).is_true()
}

fn registry_owner_retirement_controls() {
    for restored in [false, true] {
        let native = if restored {
            let blob =
                crate::snapshot::create_snapshot("globalThis.restoredRegistryMarker=1;").unwrap();
            crate::snapshot::create_isolate_from_snapshot(blob, None)
        } else {
            crate::isolate::create_isolate(None)
        };
        let mut owner = SessionIsolate::new(native);
        crate::bridge::clear_vm_context_registry_for_test(&mut owner);
        let root = create_root(&mut owner).unwrap();
        assert!(registry_lifecycle_eval(&mut owner, &root,
            "globalThis.registryBox={value:41};globalThis.registryId=_vmCreateContext(registryBox);globalThis.registryGlobal=_vmRunInContext(registryId,'globalThis',{},registryBox);globalThis.registryRead=_vmRunInContext(registryId,'(()=>value)',{},registryBox);registryRead()===41"));
        assert_eq!(
            crate::bridge::vm_context_registry_len_for_isolate_test(&mut owner),
            1
        );

        // Nested isolate ownership is supported by rusty_v8 when dropped LIFO.
        // Retiring this unrelated owner must not clear the first owner's IDs.
        let unrelated = SessionIsolate::new(crate::isolate::create_isolate(None));
        drop(unrelated);
        assert!(registry_lifecycle_eval(&mut owner, &root,
            "_vmRunInContext(registryId,'++value',{},registryBox)===42 && registryGlobal.value===42 && registryRead()===42"));
        assert_eq!(
            crate::bridge::vm_context_registry_len_for_isolate_test(&mut owner),
            1
        );
        drop(root);
        drop(owner);

        // No manual reset occurs between actual retirement and this observation.
        let mut successor = SessionIsolate::new(crate::isolate::create_isolate(None));
        assert_eq!(
            crate::bridge::vm_context_registry_len_for_isolate_test(&mut successor),
            0,
            "actual SessionIsolate retirement must not leave predecessor context roots"
        );
        let root = create_root(&mut successor).unwrap();
        assert!(registry_lifecycle_eval(&mut successor, &root,
            "globalThis.registryBox={value:7};globalThis.registryId=_vmCreateContext(registryBox);_vmRunInContext(registryId,'value',{},registryBox)===7"));
        drop(root);
        drop(successor);

        // The existing generic embedder drop helper must have the same ownership.
        let mut raw = crate::isolate::create_isolate(None);
        let root = crate::isolate::create_context(&mut raw);
        assert!(registry_lifecycle_eval(&mut raw, &root,
            "globalThis.registryBox={value:8};globalThis.registryId=_vmCreateContext(registryBox);_vmRunInContext(registryId,'value',{},registryBox)===8"));
        drop(root);
        crate::isolate::drop_isolate(Some(raw));
        let mut after_raw = SessionIsolate::new(crate::isolate::create_isolate(None));
        assert_eq!(
            crate::bridge::vm_context_registry_len_for_isolate_test(&mut after_raw),
            0,
            "generic isolate retirement must not leave predecessor context roots"
        );
    }
}

pub(crate) fn retained_family_controls() {
    registry_owner_retirement_controls();
    native_child_exit_control();
    for restored in [false, true] {
        for policy in [v8::MicrotasksPolicy::Auto, v8::MicrotasksPolicy::Explicit] {
            let native = if restored {
                let blob = crate::snapshot::create_snapshot(
                    "globalThis.__savedExit=__agentOsRequestProcessExit;",
                )
                .unwrap();
                crate::snapshot::create_isolate_from_snapshot(blob, None)
            } else {
                crate::isolate::create_isolate(None)
            };
            let mut owner = SessionIsolate::new(native);
            let isolate = &mut *owner;
            isolate.set_microtasks_policy(policy);
            // Both restored contexts must find their own serialized private
            // function with the same isolate-private symbol identity.
            let roots = [create_root(isolate).unwrap(), create_root(isolate).unwrap()];
            for (index, root) in roots.iter().enumerate() {
                {
                    let _call =
                        crate::terminal_callback::begin_call(isolate, (index * 2 + 1) as u64);
                    {
                        let scope = &mut v8::HandleScope::new(&mut *isolate);
                        let context = v8::Local::new(scope, root);
                        let scope = &mut v8::ContextScope::new(scope, context);
                        if !restored {
                            crate::terminal_callback::install(scope).unwrap();
                        }
                        crate::terminal_callback::validate_delivery(scope).unwrap();
                        let mut storage =
                            std::pin::pin!(crate::execution::MicrotaskSuppressionStorage::new());
                        let mut explicit =
                            crate::execution::ExplicitMicrotaskScope::new(scope, storage.as_mut());
                        let scope = &mut *explicit;
                        let child = crate::bridge::create_registered_child_for_test(scope);
                        let (function, read_old_job) = {
                            let scope = &mut v8::ContextScope::new(scope, child);
                            let function = run(scope, "globalThis.oldJob=false;Promise.resolve().then(()=>{oldJob=true});(()=>Promise.resolve(41).then(x=>x+1))").unwrap();
                            let read_old_job = run(scope, "(()=>oldJob)").unwrap();
                            (function, read_old_job)
                        };
                        let function = v8::Local::new(scope, &function);
                        let global = context.global(scope);
                        let key = v8::String::new(scope, "childFunction").unwrap();
                        assert_eq!(global.set(scope, key.into(), function), Some(true));
                        let child_global = child.global(scope);
                        let key = v8::String::new(scope, "childGlobal").unwrap();
                        assert_eq!(
                            global.set(scope, key.into(), child_global.into()),
                            Some(true)
                        );
                        // Observe the child's own global state through a child
                        // closure, not parent access to its raw global proxy.
                        let read_old_job = v8::Local::new(scope, &read_old_job);
                        let key = v8::String::new(scope, "childReadOldJob").unwrap();
                        assert_eq!(global.set(scope, key.into(), read_old_job), Some(true));
                        let before = run(scope, "childReadOldJob()===false").unwrap();
                        assert!(v8::Local::new(scope, &before).is_true());
                        // No strong registry retains the child. Parent globals
                        // are its only live ownership across the failed call.
                        let source = "globalThis.pendingValue=0;globalThis.pending=new Promise(r=>globalThis.resolveLater=r);pending.then(x=>pendingValue=x);(globalThis.__savedExit ?? globalThis.__agentOsRequestProcessExit)(9);globalThis.BAD_CONTINUATION=true";
                        assert!(run(scope, source).is_none());
                        assert!(crate::terminal_callback::pending(scope));
                        assert_eq!(crate::terminal_callback::result(scope).unwrap().0, 9);
                    }
                    crate::bridge::clear_vm_context_registry_for_test(isolate);
                    discard_failed_call(isolate);
                    isolate.cancel_terminate_execution();
                    crate::terminal_callback::acknowledge_stop(isolate);
                }
                {
                    let _call =
                        crate::terminal_callback::begin_call(isolate, (index * 2 + 2) as u64);
                    let scope = &mut v8::HandleScope::new(&mut *isolate);
                    let context = v8::Local::new(scope, root);
                    let scope = &mut v8::ContextScope::new(scope, context);
                    assert!(run(
                        scope,
                        "globalThis.newValue=0;childFunction().then(x=>newValue=x);resolveLater(7)"
                    )
                    .is_some());
                    checkpoint(scope);
                    let value = run(scope, "newValue===42 && pendingValue===7 && childReadOldJob()===false && typeof BAD_CONTINUATION==='undefined'").unwrap();
                    assert!(
                        v8::Local::new(scope, &value).is_true(),
                        "retained child and pending Promise survive; old queued job does not"
                    );
                }
            }
        }
    }
    // Preserve the existing 4096-by-three registry churn contract using real
    // child contexts this time. Short HandleScopes allow unreachable children
    // to be collected at bounded-family admission rather than retained here.
    let mut owner = SessionIsolate::new(crate::isolate::create_isolate(None));
    let isolate = &mut *owner;
    let root = create_root(isolate).unwrap();
    for _ in 0..4096 {
        let scope = &mut v8::HandleScope::new(&mut *isolate);
        let context = v8::Local::new(scope, &root);
        let scope = &mut v8::ContextScope::new(scope, context);
        for _ in 0..3 {
            crate::bridge::create_registered_child_for_test(scope);
        }
        crate::bridge::clear_vm_context_registry_for_test(scope);
    }
    assert!(
        isolate
            .get_slot::<FamilySlot>()
            .unwrap()
            .0
            .borrow()
            .contexts
            .len()
            <= MAX_FAMILY_CONTEXTS
    );
}
