use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const ENV_NODE: &str = "AGENT_OS_NODE";
const ENV_BUILD_SCRIPT: &str = "AGENT_OS_V8_BRIDGE_BUILD_SCRIPT";
const ENV_DEBUG: &str = "AGENT_OS_GENERATED_ASSET_DEBUG";

pub fn build_v8_bridge(crate_manifest_dir: &Path, out_dir: &Path) {
    let repo_root = crate_manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!(
                "failed to resolve repo root from CARGO_MANIFEST_DIR={}",
                crate_manifest_dir.display()
            )
        });
    let script_path = resolve_build_script(repo_root);
    let package_root = script_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!(
                "failed to resolve package root from V8 bridge build script path {}",
                script_path.display()
            )
        });
    let node_modules = package_root.join("node_modules");
    let node = env::var_os(ENV_NODE).unwrap_or_else(|| "node".into());
    let node_path = PathBuf::from(node);
    let debug = env::var_os(ENV_DEBUG).is_some();

    emit_rerun_inputs(repo_root, &script_path);
    println!("cargo:rerun-if-env-changed={ENV_NODE}");
    println!("cargo:rerun-if-env-changed={ENV_BUILD_SCRIPT}");
    println!("cargo:rerun-if-env-changed={ENV_DEBUG}");

    if !node_modules.exists() {
        panic!(
            "missing Node dependencies at {}. Run `pnpm install` from {} before building V8 bridge assets.",
            node_modules.display(),
            repo_root.display()
        );
    }

    require_pnpm(repo_root, debug);

    if debug {
        println!(
            "cargo:warning=building V8 bridge with node={} script={} out_dir={}",
            node_path.display(),
            script_path.display(),
            out_dir.display()
        );
    }

    let output = Command::new(&node_path)
        .arg(&script_path)
        .arg("--out-dir")
        .arg(out_dir)
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|error| match error.kind() {
            io::ErrorKind::NotFound => panic!(
                "failed to build V8 bridge assets because `{}` was not found. Install Node.js or set {ENV_NODE} to the Node binary.",
                node_path.display()
            ),
            _ => panic!(
                "failed to spawn V8 bridge build with `{}`: {}",
                node_path.display(),
                error
            ),
        });

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let dependency_hint = if stderr.contains("ERR_MODULE_NOT_FOUND")
            || stderr.contains("Cannot find package")
            || stderr.contains("Cannot find module")
        {
            "\nNode dependencies appear to be missing or incomplete. Run `pnpm install` from the repo root."
        } else {
            ""
        };

        panic!(
            "failed to build V8 bridge assets with `{}` (status: {}).{}\nstdout:\n{}\nstderr:\n{}",
            node_path.display(),
            output.status,
            dependency_hint,
            stdout.trim(),
            stderr.trim()
        );
    }

    let bridge_output = out_dir.join("v8-bridge.js");
    let zlib_output = out_dir.join("v8-bridge-zlib.js");
    if !bridge_output.exists() || !zlib_output.exists() {
        panic!(
            "V8 bridge build completed but expected outputs are missing: {}, {}",
            bridge_output.display(),
            zlib_output.display()
        );
    }
}

fn resolve_build_script(repo_root: &Path) -> PathBuf {
    match env::var_os(ENV_BUILD_SCRIPT) {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        }
        None => repo_root.join("packages/core/scripts/build-v8-bridge.mjs"),
    }
}

fn require_pnpm(repo_root: &Path, debug: bool) {
    let output = Command::new("pnpm")
        .arg("--version")
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                panic!(
                    "failed to build V8 bridge assets because `pnpm` was not found. Install pnpm and run `pnpm install` from {}.",
                    repo_root.display()
                )
            }
            _ => panic!("failed to check pnpm availability: {}", error),
        });

    if !output.status.success() {
        panic!(
            "failed to build V8 bridge assets because `pnpm --version` failed with status {}. Run `pnpm install` from {} after fixing pnpm.",
            output.status,
            repo_root.display()
        );
    }

    if debug {
        println!(
            "cargo:warning=pnpm version {}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
}

fn emit_rerun_inputs(repo_root: &Path, script_path: &Path) {
    let inputs = [
        repo_root.join("crates/build-support/v8_bridge_build.rs"),
        script_path.to_path_buf(),
        repo_root.join("crates/execution/assets/v8-bridge.source.js"),
        repo_root.join("packages/core/package.json"),
        repo_root.join("pnpm-lock.yaml"),
    ];

    for input in inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }

    let shim_dir = repo_root.join("crates/execution/assets/undici-shims");
    emit_rerun_dir(&shim_dir).unwrap_or_else(|error| {
        panic!(
            "failed to enumerate V8 bridge shim inputs under {}: {}",
            shim_dir.display(),
            error
        )
    });
}

fn emit_rerun_dir(dir: &Path) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            emit_rerun_dir(&path)?;
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::emit_rerun_dir;
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    fn temp_test_dir(name: &str) -> io::Result<PathBuf> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "agent-os-v8-bridge-build-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path)?;
        Ok(path)
    }

    #[cfg(unix)]
    #[test]
    fn emit_rerun_dir_does_not_follow_directory_symlinks() -> io::Result<()> {
        let dir = temp_test_dir("symlink-cycle")?;
        fs::write(dir.join("shim.js"), b"export {};")?;
        std::os::unix::fs::symlink(&dir, dir.join("self"))?;

        let result = emit_rerun_dir(&dir);
        let cleanup = fs::remove_dir_all(&dir);

        result?;
        cleanup?;
        Ok(())
    }
}
