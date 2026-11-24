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

    encode_data_fields_as_bytes(&out_dir.join("v1_generated.rs"));

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

/// The generator emits BARE `data` as a plain `Vec<u8>`, which serde encodes
/// and decodes one element at a time. `serde_bytes` writes the same BARE bytes
/// (length prefix + raw bytes) as one block copy, so mark every such field.
fn encode_data_fields_as_bytes(generated: &Path) {
    const ATTRIBUTE: &str = "    #[serde(with = \"serde_bytes\")]\n";
    let source = fs::read_to_string(generated)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", generated.display()));
    let mut output = String::with_capacity(source.len() + 1024);
    let mut previous = "";
    for line in source.lines() {
        let is_data_field = line.starts_with("    pub ")
            && (line.ends_with(": Vec<u8>,") || line.ends_with(": Option<Vec<u8>>,"));
        if is_data_field && previous != ATTRIBUTE.trim_end_matches('\n') {
            output.push_str(ATTRIBUTE);
        }
        output.push_str(line);
        output.push('\n');
        previous = line;
    }
    if output != source {
        fs::write(generated, output)
            .unwrap_or_else(|error| panic!("failed to write {}: {error}", generated.display()));
    }
}
