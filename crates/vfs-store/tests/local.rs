use agentos_vfs::{FileBlockStore, SqliteMetadataStore};
use rusqlite::Connection;
use vfs::engine::engines::{ChunkedFs, ChunkedFsOptions};
use vfs::engine::mem::MemoryBlockStore;
use vfs::engine::{BlockKey, BlockStore, VirtualFileSystem};

#[tokio::test]
async fn file_block_store_persists_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileBlockStore::new(temp.path()).unwrap();
    let key = BlockKey::from_content(b"persistent");
    store.put(&key, b"persistent").await.unwrap();
    assert_eq!(store.get(&key).await.unwrap(), b"persistent");

    let reopened = FileBlockStore::new(temp.path()).unwrap();
    assert_eq!(reopened.get(&key).await.unwrap(), b"persistent");
}

#[tokio::test]
async fn sqlite_store_installs_canonical_schema() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("schema.sqlite");
    let store = SqliteMetadataStore::open(&db).unwrap();
    assert!(store.has_schema().unwrap());
    drop(store);

    let connection = Connection::open(db).unwrap();
    let version: i64 = connection
        .query_row(
            "SELECT schema_version FROM agentos_fs_schema_version WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 1);

    let mut statement = connection
        .prepare(
            "SELECT name, sql FROM sqlite_schema
             WHERE type = 'table' AND name LIKE 'agentos_fs_%'
             ORDER BY name",
        )
        .unwrap();
    let tables = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        tables
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "agentos_fs_block_refs",
            "agentos_fs_chunks",
            "agentos_fs_dentries",
            "agentos_fs_inodes",
            "agentos_fs_schema_version",
            "agentos_fs_snapshots",
        ]
    );
    assert!(tables
        .iter()
        .all(|(_, sql)| sql.trim_end().ends_with("STRICT")));

    let legacy_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE name IN ('inodes', 'dentries', 'chunks', 'block_refs', 'snapshots',
                            'agentos_schema_versions')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_count, 0);
}

#[test]
fn sqlite_store_strict_types_and_semantic_checks_reject_invalid_rows() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("constraints.sqlite");
    drop(SqliteMetadataStore::open(&db).unwrap());
    let connection = Connection::open(db).unwrap();

    assert!(connection
        .execute(
            "INSERT INTO agentos_fs_snapshots (snapshot_id, root_ino, created_ns)
             VALUES ('not-an-integer', 1, 0)",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO agentos_fs_snapshots (snapshot_id, root_ino, created_ns)
             VALUES (0, 1, 0)",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO agentos_fs_inodes
             (ino, kind, mode, uid, gid, size, nlink, atime_ns, mtime_ns, ctime_ns,
              birthtime_ns, storage_mode, storage_chunk_size, inline_content, symlink_target)
             VALUES (2, 9, 420, 0, 0, 0, 1, 0, 0, 0, 0, 0, NULL, NULL, NULL)",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO agentos_fs_inodes
             (ino, kind, mode, uid, gid, size, nlink, atime_ns, mtime_ns, ctime_ns,
              birthtime_ns, storage_mode, storage_chunk_size, inline_content, symlink_target)
             VALUES (2, 0, 420, 0, 0, 0, 1, 0, 0, 0, 0, 2, NULL, NULL, NULL)",
            [],
        )
        .is_err());
}

#[test]
fn sqlite_store_rejects_future_schema_versions() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("future.sqlite");
    let connection = Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE agentos_fs_schema_version (
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
               schema_version INTEGER NOT NULL CHECK (schema_version >= 0)
             ) STRICT;
             INSERT INTO agentos_fs_schema_version (singleton, schema_version) VALUES (1, 2);",
        )
        .unwrap();
    drop(connection);

    let error = SqliteMetadataStore::open(db)
        .err()
        .expect("future schema must be rejected");
    assert!(error
        .message()
        .contains("version 2; latest supported version is 1"));
}

#[tokio::test]
async fn sqlite_store_reopens_persisted_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("metadata.sqlite");
    let blocks = MemoryBlockStore::new();

    {
        let metadata = SqliteMetadataStore::open(&db).unwrap();
        let fs = ChunkedFs::with_options(
            metadata,
            blocks.clone(),
            ChunkedFsOptions {
                inline_threshold: 1,
                chunk_size: 4,
                ..ChunkedFsOptions::default()
            },
        );
        fs.mkdir("/dir", false).await.unwrap();
        fs.write_file("/dir/file", b"persisted").await.unwrap();
    }

    let metadata = SqliteMetadataStore::open(&db).unwrap();
    let fs = ChunkedFs::with_options(
        metadata,
        blocks,
        ChunkedFsOptions {
            inline_threshold: 1,
            chunk_size: 4,
            ..ChunkedFsOptions::default()
        },
    );
    assert_eq!(fs.read_file("/dir/file").await.unwrap(), b"persisted");
    assert_eq!(fs.read_dir("/dir").await.unwrap(), vec!["file"]);
}

#[tokio::test]
async fn chunked_local_reopens_and_cleans_stale_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("metadata.sqlite");
    let block_root = temp.path().join("blocks");
    let stale_key = BlockKey::from_content(b"efgh");

    {
        let metadata = SqliteMetadataStore::open(&db).unwrap();
        let blocks = FileBlockStore::new(&block_root).unwrap();
        let fs = ChunkedFs::with_options(
            metadata,
            blocks,
            ChunkedFsOptions {
                inline_threshold: 1,
                chunk_size: 4,
                ..ChunkedFsOptions::default()
            },
        );
        fs.write_file("/file", b"abcdefgh").await.unwrap();
    }

    let metadata = SqliteMetadataStore::open(&db).unwrap();
    let blocks = FileBlockStore::new(&block_root).unwrap();
    let fs = ChunkedFs::with_options(
        metadata,
        blocks.clone(),
        ChunkedFsOptions {
            inline_threshold: 1,
            chunk_size: 4,
            ..ChunkedFsOptions::default()
        },
    );

    assert_eq!(fs.read_file("/file").await.unwrap(), b"abcdefgh");
    fs.truncate("/file", 5).await.unwrap();
    assert_eq!(fs.read_file("/file").await.unwrap(), b"abcde");
    assert!(!blocks.exists(&stale_key).await.unwrap());
}
