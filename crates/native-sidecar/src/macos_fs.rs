//! macOS host-mount confinement shims.
//!
//! The Linux host-mount filesystem confinement relies on three primitives that
//! do not exist on macOS:
//!   * `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)` — atomic resolve-beneath
//!     path resolution (the escape boundary for host-backed mounts),
//!   * `O_PATH` — a metadata-only anchor fd,
//!   * `/proc/self/fd/N` — re-deriving a path/handle from an fd.
//!
//! This module provides the macOS equivalents:
//!   * [`resolve_beneath`] resolves a guest-supplied relative path strictly
//!     beneath the mount root using `cap-std`, whose audited userspace walk
//!     (fd-relative, per-hop, symlink- and `..`-refusing) reproduces the
//!     escape guarantee `openat2(RESOLVE_BENEATH)` gives atomically on Linux.
//!   * [`fd_real_path`] uses `fcntl(F_GETPATH)` in place of
//!     `readlink("/proc/self/fd/N")` to recover an fd's real host path.
//!
//! `O_PATH` is mapped to a read-only anchor (`O_RDONLY`) at the call sites, and
//! `/proc/self/fd/N` to `/dev/fd/N` in `AnchoredFd::proc_path`.

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::sys::stat::Mode;
use std::io;
use std::os::fd::{IntoRawFd, RawFd};
use std::path::{Path, PathBuf};

/// Resolve `relative` strictly beneath `root` and open it, returning an owned
/// raw fd. macOS counterpart to
/// `openat2(root, relative, RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)`.
///
/// `cap-std` guarantees the resolution never escapes `root` (via `..`, an
/// absolute symlink, or a symlink whose target leaves the tree), refusing such
/// attempts with an errno-less `PermissionDenied` that [`io_to_errno`] maps to
/// `EXDEV` — matching how callers already treat `openat2`'s escape error.
///
/// `O_PATH` anchors arrive here as `O_RDONLY` (macOS has no metadata-only open);
/// the resulting fd is still only used as an anchor / re-opened via `/dev/fd/N`.
pub(crate) fn resolve_beneath(
    root: &Path,
    relative: &Path,
    flags: OFlag,
    mode: Mode,
) -> Result<RawFd, Errno> {
    let dir = Dir::open_ambient_dir(root, ambient_authority()).map_err(io_to_errno)?;

    // Directory handles: `open_dir` is cap-std's resolve-beneath `O_DIRECTORY`
    // open and returns a `Dir` we can hand back as a raw fd.
    if flags.contains(OFlag::O_DIRECTORY) {
        let sub = dir.open_dir(relative).map_err(io_to_errno)?;
        return Ok(sub.into_raw_fd());
    }

    let acc = flags & OFlag::O_ACCMODE;
    let write = acc == OFlag::O_WRONLY || acc == OFlag::O_RDWR;

    let mut opts = OpenOptions::new();
    // A pure anchor (O_PATH mapped to O_RDONLY) and any read/RDWR open needs
    // read; ensure we never request neither read nor write (cap-std rejects it).
    opts.read(!write)
        .write(write)
        .create(flags.contains(OFlag::O_CREAT))
        .create_new(flags.contains(OFlag::O_EXCL))
        .truncate(flags.contains(OFlag::O_TRUNC));
    if acc == OFlag::O_RDWR {
        opts.read(true);
    }
    if flags.contains(OFlag::O_APPEND) {
        opts.append(true);
    }
    opts.mode(u32::from(mode.bits()));
    // Preserve a caller's request not to follow the final component. cap-std
    // refuses *escaping* symlinks regardless; this additionally refuses a
    // non-escaping final symlink, matching O_NOFOLLOW semantics.
    if flags.contains(OFlag::O_NOFOLLOW) {
        opts.custom_flags(OFlag::O_NOFOLLOW.bits());
    }

    let file = dir.open_with(relative, &opts).map_err(io_to_errno)?;
    Ok(file.into_raw_fd())
}

/// Real filesystem path of an open fd via `fcntl(F_GETPATH)` — the macOS
/// counterpart to `readlink("/proc/self/fd/N")`. Uses nix's safe wrapper so the
/// sidecar crate's `#![forbid(unsafe_code)]` holds.
pub(crate) fn fd_real_path(fd: RawFd) -> io::Result<PathBuf> {
    let mut path = PathBuf::new();
    fcntl(fd, FcntlArg::F_GETPATH(&mut path))
        .map_err(|errno| io::Error::from_raw_os_error(errno as i32))?;
    Ok(path)
}

/// Map a `cap-std` filesystem error to an `Errno`. A resolve-beneath escape is
/// reported by cap-std as an errno-less `PermissionDenied`; translate that to
/// `EXDEV` so callers reuse their existing "path escapes mount" handling.
fn io_to_errno(error: io::Error) -> Errno {
    if let Some(raw) = error.raw_os_error() {
        Errno::from_raw(raw)
    } else if error.kind() == io::ErrorKind::PermissionDenied {
        Errno::EXDEV
    } else {
        Errno::EIO
    }
}
