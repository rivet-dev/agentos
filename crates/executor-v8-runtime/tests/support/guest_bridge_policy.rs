use super::*;

fn eval(scope: &mut v8::HandleScope, code: &str) -> String {
    let source = v8::String::new(scope, code).unwrap();
    let result = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
    result.to_rust_string_lossy(scope)
}
fn check_history(scope: &mut v8::HandleScope, raised: bool) {
    let result = eval(
        scope,
        r#"(() => {
        const require = __agentOsGuestCreateRequire('/entry.js');
        const readline = require('readline');
        const {PassThrough} = require('stream');
        const input = new PassThrough(), output = new PassThrough();
        try {
            const rl = readline.createInterface({input,output,terminal:true,historySize:1025});
            rl.close(); return 'accepted';
        } catch (error) { return error.code; }
        finally { input.destroy(); output.destroy(); }
    })()"#,
    );
    assert_eq!(
        result,
        if raised {
            "accepted"
        } else {
            "ERR_OUT_OF_RANGE"
        }
    );
}

// Called from the existing consolidated V8 owner test; no parallel V8 harness.
pub(crate) fn owner_controls() {
    let bridge = crate::adapter_host::V8RuntimeHost::bridge_code();
    let defaults = GuestBridgeLimits::default();
    let policy = GuestBridgePolicy {
        vm_id: "bridge-policy-a".into(),
        limits: GuestBridgeLimits {
            max_readline_history_entries: 2048,
            ..Default::default()
        },
    };
    let mut session_pin = None;
    pin_session(&mut session_pin, Some(&policy)).unwrap();
    pin_session(&mut session_pin, Some(&policy)).unwrap();
    assert!(pin_session(&mut session_pin, None).is_err());
    let mut other = policy.clone();
    other.vm_id = "bridge-policy-b".into();
    assert!(pin_session(&mut session_pin, Some(&other)).is_err());
    other = policy.clone();
    other.limits.max_pending_response_bytes += 1;
    assert!(pin_session(&mut session_pin, Some(&other)).is_err());
    pin_session(&mut None, Some(&other)).unwrap(); // independent new session
    let mut legacy = None;
    pin_session(&mut legacy, None).unwrap();
    assert!(pin_session(&mut legacy, Some(&policy)).is_err());
    let effective = bridge_code_with_limits(bridge, &policy.limits).unwrap();
    let default_source = bridge_code_with_limits(bridge, &defaults).unwrap();
    assert_eq!(default_source, bridge);
    assert_eq!(
        crate::snapshot::snapshot_cache_key(&default_source, None),
        crate::snapshot::snapshot_cache_key(bridge, None)
    );
    assert_eq!(
        effective,
        bridge_code_with_limits(bridge, &policy.limits.clone()).unwrap()
    );
    assert_eq!(
        crate::snapshot::snapshot_cache_key(&effective, Some("userland")),
        crate::snapshot::snapshot_cache_key(
            &bridge_code_with_limits(bridge, &policy.limits.clone()).unwrap(),
            Some("userland")
        )
    );
    assert!(!effective.contains("bridge-policy-a"));
    let mut b = policy.limits.clone();
    b.max_pending_response_bytes += 1;
    let effective_b = bridge_code_with_limits(bridge, &b).unwrap();
    assert_ne!(
        crate::snapshot::snapshot_cache_key(&effective, None),
        crate::snapshot::snapshot_cache_key(&effective_b, None)
    );

    // Actual cold bridge setup: legacy None chooses Rust defaults, not a JS fallback.
    for supplied in [None, Some(&policy)] {
        let source = if supplied.is_some() {
            &effective
        } else {
            &default_source
        };
        let mut isolate = crate::isolate::create_isolate(None);
        let context = crate::isolate::create_context(&mut isolate);
        {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            crate::bridge::register_stub_bridge_fns(
                scope,
                crate::session::sync_bridge_fns(),
                crate::session::async_bridge_fns(),
            );
            eval(
                scope,
                r#"Object.defineProperty(globalThis,'_processConfig',{value:Object.freeze({cwd:'/fixture',env:{},timing_mitigation:'off',frozen_time_ms:null,high_resolution_time:false}),writable:false,configurable:false});Object.defineProperty(globalThis,'_osConfig',{value:Object.freeze({homedir:'/fixture',tmpdir:'/tmp',platform:'linux',arch:'x64'}),writable:false,configurable:false})"#,
            );
            prepare(scope, supplied, false, source, &mut None).unwrap();
            assert_eq!(eval(scope, "_processConfig.cwd"), "/fixture");
            assert_eq!(eval(scope, "_osConfig.homedir"), "/fixture");
            check_history(scope, supplied.is_some());
            assert_eq!(
                eval(scope, "typeof __agentOSBridgeLimitInstaller"),
                "undefined"
            );
            prepare(scope, supplied, false, source, &mut None).unwrap(); // no duplicate bridge evaluation
            let mut changed = policy.clone();
            changed.vm_id = "bridge-policy-b".into();
            assert!(prepare(scope, Some(&changed), false, source, &mut None)
                .unwrap_err()
                .message
                .contains("owner"));
            if supplied.is_some() {
                changed = policy.clone();
                changed.limits.max_readline_history_entries = 4096;
                assert!(prepare(scope, Some(&changed), false, source, &mut None).is_err());
                assert!(prepare(scope, None, false, source, &mut None).is_err());
                check_history(scope, true);
            }
        }
        drop(context);
        crate::isolate::drop_isolate(Some(isolate));
    }
    // A watchdog-style termination during cold placeholder initialization is
    // a typed error, never an unwrap panic; cancellation permits a clean retry.
    {
        let mut isolate = crate::isolate::create_isolate(None);
        let context = crate::isolate::create_context(&mut isolate);
        isolate.terminate_execution();
        {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            assert!(prepare(scope, None, false, bridge, &mut None).is_err());
        }
        isolate.cancel_terminate_execution();
        {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            crate::bridge::register_stub_bridge_fns(
                scope,
                crate::session::sync_bridge_fns(),
                crate::session::async_bridge_fns(),
            );
            prepare(scope, None, false, bridge, &mut None).unwrap();
            check_history(scope, false);
        }
        drop(context);
        crate::isolate::drop_isolate(Some(isolate));
    }
    for supplied in [None, Some(&policy)] {
        let source = if supplied.is_some() {
            &effective
        } else {
            &default_source
        };
        let requested = if supplied.is_some() { 1025 } else { 30 };
        // Legitimate top-level HTTP/readline construction sees effective policy,
        // BEFORE VM restoration. Raised1025 would fail under the old default.
        let userland = format!(
            r#"(() => {{
            if(typeof __agentOSBridgeLimitInstaller !== 'undefined') throw Error('installer visible to snapshot userland');
            if(typeof __agentOSBridgeLimits !== 'undefined') throw Error('policy placeholder visible to snapshot userland');
            const require=__agentOsGuestCreateRequire('/entry.js');
            const {{PassThrough}}=require('stream');
            globalThis.__prebuiltInput=new PassThrough();globalThis.__prebuiltOutput=new PassThrough();
            globalThis.__prebuiltHistory=require('readline').createInterface({{input:__prebuiltInput,output:__prebuiltOutput,terminal:true,historySize:{requested},history:Array.from({{length:{requested}}},(_,i)=>String(i))}});
            globalThis.__prebuiltAgent=new (require('http').Agent)({{maxSockets:1}});
            globalThis.__fakeInstallerCalls=0;
            globalThis.__agentOSBridgeLimitInstaller=()=>{{__fakeInstallerCalls++;}};
            const savedFreeze=Object.freeze;globalThis.__restoreFreeze=()=>{{Object.freeze=savedFreeze;}};
            Object.freeze=()=>{{throw Error('untrusted replacement freeze invoked');}};
        }})();"#
        );
        let blob = crate::snapshot::create_snapshot_with_userland(source, &userland).unwrap();
        // New isolates + TWO contexts each: serialized V8 data retrieval, not a
        // surviving Global or assumption about Private::for_api name identity.
        for _ in 0..2 {
            let mut isolate = crate::snapshot::create_isolate_from_snapshot(blob.clone(), None);
            for owner in ["bridge-policy-a", "bridge-policy-b"] {
                let alternate = GuestBridgePolicy {
                    vm_id: owner.into(),
                    limits: supplied.map(|p| p.limits.clone()).unwrap_or_default(),
                };
                let context = crate::isolate::create_context(&mut isolate);
                {
                    let scope = &mut v8::HandleScope::new(&mut isolate);
                    let context = v8::Local::new(scope, &context);
                    let scope = &mut v8::ContextScope::new(scope, context);
                    // First context also exercises legacy default(None); other
                    // new contexts have explicit owner identity with same policy.
                    let selected = if supplied.is_none() && owner == "bridge-policy-a" {
                        None
                    } else {
                        Some(&alternate)
                    };
                    prepare(scope, selected, true, source, &mut None).unwrap();
                    assert_eq!(eval(scope, "String(__fakeInstallerCalls)"), "0");
                    assert_eq!(
                        eval(scope, "String(__prebuiltHistory.history.length)"),
                        requested.to_string()
                    );
                    assert_eq!(eval(scope, "String(__prebuiltAgent.maxSockets)"), "1");
                    eval(scope,"__restoreFreeze();__prebuiltHistory.close();__prebuiltInput.destroy();__prebuiltOutput.destroy();__prebuiltAgent.destroy()");
                    check_history(scope, supplied.is_some());
                    prepare(scope, selected, true, source, &mut None).unwrap();
                }
                drop(context);
            }
            crate::isolate::drop_isolate(Some(isolate));
        }
        // A blob baked with different effective values cannot be reused merely
        // because its bridge/source owner string was mistakenly passed through.
        let mut isolate = crate::snapshot::create_isolate_from_snapshot(blob, None);
        let context = crate::isolate::create_context(&mut isolate);
        {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut wrong = policy.clone();
            wrong.limits.max_pending_response_bytes += 7;
            assert!(prepare(scope, Some(&wrong), true, source, &mut None)
                .unwrap_err()
                .message
                .contains("snapshot bridge policy differs"));
        }
        drop(context);
        crate::isolate::drop_isolate(Some(isolate));
    }
}
