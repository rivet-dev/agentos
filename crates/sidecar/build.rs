use std::{env, fs, path::PathBuf};

// Stage the base filesystem fixture into OUT_DIR. In-tree builds use the
// canonical `packages/core/fixtures/base-filesystem.json`; the published crate
// falls back to the vendored `assets/base-filesystem.json` copy.
fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));

    println!("cargo:rerun-if-changed=build.rs");

    let workspace_fixture = manifest_dir.join("../../packages/core/fixtures/base-filesystem.json");
    let vendored = manifest_dir.join("assets/base-filesystem.json");
    let src = if workspace_fixture.exists() {
        workspace_fixture
    } else {
        vendored
    };

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

    let workspace_prompt =
        manifest_dir.join("../../packages/core/fixtures/AGENTOS_SYSTEM_PROMPT.md");
    let vendored_prompt = manifest_dir.join("assets/AGENTOS_SYSTEM_PROMPT.md");
    let prompt_src = if workspace_prompt.exists() {
        workspace_prompt
    } else {
        vendored_prompt
    };

    println!("cargo:rerun-if-changed={}", prompt_src.display());

    let prompt_dest = out_dir.join("AGENTOS_SYSTEM_PROMPT.md");
    fs::copy(&prompt_src, &prompt_dest).unwrap_or_else(|error| {
        panic!(
            "failed to stage AGENTOS_SYSTEM_PROMPT.md from {} to {}: {}",
            prompt_src.display(),
            prompt_dest.display(),
            error
        )
    });
}
