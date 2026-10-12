// Real bridge handles plus an unref'd timer prove host retirement before Exited.
fn javascript_terminal_callback_cleanup_lifecycle() {
    use agentos_driver_tokio::accounting::ResourceClass;
    let temp = tempdir().unwrap();
    let mut engine = support::javascript_engine();
    let resources = engine.runtime_context().unwrap().resources().clone();
    for (mode, expected) in [
        ("settled", 0),
        ("unhandled", 1),
        ("fatal", 7),
        ("exit", 9),
        ("timeout", 1),
    ] {
        let timers = resources.usage(ResourceClass::Timers).used;
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: "vm-cleanup-owner".into(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let source = format!(
            r#"
const mode={mode:?};
globalThis.cleanupSentinel={{alive:true}};
setTimeout(()=>console.log('BAD_UNREF_TIMER'),1000).unref();
_registerHandle('first','fixture-first',()=>{{console.log('FIRST_CLOSE');queueMicrotask(()=>console.log('BAD_CLOSER_MICROTASK'));process.nextTick(()=>console.log('BAD_CLOSER_TICK'));if(mode==='timeout'){{while(true){{}}}}throw Error('cleanup-interrupted')}});
_registerHandle('second','fixture-second',()=>console.log('BAD_SECOND_CLOSE'));
if(mode==='fatal')process.on('uncaughtException',()=>{{throw Error('original-fatal')}});
if(mode==='exit')process.on('uncaughtException',()=>process.exit(9));
if(!['settled','timeout'].includes(mode))queueMicrotask(()=>{{throw Error('original-callback')}});
void 0;
"#
        );
        let mut execution = engine
            .start_execution(StartJavascriptExecutionRequest {
                limits: JavascriptExecutionLimits {
                    wall_clock_limit_ms: Some(if mode == "timeout" { 100 } else { 2000 }),
                    ..Default::default()
                },
                argv0: None,
                guest_runtime: Default::default(),
                vm_id: "vm-cleanup-owner".into(),
                context_id: context.context_id.clone(),
                argv: vec!["-e".into(), source.clone()],
                env: BTreeMap::from([("AGENTOS_RETAIN_LANGUAGE_CONTEXT".into(), "1".into())]),
                cwd: temp.path().into(),
                wasm_module_bytes: None,
                inline_code: Some(source),
            })
            .unwrap();
        let (status, stdout, stderr, wall_limit) = terminal_lifecycle_result(&mut execution);
        assert_eq!(status, expected, "{mode}: {stdout:?} {stderr:?}");
        assert!(stdout.contains("FIRST_CLOSE"), "{stdout:?}");
        assert!(!stdout.contains("BAD_"), "{stdout:?}");
        if mode == "timeout" {
            assert!(
                wall_limit,
                "exact wall-clock outcome must survive cleanup interruption"
            );
        } else {
            assert!(stderr.contains("cleanup-interrupted"), "{stderr:?}");
        }
        if mode == "unhandled" {
            assert!(stderr.contains("original-callback"));
        }
        if mode == "fatal" {
            assert!(stderr.contains("original-fatal"));
        }
        assert_eq!(
            resources.usage(ResourceClass::Timers).used,
            timers,
            "unref timer retired BEFORE Exited"
        );
        let error = execution
            .execute_retained(
                "console.log('BAD_REUSED')".into(),
                "/[invalidated]".into(),
                false,
            )
            .expect_err("no poisoned reuse or silent fresh realm");
        assert!(
            error
                .to_string()
                .contains("ERR_AGENTOS_CONTEXT_INVALIDATED"),
            "{error}"
        );
        drop(execution);
        assert!(engine.dispose_context(&context.context_id));
        assert!(!engine.dispose_context(&context.context_id));
        engine.dispose_vm("vm-cleanup-owner");
        assert_eq!(resources.usage(ResourceClass::Timers).used, timers);
    }
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: "vm-cleanup-recovery".into(),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let execution = engine
        .start_execution(StartJavascriptExecutionRequest {
            limits: JavascriptExecutionLimits {
                wall_clock_limit_ms: Some(2000),
                ..Default::default()
            },
            argv0: None,
            guest_runtime: Default::default(),
            vm_id: "vm-cleanup-recovery".into(),
            context_id: context.context_id,
            argv: vec!["-e".into(), "console.log('NEW_CLEAN')".into()],
            env: BTreeMap::new(),
            cwd: temp.path().into(),
            wasm_module_bytes: None,
            inline_code: Some("console.log('NEW_CLEAN')".into()),
        })
        .unwrap();
    let result = execution.wait().unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(String::from_utf8_lossy(&result.stdout), "NEW_CLEAN\n");
    engine.dispose_vm("vm-cleanup-recovery");
}
fn terminal_lifecycle_result(execution: &mut JavascriptExecution) -> (i32, String, String, bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut wall_limit = false;
    loop {
        match execution
            .poll_event_blocking(
                deadline
                    .checked_duration_since(Instant::now())
                    .expect("bounded cleanup wait"),
            )
            .unwrap()
        {
            Some(JavascriptExecutionEvent::Stdout(b)) => stdout.extend(b),
            Some(JavascriptExecutionEvent::Stderr(b)) => stderr.extend(b),
            Some(JavascriptExecutionEvent::Exited(c)) => {
                return (
                    c,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                    wall_limit,
                )
            }
            Some(JavascriptExecutionEvent::LimitExceeded(limit)) => {
                wall_limit =
                    limit == agentos_executor_v8_runtime::ipc_binary::ExecutionLimit::WallClock
            }
            Some(JavascriptExecutionEvent::SyncRpcRequest(r)) => {
                panic!("unexpected RPC {}", r.method)
            }
            Some(_) => {}
            None => panic!("cleanup exceeded2s"),
        }
        assert!(stdout.len() + stderr.len() <= 16384);
    }
}

fn javascript_terminal_callback_early_setup_cleanup() {
    let temp = tempdir().unwrap();
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: "vm-early-terminal".into(),
        bootstrap_module: None,
        compile_cache_root: None,
    });
    let mut execution=engine.start_execution(StartJavascriptExecutionRequest {
        limits:JavascriptExecutionLimits {wall_clock_limit_ms:Some(2000),..Default::default()},argv0:None,
        guest_runtime:GuestRuntimeConfig {prelude_script:Some("globalThis.__earlyTerminal=true;queueMicrotask(()=>{throw Error('early-callback')});throw Error('early-setup');".into()),..Default::default()},
        vm_id:"vm-early-terminal".into(),context_id:context.context_id.clone(),argv:vec!["-e".into(),"console.log('BAD_USER_BODY')".into()],
        env:BTreeMap::from([("AGENTOS_RETAIN_LANGUAGE_CONTEXT".into(),"1".into())]),cwd:temp.path().into(),wasm_module_bytes:None,inline_code:Some("console.log('BAD_USER_BODY')".into()),
    }).unwrap();
    let (status, stdout, _, _) = terminal_lifecycle_result(&mut execution);
    assert_eq!(status, 1);
    assert!(!stdout.contains("BAD_"));
    execution.execute_retained("if(!globalThis.__earlyTerminal||_getPendingTimerCount()!==0)throw Error('early cleanup lost retained state');queueMicrotask(()=>console.log('EARLY_CLEAN'));".into(),"/[early-next]".into(),false).unwrap();
    let (status, stdout, stderr, _) = terminal_lifecycle_result(&mut execution);
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(stdout, "EARLY_CLEAN\n");
    drop(execution);
    engine.dispose_vm("vm-early-terminal");
}
