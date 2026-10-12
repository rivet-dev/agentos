// Real V8 callbacks exercise the existing bounded queue safeguard and ownership
// transfer. No timer, host I/O, or watchdog is involved.
fn module_evaluation_transfer_controls() {
    for rejected_first in [false, true] {
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let resolver = v8::PromiseResolver::new(scope).unwrap();
        let promise = resolver.get_promise(scope);
        let reason = v8::String::new(scope, "owned evaluation").unwrap();
        if rejected_first {
            assert_eq!(resolver.reject(scope, reason.into()), Some(true));
        }
        isolate::claim_module_evaluation_rejection(scope, promise).unwrap();
        assert!(
            take_unhandled_promise_rejection(scope).is_none(),
            "claimed rejected evaluation is removed before reset"
        );
        scope
            .get_slot_mut::<isolate::PromiseRejectState>()
            .unwrap()
            .reset_execution();
        if !rejected_first {
            assert_eq!(resolver.reject(scope, reason.into()), Some(true));
        }
        assert!(
            take_unhandled_promise_rejection(scope).is_none(),
            "old module cannot become next-call UR"
        );
    }
    {
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let resolver = v8::PromiseResolver::new(scope).unwrap();
        let promise = resolver.get_promise(scope);
        let reason = v8::undefined(scope);
        assert_eq!(resolver.reject(scope, reason.into()), Some(true));
        let state = scope.get_slot_mut::<isolate::PromiseRejectState>().unwrap();
        assert!(state.begin_drain());
        assert!(state.begin_batch().is_some());
        let error = isolate::claim_module_evaluation_rejection(scope, promise).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("ERR_AGENTOS_PROMISE_REJECTION_STATE")
        );
        let state = scope.get_slot_mut::<isolate::PromiseRejectState>().unwrap();
        assert!(
            state.pop_reporting().is_some(),
            "sealed report remains with its original owner"
        );
        state.clear_pending();
    }
    for other_overflow in [false, true] {
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let reason = v8::String::new(scope, "bounded safeguard").unwrap();
        for _ in 0..isolate::MAX_UNHANDLED_PROMISE_REJECTIONS {
            let resolver = v8::PromiseResolver::new(scope).unwrap();
            assert_eq!(resolver.reject(scope, reason.into()), Some(true));
        }
        let owned = v8::PromiseResolver::new(scope).unwrap();
        assert_eq!(owned.reject(scope, reason.into()), Some(true));
        let other = v8::PromiseResolver::new(scope).unwrap();
        if other_overflow {
            assert_eq!(other.reject(scope, reason.into()), Some(true));
        }
        let promise = owned.get_promise(scope);
        isolate::claim_module_evaluation_rejection(scope, promise).unwrap();
        let state = scope.get_slot_mut::<isolate::PromiseRejectState>().unwrap();
        assert_eq!(
            state.take_failure().is_some(),
            other_overflow,
            "only owned overflow debt is released"
        );
        state.reset_execution();
        // Old overflow history cannot decrement the new generation's debt.
        let old_other = other.get_promise(scope);
        isolate::claim_module_evaluation_rejection(scope, old_other).unwrap();
        assert!(take_unhandled_promise_rejection(scope).is_none());
    }
}
