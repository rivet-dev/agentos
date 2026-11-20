//! Persistent cache of V8 startup snapshot blobs.
//!
//! Building the bridge snapshot takes a helper process ~400 ms, which every
//! new sidecar process used to pay on its first guest Node process. Blobs are
//! stored under a directory owned by the sidecar user and keyed by this
//! build's source fingerprint plus the snapshot cache key (bridge source and
//! userland), so a blob is only ever loaded by the build that produced it.
//!
//! Trust: a blob is code the sidecar runs, so the cache is used only when the
//! directory and the blob file are owned by the sidecar's user and are not
//! writable by its group or by others. Otherwise the sidecar warns with
//! `ERR_AGENTOS_V8_SNAPSHOT_CACHE_UNSAFE_DIR` and builds snapshots in memory
//! only. The SHA-256 in each file detects corruption and truncation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use std::sync::Arc;

use crate::snapshot::SnapshotCacheKey;

/// Directory for cached blobs. Empty or `off` disables the cache. Defaults to
/// `$XDG_CACHE_HOME/agentos/v8-snapshots`, then `$HOME/.cache/...`.
pub const SNAPSHOT_CACHE_DIR_ENV: &str = "AGENTOS_V8_SNAPSHOT_CACHE_DIR";
/// Cached blobs kept per directory; the least recently used go first.
pub const MAX_SNAPSHOT_CACHE_FILES: usize = 16;
pub const SNAPSHOT_CACHE_ERROR_CODE: &str = "ERR_AGENTOS_V8_SNAPSHOT_CACHE";
pub const SNAPSHOT_CACHE_UNSAFE_DIR_CODE: &str = "ERR_AGENTOS_V8_SNAPSHOT_CACHE_UNSAFE_DIR";
/// Temp files older than this are left by a writer that died mid-store.
const STALE_TEMP_FILE_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

const MAGIC: &[u8; 8] = b"AOSV8SC1";
const HEADER_LEN: usize = 8 + 32 + 32 + 8;
const BUILD_FINGERPRINT: &str = env!("AGENTOS_V8_RUNTIME_FINGERPRINT");

pub(crate) fn cache_dir() -> Option<PathBuf> {
    match std::env::var_os(SNAPSHOT_CACHE_DIR_ENV) {
        Some(value) if value.is_empty() || value == "off" => None,
        Some(value) => Some(PathBuf::from(value)),
        // Unit and crate tests must not write into the user's cache.
        None if cfg!(any(test, feature = "test-support")) => None,
        None => {
            let base = std::env::var_os("XDG_CACHE_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .filter(|value| !value.is_empty())
                        .map(|home| PathBuf::from(home).join(".cache"))
                })?;
            Some(base.join("agentos").join("v8-snapshots"))
        }
    }
}

fn file_key(key: &SnapshotCacheKey) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(BUILD_FINGERPRINT.as_bytes());
    hasher.update([u8::from(crate::isolate::wasm_exnref_enabled())]);
    hasher.update(key);
    hasher.finalize().into()
}

fn file_path(dir: &Path, file_key: &[u8; 32]) -> PathBuf {
    let name: String = file_key.iter().map(|byte| format!("{byte:02x}")).collect();
    dir.join(format!("{name}.snap"))
}

/// Load a cached blob. A missing file is a plain miss; a damaged one is
/// logged and removed.
pub(crate) fn load(dir: &Path, key: &SnapshotCacheKey, max_blob_bytes: usize) -> Option<Arc<[u8]>> {
    if !dir_is_private(dir) {
        return None;
    }
    let file_key = file_key(key);
    let path = file_path(dir, &file_key);
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot read cached V8 snapshot");
            return None;
        }
    };
    // Check the opened file itself, so a swapped path cannot pass the check.
    match file.metadata() {
        Ok(metadata) if is_private(&metadata) => {}
        Ok(_) => {
            warn_unsafe(
                &path,
                "the cached snapshot is not owned by this user or is writable by others",
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot inspect cached V8 snapshot");
            return None;
        }
    }
    let mut bytes = Vec::new();
    if let Err(error) = std::io::Read::read_to_end(&mut file, &mut bytes) {
        tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot read cached V8 snapshot");
        return None;
    }
    match parse(&bytes, &file_key, max_blob_bytes) {
        Ok(blob) => {
            // Recency for eviction.
            if let Err(error) = fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(SystemTime::now()))
            {
                tracing::debug!(path = %path.display(), %error, "cannot refresh cached V8 snapshot mtime");
            }
            Some(Arc::from(blob))
        }
        Err(reason) => {
            tracing::warn!(path = %path.display(), reason, "{SNAPSHOT_CACHE_ERROR_CODE}: discarding damaged cached V8 snapshot");
            if let Err(error) = fs::remove_file(&path) {
                tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot remove damaged cached V8 snapshot");
            }
            None
        }
    }
}

fn parse<'a>(
    bytes: &'a [u8],
    file_key: &[u8; 32],
    max_blob_bytes: usize,
) -> Result<&'a [u8], &'static str> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return Err("bad header");
    }
    if &bytes[8..40] != file_key {
        return Err("key mismatch");
    }
    let len = u64::from_le_bytes(bytes[72..80].try_into().expect("8 bytes"));
    let blob = &bytes[HEADER_LEN..];
    if len as usize != blob.len() || blob.len() > max_blob_bytes {
        return Err("length mismatch");
    }
    let digest: [u8; 32] = Sha256::digest(blob).into();
    if digest[..] != bytes[40..72] {
        return Err("checksum mismatch");
    }
    Ok(blob)
}

/// Store a blob atomically (temp file and rename), then evict the oldest
/// files beyond the bound. Failures are logged; the in-memory snapshot is
/// still used.
pub(crate) fn store(dir: &Path, key: &SnapshotCacheKey, blob: &[u8]) {
    if let Err(error) = store_inner(dir, key, blob) {
        tracing::warn!(dir = %dir.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot store V8 snapshot; set {SNAPSHOT_CACHE_DIR_ENV} to a writable directory or to off");
        return;
    }
    evict(dir);
}

fn store_inner(dir: &Path, key: &SnapshotCacheKey, blob: &[u8]) -> std::io::Result<()> {
    create_private_dir(dir)?;
    if !dir_is_private(dir) {
        return Ok(());
    }
    let file_key = file_key(key);
    let path = file_path(dir, &file_key);
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let temp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut options = fs::File::options();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temp)?;
        file.write_all(MAGIC)?;
        file.write_all(&file_key)?;
        file.write_all(&Sha256::digest(blob))?;
        file.write_all(&(blob.len() as u64).to_le_bytes())?;
        file.write_all(blob)?;
        file.sync_all()?;
        fs::rename(&temp, &path)
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temp) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %temp.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot remove partial V8 snapshot");
            }
        }
    }
    result
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

/// Owned by this process's user and not writable by group or others.
#[cfg(unix)]
fn is_private(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let euid = unsafe { libc::geteuid() };
    metadata.uid() == euid && metadata.mode() & 0o022 == 0
}

#[cfg(not(unix))]
fn is_private(_metadata: &fs::Metadata) -> bool {
    true
}

/// Whether the cache directory is safe to load code from. A missing directory
/// is a plain miss, not a warning.
fn dir_is_private(dir: &Path) -> bool {
    match fs::metadata(dir) {
        Ok(metadata) if metadata.is_dir() && is_private(&metadata) => true,
        Ok(_) => {
            warn_unsafe(
                dir,
                "the cache directory is not owned by this user or is writable by others",
            );
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            tracing::warn!(dir = %dir.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot inspect V8 snapshot cache directory");
            false
        }
    }
}

fn warn_unsafe(path: &Path, reason: &str) {
    // Once per process: every snapshot lookup would repeat it.
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!(path = %path.display(), reason, "{SNAPSHOT_CACHE_UNSAFE_DIR_CODE}: not using the V8 snapshot disk cache; make it owned by the sidecar user with mode 0700, or set {SNAPSHOT_CACHE_DIR_ENV} to another directory or to off");
        eprintln!("{SNAPSHOT_CACHE_UNSAFE_DIR_CODE}: not using the V8 snapshot disk cache at {}: {reason}", path.display());
    }
}

fn evict(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(dir = %dir.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot list V8 snapshot cache");
            return;
        }
    };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if extension == "snap" {
            files.push((modified, path));
        } else if extension.starts_with("tmp-")
            && now
                .duration_since(modified)
                .is_ok_and(|age| age > STALE_TEMP_FILE_AGE)
        {
            if let Err(error) = fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot remove stale partial V8 snapshot");
                }
            }
        }
    }
    if files.len() <= MAX_SNAPSHOT_CACHE_FILES {
        return;
    }
    files.sort();
    let excess = files.len() - MAX_SNAPSHOT_CACHE_FILES;
    for (_, path) in files.into_iter().take(excess) {
        if let Err(error) = fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), %error, "{SNAPSHOT_CACHE_ERROR_CODE}: cannot evict cached V8 snapshot");
        }
    }
}
