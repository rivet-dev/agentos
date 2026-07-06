use crate::engine::block::BlockStore;
use crate::engine::error::{VfsError, VfsResult};
use crate::engine::metadata::MetadataStore;
use crate::engine::types::{
    normalize_path, BlockKey, ChunkEdit, ChunkRange, CreateInodeAttrs, Dentry, InodeMeta,
    InodePatch, InodeType, SnapshotId, Storage, Timespec, VirtualStat, DEFAULT_CHUNK_SIZE,
    DEFAULT_INLINE_THRESHOLD,
};
use crate::engine::vfs::{Snapshottable, VirtualFileSystem};
use async_trait::async_trait;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ChunkedFsOptions {
    pub inline_threshold: usize,
    pub chunk_size: u32,
    pub uid: u32,
    pub gid: u32,
    pub file_mode: u32,
    pub dir_mode: u32,
}

impl Default for ChunkedFsOptions {
    fn default() -> Self {
        Self {
            inline_threshold: DEFAULT_INLINE_THRESHOLD,
            chunk_size: DEFAULT_CHUNK_SIZE,
            uid: 0,
            gid: 0,
            file_mode: 0o644,
            dir_mode: 0o755,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChunkedFs<M, B> {
    metadata: M,
    blocks: B,
    options: ChunkedFsOptions,
}

impl<M, B> ChunkedFs<M, B> {
    pub fn new(metadata: M, blocks: B) -> Self {
        Self::with_options(metadata, blocks, ChunkedFsOptions::default())
    }

    pub fn with_options(metadata: M, blocks: B, options: ChunkedFsOptions) -> Self {
        Self {
            metadata,
            blocks,
            options,
        }
    }

    pub fn metadata(&self) -> &M {
        &self.metadata
    }

    pub fn blocks(&self) -> &B {
        &self.blocks
    }
}

impl<M: MetadataStore, B: BlockStore> ChunkedFs<M, B> {
    async fn write_existing_or_create(&self, path: &str, content: &[u8]) -> VfsResult<()> {
        let (parent, name) = self.metadata.resolve_parent(path).await?;
        let existing = self.metadata.lstat(path).await.ok();
        let ino = match existing {
            Some(meta) => {
                if meta.kind == InodeType::Directory {
                    return Err(VfsError::eisdir(path));
                }
                meta.ino
            }
            None => {
                let storage = if content.len() <= self.options.inline_threshold {
                    Storage::Inline(content.to_vec())
                } else {
                    Storage::Chunked {
                        chunk_size: self.options.chunk_size,
                    }
                };
                let meta = self
                    .metadata
                    .create(
                        parent.ino,
                        &name,
                        CreateInodeAttrs::file(
                            self.options.file_mode,
                            self.options.uid,
                            self.options.gid,
                            storage,
                        ),
                    )
                    .await?;
                if content.len() <= self.options.inline_threshold {
                    return Ok(());
                }
                meta.ino
            }
        };

        if content.len() <= self.options.inline_threshold {
            let freed = self
                .metadata
                .set_attr(
                    ino,
                    InodePatch {
                        storage: Some(Storage::Inline(content.to_vec())),
                        size: Some(content.len() as u64),
                        ..InodePatch::default()
                    },
                )
                .await?;
            self.blocks.delete_many(&freed).await?;
            return Ok(());
        }

        let mut edits = Vec::new();
        for (index, chunk) in content.chunks(self.options.chunk_size as usize).enumerate() {
            let key = BlockKey::from_content(chunk);
            if !self.blocks.exists(&key).await? {
                self.blocks.put(&key, chunk).await?;
            }
            edits.push(ChunkEdit {
                index: index as u64,
                key,
                len: chunk.len() as u32,
            });
        }
        self.metadata
            .set_attr(
                ino,
                InodePatch {
                    storage: Some(Storage::Chunked {
                        chunk_size: self.options.chunk_size,
                    }),
                    size: Some(content.len() as u64),
                    ..InodePatch::default()
                },
            )
            .await?;
        let freed = self
            .metadata
            .commit_write(ino, edits, content.len() as u64)
            .await?;
        self.blocks.delete_many(&freed).await?;
        Ok(())
    }

    fn file_chunk_size(&self, storage: &Storage) -> u32 {
        match storage {
            Storage::Chunked { chunk_size } => *chunk_size,
            Storage::Inline(_) | Storage::None => self.options.chunk_size,
        }
    }

    fn ensure_file<'a>(&self, path: &str, meta: &'a InodeMeta) -> VfsResult<&'a InodeMeta> {
        match meta.kind {
            InodeType::File => Ok(meta),
            InodeType::Directory => Err(VfsError::eisdir(path)),
            InodeType::Symlink => Err(VfsError::eopnotsupp("resolved symlink without target file")),
        }
    }

    async fn read_file_range(
        &self,
        meta: &InodeMeta,
        offset: u64,
        length: usize,
    ) -> VfsResult<Vec<u8>> {
        if length == 0 || offset >= meta.size {
            return Ok(Vec::new());
        }
        let available = meta.size.saturating_sub(offset).min(length as u64);
        let output_len = usize::try_from(available)
            .map_err(|_| VfsError::einval(format!("range length is too large: {available}")))?;

        match &meta.storage {
            Storage::Inline(data) => {
                let start = usize::try_from(offset).map_err(|_| {
                    VfsError::einval(format!("range offset is too large: {offset}"))
                })?;
                if start >= data.len() {
                    return Ok(vec![0; output_len]);
                }
                let end = start.saturating_add(output_len).min(data.len());
                let mut output = vec![0; output_len];
                output[..end - start].copy_from_slice(&data[start..end]);
                Ok(output)
            }
            Storage::None => Ok(vec![0; output_len]),
            Storage::Chunked { chunk_size } => {
                let chunk_size = u64::from(*chunk_size);
                let end_offset = offset
                    .checked_add(available)
                    .ok_or_else(|| VfsError::einval("range end overflows"))?;
                let start_index = offset / chunk_size;
                let end_index = end_offset.div_ceil(chunk_size);
                let chunks = self
                    .metadata
                    .get_chunks(
                        meta.ino,
                        ChunkRange {
                            start: start_index,
                            end: Some(end_index),
                        },
                    )
                    .await?;
                let mut output = vec![0; output_len];
                for chunk in chunks {
                    let chunk_start = chunk.index.saturating_mul(chunk_size);
                    let block = self.blocks.get(&chunk.key).await?;
                    let copy_start = offset.max(chunk_start);
                    let copy_end = end_offset.min(chunk_start.saturating_add(block.len() as u64));
                    if copy_start >= copy_end {
                        continue;
                    }
                    let output_start = usize::try_from(copy_start - offset)
                        .map_err(|_| VfsError::einval("range output offset is too large"))?;
                    let block_start = usize::try_from(copy_start - chunk_start)
                        .map_err(|_| VfsError::einval("range block offset is too large"))?;
                    let len = usize::try_from(copy_end - copy_start)
                        .map_err(|_| VfsError::einval("range copy length is too large"))?;
                    output[output_start..output_start + len]
                        .copy_from_slice(&block[block_start..block_start + len]);
                }
                Ok(output)
            }
        }
    }

    async fn put_chunk_edit(&self, index: u64, data: Vec<u8>) -> VfsResult<ChunkEdit> {
        let len = u32::try_from(data.len())
            .map_err(|_| VfsError::einval(format!("chunk is too large: {}", data.len())))?;
        let key = BlockKey::from_content(&data);
        if !self.blocks.exists(&key).await? {
            self.blocks.put(&key, &data).await?;
        }
        Ok(ChunkEdit { index, key, len })
    }

    async fn write_chunked_range(
        &self,
        meta: &InodeMeta,
        content: &[u8],
        offset: u64,
    ) -> VfsResult<u64> {
        if content.is_empty() {
            return Ok(meta.size);
        }
        let content_len = u64::try_from(content.len()).map_err(|_| {
            VfsError::einval(format!("pwrite content is too large: {}", content.len()))
        })?;
        let end_offset = offset
            .checked_add(content_len)
            .ok_or_else(|| VfsError::einval("pwrite end offset overflows"))?;
        let new_size = meta.size.max(end_offset);

        if !matches!(meta.storage, Storage::Chunked { .. })
            && usize::try_from(new_size)
                .ok()
                .is_some_and(|len| len <= self.options.inline_threshold)
        {
            let old_len = usize::try_from(meta.size)
                .map_err(|_| VfsError::einval(format!("file is too large: {}", meta.size)))?;
            let mut data = self.read_file_range(meta, 0, old_len).await?;
            let start = usize::try_from(offset)
                .map_err(|_| VfsError::einval(format!("pwrite offset is too large: {offset}")))?;
            let end = start.saturating_add(content.len());
            if start > data.len() {
                data.resize(start, 0);
            }
            if end > data.len() {
                data.resize(end, 0);
            }
            data[start..end].copy_from_slice(content);
            let freed = self
                .metadata
                .set_attr(
                    meta.ino,
                    InodePatch {
                        storage: Some(Storage::Inline(data)),
                        size: Some(new_size),
                        ..InodePatch::default()
                    },
                )
                .await?;
            self.blocks.delete_many(&freed).await?;
            return Ok(new_size);
        }

        let chunk_size = u64::from(self.file_chunk_size(&meta.storage));
        let start_index = offset / chunk_size;
        let end_index = end_offset.div_ceil(chunk_size);
        let existing_chunks = if matches!(meta.storage, Storage::Chunked { .. }) {
            self.metadata
                .get_chunks(
                    meta.ino,
                    ChunkRange {
                        start: start_index,
                        end: Some(end_index),
                    },
                )
                .await?
                .into_iter()
                .map(|chunk| (chunk.index, chunk.key))
                .collect::<BTreeMap<_, _>>()
        } else {
            BTreeMap::new()
        };

        let mut edits = Vec::new();
        for index in start_index..end_index {
            let chunk_start = index.saturating_mul(chunk_size);
            let chunk_len = chunk_size.min(new_size.saturating_sub(chunk_start));
            let mut chunk_data = vec![
                0;
                usize::try_from(chunk_len).map_err(|_| {
                    VfsError::einval("chunk length is too large")
                })?
            ];

            match &meta.storage {
                Storage::Inline(data) => {
                    let copy_start = chunk_start.min(data.len() as u64);
                    let copy_end = chunk_start.saturating_add(chunk_len).min(data.len() as u64);
                    if copy_start < copy_end {
                        let dst = usize::try_from(copy_start - chunk_start)
                            .map_err(|_| VfsError::einval("inline chunk offset is too large"))?;
                        let src = usize::try_from(copy_start)
                            .map_err(|_| VfsError::einval("inline source offset is too large"))?;
                        let len = usize::try_from(copy_end - copy_start)
                            .map_err(|_| VfsError::einval("inline copy length is too large"))?;
                        chunk_data[dst..dst + len].copy_from_slice(&data[src..src + len]);
                    }
                }
                Storage::Chunked { .. } => {
                    if let Some(key) = existing_chunks.get(&index) {
                        let old = self.blocks.get(key).await?;
                        let len = old.len().min(chunk_data.len());
                        chunk_data[..len].copy_from_slice(&old[..len]);
                    }
                }
                Storage::None => {}
            }

            let write_start = offset.max(chunk_start);
            let write_end = end_offset.min(chunk_start.saturating_add(chunk_len));
            if write_start < write_end {
                let dst = usize::try_from(write_start - chunk_start)
                    .map_err(|_| VfsError::einval("chunk write offset is too large"))?;
                let src = usize::try_from(write_start - offset)
                    .map_err(|_| VfsError::einval("content write offset is too large"))?;
                let len = usize::try_from(write_end - write_start)
                    .map_err(|_| VfsError::einval("chunk write length is too large"))?;
                chunk_data[dst..dst + len].copy_from_slice(&content[src..src + len]);
            }

            edits.push(self.put_chunk_edit(index, chunk_data).await?);
        }

        if !matches!(meta.storage, Storage::Chunked { .. }) {
            self.metadata
                .set_attr(
                    meta.ino,
                    InodePatch {
                        storage: Some(Storage::Chunked {
                            chunk_size: self.options.chunk_size,
                        }),
                        size: Some(new_size),
                        ..InodePatch::default()
                    },
                )
                .await?;
        }
        let freed = self
            .metadata
            .commit_write(meta.ino, edits, new_size)
            .await?;
        self.blocks.delete_many(&freed).await?;
        Ok(new_size)
    }
}

#[async_trait]
impl<M: MetadataStore, B: BlockStore> VirtualFileSystem for ChunkedFs<M, B> {
    async fn read_file(&self, path: &str) -> VfsResult<Vec<u8>> {
        let meta = self.metadata.resolve(path).await?;
        self.ensure_file(path, &meta)?;
        let len = usize::try_from(meta.size)
            .map_err(|_| VfsError::einval(format!("file is too large: {}", meta.size)))?;
        self.read_file_range(&meta, 0, len).await
    }

    async fn read_dir(&self, path: &str) -> VfsResult<Vec<String>> {
        let meta = self.metadata.resolve(path).await?;
        Ok(self
            .metadata
            .list_dir(meta.ino)
            .await?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    async fn read_dir_with_types(&self, path: &str) -> VfsResult<Vec<Dentry>> {
        let meta = self.metadata.resolve(path).await?;
        Ok(self
            .metadata
            .list_dir(meta.ino)
            .await?
            .into_iter()
            .map(|entry| Dentry {
                name: entry.name,
                ino: entry.meta.ino,
                kind: entry.meta.kind,
            })
            .collect())
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> VfsResult<()> {
        self.write_existing_or_create(path, content).await
    }

    async fn create_dir(&self, path: &str) -> VfsResult<()> {
        let (parent, name) = self.metadata.resolve_parent(path).await?;
        self.metadata
            .create(
                parent.ino,
                &name,
                CreateInodeAttrs::directory(
                    self.options.dir_mode,
                    self.options.uid,
                    self.options.gid,
                ),
            )
            .await?;
        Ok(())
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
            if !self.exists(&current).await {
                self.create_dir(&current).await?;
            }
        }
        Ok(())
    }

    async fn exists(&self, path: &str) -> bool {
        self.metadata.resolve(path).await.is_ok()
    }

    async fn stat(&self, path: &str) -> VfsResult<VirtualStat> {
        Ok(self.metadata.resolve(path).await?.to_stat())
    }

    async fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        Ok(self.metadata.lstat(path).await?.to_stat())
    }

    async fn remove_file(&self, path: &str) -> VfsResult<()> {
        let meta = self.metadata.lstat(path).await?;
        if meta.kind == InodeType::Directory {
            return Err(VfsError::eisdir(path));
        }
        let (parent, name) = self.metadata.resolve_parent(path).await?;
        let freed = self.metadata.remove(parent.ino, &name).await?;
        self.blocks.delete_many(&freed).await
    }

    async fn remove_dir(&self, path: &str) -> VfsResult<()> {
        let meta = self.metadata.lstat(path).await?;
        if meta.kind != InodeType::Directory {
            return Err(VfsError::enotdir(path));
        }
        let (parent, name) = self.metadata.resolve_parent(path).await?;
        let freed = self.metadata.remove(parent.ino, &name).await?;
        self.blocks.delete_many(&freed).await
    }

    async fn rename(&self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (src_parent, src) = self.metadata.resolve_parent(old_path).await?;
        let (dst_parent, dst) = self.metadata.resolve_parent(new_path).await?;
        let freed = self
            .metadata
            .rename(src_parent.ino, &src, dst_parent.ino, &dst)
            .await?;
        self.blocks.delete_many(&freed).await
    }

    async fn realpath(&self, path: &str) -> VfsResult<String> {
        self.metadata.resolve(path).await?;
        normalize_path(path)
    }

    async fn symlink(&self, target: &str, link_path: &str) -> VfsResult<()> {
        let (parent, name) = self.metadata.resolve_parent(link_path).await?;
        self.metadata
            .create(
                parent.ino,
                &name,
                CreateInodeAttrs::symlink(target.to_string(), self.options.uid, self.options.gid),
            )
            .await?;
        Ok(())
    }

    async fn readlink(&self, path: &str) -> VfsResult<String> {
        let meta = self.metadata.lstat(path).await?;
        if meta.kind != InodeType::Symlink {
            return Err(VfsError::einval(format!("not a symlink: {path}")));
        }
        Ok(meta.symlink_target.unwrap_or_default())
    }

    async fn link(&self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let target = self.metadata.resolve(old_path).await?;
        let (parent, name) = self.metadata.resolve_parent(new_path).await?;
        self.metadata.link(parent.ino, &name, target.ino).await
    }

    async fn chmod(&self, path: &str, mode: u32) -> VfsResult<()> {
        let meta = self.metadata.resolve(path).await?;
        self.metadata
            .set_attr(
                meta.ino,
                InodePatch {
                    mode: Some(mode),
                    ..InodePatch::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn chown(&self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        let meta = self.metadata.resolve(path).await?;
        self.metadata
            .set_attr(
                meta.ino,
                InodePatch {
                    uid: Some(uid),
                    gid: Some(gid),
                    ..InodePatch::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn utimes(&self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        let meta = self.metadata.resolve(path).await?;
        self.metadata
            .set_attr(
                meta.ino,
                InodePatch {
                    atime: Some(ms_to_timespec(atime_ms)),
                    mtime: Some(ms_to_timespec(mtime_ms)),
                    ..InodePatch::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn truncate(&self, path: &str, length: u64) -> VfsResult<()> {
        let meta = self.metadata.resolve(path).await?;
        self.ensure_file(path, &meta)?;
        if length == meta.size {
            return Ok(());
        }

        if usize::try_from(length)
            .ok()
            .is_some_and(|len| len <= self.options.inline_threshold)
        {
            let data = self
                .read_file_range(&meta, 0, usize::try_from(length).unwrap_or(0))
                .await?;
            let freed = self
                .metadata
                .set_attr(
                    meta.ino,
                    InodePatch {
                        storage: Some(Storage::Inline(data)),
                        size: Some(length),
                        ..InodePatch::default()
                    },
                )
                .await?;
            self.blocks.delete_many(&freed).await?;
            return Ok(());
        }

        let chunk_size = u64::from(self.file_chunk_size(&meta.storage));
        let mut edits = Vec::new();
        if !matches!(meta.storage, Storage::Chunked { .. }) {
            let existing_len = meta.size.min(length);
            let mut offset = 0;
            while offset < existing_len {
                let len = (existing_len - offset).min(chunk_size);
                let data = self
                    .read_file_range(
                        &meta,
                        offset,
                        usize::try_from(len)
                            .map_err(|_| VfsError::einval("truncate chunk is too large"))?,
                    )
                    .await?;
                edits.push(self.put_chunk_edit(offset / chunk_size, data).await?);
                offset = offset.saturating_add(len);
            }
            self.metadata
                .set_attr(
                    meta.ino,
                    InodePatch {
                        storage: Some(Storage::Chunked {
                            chunk_size: self.options.chunk_size,
                        }),
                        size: Some(length),
                        ..InodePatch::default()
                    },
                )
                .await?;
        } else if length < meta.size && !length.is_multiple_of(chunk_size) {
            let final_index = length / chunk_size;
            let final_start = final_index.saturating_mul(chunk_size);
            let final_len = length - final_start;
            let data = self
                .read_file_range(
                    &meta,
                    final_start,
                    usize::try_from(final_len)
                        .map_err(|_| VfsError::einval("truncate final chunk is too large"))?,
                )
                .await?;
            edits.push(self.put_chunk_edit(final_index, data).await?);
        }

        let freed = self.metadata.commit_write(meta.ino, edits, length).await?;
        self.blocks.delete_many(&freed).await
    }

    async fn pread(&self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        let meta = self.metadata.resolve(path).await?;
        self.ensure_file(path, &meta)?;
        self.read_file_range(&meta, offset, length).await
    }

    async fn pwrite(&self, path: &str, content: &[u8], offset: u64) -> VfsResult<()> {
        let meta = self.metadata.resolve(path).await?;
        self.ensure_file(path, &meta)?;
        self.write_chunked_range(&meta, content, offset).await?;
        Ok(())
    }

    async fn append(&self, path: &str, content: &[u8]) -> VfsResult<u64> {
        let meta = self.metadata.resolve(path).await?;
        self.ensure_file(path, &meta)?;
        let len = meta
            .size
            .checked_add(u64::try_from(content.len()).map_err(|_| {
                VfsError::einval(format!("append content is too large: {}", content.len()))
            })?)
            .ok_or_else(|| VfsError::einval("append size overflows"))?;
        self.write_chunked_range(&meta, content, meta.size).await?;
        Ok(len)
    }
}

#[async_trait]
impl<M: MetadataStore, B: BlockStore> Snapshottable for ChunkedFs<M, B> {
    async fn snapshot(&self, root: u64) -> VfsResult<SnapshotId> {
        self.metadata.snapshot(root).await
    }

    async fn fork(&self, snap: SnapshotId) -> VfsResult<u64> {
        self.metadata.fork(snap).await
    }
}

fn ms_to_timespec(ms: u64) -> Timespec {
    Timespec {
        sec: (ms / 1_000) as i64,
        nsec: ((ms % 1_000) * 1_000_000) as u32,
    }
}
