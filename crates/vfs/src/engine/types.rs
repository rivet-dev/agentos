use crate::engine::error::{VfsError, VfsResult};
use serde::{Deserialize, Serialize};
use web_time::{SystemTime, UNIX_EPOCH};

pub const MAX_PATH: usize = 4096;
pub const MAX_SYMLINK_DEPTH: usize = 40;
pub const DEFAULT_INLINE_THRESHOLD: usize = 64 * 1024;
pub const DEFAULT_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

pub const S_IFREG: u32 = 0o100000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFLNK: u32 = 0o120000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: u32,
}

impl Timespec {
    pub fn now() -> Self {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            sec: duration.as_secs() as i64,
            nsec: duration.subsec_nanos(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InodeType {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Storage {
    Inline(Vec<u8>),
    Chunked { chunk_size: u32 },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InodeMeta {
    pub ino: u64,
    pub kind: InodeType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub nlink: u64,
    pub atime: Timespec,
    pub mtime: Timespec,
    pub ctime: Timespec,
    pub birthtime: Timespec,
    pub storage: Storage,
    pub symlink_target: Option<String>,
}

impl InodeMeta {
    pub fn to_stat(&self) -> VirtualStat {
        let type_bits = match self.kind {
            InodeType::File => S_IFREG,
            InodeType::Directory => S_IFDIR,
            InodeType::Symlink => S_IFLNK,
        };
        VirtualStat {
            mode: type_bits | (self.mode & 0o7777),
            size: self.size,
            blocks: self.size.div_ceil(512),
            is_directory: self.kind == InodeType::Directory,
            is_symbolic_link: self.kind == InodeType::Symlink,
            atime: self.atime,
            mtime: self.mtime,
            ctime: self.ctime,
            birthtime: self.birthtime,
            ino: self.ino,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dentry {
    pub name: String,
    pub ino: u64,
    pub kind: InodeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DentryStat {
    pub name: String,
    pub meta: InodeMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualStat {
    pub mode: u32,
    pub size: u64,
    pub blocks: u64,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub atime: Timespec,
    pub mtime: Timespec,
    pub ctime: Timespec,
    pub birthtime: Timespec,
    pub ino: u64,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlockKey(pub String);

impl BlockKey {
    pub fn from_content(data: &[u8]) -> Self {
        Self(blake3::hash(data).to_hex().to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRange {
    pub start: u64,
    pub end: Option<u64>,
}

impl ChunkRange {
    pub fn all() -> Self {
        Self {
            start: 0,
            end: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRef {
    pub index: u64,
    pub key: BlockKey,
    pub len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkEdit {
    pub index: u64,
    pub key: BlockKey,
    pub len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateInodeAttrs {
    pub kind: InodeType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub storage: Storage,
    pub symlink_target: Option<String>,
}

impl CreateInodeAttrs {
    pub fn file(mode: u32, uid: u32, gid: u32, storage: Storage) -> Self {
        Self {
            kind: InodeType::File,
            mode,
            uid,
            gid,
            storage,
            symlink_target: None,
        }
    }

    pub fn directory(mode: u32, uid: u32, gid: u32) -> Self {
        Self {
            kind: InodeType::Directory,
            mode,
            uid,
            gid,
            storage: Storage::None,
            symlink_target: None,
        }
    }

    pub fn symlink(target: String, uid: u32, gid: u32) -> Self {
        Self {
            kind: InodeType::Symlink,
            mode: 0o777,
            uid,
            gid,
            storage: Storage::None,
            symlink_target: Some(target),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InodePatch {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub atime: Option<Timespec>,
    pub mtime: Option<Timespec>,
    pub size: Option<u64>,
    pub storage: Option<Storage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub size: u64,
    pub mtime: Timespec,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub kind: InodeType,
    pub symlink_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectEntry {
    pub name: String,
    pub size: u64,
    pub mtime: Timespec,
    pub is_prefix: bool,
}

pub fn validate_path(path: &str) -> VfsResult<()> {
    if path.is_empty() || !path.starts_with('/') {
        return Err(VfsError::einval(format!("path must be absolute: {path}")));
    }
    if path.len() > MAX_PATH {
        return Err(VfsError::enametoolong(path));
    }
    Ok(())
}

pub fn normalize_path(path: &str) -> VfsResult<String> {
    validate_path(path)?;
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

pub fn parent_and_name(path: &str) -> VfsResult<(String, String)> {
    let normalized = normalize_path(path)?;
    if normalized == "/" {
        return Err(VfsError::einval("root has no parent"));
    }
    let (parent, name) = normalized
        .rsplit_once('/')
        .ok_or_else(|| VfsError::einval(format!("invalid path: {path}")))?;
    let parent = if parent.is_empty() { "/" } else { parent };
    Ok((parent.to_string(), name.to_string()))
}
