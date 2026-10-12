// Default immediate retained calls already belong to the existing rejection-owner
// sequence. Cover the additional nondefault and deferred-start policy paths here.
fn javascript_retained_execution_preserves_initial_bridge_policy() {
    let temp = tempdir().expect("create retained policy fixture");
    let mut engine = support::javascript_engine();
    for (index, (raised, deferred)) in [(true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        let vm_id = format!("retained-bridge-policy-{index}");
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: vm_id.clone(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let mut guest_runtime = GuestRuntimeConfig::default();
        if raised {
            guest_runtime.bridge_limits.max_readline_history_entries = 2048;
        }
        let source = |turn| {
            format!(
                r#"{{
const assert = require('node:assert/strict');
const readline = require('node:readline');
const {{ PassThrough }} = require('node:stream');
const input = new PassThrough(), output = new PassThrough();
try {{
    globalThis.__retainedPolicyTurns = (globalThis.__retainedPolicyTurns || 0) + 1;
    assert.equal(globalThis.__retainedPolicyTurns, {turn} + 1);
    const create = historySize => readline.createInterface({{ input, output, terminal: true, historySize }});
    if ({raised}) {{
        const accepted = create(1025); accepted.close();
        assert.throws(() => create(2049), {{ code: 'ERR_OUT_OF_RANGE' }});
    }} else {{
        assert.throws(() => create(1025), {{ code: 'ERR_OUT_OF_RANGE' }});
    }}
    console.log('retained-policy-turn-{turn}');
}} finally {{ input.destroy(); output.destroy(); }}
}}
void 0;"#
            )
        };
        let initial = source(0);
        let request = StartJavascriptExecutionRequest {
            limits: JavascriptExecutionLimits {
                wall_clock_limit_ms: Some(5000),
                ..Default::default()
            },
            argv0: None,
            guest_runtime,
            vm_id,
            context_id: context.context_id.clone(),
            argv: vec![String::from("-e"), initial.clone()],
            env: BTreeMap::from([(
                String::from("AGENTOS_RETAIN_LANGUAGE_CONTEXT"),
                String::from("1"),
            )]),
            cwd: temp.path().to_path_buf(),
            wasm_module_bytes: None,
            inline_code: Some(initial),
        };
        let mut execution = if deferred {
            engine.prepare_execution(request)
        } else {
            engine.start_execution(request)
        }
        .expect("create actual retained execution");
        if deferred {
            assert!(execution.is_prepared_for_start());
            assert_eq!(
                execution.poll_event_blocking(Duration::ZERO).unwrap(),
                None,
                "deferred preparation must not enqueue guest code"
            );
            execution.start_prepared().expect("start prepared policy");
            assert!(!execution.is_prepared_for_start());
        }
        for turn in 0..3 {
            if turn != 0 {
                execution
                    .execute_retained(source(turn), "/[retained-policy]".into(), false)
                    .expect("resupply the original retained policy");
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let status = loop {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .expect("retained policy operation exceeded5s");
                match execution.poll_event_blocking(remaining).unwrap() {
                    Some(JavascriptExecutionEvent::Stdout(chunk)) => {
                        assert!(stdout.len() + chunk.len() <= 16384);
                        stdout.extend(chunk);
                    }
                    Some(JavascriptExecutionEvent::Stderr(chunk)) => {
                        assert!(stderr.len() + chunk.len() <= 16384);
                        stderr.extend(chunk);
                    }
                    Some(JavascriptExecutionEvent::Exited(code)) => break code,
                    Some(JavascriptExecutionEvent::SignalState { .. }) => {}
                    Some(event) => panic!("unexpected retained policy event: {event:?}"),
                    None => panic!("retained policy operation exceeded5s"),
                }
            };
            assert_eq!(
                status,
                0,
                "raised={raised} deferred={deferred} turn={turn}: {}",
                String::from_utf8_lossy(&stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&stdout).trim(),
                format!("retained-policy-turn-{turn}")
            );
        }
        drop(execution);
        assert!(engine.dispose_context(&context.context_id));
    }
}

// The public wrapper's private identity must survive actual retained Execute calls.
fn javascript_retained_vm_identity_survives_import_routes() {
    let temp = tempdir().expect("create retained vm identity fixture");
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("retained-vm-identity"),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let cases = [
        (
            r#"{
const cjs = require('node:vm');
const box = { value: 10 };
globalThis.__retainedVmIdentity = { cjs, first: cjs.createContext(box), box };
if (!cjs.isContext(__retainedVmIdentity.first)) throw Error('initial context identity lost');
if (new cjs.Script('value').runInContext(__retainedVmIdentity.first) !== 10) throw Error('initial Script failed');
console.log('retained-vm-identity-0');
}
void 0;"#,
            false,
        ),
        (
            r#"import vm, { Script } from 'node:vm';
const state = globalThis.__retainedVmIdentity;
if (!state || !vm.isContext(state.first) || !state.cjs.isContext(state.first)) throw Error('retained cross-import identity lost');
if (vm.runInContext('++value', state.first) !== 11) throw Error('ESM could not resume CJS context');
if (new Script('value + 1').runInContext(state.first) !== 12) throw Error('ESM Script could not use CJS context');
state.second = vm.createContext({ value: 20 });
state.esmScript = Script;
console.log('retained-vm-identity-1');"#,
            true,
        ),
        (
            r#"{
const state = globalThis.__retainedVmIdentity;
const cjs = require('node:vm');
if (!state || !cjs.isContext(state.first) || !cjs.isContext(state.second)) throw Error('identity did not survive the next retained call');
if (cjs.createContext(state.box) !== state.first) throw Error('same object was rewrapped');
if (cjs.runInContext('++value', state.second) !== 21) throw Error('CJS could not resume ESM context');
if (new state.esmScript('value').runInContext(state.first) !== 11) throw Error('saved ESM Script lost identity');
delete globalThis.__retainedVmIdentity;
console.log('retained-vm-identity-2');
}
void 0;"#,
            false,
        ),
    ];
    let mut resident: Option<JavascriptExecution> = None;
    for (turn, (code, module)) in cases.into_iter().enumerate() {
        let source = String::from(code);
        if let Some(execution) = resident.as_mut() {
            execution
                .execute_retained(
                    source,
                    format!("/[retained-vm-identity-{turn}].mjs"),
                    module,
                )
                .expect("resume actual retained vm identity execution");
        } else {
            resident = Some(
                engine
                    .start_execution(StartJavascriptExecutionRequest {
                        limits: JavascriptExecutionLimits {
                            wall_clock_limit_ms: Some(5000),
                            ..Default::default()
                        },
                        argv0: None,
                        guest_runtime: Default::default(),
                        vm_id: String::from("retained-vm-identity"),
                        context_id: context.context_id.clone(),
                        argv: vec![String::from("-e"), source.clone()],
                        env: BTreeMap::from([(
                            String::from("AGENTOS_RETAIN_LANGUAGE_CONTEXT"),
                            String::from("1"),
                        )]),
                        cwd: temp.path().to_path_buf(),
                        wasm_module_bytes: None,
                        inline_code: Some(source),
                    })
                    .expect("start actual retained vm identity execution"),
            );
        }
        let execution = resident.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("retained vm identity operation exceeded5s");
            match execution
                .poll_event_blocking(remaining)
                .expect("poll retained vm identity")
            {
                Some(JavascriptExecutionEvent::Stdout(chunk)) => {
                    assert!(stdout.len() + chunk.len() <= 16384);
                    stdout.extend(chunk);
                }
                Some(JavascriptExecutionEvent::Stderr(chunk)) => {
                    assert!(stderr.len() + chunk.len() <= 16384);
                    stderr.extend(chunk);
                }
                Some(JavascriptExecutionEvent::Exited(code)) => break code,
                Some(JavascriptExecutionEvent::SignalState { .. }) => {}
                Some(JavascriptExecutionEvent::SyncRpcRequest(request)) => {
                    assert!(
                        execution
                            .try_service_standalone_module_sync_rpc(&request)
                            .expect("service retained vm identity module sync RPC"),
                        "unexpected retained vm identity RPC: {}",
                        request.method
                    );
                }
                Some(event) => panic!("unexpected retained vm identity event: {event:?}"),
                None => panic!("retained vm identity operation exceeded5s"),
            }
        };
        assert_eq!(
            status,
            0,
            "turn={turn}: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            stderr.is_empty(),
            "turn={turn}: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&stdout).trim(),
            format!("retained-vm-identity-{turn}")
        );
    }
    drop(resident);
    assert!(engine.dispose_context(&context.context_id));
}
