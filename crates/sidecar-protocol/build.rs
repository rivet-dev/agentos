use std::{env, fs, path::Path, path::PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let source_schema = manifest_dir
        .join("protocol")
        .join("agentos_sidecar_v1.bare");

    let schema_dir = out_dir.join("protocol-schema");
    fs::create_dir_all(&schema_dir).expect("failed to create generated protocol schema dir");
    let schema_changed = copy_if_changed(&source_schema, &schema_dir.join("v1.bare"));
    let generated_missing =
        !out_dir.join("combined_imports.rs").exists() || !out_dir.join("v1_generated.rs").exists();
    if schema_changed || generated_missing {
        let cfg = vbare_compiler::Config::default();
        vbare_compiler::process_schemas_with_config(&schema_dir, &cfg)
            .expect("failed to generate sidecar protocol from BARE schema");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", source_schema.display());
}

fn copy_if_changed(source: &Path, destination: &Path) -> bool {
    let contents = fs::read(source)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source.display()));
    if fs::read(destination).is_ok_and(|existing| existing == contents) {
        return false;
    }
    fs::write(destination, contents)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", destination.display()));
    true
}
