use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use vfs::engine::block::BlockStore;
use vfs::engine::error::{VfsError, VfsResult};
use vfs::engine::types::BlockKey;

#[derive(Debug, Clone)]
pub struct FileBlockStore {
    root: PathBuf,
}

impl FileBlockStore {
    pub fn new(root: impl Into<PathBuf>) -> VfsResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|err| VfsError::eio(format!("create block root {}: {err}", root.display())))?;
        Ok(Self { root })
    }

    fn path_for(&self, key: &BlockKey) -> PathBuf {
        let (prefix, suffix) = key.0.split_at(key.0.len().min(2));
        self.root.join(prefix).join(suffix)
    }

    fn ensure_safe_key(key: &BlockKey) -> VfsResult<()> {
        if key.0.contains('/') || key.0.contains('\\') || key.0 == "." || key.0 == ".." {
            return Err(VfsError::einval(format!("unsafe block key: {}", key.0)));
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[async_trait]
impl BlockStore for FileBlockStore {
    async fn get(&self, key: &BlockKey) -> VfsResult<Vec<u8>> {
        Self::ensure_safe_key(key)?;
        let path = self.path_for(key);
        fs::read(&path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                VfsError::enoent(&key.0)
            } else {
                VfsError::eio(format!("read block {}: {err}", path.display()))
            }
        })
    }

    async fn get_range(&self, key: &BlockKey, off: u64, len: u64) -> VfsResult<Vec<u8>> {
        let data = self.get(key).await?;
        let start = usize::try_from(off)
            .map_err(|_| VfsError::einval(format!("range offset is too large: {off}")))?;
        let len = usize::try_from(len)
            .map_err(|_| VfsError::einval(format!("range length is too large: {len}")))?;
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    async fn put(&self, key: &BlockKey, data: &[u8]) -> VfsResult<()> {
        Self::ensure_safe_key(key)?;
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| VfsError::eio(format!("create block dir: {err}")))?;
        }
        fs::write(&path, data)
            .map_err(|err| VfsError::eio(format!("write block {}: {err}", path.display())))
    }

    async fn exists(&self, key: &BlockKey) -> VfsResult<bool> {
        Self::ensure_safe_key(key)?;
        Ok(self.path_for(key).exists())
    }

    async fn delete_many(&self, keys: &[BlockKey]) -> VfsResult<()> {
        let mut errors = Vec::new();
        for key in keys {
            if let Err(error) = Self::ensure_safe_key(key) {
                errors.push(error.to_string());
                continue;
            }
            match fs::remove_file(self.path_for(key)) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => errors.push(format!("delete block {}: {err}", key.0)),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(VfsError::eio(format!(
                "delete {} local blocks failed: {}",
                errors.len(),
                errors.join("; ")
            )))
        }
    }

    async fn copy(&self, src: &BlockKey, dst: &BlockKey) -> VfsResult<()> {
        let data = self.get(src).await?;
        self.put(dst, &data).await
    }
}
