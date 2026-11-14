//! Reclaims per-process temp directories that a sidecar left behind when it
//! died without running its own cleanup (for example after `SIGKILL`).
//!
//! Each such directory name embeds the owning sidecar PID. Every sidecar also
//! holds a shared `flock` on its own liveness file for its whole lifetime. A
//! directory is removed only when its owner is provably gone: a sweeper can
//! take the liveness file's exclusive lock.
//!
//! A PID alone never proves an owner is gone. Sidecars in different PID
//! namespaces can share one temp directory, so a PID that names no process here
//! can belong to a live sidecar elsewhere, and two live sidecars can have the
//! same PID. The lock is shared so that both same-PID owners hold it, and
//! entries of an owner without a liveness file (an older sidecar, or one that
//! could not take its lock) are never reclaimed.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use agentos_execution::{node_import_cache_process_root, NODE_IMPORT_CACHE_PROCESS_ROOT_PREFIX};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

/// VM shadow roots are named `<prefix><pid>-<vm id>-<suffix>`.
pub(crate) const VM_SHADOW_ROOT_PID_PREFIX: &str = "agentos-native-sidecar-shadow-pid";
/// VM Unix socket namespaces are named `<prefix><pid>-<suffix>`. The prefix is
/// short because socket paths must fit `sockaddr_un`.
pub(crate) const VM_UNIX_SOCKET_DIR_PID_PREFIX: &str = "agentos-uds-p";
/// Liveness files are named `<prefix><pid><LIVENESS_LOCK_SUFFIX>`.
const LIVENESS_LOCK_PREFIX: &str = "agentos-sidecar-pid";
const LIVENESS_LOCK_SUFFIX: &str = "-liveness.lock";

const OWNED_TEMP_ENTRY_PREFIXES: &[&str] = &[
    NODE_IMPORT_CACHE_PROCESS_ROOT_PREFIX,
    VM_SHADOW_ROOT_PID_PREFIX,
    VM_UNIX_SOCKET_DIR_PID_PREFIX,
    LIVENESS_LOCK_PREFIX,
];

/// Upper bound on entries one startup sweep removes. Later starts reclaim
/// whatever remains.
const MAX_RECLAIMED_TEMP_ENTRIES_PER_START: usize = 1024;

const RECLAIM_JOB_RESERVATION_BYTES: usize = 64 * 1024;

static PROCESS_LIVENESS_LOCK: OnceLock<Flock<File>> = OnceLock::new();

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReclaimReport {
    pub(crate) removed: usize,
    pub(crate) failed: usize,
    pub(crate) truncated: bool,
}

/// Take this process's liveness lock, then queue the sweep of exited sidecars
/// on the bounded blocking executor so startup never waits on disk work.
pub(crate) fn start_temp_dir_reclaim(runtime: &agentos_runtime::RuntimeContext) {
    let temp_dir = std::env::temp_dir();
    if let Err(error) = hold_process_liveness_lock(&temp_dir) {
        // Without the lock, no sweeper can prove this process is gone, so its
        // directories survive a crash until removed by hand.
        tracing::warn!(%error, "could not take the sidecar liveness lock");
    }
    if let Err(error) = runtime
        .blocking()
        .submit(RECLAIM_JOB_RESERVATION_BYTES, move || {
            let report = reclaim_dead_sidecar_temp_dirs(&temp_dir);
            if report.removed > 0 || report.failed > 0 {
                tracing::info!(
                    removed = report.removed,
                    failed = report.failed,
                    temp_dir = %temp_dir.display(),
                    "reclaimed temp directories of exited agentOS sidecars"
                );
            }
            if report.truncated {
                tracing::warn!(
                    limit = MAX_RECLAIMED_TEMP_ENTRIES_PER_START,
                    "stopped reclaiming exited sidecar temp directories at the per-start limit; the next sidecar start reclaims the rest"
                );
            }
        })
    {
        tracing::warn!(%error, "could not queue reclaim of exited sidecar temp directories");
    }
}

/// Remove this process's shared temp state on a clean exit. VM directories are
/// removed by VM disposal; this covers the process-wide import-cache assets.
pub(crate) fn remove_process_temp_state() {
    let root = node_import_cache_process_root();
    if let Err(error) = fs::remove_dir_all(&root) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %root.display(), "could not remove sidecar import cache root");
        }
    }
    if let Some(lock) = PROCESS_LIVENESS_LOCK.get() {
        // Another sidecar with the same PID in another PID namespace may share
        // the file. Remove it only when the exclusive lock shows no one does.
        match lock.relock(FlockArg::LockExclusiveNonblock) {
            Ok(()) => {
                let path = liveness_lock_path(&std::env::temp_dir(), std::process::id());
                if let Err(error) = fs::remove_file(&path) {
                    if error.kind() != io::ErrorKind::NotFound {
                        tracing::warn!(%error, path = %path.display(), "could not remove sidecar liveness lock");
                    }
                }
            }
            Err(Errno::EWOULDBLOCK) => {}
            Err(errno) => {
                tracing::warn!(%errno, "could not check sidecar liveness lock before removing it");
            }
        }
    }
}

fn liveness_lock_path(temp_dir: &Path, pid: u32) -> PathBuf {
    temp_dir.join(format!("{LIVENESS_LOCK_PREFIX}{pid}{LIVENESS_LOCK_SUFFIX}"))
}

fn hold_process_liveness_lock(temp_dir: &Path) -> io::Result<()> {
    if PROCESS_LIVENESS_LOCK.get().is_some() {
        return Ok(());
    }
    let path = liveness_lock_path(temp_dir, std::process::id());
    // A sweeper may unlink a stale file of the same name between open and lock;
    // only a lock on the file the path still names counts. A sweeper holds the
    // exclusive lock only while it unlinks, so a blocked attempt retries.
    for _ in 0..8 {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        let lock = match Flock::lock(file, FlockArg::LockSharedNonblock) {
            Ok(lock) => lock,
            Err((_, Errno::EWOULDBLOCK)) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            Err((_, errno)) => return Err(io::Error::from(errno)),
        };
        let locked_inode = lock.metadata()?.ino();
        match fs::metadata(&path) {
            Ok(metadata) if metadata.ino() == locked_inode => {
                if PROCESS_LIVENESS_LOCK.set(lock).is_err() {
                    tracing::debug!("sidecar liveness lock was already held");
                }
                return Ok(());
            }
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "liveness lock {} kept being replaced",
        path.display()
    )))
}

pub(crate) fn reclaim_dead_sidecar_temp_dirs(temp_dir: &Path) -> ReclaimReport {
    let mut report = ReclaimReport::default();
    let entries = match fs::read_dir(temp_dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, temp_dir = %temp_dir.display(), "could not scan temp directory for exited sidecar state");
            report.failed += 1;
            return report;
        }
    };
    let own_pid = std::process::id();
    let mut owners = std::collections::BTreeMap::<u32, bool>::new();
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(owner_pid) else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let alive = *owners
            .entry(pid)
            .or_insert_with(|| owner_is_alive(temp_dir, pid));
        if alive {
            continue;
        }
        if report.removed + report.failed >= MAX_RECLAIMED_TEMP_ENTRIES_PER_START {
            report.truncated = true;
            break;
        }
        let path = entry.path();
        match remove_owned_entry(&path) {
            Ok(()) => report.removed += 1,
            Err(error) => {
                report.failed += 1;
                tracing::warn!(%error, path = %path.display(), "could not remove temp directory of exited sidecar");
            }
        }
    }
    report
}

fn owner_pid(name: &str) -> Option<u32> {
    let rest = OWNED_TEMP_ENTRY_PREFIXES
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))?;
    let digits_end = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    if digits_end == 0 || !(rest[digits_end..].is_empty() || rest[digits_end..].starts_with('-')) {
        return None;
    }
    rest[..digits_end].parse().ok().filter(|pid| *pid > 0)
}

fn owner_is_alive(temp_dir: &Path, pid: u32) -> bool {
    let lock_path = liveness_lock_path(temp_dir, pid);
    match File::open(&lock_path) {
        Ok(file) => match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(_stale) => {
                // The owner is gone. Unlink while holding the lock so a new
                // owner that opened this file sees it replaced and retries.
                if let Err(error) = fs::remove_file(&lock_path) {
                    if error.kind() != io::ErrorKind::NotFound {
                        tracing::warn!(%error, path = %lock_path.display(), "could not remove stale sidecar liveness lock");
                    }
                }
                false
            }
            Err((_, Errno::EWOULDBLOCK)) => true,
            Err((_, errno)) => {
                tracing::warn!(%errno, path = %lock_path.display(), "could not probe sidecar liveness lock; keeping its directories");
                true
            }
        },
        // No liveness file: the owner predates the lock scheme or could not take
        // its lock. Its PID proves nothing across PID namespaces, so keep it.
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(%error, path = %lock_path.display(), "could not open sidecar liveness lock; keeping its directories");
            true
        }
    }
}

fn remove_owned_entry(path: &Path) -> io::Result<()> {
    // Never follow a symlink planted under the temp directory.
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else if metadata.is_file() {
        fs::remove_file(path)
    } else {
        return Err(io::Error::other(format!(
            "{} is neither a file nor a directory",
            path.display()
        )));
    };
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
