//! Architecture / boundary guards (CI hardening, item #2).
//!
//! This is a *chokepoint lint*: it scans the secure-exec Rust source tree and
//! FAILS if a security-sensitive host API ("banned API") appears OUTSIDE an
//! explicit allowlist of sanctioned modules. The goal is to keep host access
//! funnelled through a small, reviewable set of files so that a NEW use of
//! `std::fs`, raw sockets, `Command::new`, or process-environment reads cannot
//! be introduced without either landing in a sanctioned module or consciously
//! updating this allowlist (which forces review of the boundary).
//!
//! The four banned classes mirror the kernel/sidecar trust boundary:
//!
//!   * fs      -- `std::fs` / `tokio::fs` / `File::open` / `File::create` /
//!     `OpenOptions` / raw `openat`. Sanctioned only in the sidecar host-FS
//!     plumbing, the VFS-backed runtime modules, and runtime asset/module
//!     loaders.
//!   * net     -- `std::net` / `tokio::net` socket constructors, `reqwest`,
//!     `hyper`, `to_socket_addrs`, `UnixStream::pair`. Sanctioned only in the
//!     kernel DNS/socket plane, the sidecar host-net chokepoint
//!     (`sidecar::execution`), the embedded V8 runtime IPC pair, and
//!     host-backed storage plugins.
//!   * process -- `std::process::Command` / `tokio::process` / OS `fork`.
//!     Sanctioned only where secure-exec spawns its own helper process (the
//!     client transport that launches the sidecar). Guest "process" spawns are
//!     dispatched through the kernel `CommandDriver` registry and never touch
//!     `Command::new`.
//!   * env     -- `std::env::var` / `var_os` / `vars`. Sanctioned only at the
//!     scrubbed env-assembly / bootstrap points that read host configuration
//!     before a VM is constructed.
//!
//! IMPORTANT MAINTENANCE NOTES
//! ---------------------------
//! * The allowlist is built from the CURRENT legitimate uses so the test is
//!   GREEN today; it is designed to catch only *new* uses.
//! * Build scripts (`build.rs`, `*_build_support.rs`, ...), `tests/` and
//!   `benches/` directories, and inline `#[cfg(test)]` modules are excluded
//!   from the scan (they are not production host-access surface).
//! * `crates/execution/src/benchmark.rs`, `crates/execution/src/bin/`, and
//!   `crates/agentos-native-baseline/` hold benchmarking/dev tooling and are excluded
//!   for the same reason.
//!
//! If you are adding a genuinely new sanctioned chokepoint, add its
//! repo-relative path to the relevant allowlist below WITH a comment
//! explaining why the host access is safe. If you are adding host access
//! anywhere else, route it through an existing chokepoint instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Repo root = `<root>/crates/sidecar` -> up two levels.
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("sidecar crate should live two levels under the repo root")
        .to_path_buf()
}

/// Every production Rust source file under `crates/*/src/`, repo-relative,
/// excluding build scripts, benches, bins, and `tests/` trees.
fn production_source_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates_dir = root.join("crates");
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .expect("crates/ directory should exist")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();
    for crate_dir in crate_dirs {
        let src = crate_dir.join("src");
        if src.is_dir() {
            collect_rs(&src, root, &mut out);
        }
    }
    out.sort();
    out
}

fn collect_rs(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read_dir {dir:?}: {err}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            // Exclude bench/dev binaries that are not production runtime.
            if path.file_name().map(|n| n == "bin").unwrap_or(false) {
                continue;
            }
            collect_rs(&path, root, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            let rel = path
                .strip_prefix(root)
                .expect("source path under repo root")
                .to_path_buf();
            out.push(rel);
        }
    }
}

/// Returns true if the file is excluded from scanning entirely.
fn is_excluded_file(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.ends_with("build.rs")
        || s.ends_with("build_support.rs")
        || s.ends_with("v8_bridge_build.rs")
        // Benchmarking / dev tooling, not production host-access surface.
        || s == "crates/execution/src/benchmark.rs"
        || s.starts_with("crates/agentos-native-baseline/")
        || s.contains("/src/bin/")
}

/// Strip a trailing `//` line comment (good enough for this lint; we are not
/// trying to be a full Rust parser, only to avoid flagging commented examples).
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Track whether a line is inside a top-level `#[cfg(test)]` module so test
/// code is excluded from the scan. We watch for `#[cfg(test)]` immediately
/// followed by a `mod ... {` and then balance braces until the module closes.
struct CfgTestTracker {
    pending_cfg_test: bool,
    depth: u32,
}

impl CfgTestTracker {
    fn new() -> Self {
        Self {
            pending_cfg_test: false,
            depth: 0,
        }
    }

    /// Feed a line. Returns true if this line is inside a `#[cfg(test)]` module.
    fn in_test(&mut self, raw: &str) -> bool {
        let line = strip_line_comment(raw);
        let trimmed = line.trim();

        if self.depth > 0 {
            // Already inside a cfg(test) module: update brace balance.
            self.depth += count_open(line);
            self.depth = self.depth.saturating_sub(count_close(line));
            return true;
        }

        if trimmed.starts_with("#[cfg(test)]") {
            self.pending_cfg_test = true;
            return false;
        }

        if self.pending_cfg_test {
            if trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//") {
                // Attributes/blank lines may sit between #[cfg(test)] and the item.
                return false;
            }
            // The attribute applies to the next item. Only a `mod ... { ... }`
            // creates a test *region* we must skip wholesale; a `#[cfg(test)]`
            // on a `use` / `fn` / `const` is a single item and gets matched
            // line-by-line, so we still skip just that line below.
            self.pending_cfg_test = false;
            if trimmed.starts_with("mod ")
                || trimmed.starts_with("pub mod ")
                || trimmed.starts_with("pub(crate) mod ")
                || trimmed.starts_with("pub(super) mod ")
            {
                // Enter test module; balance braces starting from this line.
                self.depth = count_open(line).saturating_sub(count_close(line));
                if self.depth == 0 {
                    // Single-line `mod x;` declaration (no body) -- nothing to skip.
                }
                return true;
            }
            // A single `#[cfg(test)]` item (use/fn/const/static). Skip this line.
            return true;
        }

        false
    }
}

fn count_open(s: &str) -> u32 {
    s.bytes().filter(|&b| b == b'{').count() as u32
}
fn count_close(s: &str) -> u32 {
    s.bytes().filter(|&b| b == b'}').count() as u32
}

/// A banned-API class and the regex-free matchers describing it.
struct BannedClass {
    name: &'static str,
    /// Substrings; a line matches the class if it contains any of them.
    needles: &'static [&'static str],
    /// Files (repo-relative) where this class is sanctioned.
    allowlist: &'static [&'static str],
}

fn line_matches(line: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| line.contains(n))
}

/// Run the chokepoint scan for one banned class and return offending
/// `path:line: text` strings that are NOT in the allowlist.
fn scan_class(root: &Path, files: &[PathBuf], class: &BannedClass) -> Vec<String> {
    let allow: BTreeSet<&str> = class.allowlist.iter().copied().collect();
    let mut violations = Vec::new();

    for rel in files {
        if is_excluded_file(rel) {
            continue;
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let allowed = allow.contains(rel_str.as_str());
        let abs = root.join(rel);
        let content =
            std::fs::read_to_string(&abs).unwrap_or_else(|err| panic!("read {abs:?}: {err}"));
        let mut tracker = CfgTestTracker::new();
        for (idx, raw) in content.lines().enumerate() {
            let in_test = tracker.in_test(raw);
            if allowed {
                continue; // still need to advance the tracker above
            }
            if in_test {
                continue;
            }
            let code = strip_line_comment(raw);
            if line_matches(code, class.needles) {
                violations.push(format!("{}:{}: {}", rel_str, idx + 1, raw.trim()));
            }
        }
    }
    violations
}

// ---------------------------------------------------------------------------
// Allowlists -- built from the CURRENT legitimate uses (green today).
// ---------------------------------------------------------------------------

/// fs: host filesystem access.
///
/// Sanctioned surface: the sidecar host-FS plumbing + VFS-backed runtime, the
/// JS/Python/WASM runtime asset & module loaders, the sidecar bootstrap
/// (stdio/service/state/vm), and runtime support glue. These modules read
/// real host files to seed the VFS, load runtime assets, and bridge guest FS
/// syscalls to the host-dir mount.
const FS_ALLOW: &[&str] = &[
    // sidecar host-FS chokepoint + bootstrap. `host_dir.rs` also contains the
    // universal host-mount confinement primitive (the `confine` module: the
    // single resolve-beneath walk using plain `openat(2)`, fd-anchored, no
    // `openat2`, running identically on Linux, macOS, and gVisor). It replaced
    // the deleted macOS-only `macos_fs.rs` cap-std fallback; see the `confine`
    // module docs for why `openat2` was removed.
    "crates/sidecar/src/filesystem.rs",
    "crates/sidecar/src/plugins/host_dir.rs",
    "crates/sidecar/src/plugins/module_access.rs",
    // agentOS package projection: the sidecar is the host-side TCB that reads a
    // trusted, client-configured package's tar + `agentos-package.json` from the
    // host to build the read-only `/opt/agentos` granular mounts (no extraction,
    // no on-disk symlink farm). Same sanctioned read-only host-source boundary as
    // filesystem.rs/host_dir.rs.
    "crates/sidecar/src/package_projection.rs",
    "crates/sidecar/src/stdio.rs",
    "crates/sidecar/src/state.rs",
    "crates/sidecar/src/vm.rs",
    "crates/sidecar/src/service.rs",
    "crates/sidecar/src/execution.rs",
    "crates/sidecar/src/plugins/chunked_local.rs",
    "crates/agentos-vfs/src/local/file_block_store.rs",
    "crates/agentos-vfs/src/local/sqlite_metadata_store.rs",
    // Tar-backed read-only VFS: mmaps the trusted, client-configured package
    // tar from the host and serves member byte ranges without extracting.
    // Same sanctioned read-only host-source boundary as host_dir.rs (the tar is
    // an immutable, content-addressed mount source); reads are SIGBUS-guarded.
    "crates/vfs/src/posix/tar_fs.rs",
    // language-runtime asset / module loaders (read host runtime assets)
    "crates/execution/src/python.rs",
    "crates/execution/src/wasm.rs",
    "crates/execution/src/javascript.rs",
    "crates/execution/src/node_import_cache.rs",
    "crates/execution/src/runtime_support.rs",
    // Host-side V8 diagnostics: module-trace and sync-RPC latency profilers
    // write to an operator-provided file path, and snapshot bootstrap reads the
    // userland bundle from PI_SNAPSHOT_BUNDLE_PATH. Host-only, not guest-reachable.
    "crates/v8-runtime/src/execution.rs",
    "crates/v8-runtime/src/host_call.rs",
    "crates/v8-runtime/src/snapshot.rs",
    // Session-phase perf recorder writes to an operator-provided file path
    // (AGENTOS_V8_SESSION_PHASES_FILE). Host-only diagnostics, same class as
    // execution.rs/host_call.rs above.
    "crates/v8-runtime/src/session.rs",
];

/// net: host network access.
///
/// Sanctioned surface: the kernel DNS resolver plane, the sidecar host-net
/// chokepoint (`execution.rs`, which owns all guest TCP/UDP/Unix sockets), the
/// host-backed storage/agent plugins (which open egress to S3 / Google Drive /
/// the sandbox-agent control plane), the embedded V8 runtime IPC socketpair,
/// and the client transport that talks to the spawned sidecar.
const NET_ALLOW: &[&str] = &[
    // kernel network plane
    "crates/kernel/src/dns.rs",
    // Shared IP classifier only; no host sockets are opened here.
    "crates/kernel/src/network_policy.rs",
    // Shared socket-address formatting only; no host sockets are opened here.
    "crates/sidecar-core/src/net.rs",
    "crates/kernel/src/socket_table.rs",
    "crates/kernel/src/kernel.rs",
    // sidecar host-net chokepoint + bootstrap
    "crates/sidecar/src/execution.rs",
    "crates/sidecar/src/state.rs",
    "crates/sidecar/src/vm.rs",
    // host-backed storage / agent plugins (network egress)
    "crates/sidecar/src/plugins/s3_common.rs",
    "crates/agentos-vfs/src/s3/block_store.rs",
    "crates/agentos-vfs/src/s3/object_backend.rs",
    "crates/sidecar/src/plugins/google_drive.rs",
    "crates/sidecar/src/plugins/sandbox_agent.rs",
    // embedded runtime IPC socketpair (not external egress)
    "crates/v8-runtime/src/embedded_runtime.rs",
    "crates/execution/src/v8_runtime.rs",
    // client spawns + connects to the sidecar helper
    "crates/agentos-sidecar-client/src/transport.rs",
];

/// process: OS subprocess creation.
///
/// Sanctioned surface: only the client transport, which spawns secure-exec's
/// own sidecar helper binary. Guest "process" spawns go through the kernel
/// `CommandDriver` registry and never reach `Command::new`.
const PROCESS_ALLOW: &[&str] = &[
    "crates/agentos-sidecar-client/src/transport.rs",
    // V8 snapshot builder re-execs secure-exec's OWN binary as a helper
    // (SNAPSHOT_HELPER_ENV) so snapshot creation runs in a clean process.
    // Host-side bootstrap only; no guest-controlled input picks the program.
    "crates/v8-runtime/src/snapshot.rs",
];

/// env: process-environment reads.
///
/// Sanctioned surface: the scrubbed/bootstrap configuration readers that look
/// up host configuration (sidecar binary path, node binary path/PATH, codec
/// selection, subprocess re-exec markers, local-endpoint test escape hatch)
/// before a VM exists.
const ENV_ALLOW: &[&str] = &[
    "crates/agentos-sidecar-client/src/transport.rs",
    "crates/execution/src/host_node.rs",
    // Node import cache reads an operator timeout knob before materializing
    // host-side runtime assets for VM startup.
    "crates/execution/src/node_import_cache.rs",
    // Host-side perf phase diagnostics toggles, read from operator env and not
    // guest-reachable.
    "crates/execution/src/javascript.rs",
    "crates/sidecar/src/filesystem.rs",
    "crates/v8-runtime/src/bridge.rs",
    "crates/sidecar/src/execution.rs",
    "crates/sidecar/src/plugins/s3_common.rs",
    // Host-process startup log-level knob, read before any VM exists.
    "crates/sidecar/src/main.rs",
    // Host-side V8 diagnostics toggles (module-trace + sync-RPC latency
    // profiling + snapshot-bundle path), read at runtime init from operator
    // env. Not guest-reachable.
    "crates/v8-runtime/src/execution.rs",
    "crates/v8-runtime/src/host_call.rs",
    "crates/v8-runtime/src/snapshot.rs",
    // Browser sidecar reads a test-only vm.fetch timeout override (bucket 1:
    // process-wide test/debug knob, native-only); not VM policy.
    "crates/sidecar-browser/src/service.rs",
    // Warm-isolate pool sizing knob (AGENTOS_V8_WARM_ISOLATES), read at
    // executor init from operator env. Not guest-reachable.
    "crates/execution/src/v8_host.rs",
    // Wasm runner mode/cache knobs (AGENTOS_WASM_SNAPSHOT_RUNNER,
    // AGENTOS_WASM_RUNNER_NO_CACHE) + warm-pool sizing, read at executor init
    // from operator env. Not guest-reachable. (wasm.rs is already a sanctioned
    // FS asset-loading boundary above.)
    "crates/execution/src/wasm.rs",
    // Session-phase perf diagnostics toggles (AGENTOS_V8_SESSION_PHASES*),
    // read from operator env. Not guest-reachable.
    "crates/v8-runtime/src/session.rs",
];

fn fs_class() -> BannedClass {
    BannedClass {
        name: "fs",
        needles: &[
            "std::fs",
            "tokio::fs",
            "File::open",
            "File::create",
            "OpenOptions",
            "openat",
        ],
        allowlist: FS_ALLOW,
    }
}

fn net_class() -> BannedClass {
    BannedClass {
        name: "net",
        needles: &[
            "std::net::",
            "tokio::net::",
            "reqwest::",
            "reqwest ",
            "hyper::",
            "TcpStream::",
            "TcpListener::bind",
            "UdpSocket::bind",
            "UnixStream::connect",
            "UnixStream::pair",
            "UnixListener::bind",
            ".to_socket_addrs(",
            "std::os::unix::net",
        ],
        allowlist: NET_ALLOW,
    }
}

fn process_class() -> BannedClass {
    BannedClass {
        name: "process",
        needles: &[
            "std::process::Command",
            "process::Command",
            "tokio::process",
            "Command::new",
            "libc::fork",
            "nix::unistd::fork",
        ],
        allowlist: PROCESS_ALLOW,
    }
}

fn env_class() -> BannedClass {
    BannedClass {
        name: "env",
        needles: &[
            "env::var(",
            "env::var_os(",
            "env::vars(",
            "env::vars_os(",
            "std::env::var",
        ],
        allowlist: ENV_ALLOW,
    }
}

fn assert_green(root: &Path, files: &[PathBuf], class: BannedClass) {
    let violations = scan_class(root, files, &class);
    assert!(
        violations.is_empty(),
        "\n\nChokepoint lint ({}) found {} host-API use(s) OUTSIDE the sanctioned \
allowlist.\nEither route the access through an existing chokepoint, or -- if this \
is a genuinely new sanctioned boundary -- add the file to the `{}` allowlist in \
crates/sidecar/tests/architecture_guards.rs with a justifying comment.\n\n{}\n",
        class.name,
        violations.len(),
        match class.name {
            "fs" => "FS_ALLOW",
            "net" => "NET_ALLOW",
            "process" => "PROCESS_ALLOW",
            _ => "ENV_ALLOW",
        },
        violations.join("\n"),
    );
}

#[test]
fn fs_access_confined_to_chokepoints() {
    let root = repo_root();
    let files = production_source_files(&root);
    assert_green(&root, &files, fs_class());
}

#[test]
fn net_access_confined_to_chokepoints() {
    let root = repo_root();
    let files = production_source_files(&root);
    assert_green(&root, &files, net_class());
}

#[test]
fn process_spawn_confined_to_chokepoints() {
    let root = repo_root();
    let files = production_source_files(&root);
    assert_green(&root, &files, process_class());
}

#[test]
fn env_reads_confined_to_chokepoints() {
    let root = repo_root();
    let files = production_source_files(&root);
    assert_green(&root, &files, env_class());
}

/// Sanity: the scan actually sees source files and the allowlisted files exist.
/// Guards against a refactor silently making the lint scan nothing (which would
/// make it vacuously pass).
#[test]
fn lint_scans_real_sources_and_allowlist_paths_exist() {
    let root = repo_root();
    let files = production_source_files(&root);
    assert!(
        files.len() > 30,
        "expected to scan many source files, found {}",
        files.len()
    );

    let mut missing = Vec::new();
    for class in [FS_ALLOW, NET_ALLOW, PROCESS_ALLOW, ENV_ALLOW] {
        for rel in class {
            if !root.join(rel).is_file() {
                missing.push(rel.to_string());
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "allowlist references files that no longer exist (clean them up): {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Dependency-boundary guard: no secure-exec crate may depend on an
// agent / ACP / session crate. secure-exec must remain free of Agent OS
// concerns (ACP, agent adapters, sessions).
// ---------------------------------------------------------------------------

#[test]
fn no_secure_exec_crate_depends_on_agent_acp_session() {
    let root = repo_root();
    let crates_dir = root.join("crates");
    let banned_dep_markers = ["agent", "acp", "session"];

    let mut violations = Vec::new();
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .expect("crates/ exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    for crate_dir in crate_dirs {
        let manifest = crate_dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|err| panic!("read {manifest:?}: {err}"));
        let mut in_deps = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') {
                // Any table whose name mentions "dependencies" is a dep table.
                in_deps = line.contains("dependencies");
                continue;
            }
            if !in_deps || line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Dependency key is the token before `=` or whitespace.
            let key = line
                .split(['=', ' ', '\t'])
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            // Only consider secure-exec / agentos style crate names, and skip
            // false positives like "tokio" containing none of the markers.
            for marker in banned_dep_markers {
                if key.contains(marker) {
                    violations.push(format!(
                        "{}: depends on banned crate `{}`",
                        manifest
                            .strip_prefix(&root)
                            .unwrap_or(&manifest)
                            .to_string_lossy(),
                        key
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "\n\nsecure-exec crates must not depend on agent/acp/session crates \
(Agent OS owns those). Offending dependencies:\n\n{}\n",
        violations.join("\n")
    );
}
