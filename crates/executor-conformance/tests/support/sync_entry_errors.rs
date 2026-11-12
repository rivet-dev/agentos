// Actual production execution/session controls. Fresh-process public parity is
// separate from the same-JavascriptExecution retained-caller assertions below.
fn javascript_sync_entry_errors_and_retained_outcomes() {
    let temp = tempdir().expect("create sync entry fixture");
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("vm-sync-entry-owner"),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let mut mismatches = Vec::new();
    for (name, source, expected_status, expected_stdout, error_text) in [
        (
            r###"fatal handler does not checkpoint queued microtasks"###,
            r###"queueMicrotask(()=>console.log('BAD_MICRO'));process.on('uncaughtException',()=>{console.log('HANDLER');throw Error('handler-fatal')});throw Error('entry');"###,
            7,
            r###"HANDLER
"###,
            r###"handler-fatal"###,
        ),
        (
            r###"exiting handler does not checkpoint queued microtasks"###,
            r###"queueMicrotask(()=>console.log('BAD_MICRO'));process.on('uncaughtException',()=>{console.log('HANDLER');process.exit(9)});throw Error('entry');"###,
            9,
            r###"HANDLER
"###,
            r###""###,
        ),
        (
            "CJS handler precedes already queued microtasks",
            r#"queueMicrotask(()=>console.log('BEFORE_MICRO'));process.on('uncaughtException',()=>{console.log('HANDLED');queueMicrotask(()=>console.log('AFTER_MICRO'));});throw Error('entry-error');"#,
            0,
            "HANDLED\nBEFORE_MICRO\nAFTER_MICRO\n",
            "",
        ),
        (
            r###"CJS handler observes unwound context while prior timer retains its frame"###,
            r###"const { AsyncLocalStorage } = require('node:async_hooks'); const als = new AsyncLocalStorage(); process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + als.getStore() + ':' + origin)); als.run('entry-frame', () => { setTimeout(() => console.log('TIMER:' + als.getStore()), 25); throw Error('entry-error'); });"###,
            0,
            r###"HANDLED:undefined:uncaughtException
TIMER:entry-frame
"###,
            r###""###,
        ),
        (
            r###"consuming CJS handler retains explicit process exitCode"###,
            r###"process.on('uncaughtException', () => { process.exitCode = 23; console.log('HANDLED'); }); throw Error('entry-error');"###,
            23,
            r###"HANDLED
"###,
            r###""###,
        ),
        (
            r###"CJS preserves original object and origin"###,
            r###"const original = { marker: 'original' }; process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === original) + ':' + origin)); throw original; console.log('UNREACHABLE');"###,
            0,
            r###"HANDLED:true:uncaughtException
"###,
            r###""###,
        ),
        (
            r###"CJS preserves thrown null"###,
            r###"process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === null) + ':' + origin)); throw null;"###,
            0,
            r###"HANDLED:true:uncaughtException
"###,
            r###""###,
        ),
        (
            r###"CJS preserves thrown undefined"###,
            r###"process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === undefined) + ':' + origin)); throw undefined;"###,
            0,
            r###"HANDLED:true:uncaughtException
"###,
            r###""###,
        ),
        (
            r###"consumed CJS error drains a pre-existing referenced timer"###,
            r###"setTimeout(() => console.log('TIMER'), 25); process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + error.message + ':' + origin)); throw Error('entry-error');"###,
            0,
            r###"HANDLED:entry-error:uncaughtException
TIMER
"###,
            r###""###,
        ),
        (
            r###"throwing CJS error handler exits seven without draining timers"###,
            r###"setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); process.on('uncaughtException', () => { console.log('HANDLER'); throw Error('handler-fatal'); }); throw Error('entry-error');"###,
            7,
            r###"HANDLER
"###,
            r###"handler-fatal"###,
        ),
        (
            r###"CJS error handler explicit exit retains nine"###,
            r###"setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); process.on('uncaughtException', () => { console.log('HANDLER'); process.exit(9); }); throw Error('entry-error');"###,
            9,
            r###"HANDLER
"###,
            r###""###,
        ),
        (
            r###"unhandled CJS error stays terminal"###,
            r###"setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); throw Error('entry-unhandled');"###,
            1,
            r###""###,
            r###"entry-unhandled"###,
        ),
        (
            r###"local catch bypasses process error handler"###,
            r###"process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); try { throw Error('local'); } catch (error) { console.log('LOCAL:' + error.message); }"###,
            0,
            r###"LOCAL:local
"###,
            r###""###,
        ),
        (
            r###"explicit process exit bypasses process error handler"###,
            r###"process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); process.exit(9);"###,
            9,
            r###""###,
            r###""###,
        ),
        (
            r###"syntax failure does not run entrypoint handlers"###,
            r###"process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); const = ;"###,
            1,
            r###""###,
            r###"SyntaxError"###,
        ),
    ] {
        let execution = engine
            .start_execution(StartJavascriptExecutionRequest {
                limits: JavascriptExecutionLimits {
                    wall_clock_limit_ms: Some(2000),
                    ..Default::default()
                },
                argv0: None,
                guest_runtime: Default::default(),
                vm_id: String::from("vm-sync-entry-owner"),
                context_id: context.context_id.clone(),
                argv: vec![String::from("-e"), source.into()],
                env: BTreeMap::new(),
                cwd: temp.path().to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some(source.into()),
            })
            .expect("start bounded CJS execution");
        let result = execution.wait().expect("wait for bounded CJS execution");
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stdout.len() + stderr.len() <= 16_384);
        if result.exit_code != expected_status
            || stdout != expected_stdout
            || (error_text.is_empty() && !stderr.is_empty())
            || (!error_text.is_empty() && !stderr.contains(error_text))
        {
            mismatches.push(format!(
                "{name}: status={} stdout={stdout:?} stderr={stderr:?}",
                result.exit_code
            ));
        }
    }

    let cases = [
        (
            "retained-bootstrap",
            r#"
globalThis.__syncRetained = { original: { marker: 'same-object' }, calls: 0, timerCalls: 0 };
console.log('READY');
"#,
            0,
            "READY\n",
            "",
        ),
        (
            "retained-consumed-original",
            r#"
if (!globalThis.__syncRetained) throw Error('context was not retained');
process.on('uncaughtException', (error, origin) => {
    __syncRetained.calls++;
    console.log('HANDLED:' + (error === __syncRetained.original) + ':' + origin);
});
// Retained calls release leftover handles after result settlement; unlike a
// standalone Node process, a handled throw must not turn this into timer drain.
setTimeout(() => { __syncRetained.timerCalls++; console.log('BAD_TIMER'); }, 25);
throw __syncRetained.original;
"#,
            0,
            "HANDLED:true:uncaughtException\n",
            "agentOS: the context call completed with work still pending; released setTimeout x1. Released timers never run; pending I/O on released handles fails with ERR_AGENTOS_CONTEXT_HANDLE_RELEASED.\n",
        ),
        (
            "retained-unhandled-structured-error",
            r#"
if (__syncRetained.calls !== 1 || __syncRetained.timerCalls !== 0 || _getPendingTimerCount() !== 0) throw Error('retained ownership changed');
process.removeAllListeners('uncaughtException');
throw Error('retained-terminal-error');
"#,
            1,
            "",
            "retained-terminal-error",
        ),
        (
            "retained-next-clean",
            r#"
if (__syncRetained.calls !== 1 || __syncRetained.timerCalls !== 0 || _getPendingTimerCount() !== 0) throw Error('retained ownership changed');
queueMicrotask(() => console.log('RETAINED_CLEAN'));
"#,
            0,
            "RETAINED_CLEAN\n",
            "",
        ),
    ];
    let mut resident: Option<JavascriptExecution> = None;
    for (name, code, expected_status, expected_stdout, error_text) in cases {
        let source = format!("{{\n{code}\n}}\nvoid 0;");
        if let Some(execution) = resident.as_mut() {
            execution
                .execute_retained(source, String::from("/[sync-entry-retained]"), false)
                .expect("resume same production retained caller");
        } else {
            resident = Some(
                engine
                    .start_execution(StartJavascriptExecutionRequest {
                        limits: JavascriptExecutionLimits {
                            wall_clock_limit_ms: Some(2000),
                            ..Default::default()
                        },
                        argv0: None,
                        guest_runtime: Default::default(),
                        vm_id: String::from("vm-sync-entry-owner"),
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
                    .expect("start retained CJS owner"),
            );
        }
        let execution = resident.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("retained CJS control exceeded2s");
            match execution
                .poll_event_blocking(remaining)
                .expect("poll retained CJS caller")
            {
                Some(JavascriptExecutionEvent::Stdout(chunk)) => {
                    assert!(stdout.len() + chunk.len() <= 16_384);
                    stdout.extend(chunk)
                }
                Some(JavascriptExecutionEvent::Stderr(chunk)) => {
                    assert!(stderr.len() + chunk.len() <= 16_384);
                    stderr.extend(chunk)
                }
                Some(JavascriptExecutionEvent::Exited(code)) => break code,
                Some(JavascriptExecutionEvent::SyncRpcRequest(request)) => {
                    panic!("unexpected CJS fixture RPC {}", request.method)
                }
                Some(
                    JavascriptExecutionEvent::SignalState { .. }
                    | JavascriptExecutionEvent::LimitExceeded(_),
                ) => {}
                None => panic!("retained CJS control exceeded2s"),
            }
        };
        let stdout = String::from_utf8_lossy(&stdout);
        let stderr = String::from_utf8_lossy(&stderr);
        if name == "retained-consumed-original" {
            assert_eq!(stderr, error_text, "retained timer release diagnostic");
        }
        if status != expected_status
            || stdout != expected_stdout
            || (error_text.is_empty() && !stderr.is_empty())
            || (!error_text.is_empty() && !stderr.contains(error_text))
        {
            mismatches.push(format!(
                "{name}: status={status} stdout={stdout:?} stderr={stderr:?}"
            ));
        }
    }
    drop(resident);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
