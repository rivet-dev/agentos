use std::{env, fs, path::PathBuf};

// Stage the base filesystem fixture into OUT_DIR. In-tree builds use the
// canonical AgentOS runtime-core fixture from the current workspace; the
// published crate falls back to the vendored `assets/base-filesystem.json` copy.
fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));

    println!("cargo:rerun-if-changed=build.rs");

    let workspace_fixtures = [
        manifest_dir.join("../../packages/runtime-core/fixtures/base-filesystem.json"),
        manifest_dir.join("../../packages/core/fixtures/base-filesystem.json"),
    ];
    let vendored = manifest_dir.join("assets/base-filesystem.json");
    let src = workspace_fixtures
        .into_iter()
        .find(|fixture| fixture.exists())
        .unwrap_or(vendored);

    println!("cargo:rerun-if-changed={}", src.display());

    let dest = out_dir.join("base-filesystem.json");
    fs::copy(&src, &dest).unwrap_or_else(|error| {
        panic!(
            "failed to stage base-filesystem.json from {} to {}: {}",
            src.display(),
            dest.display(),
            error
        )
    });
}
