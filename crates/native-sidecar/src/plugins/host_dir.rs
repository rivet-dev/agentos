use nix::errno::Errno;
#[cfg(not(target_os = "macos"))]
use nix::fcntl::{openat2, OpenHow, ResolveFlag};
use nix::fcntl::{readlinkat, renameat, AtFlags, OFlag};
use nix::libc;

// macOS has no `O_PATH` (metadata-only anchor fd). The host-mount code only uses
// O_PATH fds as anchors that are re-opened via `/dev/fd/N`, so a read-only open
// is an adequate stand-in there; the real access mode is applied on re-open.
#[cfg(not(target_os = "macos"))]
const O_PATH_ANCHOR: OFlag = OFlag::O_PATH;
#[cfg(target_os = "macos")]
const O_PATH_ANCHOR: OFlag = OFlag::O_RDONLY;
use agentos_execution::{
    GuestModuleReader, LocalModuleResolutionCache, ModuleFsReader, ModuleResolveMode,
    ModuleResolver,
};
use agentos_kernel::mount_plugin::{
    FileSystemPluginFactory, OpenFileSystemPluginRequest, PluginError,
};
use agentos_kernel::mount_table::{
    MountedFileSystem, MountedVirtualFileSystem, ReadOnlyFileSystem,
};
use agentos_kernel::resource_accounting::DEFAULT_MAX_PREAD_BYTES;
use agentos_kernel::vfs::{
    normalize_path, VfsError, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat,
    VirtualTimeSpec, VirtualUtimeSpec,
};
use nix::sys::stat::{fstatat, mkdirat, utimensat, Mode, SFlag, UtimensatFlags};
use nix::sys::time::TimeSpec;
use nix::unistd::{chown, linkat, symlinkat, unlinkat, Gid, Uid, UnlinkatFlags};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use vfs::posix::TarFileSystem;

const MAX_HOST_DIR_READ_BYTES: usize = DEFAULT_MAX_PREAD_BYTES;

#[derive(Debug)]
struct AnchoredFd {
    fd: RawFd,
}

impl AnchoredFd {
    #[cfg(not(target_os = "macos"))]
    fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.fd))
    }

    // macOS exposes per-fd paths under `/dev/fd/N` (the kernel dups the fd),
    // serving the same role as Linux's `/proc/self/fd/N` for re-opening a
    // *file* fd. Unlike `/proc/self/fd/N` it is NOT a readdir-able directory,
    // so directory enumeration goes through [`readdir_path`]; child mutations
    // use the fd-relative `*at` syscalls below.
    #[cfg(target_os = "macos")]
    fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/dev/fd/{}", self.fd))
    }

    // Path to enumerate this fd's directory entries. Linux can `readdir`
    // `/proc/self/fd/N` directly; macOS `/dev/fd/N` yields `ENOTDIR`, so recover
    // the fd's real host path via `fcntl(F_GETPATH)` (see [`anchored_fd_real_path`]).
    #[cfg(not(target_os = "macos"))]
    fn readdir_path(&self) -> io::Result<PathBuf> {
        Ok(self.proc_path())
    }
    #[cfg(target_os = "macos")]
    fn readdir_path(&self) -> io::Result<PathBuf> {
        anchored_fd_real_path(self)
    }
}

impl AsRawFd for AnchoredFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for AnchoredFd {
    fn drop(&mut self) {
        let _ = nix::unistd::close(self.fd);
    }
}

/// Recover the real host path an anchored fd points at. Linux reads the magic
/// symlink `/proc/self/fd/N`; macOS uses `fcntl(F_GETPATH)` (see
/// [`crate::macos_fs::fd_real_path`]).
#[cfg(not(target_os = "macos"))]
fn anchored_fd_real_path(fd: &AnchoredFd) -> io::Result<PathBuf> {
    fs::read_link(fd.proc_path())
}

#[cfg(target_os = "macos")]
fn anchored_fd_real_path(fd: &AnchoredFd) -> io::Result<PathBuf> {
    crate::macos_fs::fd_real_path(fd.as_raw_fd())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostDirMountConfig {
    host_path: String,
    read_only: Option<bool>,
}

#[derive(Debug)]
pub(crate) struct HostDirMountPlugin;

pub(crate) trait HostDirReadLimitContext {
    fn host_dir_max_read_bytes(&self) -> Option<usize>;
}

impl HostDirReadLimitContext for () {
    fn host_dir_max_read_bytes(&self) -> Option<usize> {
        Some(MAX_HOST_DIR_READ_BYTES)
    }
}

impl<Context> FileSystemPluginFactory<Context> for HostDirMountPlugin
where
    Context: HostDirReadLimitContext,
{
    fn plugin_id(&self) -> &'static str {
        "host_dir"
    }

    fn open(
        &self,
        request: OpenFileSystemPluginRequest<'_, Context>,
    ) -> Result<Box<dyn MountedFileSystem>, PluginError> {
        let max_read_bytes = request.context.host_dir_max_read_bytes();
        self.open_with_read_limit(request, max_read_bytes)
    }
}

impl HostDirMountPlugin {
    fn open_with_read_limit<Context>(
        &self,
        request: OpenFileSystemPluginRequest<'_, Context>,
        max_read_bytes: Option<usize>,
    ) -> Result<Box<dyn MountedFileSystem>, PluginError> {
        let config: HostDirMountConfig = serde_json::from_value(request.config.clone())
            .map_err(|error| PluginError::invalid_input(error.to_string()))?;
        let filesystem = HostDirFilesystem::new_with_read_limit(&config.host_path, max_read_bytes)?;
        let mounted = MountedVirtualFileSystem::new(filesystem);

        if config.read_only.unwrap_or(false) {
            Ok(Box::new(ReadOnlyFileSystem::new(mounted)))
        } else {
            Ok(Box::new(mounted))
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HostDirFilesystem {
    host_root: PathBuf,
    host_root_dir: Arc<File>,
    max_read_bytes: Option<usize>,
}

impl HostDirFilesystem {
    #[allow(dead_code)]
    pub(crate) fn new(host_path: impl AsRef<Path>) -> VfsResult<Self> {
        Self::new_with_read_limit(host_path, Some(MAX_HOST_DIR_READ_BYTES))
    }

    pub(crate) fn new_with_read_limit(
        host_path: impl AsRef<Path>,
        max_read_bytes: Option<usize>,
    ) -> VfsResult<Self> {
        let host_path_str = host_path.as_ref().to_string_lossy().into_owned();
        let canonical_root = fs::canonicalize(host_path.as_ref())
            .map_err(|error| io_error_to_vfs("open", &host_path_str, error))?;
        let metadata = fs::metadata(&canonical_root)
            .map_err(|error| io_error_to_vfs("stat", &host_path_str, error))?;
        if !metadata.is_dir() {
            return Err(VfsError::new(
                "ENOTDIR",
                format!(
                    "host_dir root is not a directory: {}",
                    canonical_root.display()
                ),
            ));
        }

        Ok(Self {
            host_root: canonical_root.clone(),
            host_root_dir: Arc::new(
                File::open(&canonical_root).map_err(|error| io_error_to_vfs("open", "/", error))?,
            ),
            max_read_bytes,
        })
    }

    fn ensure_within_root(&self, resolved: &Path, virtual_path: &str) -> VfsResult<()> {
        if resolved == self.host_root {
            return Ok(());
        }

        if resolved.starts_with(&self.host_root) {
            return Ok(());
        }

        Err(VfsError::access_denied(
            "open",
            virtual_path,
            Some("path escapes host directory"),
        ))
    }

    fn lexical_host_path(&self, path: &str) -> VfsResult<PathBuf> {
        let normalized = normalize_path(path);
        let relative = normalized.trim_start_matches('/');
        let joined = lexical_normalize_path(&self.host_root.join(relative));
        self.ensure_within_root(&joined, &normalized)?;
        Ok(joined)
    }

    fn relative_virtual_path(&self, path: &str) -> (String, PathBuf) {
        let normalized = normalize_path(path);
        let relative = normalized.trim_start_matches('/');
        let relative = if relative.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(relative)
        };
        (normalized, relative)
    }

    #[cfg(not(target_os = "macos"))]
    fn resolve_flags() -> ResolveFlag {
        ResolveFlag::RESOLVE_BENEATH | ResolveFlag::RESOLVE_NO_MAGICLINKS
    }

    fn open_beneath(&self, relative: &Path, flags: OFlag, mode: Mode) -> VfsResult<AnchoredFd> {
        let relative_display = relative.display().to_string();
        let fd = self
            .resolve_beneath_fd(relative, flags, mode)
            .map_err(|error| match error {
                Errno::EXDEV => VfsError::access_denied(
                    "open",
                    &relative_display,
                    Some("path escapes host directory"),
                ),
                other => io_error_to_vfs("open", &relative_display, nix_to_io(other)),
            })?;
        Ok(AnchoredFd { fd })
    }

    /// Open `relative` strictly beneath the mount root, returning an owned raw
    /// fd. Linux uses `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)`; macOS
    /// has no such syscall and uses cap-std's audited resolve-beneath instead
    /// (see [`crate::macos_fs`]).
    #[cfg(not(target_os = "macos"))]
    fn resolve_beneath_fd(
        &self,
        relative: &Path,
        flags: OFlag,
        mode: Mode,
    ) -> Result<RawFd, Errno> {
        openat2(
            self.host_root_dir.as_raw_fd(),
            relative,
            OpenHow::new()
                .flags(flags | OFlag::O_CLOEXEC)
                .mode(mode)
                .resolve(Self::resolve_flags()),
        )
    }

    #[cfg(target_os = "macos")]
    fn resolve_beneath_fd(
        &self,
        relative: &Path,
        flags: OFlag,
        mode: Mode,
    ) -> Result<RawFd, Errno> {
        crate::macos_fs::resolve_beneath(&self.host_root, relative, flags, mode)
    }

    fn open_directory_beneath(&self, relative: &Path) -> VfsResult<AnchoredFd> {
        self.open_beneath(
            relative,
            OFlag::O_DIRECTORY | OFlag::O_RDONLY,
            Mode::empty(),
        )
    }

    fn host_path_for_fd(&self, fd: &AnchoredFd, virtual_path: &str) -> VfsResult<PathBuf> {
        let host_path = anchored_fd_real_path(fd)
            .map_err(|error| io_error_to_vfs("open", virtual_path, error))?;
        self.ensure_within_root(&host_path, virtual_path)?;
        Ok(host_path)
    }

    #[cfg(not(target_os = "macos"))]
    fn open_metadata_beneath(&self, path: &str, op: &'static str) -> VfsResult<AnchoredFd> {
        let (_, relative) = self.relative_virtual_path(path);
        let handle =
            self.open_beneath(&relative, O_PATH_ANCHOR | OFlag::O_NOFOLLOW, Mode::empty())?;
        let metadata =
            fs::metadata(handle.proc_path()).map_err(|error| io_error_to_vfs(op, path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(VfsError::new(
                "EPERM",
                format!("{op} '{path}': metadata operations do not follow symlinks"),
            ));
        }
        Ok(handle)
    }

    // macOS has no `O_PATH`, and `open(O_NOFOLLOW)` on a symlink fails outright,
    // so the Linux path above (open the link as an anchor, then inspect it)
    // cannot run — the resolve-beneath refusal instead surfaces as `EACCES`,
    // diverging from Linux's `EPERM`. Detect the symlink directly: `lstat` the
    // final component through the resolved parent fd and reject it with `EPERM`;
    // otherwise open the (non-symlink) target as the metadata anchor.
    #[cfg(target_os = "macos")]
    fn open_metadata_beneath(&self, path: &str, op: &'static str) -> VfsResult<AnchoredFd> {
        // The mount root has no final component to lstat and is always a
        // directory (never a symlink), so open it directly as the anchor —
        // matching Linux, where the `O_PATH | O_NOFOLLOW` open of the root
        // succeeds. (`split_parent` would otherwise reject `/` with EINVAL.)
        let (_, root_relative) = self.relative_virtual_path(path);
        if root_relative.file_name().is_none() {
            return self.open_beneath(&root_relative, O_PATH_ANCHOR, Mode::empty());
        }
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        let stat = fstatat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|error| io_error_to_vfs(op, &normalized, nix_to_io(error)))?;
        if stat.st_mode & SFlag::S_IFMT.bits() == SFlag::S_IFLNK.bits() {
            return Err(VfsError::new(
                "EPERM",
                format!("{op} '{path}': metadata operations do not follow symlinks"),
            ));
        }
        let (_, relative) = self.relative_virtual_path(path);
        self.open_beneath(&relative, O_PATH_ANCHOR, Mode::empty())
    }

    fn ensure_directory_tree(
        &self,
        relative_dir: &Path,
        virtual_path: &str,
        mode: u32,
    ) -> VfsResult<()> {
        if relative_dir == Path::new(".") {
            return Ok(());
        }

        let mut prefix = PathBuf::new();
        for component in relative_dir.components() {
            match component {
                Component::Normal(segment) => prefix.push(segment),
                Component::CurDir => continue,
                _ => {
                    return Err(VfsError::new(
                        "EINVAL",
                        format!("invalid host_dir component in {virtual_path}"),
                    ));
                }
            }

            if self.open_directory_beneath(&prefix).is_ok() {
                continue;
            }

            let parent = match prefix.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent_dir = self.open_directory_beneath(parent)?;
            let name = prefix.file_name().ok_or_else(|| {
                VfsError::new("EINVAL", format!("invalid directory path: {virtual_path}"))
            })?;
            match mkdirat(
                Some(parent_dir.as_raw_fd()),
                name,
                Mode::from_bits_truncate(mode as _),
            ) {
                Ok(()) => {}
                Err(Errno::EEXIST) => {}
                Err(error) => {
                    return Err(io_error_to_vfs("mkdir", virtual_path, nix_to_io(error)));
                }
            }
        }

        Ok(())
    }

    fn split_parent(
        &self,
        path: &str,
        create_parent_dirs: bool,
    ) -> VfsResult<(AnchoredFd, PathBuf, std::ffi::OsString, String)> {
        let (normalized, relative) = self.relative_virtual_path(path);
        let name = relative.file_name().ok_or_else(|| {
            VfsError::new(
                "EINVAL",
                format!("path does not reference an entry: {normalized}"),
            )
        })?;
        let parent = match relative.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        if create_parent_dirs {
            self.ensure_directory_tree(&parent, &normalized, 0o755)?;
        }
        let parent_dir = self.open_directory_beneath(&parent)?;
        Ok((parent_dir, parent, name.to_os_string(), normalized))
    }

    fn host_to_virtual_path(&self, host_path: &Path, virtual_path: &str) -> VfsResult<String> {
        let normalized = lexical_normalize_path(host_path);
        self.ensure_within_root(&normalized, virtual_path)?;
        let relative = normalized.strip_prefix(&self.host_root).map_err(|_| {
            VfsError::access_denied("open", virtual_path, Some("path escapes host directory"))
        })?;

        if relative.as_os_str().is_empty() {
            return Ok(String::from("/"));
        }

        let segments = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        Ok(format!("/{}", segments.join("/")))
    }

    fn existing_utime_specs(
        &self,
        parent_dir: &AnchoredFd,
        name: &std::ffi::OsStr,
        virtual_path: &str,
        follow_symlinks: bool,
    ) -> VfsResult<(VirtualTimeSpec, VirtualTimeSpec)> {
        let flags = if follow_symlinks {
            AtFlags::empty()
        } else {
            AtFlags::AT_SYMLINK_NOFOLLOW
        };
        let stat = fstatat(Some(parent_dir.as_raw_fd()), name, flags)
            .map_err(|error| io_error_to_vfs("utimes", virtual_path, nix_to_io(error)))?;
        let atime = VirtualTimeSpec::new(
            stat.st_atime,
            stat.st_atime_nsec.clamp(0, 999_999_999) as u32,
        )?;
        let mtime = VirtualTimeSpec::new(
            stat.st_mtime,
            stat.st_mtime_nsec.clamp(0, 999_999_999) as u32,
        )?;
        Ok((atime, mtime))
    }

    fn resolve_utime_timespec(spec: VirtualUtimeSpec, existing: VirtualTimeSpec) -> TimeSpec {
        match spec {
            VirtualUtimeSpec::Set(spec) => TimeSpec::new(spec.sec, spec.nsec as libc::c_long),
            VirtualUtimeSpec::Now => TimeSpec::new(0, libc::UTIME_NOW),
            VirtualUtimeSpec::Omit => TimeSpec::new(existing.sec, libc::UTIME_OMIT),
        }
    }

    fn apply_utimens(
        &self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        if follow_symlinks {
            let _ = self.open_metadata_beneath(path, "utimes")?;
        }
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        let existing = match (atime, mtime) {
            (VirtualUtimeSpec::Omit, _) | (_, VirtualUtimeSpec::Omit) => {
                Some(self.existing_utime_specs(
                    &parent_dir,
                    name.as_os_str(),
                    &normalized,
                    follow_symlinks,
                )?)
            }
            _ => None,
        };
        let existing_atime = existing
            .as_ref()
            .map(|(atime, _)| *atime)
            .unwrap_or(VirtualTimeSpec { sec: 0, nsec: 0 });
        let existing_mtime = existing
            .as_ref()
            .map(|(_, mtime)| *mtime)
            .unwrap_or(VirtualTimeSpec { sec: 0, nsec: 0 });
        let times = [
            Self::resolve_utime_timespec(atime, existing_atime),
            Self::resolve_utime_timespec(mtime, existing_mtime),
        ];
        let flags = if follow_symlinks {
            UtimensatFlags::FollowSymlink
        } else {
            UtimensatFlags::NoFollowSymlink
        };
        utimensat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            &times[0],
            &times[1],
            flags,
        )
        .map_err(|error| io_error_to_vfs("utimes", &normalized, nix_to_io(error)))
    }

    fn stat_from_metadata(metadata: fs::Metadata) -> VirtualStat {
        let atime_ms = metadata.atime().max(0) as u64 * 1_000
            + (metadata.atime_nsec().max(0) as u64 / 1_000_000);
        let atime_nsec = metadata.atime_nsec().clamp(0, 999_999_999) as u32;
        let mtime_ms = metadata.mtime().max(0) as u64 * 1_000
            + (metadata.mtime_nsec().max(0) as u64 / 1_000_000);
        let mtime_nsec = metadata.mtime_nsec().clamp(0, 999_999_999) as u32;
        let ctime_ms = metadata.ctime().max(0) as u64 * 1_000
            + (metadata.ctime_nsec().max(0) as u64 / 1_000_000);
        let ctime_nsec = metadata.ctime_nsec().clamp(0, 999_999_999) as u32;
        VirtualStat {
            mode: metadata.mode(),
            size: metadata.size(),
            blocks: metadata.blocks(),
            dev: metadata.dev(),
            rdev: metadata.rdev(),
            is_directory: metadata.is_dir(),
            is_symbolic_link: metadata.file_type().is_symlink(),
            atime_ms,
            atime_nsec,
            mtime_ms,
            mtime_nsec,
            ctime_ms,
            ctime_nsec,
            birthtime_ms: ctime_ms,
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }

    #[allow(clippy::unnecessary_cast)]
    fn stat_from_file_stat(stat: nix::sys::stat::FileStat) -> VirtualStat {
        let file_type = SFlag::from_bits_truncate(stat.st_mode);
        let atime_ms =
            stat.st_atime.max(0) as u64 * 1_000 + (stat.st_atime_nsec.max(0) as u64 / 1_000_000);
        let atime_nsec = stat.st_atime_nsec.clamp(0, 999_999_999) as u32;
        let mtime_ms =
            stat.st_mtime.max(0) as u64 * 1_000 + (stat.st_mtime_nsec.max(0) as u64 / 1_000_000);
        let mtime_nsec = stat.st_mtime_nsec.clamp(0, 999_999_999) as u32;
        let ctime_ms =
            stat.st_ctime.max(0) as u64 * 1_000 + (stat.st_ctime_nsec.max(0) as u64 / 1_000_000);
        let ctime_nsec = stat.st_ctime_nsec.clamp(0, 999_999_999) as u32;

        VirtualStat {
            // Widen for platform differences: mode_t/dev_t/nlink_t are narrower
            // on macOS (u16/i32/u16) than on Linux.
            mode: stat.st_mode as u32,
            size: stat.st_size as u64,
            blocks: stat.st_blocks as u64,
            dev: stat.st_dev as u64,
            rdev: stat.st_rdev as u64,
            is_directory: file_type == SFlag::S_IFDIR,
            is_symbolic_link: file_type == SFlag::S_IFLNK,
            atime_ms,
            atime_nsec,
            mtime_ms,
            mtime_nsec,
            ctime_ms,
            ctime_nsec,
            birthtime_ms: ctime_ms,
            ino: stat.st_ino,
            // st_nlink is u64 on x86_64 but u32 on aarch64 / u16 on macOS; widen.
            nlink: stat.st_nlink as u64,
            uid: stat.st_uid,
            gid: stat.st_gid,
        }
    }

    fn write_all_at(
        &self,
        file: &File,
        content: &[u8],
        mut offset: u64,
        path: &str,
    ) -> VfsResult<()> {
        let mut written = 0usize;
        while written < content.len() {
            let bytes_written = file
                .write_at(&content[written..], offset)
                .map_err(|error| io_error_to_vfs("write", path, error))?;
            if bytes_written == 0 {
                return Err(io_error_to_vfs(
                    "write",
                    path,
                    io::Error::new(io::ErrorKind::WriteZero, "failed to write whole buffer"),
                ));
            }

            written += bytes_written;
            offset = offset.checked_add(bytes_written as u64).ok_or_else(|| {
                VfsError::new("EINVAL", format!("pwrite offset overflow: {path}"))
            })?;
        }

        Ok(())
    }

    fn check_read_length(&self, path: &str, length: usize) -> VfsResult<()> {
        if let Some(limit) = self.max_read_bytes {
            if length <= limit {
                return Ok(());
            }

            return Err(VfsError::new(
                "EINVAL",
                format!("read length {length} exceeds host_dir limit {limit}: {path}"),
            ));
        }

        Ok(())
    }

    fn check_full_read_metadata(&self, path: &str, size: u64) -> VfsResult<()> {
        if let Some(limit) = self.max_read_bytes {
            if size <= limit as u64 {
                return Ok(());
            }

            return Err(VfsError::new(
                "EINVAL",
                format!("file size {size} exceeds host_dir read limit {limit}: {path}"),
            ));
        }

        Ok(())
    }

    fn read_to_end_bounded(&self, file: &mut File, path: &str) -> VfsResult<Vec<u8>> {
        let mut buffer = Vec::new();
        match self.max_read_bytes {
            Some(limit) => {
                Read::by_ref(file)
                    .take((limit as u64).saturating_add(1))
                    .read_to_end(&mut buffer)
                    .map_err(|error| io_error_to_vfs("open", path, error))?;
            }
            None => {
                file.read_to_end(&mut buffer)
                    .map_err(|error| io_error_to_vfs("open", path, error))?;
            }
        }
        self.check_read_length(path, buffer.len())?;
        Ok(buffer)
    }

    fn write_file_with_creation_mode(
        &mut self,
        path: &str,
        content: Vec<u8>,
        file_mode: u32,
    ) -> VfsResult<()> {
        let (_, relative) = self.relative_virtual_path(path);
        if let Some(parent) = relative.parent() {
            self.ensure_directory_tree(parent, path, 0o755)?;
        }
        let handle = self.open_beneath(
            &relative,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
            Mode::from_bits_truncate(file_mode as _),
        )?;
        let mut file = File::options()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(handle.proc_path())
            .map_err(|error| io_error_to_vfs("write", path, error))?;
        file.write_all(&content)
            .map_err(|error| io_error_to_vfs("write", path, error))
    }

    fn create_dir_with_creation_mode(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        mkdirat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            Mode::from_bits_truncate(mode as _),
        )
        .map_err(|error| io_error_to_vfs("mkdir", &normalized, nix_to_io(error)))
    }

    fn mkdir_with_creation_mode(
        &mut self,
        path: &str,
        recursive: bool,
        mode: u32,
    ) -> VfsResult<()> {
        if recursive {
            let (normalized, relative) = self.relative_virtual_path(path);
            self.ensure_directory_tree(&relative, &normalized, mode)
        } else {
            self.create_dir_with_creation_mode(path, mode)
        }
    }
}

impl VirtualFileSystem for HostDirFilesystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        let (_, relative) = self.relative_virtual_path(path);
        let handle = self.open_beneath(&relative, OFlag::O_RDONLY, Mode::empty())?;
        let mut file =
            File::open(handle.proc_path()).map_err(|error| io_error_to_vfs("open", path, error))?;
        self.check_full_read_metadata(
            path,
            file.metadata()
                .map_err(|error| io_error_to_vfs("open", path, error))?
                .len(),
        )?;
        self.read_to_end_bounded(&mut file, path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        let (_, relative) = self.relative_virtual_path(path);
        let directory = self.open_directory_beneath(&relative)?;
        let readdir_path = directory
            .readdir_path()
            .map_err(|error| io_error_to_vfs("readdir", path, error))?;
        let mut entries = fs::read_dir(readdir_path)
            .map_err(|error| io_error_to_vfs("readdir", path, error))?
            .map(|entry| {
                entry
                    .map_err(|error| io_error_to_vfs("readdir", path, error))
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
            })
            .collect::<VfsResult<Vec<_>>>()?;
        entries.sort();
        Ok(entries)
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        let (_, relative) = self.relative_virtual_path(path);
        let directory = self.open_directory_beneath(&relative)?;
        let readdir_path = directory
            .readdir_path()
            .map_err(|error| io_error_to_vfs("readdir", path, error))?;
        let mut entries = fs::read_dir(readdir_path)
            .map_err(|error| io_error_to_vfs("readdir", path, error))?
            .map(|entry| {
                let entry = entry.map_err(|error| io_error_to_vfs("readdir", path, error))?;
                let file_type = entry
                    .file_type()
                    .map_err(|error| io_error_to_vfs("readdir", path, error))?;
                Ok(VirtualDirEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    is_directory: file_type.is_dir(),
                    is_symbolic_link: file_type.is_symlink(),
                })
            })
            .collect::<VfsResult<Vec<_>>>()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn write_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        self.write_file_with_creation_mode(path, content.into(), 0o644)
    }

    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: impl Into<Vec<u8>>,
        mode: Option<u32>,
    ) -> VfsResult<()> {
        self.write_file_with_creation_mode(path, content.into(), mode.unwrap_or(0o666))
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        self.create_dir_with_creation_mode(path, 0o755)
    }

    fn create_dir_with_mode(&mut self, path: &str, mode: Option<u32>) -> VfsResult<()> {
        self.create_dir_with_creation_mode(path, mode.unwrap_or(0o777))
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        self.mkdir_with_creation_mode(path, recursive, 0o755)
    }

    fn mkdir_with_mode(&mut self, path: &str, recursive: bool, mode: Option<u32>) -> VfsResult<()> {
        self.mkdir_with_creation_mode(path, recursive, mode.unwrap_or(0o777))
    }

    fn exists(&self, path: &str) -> bool {
        let (_, relative) = self.relative_virtual_path(path);
        self.open_beneath(&relative, O_PATH_ANCHOR, Mode::empty())
            .is_ok()
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        let (_, relative) = self.relative_virtual_path(path);
        let handle = self.open_beneath(&relative, O_PATH_ANCHOR, Mode::empty())?;
        fs::metadata(handle.proc_path())
            .map(Self::stat_from_metadata)
            .map_err(|error| io_error_to_vfs("stat", path, error))
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        unlinkat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            UnlinkatFlags::NoRemoveDir,
        )
        .map_err(|error| io_error_to_vfs("unlink", &normalized, nix_to_io(error)))
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        unlinkat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(|error| io_error_to_vfs("rmdir", &normalized, nix_to_io(error)))
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (old_parent_dir, _, old_name, old_normalized) = self.split_parent(old_path, false)?;
        let (new_parent_dir, _, new_name, _) = self.split_parent(new_path, true)?;
        renameat(
            Some(old_parent_dir.as_raw_fd()),
            old_name.as_os_str(),
            Some(new_parent_dir.as_raw_fd()),
            new_name.as_os_str(),
        )
        .map_err(|error| io_error_to_vfs("rename", &old_normalized, nix_to_io(error)))
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        let (_, relative) = self.relative_virtual_path(path);
        let file = self.open_beneath(&relative, O_PATH_ANCHOR, Mode::empty())?;
        let resolved = self.host_path_for_fd(&file, path)?;
        self.host_to_virtual_path(&resolved, path)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        let (parent_dir, _, name, normalized) = self.split_parent(link_path, true)?;
        let parent_host_path = self.host_path_for_fd(&parent_dir, &normalized)?;
        let host_link_path = parent_host_path.join(&name);

        let link_virtual_path = normalize_path(link_path);
        let target_virtual_path = if target.starts_with('/') {
            normalize_path(target)
        } else {
            normalize_path(&format!(
                "{}/{}",
                virtual_dirname(&link_virtual_path),
                target
            ))
        };
        let host_target_path = self.lexical_host_path(&target_virtual_path)?;
        let relative_target = relative_path(
            host_link_path.parent().unwrap_or(self.host_root.as_path()),
            &host_target_path,
        );
        symlinkat(
            &relative_target,
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
        )
        .map_err(|error| io_error_to_vfs("symlink", link_path, nix_to_io(error)))
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        let parent_host_path = self.host_path_for_fd(&parent_dir, &normalized)?;
        let host_link_path = parent_host_path.join(&name);
        let link_target = readlinkat(Some(parent_dir.as_raw_fd()), name.as_os_str())
            .map_err(|error| io_error_to_vfs("readlink", path, nix_to_io(error)))?;
        let link_target_path = PathBuf::from(&link_target);
        let resolved_target = if link_target_path.is_absolute() {
            lexical_normalize_path(&link_target_path)
        } else {
            lexical_normalize_path(
                &host_link_path
                    .parent()
                    .unwrap_or(self.host_root.as_path())
                    .join(link_target_path),
            )
        };
        self.host_to_virtual_path(&resolved_target, path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        if normalize_path(path) == "/" {
            return self
                .host_root_dir
                .metadata()
                .map(Self::stat_from_metadata)
                .map_err(|error| io_error_to_vfs("lstat", path, error));
        }

        let (parent_dir, _, name, normalized) = self.split_parent(path, false)?;
        fstatat(
            Some(parent_dir.as_raw_fd()),
            name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(Self::stat_from_file_stat)
        .map_err(|error| io_error_to_vfs("lstat", &normalized, nix_to_io(error)))
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (old_parent_dir, _, old_name, _) = self.split_parent(old_path, false)?;
        let (new_parent_dir, _, new_name, new_normalized) = self.split_parent(new_path, true)?;
        linkat(
            Some(old_parent_dir.as_raw_fd()),
            old_name.as_os_str(),
            Some(new_parent_dir.as_raw_fd()),
            new_name.as_os_str(),
            AtFlags::empty(),
        )
        .map_err(|error| io_error_to_vfs("link", &new_normalized, nix_to_io(error)))
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let handle = self.open_metadata_beneath(path, "chmod")?;
        fs::set_permissions(handle.proc_path(), fs::Permissions::from_mode(mode))
            .map_err(|error| io_error_to_vfs("chmod", path, error))
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        let handle = self.open_metadata_beneath(path, "chown")?;
        chown(
            handle.proc_path().as_path(),
            Some(Uid::from_raw(uid)),
            Some(Gid::from_raw(gid)),
        )
        .map_err(|error| VfsError::new(error_code(&error), error.to_string()))
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        self.apply_utimens(
            path,
            VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(atime_ms)),
            VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(mtime_ms)),
            true,
        )
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        self.apply_utimens(path, atime, mtime, follow_symlinks)
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        let (_, relative) = self.relative_virtual_path(path);
        let handle = self.open_beneath(&relative, OFlag::O_WRONLY, Mode::empty())?;
        let file = File::options()
            .write(true)
            .open(handle.proc_path())
            .map_err(|error| io_error_to_vfs("truncate", path, error))?;
        file.set_len(length)
            .map_err(|error| io_error_to_vfs("truncate", path, error))
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        self.check_read_length(path, length)?;
        let (_, relative) = self.relative_virtual_path(path);
        let handle = self.open_beneath(&relative, OFlag::O_RDONLY, Mode::empty())?;
        let file =
            File::open(handle.proc_path()).map_err(|error| io_error_to_vfs("open", path, error))?;
        let mut buffer = vec![0; length];
        let bytes_read = file
            .read_at(&mut buffer, offset)
            .map_err(|error| io_error_to_vfs("open", path, error))?;
        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    fn pwrite(&mut self, path: &str, content: impl Into<Vec<u8>>, offset: u64) -> VfsResult<()> {
        let (_, relative) = self.relative_virtual_path(path);
        let handle = self.open_beneath(&relative, OFlag::O_WRONLY, Mode::empty())?;
        let file = File::options()
            .write(true)
            .open(handle.proc_path())
            .map_err(|error| io_error_to_vfs("open", path, error))?;
        let content = content.into();
        self.write_all_at(&file, &content, offset, path)
    }
}

/// One read-only `host_dir`/`module_access` mount, keyed by its guest mount
/// point. The filesystem reads mount-relative virtual paths (e.g. `/foo/index.js`
/// for a mount at `/root/node_modules`).
// `dead_code` is allowed because `host_dir.rs` is also `#[path]`-included by
// `tests/host_dir.rs`, whose test compilation exercises only the filesystem
// plugin and not the module-reader path (which the real lib build does use).
#[allow(dead_code)]
#[derive(Clone)]
struct HostDirModuleMount {
    /// Normalized guest mount point, e.g. `/root/node_modules`.
    guest_prefix: String,
    filesystem: ModuleMountBackend,
}

/// Backing filesystem for one module-reader mount: an anchored host dir
/// (`host_dir`/`module_access` mounts) or a packed `.aospkg` tar
/// (`agentos_packages` package-version leaves). Both are read-only and safe to
/// use off the service loop — the tar reader serves mmap-backed byte ranges
/// from the shared identity-keyed archive cache and never touches the kernel.
#[allow(dead_code)]
#[derive(Clone)]
enum ModuleMountBackend {
    Host(HostDirFilesystem),
    Tar(TarFileSystem),
}

#[allow(dead_code)]
impl ModuleMountBackend {
    fn realpath(&self, path: &str) -> VfsResult<String> {
        match self {
            Self::Host(fs) => fs.realpath(path),
            Self::Tar(fs) => VirtualFileSystem::realpath(fs, path),
        }
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        match self {
            Self::Host(fs) => fs.read_file(path),
            Self::Tar(fs) => VirtualFileSystem::read_file(fs, path),
        }
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        match self {
            Self::Host(fs) => fs.stat(path),
            Self::Tar(fs) => VirtualFileSystem::stat(fs, path),
        }
    }

    fn exists(&self, path: &str) -> bool {
        match self {
            Self::Host(fs) => fs.exists(path),
            Self::Tar(fs) => VirtualFileSystem::exists(fs, path),
        }
    }
}

#[allow(dead_code)]
impl HostDirModuleMount {
    /// If `guest_path` falls under this mount, return the mount-relative virtual
    /// path (always absolute, e.g. `/foo/index.js`).
    fn relative_virtual_path(&self, guest_path: &str) -> Option<String> {
        if guest_path == self.guest_prefix {
            return Some(String::from("/"));
        }
        let prefix_with_sep = if self.guest_prefix == "/" {
            String::from("/")
        } else {
            format!("{}/", self.guest_prefix)
        };
        let rest = guest_path.strip_prefix(&prefix_with_sep)?;
        Some(format!("/{rest}"))
    }

    /// Re-express a mount-relative virtual path (e.g. `/foo/index.js`) as a guest
    /// path under this mount (e.g. `/root/node_modules/foo/index.js`).
    fn guest_path_for_relative(&self, relative: &str) -> String {
        let trimmed = relative.trim_start_matches('/');
        if self.guest_prefix == "/" {
            if trimmed.is_empty() {
                String::from("/")
            } else {
                format!("/{trimmed}")
            }
        } else if trimmed.is_empty() {
            self.guest_prefix.clone()
        } else {
            format!("{}/{trimmed}", self.guest_prefix)
        }
    }
}

/// A `Send`-able, clonable, read-only [`ModuleFsReader`] over one or more mounted
/// `host_dir`/`module_access` filesystems. It lets module resolution run on the
/// V8 bridge thread — concurrently with the sidecar service loop — while still
/// reading exactly the mounted `node_modules` tree the guest sees (anchored
/// `openat2(RESOLVE_BENEATH)` with escaping-symlink refusal), instead of the
/// host-direct path translator.
///
/// It never touches the `&mut` kernel, so a large cold-start module graph cannot
/// serialize behind / starve work on the service-loop thread (e.g. an ACP
/// `session/new` bootstrap awaiting the adapter's response on that same loop).
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct HostDirModuleReader {
    /// Mounts sorted longest-`guest_prefix`-first so the most specific mount
    /// wins (mirrors the kernel mount table's longest-prefix dispatch).
    mounts: Vec<HostDirModuleMount>,
}

#[allow(dead_code)]
impl HostDirModuleReader {
    /// Build a reader from `(guest_path, host_path)` pairs for the VM's read-only
    /// `host_dir`/`module_access` mounts. Mounts whose host root cannot be opened
    /// are skipped. Returns `None` if no usable mount remains, so callers fall
    /// back to the service-loop kernel reader.
    pub(crate) fn from_mounts<I, G, H>(mounts: I) -> Option<Self>
    where
        I: IntoIterator<Item = (G, H)>,
        G: AsRef<str>,
        H: AsRef<Path>,
    {
        let mut entries = mounts
            .into_iter()
            .filter_map(|(guest_path, host_path)| {
                let filesystem = HostDirFilesystem::new_with_read_limit(
                    host_path.as_ref(),
                    Some(MAX_HOST_DIR_READ_BYTES),
                )
                .ok()?;
                Some(HostDirModuleMount {
                    guest_prefix: normalize_path(guest_path.as_ref()),
                    filesystem: ModuleMountBackend::Host(filesystem),
                })
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return None;
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.guest_prefix.len()));
        entries.dedup_by(|left, right| left.guest_prefix == right.guest_prefix);
        Some(Self { mounts: entries })
    }

    /// Build a reader from host-dir pairs plus packed package-version tar
    /// mounts (`(guest_path, aospkg_path, tar_root)`), so module resolution
    /// reads packed `node_modules` content directly from the `.aospkg` mount
    /// index off the service loop. Unopenable mounts are skipped.
    pub(crate) fn from_mounts_and_package_tars<I, G, H>(
        mounts: I,
        package_tars: Vec<(String, String, String)>,
    ) -> Option<Self>
    where
        I: IntoIterator<Item = (G, H)>,
        G: AsRef<str>,
        H: AsRef<Path>,
    {
        let mut entries = mounts
            .into_iter()
            .filter_map(|(guest_path, host_path)| {
                let filesystem = HostDirFilesystem::new_with_read_limit(
                    host_path.as_ref(),
                    Some(MAX_HOST_DIR_READ_BYTES),
                )
                .ok()?;
                Some(HostDirModuleMount {
                    guest_prefix: normalize_path(guest_path.as_ref()),
                    filesystem: ModuleMountBackend::Host(filesystem),
                })
            })
            .collect::<Vec<_>>();
        entries.extend(
            package_tars
                .into_iter()
                .filter_map(|(guest_path, tar_path, root)| {
                    let filesystem = TarFileSystem::open_at(&tar_path, &root).ok()?;
                    Some(HostDirModuleMount {
                        guest_prefix: normalize_path(&guest_path),
                        filesystem: ModuleMountBackend::Tar(filesystem),
                    })
                }),
        );
        if entries.is_empty() {
            return None;
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.guest_prefix.len()));
        entries.dedup_by(|left, right| left.guest_prefix == right.guest_prefix);
        Some(Self { mounts: entries })
    }

    /// Find the index of the most-specific mount containing `guest_path` and the
    /// corresponding mount-relative virtual path.
    fn mount_index_for(&self, guest_path: &str) -> Option<(usize, String)> {
        let normalized = normalize_path(guest_path);
        self.mounts.iter().enumerate().find_map(|(index, mount)| {
            mount
                .relative_virtual_path(&normalized)
                .map(|rel| (index, rel))
        })
    }
}

impl ModuleFsReader for HostDirModuleReader {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        let mount = &self.mounts[index];
        // `realpath` returns a mount-relative virtual path; re-express it as a
        // guest path so the resolver keeps operating in the guest namespace.
        let resolved = mount.filesystem.realpath(&relative).ok()?;
        Some(mount.guest_path_for_relative(&resolved))
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        let bytes = self.mounts[index].filesystem.read_file(&relative).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        // `stat` follows symlinks (O_PATH, no O_NOFOLLOW), so a symlinked package
        // directory reports as a directory just like `fs.statSync` would.
        self.mounts[index]
            .filesystem
            .stat(&relative)
            .ok()
            .map(|stat| stat.is_directory)
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        match self.mount_index_for(guest_path) {
            Some((index, relative)) => self.mounts[index].filesystem.exists(&relative),
            None => false,
        }
    }
}

/// Session-thread module reader: the mounted `HostDirModuleReader` plus a
/// persistent resolution cache, so the V8 isolate thread can both resolve
/// specifiers and read source DIRECTLY (same mount + `openat2(RESOLVE_BENEATH)`
/// confinement, same `ModuleResolver` semantics as the bridge), skipping the
/// per-module `_resolveModule`/`_loadFile` bridge round-trips.
pub(crate) struct SessionModuleReader {
    reader: HostDirModuleReader,
    cache: LocalModuleResolutionCache,
}

impl SessionModuleReader {
    pub(crate) fn new(reader: HostDirModuleReader) -> Self {
        Self {
            reader,
            cache: LocalModuleResolutionCache::default(),
        }
    }
}

impl GuestModuleReader for SessionModuleReader {
    fn read_module_source(&mut self, resolved_guest_path: &str) -> Option<String> {
        self.reader.read_to_string(resolved_guest_path)
    }

    fn resolve_module(&mut self, specifier: &str, referrer: &str) -> Option<String> {
        // Mirror the bridge's `_resolveModule` exactly: import mode, same reader,
        // same persisted cache.
        let reader: &mut dyn ModuleFsReader = &mut self.reader;
        let mut resolver = ModuleResolver::new(reader, &mut self.cache);
        resolver.resolve_module(specifier, referrer, ModuleResolveMode::Import)
    }
}

fn nix_to_io(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

fn io_error_to_vfs(op: &'static str, path: &str, error: io::Error) -> VfsError {
    let code = match error.raw_os_error() {
        Some(1) => "EPERM",
        Some(2) => "ENOENT",
        Some(13) => "EACCES",
        Some(17) => "EEXIST",
        Some(18) => "EXDEV",
        Some(20) => "ENOTDIR",
        Some(21) => "EISDIR",
        Some(22) => "EINVAL",
        Some(30) => "EROFS",
        Some(39) => "ENOTEMPTY",
        Some(40) => "ELOOP",
        _ => match error.kind() {
            io::ErrorKind::NotFound => "ENOENT",
            io::ErrorKind::PermissionDenied => "EACCES",
            io::ErrorKind::AlreadyExists => "EEXIST",
            io::ErrorKind::InvalidInput => "EINVAL",
            _ => "EIO",
        },
    };
    VfsError::new(code, format!("{op} '{path}': {error}"))
}

fn error_code(error: &nix::Error) -> &'static str {
    match error {
        nix::Error::EACCES => "EACCES",
        nix::Error::EEXIST => "EEXIST",
        nix::Error::EINVAL => "EINVAL",
        nix::Error::EISDIR => "EISDIR",
        nix::Error::ELOOP => "ELOOP",
        nix::Error::ENOENT => "ENOENT",
        nix::Error::ENOTDIR => "ENOTDIR",
        nix::Error::ENOTEMPTY => "ENOTEMPTY",
        nix::Error::EPERM => "EPERM",
        nix::Error::EROFS => "EROFS",
        _ => "EIO",
    }
}

fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        }
    }

    if normalized.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        normalized
    }
}

fn relative_path(from_dir: &Path, to: &Path) -> PathBuf {
    let from_components = from_dir.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let shared = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative = PathBuf::new();
    for _ in shared..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[shared..] {
        if let Component::Normal(segment) = component {
            relative.push(segment);
        }
    }

    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn virtual_dirname(path: &str) -> String {
    let normalized = normalize_path(path);
    match normalized.rsplit_once('/') {
        Some((head, _)) if !head.is_empty() => head.to_owned(),
        _ => String::from("/"),
    }
}

#[cfg(test)]
mod tar_module_reader_tests {
    use super::*;
    use agentos_execution::{ModuleResolveMode, ModuleResolver};

    #[test]
    fn tar_reader_resolves_packed_node_modules() {
        let aospkg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../software/pi/dist/package.aospkg");
        if !aospkg.is_file() {
            eprintln!("skip: pi aospkg not built");
            return;
        }
        let mut reader = HostDirModuleReader::from_mounts_and_package_tars(
            Vec::<(String, std::path::PathBuf)>::new(),
            vec![(
                String::from("/opt/agentos/pkgs/pi/0.2.1"),
                aospkg.to_string_lossy().into_owned(),
                String::from("/"),
            )],
        )
        .expect("reader");
        let probe = "/opt/agentos/pkgs/pi/0.2.1/node_modules/@anthropic-ai/sdk/package.json";
        assert!(reader.path_exists(probe), "packed package.json must exist");
        assert!(
            reader.read_to_string(probe).is_some(),
            "packed package.json must read"
        );
        let mut cache = Default::default();
        let dyn_reader: &mut dyn ModuleFsReader = &mut reader;
        let mut resolver = ModuleResolver::new(dyn_reader, &mut cache);
        let resolved = resolver.resolve_module(
            "@anthropic-ai/sdk",
            "/opt/agentos/pkgs/pi/0.2.1/node_modules/@agentos-software/pi/dist/adapter.js",
            ModuleResolveMode::Require,
        );
        assert!(
            resolved.is_some(),
            "require-mode resolution from the packed tar"
        );
        let resolved_import = resolver.resolve_module(
            "@anthropic-ai/sdk",
            "/opt/agentos/pkgs/pi/0.2.1/node_modules/@agentos-software/pi/dist/adapter.js",
            ModuleResolveMode::Import,
        );
        assert!(
            resolved_import.is_some(),
            "import-mode resolution from the packed tar"
        );
        assert!(
            resolved.is_some(),
            "must resolve @anthropic-ai/sdk from the packed tar"
        );
    }
}
