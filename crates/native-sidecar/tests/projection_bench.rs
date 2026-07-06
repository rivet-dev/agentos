//! Reproducible micro-benchmark for the agentOS package **load** step: the
//! work the sidecar does per packed `.aospkg` at VM configure time — decode
//! the chunk1 vbare manifest, build the granular leaf mounts (tar-backed
//! version mount + `current`/`bin/*`/man-page symlinks), and open the tar
//! mount from its precomputed index. The tar is never extracted.
//!
//! ## What is timed
//! The timed span, per sample, is **load time only** — the work the sidecar
//! does at VM configure time for an already-packed `.aospkg`:
//!   1. `read_package_manifest_from_path(<pkg>.aospkg)`
//!        → read the 16-byte header, decode the chunk1 `PackageManifest`
//!          (commands included)
//!   2. `build_package_leaf_mounts(&[descriptor], "/opt/agentos")`
//!        → cross-package command-collision check + leaf-mount construction
//!   3. `TarFileSystem::open` on the `.aospkg`
//!        → decode the precomputed chunk2 mount index + mmap the mount tar
//!          (cold each sample: the identity-keyed archive cache holds only
//!          weak refs, so dropping the fs between samples re-loads the index)
//! Pack time (scanning a source `package.tar` and encoding the `.aospkg`
//! header/manifest/index — the "compile" step) is explicitly NOT counted: it
//! happens once at package build time, not at VM load. It is printed once per
//! target as an informational `pack (excluded)` line.
//! Nothing else (no ConfigureVm / no VM boot) is included.
//!
//! ## Reproducibility
//! Fixed warmup + sample count, deterministic on-disk inputs. Each sample
//! re-runs the *full* projection from scratch. Re-running the same command on
//! the same inputs yields comparable numbers.
//!
//! ## Inputs
//! Always synthesizes deterministic package tars in a tempdir first, so the
//! benchmark prints useful rows in a fresh checkout with no built registry:
//!   - synthetic-tiny:   a few `bin/` commands and a small manifest
//!   - synthetic-medium: the same shape plus a deterministic ~5 MiB payload
//!
//! Then runs the repo's built registry tars (skipped with a note if a tar is
//! absent, e.g. in a clean checkout that has not built `dist/`):
//!   - coreutils: `registry/software/coreutils/dist/package.tar`  (small-ish, many commands)
//!   - tar:       `registry/software/tar/dist/package.tar`        (single wasm binary)
//! Override the source tars via env:
//!   PROJ_BENCH_COREUTILS_TAR=/abs/package.tar  PROJ_BENCH_TAR_TAR=/abs/package.tar
//! Tune the run via env: PROJ_BENCH_SAMPLES (default 30), PROJ_BENCH_WARMUP
//! (default 2).
//!
//! ## Run
//! ```text
//! cargo test -p agentos-native-sidecar --release --test projection_bench -- --ignored --nocapture
//! # or, pointing at built tars elsewhere:
//! PROJ_BENCH_COREUTILS_TAR=/abs/registry/software/coreutils/dist/package.tar \
//! PROJ_BENCH_TAR_TAR=/abs/registry/software/tar/dist/package.tar \
//!   cargo test -p agentos-native-sidecar --release --test projection_bench -- --ignored --nocapture
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use agentos_native_sidecar::package_projection::{
    build_package_leaf_mounts, read_package_manifest_from_path, DEFAULT_PACKAGE_TAR_NAME,
};
use vfs::package_format::pack::pack_aospkg_from_tar;
use vfs::posix::TarFileSystem;

const SOURCE_PACKAGE_TAR_NAME: &str = "package.tar";

fn repo_root() -> PathBuf {
    // crates/sidecar -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").into())
}

fn source_tar_path(env_key: &str, default_rel: &str) -> PathBuf {
    match std::env::var(env_key) {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => repo_root().join(default_rel),
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Nearest-rank percentile (p in [0,100]) over an already-sorted slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

struct Sample {
    total: f64,
    manifest: f64,
    mounts: f64,
    tar_open: f64,
}

struct SyntheticTargets {
    root: PathBuf,
    tiny: PathBuf,
    medium: PathBuf,
    tiny_pack: Duration,
    medium_pack: Duration,
}

struct RepackedTargets {
    root: PathBuf,
    coreutils: PathBuf,
    tar: PathBuf,
    coreutils_pack: Option<Duration>,
    tar_pack: Option<Duration>,
}

impl Drop for RepackedTargets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Drop for SyntheticTargets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_repeated_file(path: &Path, len: usize) {
    let mut file = fs::File::create(path)
        .unwrap_or_else(|e| panic!("create synthetic payload {} failed: {e}", path.display()));
    let chunk = b"secure-exec projection bench payload\n";
    let mut remaining = len;
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        std::io::Write::write_all(&mut file, &chunk[..n])
            .unwrap_or_else(|e| panic!("write synthetic payload {} failed: {e}", path.display()));
        remaining -= n;
    }
}

fn create_package_tar(label: &str, dest: &Path, commands: &[&str], payload_bytes: usize) {
    let source = dest.join("package");
    fs::create_dir_all(source.join("bin"))
        .unwrap_or_else(|e| panic!("create synthetic package {} failed: {e}", source.display()));
    fs::write(
        source.join("agentos-package.json"),
        format!("{{\"name\":\"synthetic-{label}\",\"version\":\"1.0.0\"}}\n"),
    )
    .unwrap_or_else(|e| panic!("write synthetic manifest for {label} failed: {e}"));

    for command in commands {
        fs::write(
            source.join("bin").join(command),
            format!("#!/bin/sh\nprintf '{command}\\n'\n"),
        )
        .unwrap_or_else(|e| panic!("write synthetic bin/{command} failed: {e}"));
    }

    let mut members = vec!["agentos-package.json", "bin"];
    if payload_bytes > 0 {
        write_repeated_file(&source.join("payload.dat"), payload_bytes);
        members.push("payload.dat");
    }

    let tar_path = dest.join(SOURCE_PACKAGE_TAR_NAME);
    let status = Command::new("tar")
        .args([
            "--sort=name",
            "--mtime=@0",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "-cf",
        ])
        .arg(&tar_path)
        .arg("-C")
        .arg(&source)
        .args(members)
        .status()
        .unwrap_or_else(|e| panic!("run tar for synthetic {label} failed: {e}"));
    assert!(
        status.success(),
        "tar failed for synthetic {label} with status {status}"
    );
}

fn create_synthetic_targets() -> SyntheticTargets {
    let unique = format!(
        "secure-exec-projection-bench-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let tiny = root.join("tiny");
    let medium = root.join("medium");
    fs::create_dir_all(&tiny)
        .unwrap_or_else(|e| panic!("create synthetic tiny dir {} failed: {e}", tiny.display()));
    fs::create_dir_all(&medium).unwrap_or_else(|e| {
        panic!(
            "create synthetic medium dir {} failed: {e}",
            medium.display()
        )
    });

    create_package_tar("tiny", &tiny, &["alpha", "beta", "gamma"], 0);
    create_package_tar(
        "medium",
        &medium,
        &["alpha", "beta", "gamma"],
        5 * 1024 * 1024,
    );

    let tiny_pack = repack_package_tar_to_aospkg(
        &tiny.join(SOURCE_PACKAGE_TAR_NAME),
        &tiny.join(DEFAULT_PACKAGE_TAR_NAME),
    );
    let medium_pack = repack_package_tar_to_aospkg(
        &medium.join(SOURCE_PACKAGE_TAR_NAME),
        &medium.join(DEFAULT_PACKAGE_TAR_NAME),
    );

    SyntheticTargets {
        root,
        tiny,
        medium,
        tiny_pack,
        medium_pack,
    }
}

fn create_repacked_real_targets(coreutils_tar: &Path, tar_tar: &Path) -> RepackedTargets {
    let unique = format!(
        "secure-exec-projection-real-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let coreutils = root.join("coreutils");
    let tar = root.join("tar");
    fs::create_dir_all(&coreutils).expect("create repacked coreutils dir");
    fs::create_dir_all(&tar).expect("create repacked tar dir");

    let coreutils_pack = coreutils_tar.is_file().then(|| {
        repack_package_tar_to_aospkg(coreutils_tar, &coreutils.join(DEFAULT_PACKAGE_TAR_NAME))
    });
    let tar_pack = tar_tar
        .is_file()
        .then(|| repack_package_tar_to_aospkg(tar_tar, &tar.join(DEFAULT_PACKAGE_TAR_NAME)));

    RepackedTargets {
        root,
        coreutils,
        tar,
        coreutils_pack,
        tar_pack,
    }
}

/// Pack a source `package.tar` into a `.aospkg` via the canonical packer in
/// `vfs::package_format::pack` (agentos-package.json is consumed at pack time
/// and stripped from the mount tar). This is the "compile" step; it runs at
/// package build time and is never part of the timed load span. Returns the
/// pack duration so callers can print it as an excluded stat.
fn repack_package_tar_to_aospkg(source_tar: &Path, dest_aospkg: &Path) -> Duration {
    let started = Instant::now();
    pack_aospkg_from_tar(source_tar, dest_aospkg)
        .unwrap_or_else(|e| panic!("pack {} failed: {e}", source_tar.display()));
    started.elapsed()
}


fn project_once(dir: &str) -> (Sample, usize, usize) {
    // Step 1: read the .aospkg header + chunk1 manifest (commands included).
    // The wire carries the packed file path, so the bench does too.
    let aospkg = Path::new(dir).join(DEFAULT_PACKAGE_TAR_NAME);
    let aospkg = aospkg.to_str().expect("utf8 aospkg path");
    let t0 = Instant::now();
    let descriptor = read_package_manifest_from_path(aospkg)
        .unwrap_or_else(|e| panic!("read_package_manifest_from_path({aospkg}) failed: {e:?}"));
    let manifest_ms = ms(t0.elapsed());
    let command_count = descriptor.commands.len();
    let tar_path = descriptor
        .tar_path
        .clone()
        .unwrap_or_else(|| panic!("bench package in {dir} must have a .aospkg tar"));

    // Step 2: build the granular leaf mounts (+ collision check).
    let t1 = Instant::now();
    let mounts = build_package_leaf_mounts(&[descriptor], "/opt/agentos")
        .unwrap_or_else(|e| panic!("build_package_leaf_mounts({dir}) failed: {e:?}"));
    let mounts_ms = ms(t1.elapsed());

    // Step 3: open the tar mount — decode the precomputed chunk2 index and
    // mmap the mount tar. The archive cache only holds weak refs, so dropping
    // the fs at the end of this sample makes the next sample a cold load.
    let t2 = Instant::now();
    let fs = TarFileSystem::open(&tar_path)
        .unwrap_or_else(|e| panic!("TarFileSystem::open({tar_path}) failed: {e:?}"));
    let tar_open_ms = ms(t2.elapsed());
    drop(fs);

    let total_ms = ms(t0.elapsed());
    (
        Sample {
            total: total_ms,
            manifest: manifest_ms,
            mounts: mounts_ms,
            tar_open: tar_open_ms,
        },
        command_count,
        mounts.len(),
    )
}

fn run_target(label: &str, dir: &Path, warmup: usize, samples: usize, pack: Option<Duration>) {
    let tar = dir.join(DEFAULT_PACKAGE_TAR_NAME);
    if !tar.is_file() {
        println!(
            "[skip] {label}: no {DEFAULT_PACKAGE_TAR_NAME} at {} (build its dist/ first)",
            dir.display()
        );
        return;
    }
    let tar_bytes = std::fs::metadata(&tar).map(|m| m.len()).unwrap_or(0);
    let dir_str = dir.to_str().expect("utf8 dir");

    // Warmup (warms page cache; results discarded).
    let mut cmd_count = 0usize;
    let mut mount_count = 0usize;
    for _ in 0..warmup {
        let (_, c, m) = project_once(dir_str);
        cmd_count = c;
        mount_count = m;
    }

    let mut rows: Vec<Sample> = Vec::with_capacity(samples);
    for _ in 0..samples {
        let (sample, c, m) = project_once(dir_str);
        cmd_count = c;
        mount_count = m;
        rows.push(sample);
    }

    let mut totals: Vec<f64> = rows.iter().map(|r| r.total).collect();
    let mut manifests: Vec<f64> = rows.iter().map(|r| r.manifest).collect();
    let mut mounts_v: Vec<f64> = rows.iter().map(|r| r.mounts).collect();
    let mut tar_opens: Vec<f64> = rows.iter().map(|r| r.tar_open).collect();
    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    manifests.sort_by(|a, b| a.partial_cmp(b).unwrap());
    mounts_v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tar_opens.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!(
        "\n=== {label}  ({:.1} MiB tar, {cmd_count} commands, {mount_count} leaf mounts, N={samples}, warmup={warmup}) ===",
        tar_bytes as f64 / (1024.0 * 1024.0)
    );
    if let Some(pack) = pack {
        println!(
            "  pack .tar -> .aospkg (excluded from load): {:.1} ms, once at package build time",
            ms(pack)
        );
    }
    println!(
        "  {:<28} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "load span (ms)", "min", "median", "mean", "p95", "max"
    );
    // mean() is order-invariant, so the sorted slices serve every stat.
    let print_stat = |name: &str, sorted: &[f64]| {
        println!(
            "  {:<28} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
            name,
            sorted.first().copied().unwrap_or(f64::NAN),
            median(sorted),
            mean(sorted),
            percentile(sorted, 95.0),
            sorted.last().copied().unwrap_or(f64::NAN),
        );
    };
    print_stat("FULL load (1+2+3)", &totals);
    print_stat("  manifest+cmds (step1)", &manifests);
    print_stat("  leaf mounts   (step2)", &mounts_v);
    print_stat("  tar mount open (step3)", &tar_opens);

}

#[test]
#[ignore = "bench: prints a projection-timing table; run with --ignored --nocapture"]
fn projection_bench() {
    let warmup = env_usize("PROJ_BENCH_WARMUP", 2);
    let samples = env_usize("PROJ_BENCH_SAMPLES", 30);

    let coreutils_tar = source_tar_path(
        "PROJ_BENCH_COREUTILS_TAR",
        "registry/software/coreutils/dist/package.tar",
    );
    let tar_tar = source_tar_path("PROJ_BENCH_TAR_TAR", "registry/software/tar/dist/package.tar");

    println!("\n# agentOS package load benchmark (.aospkg)");
    println!("# timed span = manifest chunk read + leaf mounts + tar mount open (index decode + mmap)");
    println!("# pack (.tar -> .aospkg) runs once in setup and is excluded from all load stats");
    println!("# repo root = {}", repo_root().display());

    let synthetic = create_synthetic_targets();
    run_target(
        "synthetic-tiny",
        &synthetic.tiny,
        warmup,
        samples,
        Some(synthetic.tiny_pack),
    );
    run_target(
        "synthetic-medium",
        &synthetic.medium,
        warmup,
        samples,
        Some(synthetic.medium_pack),
    );
    let real = create_repacked_real_targets(&coreutils_tar, &tar_tar);
    run_target(
        "coreutils",
        &real.coreutils,
        warmup,
        samples,
        real.coreutils_pack,
    );
    run_target("tar (wasm binary)", &real.tar, warmup, samples, real.tar_pack);
    println!();
}

/// Default-suite load budget: the coreutils FULL load (manifest + leaf mounts
/// + tar index open) must stay under 20 ms median even in a debug build (it
/// measures ~0.15 ms in release, ~1 ms in debug — 20 ms means something is
/// structurally wrong, e.g. a whole-archive read snuck back in). Not hidden
/// behind `#[ignore]` so the budget cannot silently regress; skips cleanly
/// when the registry package is not built.
#[test]
fn coreutils_load_budget() {
    let coreutils_tar = source_tar_path(
        "PROJ_BENCH_COREUTILS_TAR",
        "registry/software/coreutils/dist/package.tar",
    );
    if !coreutils_tar.is_file() {
        eprintln!("skipping coreutils_load_budget: {} not built", coreutils_tar.display());
        return;
    }
    let real = create_repacked_real_targets(&coreutils_tar, Path::new("/nonexistent"));
    let dir = real.coreutils.to_str().expect("utf8 dir");
    for _ in 0..2 {
        let _ = project_once(dir);
    }
    let mut totals: Vec<f64> = (0..10).map(|_| project_once(dir).0.total).collect();
    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(
        median(&totals) < 20.0,
        "coreutils FULL-load median (incl. tar index decode + mmap) must be < 20 ms, got {:.3} ms",
        median(&totals)
    );
}
