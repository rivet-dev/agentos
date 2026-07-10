use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildManifest {
    schema: u32,
    architecture: Architecture,
    source_date_epoch: u64,
    wasm: FileIdentity,
    abi_manifest: FileIdentity,
    vendor_manifest: FileIdentity,
    source_tree_sha256: String,
    patches: Vec<NamedIdentity>,
    sysroot: SysrootIdentity,
    openssl: OpenSslIdentity,
    tools: Vec<NamedIdentity>,
    build: BuildContract,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Architecture {
    javascript_engine: &'static str,
    wasm_engine: &'static str,
    host_capability_import_module: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileIdentity {
    sha256: String,
    bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NamedIdentity {
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SysrootIdentity {
    libc: NamedIdentity,
    libsetjmp: NamedIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenSslIdentity {
    manifest: NamedIdentity,
    libcrypto: NamedIdentity,
    libssl: NamedIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildContract {
    cmake_build_type: &'static str,
    clang_target: &'static str,
    execution_model: &'static str,
    initial_memory_pages: u32,
    maximum_memory_pages: u32,
    maximum_table_elements: u32,
    maximum_source_bytes: u32,
    maximum_outstanding_allocation_bytes: u32,
    maximum_outstanding_allocations: u32,
    maximum_runtime_threads_including_root: u32,
    runtime_thread_warning_at: u32,
    locale: &'static str,
    timezone: &'static str,
    zero_archive_date: bool,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn identity(path: &Path) -> Result<FileIdentity, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    Ok(FileIdentity {
        sha256: sha256(&bytes),
        bytes: bytes.len() as u64,
    })
}

fn named_identity(path: &Path, base: &Path) -> Result<NamedIdentity, Box<dyn Error>> {
    let identity = identity(path)?;
    Ok(NamedIdentity {
        path: path
            .strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        sha256: identity.sha256,
        bytes: identity.bytes,
    })
}

fn collect_files(
    root: &Path,
    path: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            collect_files(root, &path, output)?;
        } else {
            path.strip_prefix(root)?;
            output.push(path);
        }
    }
    Ok(())
}

fn tree_sha256(root: &Path) -> Result<String, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by_key(|path| path.strip_prefix(root).unwrap_or(path).to_path_buf());

    let mut tree = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = fs::symlink_metadata(&path)?;
        tree.update(relative.len().to_le_bytes());
        tree.update(relative.as_bytes());
        if metadata.file_type().is_symlink() {
            tree.update(b"symlink\0");
            let target = fs::read_link(&path)?;
            tree.update(target.to_string_lossy().as_bytes());
        } else {
            tree.update(b"file\0");
            tree.update(fs::read(&path)?);
        }
    }
    Ok(format!("{:x}", tree.finalize()))
}

fn collect_patches(root: &Path) -> Result<Vec<NamedIdentity>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    files
        .iter()
        .map(|path| named_identity(path, root.parent().unwrap_or(root)))
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let [wasm, abi, vendor, patches, source, sysroot, openssl, wasi_sdk, output] = args.as_slice()
    else {
        return Err("usage: generate-build-manifest WASM ABI VENDOR PATCHES SOURCE SYSROOT OPENSSL WASI_SDK OUTPUT".into());
    };

    let tools = ["clang", "clang++", "wasm-ld", "llvm-ar", "llvm-ranlib"]
        .iter()
        .map(|name| named_identity(&wasi_sdk.join("bin").join(name), wasi_sdk))
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = BuildManifest {
        schema: 1,
        architecture: Architecture {
            javascript_engine: "existing-native-v8-isolate",
            wasm_engine: "v8-webassembly-module-instance",
            host_capability_import_module: "agentos_posix_v1",
        },
        source_date_epoch: 1_775_854_510,
        wasm: identity(wasm)?,
        abi_manifest: identity(abi)?,
        vendor_manifest: identity(vendor)?,
        source_tree_sha256: tree_sha256(source)?,
        patches: collect_patches(patches)?,
        sysroot: SysrootIdentity {
            libc: named_identity(&sysroot.join("lib/wasm32-wasi-threads/libc.a"), sysroot)?,
            libsetjmp: named_identity(
                &sysroot.join("lib/wasm32-wasi-threads/libsetjmp.a"),
                sysroot,
            )?,
        },
        openssl: OpenSslIdentity {
            manifest: named_identity(&openssl.join("manifest.json"), openssl)?,
            libcrypto: named_identity(&openssl.join("lib/libcrypto.a"), openssl)?,
            libssl: named_identity(&openssl.join("lib/libssl.a"), openssl)?,
        },
        tools,
        build: BuildContract {
            cmake_build_type: "Release",
            clang_target: "wasm32-wasi-threads",
            execution_model: "reactor",
            initial_memory_pages: 1024,
            maximum_memory_pages: 4096,
            maximum_table_elements: 16_384,
            maximum_source_bytes: 8 * 1024 * 1024,
            maximum_outstanding_allocation_bytes: 8 * 1024 * 1024,
            maximum_outstanding_allocations: 32,
            maximum_runtime_threads_including_root: 8,
            runtime_thread_warning_at: 7,
            locale: "C",
            timezone: "UTC",
            zero_archive_date: true,
        },
    };
    fs::write(
        output,
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    Ok(())
}
