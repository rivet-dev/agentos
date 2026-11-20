//! The V8 snapshot disk cache runs code it loads, so it must load only from a
//! directory that the sidecar user owns and nobody else can write.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use agentos_executor_v8_runtime::snapshot::SnapshotCache;
use agentos_executor_v8_runtime::snapshot_disk_cache::SNAPSHOT_CACHE_DIR_ENV;

fn snap_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .expect("list snapshot cache")
        .map(|entry| entry.expect("cache entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "snap"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .expect("cache file mtime")
}

fn set_modified(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(time))
        .expect("set cache file mtime");
}

fn bridge(marker: u32) -> String {
    format!("(function() {{ globalThis.__diskCacheMarker = {marker}; }})();")
}

// One test per process: the cache directory comes from the environment.
#[test]
fn snapshot_disk_cache_loads_only_from_a_private_directory() {
    let root =
        std::env::temp_dir().join(format!("agentos-v8-disk-cache-test-{}", std::process::id()));
    let dir = root.join("v8-snapshots");
    std::env::set_var(SNAPSHOT_CACHE_DIR_ENV, &dir);

    SnapshotCache::new(4)
        .get_or_create(&bridge(1))
        .expect("build and store the first snapshot");
    let stored = snap_files(&dir);
    assert_eq!(stored.len(), 1, "the first build stores one blob");
    let first = stored[0].clone();
    assert_eq!(
        fs::metadata(&dir).expect("cache dir").permissions().mode() & 0o777,
        0o700
    );

    // A writer that died mid-store leaves a temp file; the next store removes
    // it once it is old, and keeps one that a live writer may still use.
    let stale = dir.join("dead.tmp-1-1");
    let fresh = dir.join("live.tmp-2-2");
    fs::write(&stale, b"partial").expect("seed stale temp file");
    fs::write(&fresh, b"partial").expect("seed fresh temp file");
    set_modified(&stale, SystemTime::now() - Duration::from_secs(3600));
    SnapshotCache::new(4)
        .get_or_create(&bridge(2))
        .expect("build and store a second snapshot");
    assert!(!stale.exists(), "a stale temp file is evicted");
    assert!(fresh.exists(), "a recent temp file is kept");
    let stored = snap_files(&dir);
    assert_eq!(stored.len(), 2);

    // A loaded blob gets its mtime refreshed for LRU eviction, so an unchanged
    // mtime shows the blob was not loaded.
    let old = SystemTime::now() - Duration::from_secs(3600);
    set_modified(&first, old);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).expect("open up cache dir");
    SnapshotCache::new(4)
        .get_or_create(&bridge(1))
        .expect("a snapshot still builds without the disk cache");
    assert_eq!(
        modified(&first),
        old,
        "a blob in a directory others can write must not be loaded"
    );
    SnapshotCache::new(4)
        .get_or_create(&bridge(3))
        .expect("build a third snapshot");
    assert_eq!(
        snap_files(&dir),
        stored,
        "nothing is stored into a directory others can write"
    );

    // A blob file others can write is refused even in a private directory,
    // and the rebuilt snapshot replaces it with a private file. A load would
    // keep the open mode.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("restore cache dir");
    fs::set_permissions(&first, fs::Permissions::from_mode(0o666)).expect("open up blob");
    SnapshotCache::new(4)
        .get_or_create(&bridge(1))
        .expect("a snapshot still builds without the blob");
    assert_eq!(
        fs::metadata(&first).expect("blob").permissions().mode() & 0o777,
        0o600,
        "a blob others can write must be replaced, not loaded"
    );

    // A private blob in a private directory is loaded.
    set_modified(&first, old);
    SnapshotCache::new(4)
        .get_or_create(&bridge(1))
        .expect("load the cached snapshot");
    assert!(modified(&first) > old, "a private blob is loaded again");

    fs::remove_dir_all(&root).expect("remove test cache root");
}
