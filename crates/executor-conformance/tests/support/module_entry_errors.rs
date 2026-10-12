// Owned module evaluation failures, including delayed completion and the same
// production retained caller. Ordinary Promise reporting stays independent.
fn javascript_module_entry_errors() {
    let temp = tempdir().expect("create module entry fixture");
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("vm-module-entry-owner"),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let mut mismatches = Vec::new();
    for (name, source, expected_status, expected_stdout, error_text) in [
        (
            r###"module owner avoids species"###,
            r###"Object.defineProperty(Promise,Symbol.species,{get(){throw Error('BAD_SPECIES')}});const original={};process.on('uncaughtException',(e,o)=>console.log('HANDLED:'+(e===original)+':'+o));throw original;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"overflowed evaluation transfers only its own debt"###,
            r###"let reports=0;process.on('unhandledRejection',()=>{if(++reports===1024)console.log('UR:'+reports)});process.on('uncaughtException',(e,o)=>console.log('ENTRY:'+reports+':'+e.message+':'+o));for(let i=0;i<1024;i++)Promise.reject(Error('ordinary'));throw Error('module');"###,
            0,
            r###"ENTRY:0:module:unhandledRejection
UR:1024
"###,
            r###""###,
        ),
        (
            "handler-created rejection stays owned",
            r###"process.on('unhandledRejection',e=>console.log('UR:'+e.message));process.on('uncaughtException',()=>{console.log('ENTRY');Promise.reject(Error('spawned'))});throw Error('entry');"###,
            0,
            "ENTRY\nUR:spawned\n",
            "",
        ),
        (
            r###"entry original and rejection origin"###,
            r###"const original={marker:1};process.on('unhandledRejection',()=>console.log('BAD_UR'));process.on('uncaughtException',(e,o)=>console.log('HANDLED:'+ (e===original)+':'+o));throw original;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"delayed TLA original"###,
            r###"const original={marker:1};process.on('unhandledRejection',()=>console.log('BAD_UR'));process.on('uncaughtException',(e,o)=>console.log('HANDLED:'+(e===original)+':'+o));await new Promise(r=>setTimeout(r,1));throw original;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"undefined reason"###,
            r###"process.on('uncaughtException',(e,o)=>console.log('HANDLED:'+(e===undefined)+':'+o));throw undefined;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"null reason"###,
            r###"process.on('uncaughtException',(e,o)=>console.log('HANDLED:'+(e===null)+':'+o));await Promise.reject(null);"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"consumed drains timer"###,
            r###"setTimeout(()=>console.log('TIMER'),25);process.on('uncaughtException',()=>console.log('HANDLED'));throw Error('entry');"###,
            0,
            r###"HANDLED
TIMER
"###,
            r###""###,
        ),
        (
            r###"handler fatal"###,
            r###"process.on('uncaughtException',()=>{console.log('HANDLER');throw Error('handler-fatal')});throw Error('entry');"###,
            7,
            r###"HANDLER
"###,
            r###"handler-fatal"###,
        ),
        (
            r###"handler exit"###,
            r###"process.on('uncaughtException',()=>{console.log('HANDLER');process.exit(9)});throw Error('entry');"###,
            9,
            r###"HANDLER
"###,
            r###""###,
        ),
        (
            r###"exitCode retained"###,
            r###"process.on('uncaughtException',()=>{process.exitCode=23;console.log('HANDLED')});throw Error('entry');"###,
            23,
            r###"HANDLED
"###,
            r###""###,
        ),
        (
            r###"ordinary rejection listener remains"###,
            r###"process.on('unhandledRejection',e=>console.log('UR:'+e.message));Promise.reject(Error('ordinary'));await new Promise(r=>setTimeout(r,25));"###,
            0,
            r###"UR:ordinary
"###,
            r###""###,
        ),
        (
            r###"local catch"###,
            r###"process.on('uncaughtException',()=>console.log('BAD'));try{await Promise.reject(Error('local'))}catch(e){console.log('LOCAL:'+e.message)}"###,
            0,
            r###"LOCAL:local
"###,
            r###""###,
        ),
        (
            r###"unhandled entry"###,
            r###"throw Error('entry-unhandled')"###,
            1,
            r###""###,
            r###"entry-unhandled"###,
        ),
        (
            r###"syntax remains compiler error"###,
            r###"process.on('uncaughtException',()=>console.log('BAD')); const = ;"###,
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
                vm_id: String::from("vm-module-entry-owner"),
                context_id: context.context_id.clone(),
                argv: vec![String::from("./entry.mjs")],
                env: BTreeMap::new(),
                cwd: temp.path().to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some(source.into()),
            })
            .expect("start bounded module execution");
        let result = execution.wait().expect("wait for bounded module execution");
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
            r###"module-retained-bootstrap"###,
            r###"globalThis.__moduleOwner={original:{marker:1},calls:0,timers:0};console.log('READY');"###,
            0,
            r###"READY
"###,
            r###""###,
        ),
        (
            r###"module-retained-consumed"###,
            r###"process.on('unhandledRejection',()=>console.log('BAD_UR'));process.on('uncaughtException',(e,o)=>{__moduleOwner.calls++;console.log('HANDLED:'+(e===__moduleOwner.original)+':'+o)});setTimeout(()=>{__moduleOwner.timers++;console.log('BAD_TIMER')},25);throw __moduleOwner.original;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            "agentOS: the context call completed with work still pending; released setTimeout x1. Released timers never run; pending I/O on released handles fails with ERR_AGENTOS_CONTEXT_HANDLE_RELEASED.\n",
        ),
        (
            r###"module-retained-delayed"###,
            r###"if(__moduleOwner.calls!==1||__moduleOwner.timers!==0||_getPendingTimerCount()!==0)throw Error('retained owner changed');await new Promise(r=>setTimeout(r,1));throw __moduleOwner.original;"###,
            0,
            r###"HANDLED:true:unhandledRejection
"###,
            r###""###,
        ),
        (
            r###"module-retained-terminal"###,
            r###"process.removeAllListeners('uncaughtException');throw Error('module-retained-terminal');"###,
            1,
            r###""###,
            r###"module-retained-terminal"###,
        ),
        (
            r###"module-retained-next-clean"###,
            r###"if(__moduleOwner.calls!==2||__moduleOwner.timers!==0||_getPendingTimerCount()!==0)throw Error('retained owner changed');console.log('MODULE_RETAINED_CLEAN');"###,
            0,
            r###"MODULE_RETAINED_CLEAN
"###,
            r###""###,
        ),
    ];
    let mut resident: Option<JavascriptExecution> = None;
    for (name, code, expected_status, expected_stdout, error_text) in cases {
        let source = code.to_string();
        if let Some(execution) = resident.as_mut() {
            execution
                .execute_retained(source, format!("/{name}.mjs"), true)
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
                        vm_id: String::from("vm-module-entry-owner"),
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
                    .expect("start retained module owner"),
            );
        }
        let execution = resident.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("retained module control exceeded2s");
            match execution
                .poll_event_blocking(remaining)
                .expect("poll retained module caller")
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
                    panic!("unexpected module fixture RPC {}", request.method)
                }
                Some(
                    JavascriptExecutionEvent::SignalState { .. }
                    | JavascriptExecutionEvent::LimitExceeded(_),
                ) => {}
                None => panic!("retained module control exceeded2s"),
            }
        };
        let stdout = String::from_utf8_lossy(&stdout);
        let stderr = String::from_utf8_lossy(&stderr);
        if name == "module-retained-consumed" {
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
