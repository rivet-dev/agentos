use crate::posix::{
    MountedFileSystem, VfsError as PosixVfsError, VfsResult as PosixVfsResult, VirtualDirEntry,
    VirtualStat,
};
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::runtime::{Builder, Runtime};

static NEXT_ENGINE_DEVICE_ID: AtomicU64 = AtomicU64::new(4096);

pub struct MountedEngineFileSystem<F> {
    inner: F,
    runtime: Runtime,
    device_id: u64,
}

impl<F> MountedEngineFileSystem<F> {
    pub fn new(inner: F) -> PosixVfsResult<Self> {
        Ok(Self {
            inner,
            runtime: Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    PosixVfsError::io(format!("create vfs engine runtime: {error}"))
                })?,
            device_id: NEXT_ENGINE_DEVICE_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    fn block_on<T>(
        &self,
        future: impl std::future::Future<Output = crate::engine::VfsResult<T>>,
    ) -> PosixVfsResult<T> {
        self.runtime.block_on(future).map_err(convert_error)
    }
}

impl<F> MountedFileSystem for MountedEngineFileSystem<F>
where
    F: crate::engine::VirtualFileSystem + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn read_file(&mut self, path: &str) -> PosixVfsResult<Vec<u8>> {
        self.block_on(self.inner.read_file(path))
    }

    fn read_dir(&mut self, path: &str) -> PosixVfsResult<Vec<String>> {
        self.block_on(self.inner.read_dir(path))
    }

    fn read_dir_with_types(&mut self, path: &str) -> PosixVfsResult<Vec<VirtualDirEntry>> {
        self.block_on(self.inner.read_dir_with_types(path))
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| VirtualDirEntry {
                        name: entry.name,
                        is_directory: entry.kind == crate::engine::InodeType::Directory,
                        is_symbolic_link: entry.kind == crate::engine::InodeType::Symlink,
                    })
                    .collect()
            })
    }

    fn write_file(&mut self, path: &str, content: Vec<u8>) -> PosixVfsResult<()> {
        self.block_on(self.inner.write_file(path, &content))
    }

    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        mode: Option<u32>,
    ) -> PosixVfsResult<()> {
        self.write_file(path, content)?;
        if let Some(mode) = mode {
            self.chmod(path, mode)?;
        }
        Ok(())
    }

    fn create_file_exclusive(&mut self, path: &str, content: Vec<u8>) -> PosixVfsResult<()> {
        if self.exists(path) {
            return Err(PosixVfsError::new(
                "EEXIST",
                format!("file already exists, open '{path}'"),
            ));
        }
        self.write_file(path, content)
    }

    fn create_file_exclusive_with_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        mode: Option<u32>,
    ) -> PosixVfsResult<()> {
        self.create_file_exclusive(path, content)?;
        if let Some(mode) = mode {
            self.chmod(path, mode)?;
        }
        Ok(())
    }

    fn append_file(&mut self, path: &str, content: Vec<u8>) -> PosixVfsResult<u64> {
        self.block_on(self.inner.append(path, &content))
    }

    fn create_dir(&mut self, path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.create_dir(path))
    }

    fn create_dir_with_mode(&mut self, path: &str, mode: Option<u32>) -> PosixVfsResult<()> {
        self.create_dir(path)?;
        if let Some(mode) = mode {
            self.chmod(path, mode)?;
        }
        Ok(())
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> PosixVfsResult<()> {
        self.block_on(self.inner.mkdir(path, recursive))
    }

    fn mkdir_with_mode(
        &mut self,
        path: &str,
        recursive: bool,
        mode: Option<u32>,
    ) -> PosixVfsResult<()> {
        self.mkdir(path, recursive)?;
        if let Some(mode) = mode {
            self.chmod(path, mode)?;
        }
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.runtime.block_on(self.inner.exists(path))
    }

    fn stat(&mut self, path: &str) -> PosixVfsResult<VirtualStat> {
        let stat = self.block_on(self.inner.stat(path))?;
        Ok(convert_stat(stat, self.device_id))
    }

    fn remove_file(&mut self, path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.remove_file(path))
    }

    fn remove_dir(&mut self, path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.remove_dir(path))
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.rename(old_path, new_path))
    }

    fn realpath(&self, path: &str) -> PosixVfsResult<String> {
        self.block_on(self.inner.realpath(path))
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.symlink(target, link_path))
    }

    fn read_link(&self, path: &str) -> PosixVfsResult<String> {
        self.block_on(self.inner.readlink(path))
    }

    fn lstat(&self, path: &str) -> PosixVfsResult<VirtualStat> {
        let stat = self.block_on(self.inner.lstat(path))?;
        Ok(convert_stat(stat, self.device_id))
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> PosixVfsResult<()> {
        self.block_on(self.inner.link(old_path, new_path))
    }

    fn chmod(&mut self, path: &str, mode: u32) -> PosixVfsResult<()> {
        self.block_on(self.inner.chmod(path, mode))
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> PosixVfsResult<()> {
        self.block_on(self.inner.chown(path, uid, gid))
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> PosixVfsResult<()> {
        self.block_on(self.inner.utimes(path, atime_ms, mtime_ms))
    }

    fn truncate(&mut self, path: &str, length: u64) -> PosixVfsResult<()> {
        self.block_on(self.inner.truncate(path, length))
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> PosixVfsResult<Vec<u8>> {
        self.block_on(self.inner.pread(path, offset, length))
    }
}

fn convert_error(error: crate::engine::VfsError) -> PosixVfsError {
    PosixVfsError::new(error.code(), error.message().to_owned())
}

fn convert_stat(stat: crate::engine::VirtualStat, device_id: u64) -> VirtualStat {
    VirtualStat {
        mode: stat.mode,
        size: stat.size,
        blocks: stat.blocks,
        dev: device_id,
        rdev: 0,
        is_directory: stat.is_directory,
        is_symbolic_link: stat.is_symbolic_link,
        atime_ms: timespec_ms(stat.atime),
        atime_nsec: stat.atime.nsec,
        mtime_ms: timespec_ms(stat.mtime),
        mtime_nsec: stat.mtime.nsec,
        ctime_ms: timespec_ms(stat.ctime),
        ctime_nsec: stat.ctime.nsec,
        birthtime_ms: timespec_ms(stat.birthtime),
        ino: stat.ino,
        nlink: stat.nlink,
        uid: stat.uid,
        gid: stat.gid,
    }
}

fn timespec_ms(time: crate::engine::Timespec) -> u64 {
    if time.sec < 0 {
        return 0;
    }
    (time.sec as u64).saturating_mul(1_000) + u64::from(time.nsec / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::engines::{ChunkedFs, ChunkedFsOptions};
    use crate::engine::mem::{InMemoryMetadataStore, MemoryBlockStore};
    use crate::posix::S_IFREG;

    #[test]
    fn mounted_engine_filesystem_bridges_sync_posix_calls() {
        let fs = ChunkedFs::with_options(
            InMemoryMetadataStore::new(),
            MemoryBlockStore::new(),
            ChunkedFsOptions {
                inline_threshold: 2,
                chunk_size: 4,
                ..ChunkedFsOptions::default()
            },
        );
        let mut mounted = MountedEngineFileSystem::new(fs).expect("create mounted engine fs");

        mounted
            .mkdir("/work/nested", true)
            .expect("create nested dir");
        mounted
            .write_file_with_mode("/work/nested/file.txt", b"hello".to_vec(), Some(0o600))
            .expect("write file");
        assert_eq!(
            mounted
                .pread("/work/nested/file.txt", 1, 3)
                .expect("pread file"),
            b"ell"
        );
        let entries = mounted
            .read_dir_with_types("/work/nested")
            .expect("read typed dir");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
        assert!(!entries[0].is_directory);

        let stat = mounted.stat("/work/nested/file.txt").expect("stat file");
        assert_eq!(stat.mode & 0o777, 0o600);
        assert_eq!(stat.mode & S_IFREG, S_IFREG);
        assert_eq!(stat.size, 5);
    }
}
