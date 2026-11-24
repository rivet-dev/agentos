use super::vfs::{
    VfsError, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat, VirtualUtimeSpec,
    S_IFDIR, S_IFLNK,
};
use std::sync::Arc;

/// Read-only flat directory whose entries are symbolic links.
///
/// Package projection mounts one of these per command directory (for example
/// `/opt/agentos/bin`) with the `entry_overlay` mount option, so a single
/// mount serves every managed command link while the directory itself stays
/// on its parent filesystem and remains writable for guest-installed files.
/// Entry stats match `SingleSymlinkFileSystem` leaves. Clones share entries
/// and the logical device; `new` creates a distinct device.
#[derive(Debug, Clone)]
pub struct SymlinkDirFileSystem {
    /// Sorted by name: lookups binary-search, and the index is the inode.
    entries: Arc<[(String, String)]>,
    device_id: u64,
}

impl SymlinkDirFileSystem {
    /// Builds the directory from `(name, target)` pairs. Names must be single
    /// path components and unique.
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> VfsResult<Self> {
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        for (index, (name, _)) in entries.iter().enumerate() {
            if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                return Err(VfsError::new(
                    "EINVAL",
                    format!("invalid symlink directory entry name {name:?}"),
                ));
            }
            if index > 0 && entries[index - 1].0 == *name {
                return Err(VfsError::new(
                    "EEXIST",
                    format!("duplicate symlink directory entry {name:?}"),
                ));
            }
        }
        Ok(Self {
            entries: entries.into(),
            device_id: super::vfs::allocate_memory_filesystem_device_id(),
        })
    }

    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(name, _)| name.as_str())
    }

    fn entry(&self, path: &str) -> Option<(usize, &str)> {
        let name = path.strip_prefix('/')?;
        let index = self
            .entries
            .binary_search_by(|(entry, _)| entry.as_str().cmp(name))
            .ok()?;
        Some((index, self.entries[index].1.as_str()))
    }

    fn base_stat(&self) -> VirtualStat {
        VirtualStat {
            mode: 0,
            size: 0,
            blocks: 0,
            dev: self.device_id,
            rdev: 0,
            is_directory: false,
            is_symbolic_link: false,
            atime_ms: 0,
            atime_nsec: 0,
            mtime_ms: 0,
            mtime_nsec: 0,
            ctime_ms: 0,
            ctime_nsec: 0,
            birthtime_ms: 0,
            ino: 1,
            nlink: 1,
            uid: 0,
            gid: 0,
        }
    }

    fn not_found(path: &str) -> VfsError {
        VfsError::new("ENOENT", format!("no such file or directory, '{path}'"))
    }

    fn readonly(op: &str, path: &str) -> VfsError {
        VfsError::new(
            "EROFS",
            format!("read-only symlink directory, {op} '{path}'"),
        )
    }

    fn not_directory(path: &str) -> VfsError {
        VfsError::new("ENOTDIR", format!("not a directory, readdir '{path}'"))
    }
}

impl VirtualFileSystem for SymlinkDirFileSystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        if path == "/" {
            return Err(VfsError::new(
                "EISDIR",
                format!("illegal operation on a directory, read '{path}'"),
            ));
        }
        Err(Self::not_found(path))
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        if path == "/" {
            return Ok(self.entry_names().map(str::to_owned).collect());
        }
        Err(match self.entry(path) {
            Some(_) => Self::not_directory(path),
            None => Self::not_found(path),
        })
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        if path == "/" {
            return Ok(self
                .entry_names()
                .map(|name| VirtualDirEntry {
                    name: name.to_owned(),
                    is_directory: false,
                    is_symbolic_link: true,
                })
                .collect());
        }
        Err(match self.entry(path) {
            Some(_) => Self::not_directory(path),
            None => Self::not_found(path),
        })
    }

    fn write_file(&mut self, path: &str, _content: impl Into<Vec<u8>>) -> VfsResult<()> {
        Err(Self::readonly("write", path))
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly("mkdir", path))
    }

    fn mkdir(&mut self, path: &str, _recursive: bool) -> VfsResult<()> {
        Err(Self::readonly("mkdir", path))
    }

    fn exists(&self, path: &str) -> bool {
        path == "/" || self.entry(path).is_some()
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        // The mount table resolves links across mounts before content stats.
        self.lstat(path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly("unlink", path))
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly("rmdir", path))
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only symlink directory, rename '{old_path}' to '{new_path}'"),
        ))
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        if path == "/" {
            return Ok(String::from("/"));
        }
        if self.entry(path).is_some() {
            return Err(VfsError::new(
                "ELOOP",
                "symlink directory entries must be resolved by the mount table",
            ));
        }
        Err(Self::not_found(path))
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only symlink directory, symlink '{link_path}' -> '{target}'"),
        ))
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        if path == "/" {
            return Err(VfsError::new(
                "EINVAL",
                format!("invalid argument, readlink '{path}'"),
            ));
        }
        self.entry(path)
            .map(|(_, target)| target.to_owned())
            .ok_or_else(|| Self::not_found(path))
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        if path == "/" {
            // Matches the synthesized mountpoint of a read-only parent.
            return Ok(VirtualStat {
                mode: S_IFDIR | 0o555,
                is_directory: true,
                nlink: 2,
                ..self.base_stat()
            });
        }
        let (index, target) = self.entry(path).ok_or_else(|| Self::not_found(path))?;
        // Same shape as a `SingleSymlinkFileSystem` leaf.
        Ok(VirtualStat {
            mode: S_IFLNK | 0o777,
            size: target.len() as u64,
            is_symbolic_link: true,
            ino: index as u64 + 2,
            uid: 1000,
            gid: 1000,
            ..self.base_stat()
        })
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only symlink directory, link '{old_path}' to '{new_path}'"),
        ))
    }

    fn chmod(&mut self, path: &str, _mode: u32) -> VfsResult<()> {
        Err(Self::readonly("chmod", path))
    }

    fn chown(&mut self, path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        Err(Self::readonly("chown", path))
    }

    fn utimes(&mut self, path: &str, _atime_ms: u64, _mtime_ms: u64) -> VfsResult<()> {
        Err(Self::readonly("utimes", path))
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        _atime: VirtualUtimeSpec,
        _mtime: VirtualUtimeSpec,
        _follow_symlinks: bool,
    ) -> VfsResult<()> {
        Err(Self::readonly("utimes", path))
    }

    fn truncate(&mut self, path: &str, _length: u64) -> VfsResult<()> {
        Err(Self::readonly("truncate", path))
    }

    fn pread(&mut self, path: &str, _offset: u64, _length: usize) -> VfsResult<Vec<u8>> {
        self.read_file(path)
    }
}
