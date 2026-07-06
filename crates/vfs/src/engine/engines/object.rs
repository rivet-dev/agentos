use crate::engine::block::ObjectBackend;
use crate::engine::error::{VfsError, VfsResult};
use crate::engine::types::{normalize_path, Dentry, InodeType, ObjectMeta, Timespec, VirtualStat};
use crate::engine::vfs::VirtualFileSystem;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ObjectFsOptions {
    pub prefix: String,
    pub uid: u32,
    pub gid: u32,
    pub file_mode: u32,
    pub dir_mode: u32,
}

impl Default for ObjectFsOptions {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            uid: 0,
            gid: 0,
            file_mode: 0o644,
            dir_mode: 0o755,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectFs<B> {
    backend: B,
    options: ObjectFsOptions,
}

impl<B> ObjectFs<B> {
    pub fn new(backend: B) -> Self {
        Self::with_options(backend, ObjectFsOptions::default())
    }

    pub fn with_options(backend: B, options: ObjectFsOptions) -> Self {
        Self { backend, options }
    }

    fn key_for(&self, path: &str) -> VfsResult<String> {
        let normalized = normalize_path(path)?;
        let relative = normalized.trim_start_matches('/');
        Ok(format!("{}{}", self.options.prefix, relative))
    }

    fn dir_prefix_for(&self, path: &str) -> VfsResult<String> {
        let mut key = self.key_for(path)?;
        if !key.is_empty() && !key.ends_with('/') {
            key.push('/');
        }
        Ok(key)
    }

    fn file_meta(&self, size: u64) -> ObjectMeta {
        ObjectMeta {
            size,
            mtime: Timespec::now(),
            mode: self.options.file_mode,
            uid: self.options.uid,
            gid: self.options.gid,
            kind: InodeType::File,
            symlink_target: None,
        }
    }

    fn dir_meta(&self) -> ObjectMeta {
        ObjectMeta {
            size: 0,
            mtime: Timespec::now(),
            mode: self.options.dir_mode,
            uid: self.options.uid,
            gid: self.options.gid,
            kind: InodeType::Directory,
            symlink_target: None,
        }
    }
}

impl<B: ObjectBackend> ObjectFs<B> {
    async fn collect_objects_under(&self, prefix: &str) -> VfsResult<Vec<String>> {
        let mut pending = vec![prefix.to_string()];
        let mut objects = Vec::new();
        while let Some(current) = pending.pop() {
            for entry in self.backend.list(&current).await? {
                if entry.is_prefix {
                    pending.push(entry.name);
                } else {
                    objects.push(entry.name);
                }
            }
        }
        Ok(objects)
    }
}

#[async_trait]
impl<B: ObjectBackend> VirtualFileSystem for ObjectFs<B> {
    async fn read_file(&self, path: &str) -> VfsResult<Vec<u8>> {
        let key = self.key_for(path)?;
        let meta = self
            .backend
            .head(&key)
            .await?
            .ok_or_else(|| VfsError::enoent(path))?;
        if meta.kind == InodeType::Directory {
            return Err(VfsError::eisdir(path));
        }
        if meta.kind == InodeType::Symlink {
            let target = meta.symlink_target.ok_or_else(|| VfsError::enoent(path))?;
            return self.read_file(&target).await;
        }
        self.backend.get_range(&key, 0, meta.size).await
    }

    async fn read_dir(&self, path: &str) -> VfsResult<Vec<String>> {
        Ok(self
            .read_dir_with_types(path)
            .await?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    async fn read_dir_with_types(&self, path: &str) -> VfsResult<Vec<Dentry>> {
        let prefix = self.dir_prefix_for(path)?;
        let entries = self.backend.list(&prefix).await?;
        let mut result = Vec::new();
        for entry in entries {
            let name = entry
                .name
                .trim_start_matches(&prefix)
                .trim_end_matches('/')
                .to_string();
            if name.is_empty() || name.contains('/') {
                continue;
            }
            result.push(Dentry {
                name,
                ino: 0,
                kind: if entry.is_prefix {
                    InodeType::Directory
                } else {
                    InodeType::File
                },
            });
        }
        Ok(result)
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> VfsResult<()> {
        let key = self.key_for(path)?;
        self.backend
            .put(&key, content, self.file_meta(content.len() as u64))
            .await
    }

    async fn create_dir(&self, path: &str) -> VfsResult<()> {
        let key = self.dir_prefix_for(path)?;
        self.backend.put(&key, &[], self.dir_meta()).await
    }

    async fn mkdir(&self, path: &str, recursive: bool) -> VfsResult<()> {
        if !recursive {
            return self.create_dir(path).await;
        }
        let normalized = normalize_path(path)?;
        let mut current = String::new();
        for part in normalized
            .trim_start_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
        {
            current.push('/');
            current.push_str(part);
            self.create_dir(&current).await?;
        }
        Ok(())
    }

    async fn exists(&self, path: &str) -> bool {
        let Ok(key) = self.key_for(path) else {
            return false;
        };
        if self.backend.head(&key).await.ok().flatten().is_some() {
            return true;
        }
        let Ok(prefix) = self.dir_prefix_for(path) else {
            return false;
        };
        self.backend
            .list(&prefix)
            .await
            .map(|entries| !entries.is_empty())
            .unwrap_or(false)
    }

    async fn stat(&self, path: &str) -> VfsResult<VirtualStat> {
        let key = self.key_for(path)?;
        if let Some(meta) = self.backend.head(&key).await? {
            return Ok(object_stat(meta));
        }
        let entries = self.backend.list(&self.dir_prefix_for(path)?).await?;
        if entries.is_empty() {
            return Err(VfsError::enoent(path));
        }
        Ok(object_stat(self.dir_meta()))
    }

    async fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        self.stat(path).await
    }

    async fn remove_file(&self, path: &str) -> VfsResult<()> {
        self.backend.delete(&self.key_for(path)?).await
    }

    async fn remove_dir(&self, path: &str) -> VfsResult<()> {
        let prefix = self.dir_prefix_for(path)?;
        let entries = self.backend.list(&prefix).await?;
        if entries.iter().any(|entry| entry.name != prefix) {
            return Err(VfsError::enotempty(path));
        }
        self.backend.delete(&prefix).await
    }

    async fn rename(&self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let old_key = self.key_for(old_path)?;
        if self.backend.head(&old_key).await?.is_some() {
            let new_key = self.key_for(new_path)?;
            self.backend.copy(&old_key, &new_key).await?;
            self.backend.delete(&old_key).await?;
            return Ok(());
        }
        let old_prefix = self.dir_prefix_for(old_path)?;
        let new_prefix = self.dir_prefix_for(new_path)?;
        let objects = self.collect_objects_under(&old_prefix).await?;
        if objects.is_empty() {
            return Err(VfsError::enoent(old_path));
        }
        for key in &objects {
            let dst = format!("{new_prefix}{}", key.trim_start_matches(&old_prefix));
            self.backend.copy(key, &dst).await?;
        }
        for key in objects {
            self.backend.delete(&key).await?;
        }
        Ok(())
    }

    async fn realpath(&self, path: &str) -> VfsResult<String> {
        if !self.exists(path).await {
            return Err(VfsError::enoent(path));
        }
        normalize_path(path)
    }

    async fn symlink(&self, target: &str, link_path: &str) -> VfsResult<()> {
        let key = self.key_for(link_path)?;
        let meta = ObjectMeta {
            size: 0,
            mtime: Timespec::now(),
            mode: 0o777,
            uid: self.options.uid,
            gid: self.options.gid,
            kind: InodeType::Symlink,
            symlink_target: Some(target.to_string()),
        };
        self.backend.put(&key, &[], meta).await
    }

    async fn readlink(&self, path: &str) -> VfsResult<String> {
        let key = self.key_for(path)?;
        let meta = self
            .backend
            .head(&key)
            .await?
            .ok_or_else(|| VfsError::enoent(path))?;
        if meta.kind != InodeType::Symlink {
            return Err(VfsError::einval(format!("not a symlink: {path}")));
        }
        Ok(meta.symlink_target.unwrap_or_default())
    }

    async fn link(&self, _old_path: &str, _new_path: &str) -> VfsResult<()> {
        Err(VfsError::eopnotsupp("ObjectFs does not support hard links"))
    }

    async fn chmod(&self, _path: &str, _mode: u32) -> VfsResult<()> {
        Ok(())
    }

    async fn chown(&self, _path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        Ok(())
    }

    async fn utimes(&self, _path: &str, _atime_ms: u64, _mtime_ms: u64) -> VfsResult<()> {
        Ok(())
    }

    async fn truncate(&self, path: &str, length: u64) -> VfsResult<()> {
        let mut data = self.read_file(path).await?;
        let length = usize::try_from(length)
            .map_err(|_| VfsError::einval(format!("truncate length is too large: {length}")))?;
        data.resize(length, 0);
        self.write_file(path, &data).await
    }

    async fn pread(&self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        let key = self.key_for(path)?;
        self.backend.get_range(&key, offset, length as u64).await
    }

    async fn pwrite(&self, path: &str, content: &[u8], offset: u64) -> VfsResult<()> {
        let mut data = self.read_file(path).await?;
        let start = usize::try_from(offset)
            .map_err(|_| VfsError::einval(format!("pwrite offset is too large: {offset}")))?;
        if start > data.len() {
            data.resize(start, 0);
        }
        let end = start.saturating_add(content.len());
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(content);
        self.write_file(path, &data).await
    }

    async fn append(&self, path: &str, content: &[u8]) -> VfsResult<u64> {
        let mut data = self.read_file(path).await?;
        data.extend_from_slice(content);
        let len = data.len() as u64;
        self.write_file(path, &data).await?;
        Ok(len)
    }
}

fn object_stat(meta: ObjectMeta) -> VirtualStat {
    let type_bits = match meta.kind {
        InodeType::File => crate::engine::types::S_IFREG,
        InodeType::Directory => crate::engine::types::S_IFDIR,
        InodeType::Symlink => crate::engine::types::S_IFLNK,
    };
    VirtualStat {
        mode: type_bits | (meta.mode & 0o7777),
        size: meta.size,
        blocks: meta.size.div_ceil(512),
        is_directory: meta.kind == InodeType::Directory,
        is_symbolic_link: meta.kind == InodeType::Symlink,
        atime: meta.mtime,
        mtime: meta.mtime,
        ctime: meta.mtime,
        birthtime: meta.mtime,
        ino: 0,
        nlink: 1,
        uid: meta.uid,
        gid: meta.gid,
    }
}
