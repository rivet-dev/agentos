use std::time::Instant;

use agentos_v8_runtime::snapshot::{bridge_code_for_flavor, create_snapshot, StdlibFlavor};

fn snapshot_metric(flavor: StdlibFlavor) -> serde_json::Value {
    let bridge = bridge_code_for_flavor(flavor, "globalThis.__snapshotMetric = true;");
    let started = Instant::now();
    let blob = create_snapshot(&bridge).expect("create stdlib metric snapshot");
    serde_json::json!({
        "blobBytes": blob.len(),
        "buildMs": started.elapsed().as_secs_f64() * 1000.0,
    })
}

fn main() {
    agentos_v8_runtime::isolate::init_v8_platform();
    let mut isolate = agentos_v8_runtime::isolate::create_isolate(None);
    let scope = &mut v8::HandleScope::new(&mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let mut lazy_compile = Vec::new();
    for id in agentos_node_stdlib::PUBLIC_BUILTIN_IDS {
        let source = agentos_node_stdlib::builtin_source(id).expect("public builtin source");
        let wrapped = format!(
            "(function(exports,require,module,process,internalBinding,primordials){{\n{source}\n}})"
        );
        let started = Instant::now();
        let source = v8::String::new(scope, &wrapped).expect("create builtin source string");
        let script = v8::Script::compile(scope, source, None)
            .unwrap_or_else(|| panic!("compile pinned Node builtin {id}"));
        let _ = script;
        lazy_compile.push(serde_json::json!({
            "builtin": id,
            "compileMs": started.elapsed().as_secs_f64() * 1000.0,
        }));
    }

    let output = serde_json::json!({
        "schema": 1,
        "node": agentos_node_stdlib::NODE_VERSION,
        "sourceBytes": agentos_node_stdlib::NODE_BUILTIN_SOURCE_BYTES,
        "publicBuiltins": agentos_node_stdlib::PUBLIC_BUILTIN_IDS.len(),
        "snapshots": {
            "legacy": snapshot_metric(StdlibFlavor::Legacy),
            "real": snapshot_metric(StdlibFlavor::Real),
        },
        "lazyCompile": lazy_compile,
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
