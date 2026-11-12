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
