#![allow(unsafe_code)]

use super::vfs::{
    normalize_path, VfsError, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat,
    VirtualUtimeSpec, S_IFDIR, S_IFLNK, S_IFREG,
};
use memmap2::Mmap;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tar::EntryType;

const MAX_TAR_INDEX_ENTRIES: usize = 200_000;
const MAX_TAR_CACHE_ARCHIVES: usize = 64;
const MAX_TAR_SYMLINKS: usize = 40;

/// Read-only filesystem backed directly by an uncompressed package tar.
///
/// The package tar already contains each file's bytes at a stable byte range,
/// so mounting it is cheaper and simpler than extracting it: we scan headers
/// once, store `path -> offset/size/metadata`, and serve reads from a shared
/// `mmap` slice. Extraction would create a duplicate host tree, thousands of
/// physical inodes, and a cleanup problem before reading the same bytes again.
///
/// Performance is intentionally front-loaded into a small index scan over tar
/// headers. File reads are an O(1) map lookup plus a page-cache-backed memory
/// slice; metadata and directory listings come from the in-memory index. The
/// mmap is keyed by content digest and shared across VMs, so RSS follows the
/// pages actually touched rather than the full archive size. The caller must
/// pass an immutable registry/package file: replacing by rename is fine because
/// this filesystem holds the opened file, but truncating the same inode during
/// a live VM would violate the mmap lifecycle and can SIGBUS on Unix.
///
/// This filesystem is mounted only as a granular package-version leaf such as
/// `/opt/agentos/pkgs/<pkg>/<version>`. Managed commands and `current` aliases
/// are separate symlink leaf mounts, while parent directories stay writable
/// overlay directories so user-installed files can coexist with managed ones.
pub struct TarFileSystem {
    archive: Arc<CachedTarArchive>,
    root: String,
}

impl TarFileSystem {
    pub fn open(path: impl AsRef<Path>, digest: impl Into<String>) -> VfsResult<Self> {
        Self::open_at(path, digest, "/")
    }

    pub fn open_at(
        path: impl AsRef<Path>,
        digest: impl Into<String>,
        root: &str,
    ) -> VfsResult<Self> {
        let digest = digest.into();
        let path = path.as_ref().to_path_buf();
        let archive = cached_archive(path, digest)?;
        let root = normalize_path(root);
        let node = archive.node(&root)?;
        if !matches!(node.kind, TarNodeKind::Directory) {
            return Err(VfsError::new(
                "ENOTDIR",
                format!("tar mount root is not a directory: {root}"),
            ));
        }
        Ok(Self { archive, root })
    }

    pub fn digest(&self) -> &str {
        &self.archive.digest
    }

    pub fn source_path(&self) -> &Path {
        &self.archive.path
    }

    pub fn archive_root(&self) -> &str {
        &self.root
    }

    fn to_archive_path(&self, path: &str) -> String {
        let normalized = normalize_path(path);
        if self.root == "/" {
            normalized
        } else if normalized == "/" {
            self.root.clone()
        } else {
            normalize_path(&format!(
                "{}/{}",
                self.root,
                normalized.trim_start_matches('/')
            ))
        }
    }

    fn to_guest_path(&self, archive_path: &str) -> VfsResult<String> {
        if self.root == "/" {
            return Ok(archive_path.to_owned());
        }
        if archive_path == self.root {
            return Ok(String::from("/"));
        }
        let prefix = format!("{}/", self.root.trim_end_matches('/'));
        let suffix = archive_path.strip_prefix(&prefix).ok_or_else(|| {
            VfsError::new(
                "EXDEV",
                format!("tar symlink resolved outside mounted subtree: {archive_path}"),
            )
        })?;
        Ok(format!("/{suffix}"))
    }

    fn ensure_within_root(&self, archive_path: &str) -> VfsResult<()> {
        if self.root == "/" || archive_path == self.root {
            return Ok(());
        }
        let prefix = format!("{}/", self.root.trim_end_matches('/'));
        if archive_path.starts_with(&prefix) {
            Ok(())
        } else {
            Err(VfsError::new(
                "EXDEV",
                format!("tar path resolved outside mounted subtree: {archive_path}"),
            ))
        }
    }

    fn resolve_path(&self, path: &str, follow_final_symlink: bool) -> VfsResult<String> {
        let normalized = self.to_archive_path(path);
        if normalized == "/" {
            return Ok(normalized);
        }

        let mut pending = path_components(&normalized);
        let mut current = String::from("/");
        let mut followed = 0usize;

        while let Some(component) = pending.pop_front() {
            let candidate = join_path(&current, &component);
            let node = self.archive.node(&candidate)?;
            let should_follow = follow_final_symlink || !pending.is_empty();

            if should_follow {
                if let TarNodeKind::Symlink { target } = &node.kind {
                    followed += 1;
                    if followed > MAX_TAR_SYMLINKS {
                        return Err(VfsError::new(
                            "ELOOP",
                            format!("too many levels of symbolic links, '{path}'"),
                        ));
                    }
                    let target_path = if target.starts_with('/') {
                        normalize_path(target)
                    } else {
                        normalize_path(&format!("{}/{}", parent_path(&candidate), target))
                    };
                    ensure_archive_path(&target_path)?;
                    let mut target_components = path_components(&target_path);
                    target_components.extend(pending);
                    pending = target_components;
                    current = String::from("/");
                    continue;
                }
            }

            if !pending.is_empty() && !matches!(node.kind, TarNodeKind::Directory) {
                return Err(VfsError::new(
                    "ENOTDIR",
                    format!("not a directory, realpath '{candidate}'"),
                ));
            }

            current = candidate;
        }

        self.ensure_within_root(&current)?;
        Ok(current)
    }

    fn readonly_error(op: &str, path: &str) -> VfsError {
        VfsError::new("EROFS", format!("read-only tar filesystem, {op} '{path}'"))
    }
}

impl VirtualFileSystem for TarFileSystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        let resolved = self.resolve_path(path, true)?;
        let node = self.archive.node(&resolved)?;
        let TarNodeKind::File { offset, size } = node.kind else {
            return Err(if matches!(node.kind, TarNodeKind::Directory) {
                VfsError::new(
                    "EISDIR",
                    format!("illegal operation on a directory, read '{path}'"),
                )
            } else {
                VfsError::new("EINVAL", format!("not a regular file, read '{path}'"))
            });
        };
        self.archive.validate_backing_file()?;
        let start = usize::try_from(offset)
            .map_err(|_| VfsError::new("EOVERFLOW", format!("tar offset too large: {offset}")))?;
        let len = usize::try_from(size)
            .map_err(|_| VfsError::new("EOVERFLOW", format!("tar member too large: {size}")))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| VfsError::new("EOVERFLOW", "tar member byte range overflows usize"))?;
        if end > self.archive.mmap.len() {
            return Err(VfsError::new(
                "EIO",
                format!(
                    "tar member range exceeds archive size: offset {offset} bytes + size {size} bytes > {} bytes",
                    self.archive.mmap.len()
                ),
            ));
        }
        Ok(self.archive.mmap[start..end].to_vec())
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        Ok(self
            .read_dir_with_types(path)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        let resolved = self.resolve_path(path, true)?;
        let node = self.archive.node(&resolved)?;
        if !matches!(node.kind, TarNodeKind::Directory) {
            return Err(VfsError::new(
                "ENOTDIR",
                format!("not a directory, readdir '{path}'"),
            ));
        }

        let children = self
            .archive
            .children
            .get(&resolved)
            .cloned()
            .unwrap_or_default();
        Ok(children
            .into_iter()
            .filter_map(|name| {
                let child_path = join_path(&resolved, &name);
                self.archive
                    .nodes
                    .get(&child_path)
                    .map(|child| VirtualDirEntry {
                        name,
                        is_directory: matches!(child.kind, TarNodeKind::Directory),
                        is_symbolic_link: matches!(child.kind, TarNodeKind::Symlink { .. }),
                    })
            })
            .collect())
    }

    fn write_file(&mut self, path: &str, _content: impl Into<Vec<u8>>) -> VfsResult<()> {
        Err(Self::readonly_error("write", path))
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly_error("mkdir", path))
    }

    fn mkdir(&mut self, path: &str, _recursive: bool) -> VfsResult<()> {
        Err(Self::readonly_error("mkdir", path))
    }

    fn exists(&self, path: &str) -> bool {
        self.resolve_path(path, true)
            .map(|resolved| self.archive.nodes.contains_key(&resolved))
            .unwrap_or(false)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        let resolved = self.resolve_path(path, true)?;
        Ok(self.archive.node(&resolved)?.stat())
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly_error("unlink", path))
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        Err(Self::readonly_error("rmdir", path))
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only tar filesystem, rename '{old_path}' to '{new_path}'"),
        ))
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        let resolved = self.resolve_path(path, true)?;
        self.to_guest_path(&resolved)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only tar filesystem, symlink '{link_path}' -> '{target}'"),
        ))
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        let normalized = self.resolve_path(path, false)?;
        match &self.archive.node(&normalized)?.kind {
            TarNodeKind::Symlink { target } => Ok(target.clone()),
            _ => Err(VfsError::new(
                "EINVAL",
                format!("not a symlink, readlink '{path}'"),
            )),
        }
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        let normalized = self.resolve_path(path, false)?;
        Ok(self.archive.node(&normalized)?.stat())
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        Err(VfsError::new(
            "EROFS",
            format!("read-only tar filesystem, link '{old_path}' to '{new_path}'"),
        ))
    }

    fn chmod(&mut self, path: &str, _mode: u32) -> VfsResult<()> {
        Err(Self::readonly_error("chmod", path))
    }

    fn chown(&mut self, path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        Err(Self::readonly_error("chown", path))
    }

    fn utimes(&mut self, path: &str, _atime_ms: u64, _mtime_ms: u64) -> VfsResult<()> {
        Err(Self::readonly_error("utimes", path))
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        _atime: VirtualUtimeSpec,
        _mtime: VirtualUtimeSpec,
        _follow_symlinks: bool,
    ) -> VfsResult<()> {
        Err(Self::readonly_error("utimes", path))
    }

    fn truncate(&mut self, path: &str, _length: u64) -> VfsResult<()> {
        Err(Self::readonly_error("truncate", path))
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        let content = self.read_file(path)?;
        let start = usize::try_from(offset)
            .map_err(|_| VfsError::new("EOVERFLOW", format!("pread offset too large: {offset}")))?;
        if start >= content.len() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(length).min(content.len());
        Ok(content[start..end].to_vec())
    }
}

struct CachedTarArchive {
    digest: String,
    path: PathBuf,
    file: File,
    mmap: Mmap,
    identity: FileIdentity,
    nodes: BTreeMap<String, TarNode>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl CachedTarArchive {
    fn node(&self, path: &str) -> VfsResult<&TarNode> {
        self.nodes
            .get(path)
            .ok_or_else(|| VfsError::new("ENOENT", format!("no such file or directory, '{path}'")))
    }

    fn validate_backing_file(&self) -> VfsResult<()> {
        let current = FileIdentity::from_file(&self.file)?;
        if current != self.identity {
            return Err(VfsError::new(
                "ESTALE",
                format!(
                    "tar archive backing file changed while mounted: {}",
                    self.path.display()
                ),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TarNode {
    kind: TarNodeKind,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime_ms: u64,
    ino: u64,
    dev: u64,
}

impl TarNode {
    fn stat(&self) -> VirtualStat {
        let size = match &self.kind {
            TarNodeKind::File { size, .. } => *size,
            TarNodeKind::Directory => 4096,
            TarNodeKind::Symlink { target } => target.len() as u64,
        };
        VirtualStat {
            mode: self.mode,
            size,
            blocks: size.div_ceil(512),
            dev: self.dev,
            rdev: 0,
            is_directory: matches!(self.kind, TarNodeKind::Directory),
            is_symbolic_link: matches!(self.kind, TarNodeKind::Symlink { .. }),
            atime_ms: self.mtime_ms,
            atime_nsec: 0,
            mtime_ms: self.mtime_ms,
            mtime_nsec: 0,
            ctime_ms: self.mtime_ms,
            ctime_nsec: 0,
            birthtime_ms: self.mtime_ms,
            ino: self.ino,
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
        }
    }
}

#[derive(Debug, Clone)]
enum TarNodeKind {
    File { offset: u64, size: u64 },
    Directory,
    Symlink { target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified_ms: u128,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl FileIdentity {
    fn from_file(file: &File) -> VfsResult<Self> {
        Self::from_metadata(file.metadata().map_err(io_to_vfs)?)
    }

    fn from_metadata(metadata: std::fs::Metadata) -> VfsResult<Self> {
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        Ok(Self {
            len: metadata.len(),
            modified_ms,
            #[cfg(unix)]
            dev: {
                use std::os::unix::fs::MetadataExt;
                metadata.dev()
            },
            #[cfg(unix)]
            ino: {
                use std::os::unix::fs::MetadataExt;
                metadata.ino()
            },
        })
    }
}

fn cached_archive(path: PathBuf, digest: String) -> VfsResult<Arc<CachedTarArchive>> {
    let cache = archive_cache();
    let mut guard = cache
        .lock()
        .map_err(|_| VfsError::new("EIO", "tar archive cache mutex poisoned"))?;

    if let Some(existing) = guard.archives.get(&digest).and_then(Weak::upgrade) {
        if existing.path == path {
            return Ok(existing);
        }
        return Err(VfsError::new(
            "EINVAL",
            format!(
                "tar digest collision or moved source: digest {digest} already maps to {} not {}",
                existing.path.display(),
                path.display()
            ),
        ));
    }

    guard.archives.retain(|key, weak| {
        let live = weak.strong_count() > 0;
        if !live {
            tracing::warn!(
                digest = key.as_str(),
                "evicting unused tar archive cache entry"
            );
        }
        live
    });

    if guard.archives.len() >= MAX_TAR_CACHE_ARCHIVES {
        return Err(VfsError::new(
            "ENOMEM",
            format!(
                "tar archive cache entries exceeded: {} entries > {} entries (raise via invariant.tarArchiveCacheEntries)",
                guard.archives.len() + 1,
                MAX_TAR_CACHE_ARCHIVES
            ),
        ));
    }

    let archive = Arc::new(load_archive(&path, digest.clone())?);
    guard.archives.insert(digest, Arc::downgrade(&archive));
    Ok(archive)
}

fn archive_cache() -> &'static Mutex<TarArchiveCache> {
    static CACHE: OnceLock<Mutex<TarArchiveCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TarArchiveCache::default()))
}

#[derive(Default)]
struct TarArchiveCache {
    archives: BTreeMap<String, Weak<CachedTarArchive>>,
}

fn load_archive(path: &Path, digest: String) -> VfsResult<CachedTarArchive> {
    let file = File::open(path).map_err(io_to_vfs)?;
    let identity = FileIdentity::from_file(&file)?;
    let mmap = unsafe {
        // SAFETY: TarFileSystem is only constructed for immutable package tar
        // artifacts. We hold the opened file for the lifetime of the mmap and
        // validate size/identity before reading from mapped member ranges. A
        // caller that truncates the same inode while a VM is live violates the
        // package-store lifecycle documented on TarFileSystem.
        Mmap::map(&file)
    }
    .map_err(io_to_vfs)?;

    let mut archive = tar::Archive::new(file.try_clone().map_err(io_to_vfs)?);
    let mut nodes = BTreeMap::new();
    let mut children = BTreeMap::<String, BTreeSet<String>>::new();
    let dev = digest_device(&digest);
    let mut next_ino = 1u64;

    nodes.insert(
        String::from("/"),
        TarNode {
            kind: TarNodeKind::Directory,
            mode: S_IFDIR | 0o755,
            uid: 0,
            gid: 0,
            mtime_ms: 0,
            ino: next_ino,
            dev,
        },
    );
    next_ino += 1;
    children.entry(String::from("/")).or_default();

    let entries = archive.entries().map_err(io_to_vfs)?;
    for entry in entries {
        let entry = entry.map_err(io_to_vfs)?;
        let path = normalize_tar_member_path(&entry)?;
        if path == "/" {
            continue;
        }
        ensure_index_capacity(nodes.len() + 1)?;
        synthesize_parent_dirs(&path, dev, &mut next_ino, &mut nodes, &mut children)?;

        let header = entry.header();
        let entry_type = header.entry_type();
        let mode = header.mode().unwrap_or(0o755);
        let uid = header.uid().unwrap_or(0) as u32;
        let gid = header.gid().unwrap_or(0) as u32;
        let mtime_ms = header.mtime().unwrap_or(0).saturating_mul(1_000);
        let kind = if entry_type.is_dir() {
            TarNodeKind::Directory
        } else if entry_type.is_symlink() {
            let target = entry
                .link_name()
                .map_err(io_to_vfs)?
                .ok_or_else(|| VfsError::new("EINVAL", format!("missing linkname for {path}")))?
                .to_string_lossy()
                .into_owned();
            TarNodeKind::Symlink { target }
        } else if entry_type.is_file() || entry_type == EntryType::Continuous {
            TarNodeKind::File {
                offset: entry.raw_file_position(),
                size: header.size().map_err(io_to_vfs)?,
            }
        } else {
            continue;
        };

        let mode = match kind {
            TarNodeKind::Directory => S_IFDIR | (mode & 0o7777),
            TarNodeKind::File { .. } => S_IFREG | (mode & 0o7777),
            TarNodeKind::Symlink { .. } => S_IFLNK | (mode & 0o7777).max(0o777),
        };
        nodes.insert(
            path.clone(),
            TarNode {
                kind,
                mode,
                uid,
                gid,
                mtime_ms,
                ino: next_ino,
                dev,
            },
        );
        next_ino += 1;
        add_child(&path, &mut children);
        if matches!(
            nodes.get(&path).map(|node| &node.kind),
            Some(TarNodeKind::Directory)
        ) {
            children.entry(path).or_default();
        }
    }

    Ok(CachedTarArchive {
        digest,
        path: path.to_path_buf(),
        file,
        mmap,
        identity,
        nodes,
        children,
    })
}

fn synthesize_parent_dirs(
    path: &str,
    dev: u64,
    next_ino: &mut u64,
    nodes: &mut BTreeMap<String, TarNode>,
    children: &mut BTreeMap<String, BTreeSet<String>>,
) -> VfsResult<()> {
    let mut current = String::from("/");
    let components = path_components(path);
    let parent_count = components.len().saturating_sub(1);
    for component in components.into_iter().take(parent_count) {
        let parent = current.clone();
        current = join_path(&current, &component);
        if !nodes.contains_key(&current) {
            ensure_index_capacity(nodes.len() + 1)?;
            nodes.insert(
                current.clone(),
                TarNode {
                    kind: TarNodeKind::Directory,
                    mode: S_IFDIR | 0o755,
                    uid: 0,
                    gid: 0,
                    mtime_ms: 0,
                    ino: *next_ino,
                    dev,
                },
            );
            *next_ino += 1;
        }
        children.entry(parent).or_default().insert(component);
        children.entry(current.clone()).or_default();
    }
    Ok(())
}

fn add_child(path: &str, children: &mut BTreeMap<String, BTreeSet<String>>) {
    let parent = parent_path(path);
    let name = basename(path);
    children.entry(parent).or_default().insert(name);
}

fn ensure_index_capacity(observed: usize) -> VfsResult<()> {
    if observed > MAX_TAR_INDEX_ENTRIES {
        return Err(VfsError::new(
            "ENOMEM",
            format!(
                "tar filesystem index entries exceeded: {observed} entries > {MAX_TAR_INDEX_ENTRIES} entries (raise via invariant.tarFilesystemIndexEntries)"
            ),
        ));
    }
    Ok(())
}

fn normalize_tar_member_path(entry: &tar::Entry<'_, File>) -> VfsResult<String> {
    let path = entry.path().map_err(io_to_vfs)?;
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(VfsError::new(
                    "EINVAL",
                    format!("tar member path escapes archive root: {}", path.display()),
                ));
            }
        }
    }
    if parts.is_empty() {
        Ok(String::from("/"))
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn ensure_archive_path(path: &str) -> VfsResult<()> {
    let normalized = normalize_path(path);
    if normalized != path {
        return Err(VfsError::new(
            "EINVAL",
            format!("path normalization mismatch in tar filesystem: {path}"),
        ));
    }
    Ok(())
}

fn path_components(path: &str) -> std::collections::VecDeque<String> {
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

fn digest_device(digest: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    digest.hash(&mut hasher);
    hasher.finish().max(1)
}

fn io_to_vfs(error: io::Error) -> VfsError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "ENOENT",
        io::ErrorKind::PermissionDenied => "EACCES",
        io::ErrorKind::AlreadyExists => "EEXIST",
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => "EINVAL",
        io::ErrorKind::UnexpectedEof => "EIO",
        _ => "EIO",
    };
    VfsError::new(code, error.to_string())
}
