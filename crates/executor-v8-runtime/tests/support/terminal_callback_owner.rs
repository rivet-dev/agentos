// Included in the native owner module; invoked by the single V8 suite.
#[cfg(test)]
pub(crate) fn terminal_callback_owner_controls() {
    native_process_exit_owner_controls();
    for restored in [false, true] {
        for (source, expected_status, expected_error) in [
            ("Error('first-original')", 1, None),
            ("null", 1, None),
            ("undefined", 1, None),
            (
                "({_isProcessExit:true,code:9,message:'process.exit(9)',name:'ProcessExitError'})",
                9,
                None,
            ),
            (
                "({_isAgentOSFatalException:true,cause:{_isProcessExit:true,code:9,message:'process.exit(9)'}})",
                9,
                None,
            ),
            (
                "({get _isProcessExit(){__savedTerminal(Error('second'));return true},code:9})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "({_isProcessExit:true,get code(){__savedTerminal(Error('second'));return 9}})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "Error('process.exit(9)')",
                9,
                None,
            ),
            (
                "({_isAgentOSFatalException:true,cause:Error('fatal-original')})",
                7,
                None,
            ),
            (
                "({get constructor(){__savedTerminal(Error('second'));return Object}})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "({get message(){__savedTerminal(Error('second'));return 'BAD'}})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "({get stack(){__savedTerminal(Error('second'));return 'BAD'}})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "({get code(){__savedTerminal(Error('second'));return 'BAD'}})",
                1,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
        ] {
            let mut isolate = if restored {
                let blob = crate::snapshot::create_snapshot("globalThis.__savedTerminal=__agentOsReportTerminalCallback;delete globalThis.__agentOsReportTerminalCallback;").expect("snapshot captures native terminal callback");
                crate::snapshot::create_isolate_from_snapshot(blob, None)
            } else {
                crate::isolate::create_isolate(None)
            };
            isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
            let context = crate::isolate::create_context(&mut isolate);
            {
                let scope = &mut v8::HandleScope::new(&mut isolate);
                let context = v8::Local::new(scope, &context);
                let scope = &mut v8::ContextScope::new(scope, context);
                if !restored {
                    install(scope).expect("native terminal hooks installed");
                }
                let code = format!("globalThis.__savedTerminal ??= globalThis.__agentOsReportTerminalCallback;delete globalThis.__agentOsReportTerminalCallback;globalThis.__after=false;globalThis.__first={source};Promise.resolve().then(()=>__savedTerminal(__first));Promise.resolve().then(()=>{{__after=true}});");
                let script_source = v8::String::new(scope, &code).unwrap();
                let script = v8::Script::compile(scope, script_source, None).unwrap();
                assert!(script.run(scope).is_some());
                scope.perform_microtask_checkpoint();
                assert!(pending(scope));
                let (status, error) =
                    result(scope).expect("terminal outcome recorded before checkpoint returns");
                assert_eq!(
                    status, expected_status,
                    "restored={restored}, source={source}, error={error:?}"
                );
                assert_eq!(
                    is_process_exit(scope),
                    expected_error.is_none() && source.contains("_isProcessExit:true"),
                    "classification must preserve explicit sentinel kind and reject reentry/message fallback; restored={restored}, source={source}"
                );
                if let Some(expected_error) = expected_error {
                    assert_eq!(
                        error.code.as_deref(),
                        Some(expected_error),
                        "restored={restored}, source={source}, error={error:?}"
                    );
                }
            }
            assert!(isolate.is_execution_terminating());
            // Test-local cancellation has no external abort owner. Production
            // cancellation is separately serialized by SharedExecutionAbort.
            isolate.cancel_terminate_execution();
            let mut original_weak = None;
            let sibling_weak;
            {
                let scope = &mut v8::HandleScope::new(&mut isolate);
                let context = v8::Local::new(scope, &context);
                let scope = &mut v8::ContextScope::new(scope, context);
                let global = context.global(scope);
                let key = v8::String::new(scope, "__after").unwrap();
                assert!(
                    global.get(scope, key.into()).unwrap().is_false(),
                    "later microtask cannot rescue terminal callback"
                );
                let key = v8::String::new(scope, "__first").unwrap();
                let first = global.get(scope, key.into()).unwrap();
                let owner = scope.get_slot::<TerminalCallbackOwner>().unwrap().clone();
                let original = {
                    let state = owner.0.borrow();
                    let TerminalCallbackPhase::Recorded { original, .. } = &state.phase else {
                        panic!("recorded owner")
                    };
                    original.clone()
                };
                let original = v8::Local::new(scope, original);
                assert!(
                    original.strict_equals(first),
                    "first original survives getter reentry"
                );
                if source == "Error('first-original')" {
                    let object = v8::Local::<v8::Object>::try_from(original).unwrap();
                    original_weak = Some(v8::Weak::new(scope, object));
                }
                let key = v8::String::new(scope, "__first").unwrap();
                let undefined = v8::undefined(scope);
                assert_eq!(global.set(scope, key.into(), undefined.into()), Some(true));
                assert!(
                    global.get(scope, key.into()).unwrap().is_undefined(),
                    "fixture global reference must be removed"
                );
                let sibling = v8::Object::new(scope);
                sibling_weak = v8::Weak::new(scope, sibling);
                let source = v8::String::new(
                    scope,
                    "Promise.resolve().then(()=>{globalThis.__clean=true});",
                )
                .unwrap();
                let script = v8::Script::compile(scope, source, None).unwrap();
                assert!(script.run(scope).is_some());
                scope.perform_microtask_checkpoint();
                let key = v8::String::new(scope, "__clean").unwrap();
                assert!(global.get(scope, key.into()).unwrap().is_true());
                let key = v8::String::new(scope, "__after").unwrap();
                assert!(
                    global.get(scope, key.into()).unwrap().is_false(),
                    "aborted microtask never resurfaces next execution"
                );
            }
            // Opening a fresh HandleScope below exits rusty_v8's deferred
            // zombie scope, releasing its Locals before collection. Direct GC
            // on the isolate would leave those fixture roots alive.
            // FullGC must retain the original solely because our owner holds it.
            assert!(!after_settle(&isolate));
            mark_settled(&mut isolate);
            assert!(
                !after_settle(&isolate),
                "settling later cannot rewrite original disposition"
            );
            acknowledge_stop(&mut isolate);
            assert!(!pending(&isolate));
            assert!(isolate
                .get_slot::<TerminalCallbackOwner>()
                .unwrap()
                .0
                .borrow()
                .occupied());
            {
                let scope = &mut v8::HandleScope::new(&mut isolate);
                scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
            }
            assert!(sibling_weak.is_empty(), "unowned sibling must collect after same Local scope; restored={restored}, source={source}");
            if let Some(original_weak) = original_weak.as_ref() {
                {
                    let scope = &mut v8::HandleScope::new(&mut isolate);
                    scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
                }
                assert!(
                    !original_weak.is_empty(),
                    "recorded owner must retain original across FullGC; restored={restored}, source={source}"
                );
            }
            clear(&mut isolate);
            assert!(!isolate
                .get_slot::<TerminalCallbackOwner>()
                .unwrap()
                .0
                .borrow()
                .occupied());
            assert!(
                !pending(&isolate),
                "current call cleanup leaves the owner empty"
            );
            if let Some(original_weak) = original_weak {
                {
                    let scope = &mut v8::HandleScope::new(&mut isolate);
                    scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
                }
                let released = original_weak.is_empty();
                assert!(
                    released,
                    "cleared owner must release original after FullGC; restored={restored}, source={source}"
                );
            }
            assert_eq!(
                isolate.get_microtasks_policy(),
                v8::MicrotasksPolicy::Explicit
            );
        }
    }
    // The per-call guard also clears a reserved original on an early setup
    // return; no next Execute or isolate disposal is needed for that cleanup.
    {
        let mut isolate = crate::isolate::create_isolate(None);
        let context = crate::isolate::create_context(&mut isolate);
        {
            let generation = isolate
                .get_slot::<crate::isolate::PromiseRejectState>()
                .unwrap()
                .generation();
            let _call = begin_call(&mut isolate, generation);
            mark_settled(&mut isolate);
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            install(scope).expect("native terminal hooks installed");
            let source = v8::String::new(
                scope,
                "__agentOsReportTerminalCallback(Error('early-setup')); ",
            )
            .unwrap();
            let script = v8::Script::compile(scope, source, None).unwrap();
            // The native owner is authoritative: V8 can return a value before
            // its pending termination request becomes visible to Script::run.
            let _immediate_v8_result = script.run(scope);
            assert!(pending(scope));
            let (status, error) = result(scope).expect("early return retains terminal outcome");
            assert_eq!(status, 1);
            assert_eq!(error.error_type, "Error");
            assert_eq!(error.message, "early-setup");
            assert!(scope
                .get_slot::<TerminalCallbackOwner>()
                .unwrap()
                .0
                .borrow()
                .occupied());
            assert!(
                after_settle(scope),
                "late failure retains its already-settled disposition"
            );
            // Leaving this block models the production config/post-restore
            // failure's early continue and exercises its actual guard Drop.
        }
        assert!(!isolate
            .get_slot::<TerminalCallbackOwner>()
            .unwrap()
            .0
            .borrow()
            .occupied());
        assert!(
            !pending(&isolate),
            "guard Drop clears pending termination ownership"
        );
        assert!(
            result(&isolate).is_none(),
            "guard Drop clears terminal outcome"
        );
        isolate.cancel_terminate_execution();
    }
}

// Uses the existing owner suite and snapshot lifetime; no new standalone runner.
fn native_process_exit_owner_controls() {
    crate::owned_microtasks::tests::retained_family_controls();
    for restored in [false, true] {
        for (source, expected_code, expected_diagnostic) in [
            ("__savedExit(0)", 0, None),
            ("__savedExit(9)", 9, None),
            (
                "(()=>{function descend(n){if(n===65536)throw Error('finite recursion ceiling reached');try{descend(n+1)}catch(error){if(!(error instanceof RangeError))throw error;__savedExit(9);globalThis.__continued=true}}descend(0)})()",
                9,
                None,
            ),
            (
                "__savedExit(5, Error('listener-original'))",
                5,
                Some("listener-original"),
            ),
            (
                "__savedExit(5, Error('process.exit(9)'))",
                5,
                Some("process.exit(9)"),
            ),
            (
                "__savedExit(5, new (class ProcessExitError extends Error {})('listener-name'))",
                5,
                Some("listener-name"),
            ),
            (
                "__savedExit(5, {get message(){throw Error('diagnostic-getter')}})",
                5,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
            (
                "__savedExit(5, {get message(){__savedTerminal(Error('reentry'));return 'BAD'}})",
                5,
                Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE"),
            ),
        ] {
            let mut session_isolate = crate::owned_microtasks::SessionIsolate::new(if restored {
                let blob = crate::snapshot::create_snapshot("globalThis.__savedExit=__agentOsRequestProcessExit;globalThis.__savedTerminal=__agentOsReportTerminalCallback;delete globalThis.__agentOsRequestProcessExit;delete globalThis.__agentOsReportTerminalCallback;").expect("snapshot captures native exit");
                crate::snapshot::create_isolate_from_snapshot(blob, None)
            } else {
                crate::isolate::create_isolate(None)
            });
            let isolate = &mut *session_isolate;
            isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
            let context = crate::owned_microtasks::create_root(isolate).expect("owned test context");
            // Repeat on the same isolate so restored stack policy and the new
            // call's Promise delivery are exercised after a terminal unwind.
            for cycle in 0..2 {
            {
                let _call = begin_call(isolate, cycle * 2 + 1);
                {
                let scope = &mut v8::HandleScope::new(&mut *isolate);
                let context = v8::Local::new(scope, &context);
                let scope = &mut v8::ContextScope::new(scope, context);
                if !restored {
                    install(scope).expect("native terminal hooks installed");
                }
                let code = format!("globalThis.__savedExit ??= globalThis.__agentOsRequestProcessExit;globalThis.__savedTerminal ??= globalThis.__agentOsReportTerminalCallback;globalThis.__continued=false;globalThis.__caught=false;globalThis.__finally=false;globalThis.__microtask=false;Promise.resolve().then(()=>{{__microtask=true}});try{{{source};__continued=true}}catch{{__caught=true}}finally{{__finally=true}}");
                let code = v8::String::new(scope, &code).unwrap();
                let script = v8::Script::compile(scope, code, None).unwrap();
                let _immediate_value = script.run(scope);
                assert!(pending(scope), "{restored}: {source}");
                assert!(
                    is_process_exit(scope),
                    "explicit exit remains typed on diagnostic failure"
                );
                assert_eq!(
                    is_silent_process_exit(scope),
                    expected_diagnostic.is_none(),
                    "real diagnostics are never silent exits"
                );
                let (code, diagnostic) = result(scope).expect("native exit recorded before unwind");
                assert_eq!(code, expected_code);
                match expected_diagnostic {
                    Some("ERR_AGENTOS_CALLBACK_TERMINAL_STATE") => {
                        assert_eq!(diagnostic.code.as_deref(), expected_diagnostic)
                    }
                    Some(message) => assert_eq!(diagnostic.message, message),
                    None => assert_eq!(diagnostic.error_type, "ProcessExitError"),
                }
                }
                // Match the production owner: scopes unwind, queued failed-call
                // jobs are discarded, then the owned stop is canceled.
                crate::owned_microtasks::discard_failed_call(isolate);
                isolate.cancel_terminate_execution();
                acknowledge_stop(isolate);
                let scope = &mut v8::HandleScope::new(&mut *isolate);
                let context = v8::Local::new(scope, &context);
                let scope = &mut v8::ContextScope::new(scope, context);
                crate::owned_microtasks::checkpoint(scope);
                let global = scope.get_current_context().global(scope);
                for name in ["__continued", "__caught", "__finally", "__microtask"] {
                    let key = v8::String::new(scope, name).unwrap();
                    assert!(
                        global.get(scope, key.into()).unwrap().is_false(),
                        "{restored}: {source}: {name}"
                    );
                }
            }
            assert!(!pending(isolate), "call guard clears native exit");
            assert!(result(isolate).is_none());
            let _next_call = begin_call(isolate, cycle * 2 + 2);
            assert!(!is_process_exit(isolate), "next call never inherits exit");
            let scope = &mut v8::HandleScope::new(&mut *isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let code = v8::String::new(
                scope,
                "Promise.resolve().then(()=>{globalThis.__newCall=true})",
            )
            .unwrap();
            let script = v8::Script::compile(scope, code, None).unwrap();
            assert!(script.run(scope).is_some());
            crate::owned_microtasks::checkpoint(scope);
            let global = scope.get_current_context().global(scope);
            for (name, expected) in [("__newCall", true), ("__microtask", false)] {
                let key = v8::String::new(scope, name).unwrap();
                assert_eq!(
                    global.get(scope, key.into()).unwrap().is_true(),
                    expected,
                    "next call executes only its own queued work"
                );
            }
            }
        }
    }
}
