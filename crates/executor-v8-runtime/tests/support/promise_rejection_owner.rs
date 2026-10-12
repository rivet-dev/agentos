fn rejection_owner_controls() {
    rejection_owner_policy_restoration_controls();
    // Actual native V8 rejection-owner controls; kept in this serialized
    // test because independent V8 test threads may race platform lifecycle.
    for (script, expected) in [
        (r#"
            globalThis.ownerEvents=[];globalThis.reason={original:true};globalThis.promise=undefined;
            globalThis._dispatchPromiseRejection=(event,reason,promise,frame)=>{
                ownerEvents.push([event,reason===globalThis.reason,promise===globalThis.promise,frame]);
                return 'continue';
            };
            __agentOsSetAsyncContext('rejection-frame');
            globalThis.promise=Promise.reject(globalThis.reason);
            __agentOsSetAsyncContext(undefined);
        "#, r#"[["unhandledRejection",true,true,"rejection-frame"]]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.second=undefined;
            globalThis._dispatchPromiseRejection=(event,reason)=>{
                ownerEvents.push(event+':'+reason);
                if(reason==='first')second.catch(()=>ownerEvents.push('caught-second'));
                return 'continue';
            };
            Promise.reject('first');globalThis.second=Promise.reject('second');void 0;
        "#, r#"["unhandledRejection:first","unhandledRejection:second","caught-second"]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.old=undefined;
            globalThis._dispatchPromiseRejection=(event,reason)=>{
                ownerEvents.push(event+':'+reason);
                if(reason==='old')Promise.resolve().then(()=>{
                    Promise.reject('new');old.catch(()=>ownerEvents.push('caught-old'));
                });
                return 'continue';
            };
            globalThis.old=Promise.reject('old');void 0;
        "#, r#"["unhandledRejection:old","caught-old","rejectionHandled:undefined","unhandledRejection:new"]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.a=undefined;globalThis.b=undefined;
            globalThis._dispatchPromiseRejection=(event,reason,promise)=>{
                if(event==='rejectionHandled'){
                    const name=promise===a?'a':'b';ownerEvents.push('H:'+name);
                    if(name==='a')b.catch(()=>ownerEvents.push('C:b'));
                }else{
                    ownerEvents.push('U:'+reason);
                    if(reason==='b')Promise.resolve().then(()=>{
                        Promise.reject('new');a.catch(()=>ownerEvents.push('C:a'));
                    });
                }
                return 'continue';
            };
            globalThis.a=Promise.reject('a');globalThis.b=Promise.reject('b');void 0;
        "#, r#"["U:a","U:b","C:a","H:a","H:b","U:new","C:b"]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.first=undefined;globalThis.second=undefined;globalThis.newest=undefined;
            globalThis._dispatchPromiseRejection=(event,reason,promise)=>{
                if(event==='rejectionHandled'){
                    ownerEvents.push('H:'+(promise===first?'first':promise===second?'second':'new'));
                    return 'continue';
                }
                ownerEvents.push('U:'+reason);
                if(reason==='first'){newest=Promise.reject('new');return 'discardUnhandledSnapshot'}
                if(reason==='new')Promise.resolve().then(()=>{
                    first.catch(()=>ownerEvents.push('C:first'));
                    second.catch(()=>ownerEvents.push('C:second'));
                    newest.catch(()=>ownerEvents.push('C:new'));
                });
                return 'continue';
            };
            globalThis.first=Promise.reject('first');globalThis.second=Promise.reject('second');void 0;
        "#, r#"["U:first","U:new","C:first","C:second","C:new","H:first","H:new"]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.a=undefined;globalThis.b=undefined;
            globalThis._dispatchPromiseRejection=(event,reason,promise)=>{
                if(event==='rejectionHandled'){
                    const name=promise===a?'a':'b';ownerEvents.push('H:'+name);
                    if(name==='a'){
                        Promise.reject('new');Promise.resolve().then(()=>ownerEvents.push('MICRO'));
                        return 'yieldCheckpoint';
                    }
                }else{
                    ownerEvents.push('U:'+reason);
                    if(reason==='b')Promise.resolve().then(()=>{
                        a.catch(()=>ownerEvents.push('C:a'));b.catch(()=>ownerEvents.push('C:b'));
                    });
                }
                return 'continue';
            };
            globalThis.a=Promise.reject('a');globalThis.b=Promise.reject('b');void 0;
        "#, r#"["U:a","U:b","C:a","C:b","H:a","MICRO","H:b","U:new"]"#),
        (r#"
            globalThis.ownerEvents=[];globalThis.count=0;
            globalThis._dispatchPromiseRejection=()=>{count++;if(count===1024)ownerEvents.push(count);return 'continue'};
            for(let i=0;i<1024;i++)Promise.reject(i);void 0;
        "#, "[1024]"),
    ] {
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let previous_policy=isolate.get_microtasks_policy();
        let (code,error) = {
            let scope=&mut v8::HandleScope::new(&mut isolate);
            let ctx=v8::Local::new(scope,&context);
            let scope=&mut v8::ContextScope::new(scope,ctx);
            crate::async_context::install_async_context_globals(scope);
            execute_script(scope,"",script,&mut None)
        };
        assert_eq!(isolate.get_microtasks_policy(),previous_policy);
        assert_eq!(code,0,"{error:?}");assert!(error.is_none(),"{error:?}");
        assert_eq!(eval(&mut isolate,&context,"JSON.stringify(ownerEvents)"),expected);
    }
    // Promise metadata creation must not call indexed guest Array setters,
    // including marker writes made from inside V8's own rejection callback.
    for (rejections, expected_code, expected_reports) in [(1,0,1),(1025,1,0)] {
        let mut isolate=isolate::create_isolate(None);
        let context=isolate::create_context(&mut isolate);
        let script=format!(r#"
            globalThis.setterCalls=0;globalThis.reports=0;
            Object.defineProperty(Array.prototype,'0',{{configurable:true,set(){{setterCalls++;throw Error('indexed guest setter')}}}});
            Object.defineProperty(Array.prototype,'1',{{configurable:true,set(){{setterCalls++;throw Error('indexed guest setter')}}}});
            globalThis._dispatchPromiseRejection=()=>{{reports++;return 'continue'}};
            for(let i=0;i<{rejections};i++)Promise.reject(i);void 0;
        "#);
        let (code,error)={
            let scope=&mut v8::HandleScope::new(&mut isolate);
            let ctx=v8::Local::new(scope,&context);
            let scope=&mut v8::ContextScope::new(scope,ctx);
            execute_script(scope,"",&script,&mut None)
        };
        assert_eq!(isolate.get_microtasks_policy(),v8::MicrotasksPolicy::Auto);
        assert_eq!(code,expected_code,"{error:?}");
        if expected_code==1 { assert_eq!(error.unwrap().code.as_deref(),Some("ERR_AGENTOS_UNHANDLED_REJECTION_LIMIT")); }
        else { assert!(error.is_none(),"{error:?}"); }
        assert_eq!(eval(&mut isolate,&context,"delete Array.prototype[0];delete Array.prototype[1];String(setterCalls)+':'+String(reports)"),format!("0:{expected_reports}"));
    }
    // Recursive notification creates one rejection at a time, so queue
    // capacity alone cannot enforce the bounded checkpoint callback work.
    {
        let mut isolate=isolate::create_isolate(None);
        let context=isolate::create_context(&mut isolate);
        let (code,error)={
            let scope=&mut v8::HandleScope::new(&mut isolate);
            let ctx=v8::Local::new(scope,&context);
            let scope=&mut v8::ContextScope::new(scope,ctx);
            execute_script(scope,"",r#"
                globalThis._dispatchPromiseRejection=()=>{Promise.reject('recursive');return 'continue'};
                Promise.reject('first');
            "#,&mut None)
        };
        assert_eq!(isolate.get_microtasks_policy(),v8::MicrotasksPolicy::Auto);
        assert_eq!(code,1);assert_eq!(error.unwrap().code.as_deref(),Some("ERR_AGENTOS_UNHANDLED_REJECTION_LIMIT"));
        assert_eq!(eval(&mut isolate,&context,"42"),"42");
    }

}

fn nested_rejection_owner_checkpoint(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let previous=scope.get_microtasks_policy();
    let nested=take_unhandled_promise_rejection(scope);
    result.set_bool(nested.is_none() && previous==v8::MicrotasksPolicy::Explicit
        && scope.get_microtasks_policy()==previous);
}

fn rejection_owner_policy_restoration_controls() {
    for policy in [v8::MicrotasksPolicy::Auto,v8::MicrotasksPolicy::Explicit] {
        for (script,expected_status) in [
            ("globalThis._dispatchUncaughtException=()=>{if(!__nestedRejectionOwner())throw Error('entry callback changed policy');return true};throw Error('entry');",0),
            ("globalThis._dispatchUncaughtException=error=>{throw error};throw Error('entry');",1),
            ("globalThis._dispatchUncaughtException=()=>{throw {_isAgentOSFatalException:true,cause:Error('fatal')}};throw Error('entry');",7),
            ("globalThis._dispatchUncaughtException=()=>{throw {_isProcessExit:true,code:9}};throw Error('entry');",9),
            ("globalThis._dispatchPromiseRejection=()=>{if(!__nestedRejectionOwner())throw Error('nested drain changed owner');return 'continue'};Promise.reject('one');Promise.reject('two');void 0;",0),
            ("globalThis._dispatchPromiseRejection=()=>{throw Error('terminal-callback')};Promise.reject('one');void 0;",1),
            ("globalThis._dispatchPromiseRejection=()=>{throw Object.assign(Error('exit'),{_isProcessExit:true,code:9})};Promise.reject('one');void 0;",9),
            ("globalThis._dispatchPromiseRejection=()=>42;Promise.reject('one');void 0;",1),
            ("globalThis._dispatchPromiseRejection=()=>{Promise.reject('recursive');return 'continue'};Promise.reject('one');void 0;",1),
            ("Promise.reject('cancelled').catch(()=>{});void 0;",0),
        ] {
            let mut isolate=isolate::create_isolate(None);
            isolate.set_microtasks_policy(policy);
            let context=isolate::create_context(&mut isolate);
            let (code,error)={
                let scope=&mut v8::HandleScope::new(&mut isolate);
                let ctx=v8::Local::new(scope,&context);
                let scope=&mut v8::ContextScope::new(scope,ctx);
                let callback=v8::Function::new(scope,nested_rejection_owner_checkpoint).expect("nested checkpoint callback");
                let key=v8::String::new(scope,"__nestedRejectionOwner").expect("nested callback name");
                let global=scope.get_current_context().global(scope);
                assert_eq!(global.set(scope,key.into(),callback.into()),Some(true));
                execute_script(scope,"",script,&mut None)
            };
            assert_eq!(isolate.get_microtasks_policy(),policy,"{error:?}");
            assert_eq!(code,expected_status,"{error:?}");
        }
    }
}
