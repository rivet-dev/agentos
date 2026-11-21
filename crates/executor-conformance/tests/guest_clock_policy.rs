mod support;

use std::collections::BTreeMap;

use agentos_executor_conformance::{
    CreateJavascriptContextRequest, StartJavascriptExecutionRequest,
};
use tempfile::tempdir;

#[test]
fn javascript_execution_high_resolution_time_default_off_keeps_coarse_clock() {
    let temp = tempdir().expect("create temp dir");
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("vm-js-high-res-off"),
        bootstrap_module: None,
        compile_cache_root: None,
    });

    let execution = engine
        .start_execution(StartJavascriptExecutionRequest {
            limits: Default::default(),
            argv0: None,
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js-high-res-off"),
            context_id: context.context_id,
            argv: vec![String::from("./entry.mjs")],
            env: BTreeMap::new(),
            cwd: temp.path().to_path_buf(),
            wasm_module_bytes: None,
            inline_code: Some(String::from(
                r#"
import vm from "node:vm";

if (typeof __agentOsHrNowUs !== "undefined") {
  throw new Error("high-resolution host clock exists without opt-in");
}
for (let attempt = 0; attempt < 20; attempt++) {
  const now = process.hrtime.bigint();
  if (now % 1000000n !== 0n) {
    throw new Error("process.hrtime.bigint was not millisecond aligned: " + now);
  }
}
const childClockIsCoarse = `
  typeof __agentOsHrNowUs === "undefined" &&
  Number.isInteger(Date.now()) &&
  (typeof performance === "undefined" || Number.isInteger(performance.now()))
`;
if (!vm.runInNewContext(childClockIsCoarse)) {
  throw new Error("node:vm child exposes sub-millisecond time without opt-in");
}
"#,
            )),
        })
        .expect("start JavaScript execution");

    let result = execution.wait().expect("wait for JavaScript execution");
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.exit_code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
}
