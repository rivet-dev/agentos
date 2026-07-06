use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use vfs::engine::error::{VfsError, VfsResult};
use vfs::engine::mem::metadata_store::MetadataDump;
use vfs::engine::mem::InMemoryMetadataStore;
use vfs::engine::metadata::MetadataStore;
use vfs::engine::types::{
    BlockKey, ChunkEdit, ChunkRange, ChunkRef, CreateInodeAttrs, DentryStat, InodeMeta, InodePatch,
    InodeType, SnapshotId, Storage, Timespec, DEFAULT_CHUNK_SIZE,
};

pub struct SqliteMetadataStore {
    connection: Mutex<Connection>,
    inner: InMemoryMetadataStore,
}

impl SqliteMetadataStore {
    pub fn open(path: impl AsRef<Path>) -> VfsResult<Self> {
        let connection = Connection::open(path)
            .map_err(|err| VfsError::eio(format!("open SQLite metadata store: {err}")))?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> VfsResult<Self> {
        let connection = Connection::open_in_memory()
            .map_err(|err| VfsError::eio(format!("open in-memory SQLite metadata store: {err}")))?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> VfsResult<Self> {
        install_schema(&connection)?;
        let dump = load_dump(&connection)?;
        let inner = dump
            .map(InMemoryMetadataStore::from_dump)
            .unwrap_or_default();
        if load_dump(&connection)?.is_none() {
            persist_dump(&mut connection, &inner.dump())?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            inner,
        })
    }

    pub fn has_schema(&self) -> VfsResult<bool> {
        let connection = self.connection.lock().expect("sqlite mutex poisoned");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('inodes', 'dentries', 'chunks', 'block_refs', 'snapshots')",
                [],
                |row| row.get(0),
            )
            .map_err(|err| VfsError::eio(format!("inspect SQLite schema: {err}")))?;
        Ok(count == 5)
    }

    fn persist(&self) -> VfsResult<()> {
        let dump = self.inner.dump();
        let mut connection = self.connection.lock().expect("sqlite mutex poisoned");
        persist_dump(&mut connection, &dump)
    }
}

fn install_schema(connection: &Connection) -> VfsResult<()> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS inodes (
              ino INTEGER PRIMARY KEY,
              kind INTEGER NOT NULL,
              mode INTEGER NOT NULL,
              uid INTEGER NOT NULL,
              gid INTEGER NOT NULL,
              size INTEGER NOT NULL,
              nlink INTEGER NOT NULL,
              atime_ns INTEGER NOT NULL, mtime_ns INTEGER NOT NULL,
              ctime_ns INTEGER NOT NULL, birthtime_ns INTEGER NOT NULL,
              storage_mode INTEGER NOT NULL,
              storage_chunk_size INTEGER,
              inline_content BLOB,
              symlink_target TEXT
            );
            CREATE TABLE IF NOT EXISTS dentries (
              parent_ino INTEGER NOT NULL,
              name TEXT NOT NULL,
              child_ino INTEGER NOT NULL,
              kind INTEGER NOT NULL,
              PRIMARY KEY (parent_ino, name)
            );
            CREATE INDEX IF NOT EXISTS dentries_parent ON dentries(parent_ino);
            CREATE TABLE IF NOT EXISTS chunks (
              ino INTEGER NOT NULL,
              chunk_index INTEGER NOT NULL,
              block_key TEXT NOT NULL,
              len INTEGER NOT NULL,
              PRIMARY KEY (ino, chunk_index)
            );
            CREATE TABLE IF NOT EXISTS block_refs (
              block_key TEXT PRIMARY KEY,
              refcount INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS snapshots (
              snapshot_id INTEGER PRIMARY KEY,
              root_ino INTEGER NOT NULL,
              created_ns INTEGER NOT NULL
            );
            ",
        )
        .map_err(|err| VfsError::eio(format!("install SQLite metadata schema: {err}")))
}

fn load_dump(connection: &Connection) -> VfsResult<Option<MetadataDump>> {
    let inode_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM inodes", [], |row| row.get(0))
        .map_err(|err| VfsError::eio(format!("count SQLite inodes: {err}")))?;
    if inode_count == 0 {
        return Ok(None);
    }

    let mut inodes = BTreeMap::new();
    let mut next_ino = 1;
    let mut statement = connection
        .prepare(
            "SELECT ino, kind, mode, uid, gid, size, nlink, atime_ns, mtime_ns, ctime_ns,
                    birthtime_ns, storage_mode, storage_chunk_size, inline_content, symlink_target
             FROM inodes",
        )
        .map_err(|err| VfsError::eio(format!("prepare inode load: {err}")))?;
    let rows = statement
        .query_map([], |row| {
            let ino: u64 = row.get(0)?;
            let kind_id: i64 = row.get(1)?;
            let storage_id: i64 = row.get(11)?;
            let chunk_size: Option<u32> = row.get(12)?;
            let inline_content: Option<Vec<u8>> = row.get(13)?;
            let symlink_target: Option<String> = row.get(14)?;
            let kind = match kind_id {
                0 => InodeType::File,
                1 => InodeType::Directory,
                _ => InodeType::Symlink,
            };
            let storage = match storage_id {
                1 => Storage::Inline(inline_content.unwrap_or_default()),
                2 => Storage::Chunked {
                    chunk_size: chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE),
                },
                _ => Storage::None,
            };
            Ok(InodeMeta {
                ino,
                kind,
                mode: row.get(2)?,
                uid: row.get(3)?,
                gid: row.get(4)?,
                size: row.get(5)?,
                nlink: row.get(6)?,
                atime: ns_to_timespec(row.get(7)?),
                mtime: ns_to_timespec(row.get(8)?),
                ctime: ns_to_timespec(row.get(9)?),
                birthtime: ns_to_timespec(row.get(10)?),
                storage,
                symlink_target,
            })
        })
        .map_err(|err| VfsError::eio(format!("load SQLite inodes: {err}")))?;
    for row in rows {
        let meta = row.map_err(|err| VfsError::eio(format!("load SQLite inode row: {err}")))?;
        next_ino = next_ino.max(meta.ino + 1);
        inodes.insert(meta.ino, meta);
    }

    let mut dentries = BTreeMap::new();
    let mut statement = connection
        .prepare("SELECT parent_ino, name, child_ino FROM dentries")
        .map_err(|err| VfsError::eio(format!("prepare dentry load: {err}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                (row.get::<_, u64>(0)?, row.get::<_, String>(1)?),
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|err| VfsError::eio(format!("load SQLite dentries: {err}")))?;
    for row in rows {
        let (key, value) =
            row.map_err(|err| VfsError::eio(format!("load SQLite dentry row: {err}")))?;
        dentries.insert(key, value);
    }

    let mut chunks = BTreeMap::new();
    let mut statement = connection
        .prepare("SELECT ino, chunk_index, block_key, len FROM chunks")
        .map_err(|err| VfsError::eio(format!("prepare chunk load: {err}")))?;
    let rows = statement
        .query_map([], |row| {
            let index = row.get::<_, u64>(1)?;
            Ok((
                (row.get::<_, u64>(0)?, index),
                ChunkRef {
                    index,
                    key: BlockKey(row.get(2)?),
                    len: row.get(3)?,
                },
            ))
        })
        .map_err(|err| VfsError::eio(format!("load SQLite chunks: {err}")))?;
    for row in rows {
        let (key, value) =
            row.map_err(|err| VfsError::eio(format!("load SQLite chunk row: {err}")))?;
        chunks.insert(key, value);
    }

    let mut block_refs = BTreeMap::new();
    let mut statement = connection
        .prepare("SELECT block_key, refcount FROM block_refs")
        .map_err(|err| VfsError::eio(format!("prepare block ref load: {err}")))?;
    let rows = statement
        .query_map([], |row| Ok((BlockKey(row.get(0)?), row.get::<_, u64>(1)?)))
        .map_err(|err| VfsError::eio(format!("load SQLite block refs: {err}")))?;
    for row in rows {
        let (key, value) =
            row.map_err(|err| VfsError::eio(format!("load SQLite block ref row: {err}")))?;
        block_refs.insert(key, value);
    }

    Ok(Some(MetadataDump {
        next_ino,
        inodes,
        dentries,
        chunks,
        block_refs,
    }))
}

fn persist_dump(connection: &mut Connection, dump: &MetadataDump) -> VfsResult<()> {
    let tx = connection
        .transaction()
        .map_err(|err| VfsError::eio(format!("begin SQLite metadata transaction: {err}")))?;
    tx.execute_batch(
        "
        DELETE FROM snapshots;
        DELETE FROM block_refs;
        DELETE FROM chunks;
        DELETE FROM dentries;
        DELETE FROM inodes;
        ",
    )
    .map_err(|err| VfsError::eio(format!("clear SQLite metadata tables: {err}")))?;

    for meta in dump.inodes.values() {
        let (storage_mode, storage_chunk_size, inline_content) = match &meta.storage {
            Storage::None => (0, None, None),
            Storage::Inline(data) => (1, None, Some(data.as_slice())),
            Storage::Chunked { chunk_size } => (2, Some(*chunk_size), None),
        };
        tx.execute(
            "INSERT INTO inodes
             (ino, kind, mode, uid, gid, size, nlink, atime_ns, mtime_ns, ctime_ns, birthtime_ns,
              storage_mode, storage_chunk_size, inline_content, symlink_target)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                meta.ino,
                kind_id(meta.kind),
                meta.mode,
                meta.uid,
                meta.gid,
                meta.size,
                meta.nlink,
                timespec_to_ns(meta.atime),
                timespec_to_ns(meta.mtime),
                timespec_to_ns(meta.ctime),
                timespec_to_ns(meta.birthtime),
                storage_mode,
                storage_chunk_size,
                inline_content,
                meta.symlink_target,
            ],
        )
        .map_err(|err| VfsError::eio(format!("persist SQLite inode {}: {err}", meta.ino)))?;
    }

    for ((parent, name), child) in &dump.dentries {
        let kind = dump
            .inodes
            .get(child)
            .map(|meta| meta.kind)
            .ok_or_else(|| VfsError::eio(format!("dentry points to missing inode {child}")))?;
        tx.execute(
            "INSERT INTO dentries (parent_ino, name, child_ino, kind) VALUES (?, ?, ?, ?)",
            params![parent, name, child, kind_id(kind)],
        )
        .map_err(|err| VfsError::eio(format!("persist SQLite dentry {name}: {err}")))?;
    }

    for ((ino, index), chunk) in &dump.chunks {
        tx.execute(
            "INSERT INTO chunks (ino, chunk_index, block_key, len) VALUES (?, ?, ?, ?)",
            params![ino, index, chunk.key.0, chunk.len],
        )
        .map_err(|err| VfsError::eio(format!("persist SQLite chunk {ino}/{index}: {err}")))?;
    }

    for (key, refcount) in &dump.block_refs {
        tx.execute(
            "INSERT INTO block_refs (block_key, refcount) VALUES (?, ?)",
            params![key.0, refcount],
        )
        .map_err(|err| VfsError::eio(format!("persist SQLite block ref {}: {err}", key.0)))?;
    }

    tx.commit()
        .map_err(|err| VfsError::eio(format!("commit SQLite metadata transaction: {err}")))
}

fn kind_id(kind: InodeType) -> i64 {
    match kind {
        InodeType::File => 0,
        InodeType::Directory => 1,
        InodeType::Symlink => 2,
    }
}

fn timespec_to_ns(time: Timespec) -> i64 {
    time.sec.saturating_mul(1_000_000_000) + i64::from(time.nsec)
}

fn ns_to_timespec(ns: i64) -> Timespec {
    Timespec {
        sec: ns / 1_000_000_000,
        nsec: ns.rem_euclid(1_000_000_000) as u32,
    }
}

#[async_trait]
impl MetadataStore for SqliteMetadataStore {
    async fn resolve(&self, path: &str) -> VfsResult<InodeMeta> {
        self.inner.resolve(path).await
    }

    async fn resolve_parent(&self, path: &str) -> VfsResult<(InodeMeta, String)> {
        self.inner.resolve_parent(path).await
    }

    async fn lstat(&self, path: &str) -> VfsResult<InodeMeta> {
        self.inner.lstat(path).await
    }

    async fn list_dir(&self, ino: u64) -> VfsResult<Vec<DentryStat>> {
        self.inner.list_dir(ino).await
    }

    async fn create(
        &self,
        parent: u64,
        name: &str,
        attrs: CreateInodeAttrs,
    ) -> VfsResult<InodeMeta> {
        let result = self.inner.create(parent, name, attrs).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn link(&self, parent: u64, name: &str, target: u64) -> VfsResult<()> {
        let result = self.inner.link(parent, name, target).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn remove(&self, parent: u64, name: &str) -> VfsResult<Vec<BlockKey>> {
        let result = self.inner.remove(parent, name).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn rename(
        &self,
        src_parent: u64,
        src: &str,
        dst_parent: u64,
        dst: &str,
    ) -> VfsResult<Vec<BlockKey>> {
        let result = self.inner.rename(src_parent, src, dst_parent, dst).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn set_attr(&self, ino: u64, patch: InodePatch) -> VfsResult<Vec<BlockKey>> {
        let result = self.inner.set_attr(ino, patch).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn commit_write(
        &self,
        ino: u64,
        edits: Vec<ChunkEdit>,
        new_size: u64,
    ) -> VfsResult<Vec<BlockKey>> {
        let result = self.inner.commit_write(ino, edits, new_size).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn get_chunks(&self, ino: u64, range: ChunkRange) -> VfsResult<Vec<ChunkRef>> {
        self.inner.get_chunks(ino, range).await
    }

    async fn snapshot(&self, root: u64) -> VfsResult<SnapshotId> {
        self.inner.snapshot(root).await
    }

    async fn fork(&self, snap: SnapshotId) -> VfsResult<u64> {
        let result = self.inner.fork(snap).await;
        if result.is_ok() {
            self.persist()?;
        }
        result
    }

    async fn gc(&self) -> VfsResult<Vec<BlockKey>> {
        self.inner.gc().await
    }
}
