use std::path::PathBuf;

#[path = "src/actions/contract_surface.rs"]
mod contract_surface;

fn main() {
    println!("cargo:rerun-if-changed=src/actions/contract_surface.rs");

    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let generated_path = manifest_dir
        .join("../..")
        .join(contract_surface::GENERATED_ACTOR_ACTIONS_PATH);
    let parent = generated_path
        .parent()
        .expect("generated actor actions path must have a parent");

    std::fs::create_dir_all(parent)
        .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
    std::fs::write(&generated_path, contract_surface::render_actor_actions_ts())
        .unwrap_or_else(|error| panic!("write {}: {error}", generated_path.display()));
}
