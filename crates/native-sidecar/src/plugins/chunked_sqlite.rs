use crate::bridge::MountPluginContext;
use crate::vm_sqlite::{
    migrate_schema, QueryResult, SharedVmSqliteDatabase, SqlStatement, SqlValue, VmSqliteMigration,
};
use agentos_kernel::mount_plugin::{
    FileSystemPluginFactory, OpenFileSystemPluginRequest, PluginError,
};
use agentos_kernel::mount_table::MountedFileSystem;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::{Mutex, OnceCell};
use vfs::adapter::MountedEngineFileSystem;
use vfs::engine::block::BlockStore;
use vfs::engine::engines::{ChunkedFs, ChunkedFsOptions};
use vfs::engine::error::{VfsError, VfsResult};
use vfs::engine::mem::metadata_store::{InMemoryMetadataStore, MetadataDump};
use vfs::engine::metadata::MetadataStore;
use vfs::engine::types::{
    BlockKey, ChunkEdit, ChunkRange, ChunkRef, CreateInodeAttrs, DentryStat, InodeMeta, InodePatch,
    SnapshotId,
};
use vfs::engine::CachedMetadataStore;

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    #[test]
    fn metadata_and_blocks_survive_local_database_reopen() {
        let runtime =
            agentos_runtime::SidecarRuntime::process(&agentos_runtime::RuntimeConfig::default())
                .unwrap();
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("state.sqlite");
            let database = crate::vm_sqlite::open_local_vm_sqlite(
                path.clone(),
                runtime.context(),
                128 * 1024 * 1024,
            )
            .await
            .unwrap();
            bootstrap_schema(database.as_ref()).await.unwrap();
            {
                let metadata = SqliteMetadataStore::new(
                    database.clone(),
                    "test".to_owned(),
                    DEFAULT_MAX_METADATA_BYTES,
                );
                let root = metadata.resolve("/").await.unwrap();
                metadata
                    .create(
                        root.ino,
                        "workspace",
                        CreateInodeAttrs::directory(0o755, 1000, 1000),
                    )
                    .await
                    .unwrap();
                let blocks = SqliteBlockStore::new(database.clone(), "test".to_owned());
                blocks
                    .put(&BlockKey("first".to_owned()), b"persisted bytes")
                    .await
                    .unwrap();
                blocks
                    .copy(
                        &BlockKey("first".to_owned()),
                        &BlockKey("copied".to_owned()),
                    )
                    .await
                    .unwrap();
            }
            database.close().await.unwrap();
            drop(database);

            let database =
                crate::vm_sqlite::open_local_vm_sqlite(path, runtime.context(), 128 * 1024 * 1024)
                    .await
                    .unwrap();
            bootstrap_schema(database.as_ref()).await.unwrap();
            {
                let metadata = SqliteMetadataStore::new(
                    database.clone(),
                    "test".to_owned(),
                    DEFAULT_MAX_METADATA_BYTES,
                );
                let workspace = metadata.resolve("/workspace").await.unwrap();
                assert_eq!(workspace.uid, 1000);
                let blocks = SqliteBlockStore::new(database.clone(), "test".to_owned());
                for key in ["first", "copied"] {
                    assert_eq!(
                        blocks.get(&BlockKey(key.to_owned())).await.unwrap(),
                        b"persisted bytes"
                    );
                }
            }
            database.close().await.unwrap();
        });
    }

    const NO_FAULT: u8 = 0;
    const COMMIT_THEN_FAIL: u8 = 1;
    const FAIL_BEFORE_COMMIT: u8 = 2;

    /// Real local VM SQLite with one armed fault on the next transaction that
    /// moves the index head.
    struct HeadFaultDatabase {
        inner: SharedVmSqliteDatabase,
        fault: AtomicU8,
    }

    impl HeadFaultDatabase {
        fn arm(&self, fault: u8) {
            self.fault.store(fault, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl crate::vm_sqlite::VmSqliteDatabase for HeadFaultDatabase {
        async fn query(
            &self,
            statement: SqlStatement,
        ) -> Result<QueryResult, crate::vm_sqlite::VmSqliteError> {
            self.inner.query(statement).await
        }

        async fn transaction(
            &self,
            statements: Vec<SqlStatement>,
        ) -> Result<Vec<QueryResult>, crate::vm_sqlite::VmSqliteError> {
            let moves_head = statements.iter().any(|statement| {
                statement
                    .sql
                    .starts_with("INSERT INTO agentos_fs_metadata_heads")
            });
            let fault = match moves_head {
                true => self.fault.swap(NO_FAULT, Ordering::SeqCst),
                false => NO_FAULT,
            };
            match fault {
                COMMIT_THEN_FAIL => {
                    self.inner.transaction(statements).await?;
                    Err(crate::vm_sqlite::VmSqliteError::Callback(
                        "reply lost after commit".to_owned(),
                    ))
                }
                FAIL_BEFORE_COMMIT => Err(crate::vm_sqlite::VmSqliteError::Callback(
                    "transaction failed".to_owned(),
                )),
                _ => self.inner.transaction(statements).await,
            }
        }

        async fn close(&self) -> Result<(), crate::vm_sqlite::VmSqliteError> {
            self.inner.close().await
        }
    }

    #[test]
    fn index_survives_a_lost_save_reply_followed_by_a_failed_save() {
        let runtime =
            agentos_runtime::SidecarRuntime::process(&agentos_runtime::RuntimeConfig::default())
                .unwrap();
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let database = crate::vm_sqlite::open_local_vm_sqlite(
                directory.path().join("state.sqlite"),
                runtime.context(),
                128 * 1024 * 1024,
            )
            .await
            .unwrap();
            bootstrap_schema(database.as_ref()).await.unwrap();
            let faults = std::sync::Arc::new(HeadFaultDatabase {
                inner: database.clone(),
                fault: AtomicU8::new(NO_FAULT),
            });
            {
                let metadata = SqliteMetadataStore::new(
                    faults.clone(),
                    "test".to_owned(),
                    DEFAULT_MAX_METADATA_BYTES,
                );
                let root = metadata.resolve("/").await.unwrap();
                // A 64 KiB xattr on each of 40 directories makes the index larger
                // than one save transaction can carry.
                for index in 0..40u8 {
                    let mut attrs = CreateInodeAttrs::directory(0o755, 1000, 1000);
                    attrs.xattrs.insert(
                        "user.fill".to_owned(),
                        vec![index; vfs::engine::types::XATTR_SIZE_MAX],
                    );
                    metadata
                        .create(root.ino, &format!("dir-{index}"), attrs)
                        .await
                        .unwrap();
                }

                faults.arm(COMMIT_THEN_FAIL);
                metadata
                    .create(
                        root.ino,
                        "saved",
                        CreateInodeAttrs::directory(0o755, 1000, 1000),
                    )
                    .await
                    .unwrap_err();
                faults.arm(FAIL_BEFORE_COMMIT);
                metadata
                    .create(
                        root.ino,
                        "not-saved",
                        CreateInodeAttrs::directory(0o755, 1000, 1000),
                    )
                    .await
                    .unwrap_err();
            }

            let metadata = SqliteMetadataStore::new(
                database.clone(),
                "test".to_owned(),
                DEFAULT_MAX_METADATA_BYTES,
            );
            metadata.resolve("/saved").await.unwrap();
            metadata.resolve("/dir-39").await.unwrap();
            assert!(metadata.resolve("/not-saved").await.is_err());
            database.close().await.unwrap();
        });
    }
}

const DEFAULT_METADATA_CACHE_ENTRIES: usize = 4096;
const MAX_METADATA_CACHE_ENTRIES: usize = 1_000_000;
/// Remote actor SQLite rejects a statement whose bound values exceed 128 KiB.
/// The other values in a metadata chunk write are at most 272 bytes, so 64 KiB
/// leaves ample room.
const METADATA_CHUNK_SIZE: usize = 64 * 1024;
/// VM SQLite requests are JSON, which writes each blob byte as up to 4
/// characters, and a sidecar frame is at most 16 MiB. 32 statements carry at
/// most 2 MiB of index, which encodes to at most about 8 MiB.
const METADATA_SAVE_STATEMENTS_PER_TRANSACTION: usize = 32;
const DEFAULT_MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024 * 1024;
const MAX_CHUNK_SIZE: u32 = 16 * 1024 * 1024;
const VFS_MIGRATION_1: &[&str] = &[
    "CREATE TABLE agentos_fs_metadata_heads (
    namespace TEXT PRIMARY KEY CHECK (length(namespace) > 0),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    chunk_count INTEGER NOT NULL CHECK (chunk_count >= 0),
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0)
) STRICT",
    "CREATE TABLE agentos_fs_metadata_chunks (
    namespace TEXT NOT NULL CHECK (length(namespace) > 0),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    content BLOB NOT NULL,
    PRIMARY KEY (namespace, generation, chunk_index)
) STRICT",
    "CREATE TABLE agentos_fs_blocks (
    namespace TEXT NOT NULL CHECK (length(namespace) > 0),
    block_key TEXT NOT NULL CHECK (length(block_key) > 0),
    content BLOB NOT NULL,
    PRIMARY KEY (namespace, block_key)
) STRICT",
];

const VFS_MIGRATIONS: &[VmSqliteMigration] = &[VmSqliteMigration {
    version: 1,
    statements: VFS_MIGRATION_1,
}];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChunkedSqliteMountConfig {
    #[serde(default = "default_namespace")]
    namespace: String,
    chunk_size: Option<u32>,
    inline_threshold: Option<usize>,
    uid: Option<u32>,
    gid: Option<u32>,
    file_mode: Option<u32>,
    dir_mode: Option<u32>,
    metadata_cache_entries: Option<usize>,
    max_metadata_bytes: Option<usize>,
}

fn default_namespace() -> String {
    "agentos-root".to_owned()
}

#[derive(Debug)]
pub(crate) struct ChunkedSqliteMountPlugin;

impl<B> FileSystemPluginFactory<MountPluginContext<B>> for ChunkedSqliteMountPlugin {
    fn plugin_id(&self) -> &'static str {
        "chunked_sqlite"
    }

    fn open(
        &self,
        request: OpenFileSystemPluginRequest<'_, MountPluginContext<B>>,
    ) -> Result<Box<dyn MountedFileSystem>, PluginError> {
        let config: ChunkedSqliteMountConfig = serde_json::from_value(request.config.clone())
            .map_err(|error| PluginError::invalid_input(error.to_string()))?;
        validate_config(&config)?;

        let chunk_size = config.chunk_size.unwrap_or(vfs::engine::DEFAULT_CHUNK_SIZE);
        let inline_threshold = config
            .inline_threshold
            .unwrap_or(vfs::engine::DEFAULT_INLINE_THRESHOLD);
        let database = request.context.database.clone().ok_or_else(|| {
            PluginError::invalid_input("chunked_sqlite requires createVm database configuration")
        })?;
        let metadata = CachedMetadataStore::new(
            SqliteMetadataStore::new(
                database.clone(),
                config.namespace.clone(),
                config
                    .max_metadata_bytes
                    .unwrap_or(DEFAULT_MAX_METADATA_BYTES),
            ),
            config
                .metadata_cache_entries
                .unwrap_or(DEFAULT_METADATA_CACHE_ENTRIES),
        );
        let blocks = SqliteBlockStore::new(database, config.namespace);
        let fs = ChunkedFs::with_options(
            metadata,
            blocks,
            ChunkedFsOptions {
                inline_threshold,
                chunk_size,
                uid: config.uid.unwrap_or(0),
                gid: config.gid.unwrap_or(0),
                file_mode: config.file_mode.unwrap_or(0o644),
                dir_mode: config.dir_mode.unwrap_or(0o755),
            },
        );
        Ok(Box::new(MountedEngineFileSystem::with_runtime_context(
            fs,
            request.context.runtime_context.clone(),
        )))
    }
}

fn validate_config(config: &ChunkedSqliteMountConfig) -> Result<(), PluginError> {
    if config.namespace.is_empty() || config.namespace.len() > 256 {
        return Err(PluginError::invalid_input(
            "chunked_sqlite.namespace must contain 1..=256 bytes",
        ));
    }
    let chunk_size = config.chunk_size.unwrap_or(vfs::engine::DEFAULT_CHUNK_SIZE);
    if chunk_size == 0 || chunk_size > MAX_CHUNK_SIZE {
        return Err(PluginError::invalid_input(format!(
            "chunked_sqlite.chunkSize must be between 1 and {MAX_CHUNK_SIZE} bytes"
        )));
    }
    let inline_threshold = config
        .inline_threshold
        .unwrap_or(vfs::engine::DEFAULT_INLINE_THRESHOLD);
    if inline_threshold > chunk_size as usize {
        return Err(PluginError::invalid_input(
            "chunked_sqlite.inlineThreshold must not exceed chunkSize",
        ));
    }
    let cache_entries = config
        .metadata_cache_entries
        .unwrap_or(DEFAULT_METADATA_CACHE_ENTRIES);
    if cache_entries > MAX_METADATA_CACHE_ENTRIES {
        return Err(PluginError::invalid_input(format!(
            "chunked_sqlite.metadataCacheEntries exceeds limit {MAX_METADATA_CACHE_ENTRIES}"
        )));
    }
    let max_metadata_bytes = config
        .max_metadata_bytes
        .unwrap_or(DEFAULT_MAX_METADATA_BYTES);
    if max_metadata_bytes == 0 || max_metadata_bytes > MAX_METADATA_BYTES {
        return Err(PluginError::invalid_input(format!(
            "chunked_sqlite.maxMetadataBytes must be between 1 and {MAX_METADATA_BYTES} bytes"
        )));
    }
    Ok(())
}

struct SqliteMetadataStore {
    database: SharedVmSqliteDatabase,
    namespace: String,
    max_metadata_bytes: usize,
    inner: OnceCell<InMemoryMetadataStore>,
    mutation: Mutex<()>,
}

impl SqliteMetadataStore {
    fn new(database: SharedVmSqliteDatabase, namespace: String, max_metadata_bytes: usize) -> Self {
        Self {
            database,
            namespace,
            max_metadata_bytes,
            inner: OnceCell::new(),
            mutation: Mutex::new(()),
        }
    }

    async fn inner(&self) -> VfsResult<&InMemoryMetadataStore> {
        self.inner
            .get_or_try_init(|| async {
                match load_metadata(&self.database, &self.namespace, self.max_metadata_bytes)
                    .await?
                {
                    Some(bytes) => {
                        let dump: MetadataDump =
                            serde_bare::from_slice(&bytes).map_err(|error| {
                                VfsError::eio(format!(
                                    "decode SQLite VFS metadata for namespace {}: {error}",
                                    self.namespace
                                ))
                            })?;
                        Ok(InMemoryMetadataStore::from_dump(dump))
                    }
                    None => {
                        let inner = InMemoryMetadataStore::default();
                        persist_metadata(
                            &self.database,
                            &self.namespace,
                            &inner,
                            self.max_metadata_bytes,
                        )
                        .await?;
                        Ok(inner)
                    }
                }
            })
            .await
    }

    async fn persist(&self, inner: &InMemoryMetadataStore) -> VfsResult<()> {
        persist_metadata(
            &self.database,
            &self.namespace,
            inner,
            self.max_metadata_bytes,
        )
        .await
    }
}

pub(crate) async fn bootstrap_schema(
    database: &dyn crate::vm_sqlite::VmSqliteDatabase,
) -> VfsResult<()> {
    migrate_schema(
        database,
        "filesystem",
        "agentos_fs_schema_version",
        VFS_MIGRATIONS,
    )
    .await
    .map_err(actor_sql_error)
}

async fn persist_metadata(
    database: &SharedVmSqliteDatabase,
    namespace: &str,
    inner: &InMemoryMetadataStore,
    max_metadata_bytes: usize,
) -> VfsResult<()> {
    let dump = serde_bare::to_vec(&inner.dump())
        .map_err(|error| VfsError::eio(format!("encode SQLite VFS metadata: {error}")))?;
    if dump.len() > max_metadata_bytes {
        return Err(VfsError::new(
            "EFBIG",
            format!(
                "SQLite VFS metadata is {} bytes, exceeding maxMetadataBytes={max_metadata_bytes}; raise chunked_sqlite.maxMetadataBytes",
                dump.len()
            ),
        ));
    }
    if dump.len() >= max_metadata_bytes.saturating_mul(4) / 5 {
        eprintln!(
            "agentos chunked_sqlite metadata is nearing maxMetadataBytes: actual={} limit={max_metadata_bytes}",
            dump.len()
        );
    }

    let generation_result = database
        .query(SqlStatement::new(
            "SELECT COALESCE(MAX(generation), 0) FROM agentos_fs_metadata_chunks WHERE namespace = ?",
            vec![SqlValue::SqlText(namespace.to_owned())],
        ))
        .await
        .map_err(actor_sql_error)?;
    let generation = first_integer(generation_result, "metadata generation")?
        .checked_add(1)
        .ok_or_else(|| VfsError::eio("SQLite VFS metadata generation overflow"))?;

    let chunk_count = i64::try_from(dump.len().div_ceil(METADATA_CHUNK_SIZE))
        .map_err(|_| VfsError::eio("SQLite VFS metadata chunk count overflow"))?;
    let byte_length = i64::try_from(dump.len())
        .map_err(|_| VfsError::eio("SQLite VFS metadata byte length overflow"))?;
    // A load keeps reading the previous generation until the transaction that
    // moves the head commits. Each batch is built only when it is sent.
    let mut statements = (0_i64..)
        .zip(dump.chunks(METADATA_CHUNK_SIZE))
        .map(|(chunk_index, content)| {
            SqlStatement::new(
                "INSERT INTO agentos_fs_metadata_chunks (namespace, generation, chunk_index, content) VALUES (?, ?, ?, ?)",
                vec![
                    SqlValue::SqlText(namespace.to_owned()),
                    SqlValue::SqlInteger(generation),
                    SqlValue::SqlInteger(chunk_index),
                    SqlValue::SqlBlob(content.to_vec()),
                ],
            )
        })
        .chain([
            SqlStatement::new(
                "INSERT INTO agentos_fs_metadata_heads (namespace, generation, chunk_count, byte_length) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(namespace) DO UPDATE SET generation = excluded.generation, chunk_count = excluded.chunk_count, byte_length = excluded.byte_length",
                vec![
                    SqlValue::SqlText(namespace.to_owned()),
                    SqlValue::SqlInteger(generation),
                    SqlValue::SqlInteger(chunk_count),
                    SqlValue::SqlInteger(byte_length),
                ],
            ),
            SqlStatement::new(
                "DELETE FROM agentos_fs_metadata_chunks WHERE namespace = ? AND generation <> ?",
                vec![
                    SqlValue::SqlText(namespace.to_owned()),
                    SqlValue::SqlInteger(generation),
                ],
            ),
        ]);
    loop {
        let batch = statements
            .by_ref()
            .take(METADATA_SAVE_STATEMENTS_PER_TRANSACTION)
            .collect::<Vec<_>>();
        if batch.is_empty() {
            return Ok(());
        }
        database.transaction(batch).await.map_err(actor_sql_error)?;
    }
}

async fn load_metadata(
    database: &SharedVmSqliteDatabase,
    namespace: &str,
    max_metadata_bytes: usize,
) -> VfsResult<Option<Vec<u8>>> {
    let result = database
        .query(SqlStatement::new(
            "SELECT generation, chunk_count, byte_length FROM agentos_fs_metadata_heads WHERE namespace = ?",
            vec![SqlValue::SqlText(namespace.to_owned())],
        ))
        .await
        .map_err(actor_sql_error)?;
    let Some(row) = result.rows.into_iter().next() else {
        return Ok(None);
    };
    if row.len() != 3 {
        return Err(VfsError::eio("SQLite returned malformed VFS metadata head"));
    }
    let generation = sql_nonnegative_integer(&row[0], "metadata generation")?;
    let chunk_count = usize::try_from(sql_nonnegative_integer(&row[1], "metadata chunk count")?)
        .map_err(|_| VfsError::eio("SQLite VFS metadata chunk count overflow"))?;
    let byte_length = usize::try_from(sql_nonnegative_integer(&row[2], "metadata byte length")?)
        .map_err(|_| VfsError::eio("SQLite VFS metadata byte length overflow"))?;
    if byte_length > max_metadata_bytes {
        return Err(VfsError::new(
            "EFBIG",
            format!(
                "stored SQLite VFS metadata is {byte_length} bytes, exceeding maxMetadataBytes={max_metadata_bytes}; raise chunked_sqlite.maxMetadataBytes"
            ),
        ));
    }
    let expected_chunks = byte_length.div_ceil(METADATA_CHUNK_SIZE);
    if chunk_count != expected_chunks {
        return Err(VfsError::eio(format!(
            "SQLite VFS metadata head has {chunk_count} chunks for {byte_length} bytes; expected {expected_chunks}"
        )));
    }

    let mut dump = Vec::with_capacity(byte_length);
    for chunk_index in 0..chunk_count {
        let result = database
            .query(SqlStatement::new(
                "SELECT content FROM agentos_fs_metadata_chunks WHERE namespace = ? AND generation = ? AND chunk_index = ?",
                vec![
                    SqlValue::SqlText(namespace.to_owned()),
                    SqlValue::SqlInteger(generation),
                    SqlValue::SqlInteger(i64::try_from(chunk_index).map_err(|_| {
                        VfsError::eio("SQLite VFS metadata chunk index overflow")
                    })?),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        let content = first_blob(result)?.ok_or_else(|| {
            VfsError::eio(format!(
                "SQLite VFS metadata generation {generation} is missing chunk {chunk_index}"
            ))
        })?;
        if content.len() > METADATA_CHUNK_SIZE {
            return Err(VfsError::eio(format!(
                "SQLite VFS metadata chunk {chunk_index} is {} bytes, exceeding {METADATA_CHUNK_SIZE}",
                content.len()
            )));
        }
        dump.extend_from_slice(&content);
    }
    if dump.len() != byte_length {
        return Err(VfsError::eio(format!(
            "SQLite VFS metadata decoded to {} bytes; expected {byte_length}",
            dump.len()
        )));
    }
    Ok(Some(dump))
}

fn first_integer(result: QueryResult, description: &str) -> VfsResult<i64> {
    let row = result
        .rows
        .into_iter()
        .next()
        .ok_or_else(|| VfsError::eio(format!("SQLite returned no {description}")))?;
    let value = row
        .first()
        .ok_or_else(|| VfsError::eio(format!("SQLite returned empty {description} row")))?;
    sql_nonnegative_integer(value, description)
}

fn sql_nonnegative_integer(value: &SqlValue, description: &str) -> VfsResult<i64> {
    match value {
        SqlValue::SqlInteger(value) if *value >= 0 => Ok(*value),
        _ => Err(VfsError::eio(format!(
            "SQLite returned invalid {description}: {value:?}"
        ))),
    }
}

#[async_trait]
impl MetadataStore for SqliteMetadataStore {
    async fn resolve(&self, path: &str) -> VfsResult<InodeMeta> {
        self.inner().await?.resolve(path).await
    }

    async fn resolve_parent(&self, path: &str) -> VfsResult<(InodeMeta, String)> {
        self.inner().await?.resolve_parent(path).await
    }

    async fn lstat(&self, path: &str) -> VfsResult<InodeMeta> {
        self.inner().await?.lstat(path).await
    }

    async fn list_dir(&self, ino: u64) -> VfsResult<Vec<DentryStat>> {
        self.inner().await?.list_dir(ino).await
    }

    async fn create(
        &self,
        parent: u64,
        name: &str,
        attrs: CreateInodeAttrs,
    ) -> VfsResult<InodeMeta> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner.create(parent, name, attrs).await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn link(&self, parent: u64, name: &str, target: u64) -> VfsResult<()> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        inner.link(parent, name, target).await?;
        self.persist(inner).await
    }

    async fn remove(&self, parent: u64, name: &str) -> VfsResult<Vec<BlockKey>> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner.remove(parent, name).await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn rename(
        &self,
        src_parent: u64,
        src: &str,
        dst_parent: u64,
        dst: &str,
    ) -> VfsResult<Vec<BlockKey>> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner.rename(src_parent, src, dst_parent, dst).await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn set_attr(&self, ino: u64, patch: InodePatch) -> VfsResult<Vec<BlockKey>> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner.set_attr(ino, patch).await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn commit_write(
        &self,
        ino: u64,
        edits: Vec<ChunkEdit>,
        new_size: u64,
        allocated_extents: Vec<(u64, u64)>,
    ) -> VfsResult<Vec<BlockKey>> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner
            .commit_write(ino, edits, new_size, allocated_extents)
            .await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn get_chunks(&self, ino: u64, range: ChunkRange) -> VfsResult<Vec<ChunkRef>> {
        self.inner().await?.get_chunks(ino, range).await
    }

    async fn snapshot(&self, root: u64) -> VfsResult<SnapshotId> {
        self.inner().await?.snapshot(root).await
    }

    async fn fork(&self, snap: SnapshotId) -> VfsResult<u64> {
        let _guard = self.mutation.lock().await;
        let inner = self.inner().await?;
        let result = inner.fork(snap).await?;
        self.persist(inner).await?;
        Ok(result)
    }

    async fn gc(&self) -> VfsResult<Vec<BlockKey>> {
        self.inner().await?.gc().await
    }
}

struct SqliteBlockStore {
    database: SharedVmSqliteDatabase,
    namespace: String,
}

impl SqliteBlockStore {
    fn new(database: SharedVmSqliteDatabase, namespace: String) -> Self {
        Self {
            database,
            namespace,
        }
    }
}

#[async_trait]
impl BlockStore for SqliteBlockStore {
    async fn get(&self, key: &BlockKey) -> VfsResult<Vec<u8>> {
        let result = self
            .database
            .query(SqlStatement::new(
                "SELECT content FROM agentos_fs_blocks WHERE namespace = ? AND block_key = ?",
                vec![
                    SqlValue::SqlText(self.namespace.clone()),
                    SqlValue::SqlText(key.0.clone()),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        first_blob(result)?.ok_or_else(|| VfsError::enoent(&key.0))
    }

    async fn get_range(&self, key: &BlockKey, off: u64, len: u64) -> VfsResult<Vec<u8>> {
        let offset = i64::try_from(off)
            .map_err(|_| VfsError::einval(format!("block range offset is too large: {off}")))?;
        let length = i64::try_from(len)
            .map_err(|_| VfsError::einval(format!("block range length is too large: {len}")))?;
        let result = self
            .database
            .query(SqlStatement::new(
                "SELECT substr(content, ?, ?) FROM agentos_fs_blocks \
                 WHERE namespace = ? AND block_key = ?",
                vec![
                    SqlValue::SqlInteger(offset.saturating_add(1)),
                    SqlValue::SqlInteger(length),
                    SqlValue::SqlText(self.namespace.clone()),
                    SqlValue::SqlText(key.0.clone()),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        first_blob(result)?.ok_or_else(|| VfsError::enoent(&key.0))
    }

    async fn put(&self, key: &BlockKey, data: &[u8]) -> VfsResult<()> {
        self.database
            .query(SqlStatement::new(
                "INSERT INTO agentos_fs_blocks (namespace, block_key, content) VALUES (?, ?, ?) \
                 ON CONFLICT(namespace, block_key) DO UPDATE SET content = excluded.content",
                vec![
                    SqlValue::SqlText(self.namespace.clone()),
                    SqlValue::SqlText(key.0.clone()),
                    SqlValue::SqlBlob(data.to_vec()),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        Ok(())
    }

    async fn exists(&self, key: &BlockKey) -> VfsResult<bool> {
        let result = self
            .database
            .query(SqlStatement::new(
                "SELECT 1 FROM agentos_fs_blocks WHERE namespace = ? AND block_key = ? LIMIT 1",
                vec![
                    SqlValue::SqlText(self.namespace.clone()),
                    SqlValue::SqlText(key.0.clone()),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        Ok(!result.rows.is_empty())
    }

    async fn delete_many(&self, keys: &[BlockKey]) -> VfsResult<()> {
        if keys.is_empty() {
            return Ok(());
        }
        self.database
            .transaction(
                keys.iter()
                    .map(|key| {
                        SqlStatement::new(
                            "DELETE FROM agentos_fs_blocks WHERE namespace = ? AND block_key = ?",
                            vec![
                                SqlValue::SqlText(self.namespace.clone()),
                                SqlValue::SqlText(key.0.clone()),
                            ],
                        )
                    })
                    .collect(),
            )
            .await
            .map_err(actor_sql_error)?;
        Ok(())
    }

    async fn copy(&self, src: &BlockKey, dst: &BlockKey) -> VfsResult<()> {
        let result = self
            .database
            .query(SqlStatement::new(
                "INSERT INTO agentos_fs_blocks (namespace, block_key, content) \
                 SELECT namespace, ?, content FROM agentos_fs_blocks \
                 WHERE namespace = ? AND block_key = ? \
                 ON CONFLICT(namespace, block_key) DO UPDATE SET content = excluded.content",
                vec![
                    SqlValue::SqlText(dst.0.clone()),
                    SqlValue::SqlText(self.namespace.clone()),
                    SqlValue::SqlText(src.0.clone()),
                ],
            ))
            .await
            .map_err(actor_sql_error)?;
        if result.changes == 0 {
            return Err(VfsError::enoent(&src.0));
        }
        Ok(())
    }
}

fn first_blob(result: QueryResult) -> VfsResult<Option<Vec<u8>>> {
    let Some(row) = result.rows.into_iter().next() else {
        return Ok(None);
    };
    match row.into_iter().next() {
        Some(SqlValue::SqlBlob(bytes)) => Ok(Some(bytes)),
        Some(SqlValue::SqlNull) | None => Ok(None),
        Some(value) => Err(VfsError::eio(format!(
            "SQLite returned non-BLOB value: {value:?}"
        ))),
    }
}

fn actor_sql_error(error: impl std::fmt::Display) -> VfsError {
    VfsError::eio(format!("local SQLite: {error}"))
}
