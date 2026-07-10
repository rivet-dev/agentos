use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const INERT_BINDING_BOOTSTRAP: &str = r#"
(function installAgentOSInertBindings(global) {
  const names = __BINDING_NAMES__;
  const records = new Map();
  function unsupported(feature) {
    const error = new Error(`${feature} is not supported by this AgentOS Node migration milestone`);
    error.code = 'ERR_AGENTOS_UNSUPPORTED';
    throw error;
  }
  function record(name) {
    if (records.has(name)) return records.get(name);
    let proxy;
    const callable = function inertBindingAction() { return proxy; };
    proxy = new Proxy(callable, {
      apply() { return proxy; },
      construct() { return proxy; },
      get(target, property) {
        if (property === Symbol.toPrimitive) return () => 0;
        if (property === 'toJSON') return () => Object.create(null);
        if (property === 'isMainThread' || property === 'ownsProcessState') return true;
        if (property === 'threadId' || property === 'environmentData') return 0;
        if (property === 'hasIntl' || property === 'hasInspector') return false;
        if (property === 'open' && (name === 'inspector' || name === 'profiler')) {
          return () => unsupported(name);
        }
        return proxy;
      },
    });
    records.set(name, proxy);
    return proxy;
  }
  for (const name of names) record(name);
  Object.defineProperty(global, '__agentOSGetInternalBinding', {
    configurable: false,
    enumerable: false,
    value(name) {
      if (!records.has(name)) {
        const error = new Error(`No such internal binding: ${name}`);
        error.code = 'ERR_UNKNOWN_INTERNAL_BINDING';
        throw error;
      }
      return records.get(name);
    },
  });
})(globalThis);
"#;

const PROCESS_BOOTSTRAP: &str = r#"
(function installAgentOSProcess(global) {
  const process = global.process && typeof global.process === 'object'
    ? global.process
    : Object.create(null);
  const defaults = {
    platform: 'linux',
    arch: 'wasm32',
    execPath: '/opt/agentos/bin/node',
    env: process.env || Object.create(null),
  };
  for (const [key, value] of Object.entries(defaults)) {
    if (process[key] !== undefined) continue;
    try { process[key] = value; } catch {}
  }
  const versions = process.versions && typeof process.versions === 'object'
    ? process.versions
    : Object.create(null);
  for (const [key, value] of Object.entries({ node: '24.15.0', openssl: '3.5.5' })) {
    try { versions[key] = value; } catch {}
  }
  if (process.versions === undefined) {
    try { process.versions = versions; } catch {}
  }
  Object.defineProperty(global, '__agentOSNodeProcessIdentity', {
    configurable: false,
    enumerable: false,
    value: Object.freeze({ platform: 'linux', arch: 'wasm32', node: '24.15.0', openssl: '3.5.5' }),
  });
  global.process = process;
})(globalThis);
"#;

fn collect_js(dir: &Path, base: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(dir).expect("read vendored Node source directory") {
        let path = entry.expect("read vendored Node source entry").path();
        if path.is_dir() {
            collect_js(&path, base, files);
        } else if path.extension().is_some_and(|ext| ext == "js") {
            let id = path
                .strip_prefix(base)
                .expect("vendored source below lib root")
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((id, path));
        }
    }
}

fn literal(value: &str) -> String {
    format!("{value:?}")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("vendor");
    let lib_dir = vendor_dir.join("lib");
    let manifest = vendor_dir.join("manifest.json");
    let binding_inventory = manifest_dir.join("bindings/inventory.json");
    let inert_loader = manifest_dir.join("adapter/inert-loader.js");
    if !manifest.is_file() || !lib_dir.is_dir() {
        panic!(
            "ERR_NODE_STDLIB_VENDOR_MISSING: run `node crates/node-stdlib/scripts/vendor-node.mjs --node-src <node-v24.15.0-checkout>`"
        );
    }

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", lib_dir.display());
    println!("cargo:rerun-if-changed={}", binding_inventory.display());
    println!("cargo:rerun-if-changed={}", inert_loader.display());

    let mut files = Vec::new();
    collect_js(&lib_dir, &lib_dir, &mut files);
    let deps_dir = vendor_dir.join("deps");
    let mut dependency_files = Vec::new();
    for dependency in ["acorn", "undici"] {
        let dependency_dir = deps_dir.join(dependency);
        collect_js(&dependency_dir, &deps_dir, &mut dependency_files);
    }
    files.extend(
        dependency_files
            .into_iter()
            .map(|(id, path)| (format!("internal/deps/{id}"), path)),
    );
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut generated = String::from("pub static BUILTIN_IDS: &[&str] = &[\n");
    for (id, _) in &files {
        generated.push_str(&format!("    {},\n", literal(id)));
    }
    generated.push_str(
        "];\n\npub fn builtin_source(id: &str) -> Option<&'static str> {\n    match id {\n",
    );
    for (id, path) in &files {
        generated.push_str(&format!(
            "        {} => Some(include_str!({})),\n",
            literal(id),
            literal(&path.to_string_lossy())
        ));
    }
    generated.push_str("        _ => None,\n    }\n}\n");

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("builtin_sources.rs");
    fs::write(out, generated).expect("write generated Node builtin source map");

    let inventory: serde_json::Value = serde_json::from_slice(
        &fs::read(&binding_inventory).expect("read reconciled Node binding inventory"),
    )
    .expect("parse reconciled Node binding inventory");
    let binding_names = inventory["pinned_node"]["names"]
        .as_array()
        .expect("pinned_node.names array");
    let binding_names_json = serde_json::to_string(binding_names).expect("serialize binding names");
    let inert_bootstrap = INERT_BINDING_BOOTSTRAP.replace("__BINDING_NAMES__", &binding_names_json);
    let mut sources = serde_json::Map::new();
    let mut public_ids = Vec::new();
    let mut source_bytes = 0usize;
    for (id, path) in &files {
        let source = fs::read_to_string(path).expect("read pinned Node builtin source");
        source_bytes += source.len();
        sources.insert(id.clone(), serde_json::Value::String(source));
        if !id.starts_with("internal/") && id != "quic" && id != "sqlite" {
            public_ids.push(id.as_str());
        }
    }
    let sources_json = serde_json::to_string(&sources).expect("serialize pinned Node sources");
    let loader_source = fs::read_to_string(&inert_loader).expect("read inert Node realm loader");
    let source_registry_bootstrap = format!("globalThis.__agentOSNodeSources = {sources_json};");
    let real_bootstrap = format!("{source_registry_bootstrap}\n{loader_source}");
    let public_ids_json = serde_json::to_string(&public_ids).expect("serialize public builtin ids");
    let binding_assets = format!(
        "pub static BINDING_IDS: &[&str] = &{binding_names_json};\n\
         pub static PUBLIC_BUILTIN_IDS: &[&str] = &{public_ids_json};\n\
         pub const NODE_BUILTIN_SOURCE_BYTES: usize = {source_bytes};\n\
         pub const INERT_BINDING_BOOTSTRAP_SOURCE: &str = {inert_bootstrap:?};\n\
         pub const PROCESS_BOOTSTRAP_SOURCE: &str = {PROCESS_BOOTSTRAP:?};\n\
         pub const NODE_SOURCE_REGISTRY_BOOTSTRAP_SOURCE: &str = {source_registry_bootstrap:?};\n\
         pub const REAL_STDLIB_RUNTIME_BOOTSTRAP_SOURCE: &str = {loader_source:?};\n\
         pub const REAL_STDLIB_BOOTSTRAP_SOURCE: &str = {real_bootstrap:?};\n"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("binding_assets.rs");
    fs::write(out, binding_assets).expect("write generated Node binding assets");
}
