use std::env;
use std::path::PathBuf;

#[path = "../build-support/v8_bridge_build.rs"]
mod v8_bridge_build;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));

    println!("cargo:rerun-if-changed=build.rs");
    v8_bridge_build::build_v8_bridge(&manifest_dir, &out_dir);
}
