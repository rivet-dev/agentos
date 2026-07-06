use agentos_vfs::{FileBlockStore, SqliteMetadataStore};
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
    let store = SqliteMetadataStore::in_memory().unwrap();
    assert!(store.has_schema().unwrap());
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
