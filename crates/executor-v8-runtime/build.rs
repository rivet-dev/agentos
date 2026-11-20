use std::env;
use std::fs;
use std::path::{Path, PathBuf};

mod build_assets;
mod build_bridge;

// This is the exact rusty_v8 binding plus the product's backported V8 tree.
// An upstream package with the same Rust version can have different V8 headers.
const RUSTY_V8_REV: &str = "548adfd54db42c670e0f3fd42211c3af109c0a73";
const V8_TREE: &str = "91306fc87da850ec8b222f0030b37d2da90a6dea";
const ICU_REV: &str = "2a51536dfb3b448bdf3dae70c782ea2879c90904";
const HEADER_SHA256: &str = "3ab78300ff42414d7706891bf101ae9fa18ca1b5eaec27d38ff56a08bad1b6ac";
const ICU_SHA256: &str = "e1ed8f674bb8449731b79cae956042da59f9ecbd37c8d3f86586a22b4836e5b2";

fn required_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn v8_source_root() -> PathBuf {
    println!("cargo:rerun-if-env-changed=AGENTOS_RUSTY_V8_SOURCE");
    println!("cargo:rerun-if-env-changed=RUSTY_V8_ARCHIVE");
    if let Some(root) = env::var_os("AGENTOS_RUSTY_V8_SOURCE") {
        return root.into();
    }
    if let Some(archive) = env::var_os("RUSTY_V8_ARCHIVE").map(PathBuf::from) {
        if archive.is_file() {
            return archive
                .parent()
                .expect("archive parent directory")
                .to_owned();
        }
    }
    panic!("set AGENTOS_RUSTY_V8_SOURCE to the prepared agentOS V8 source or pass the local matched bundle archive via RUSTY_V8_ARCHIVE");
}

fn header_paths(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
    {
        let path = entry.expect("V8 header directory entry").path();
        if path.is_dir() {
            header_paths(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "h") {
            files.push(path);
        }
    }
}

fn verify_v8_headers(include: &Path) {
    use sha2::{Digest, Sha256};
    let mut files = Vec::new();
    header_paths(include, &mut files);
    files.sort_by_cached_key(|path| {
        path.strip_prefix(include)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    });
    let mut digest = Sha256::new();
    for file in files {
        let relative = file
            .strip_prefix(include)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(Sha256::digest(required_file(&file)));
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        HEADER_SHA256,
        "public V8 headers in {} do not match engine tree {V8_TREE}",
        include.display()
    );
}

fn build_v8_thread_support(manifest_dir: &Path, include: &Path, gn_include: &Path) {
    let source = manifest_dir.join("src/v8_thread_support.cc");
    let serializer = manifest_dir.join("src/v8_bounded_serializer.cc");
    let allocator = manifest_dir.join("src/v8_array_buffer_allocator.cc");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", serializer.display());
    println!("cargo:rerun-if-changed={}", allocator.display());
    cc::Build::new()
        .cpp(true)
        .std("c++23")
        .flag_if_supported("-fno-rtti")
        .warnings(false)
        .define("V8_GN_HEADER", None)
        .include(gn_include)
        .include(include)
        .file(source)
        .file(serializer)
        .file(allocator)
        .compile("agentos_v8_thread_support");
}

/// Fingerprint of everything that shapes a snapshot blob besides its key: the
/// V8 version, the target, and the sources that define external references,
/// bridge stub functions, and snapshot creation. Persisted blobs are only
/// loaded by a build with the same fingerprint.
fn emit_snapshot_fingerprint(manifest_dir: &Path, gn_header: &[u8]) {
    use sha2::{Digest, Sha256};
    fn collect(path: &Path, files: &mut Vec<PathBuf>) {
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error))
                .map(|entry| entry.expect("directory entry").path())
                .collect();
            entries.sort();
            for entry in entries {
                collect(&entry, files);
            }
        } else if path.is_file() {
            files.push(path.to_path_buf());
        }
    }
    let mut files = Vec::new();
    for root in [
        manifest_dir.join("src"),
        manifest_dir.join("../bridge/src"),
        manifest_dir.join("../bridge/bridge-contract.json"),
    ] {
        println!("cargo:rerun-if-changed={}", root.display());
        collect(&root, &mut files);
    }
    let mut hasher = Sha256::new();
    for pin in [RUSTY_V8_REV, V8_TREE, ICU_REV, HEADER_SHA256, ICU_SHA256] {
        hasher.update(pin.as_bytes());
    }
    hasher.update(gn_header);
    hasher.update(env::var("TARGET").unwrap_or_default().as_bytes());
    for file in files {
        let relative = file.strip_prefix(manifest_dir).unwrap_or(&file);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(
            fs::read(&file)
                .unwrap_or_else(|error| panic!("failed to read {}: {}", file.display(), error)),
        );
    }
    let fingerprint: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=AGENTOS_V8_RUNTIME_FINGERPRINT={fingerprint}");
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));

    println!("cargo:rerun-if-changed=build.rs");

    build_bridge::build_v8_bridge(&manifest_dir, &out_dir);
    build_assets::stage_executor_assets();

    let root = v8_source_root();
    let include = root.join("v8/include");
    println!("cargo:rerun-if-changed={}", include.display());
    verify_v8_headers(&include);
    println!("cargo:rerun-if-env-changed=AGENTOS_RUSTY_V8_GN_INCLUDE");
    let gn_include = env::var_os("AGENTOS_RUSTY_V8_GN_INCLUDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("generated"));
    let gn_header = gn_include.join("v8-gn.h");
    println!("cargo:rerun-if-changed={}", gn_header.display());
    emit_snapshot_fingerprint(&manifest_dir, &required_file(&gn_header));
    build_v8_thread_support(&manifest_dir, &include, &gn_include);

    let icu_data = root.join("third_party/icu/common/icudtl.dat");
    println!("cargo:rerun-if-changed={}", icu_data.display());
    let icu_bytes = required_file(&icu_data);
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(&icu_bytes)),
        ICU_SHA256,
        "ICU common data in {} does not match ICU revision {ICU_REV}",
        icu_data.display()
    );
    fs::write(out_dir.join("icudtl.dat"), icu_bytes).expect("stage pinned ICU common data");
}
