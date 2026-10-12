mod support;
use agentos_executor_conformance::{
    CreateJavascriptContextRequest, GuestRuntimeConfig, StartJavascriptExecutionRequest,
};
use std::collections::BTreeMap;

#[test]
fn typed_bridge_policy_reaches_actual_execute_and_keeps_other_vms_defaulted() {
    let temp = tempfile::tempdir().unwrap();
    let mut engine = support::javascript_engine();
    for (index, raised) in [false, true, false].into_iter().enumerate() {
        let vm_id = format!("bridge-budget-{index}");
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: vm_id.clone(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let mut guest = GuestRuntimeConfig::default();
        if raised {
            guest.bridge_limits.max_readline_history_entries = 2048;
        }
        guest.snapshot_userland_code = Some("if(typeof __agentOSBridgeLimitInstaller!=='undefined'||typeof __agentOSBridgeLimits!=='undefined')throw Error('native installer leaked to snapshot userland');globalThis.__bridgePolicySnapshot=true;".into());
        let source = format!(
            r#"
import assert from 'node:assert/strict';
import readline from 'node:readline';
import {{PassThrough}} from 'node:stream';
assert.equal(globalThis.__bridgePolicySnapshot,true);
let counterfeitCalls=0;
globalThis.__agentOSBridgeLimitInstaller=()=>{{counterfeitCalls++;}};
globalThis.__agentOSBridgeLimits={{maxReadlineHistoryEntries:9999}};
const input=new PassThrough(),output=new PassThrough();
try {{
    const create=options=>readline.createInterface({{input,output,terminal:true,...options}});
    const zero=create({{historySize:0,history:['seed']}});assert.equal(zero.history.length,0);zero.close();
    const ordinary=create({{history:Array.from({{length:31}},(_,i)=>String(i))}});assert.equal(ordinary.history.length,30);ordinary.close();
    if ({raised}) {{const rl=create({{historySize:1025}});rl.close();}}
    else assert.throws(()=>create({{historySize:1025}}),{{code:'ERR_OUT_OF_RANGE'}});
    assert.equal(counterfeitCalls,0);
    console.log('bridge-policy-execute-complete');
}} finally {{input.destroy();output.destroy();}}
"#
        );
        let execution = engine
            .start_execution(StartJavascriptExecutionRequest {
                limits: Default::default(),
                argv0: None,
                guest_runtime: guest,
                vm_id,
                context_id: context.context_id.clone(),
                argv: vec!["./entry.mjs".into()],
                env: BTreeMap::new(),
                cwd: temp.path().to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some(source),
            })
            .unwrap();
        let result = execution.wait().unwrap();
        assert_eq!(
            result.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            "bridge-policy-execute-complete"
        );
        assert!(engine.dispose_context(&context.context_id));
    }
}
