use super::root_fs::RootFileSystem;
use super::usage::FileSystemUsage;
use super::vfs::{
    VfsError, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat, VirtualUtimeSpec,
};
use std::any::Any;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::path::{Component, Path};
use web_time::{SystemTime, UNIX_EPOCH};

const MAX_REALPATH_SYMLINKS: usize = 40;

pub trait MountedFileSystem: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>>;
    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>>;
    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        let entries = self.read_dir(path)?;
        if entries.len() > max_entries {
            return Err(VfsError::new(
                "ENOMEM",
                format!(
                    "directory listing for '{path}' exceeds configured limit of {max_entries} entries"
                ),
            ));
        }
        Ok(entries)
    }
    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>>;
    fn write_file(&mut self, path: &str, content: Vec<u8>) -> VfsResult<()>;
    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        let _ = mode;
        self.write_file(path, content)
    }
    fn create_file_exclusive(&mut self, path: &str, content: Vec<u8>) -> VfsResult<()> {
        if self.exists(path) {
            return Err(VfsError::new(
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
    ) -> VfsResult<()> {
        let _ = mode;
        self.create_file_exclusive(path, content)
    }
    fn append_file(&mut self, path: &str, content: Vec<u8>) -> VfsResult<u64> {
        let mut existing = self.read_file(path)?;
        existing.extend_from_slice(&content);
        let new_len = existing.len() as u64;
        self.write_file(path, existing)?;
        Ok(new_len)
    }
    fn create_dir(&mut self, path: &str) -> VfsResult<()>;
    fn create_dir_with_mode(&mut self, path: &str, mode: Option<u32>) -> VfsResult<()> {
        let _ = mode;
        self.create_dir(path)
    }
    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()>;
    fn mkdir_with_mode(&mut self, path: &str, recursive: bool, mode: Option<u32>) -> VfsResult<()> {
        let _ = mode;
        self.mkdir(path, recursive)
    }
    fn exists(&self, path: &str) -> bool;
    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat>;
    fn remove_file(&mut self, path: &str) -> VfsResult<()>;
    fn remove_dir(&mut self, path: &str) -> VfsResult<()>;
    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()>;
    fn realpath(&self, path: &str) -> VfsResult<String>;
    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()>;
    fn read_link(&self, path: &str) -> VfsResult<String>;
    fn lstat(&self, path: &str) -> VfsResult<VirtualStat>;
    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()>;
    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()>;
    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()>;
    fn chown_spec(
        &mut self,
        path: &str,
        uid: u32,
        gid: u32,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        if !follow_symlinks {
            return Err(VfsError::unsupported(format!(
                "lchown is not supported for mount path '{path}'"
            )));
        }
        self.chown(path, uid, gid)
    }
    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()>;
    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        if !follow_symlinks {
            return Err(VfsError::unsupported(format!(
                "lutimes is not supported for mount path '{path}'"
            )));
        }
        let existing = match (atime, mtime) {
            (VirtualUtimeSpec::Omit, _) | (_, VirtualUtimeSpec::Omit) => Some(self.stat(path)?),
            _ => None,
        };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let atime_ms = match atime {
            VirtualUtimeSpec::Set(spec) => spec.to_truncated_millis()?,
            VirtualUtimeSpec::Now => now_ms,
            VirtualUtimeSpec::Omit => {
                existing
                    .as_ref()
                    .ok_or_else(|| {
                        VfsError::new("EINVAL", "UTIME_OMIT requires existing metadata")
                    })?
                    .atime_ms
            }
        };
        let mtime_ms = match mtime {
            VirtualUtimeSpec::Set(spec) => spec.to_truncated_millis()?,
            VirtualUtimeSpec::Now => now_ms,
            VirtualUtimeSpec::Omit => {
                existing
                    .as_ref()
                    .ok_or_else(|| {
                        VfsError::new("EINVAL", "UTIME_OMIT requires existing metadata")
                    })?
                    .mtime_ms
            }
        };
        self.utimes(path, atime_ms, mtime_ms)
    }
    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()>;
    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>>;
    fn shutdown(&mut self) -> VfsResult<()> {
        Ok(())
    }
}

pub struct MountedVirtualFileSystem<F> {
    inner: F,
}

impl<F> MountedVirtualFileSystem<F> {
    pub fn new(inner: F) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &F {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut F {
        &mut self.inner
    }
}

impl<F> MountedFileSystem for MountedVirtualFileSystem<F>
where
    F: VirtualFileSystem + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        VirtualFileSystem::read_file(&mut self.inner, path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        VirtualFileSystem::read_dir(&mut self.inner, path)
    }

    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        VirtualFileSystem::read_dir_limited(&mut self.inner, path, max_entries)
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        VirtualFileSystem::read_dir_with_types(&mut self.inner, path)
    }

    fn write_file(&mut self, path: &str, content: Vec<u8>) -> VfsResult<()> {
        VirtualFileSystem::write_file(&mut self.inner, path, content)
    }

    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        VirtualFileSystem::write_file_with_mode(&mut self.inner, path, content, mode)
    }

    fn create_file_exclusive(&mut self, path: &str, content: Vec<u8>) -> VfsResult<()> {
        VirtualFileSystem::create_file_exclusive(&mut self.inner, path, content)
    }

    fn create_file_exclusive_with_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        VirtualFileSystem::create_file_exclusive_with_mode(&mut self.inner, path, content, mode)
    }

    fn append_file(&mut self, path: &str, content: Vec<u8>) -> VfsResult<u64> {
        VirtualFileSystem::append_file(&mut self.inner, path, content)
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        VirtualFileSystem::create_dir(&mut self.inner, path)
    }

    fn create_dir_with_mode(&mut self, path: &str, mode: Option<u32>) -> VfsResult<()> {
        VirtualFileSystem::create_dir_with_mode(&mut self.inner, path, mode)
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        VirtualFileSystem::mkdir(&mut self.inner, path, recursive)
    }

    fn mkdir_with_mode(&mut self, path: &str, recursive: bool, mode: Option<u32>) -> VfsResult<()> {
        VirtualFileSystem::mkdir_with_mode(&mut self.inner, path, recursive, mode)
    }

    fn exists(&self, path: &str) -> bool {
        VirtualFileSystem::exists(&self.inner, path)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        VirtualFileSystem::stat(&mut self.inner, path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        VirtualFileSystem::remove_file(&mut self.inner, path)
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        VirtualFileSystem::remove_dir(&mut self.inner, path)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        VirtualFileSystem::rename(&mut self.inner, old_path, new_path)
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        VirtualFileSystem::realpath(&self.inner, path)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        VirtualFileSystem::symlink(&mut self.inner, target, link_path)
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        VirtualFileSystem::read_link(&self.inner, path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        VirtualFileSystem::lstat(&self.inner, path)
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        VirtualFileSystem::link(&mut self.inner, old_path, new_path)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        VirtualFileSystem::chmod(&mut self.inner, path, mode)
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        VirtualFileSystem::chown(&mut self.inner, path, uid, gid)
    }

    fn chown_spec(
        &mut self,
        path: &str,
        uid: u32,
        gid: u32,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        VirtualFileSystem::chown_spec(&mut self.inner, path, uid, gid, follow_symlinks)
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        VirtualFileSystem::utimes(&mut self.inner, path, atime_ms, mtime_ms)
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        VirtualFileSystem::utimes_spec(&mut self.inner, path, atime, mtime, follow_symlinks)
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        VirtualFileSystem::truncate(&mut self.inner, path, length)
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        VirtualFileSystem::pread(&mut self.inner, path, offset, length)
    }
}

impl<T> MountedFileSystem for Box<T>
where
    T: MountedFileSystem + ?Sized + 'static,
{
    fn as_any(&self) -> &dyn Any {
        (**self).as_any()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        (**self).as_any_mut()
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        (**self).read_file(path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        (**self).read_dir(path)
    }

    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        (**self).read_dir_limited(path, max_entries)
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        (**self).read_dir_with_types(path)
    }

    fn write_file(&mut self, path: &str, content: Vec<u8>) -> VfsResult<()> {
        (**self).write_file(path, content)
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        (**self).create_dir(path)
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        (**self).mkdir(path, recursive)
    }

    fn exists(&self, path: &str) -> bool {
        (**self).exists(path)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        (**self).stat(path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        (**self).remove_file(path)
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        (**self).remove_dir(path)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        (**self).rename(old_path, new_path)
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        (**self).realpath(path)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        (**self).symlink(target, link_path)
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        (**self).read_link(path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        (**self).lstat(path)
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        (**self).link(old_path, new_path)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        (**self).chmod(path, mode)
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        (**self).chown(path, uid, gid)
    }

    fn chown_spec(
        &mut self,
        path: &str,
        uid: u32,
        gid: u32,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        (**self).chown_spec(path, uid, gid, follow_symlinks)
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        (**self).utimes(path, atime_ms, mtime_ms)
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        (**self).utimes_spec(path, atime, mtime, follow_symlinks)
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        (**self).truncate(path, length)
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        (**self).pread(path, offset, length)
    }

    fn shutdown(&mut self) -> VfsResult<()> {
        (**self).shutdown()
    }
}

pub struct ReadOnlyFileSystem<F> {
    inner: F,
}

impl<F> ReadOnlyFileSystem<F> {
    pub fn new(inner: F) -> Self {
        Self { inner }
    }
}

impl<F> MountedFileSystem for ReadOnlyFileSystem<F>
where
    F: MountedFileSystem + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        self.inner.read_file(path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        self.inner.read_dir(path)
    }

    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        self.inner.read_dir_limited(path, max_entries)
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        self.inner.read_dir_with_types(path)
    }

    fn write_file(&mut self, path: &str, _content: Vec<u8>) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn mkdir(&mut self, path: &str, _recursive: bool) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        self.inner.stat(path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn rename(&mut self, old_path: &str, _new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {old_path}"),
        ))
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        self.inner.realpath(path)
    }

    fn symlink(&mut self, _target: &str, link_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {link_path}"),
        ))
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        self.inner.read_link(path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        self.inner.lstat(path)
    }

    fn link(&mut self, _old_path: &str, new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {new_path}"),
        ))
    }

    fn chmod(&mut self, path: &str, _mode: u32) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn chown(&mut self, path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn chown_spec(
        &mut self,
        path: &str,
        _uid: u32,
        _gid: u32,
        _follow_symlinks: bool,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn utimes(&mut self, path: &str, _atime_ms: u64, _mtime_ms: u64) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        _atime: VirtualUtimeSpec,
        _mtime: VirtualUtimeSpec,
        _follow_symlinks: bool,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn truncate(&mut self, path: &str, _length: u64) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only filesystem: {path}"),
        ))
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        self.inner.pread(path, offset, length)
    }

    fn shutdown(&mut self) -> VfsResult<()> {
        self.inner.shutdown()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub path: String,
    pub plugin_id: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountOptions {
    pub plugin_id: String,
    pub read_only: bool,
}

impl MountOptions {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            read_only: false,
        }
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}

struct MountRegistration {
    path: String,
    plugin_id: String,
    read_only: bool,
    filesystem: Box<dyn MountedFileSystem>,
}

pub struct MountTable {
    mounts: Vec<MountRegistration>,
}

impl MountTable {
    pub fn new(root_fs: impl VirtualFileSystem + 'static) -> Self {
        Self {
            mounts: vec![MountRegistration {
                path: String::from("/"),
                plugin_id: String::from("root"),
                read_only: false,
                filesystem: Box::new(MountedVirtualFileSystem::new(root_fs)),
            }],
        }
    }

    pub fn new_boxed_root(filesystem: Box<dyn MountedFileSystem>, options: MountOptions) -> Self {
        let filesystem = if options.read_only {
            Box::new(ReadOnlyFileSystem::new(filesystem)) as Box<dyn MountedFileSystem>
        } else {
            filesystem
        };

        Self {
            mounts: vec![MountRegistration {
                path: String::from("/"),
                plugin_id: options.plugin_id,
                read_only: options.read_only,
                filesystem,
            }],
        }
    }

    pub fn mount(
        &mut self,
        path: &str,
        filesystem: impl VirtualFileSystem + 'static,
        options: MountOptions,
    ) -> VfsResult<()> {
        self.mount_boxed(
            path,
            Box::new(MountedVirtualFileSystem::new(filesystem)),
            options,
        )
    }

    pub fn mount_boxed(
        &mut self,
        path: &str,
        mut filesystem: Box<dyn MountedFileSystem>,
        options: MountOptions,
    ) -> VfsResult<()> {
        let normalized = normalize_path(path);
        if normalized == "/" {
            return Err(VfsError::new("EINVAL", "cannot mount over root"));
        }
        if self.mounts.iter().any(|mount| mount.path == normalized) {
            return Err(VfsError::new(
                "EEXIST",
                format!("already mounted at {normalized}"),
            ));
        }

        let (parent_index, relative_path) = self.resolve_index(&normalized)?;
        let parent_mount = &mut self.mounts[parent_index];
        if !parent_mount.filesystem.exists(&relative_path) {
            // Materializing the mountpoint directory on the parent is
            // cosmetic: child mounts resolve by path prefix before the parent
            // is consulted. A read-only parent (for example a read-only
            // module-access mount hosting nested package mounts) cannot
            // materialize the entry, but the mount must still succeed.
            if let Err(error) = parent_mount.filesystem.mkdir(&relative_path, true) {
                if error.code() != "EROFS" {
                    if let Err(shutdown_error) = filesystem.shutdown() {
                        return Err(VfsError::new(
                            shutdown_error.code(),
                            format!(
                                "failed to shut down filesystem after mount failure ({error}): {}",
                                shutdown_error.message()
                            ),
                        ));
                    }

                    return Err(error);
                }
            }
        }

        let filesystem = if options.read_only {
            Box::new(ReadOnlyFileSystem::new(filesystem)) as Box<dyn MountedFileSystem>
        } else {
            filesystem
        };

        self.mounts.push(MountRegistration {
            path: normalized,
            plugin_id: options.plugin_id,
            read_only: options.read_only,
            filesystem,
        });
        self.mounts
            .sort_by_key(|mount| std::cmp::Reverse(mount.path.len()));
        Ok(())
    }

    pub fn unmount(&mut self, path: &str) -> VfsResult<()> {
        let normalized = normalize_path(path);
        if normalized == "/" {
            return Err(VfsError::new("EINVAL", "cannot unmount root"));
        }

        let child_mount_prefix = format!("{normalized}/");
        if self
            .mounts
            .iter()
            .any(|mount| mount.path.starts_with(&child_mount_prefix))
        {
            return Err(VfsError::new(
                "EBUSY",
                format!("mount point has child mounts: {normalized}"),
            ));
        }

        let Some(index) = self
            .mounts
            .iter()
            .position(|mount| mount.path == normalized)
        else {
            return Err(VfsError::new(
                "EINVAL",
                format!("not a mount point: {normalized}"),
            ));
        };

        let mut mount = self.mounts.remove(index);
        mount.filesystem.shutdown()?;
        Ok(())
    }

    pub fn get_mounts(&self) -> Vec<MountEntry> {
        self.mounts
            .iter()
            .map(|mount| MountEntry {
                path: mount.path.clone(),
                plugin_id: mount.plugin_id.clone(),
                read_only: mount.read_only,
            })
            .collect()
    }

    pub fn root_virtual_filesystem_mut<T: VirtualFileSystem + 'static>(
        &mut self,
    ) -> Option<&mut T> {
        let root = self.mounts.iter_mut().find(|mount| mount.path == "/")?;
        root.filesystem
            .as_any_mut()
            .downcast_mut::<MountedVirtualFileSystem<T>>()
            .map(MountedVirtualFileSystem::inner_mut)
    }

    pub fn check_rename_copy_up_limits(
        &mut self,
        old_path: &str,
        new_path: &str,
        max_bytes: Option<u64>,
        max_inodes: Option<usize>,
    ) -> VfsResult<()> {
        let (old_index, old_relative_path) = self.resolve_index(old_path)?;
        let (new_index, new_relative_path) = self.resolve_index(new_path)?;
        if old_index != new_index {
            return Ok(());
        }

        let filesystem = &mut self.mounts[old_index].filesystem;
        if let Some(root) = filesystem
            .as_any_mut()
            .downcast_mut::<MountedVirtualFileSystem<RootFileSystem>>()
        {
            root.inner_mut().check_rename_copy_up_limits(
                &old_relative_path,
                &new_relative_path,
                max_bytes,
                max_inodes,
            )?;
        }

        Ok(())
    }

    pub fn root_usage(&mut self) -> VfsResult<FileSystemUsage> {
        let root = self
            .mounts
            .iter_mut()
            .find(|mount| mount.path == "/")
            .ok_or_else(|| VfsError::new("ENOENT", "missing root mount"))?;
        measure_mounted_filesystem_usage(root.filesystem.as_mut(), "/", &mut BTreeSet::new())
    }

    fn resolve_index(&self, full_path: &str) -> VfsResult<(usize, String)> {
        let normalized = normalize_path(full_path);
        for (index, mount) in self.mounts.iter().enumerate() {
            if mount.path == "/" {
                return Ok((index, normalized));
            }
            if normalized == mount.path {
                return Ok((index, String::from("/")));
            }
            let mount_prefix = format!("{}/", mount.path);
            if let Some(suffix) = normalized.strip_prefix(&mount_prefix) {
                // Strip exactly the mount prefix once. `trim_start_matches` would
                // strip every leading repetition of `mount.path`, so a path like
                // `/data/data/file` under mount `/data` would alias to `/file`
                // instead of `/data/file`, routing reads/writes to the wrong file.
                return Ok((index, format!("/{suffix}")));
            }
        }

        Err(VfsError::new(
            "ENOENT",
            format!("no such file or directory, resolve '{full_path}'"),
        ))
    }

    /// Resolve a path for a CONTENT operation (read_file/stat/pread/read_dir)
    /// that must follow symlinks like POSIX `open()`. `resolve_index` is purely
    /// lexical, so a path that descends through a symlink whose target lives in a
    /// *different* mount (e.g. `/opt/agentos/pkgs/<pkg>/current -> <version>`,
    /// where `current` is its own single-symlink leaf mount) would route into the
    /// symlink mount and fail. `realpath` follows those cross-mount symlinks, so
    /// resolve it first, then route the resolved path. Falls back to the raw path
    /// when realpath can't resolve it (e.g. a genuinely missing file) so callers
    /// still receive the mount's own ENOENT.
    /// True only for the tar-vfs single-symlink-root leaf mount (`<pkg>/current ->
    /// <version>` and the `bin/<cmd>` links), whose root inode IS a symlink. Only
    /// these mounts need cross-mount realpath resolution for content ops and the
    /// ENOENT->component-walk fallback; every other mount serves its own paths and
    /// MUST keep its native error semantics (e.g. a js_bridge mount's ENOENT/EIO),
    /// so gating on the concrete leaf type leaves normal mounts untouched.
    fn mount_is_symlink_leaf(&self, index: usize) -> bool {
        // Behavioral check (robust through the ReadOnly/MountedVirtual wrappers the
        // projection applies): a leaf whose root inode is itself a symbolic link.
        // Only the tar-vfs `<pkg>/current -> <version>` and `bin/<cmd>` single-symlink
        // mounts satisfy this; every normal mount's root is a directory.
        self.mounts[index]
            .filesystem
            .lstat("/")
            .map(|stat| stat.is_symbolic_link)
            .unwrap_or(false)
    }

    /// Resolve a path for a LINK-LEAF operation (lstat/readlink) that must
    /// follow INTERMEDIATE symlinks but not the final component. When the
    /// lexical route lands inside a symlink-root leaf mount at a descendant
    /// path, the component being operated on is not the mount's own symlink —
    /// resolve the parent across mounts and re-route, keeping the leaf
    /// unresolved. `relative == "/"` (the symlink itself) keeps the raw route
    /// so `lstat(<pkg>/current)` still reports the symlink.
    fn resolve_link_leaf_index(&self, path: &str) -> VfsResult<(usize, String)> {
        let normalized = normalize_path(path);
        let raw = self.resolve_index(&normalized)?;
        if raw.1 == "/" || !self.mount_is_symlink_leaf(raw.0) {
            return Ok(raw);
        }
        let parent = parent_path(&normalized);
        let leaf = basename(&normalized);
        match self.realpath(&parent) {
            Ok(resolved_parent) => self.resolve_index(&join_path(&resolved_parent, &leaf)),
            Err(_) => Ok(raw),
        }
    }

    fn resolve_content_index(&self, path: &str) -> VfsResult<(usize, String)> {
        let raw = self.resolve_index(&normalize_path(path))?;
        if !self.mount_is_symlink_leaf(raw.0) {
            return Ok(raw);
        }
        // The leaf's root is a symlink, so a descendant path must be followed across
        // the mount boundary to the real version tree before the content op runs.
        match self.realpath(path) {
            Ok(resolved) => self.resolve_index(&resolved),
            Err(_) => Ok(raw),
        }
    }

    fn child_mount_basenames(&self, path: &str) -> Vec<String> {
        let normalized = normalize_path(path);
        let mut basenames = BTreeSet::new();
        for mount in &self.mounts {
            if mount.path == "/" || mount.path == normalized {
                continue;
            }

            if parent_path(&mount.path) == normalized {
                basenames.insert(basename(&mount.path));
            }
        }
        basenames.into_iter().collect()
    }

    fn realpath_in_mount(&self, index: usize, relative_path: &str) -> VfsResult<String> {
        let mount = &self.mounts[index];
        let resolved = mount.filesystem.realpath(relative_path)?;
        if mount.path == "/" {
            return Ok(normalize_path(&resolved));
        }
        if resolved == "/" {
            return Ok(mount.path.clone());
        }
        Ok(normalize_path(&format!(
            "{}/{}",
            mount.path,
            resolved.trim_start_matches('/')
        )))
    }
}

fn measure_mounted_filesystem_usage(
    filesystem: &mut dyn MountedFileSystem,
    path: &str,
    visited: &mut BTreeSet<u64>,
) -> VfsResult<FileSystemUsage> {
    let stat = filesystem.lstat(path)?;
    let mut usage = FileSystemUsage::default();

    if visited.insert(stat.ino) {
        usage.inode_count += 1;
        if !stat.is_directory {
            usage.total_bytes = usage.total_bytes.saturating_add(stat.size);
        }
    }

    if !stat.is_directory || stat.is_symbolic_link {
        return Ok(usage);
    }

    for entry in filesystem.read_dir_with_types(path)? {
        if matches!(entry.name.as_str(), "." | "..") {
            continue;
        }

        let child_path = if path == "/" {
            format!("/{}", entry.name)
        } else {
            format!("{path}/{}", entry.name)
        };
        let child_usage = measure_mounted_filesystem_usage(filesystem, &child_path, visited)?;
        usage.total_bytes = usage.total_bytes.saturating_add(child_usage.total_bytes);
        usage.inode_count = usage.inode_count.saturating_add(child_usage.inode_count);
    }

    Ok(usage)
}

impl Drop for MountTable {
    fn drop(&mut self) {
        for mount in self.mounts.iter_mut().rev() {
            let _ = mount.filesystem.shutdown();
        }
    }
}

impl VirtualFileSystem for MountTable {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        let (index, relative_path) = self.resolve_content_index(path)?;
        self.mounts[index].filesystem.read_file(&relative_path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        let normalized = normalize_path(path);
        // Directory listings are content ops: a path that descends through a
        // symlink-root leaf mount (`<pkg>/current -> <version>`) must be
        // followed into the target mount, exactly like read_file/stat. Child
        // mounts still merge on the LEXICAL path — mount points attach to the
        // caller-visible path, not the resolved target.
        let (index, relative_path) = self.resolve_content_index(&normalized)?;
        let mut entries = self.mounts[index].filesystem.read_dir(&relative_path)?;
        let child_mounts = self.child_mount_basenames(&normalized);
        if child_mounts.is_empty() {
            return Ok(entries);
        }

        let mut merged = BTreeSet::new();
        merged.extend(entries.drain(..));
        merged.extend(child_mounts);
        Ok(merged.into_iter().collect())
    }

    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        let normalized = normalize_path(path);
        let (index, relative_path) = self.resolve_content_index(&normalized)?;
        let mut entries = self.mounts[index]
            .filesystem
            .read_dir_limited(&relative_path, max_entries)?;
        let child_mounts = self.child_mount_basenames(&normalized);
        if child_mounts.is_empty() {
            return Ok(entries);
        }

        let mut merged = BTreeSet::new();
        merged.extend(entries.drain(..));
        merged.extend(child_mounts);
        if merged.len() > max_entries {
            return Err(VfsError::new(
                "ENOMEM",
                format!(
                    "directory listing for '{path}' exceeds configured limit of {max_entries} entries"
                ),
            ));
        }
        Ok(merged.into_iter().collect())
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        let normalized = normalize_path(path);
        let (index, relative_path) = self.resolve_content_index(&normalized)?;
        let mut entries = self.mounts[index]
            .filesystem
            .read_dir_with_types(&relative_path)?;
        let child_mounts = self.child_mount_basenames(&normalized);
        if child_mounts.is_empty() {
            return Ok(entries);
        }

        let existing = entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<BTreeSet<_>>();
        for mount_name in child_mounts {
            if existing.contains(&mount_name) {
                continue;
            }
            entries.push(VirtualDirEntry {
                name: mount_name,
                is_directory: true,
                is_symbolic_link: false,
            });
        }
        Ok(entries)
    }

    fn write_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .write_file(&relative_path, content.into())
    }

    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: impl Into<Vec<u8>>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .write_file_with_mode(&relative_path, content.into(), mode)
    }

    fn create_file_exclusive(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .create_file_exclusive(&relative_path, content.into())
    }

    fn create_file_exclusive_with_mode(
        &mut self,
        path: &str,
        content: impl Into<Vec<u8>>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .create_file_exclusive_with_mode(&relative_path, content.into(), mode)
    }

    fn append_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<u64> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .append_file(&relative_path, content.into())
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index].filesystem.create_dir(&relative_path)
    }

    fn create_dir_with_mode(&mut self, path: &str, mode: Option<u32>) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .create_dir_with_mode(&relative_path, mode)
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .mkdir(&relative_path, recursive)
    }

    fn mkdir_with_mode(&mut self, path: &str, recursive: bool, mode: Option<u32>) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .mkdir_with_mode(&relative_path, recursive, mode)
    }

    fn exists(&self, path: &str) -> bool {
        // `exists` follows symlinks like POSIX access(); route through the
        // content resolver so paths under a symlink-root leaf mount resolve.
        self.resolve_content_index(path)
            .map(|(index, relative_path)| self.mounts[index].filesystem.exists(&relative_path))
            .unwrap_or(false)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        let (index, relative_path) = self.resolve_content_index(path)?;
        self.mounts[index].filesystem.stat(&relative_path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index].filesystem.remove_file(&relative_path)
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index].filesystem.remove_dir(&relative_path)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (old_index, old_relative_path) = self.resolve_index(old_path)?;
        let (new_index, new_relative_path) = self.resolve_index(new_path)?;
        if old_index != new_index {
            return Err(VfsError::new(
                "EXDEV",
                format!("rename across mounts: {old_path} -> {new_path}"),
            ));
        }

        self.mounts[old_index]
            .filesystem
            .rename(&old_relative_path, &new_relative_path)
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        let normalized = normalize_path(path);
        let (index, relative_path) = self.resolve_index(&normalized)?;
        match self.realpath_in_mount(index, &relative_path) {
            Ok(resolved) => return Ok(resolved),
            // Always fall back to the component walk on ELOOP. Fall back on ENOENT
            // ONLY for a single-symlink LEAF mount (e.g. `<pkg>/current -> <version>`):
            // it cannot resolve a descendant path itself — its root IS the symlink, so
            // a subpath resolves into it and ENOENTs; the walk then follows that
            // symlink across mounts. For every OTHER mount an ENOENT is a genuine
            // "missing file" that must propagate unchanged (walking it would rewrite a
            // mount's native ENOENT into a spurious success/EIO — see the js_bridge
            // errno-mapping mount). Non-leaf within-mount paths resolve via
            // `realpath_in_mount` above and return early, so pnpm resolution is
            // unaffected either way.
            Err(error) if error.code() == "ELOOP" => {}
            Err(error) if error.code() == "ENOENT" && self.mount_is_symlink_leaf(index) => {}
            Err(error) => return Err(error),
        }

        let mut pending = path_components(&normalized);
        let mut current = String::from("/");
        let mut followed_symlinks = 0usize;

        while let Some(component) = pending.pop_front() {
            let candidate = join_path(&current, &component);
            let stat = self.lstat(&candidate)?;

            if stat.is_symbolic_link {
                followed_symlinks += 1;
                if followed_symlinks > MAX_REALPATH_SYMLINKS {
                    return Err(VfsError::new(
                        "ELOOP",
                        format!("too many levels of symbolic links, '{path}'"),
                    ));
                }

                // Mounted filesystems express absolute symlink targets in their
                // own root namespace. Keep the mount index while reading the
                // link so the component-walk fallback does not accidentally
                // reinterpret `/target` as the VM root after crossing a
                // synthetic leaf mount.
                let (link_index, relative_path) = self.resolve_link_leaf_index(&candidate)?;
                let target = self.mounts[link_index]
                    .filesystem
                    .read_link(&relative_path)?;
                let target_path = if target.starts_with('/') {
                    if self.mounts[link_index].path == "/" {
                        normalize_path(&target)
                    } else {
                        normalize_path(&format!(
                            "{}/{}",
                            self.mounts[link_index].path,
                            target.trim_start_matches('/')
                        ))
                    }
                } else {
                    normalize_path(&format!("{}/{}", parent_path(&candidate), target))
                };
                let mut resolved_target = path_components(&target_path);
                resolved_target.extend(pending);
                pending = resolved_target;
                current = String::from("/");
                continue;
            }

            if !pending.is_empty() && !stat.is_directory {
                return Err(VfsError::new(
                    "ENOTDIR",
                    format!("not a directory, realpath '{candidate}'"),
                ));
            }

            current = candidate;
        }

        Ok(current)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        let normalized_link_path = normalize_path(link_path);
        let link_parent = parent_path(&normalized_link_path);
        let absolute_target = if target.starts_with('/') {
            normalize_path(target)
        } else {
            normalize_path(&format!("{link_parent}/{target}"))
        };

        let (index, relative_path) = self.resolve_index(&normalized_link_path)?;
        let (target_index, _) = self.resolve_index(&absolute_target)?;
        if index != target_index {
            return Err(VfsError::new(
                "EXDEV",
                format!("symlink across mounts: {link_path} -> {target}"),
            ));
        }

        self.mounts[index]
            .filesystem
            .symlink(target, &relative_path)
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        let (index, relative_path) = self.resolve_link_leaf_index(path)?;
        self.mounts[index].filesystem.read_link(&relative_path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        let (index, relative_path) = self.resolve_link_leaf_index(path)?;
        self.mounts[index].filesystem.lstat(&relative_path)
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (old_index, old_relative_path) = self.resolve_index(old_path)?;
        let (new_index, new_relative_path) = self.resolve_index(new_path)?;
        if old_index != new_index {
            return Err(VfsError::new(
                "EXDEV",
                format!("link across mounts: {old_path} -> {new_path}"),
            ));
        }

        self.mounts[old_index]
            .filesystem
            .link(&old_relative_path, &new_relative_path)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index].filesystem.chmod(&relative_path, mode)
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        self.chown_spec(path, uid, gid, true)
    }

    fn chown_spec(
        &mut self,
        path: &str,
        uid: u32,
        gid: u32,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        let (index, relative_path) = if follow_symlinks {
            self.resolve_index(path)?
        } else {
            self.resolve_link_leaf_index(path)?
        };
        self.mounts[index]
            .filesystem
            .chown_spec(&relative_path, uid, gid, follow_symlinks)
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .utimes(&relative_path, atime_ms, mtime_ms)
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .utimes_spec(&relative_path, atime, mtime, follow_symlinks)
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        let (index, relative_path) = self.resolve_index(path)?;
        self.mounts[index]
            .filesystem
            .truncate(&relative_path, length)
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        let (index, relative_path) = self.resolve_content_index(path)?;
        self.mounts[index]
            .filesystem
            .pread(&relative_path, offset, length)
    }
}

fn normalize_path(path: &str) -> String {
    let mut segments = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => segments.clear(),
            Component::ParentDir => {
                segments.pop();
            }
            Component::CurDir => {}
            Component::Normal(value) => segments.push(value.to_string_lossy().into_owned()),
            Component::Prefix(prefix) => {
                segments.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
        }
    }

    if segments.is_empty() {
        String::from("/")
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn path_components(path: &str) -> VecDeque<String> {
    normalize_path(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(String::from)
        .collect()
}

fn join_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{parent}/{child}")
    }
}

fn parent_path(path: &str) -> String {
    let normalized = normalize_path(path);
    let parent = Path::new(&normalized)
        .parent()
        .unwrap_or_else(|| Path::new("/"));
    let value = parent.to_string_lossy();
    if value.is_empty() {
        String::from("/")
    } else {
        value.into_owned()
    }
}

fn basename(path: &str) -> String {
    let normalized = normalize_path(path);
    Path::new(&normalized)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("/"))
}
