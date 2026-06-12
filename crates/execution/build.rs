use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "build_support.rs"]
mod v8_bridge_build;

/// Large Pyodide runtime assets are excluded from the published crate (see the
/// `exclude` list in Cargo.toml) to keep it under the registry size limit.
/// During in-tree builds they are copied from `assets/pyodide/`; when building
/// the published crate (where they are absent) they are downloaded from the
/// release CDN at the crate version.
const EXTERNALIZED_PYODIDE_ASSETS: &[&str] = &[
    "pyodide.asm.wasm",
    "pyodide.asm.js",
    "python_stdlib.zip",
    "numpy-2.2.5-cp313-cp313-pyodide_2025_0_wasm32.whl",
    "pandas-2.3.3-cp313-cp313-pyodide_2025_0_wasm32.whl",
];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));

    println!("cargo:rerun-if-changed=build.rs");
    v8_bridge_build::build_v8_bridge(&manifest_dir, &out_dir);
    stage_pyodide_assets(&manifest_dir, &out_dir);
}

fn stage_pyodide_assets(manifest_dir: &Path, out_dir: &Path) {
    let pyodide_out = out_dir.join("pyodide");
    fs::create_dir_all(&pyodide_out).unwrap_or_else(|error| {
        panic!(
            "failed to create pyodide staging dir {}: {}",
            pyodide_out.display(),
            error
        )
    });

    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION must be set");
    let base_url = format!("https://releases.rivet.dev/agent-os/{version}/pyodide");

    for asset in EXTERNALIZED_PYODIDE_ASSETS {
        let in_tree = manifest_dir.join("assets/pyodide").join(asset);
        let dest = pyodide_out.join(asset);
        println!("cargo:rerun-if-changed={}", in_tree.display());

        if dest.exists() {
            continue;
        }

        if in_tree.exists() {
            fs::copy(&in_tree, &dest).unwrap_or_else(|error| {
                panic!(
                    "failed to copy pyodide asset {} to {}: {}",
                    in_tree.display(),
                    dest.display(),
                    error
                )
            });
        } else {
            let url = format!("{base_url}/{asset}");
            download_asset(&url, &dest);
        }
    }
}

fn download_asset(url: &str, dest: &Path) {
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to spawn curl to download externalized pyodide asset {url}: {error}. \
                 curl is required to build the published agent-os-execution crate."
            )
        });

    if !status.success() {
        panic!(
            "failed to download externalized pyodide asset from {url} (curl exited with {status}). \
             The release CDN must serve this asset before the crate is published."
        );
    }
}
