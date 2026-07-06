use crate::engine::error::{VfsError, VfsResult};
use crate::engine::types::{Dentry, SnapshotId, VirtualStat};
use async_trait::async_trait;

#[async_trait]
pub trait VirtualFileSystem: Send + Sync {
    async fn read_file(&self, path: &str) -> VfsResult<Vec<u8>>;

    async fn read_text(&self, path: &str) -> VfsResult<String> {
        String::from_utf8(self.read_file(path).await?)
            .map_err(|_| VfsError::einval(format!("file is not valid UTF-8: {path}")))
    }

    async fn read_dir(&self, path: &str) -> VfsResult<Vec<String>>;
    async fn read_dir_with_types(&self, path: &str) -> VfsResult<Vec<Dentry>>;
    async fn write_file(&self, path: &str, content: &[u8]) -> VfsResult<()>;
    async fn create_dir(&self, path: &str) -> VfsResult<()>;
    async fn mkdir(&self, path: &str, recursive: bool) -> VfsResult<()>;
    async fn exists(&self, path: &str) -> bool;
    async fn stat(&self, path: &str) -> VfsResult<VirtualStat>;
    async fn lstat(&self, path: &str) -> VfsResult<VirtualStat>;
    async fn remove_file(&self, path: &str) -> VfsResult<()>;
    async fn remove_dir(&self, path: &str) -> VfsResult<()>;
    async fn rename(&self, old_path: &str, new_path: &str) -> VfsResult<()>;
    async fn realpath(&self, path: &str) -> VfsResult<String>;
    async fn symlink(&self, target: &str, link_path: &str) -> VfsResult<()>;
    async fn readlink(&self, path: &str) -> VfsResult<String>;
    async fn link(&self, old_path: &str, new_path: &str) -> VfsResult<()>;
    async fn chmod(&self, path: &str, mode: u32) -> VfsResult<()>;
    async fn chown(&self, path: &str, uid: u32, gid: u32) -> VfsResult<()>;
    async fn utimes(&self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()>;
    async fn truncate(&self, path: &str, length: u64) -> VfsResult<()>;
    async fn pread(&self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>>;
    async fn pwrite(&self, path: &str, content: &[u8], offset: u64) -> VfsResult<()>;
    async fn append(&self, path: &str, content: &[u8]) -> VfsResult<u64>;
}

#[async_trait]
pub trait Snapshottable: Send + Sync {
    async fn snapshot(&self, root: u64) -> VfsResult<SnapshotId>;
    async fn fork(&self, snap: SnapshotId) -> VfsResult<u64>;
}
