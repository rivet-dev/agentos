use async_trait::async_trait;
use std::sync::{Arc, Condvar, Mutex};
use vfs::engine::engines::{ChunkedFs, ChunkedFsOptions, ObjectFs};
use vfs::engine::mem::{InMemoryMetadataStore, MemoryBlockStore, MemoryObjectBackend};
use vfs::engine::{
    BlockKey, CachedMetadataStore, ChunkEdit, ChunkRange, CreateInodeAttrs, InodePatch, InodeType,
    MetadataStore, SnapshotId, Storage, VfsResult, VirtualFileSystem,
};

#[tokio::test]
async fn chunked_fs_round_trips_inline_and_chunked_files() {
    let metadata = InMemoryMetadataStore::new();
    let blocks = MemoryBlockStore::new();
    let fs = ChunkedFs::with_options(
        metadata,
        blocks.clone(),
        ChunkedFsOptions {
            inline_threshold: 4,
            chunk_size: 3,
            ..ChunkedFsOptions::default()
        },
    );

    fs.write_file("/small.txt", b"abc").await.unwrap();
    assert_eq!(fs.read_file("/small.txt").await.unwrap(), b"abc");
    assert_eq!(blocks.len(), 0);

    fs.write_file("/large.txt", b"abcdefghi").await.unwrap();
    assert_eq!(fs.read_file("/large.txt").await.unwrap(), b"abcdefghi");
    assert_eq!(blocks.len(), 3);

    fs.pwrite("/large.txt", b"ZZ", 2).await.unwrap();
    assert_eq!(fs.read_file("/large.txt").await.unwrap(), b"abZZefghi");
}

#[tokio::test]
async fn chunked_fs_partial_writes_preserve_untouched_chunks() {
    let metadata = InMemoryMetadataStore::new();
    let blocks = MemoryBlockStore::new();
    let fs = ChunkedFs::with_options(
        metadata.clone(),
        blocks,
        ChunkedFsOptions {
            inline_threshold: 1,
            chunk_size: 4,
            ..ChunkedFsOptions::default()
        },
    );

    fs.write_file("/large.txt", b"aaaabbbbcccc").await.unwrap();
    let ino = metadata.resolve("/large.txt").await.unwrap().ino;
    let before = metadata.get_chunks(ino, ChunkRange::all()).await.unwrap();
    assert_eq!(before.len(), 3);

    fs.pwrite("/large.txt", b"Z", 5).await.unwrap();

    assert_eq!(fs.read_file("/large.txt").await.unwrap(), b"aaaabZbbcccc");
    let after = metadata.get_chunks(ino, ChunkRange::all()).await.unwrap();
    assert_eq!(after.len(), 3);
    assert_eq!(after[0].key, before[0].key);
    assert_ne!(after[1].key, before[1].key);
    assert_eq!(after[2].key, before[2].key);
}

#[tokio::test]
async fn chunked_fs_sparse_pwrite_and_truncate_zero_fill_holes() {
    let fs = ChunkedFs::with_options(
        InMemoryMetadataStore::new(),
        MemoryBlockStore::new(),
        ChunkedFsOptions {
            inline_threshold: 1,
            chunk_size: 4,
            ..ChunkedFsOptions::default()
        },
    );

    fs.write_file("/sparse", b"").await.unwrap();
    fs.pwrite("/sparse", b"end", 10).await.unwrap();
    assert_eq!(
        fs.read_file("/sparse").await.unwrap(),
        b"\0\0\0\0\0\0\0\0\0\0end"
    );

    fs.write_file("/truncate", b"abcdefgh").await.unwrap();
    fs.truncate("/truncate", 5).await.unwrap();
    fs.truncate("/truncate", 8).await.unwrap();
    assert_eq!(fs.read_file("/truncate").await.unwrap(), b"abcde\0\0\0");
}

#[tokio::test]
async fn chunked_fs_dedups_identical_content_and_gc_deletes_on_unlink() {
    let metadata = InMemoryMetadataStore::new();
    let blocks = MemoryBlockStore::new();
    let fs = ChunkedFs::with_options(
        metadata.clone(),
        blocks.clone(),
        ChunkedFsOptions {
            inline_threshold: 1,
            chunk_size: 16,
            ..ChunkedFsOptions::default()
        },
    );

    fs.write_file("/a", b"same content").await.unwrap();
    fs.write_file("/b", b"same content").await.unwrap();
    let key = BlockKey::from_content(b"same content");
    assert_eq!(blocks.len(), 1);
    assert_eq!(metadata.refcount(&key), 2);

    fs.remove_file("/a").await.unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(metadata.refcount(&key), 1);

    fs.remove_file("/b").await.unwrap();
    assert_eq!(blocks.len(), 0);
    assert_eq!(metadata.refcount(&key), 0);
}

#[tokio::test]
async fn metadata_snapshot_and_fork_share_chunk_refs_until_cow_write() {
    let metadata = InMemoryMetadataStore::new();
    let parent = metadata.resolve("/").await.unwrap();
    let file = metadata
        .create(
            parent.ino,
            "file",
            CreateInodeAttrs::file(0o644, 0, 0, Storage::Chunked { chunk_size: 16 }),
        )
        .await
        .unwrap();
    let key = BlockKey::from_content(b"hello");
    metadata
        .commit_write(
            file.ino,
            vec![ChunkEdit {
                index: 0,
                key: key.clone(),
                len: 5,
            }],
            5,
        )
        .await
        .unwrap();
    assert_eq!(metadata.refcount(&key), 1);

    let snap = metadata.snapshot(parent.ino).await.unwrap();
    assert_eq!(metadata.refcount(&key), 2);
    let fork_root = metadata.fork(snap).await.unwrap();
    assert_eq!(metadata.refcount(&key), 3);

    let fork_entries = metadata.list_dir(fork_root).await.unwrap();
    let fork_file = fork_entries
        .iter()
        .find(|entry| entry.name == "file")
        .unwrap()
        .meta
        .ino;
    let new_key = BlockKey::from_content(b"goodbye");
    metadata
        .commit_write(
            fork_file,
            vec![ChunkEdit {
                index: 0,
                key: new_key.clone(),
                len: 7,
            }],
            7,
        )
        .await
        .unwrap();
    assert_eq!(metadata.refcount(&key), 2);
    assert_eq!(metadata.refcount(&new_key), 1);
}

#[tokio::test]
async fn object_fs_maps_files_to_native_objects() {
    let backend = MemoryObjectBackend::new();
    let fs = ObjectFs::new(backend);
    fs.mkdir("/dir", false).await.unwrap();
    fs.write_file("/dir/file.txt", b"hello").await.unwrap();

    assert_eq!(fs.read_file("/dir/file.txt").await.unwrap(), b"hello");
    assert_eq!(fs.read_dir("/dir").await.unwrap(), vec!["file.txt"]);
    assert_eq!(fs.pread("/dir/file.txt", 1, 3).await.unwrap(), b"ell");
}

#[tokio::test]
async fn object_fs_recursively_renames_prefix_directories() {
    let backend = MemoryObjectBackend::new();
    let fs = ObjectFs::new(backend);
    fs.mkdir("/src/nested", true).await.unwrap();
    fs.write_file("/src/root.txt", b"root").await.unwrap();
    fs.write_file("/src/nested/leaf.txt", b"leaf")
        .await
        .unwrap();

    fs.rename("/src", "/dst").await.unwrap();

    assert!(!fs.exists("/src/root.txt").await);
    assert_eq!(fs.read_file("/dst/root.txt").await.unwrap(), b"root");
    assert_eq!(fs.read_file("/dst/nested/leaf.txt").await.unwrap(), b"leaf");
}

#[tokio::test]
async fn cache_invalidates_on_mutation() {
    let metadata = CachedMetadataStore::new(InMemoryMetadataStore::new(), 16);
    let root = metadata.resolve("/").await.unwrap();
    assert!(metadata.resolve("/new").await.is_err());
    metadata
        .create(root.ino, "new", CreateInodeAttrs::directory(0o755, 0, 0))
        .await
        .unwrap();
    assert_eq!(
        metadata.resolve("/new").await.unwrap().kind,
        InodeType::Directory
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cache_does_not_store_stale_read_when_mutation_wins_race() {
    let inner = InMemoryMetadataStore::new();
    let root = inner.resolve("/").await.unwrap();
    inner
        .create(
            root.ino,
            "stale",
            CreateInodeAttrs::file(0o644, 0, 0, Storage::Inline(Vec::new())),
        )
        .await
        .unwrap();
    let gate = Arc::new(RaceGate::default());
    let metadata = Arc::new(CachedMetadataStore::new(
        PausingResolveStore {
            inner,
            path: "/stale".to_string(),
            gate: gate.clone(),
        },
        16,
    ));

    let pending_metadata = metadata.clone();
    let pending = tokio::spawn(async move { pending_metadata.resolve("/stale").await });
    gate.wait_until_paused();
    metadata.remove(root.ino, "stale").await.unwrap();
    gate.release();
    assert!(pending.await.unwrap().is_ok());

    let result = metadata.resolve("/stale").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn metadata_set_attr_drops_chunk_refs_when_file_becomes_inline() {
    let metadata = InMemoryMetadataStore::new();
    let parent = metadata.resolve("/").await.unwrap();
    let file = metadata
        .create(
            parent.ino,
            "file",
            CreateInodeAttrs::file(0o644, 0, 0, Storage::Chunked { chunk_size: 16 }),
        )
        .await
        .unwrap();
    let key = BlockKey::from_content(b"hello");
    metadata
        .commit_write(
            file.ino,
            vec![ChunkEdit {
                index: 0,
                key: key.clone(),
                len: 5,
            }],
            5,
        )
        .await
        .unwrap();

    let freed = metadata
        .set_attr(
            file.ino,
            InodePatch {
                storage: Some(Storage::Inline(b"hi".to_vec())),
                size: Some(2),
                ..InodePatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(freed, vec![key]);
    assert!(metadata
        .get_chunks(file.ino, ChunkRange::all())
        .await
        .unwrap()
        .is_empty());
}

#[derive(Debug, Default)]
struct RaceGate {
    state: Mutex<RaceGateState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct RaceGateState {
    paused_once: bool,
    paused: bool,
    released: bool,
}

impl RaceGate {
    fn pause_once(&self) {
        let mut state = self.state.lock().expect("race gate poisoned");
        if state.paused_once {
            return;
        }
        state.paused_once = true;
        state.paused = true;
        self.changed.notify_all();
        while !state.released {
            state = self.changed.wait(state).expect("race gate poisoned");
        }
    }

    fn wait_until_paused(&self) {
        let mut state = self.state.lock().expect("race gate poisoned");
        while !state.paused {
            state = self.changed.wait(state).expect("race gate poisoned");
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().expect("race gate poisoned");
        state.released = true;
        self.changed.notify_all();
    }
}

#[derive(Debug, Clone)]
struct PausingResolveStore {
    inner: InMemoryMetadataStore,
    path: String,
    gate: Arc<RaceGate>,
}

#[async_trait]
impl MetadataStore for PausingResolveStore {
    async fn resolve(&self, path: &str) -> VfsResult<vfs::engine::InodeMeta> {
        let result = self.inner.resolve(path).await;
        if path == self.path {
            self.gate.pause_once();
        }
        result
    }

    async fn resolve_parent(&self, path: &str) -> VfsResult<(vfs::engine::InodeMeta, String)> {
        self.inner.resolve_parent(path).await
    }

    async fn lstat(&self, path: &str) -> VfsResult<vfs::engine::InodeMeta> {
        self.inner.lstat(path).await
    }

    async fn list_dir(&self, ino: u64) -> VfsResult<Vec<vfs::engine::DentryStat>> {
        self.inner.list_dir(ino).await
    }

    async fn create(
        &self,
        parent: u64,
        name: &str,
        attrs: CreateInodeAttrs,
    ) -> VfsResult<vfs::engine::InodeMeta> {
        self.inner.create(parent, name, attrs).await
    }

    async fn link(&self, parent: u64, name: &str, target: u64) -> VfsResult<()> {
        self.inner.link(parent, name, target).await
    }

    async fn remove(&self, parent: u64, name: &str) -> VfsResult<Vec<BlockKey>> {
        self.inner.remove(parent, name).await
    }

    async fn rename(
        &self,
        src_parent: u64,
        src: &str,
        dst_parent: u64,
        dst: &str,
    ) -> VfsResult<Vec<BlockKey>> {
        self.inner.rename(src_parent, src, dst_parent, dst).await
    }

    async fn set_attr(&self, ino: u64, patch: InodePatch) -> VfsResult<Vec<BlockKey>> {
        self.inner.set_attr(ino, patch).await
    }

    async fn commit_write(
        &self,
        ino: u64,
        edits: Vec<ChunkEdit>,
        new_size: u64,
    ) -> VfsResult<Vec<BlockKey>> {
        self.inner.commit_write(ino, edits, new_size).await
    }

    async fn get_chunks(
        &self,
        ino: u64,
        range: ChunkRange,
    ) -> VfsResult<Vec<vfs::engine::ChunkRef>> {
        self.inner.get_chunks(ino, range).await
    }

    async fn snapshot(&self, root: u64) -> VfsResult<SnapshotId> {
        self.inner.snapshot(root).await
    }

    async fn fork(&self, snap: SnapshotId) -> VfsResult<u64> {
        self.inner.fork(snap).await
    }

    async fn gc(&self) -> VfsResult<Vec<BlockKey>> {
        self.inner.gc().await
    }
}
