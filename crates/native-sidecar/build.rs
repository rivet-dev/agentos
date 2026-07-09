use std::{
    env, fs,
    path::{Path, PathBuf},
};

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

    stage_ca_bundle(&manifest_dir, &out_dir);
}

/// Stage the Mozilla CA bundle into OUT_DIR so it can be embedded via
/// `include_bytes!` and seeded into the VM at `/etc/ssl/certs/ca-certificates.crt`.
///
/// The ~230 KB PEM blob is never committed. It is fetched into
/// `assets/ca-certificates.crt` by `make -C toolchain/c ca-certificates`. When
/// the asset is absent (fresh checkout, or `cargo publish` verification with no
/// network) we stage an empty placeholder — the sidecar then simply skips
/// seeding the bundle, matching the Pyodide "asset unavailable" pattern. Runtime
/// TLS still works via `--cacert`/`SSL_CERT_FILE` overrides in that case.
fn stage_ca_bundle(manifest_dir: &Path, out_dir: &Path) {
    let asset = manifest_dir.join("assets/ca-certificates.crt");
    println!("cargo:rerun-if-changed={}", asset.display());

    let dest = out_dir.join("ca-certificates.crt");
    if asset.exists() {
        fs::copy(&asset, &dest).unwrap_or_else(|error| {
            panic!(
                "failed to stage ca-certificates.crt from {} to {}: {}",
                asset.display(),
                dest.display(),
                error
            )
        });
    } else {
        // Empty placeholder keeps the include_bytes! of the OUT_DIR copy valid.
        fs::write(&dest, b"").unwrap_or_else(|error| {
            panic!(
                "failed to stage empty ca-certificates.crt placeholder to {}: {}",
                dest.display(),
                error
            )
        });
    }
}
