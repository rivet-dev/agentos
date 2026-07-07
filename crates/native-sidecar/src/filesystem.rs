//! Guest filesystem and VFS dispatch extracted from service.rs.

use crate::execution::{
    host_path_from_runtime_guest_mappings, is_protected_agentos_shadow_sync_path,
    sync_active_process_host_writes_to_kernel,
};
use crate::protocol::{
    GuestFilesystemCallRequest, GuestFilesystemOperation, GuestRuntimeKind, RequestFrame,
    ResponsePayload,
};
use crate::service::{
    javascript_sync_rpc_arg_str, javascript_sync_rpc_arg_u32, javascript_sync_rpc_arg_u32_optional,
    javascript_sync_rpc_arg_u64, javascript_sync_rpc_arg_u64_optional,
    javascript_sync_rpc_bytes_arg, javascript_sync_rpc_bytes_value, javascript_sync_rpc_encoding,
    javascript_sync_rpc_option_bool, javascript_sync_rpc_option_u32, kernel_error,
    log_stale_process_event, normalize_host_path, normalize_path, path_is_within_root,
};
use crate::state::{
    ActiveExecutionEvent, ActiveProcess, BridgeError, SidecarKernel, VmState,
    EXECUTION_DRIVER_NAME, EXECUTION_SANDBOX_ROOT_ENV, PYTHON_VFS_RPC_GUEST_ROOT,
};
use crate::{DispatchResult, NativeSidecar, NativeSidecarBridge, SidecarError};

use base64::Engine;
use nix::errno::Errno;
use nix::fcntl::{open, OFlag};
#[cfg(not(target_os = "macos"))]
use nix::fcntl::{openat2, OpenHow, ResolveFlag};
use nix::libc;

// macOS has neither `O_PATH` (metadata-only anchor) nor `O_TMPFILE`. O_PATH
// anchors are re-opened via `/dev/fd/N`, so a read-only open stands in; no
// caller actually passes O_TMPFILE (it appears only in a defensive `intersects`
// check), so an empty flag is an exact behavioural match there.
#[cfg(not(target_os = "macos"))]
const O_PATH_ANCHOR: OFlag = OFlag::O_PATH;
#[cfg(target_os = "macos")]
const O_PATH_ANCHOR: OFlag = OFlag::O_RDONLY;
#[cfg(not(target_os = "macos"))]
const O_TMPFILE_FLAG: OFlag = OFlag::O_TMPFILE;
#[cfg(target_os = "macos")]
const O_TMPFILE_FLAG: OFlag = OFlag::empty();
use agentos_execution::{
    JavascriptSyncRpcRequest, LocalResolvedModuleFormat, ModuleFsReader, ModuleResolveMode,
    ModuleResolver, PythonVfsRpcMethod, PythonVfsRpcRequest, PythonVfsRpcResponsePayload,
    PythonVfsRpcStat,
};
use agentos_kernel::vfs::{VirtualStat, VirtualTimeSpec, VirtualUtimeSpec};
use agentos_native_sidecar_core::{
    decode_guest_filesystem_content, handle_guest_filesystem_call as core_guest_filesystem_call,
};
use nix::sys::stat::{utimensat, Mode, UtimensatFlags};
use nix::sys::time::TimeSpec;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{symlink, FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const PYTHON_PYODIDE_GUEST_ROOT: &str = "/__agentos_pyodide";

fn kernel_path_error(
    operation: &str,
    path: &str,
    error: impl Into<agentos_kernel::kernel::KernelError>,
) -> SidecarError {
    let error = error.into();
    let base = kernel_error(error);
    match base {
        SidecarError::Kernel(message) => {
            SidecarError::Kernel(format!("{operation} {path}: {message}"))
        }
        other => other,
    }
}

fn filesystem_access_denied(path: &str, mode: u32) -> SidecarError {
    SidecarError::Execution(format!(
        "EACCES: filesystem access denied for {path} with mode {mode:o}"
    ))
}

fn filesystem_access_mode_is_allowed(file_mode: u32, requested: u32) -> bool {
    const ACCESS_MASK: u32 = libc::R_OK as u32 | libc::W_OK as u32 | libc::X_OK as u32;
    if requested & ACCESS_MASK == 0 {
        return true;
    }
    if requested & libc::R_OK as u32 != 0 && file_mode & 0o444 == 0 {
        return false;
    }
    if requested & libc::W_OK as u32 != 0 && file_mode & 0o222 == 0 {
        return false;
    }
    if requested & libc::X_OK as u32 != 0 && file_mode & 0o111 == 0 {
        return false;
    }
    true
}
const PYTHON_PYODIDE_CACHE_GUEST_ROOT: &str = "/__agentos_pyodide_cache";
const UTIME_NOW_NSEC: i64 = libc::UTIME_NOW;
const UTIME_OMIT_NSEC: i64 = libc::UTIME_OMIT;

/// Backstop bound on a guest-controlled `ftruncate` length for a mapped host fd.
/// The kernel's configured truncate-size limit is the primary enforcement for
/// paths visible in the VFS; this caps the raw host `set_len` (and covers fds
/// with no kernel-visible guest path) so a hostile length cannot create an
/// enormous sparse host file or drive an unbounded sidecar-side mirror read.
const MAX_MAPPED_TRUNCATE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
struct MappedRuntimeHostPath {
    guest_path: String,
    host_root: PathBuf,
    host_path: PathBuf,
}

#[derive(Debug, Clone)]
enum MappedRuntimeHostAccess {
    Writable(MappedRuntimeHostPath),
    ReadOnly(MappedRuntimeHostPath),
}

#[derive(Debug)]
struct AnchoredFd {
    fd: RawFd,
}

impl AnchoredFd {
    #[cfg(not(target_os = "macos"))]
    fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.fd))
    }

    // macOS `/dev/fd/N` re-opens a *file* fd (the kernel dups it), standing in
    // for Linux's `/proc/self/fd/N` at the file read/write/metadata call sites.
    // It is NOT, however, a drop-in for directory work: `/dev/fd/N` is not a
    // `readdir`-able directory and child components cannot be appended
    // (`/dev/fd/N/child` fails). Directory enumeration uses [`readdir_path`];
    // child mutations use fd-relative `*at` calls.
    #[cfg(target_os = "macos")]
    fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/dev/fd/{}", self.fd))
    }

    // Path to enumerate this fd's directory entries. Linux can `readdir`
    // `/proc/self/fd/N` directly; macOS `/dev/fd/N` is not a readdir-able
    // directory (it yields `ENOTDIR`), so recover the fd's real host path via
    // `fcntl(F_GETPATH)` — the same fd→path recovery used elsewhere on macOS.
    #[cfg(not(target_os = "macos"))]
    fn readdir_path(&self) -> std::io::Result<PathBuf> {
        Ok(self.proc_path())
    }
    #[cfg(target_os = "macos")]
    fn readdir_path(&self) -> std::io::Result<PathBuf> {
        crate::macos_fs::fd_real_path(self.fd)
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

#[derive(Debug)]
struct MappedRuntimeOpenedPath {
    handle: AnchoredFd,
    host_path: PathBuf,
}

#[derive(Debug)]
struct MappedRuntimeParentPath {
    directory: AnchoredFd,
    host_path: PathBuf,
    child_name: OsString,
}

#[derive(Debug, Deserialize)]
struct RuntimeGuestPathMappingWire {
    #[serde(rename = "guestPath")]
    guest_path: String,
    #[serde(rename = "hostPath")]
    host_path: String,
}

fn parse_timespec_seconds(value: f64, label: &str) -> Result<VirtualTimeSpec, SidecarError> {
    if !value.is_finite() {
        return Err(SidecarError::InvalidState(format!(
            "{label} must be a finite numeric value"
        )));
    }
    let seconds = value.floor();
    let mut sec = seconds as i64;
    let mut nanos = ((value - seconds) * 1_000_000_000.0).round() as i64;
    if nanos >= 1_000_000_000 {
        sec = sec.saturating_add(1);
        nanos -= 1_000_000_000;
    }
    VirtualTimeSpec::new(sec, nanos as u32)
        .map_err(|error| SidecarError::InvalidState(format!("{label}: {error}")))
}

fn parse_timespec_integer(value: &Value, label: &str) -> Result<i64, SidecarError> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .ok_or_else(|| SidecarError::InvalidState(format!("{label} must be an integer")))
}

fn parse_utime_spec_value(value: &Value, label: &str) -> Result<VirtualUtimeSpec, SidecarError> {
    if let Some(number) = value.as_f64() {
        return parse_timespec_seconds(number, label).map(VirtualUtimeSpec::Set);
    }

    let Some(object) = value.as_object() else {
        return Err(SidecarError::InvalidState(format!(
            "{label} must be a numeric seconds value or {{ sec, nsec }}"
        )));
    };

    if let Some(kind) = object.get("kind").and_then(Value::as_str) {
        return match kind {
            "now" | "UTIME_NOW" => Ok(VirtualUtimeSpec::Now),
            "omit" | "UTIME_OMIT" => Ok(VirtualUtimeSpec::Omit),
            other => Err(SidecarError::InvalidState(format!(
                "{label} kind must be 'now' or 'omit', got {other}"
            ))),
        };
    }

    let Some(nsec_value) = object.get("nsec") else {
        return Err(SidecarError::InvalidState(format!(
            "{label} timespec requires nsec"
        )));
    };
    if let Some(text) = nsec_value.as_str() {
        return match text {
            "UTIME_NOW" => Ok(VirtualUtimeSpec::Now),
            "UTIME_OMIT" => Ok(VirtualUtimeSpec::Omit),
            _ => Err(SidecarError::InvalidState(format!(
                "{label} nsec must be numeric, UTIME_NOW, or UTIME_OMIT"
            ))),
        };
    }
    if let Some(integer) = nsec_value.as_i64().or_else(|| {
        nsec_value
            .as_u64()
            .and_then(|value| i64::try_from(value).ok())
    }) {
        if integer == UTIME_NOW_NSEC {
            return Ok(VirtualUtimeSpec::Now);
        }
        if integer == UTIME_OMIT_NSEC {
            return Ok(VirtualUtimeSpec::Omit);
        }
    }

    let sec_value = object
        .get("sec")
        .ok_or_else(|| SidecarError::InvalidState(format!("{label} timespec requires sec")))?;
    let sec = parse_timespec_integer(sec_value, &format!("{label}.sec"))?;
    let nsec = u32::try_from(parse_timespec_integer(
        nsec_value,
        &format!("{label}.nsec"),
    )?)
    .map_err(|_| SidecarError::InvalidState(format!("{label}.nsec must fit within u32")))?;
    VirtualTimeSpec::new(sec, nsec)
        .map(VirtualUtimeSpec::Set)
        .map_err(|error| SidecarError::InvalidState(format!("{label}: {error}")))
}

fn parse_utime_arg(
    args: &[Value],
    index: usize,
    label: &str,
) -> Result<VirtualUtimeSpec, SidecarError> {
    let value = args
        .get(index)
        .ok_or_else(|| SidecarError::InvalidState(format!("{label} is required")))?;
    parse_utime_spec_value(value, label)
}

fn metadata_timespec(
    metadata: &fs::Metadata,
    access_time: bool,
) -> Result<VirtualTimeSpec, SidecarError> {
    let (sec, nsec) = if access_time {
        (metadata.atime(), metadata.atime_nsec())
    } else {
        (metadata.mtime(), metadata.mtime_nsec())
    };
    VirtualTimeSpec::new(sec, nsec.clamp(0, 999_999_999) as u32)
        .map_err(|error| SidecarError::InvalidState(format!("invalid host metadata time: {error}")))
}

fn resolve_host_utime(spec: VirtualUtimeSpec, existing: VirtualTimeSpec) -> TimeSpec {
    match spec {
        VirtualUtimeSpec::Set(spec) => TimeSpec::new(spec.sec, spec.nsec as libc::c_long),
        VirtualUtimeSpec::Now => TimeSpec::new(0, libc::UTIME_NOW),
        VirtualUtimeSpec::Omit => TimeSpec::new(existing.sec, libc::UTIME_OMIT),
    }
}

fn apply_host_path_utimens(
    host_path: &Path,
    atime: VirtualUtimeSpec,
    mtime: VirtualUtimeSpec,
    follow_symlinks: bool,
    context: &str,
) -> Result<(), SidecarError> {
    let existing = match (atime, mtime) {
        (VirtualUtimeSpec::Omit, _) | (_, VirtualUtimeSpec::Omit) => {
            let metadata = if follow_symlinks {
                fs::metadata(host_path)
            } else {
                fs::symlink_metadata(host_path)
            }
            .map_err(|error| {
                SidecarError::Io(format!(
                    "{context}: failed to stat {}: {error}",
                    host_path.display()
                ))
            })?;
            Some((
                metadata_timespec(&metadata, true)?,
                metadata_timespec(&metadata, false)?,
            ))
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
        resolve_host_utime(atime, existing_atime),
        resolve_host_utime(mtime, existing_mtime),
    ];
    let flags = if follow_symlinks {
        UtimensatFlags::FollowSymlink
    } else {
        UtimensatFlags::NoFollowSymlink
    };
    utimensat(None, host_path, &times[0], &times[1], flags).map_err(|error| {
        SidecarError::Io(format!(
            "{context}: failed to update {}: {error}",
            host_path.display()
        ))
    })
}

pub(crate) async fn guest_filesystem_call<B>(
    sidecar: &mut NativeSidecar<B>,
    request: &RequestFrame,
    payload: GuestFilesystemCallRequest,
) -> Result<DispatchResult, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let (connection_id, session_id, vm_id) = sidecar.vm_scope_for(&request.ownership)?;
    sidecar.require_owned_vm(&connection_id, &session_id, &vm_id)?;

    let response = {
        let vm = match sidecar.vms.get_mut(&vm_id) {
            Some(vm) => vm,
            None => {
                return Err(stale_filesystem_request_error(
                    sidecar,
                    &vm_id,
                    None,
                    "guest filesystem dispatch",
                ));
            }
        };
        sync_guest_filesystem_shadow_before_call(vm, &payload)?;
        let response = core_guest_filesystem_call(&mut vm.kernel, payload.clone())
            .map_err(native_guest_filesystem_core_error)?;
        mirror_guest_filesystem_shadow_after_call(vm, &payload)?;
        response
    };

    Ok(DispatchResult {
        response: sidecar.respond(request, ResponsePayload::GuestFilesystemResult(response)),
        events: Vec::new(),
    })
}

fn native_guest_filesystem_core_error(
    error: agentos_native_sidecar_core::SidecarCoreError,
) -> SidecarError {
    let message = error.to_string();
    if message
        .split_once(':')
        .is_some_and(|(code, _)| is_posix_errno_code(code))
    {
        SidecarError::Kernel(message)
    } else {
        SidecarError::InvalidState(message)
    }
}

fn is_posix_errno_code(code: &str) -> bool {
    code.len() >= 2
        && code.starts_with('E')
        && code[1..]
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn sync_guest_filesystem_shadow_before_call(
    vm: &mut VmState,
    payload: &GuestFilesystemCallRequest,
) -> Result<(), SidecarError> {
    match payload.operation {
        GuestFilesystemOperation::ReadFile
        | GuestFilesystemOperation::Pread
        | GuestFilesystemOperation::Pwrite
        | GuestFilesystemOperation::Exists
        | GuestFilesystemOperation::Stat
        | GuestFilesystemOperation::Lstat
        | GuestFilesystemOperation::ReadDirRecursive
        | GuestFilesystemOperation::Remove
        | GuestFilesystemOperation::Copy
        | GuestFilesystemOperation::Move => {
            // Pwrite is a partial write that preserves the unmodified bytes, so
            // the existing shadow content must be present in the kernel before
            // the call, exactly like a read.
            sync_active_shadow_path_to_kernel(vm, &payload.path)?;
        }
        GuestFilesystemOperation::WriteFile
        | GuestFilesystemOperation::CreateDir
        | GuestFilesystemOperation::Mkdir
        | GuestFilesystemOperation::ReadDir
        | GuestFilesystemOperation::RemoveFile
        | GuestFilesystemOperation::RemoveDir
        | GuestFilesystemOperation::Rename
        | GuestFilesystemOperation::Realpath
        | GuestFilesystemOperation::Symlink
        | GuestFilesystemOperation::ReadLink
        | GuestFilesystemOperation::Link
        | GuestFilesystemOperation::Chmod
        | GuestFilesystemOperation::Chown
        | GuestFilesystemOperation::Utimes
        | GuestFilesystemOperation::Truncate => {}
    }
    Ok(())
}

fn mirror_guest_filesystem_shadow_after_call(
    vm: &mut VmState,
    payload: &GuestFilesystemCallRequest,
) -> Result<(), SidecarError> {
    match payload.operation {
        GuestFilesystemOperation::WriteFile => {
            let bytes = decode_guest_filesystem_content(
                &payload.path,
                payload.content.as_deref(),
                payload.encoding.clone(),
            )
            .map_err(|error| SidecarError::InvalidState(error.to_string()))?;
            mirror_guest_file_write_to_shadow(vm, &payload.path, &bytes)?;
        }
        GuestFilesystemOperation::Pwrite => {
            // A positional write only carries the changed region; mirror the
            // full post-write file from the kernel so the shadow stays faithful.
            let bytes = vm.kernel.read_file(&payload.path).map_err(kernel_error)?;
            mirror_guest_file_write_to_shadow(vm, &payload.path, &bytes)?;
        }
        GuestFilesystemOperation::CreateDir | GuestFilesystemOperation::Mkdir => {
            mirror_guest_directory_write_to_shadow(vm, &payload.path)?;
        }
        GuestFilesystemOperation::RemoveFile | GuestFilesystemOperation::RemoveDir => {
            remove_guest_shadow_path(vm, &payload.path)?;
        }
        GuestFilesystemOperation::Remove => {
            remove_guest_shadow_path(vm, &payload.path)?;
        }
        GuestFilesystemOperation::Copy => {
            let destination = payload.destination_path.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem copy requires a destination_path",
                ))
            })?;
            remove_guest_shadow_path(vm, destination)?;
            mirror_guest_subtree_to_shadow(vm, destination)?;
        }
        GuestFilesystemOperation::Move => {
            let destination = payload.destination_path.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem move requires a destination_path",
                ))
            })?;
            remove_guest_shadow_path(vm, &payload.path)?;
            remove_guest_shadow_path(vm, destination)?;
            mirror_guest_subtree_to_shadow(vm, destination)?;
        }
        GuestFilesystemOperation::Rename => {
            let destination = payload.destination_path.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem rename requires a destination_path",
                ))
            })?;
            rename_guest_shadow_path(vm, &payload.path, destination)?;
        }
        GuestFilesystemOperation::Symlink => {
            let target = payload.target.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem symlink requires a target",
                ))
            })?;
            mirror_guest_symlink_to_shadow(vm, &payload.path, target)?;
        }
        GuestFilesystemOperation::Link => {
            let destination = payload.destination_path.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem link requires a destination_path",
                ))
            })?;
            mirror_guest_link_to_shadow(vm, &payload.path, destination)?;
        }
        GuestFilesystemOperation::Chmod => {
            let mode = payload.mode.ok_or_else(|| {
                SidecarError::InvalidState(String::from("guest filesystem chmod requires a mode"))
            })?;
            mirror_guest_chmod_to_shadow(vm, &payload.path, mode)?;
        }
        GuestFilesystemOperation::Utimes => {
            let atime_ms = payload.atime_ms.ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem utimes requires atime_ms",
                ))
            })?;
            let mtime_ms = payload.mtime_ms.ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "guest filesystem utimes requires mtime_ms",
                ))
            })?;
            mirror_guest_utimes_to_shadow(
                vm,
                &payload.path,
                VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(atime_ms)),
                VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(mtime_ms)),
                true,
            )?;
        }
        GuestFilesystemOperation::Truncate => {
            let len = payload.len.ok_or_else(|| {
                SidecarError::InvalidState(String::from("guest filesystem truncate requires len"))
            })?;
            mirror_guest_truncate_to_shadow(vm, &payload.path, len)?;
        }
        GuestFilesystemOperation::ReadFile
        | GuestFilesystemOperation::Pread
        | GuestFilesystemOperation::Exists
        | GuestFilesystemOperation::Stat
        | GuestFilesystemOperation::Lstat
        | GuestFilesystemOperation::ReadDir
        | GuestFilesystemOperation::ReadDirRecursive
        | GuestFilesystemOperation::Realpath
        | GuestFilesystemOperation::ReadLink
        | GuestFilesystemOperation::Chown => {}
    }
    Ok(())
}

pub(crate) fn handle_python_vfs_rpc_request<B>(
    sidecar: &mut NativeSidecar<B>,
    vm_id: &str,
    process_id: &str,
    request: PythonVfsRpcRequest,
) -> Result<(), SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let Some(vm) = sidecar.vms.get(vm_id) else {
        log_stale_process_event(&sidecar.bridge, vm_id, process_id, "python VFS RPC");
        return Ok(());
    };
    if !vm.active_processes.contains_key(process_id) {
        log_stale_process_event(&sidecar.bridge, vm_id, process_id, "python VFS RPC");
        return Ok(());
    }

    let response = match normalize_python_vfs_rpc_path(&request.path) {
        Ok(path) => {
            let Some(vm) = sidecar.vms.get_mut(vm_id) else {
                log_stale_process_event(&sidecar.bridge, vm_id, process_id, "python VFS RPC");
                return Ok(());
            };
            match request.method {
                PythonVfsRpcMethod::Read => vm
                    .kernel
                    .read_file(&path)
                    .map(|content| PythonVfsRpcResponsePayload::Read {
                        content_base64: base64::engine::general_purpose::STANDARD.encode(content),
                    })
                    .map_err(kernel_error),
                PythonVfsRpcMethod::Write => {
                    let content_base64 = request.content_base64.as_deref().ok_or_else(|| {
                        SidecarError::InvalidState(format!(
                            "python VFS fsWrite for {} requires contentBase64",
                            path
                        ))
                    })?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(content_base64)
                        .map_err(|error| {
                            SidecarError::InvalidState(format!(
                                "invalid base64 python VFS content for {}: {error}",
                                path
                            ))
                        })?;
                    vm.kernel
                        .write_file(&path, bytes)
                        .map(|()| PythonVfsRpcResponsePayload::Empty)
                        .map_err(kernel_error)
                }
                PythonVfsRpcMethod::Stat => vm
                    .kernel
                    .stat(&path)
                    .map(|stat| PythonVfsRpcResponsePayload::Stat {
                        stat: PythonVfsRpcStat {
                            mode: stat.mode,
                            size: stat.size,
                            is_directory: stat.is_directory,
                            is_symbolic_link: stat.is_symbolic_link,
                        },
                    })
                    .map_err(kernel_error),
                // Like Stat but does NOT follow symlinks, so the runner can
                // represent a host-preexisting symlink as a link node.
                PythonVfsRpcMethod::Lstat => vm
                    .kernel
                    .lstat(&path)
                    .map(|stat| PythonVfsRpcResponsePayload::Stat {
                        stat: PythonVfsRpcStat {
                            mode: stat.mode,
                            size: stat.size,
                            is_directory: stat.is_directory,
                            is_symbolic_link: stat.is_symbolic_link,
                        },
                    })
                    .map_err(kernel_error),
                PythonVfsRpcMethod::ReadDir => vm
                    .kernel
                    .read_dir(&path)
                    .map(|entries| PythonVfsRpcResponsePayload::ReadDir { entries })
                    .map_err(kernel_error),
                PythonVfsRpcMethod::Mkdir => vm
                    .kernel
                    .mkdir(&path, request.recursive)
                    .map(|()| PythonVfsRpcResponsePayload::Empty)
                    .map_err(kernel_error),
                // Mirror the delete/rename into the host-side shadow too, the
                // same way the wire `GuestFilesystemOperation` handlers do —
                // otherwise a later shadow→kernel sync would resurrect the
                // entry the guest just removed.
                PythonVfsRpcMethod::Unlink => {
                    match vm.kernel.remove_file(&path).map_err(kernel_error) {
                        Ok(()) => remove_guest_shadow_path(vm, &path)
                            .map(|()| PythonVfsRpcResponsePayload::Empty),
                        Err(error) => Err(error),
                    }
                }
                PythonVfsRpcMethod::Rmdir => {
                    match vm.kernel.remove_dir(&path).map_err(kernel_error) {
                        Ok(()) => remove_guest_shadow_path(vm, &path)
                            .map(|()| PythonVfsRpcResponsePayload::Empty),
                        Err(error) => Err(error),
                    }
                }
                PythonVfsRpcMethod::Rename => {
                    let destination = request.destination.as_deref().ok_or_else(|| {
                        SidecarError::InvalidState(format!(
                            "python VFS fsRename for {} requires destination",
                            path
                        ))
                    })?;
                    let destination = normalize_python_vfs_rpc_path(destination)?;
                    match vm.kernel.rename(&path, &destination).map_err(kernel_error) {
                        Ok(()) => rename_guest_shadow_path(vm, &path, &destination)
                            .map(|()| PythonVfsRpcResponsePayload::Empty),
                        Err(error) => Err(error),
                    }
                }
                // Kernel-direct (no shadow mirror): guest Python writes/creates
                // land only in the kernel VFS, so mirroring create/modify ops into
                // the host-side shadow would leave empty stubs that a later
                // shadow->kernel sync resurrects over real content. (Delete/rename
                // still mirror — to *remove* stale wire-written shadow entries.)
                PythonVfsRpcMethod::Symlink => {
                    let target = request.target.clone().ok_or_else(|| {
                        SidecarError::InvalidState(format!(
                            "python VFS fsSymlink for {} requires a target",
                            path
                        ))
                    })?;
                    vm.kernel
                        .symlink(&target, &path)
                        .map(|()| PythonVfsRpcResponsePayload::Empty)
                        .map_err(kernel_error)
                }
                PythonVfsRpcMethod::ReadLink => vm
                    .kernel
                    .read_link(&path)
                    .map(|target| PythonVfsRpcResponsePayload::SymlinkTarget { target })
                    .map_err(kernel_error),
                // `setattr` carries any of mode/uid/gid/atime+mtime; apply each
                // present field to the host VFS.
                PythonVfsRpcMethod::Setattr => {
                    (|| -> Result<PythonVfsRpcResponsePayload, SidecarError> {
                        // Mirror metadata into the host shadow only when the entry
                        // already exists there (a host-mounted / wire-written file),
                        // so the next shadow->kernel reconcile keeps the guest's
                        // change. Never *create* a shadow stub for a kernel-only
                        // guest file (that resurrected empty content).
                        let mirror = shadow_host_path_for_guest(&vm.cwd, &path).exists();
                        if let Some(mode) = request.mode {
                            vm.kernel.chmod(&path, mode).map_err(kernel_error)?;
                            if mirror {
                                mirror_guest_chmod_to_shadow(vm, &path, mode)?;
                            }
                        }
                        // uid/gid apply independently (`os.chown(p, uid, -1)` keeps
                        // the other side); fill the missing side from the current
                        // owner rather than dropping the whole chown.
                        if request.uid.is_some() || request.gid.is_some() {
                            let current = vm.kernel.stat(&path).map_err(kernel_error)?;
                            let uid = request.uid.unwrap_or(current.uid);
                            let gid = request.gid.unwrap_or(current.gid);
                            vm.kernel.chown(&path, uid, gid).map_err(kernel_error)?;
                        }
                        if let (Some(atime_ms), Some(mtime_ms)) =
                            (request.atime_ms, request.mtime_ms)
                        {
                            vm.kernel
                                .utimes(&path, atime_ms, mtime_ms)
                                .map_err(kernel_error)?;
                            if mirror {
                                mirror_guest_utimes_to_shadow(
                                    vm,
                                    &path,
                                    VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(atime_ms)),
                                    VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(mtime_ms)),
                                    true,
                                )?;
                            }
                        }
                        Ok(PythonVfsRpcResponsePayload::Empty)
                    })()
                }
                PythonVfsRpcMethod::HttpRequest
                | PythonVfsRpcMethod::DnsLookup
                | PythonVfsRpcMethod::SubprocessRun
                | PythonVfsRpcMethod::SocketConnect
                | PythonVfsRpcMethod::SocketSend
                | PythonVfsRpcMethod::SocketRecv
                | PythonVfsRpcMethod::SocketClose
                | PythonVfsRpcMethod::UdpCreate
                | PythonVfsRpcMethod::UdpSendto
                | PythonVfsRpcMethod::UdpRecvfrom => Err(SidecarError::InvalidState(String::from(
                    "python non-filesystem RPC reached filesystem dispatcher unexpectedly",
                ))),
            }
        }
        Err(error) => Err(error),
    };

    let Some(vm) = sidecar.vms.get_mut(vm_id) else {
        log_stale_process_event(&sidecar.bridge, vm_id, process_id, "python VFS RPC");
        return Ok(());
    };
    let Some(process) = vm.active_processes.get_mut(process_id) else {
        log_stale_process_event(&sidecar.bridge, vm_id, process_id, "python VFS RPC");
        return Ok(());
    };

    match response {
        Ok(payload) => process
            .execution
            .respond_python_vfs_rpc_success(request.id, payload),
        Err(error) => process.execution.respond_python_vfs_rpc_error(
            request.id,
            "ERR_AGENTOS_PYTHON_VFS_RPC",
            error.to_string(),
        ),
    }
}

fn stale_filesystem_request_error<B>(
    sidecar: &NativeSidecar<B>,
    vm_id: &str,
    process_id: Option<&str>,
    context: &str,
) -> SidecarError
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let message = match process_id {
        Some(process_id) => format!(
            "Ignoring stale filesystem request during {context}: VM {vm_id} process {process_id} was already reaped"
        ),
        None => format!(
            "Ignoring stale filesystem request during {context}: VM {vm_id} was already reaped"
        ),
    };
    let _ = sidecar.bridge.emit_log(vm_id, message.clone());
    SidecarError::InvalidState(message)
}

pub(crate) fn normalize_python_vfs_rpc_path(path: &str) -> Result<String, SidecarError> {
    if !path.starts_with('/') {
        return Err(SidecarError::InvalidState(format!(
            "python VFS RPC path {path} must be absolute within {PYTHON_VFS_RPC_GUEST_ROOT}"
        )));
    }

    // Root is `/`: Python may address the whole guest VFS. Textual `..` segments
    // are resolved by `normalize_path`, and the kernel enforces fs permissions
    // plus mount-confinement (openat2 RESOLVE_BENEATH refuses escaping symlinks)
    // on every op — so confinement is the kernel's job, not a prefix check here.
    let normalized = normalize_path(path);
    debug_assert_eq!(PYTHON_VFS_RPC_GUEST_ROOT, "/");
    Ok(normalized)
}

/// Kernel-VFS-backed reader for resolver unit tests and kernel-only callers.
#[cfg(test)]
struct KernelModuleFsReader<'a> {
    kernel: &'a mut SidecarKernel,
}

#[cfg(test)]
impl ModuleFsReader for KernelModuleFsReader<'_> {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        self.kernel.realpath(guest_path).ok()
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        let bytes = self.kernel.read_file(guest_path).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        self.kernel
            .stat(guest_path)
            .ok()
            .map(|stat| stat.is_directory)
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        self.kernel.exists(guest_path).unwrap_or(false)
    }
}

/// Module reader for live JavaScript processes. In the NodeRuntime embedding,
/// guest filesystem calls operate on the process' mapped host shadow first and
/// reconcile back to the kernel on exit. Module resolution must therefore check
/// that same process shadow before falling back to the sidecar kernel, otherwise
/// `fs.writeFileSync(...); await import(...)` observes an older filesystem.
struct ProcessModuleFsReader<'a> {
    kernel: &'a mut SidecarKernel,
    process: &'a ActiveProcess,
}

impl ProcessModuleFsReader<'_> {
    fn normalize_guest_path(&self, guest_path: &str) -> String {
        normalize_process_filesystem_rpc_path(self.process, guest_path)
    }

    fn mapped_host_path(&self, guest_path: &str) -> Option<MappedRuntimeHostPath> {
        mapped_runtime_host_path_for_read(self.process, guest_path)
    }

    fn materialize_mapped_path(
        &mut self,
        guest_path: &str,
        mapped: &MappedRuntimeHostPath,
    ) -> Result<(), SidecarError> {
        materialize_mapped_host_path_from_kernel(
            self.kernel,
            self.process.kernel_pid,
            guest_path,
            mapped,
        )
    }

    fn open_mapped_path(
        &mut self,
        guest_path: &str,
        operation: &'static str,
        flags: OFlag,
    ) -> Option<MappedRuntimeOpenedPath> {
        let mapped = self.mapped_host_path(guest_path)?;
        self.materialize_mapped_path(guest_path, &mapped).ok()?;
        open_mapped_runtime_beneath(&mapped, operation, flags, Mode::empty()).ok()
    }
}

impl ModuleFsReader for ProcessModuleFsReader<'_> {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        let normalized = self.normalize_guest_path(guest_path);
        if self
            .open_mapped_path(&normalized, "module.realpath", O_PATH_ANCHOR)
            .is_some()
        {
            return Some(normalized);
        }
        self.kernel.realpath(&normalized).ok()
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        let normalized = self.normalize_guest_path(guest_path);
        if let Some(opened) = self.open_mapped_path(&normalized, "module.readFile", OFlag::O_RDONLY)
        {
            if let Ok(source) = fs::read_to_string(opened.handle.proc_path()) {
                return Some(source);
            }
        }

        let bytes = self.kernel.read_file(&normalized).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        let normalized = self.normalize_guest_path(guest_path);
        if let Some(opened) = self.open_mapped_path(&normalized, "module.stat", O_PATH_ANCHOR) {
            if let Ok(metadata) = fs::metadata(opened.handle.proc_path()) {
                return Some(metadata.is_dir());
            }
        }

        self.kernel
            .stat(&normalized)
            .ok()
            .map(|stat| stat.is_directory)
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        let normalized = self.normalize_guest_path(guest_path);
        if self
            .open_mapped_path(&normalized, "module.exists", O_PATH_ANCHOR)
            .is_some()
        {
            return true;
        }
        self.kernel.exists(&normalized).unwrap_or(false)
    }
}

/// Resolve / load / format / batch-resolve module requests against the kernel
/// VFS. Routed here from `service_javascript_sync_rpc` for the
/// `__resolve_module` / `__load_file` / `__module_format` /
/// `__batch_resolve_modules` methods (mapped from the guest bridge's
/// `_resolveModule` / `_loadFile` / `_moduleFormat` / `_batchResolveModules`).
/// The `/opt/agentos/pkgs/<name>/<version>` root containing `guest_entrypoint`,
/// when the entrypoint lives inside a projected package. `current` is a valid
/// version segment here — the resolver canonicalizes it through the kernel.
fn agentos_package_version_root(guest_entrypoint: &str) -> Option<String> {
    let rest = guest_entrypoint.strip_prefix("/opt/agentos/pkgs/")?;
    let mut parts = rest.split('/');
    let name = parts.next().filter(|part| !part.is_empty())?;
    let version = parts.next().filter(|part| !part.is_empty())?;
    Some(format!("/opt/agentos/pkgs/{name}/{version}"))
}

fn is_bare_module_specifier(specifier: &str) -> bool {
    !(specifier.starts_with('/')
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier == "."
        || specifier == ".."
        || specifier.starts_with('#')
        || specifier.starts_with("file:"))
}

pub(crate) fn service_javascript_module_sync_rpc(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &JavascriptSyncRpcRequest,
) -> Result<Value, SidecarError> {
    // Self-contained package processes (agent adapters, packed JS commands)
    // carry their whole dependency closure inside the package mount. A bare
    // specifier that misses from an unpackaged context (a parent module path
    // like `/root` from cwd-based requires) retries from the package's own
    // version root, so packed packages resolve exactly what they shipped.
    let package_fallback_from = process
        .env
        .get("AGENTOS_GUEST_ENTRYPOINT")
        .and_then(|entrypoint| agentos_package_version_root(entrypoint));
    let mut cache = std::mem::take(&mut process.module_resolution_cache);
    let value = {
        let reader = ProcessModuleFsReader {
            kernel,
            process: &*process,
        };
        let mut resolver = ModuleResolver::new(reader, &mut cache);

        match request.method.as_str() {
            "__resolve_module" | "_resolveModule" | "_resolveModuleSync" => {
                let specifier =
                    javascript_sync_rpc_arg_str(&request.args, 0, "module resolve specifier")?;
                let parent = request.args.get(1).and_then(Value::as_str).unwrap_or("/");
                let mode = match request.args.get(2).and_then(Value::as_str) {
                    Some("import") => ModuleResolveMode::Import,
                    Some("require") => ModuleResolveMode::Require,
                    // `_resolveModule` defaults to import; `_resolveModuleSync` to require.
                    _ if request.method == "_resolveModuleSync" => ModuleResolveMode::Require,
                    _ => ModuleResolveMode::Import,
                };
                let mut resolved = resolver.resolve_module(specifier, parent, mode);
                if resolved.is_none() && is_bare_module_specifier(specifier) {
                    if let Some(fallback_from) = package_fallback_from
                        .as_deref()
                        .filter(|fallback| *fallback != parent)
                    {
                        resolved = resolver.resolve_module(specifier, fallback_from, mode);
                    }
                }
                if resolved.is_none() && std::env::var("AGENTOS_MODULE_READER_TRACE").is_ok() {
                    eprintln!("kernel-resolve MISS: {specifier} from {parent} mode={mode:?}");
                }
                resolved.map(Value::String).unwrap_or(Value::Null)
            }
            "__load_file" | "_loadFile" | "_loadFileSync" => {
                let path = javascript_sync_rpc_arg_str(&request.args, 0, "module load path")?;
                resolver
                    .load_file(path)
                    .map(Value::String)
                    .unwrap_or(Value::Null)
            }
            "__module_format" | "_moduleFormat" => {
                let path = javascript_sync_rpc_arg_str(&request.args, 0, "module format path")?;
                resolver
                    .module_format(path)
                    .map(|format: LocalResolvedModuleFormat| {
                        Value::String(String::from(format.as_str()))
                    })
                    .unwrap_or(Value::Null)
            }
            "__batch_resolve_modules" | "_batchResolveModules" => {
                resolver.batch_resolve_modules(&request.args)
            }
            other => {
                process.module_resolution_cache = cache;
                return Err(SidecarError::InvalidState(format!(
                    "unsupported JavaScript module sync RPC method {other}"
                )));
            }
        }
    };
    process.module_resolution_cache = cache;

    Ok(value)
}

#[derive(Clone, Copy, Default)]
struct FsSyncPhaseStats {
    calls: u64,
    total_ns: u128,
    max_ns: u128,
}

static FS_SYNC_PHASES: OnceLock<Mutex<BTreeMap<String, FsSyncPhaseStats>>> = OnceLock::new();

struct FsSyncPhaseTimer<'a> {
    method: &'a str,
    start: Option<Instant>,
}

impl<'a> FsSyncPhaseTimer<'a> {
    fn start(method: &'a str) -> Self {
        let start = fs_sync_phases_enabled().then(Instant::now);
        Self { method, start }
    }
}

impl Drop for FsSyncPhaseTimer<'_> {
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        record_fs_sync_phase(self.method, start.elapsed().as_nanos());
    }
}

fn record_fs_sync_subphase(method: &str, stage: &str, start: Instant) {
    if !fs_sync_phases_enabled() {
        return;
    }
    record_fs_sync_phase(&format!("{method}:{stage}"), start.elapsed().as_nanos());
}

fn fs_sync_phases_enabled() -> bool {
    matches!(env::var("AGENTOS_FS_SYNC_PHASES").as_deref(), Ok("1"))
}

fn record_fs_sync_phase(method: &str, elapsed_ns: u128) {
    let phases = FS_SYNC_PHASES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let Ok(mut phases) = phases.lock() else {
        return;
    };
    let stats = phases.entry(method.to_string()).or_default();
    stats.calls += 1;
    stats.total_ns += elapsed_ns;
    stats.max_ns = stats.max_ns.max(elapsed_ns);

    let Some(path) = env::var_os("AGENTOS_FS_SYNC_PHASES_FILE") else {
        return;
    };
    let mut output = String::new();
    for (method, stats) in phases.iter() {
        let total_us = stats.total_ns / 1_000;
        let avg_us = if stats.calls == 0 {
            0
        } else {
            total_us / u128::from(stats.calls)
        };
        let max_us = stats.max_ns / 1_000;
        output.push_str(&format!(
            "method={method} calls={} total_us={total_us} avg_us={avg_us} max_us={max_us}\n",
            stats.calls
        ));
    }
    let _ = fs::write(path, output);
}

fn fs_sync_request_marks_host_write_dirty(
    request: &JavascriptSyncRpcRequest,
) -> Result<bool, SidecarError> {
    Ok(match request.method.as_str() {
        "fs.open" | "fs.openSync" => {
            let flags = javascript_sync_rpc_arg_u32(&request.args, 1, "filesystem open flags")?;
            mapped_host_open_is_writable(flags)
        }
        "fs.write"
        | "fs.writeSync"
        | "fs.writevSync"
        | "fs.writeFileSync"
        | "fs.promises.writeFile"
        | "fs.mkdirSync"
        | "fs.promises.mkdir"
        | "fs.copyFileSync"
        | "fs.promises.copyFile"
        | "fs.symlinkSync"
        | "fs.promises.symlink"
        | "fs.linkSync"
        | "fs.promises.link"
        | "fs.renameSync"
        | "fs.promises.rename"
        | "fs.rmdirSync"
        | "fs.promises.rmdir"
        | "fs.unlinkSync"
        | "fs.promises.unlink"
        | "fs.chmodSync"
        | "fs.promises.chmod"
        | "fs.chownSync"
        | "fs.promises.chown"
        | "fs.utimesSync"
        | "fs.promises.utimes"
        | "fs.lutimesSync"
        | "fs.promises.lutimes"
        | "fs.futimesSync" => true,
        _ => false,
    })
}

pub(crate) fn service_javascript_fs_read_sync_rpc(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    kernel_pid: u32,
    request: &JavascriptSyncRpcRequest,
) -> Result<Vec<u8>, SidecarError> {
    let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem read fd")?;
    let length = usize::try_from(javascript_sync_rpc_arg_u64(
        &request.args,
        1,
        "filesystem read length",
    )?)
    .map_err(|_| {
        SidecarError::InvalidState("filesystem read length must fit within usize".to_string())
    })?;
    let position =
        javascript_sync_rpc_arg_u64_optional(&request.args, 2, "filesystem read position")?;
    if let Some(mapped) = process.mapped_host_fd_mut(fd) {
        let value = read_mapped_host_fd(mapped, fd, length, position)?;
        return javascript_sync_rpc_bytes_arg(
            std::slice::from_ref(&value),
            0,
            "filesystem mapped read response",
        );
    }
    match position {
        Some(offset) => kernel.fd_pread(EXECUTION_DRIVER_NAME, kernel_pid, fd, length, offset),
        None => kernel.fd_read(EXECUTION_DRIVER_NAME, kernel_pid, fd, length),
    }
    .map_err(kernel_error)
}

pub(crate) fn service_javascript_fs_sync_rpc(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    kernel_pid: u32,
    request: &JavascriptSyncRpcRequest,
) -> Result<Value, SidecarError> {
    let _phase_timer = FsSyncPhaseTimer::start(request.method.as_str());
    if fs_sync_request_marks_host_write_dirty(request)? {
        process.mark_host_write_dirty();
    }
    match request.method.as_str() {
        "fs.open" | "fs.openSync" => {
            let phase_start = Instant::now();
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem open path")?;
            let path = path.as_str();
            let flags = javascript_sync_rpc_arg_u32(&request.args, 1, "filesystem open flags")?;
            let mode =
                javascript_sync_rpc_arg_u32_optional(&request.args, 2, "filesystem open mode")?;
            record_fs_sync_subphase(request.method.as_str(), "parse", phase_start);
            let phase_start = Instant::now();
            match mapped_runtime_host_path(process, path, mapped_host_open_is_writable(flags)) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    record_fs_sync_subphase(
                        request.method.as_str(),
                        "mapped_host_match",
                        phase_start,
                    );
                    let phase_start = Instant::now();
                    materialize_mapped_host_path_from_kernel(
                        kernel,
                        kernel_pid,
                        path,
                        &mapped_host,
                    )?;
                    record_fs_sync_subphase(
                        request.method.as_str(),
                        "materialize_mapped_host",
                        phase_start,
                    );
                    let phase_start = Instant::now();
                    let opened = open_mapped_runtime_beneath(
                        &mapped_host,
                        "fs.open",
                        OFlag::from_bits_truncate(flags as i32),
                        Mode::from_bits_truncate(mode.unwrap_or(0o666) as _),
                    )?;
                    record_fs_sync_subphase(
                        request.method.as_str(),
                        "open_mapped_beneath",
                        phase_start,
                    );
                    let host_path = opened.host_path.clone();
                    let phase_start = Instant::now();
                    return open_mapped_host_fd(
                        process,
                        host_path,
                        Some(path.to_string()),
                        opened.handle.proc_path(),
                        flags,
                    )
                    .inspect(|_| {
                        record_fs_sync_subphase(
                            request.method.as_str(),
                            "open_mapped_fd",
                            phase_start,
                        );
                    });
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            record_fs_sync_subphase(request.method.as_str(), "mapped_host_none", phase_start);
            let phase_start = Instant::now();
            kernel
                .fd_open(EXECUTION_DRIVER_NAME, kernel_pid, path, flags, mode)
                .map(|fd| json!(fd))
                .map_err(|error| kernel_path_error("fs.open", path, error))
                .inspect(|_| {
                    record_fs_sync_subphase(request.method.as_str(), "kernel_fd_open", phase_start);
                })
        }
        "fs.read" | "fs.readSync" => {
            service_javascript_fs_read_sync_rpc(kernel, process, kernel_pid, request)
                .map(|bytes| javascript_sync_rpc_bytes_value(&bytes))
        }
        "fs.write" | "fs.writeSync" => {
            let phase_start = Instant::now();
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem write fd")?;
            let contents = if let Some(bytes) = request.raw_bytes_args.get(&1) {
                bytes.clone()
            } else {
                javascript_sync_rpc_bytes_arg(&request.args, 1, "filesystem write contents")?
            };
            let position = javascript_sync_rpc_arg_u64_optional(
                &request.args,
                2,
                "filesystem write position",
            )?;
            record_fs_sync_subphase(request.method.as_str(), "parse", phase_start);
            let phase_start = Instant::now();
            if let Some(mapped) = process.mapped_host_fd_mut(fd) {
                record_fs_sync_subphase(request.method.as_str(), "mapped_fd_match", phase_start);
                return write_mapped_host_fd(mapped, fd, &contents, position);
            }
            record_fs_sync_subphase(request.method.as_str(), "mapped_fd_none", phase_start);
            let phase_start = Instant::now();
            let written = match position {
                Some(offset) => kernel
                    .fd_pwrite(EXECUTION_DRIVER_NAME, kernel_pid, fd, &contents, offset)
                    .map_err(kernel_error)?,
                None => kernel
                    .fd_write(EXECUTION_DRIVER_NAME, kernel_pid, fd, &contents)
                    .map_err(kernel_error)?,
            };
            record_fs_sync_subphase(request.method.as_str(), "kernel_fd_write", phase_start);
            let phase_start = Instant::now();
            let surfaces_stdio =
                position.is_none() && kernel_fd_surfaces_stdio_event(kernel, kernel_pid, fd)?;
            record_fs_sync_subphase(request.method.as_str(), "stdio_check", phase_start);
            if surfaces_stdio {
                let phase_start = Instant::now();
                let event = if fd == 1 {
                    ActiveExecutionEvent::Stdout(contents)
                } else {
                    ActiveExecutionEvent::Stderr(contents)
                };
                process.queue_pending_execution_event(event)?;
                record_fs_sync_subphase(request.method.as_str(), "queue_stdio_event", phase_start);
            } else {
                let phase_start = Instant::now();
                mirror_kernel_fd_contents_to_process_shadow(kernel, process, kernel_pid, fd)?;
                record_fs_sync_subphase(request.method.as_str(), "mirror_shadow", phase_start);
            }
            Ok(json!(written))
        }
        "fs.writevSync" => {
            let phase_start = Instant::now();
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem writev fd")?;
            let contents = request.raw_bytes_args.get(&1).ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "filesystem writev requires raw byte payload",
                ))
            })?;
            let position = javascript_sync_rpc_arg_u64_optional(
                &request.args,
                2,
                "filesystem writev position",
            )?;
            let buffers = decode_javascript_writev_raw_payload(contents)?;
            record_fs_sync_subphase(request.method.as_str(), "parse", phase_start);

            let mut total_written = 0usize;
            if let Some(mapped) = process.mapped_host_fd_mut(fd) {
                record_fs_sync_subphase(request.method.as_str(), "mapped_fd_match", phase_start);
                let mut next_position = position;
                for buffer in buffers {
                    let written = write_all_mapped_host_fd(mapped, fd, buffer, next_position)?;
                    total_written = total_written.saturating_add(written);
                    if let Some(position) = &mut next_position {
                        *position = position.saturating_add(written as u64);
                    }
                }
                return Ok(json!(total_written));
            }
            record_fs_sync_subphase(request.method.as_str(), "mapped_fd_none", phase_start);

            let surfaces_stdio =
                position.is_none() && kernel_fd_surfaces_stdio_event(kernel, kernel_pid, fd)?;
            let mut next_position = position;
            let mut combined_stdio = Vec::new();
            for buffer in buffers {
                let mut offset = 0usize;
                while offset < buffer.len() {
                    let slice = &buffer[offset..];
                    let written = match next_position {
                        Some(position) => kernel
                            .fd_pwrite(EXECUTION_DRIVER_NAME, kernel_pid, fd, slice, position)
                            .map_err(kernel_error)?,
                        None => kernel
                            .fd_write(EXECUTION_DRIVER_NAME, kernel_pid, fd, slice)
                            .map_err(kernel_error)?,
                    };
                    if written == 0 {
                        return Err(SidecarError::Execution(format!(
                            "EIO: filesystem writev made no progress on fd {fd}"
                        )));
                    }
                    offset += written;
                    total_written = total_written.saturating_add(written);
                    if let Some(position) = &mut next_position {
                        *position = position.saturating_add(written as u64);
                    }
                }
                if surfaces_stdio {
                    combined_stdio.extend_from_slice(buffer);
                }
            }
            record_fs_sync_subphase(request.method.as_str(), "kernel_fd_write", phase_start);
            if surfaces_stdio && !combined_stdio.is_empty() {
                let event = if fd == 1 {
                    ActiveExecutionEvent::Stdout(combined_stdio)
                } else {
                    ActiveExecutionEvent::Stderr(combined_stdio)
                };
                process.queue_pending_execution_event(event)?;
            } else {
                mirror_kernel_fd_contents_to_process_shadow(kernel, process, kernel_pid, fd)?;
            }
            Ok(json!(total_written))
        }
        "fs.close" | "fs.closeSync" => {
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem close fd")?;
            if process.close_mapped_host_fd(fd) {
                return Ok(Value::Null);
            }
            kernel
                .fd_close(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.fstat" | "fs.fstatSync" => {
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem fstat fd")?;
            if let Some(mapped) = process.mapped_host_fd(fd) {
                let metadata = mapped.file.metadata().map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to stat mapped guest fd {fd} -> {}: {error}",
                        mapped.path.display()
                    ))
                })?;
                return Ok(javascript_sync_rpc_host_stat_value(&metadata));
            }
            kernel
                .fd_stat(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map_err(kernel_error)?;
            kernel
                .dev_fd_stat(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map(javascript_sync_rpc_stat_value)
                .map_err(kernel_error)
        }
        "fs.fsyncSync" | "fs.fdatasyncSync" => {
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem sync fd")?;
            if let Some(mapped) = process.mapped_host_fd(fd) {
                return mapped
                    .file
                    .sync_all()
                    .map(|()| Value::Null)
                    .map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to sync mapped guest fd {fd} -> {}: {error}",
                            mapped.path.display()
                        ))
                    });
            }
            kernel
                .fd_stat(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map(|_| Value::Null)
                .map_err(kernel_error)
        }
        "fs.ftruncateSync" => {
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem ftruncate fd")?;
            let length = javascript_sync_rpc_arg_u64_optional(
                &request.args,
                1,
                "filesystem ftruncate length",
            )?
            .unwrap_or(0);
            if let Some(mapped_guest_path) = process
                .mapped_host_fd_mut(fd)
                .map(|mapped| mapped.guest_path.clone())
            {
                // `length` is guest-controlled. Bound it before resizing the host
                // file so a hostile value cannot create an enormous sparse host
                // file. For a VFS-visible guest path the kernel truncate below is
                // the primary (configured) size enforcement and mirrors the new
                // length without reading the whole host file into sidecar memory.
                if length > MAX_MAPPED_TRUNCATE_BYTES {
                    return Err(SidecarError::Io(format!(
                        "ftruncate length {length} exceeds maximum \
                         {MAX_MAPPED_TRUNCATE_BYTES} for mapped guest fd {fd}"
                    )));
                }
                if let Some(guest_path) = mapped_guest_path.as_deref() {
                    kernel
                        .truncate(guest_path, length)
                        .map_err(|error| kernel_path_error("fs.ftruncate", guest_path, error))?;
                }
                let mapped = process.mapped_host_fd_mut(fd).ok_or_else(|| {
                    SidecarError::Io(format!("mapped guest fd {fd} disappeared during ftruncate"))
                })?;
                mapped.file.set_len(length).map_err(|error| {
                    SidecarError::Io(format!("failed to truncate mapped guest fd {fd}: {error}"))
                })?;
                if let Some(guest_path) = mapped_guest_path.as_deref() {
                    mirror_kernel_path_to_process_shadow(kernel, process, kernel_pid, guest_path)?;
                }
                return Ok(Value::Null);
            }
            let fd_stat = kernel
                .fd_stat(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map_err(kernel_error)?;
            if (fd_stat.flags & libc::O_ACCMODE as u32) == libc::O_RDONLY as u32 {
                return Err(SidecarError::Execution(format!(
                    "EBADF: file descriptor {fd} is not open for writing"
                )));
            }
            let path = kernel
                .fd_path(EXECUTION_DRIVER_NAME, kernel_pid, fd)
                .map_err(kernel_error)?;
            kernel.truncate(&path, length).map_err(kernel_error)?;
            mirror_kernel_path_to_process_shadow(kernel, process, kernel_pid, &path)?;
            Ok(Value::Null)
        }
        "fs.readFileSync" | "fs.promises.readFile" => {
            let path = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                0,
                "filesystem readFile path",
            )?;
            let path = path.as_str();
            let encoding = javascript_sync_rpc_encoding(&request.args);
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let opened = open_mapped_runtime_beneath(
                    &mapped_host,
                    "fs.readFile",
                    OFlag::O_RDONLY,
                    Mode::empty(),
                )?;
                let content = fs::read(opened.handle.proc_path()).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to read mapped guest file {} -> {}: {error}",
                        path,
                        opened.host_path.display()
                    ))
                })?;
                return Ok(match encoding.as_deref() {
                    Some("utf8") | Some("utf-8") => {
                        Value::String(String::from_utf8_lossy(&content).into_owned())
                    }
                    _ => javascript_sync_rpc_bytes_value(&content),
                });
            }
            kernel
                .read_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map(|content| match encoding.as_deref() {
                    Some("utf8") | Some("utf-8") => {
                        Value::String(String::from_utf8_lossy(&content).into_owned())
                    }
                    _ => javascript_sync_rpc_bytes_value(&content),
                })
                .map_err(kernel_error)
        }
        "fs.writeFileSync" | "fs.promises.writeFile" => {
            let path = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                0,
                "filesystem writeFile path",
            )?;
            let path = path.as_str();
            let contents = if let Some(bytes) = request.raw_bytes_args.get(&1) {
                bytes.clone()
            } else {
                javascript_sync_rpc_bytes_arg(&request.args, 1, "filesystem writeFile contents")?
            };
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    let opened = open_mapped_runtime_beneath(
                        &mapped_host,
                        "fs.writeFile",
                        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
                        Mode::from_bits_truncate(
                            javascript_sync_rpc_option_u32(&request.args, 2, "mode")?
                                .unwrap_or(0o666) as _,
                        ),
                    )?;
                    fs::write(opened.handle.proc_path(), contents).map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to write mapped guest file {} -> {}: {error}",
                            path,
                            opened.host_path.display()
                        ))
                    })?;
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            kernel
                .write_file_for_process(
                    EXECUTION_DRIVER_NAME,
                    kernel_pid,
                    path,
                    contents,
                    javascript_sync_rpc_option_u32(&request.args, 2, "mode")?,
                )
                .map_err(|error| kernel_path_error("fs.writeFile", path, error))?;
            mirror_kernel_path_to_process_shadow(kernel, process, kernel_pid, path)?;
            Ok(Value::Null)
        }
        "fs.statSync" | "fs.promises.stat" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem stat path")?;
            let path = path.as_str();
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let opened = open_mapped_runtime_beneath(
                    &mapped_host,
                    "fs.stat",
                    O_PATH_ANCHOR,
                    Mode::empty(),
                )?;
                let metadata = fs::metadata(opened.handle.proc_path()).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to stat mapped guest path {} -> {}: {error}",
                        path,
                        opened.host_path.display()
                    ))
                })?;
                return Ok(javascript_sync_rpc_host_stat_value(&metadata));
            }
            kernel
                .stat_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map(javascript_sync_rpc_stat_value)
                .map_err(kernel_error)
        }
        "fs.lstatSync" | "fs.promises.lstat" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem lstat path")?;
            let path = path.as_str();
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let metadata = mapped_runtime_symlink_metadata(&mapped_host, "fs.lstat")?;
                return Ok(metadata.to_value());
            }
            kernel
                .lstat_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map(javascript_sync_rpc_stat_value)
                .map_err(kernel_error)
        }
        "fs.readdirSync" | "fs.promises.readdir" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem readdir path")?;
            let path = path.as_str();
            service_javascript_fs_readdir_entries(kernel, process, kernel_pid, path)
                .map(javascript_sync_rpc_readdir_typed_value)
        }
        "fs.mkdirSync" | "fs.promises.mkdir" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem mkdir path")?;
            let path = path.as_str();
            let recursive =
                javascript_sync_rpc_option_bool(&request.args, 1, "recursive").unwrap_or(false);
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    if mapped_runtime_relative_path(&mapped_host)? == Path::new(".") {
                        create_mapped_runtime_root_directory(&mapped_host, recursive)?;
                    } else {
                        if recursive {
                            ensure_mapped_runtime_parent_dirs(&mapped_host, "fs.mkdir")?;
                            let parent =
                                open_mapped_runtime_parent_beneath(&mapped_host, "fs.mkdir")?;
                            create_mapped_runtime_directory(&parent, path, true)?;
                        } else {
                            let parent =
                                open_mapped_runtime_parent_beneath(&mapped_host, "fs.mkdir")?;
                            create_mapped_runtime_directory(&parent, path, false)?;
                        }
                    }
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            kernel
                .mkdir_for_process(
                    EXECUTION_DRIVER_NAME,
                    kernel_pid,
                    path,
                    recursive,
                    javascript_sync_rpc_option_u32(&request.args, 1, "mode")?,
                )
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.accessSync" | "fs.promises.access" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem access path")?;
            let path = path.as_str();
            let mode =
                javascript_sync_rpc_arg_u32_optional(&request.args, 1, "filesystem access mode")?
                    .unwrap_or(0);
            let valid_mask = libc::R_OK as u32 | libc::W_OK as u32 | libc::X_OK as u32;
            if mode & !valid_mask != 0 {
                return Err(SidecarError::Execution(format!(
                    "EINVAL: invalid filesystem access mode {mode:o}"
                )));
            }
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let opened = open_mapped_runtime_beneath(
                    &mapped_host,
                    "fs.access",
                    O_PATH_ANCHOR,
                    Mode::empty(),
                )?;
                let metadata = fs::metadata(opened.handle.proc_path()).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to access mapped guest path {} -> {}: {error}",
                        path,
                        opened.host_path.display()
                    ))
                })?;
                if !filesystem_access_mode_is_allowed(metadata.permissions().mode(), mode) {
                    return Err(filesystem_access_denied(path, mode));
                }
                return Ok(Value::Null);
            }
            let stat = kernel
                .stat_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map_err(kernel_error)?;
            if !filesystem_access_mode_is_allowed(stat.mode, mode) {
                return Err(filesystem_access_denied(path, mode));
            }
            Ok(Value::Null)
        }
        "fs.copyFileSync" | "fs.promises.copyFile" => {
            let source = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                0,
                "filesystem copyFile source",
            )?;
            let source = source.as_str();
            let destination = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                1,
                "filesystem copyFile destination",
            )?;
            let destination = destination.as_str();
            let source_host = mapped_runtime_host_path(process, source, false);
            let destination_host = mapped_runtime_host_path(process, destination, true);
            if matches!(destination_host, Some(MappedRuntimeHostAccess::ReadOnly(_))) {
                return Err(read_only_mapped_runtime_host_path_error(destination));
            }
            if source_host.is_some() || destination_host.is_some() {
                let contents = match source_host {
                    Some(MappedRuntimeHostAccess::Writable(ref mapped_host)) => {
                        let opened = open_mapped_runtime_beneath(
                            mapped_host,
                            "fs.copyFile source",
                            OFlag::O_RDONLY,
                            Mode::empty(),
                        )?;
                        fs::read(opened.handle.proc_path()).map_err(|error| {
                            SidecarError::Io(format!(
                                "failed to read mapped guest file {} -> {}: {error}",
                                source,
                                opened.host_path.display()
                            ))
                        })?
                    }
                    Some(MappedRuntimeHostAccess::ReadOnly(ref mapped_host)) => {
                        let opened = open_mapped_runtime_beneath(
                            mapped_host,
                            "fs.copyFile source",
                            OFlag::O_RDONLY,
                            Mode::empty(),
                        )?;
                        fs::read(opened.handle.proc_path()).map_err(|error| {
                            SidecarError::Io(format!(
                                "failed to read mapped guest file {} -> {}: {error}",
                                source,
                                opened.host_path.display()
                            ))
                        })?
                    }
                    None => kernel
                        .read_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, source)
                        .map_err(kernel_error)?,
                };
                return match destination_host {
                    Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                        let opened = open_mapped_runtime_beneath(
                            &mapped_host,
                            "fs.copyFile destination",
                            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
                            Mode::from_bits_truncate(0o666),
                        )?;
                        fs::write(opened.handle.proc_path(), contents)
                            .map(|()| Value::Null)
                            .map_err(|error| {
                                SidecarError::Io(format!(
                                    "failed to write mapped guest file {} -> {}: {error}",
                                    destination,
                                    opened.host_path.display()
                                ))
                            })
                    }
                    Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                        Err(read_only_mapped_runtime_host_path_error(destination))
                    }
                    None => kernel
                        .write_file_for_process(
                            EXECUTION_DRIVER_NAME,
                            kernel_pid,
                            destination,
                            contents,
                            None,
                        )
                        .map(|()| Value::Null)
                        .map_err(kernel_error),
                };
            }
            let contents = kernel
                .read_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, source)
                .map_err(kernel_error)?;
            kernel
                .write_file_for_process(
                    EXECUTION_DRIVER_NAME,
                    kernel_pid,
                    destination,
                    contents,
                    None,
                )
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.existsSync" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem exists path")?;
            let path = path.as_str();
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let exists = match open_mapped_runtime_beneath(
                    &mapped_host,
                    "fs.exists",
                    O_PATH_ANCHOR,
                    Mode::empty(),
                ) {
                    Ok(opened) => fs::metadata(opened.handle.proc_path()).is_ok(),
                    Err(_) => false,
                };
                return Ok(Value::Bool(exists));
            }
            kernel
                .exists_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map(Value::Bool)
                .map_err(kernel_error)
        }
        "fs.readlinkSync" | "fs.promises.readlink" => {
            let path = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                0,
                "filesystem readlink path",
            )?;
            let path = path.as_str();
            if let Some(mapped_host) = mapped_runtime_host_path_for_read(process, path) {
                materialize_mapped_host_path_from_kernel(kernel, kernel_pid, path, &mapped_host)?;
                let target = read_mapped_runtime_link(&mapped_host, path, "fs.readlink")?;
                return Ok(Value::String(target.to_string_lossy().into_owned()));
            }
            kernel
                .read_link_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                .map(Value::String)
                .map_err(kernel_error)
        }
        "fs.symlinkSync" | "fs.promises.symlink" => {
            let target =
                javascript_sync_rpc_arg_str(&request.args, 0, "filesystem symlink target")?;
            let link_path =
                javascript_sync_rpc_path_arg(process, &request.args, 1, "filesystem symlink path")?;
            let link_path = link_path.as_str();
            match mapped_runtime_host_path(process, link_path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    ensure_mapped_runtime_parent_dirs(&mapped_host, "fs.symlink")?;
                    let parent = open_mapped_runtime_parent_beneath(&mapped_host, "fs.symlink")?;
                    let host_path = parent.host_path.join(&parent.child_name);
                    remove_shadow_path_if_exists(&host_path, link_path)?;
                    mapped_child_symlink(&parent, target).map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to create mapped guest symlink {} -> {} ({target}): {error}",
                            link_path,
                            host_path.display()
                        ))
                    })?;
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(link_path));
                }
                None => {}
            }
            kernel
                .symlink(target, link_path)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.linkSync" | "fs.promises.link" => {
            let source =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem link source")?;
            let source = source.as_str();
            let destination =
                javascript_sync_rpc_path_arg(process, &request.args, 1, "filesystem link path")?;
            let destination = destination.as_str();
            kernel
                .link(source, destination)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.renameSync" | "fs.promises.rename" => {
            let source = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                0,
                "filesystem rename source",
            )?;
            let source = source.as_str();
            let destination = javascript_sync_rpc_path_arg(
                process,
                &request.args,
                1,
                "filesystem rename destination",
            )?;
            let destination = destination.as_str();
            let source_host = mapped_runtime_host_path(process, source, true);
            let destination_host = mapped_runtime_host_path(process, destination, true);
            if matches!(source_host, Some(MappedRuntimeHostAccess::ReadOnly(_))) {
                return Err(read_only_mapped_runtime_host_path_error(source));
            }
            if matches!(destination_host, Some(MappedRuntimeHostAccess::ReadOnly(_))) {
                return Err(read_only_mapped_runtime_host_path_error(destination));
            }
            if source_host.is_some() || destination_host.is_some() {
                return rename_mapped_host_path(source, source_host, destination, destination_host);
            }
            kernel
                .rename(source, destination)
                .map_err(kernel_error)?;
            // Mirror the rename into the process shadow tree, otherwise the
            // exit-time shadow->kernel sync resurrects the stale source path
            // (the shadow walk only copies entries in, it cannot express
            // deletions).
            rename_process_shadow_path(process, source, destination)?;
            Ok(Value::Null)
        }
        "fs.rmdirSync" | "fs.promises.rmdir" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem rmdir path")?;
            let path = path.as_str();
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    let parent = open_mapped_runtime_parent_beneath(&mapped_host, "fs.rmdir")?;
                    let host_path = parent.host_path.join(&parent.child_name);
                    mapped_child_remove_dir(&parent).map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to remove mapped guest directory {} -> {}: {error}",
                            path,
                            host_path.display()
                        ))
                    })?;
                    // Mirror the deletion into the kernel for the same reason as
                    // fs.unlink below: readdir/stat merge kernel state, so a
                    // kernel-backed directory would otherwise resurrect.
                    if let Err(error) = kernel.remove_dir(path) {
                        if error.code() != "ENOENT" {
                            return Err(kernel_error(error));
                        }
                    }
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            kernel.remove_dir(path).map_err(kernel_error)?;
            // Mirror the removal into the process shadow tree, otherwise the
            // exit-time shadow->kernel sync resurrects the deleted directory.
            remove_process_shadow_path(process, path)?;
            Ok(Value::Null)
        }
        "fs.unlinkSync" | "fs.promises.unlink" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem unlink path")?;
            let path = path.as_str();
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    let parent = open_mapped_runtime_parent_beneath(&mapped_host, "fs.unlink")?;
                    let host_path = parent.host_path.join(&parent.child_name);
                    mapped_child_remove_file(&parent).map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to remove mapped guest file {} -> {}: {error}",
                            path,
                            host_path.display()
                        ))
                    })?;
                    // The shadow cannot express deletions, and readdir/stat now
                    // merge kernel state into the mapped view — without a kernel
                    // removal a kernel-backed file (e.g. created by a wasm
                    // command) would resurrect in the very listing that follows
                    // the unlink. Best-effort: absent kernel entries are fine.
                    if let Err(error) = kernel.remove_file(path) {
                        if error.code() != "ENOENT" {
                            return Err(kernel_error(error));
                        }
                    }
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            kernel.remove_file(path).map_err(kernel_error)?;
            // Mirror the deletion into the process shadow tree: wasm guest
            // deletions route kernel-direct, and without removing the shadow
            // copy the exit-time shadow->kernel sync resurrects the file for
            // later builtins in the same shell and for subsequent execs.
            remove_process_shadow_path(process, path)?;
            Ok(Value::Null)
        }
        "fs.chmodSync" | "fs.promises.chmod" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem chmod path")?;
            let path = path.as_str();
            let mode = javascript_sync_rpc_arg_u32(&request.args, 1, "filesystem chmod mode")?;
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    materialize_mapped_host_path_from_kernel(
                        kernel,
                        kernel_pid,
                        path,
                        &mapped_host,
                    )?;
                    let opened = open_mapped_runtime_beneath(
                        &mapped_host,
                        "fs.chmod",
                        O_PATH_ANCHOR,
                        Mode::empty(),
                    )?;
                    if kernel
                        .exists_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                        .map_err(kernel_error)?
                    {
                        kernel.chmod(path, mode).map_err(kernel_error)?;
                    }
                    fs::set_permissions(
                        opened.handle.proc_path(),
                        fs::Permissions::from_mode(mode & 0o7777),
                    )
                    .map_err(|error| {
                        SidecarError::Io(format!(
                            "failed to chmod mapped guest path {} -> {}: {error}",
                            path,
                            opened.host_path.display()
                        ))
                    })?;
                    return Ok(Value::Null);
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            kernel
                .chmod(path, mode)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.chownSync" | "fs.promises.chown" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem chown path")?;
            let path = path.as_str();
            let uid = javascript_sync_rpc_arg_u32(&request.args, 1, "filesystem chown uid")?;
            let gid = javascript_sync_rpc_arg_u32(&request.args, 2, "filesystem chown gid")?;
            kernel
                .chown(path, uid, gid)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        "fs.utimesSync" | "fs.promises.utimes" | "fs.lutimesSync" | "fs.promises.lutimes" => {
            let path =
                javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem utimes path")?;
            let path = path.as_str();
            let atime = parse_utime_arg(&request.args, 1, "filesystem utimes atime")?;
            let mtime = parse_utime_arg(&request.args, 2, "filesystem utimes mtime")?;
            let follow_symlinks = !matches!(
                request.method.as_str(),
                "fs.lutimesSync" | "fs.promises.lutimes"
            );
            if let Some(shadow_path) = process_shadow_host_path(process, path) {
                if fs::symlink_metadata(&shadow_path).is_ok() {
                    let result = if follow_symlinks {
                        kernel.utimes_spec(path, atime, mtime)
                    } else {
                        kernel.lutimes(path, atime, mtime)
                    };
                    if let Err(error) = result {
                        if error.code() != "ENOENT" {
                            return Err(kernel_error(error));
                        }
                    }
                    apply_host_path_utimens(
                        &shadow_path,
                        atime,
                        mtime,
                        follow_symlinks,
                        &format!("failed to update process shadow path times {path}"),
                    )?;
                    return Ok(Value::Null);
                }
            }
            match mapped_runtime_host_path(process, path, true) {
                Some(MappedRuntimeHostAccess::Writable(mapped_host)) => {
                    let mapped_host_exists = match fs::symlink_metadata(&mapped_host.host_path) {
                        Ok(_) => true,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            materialize_mapped_host_path_from_kernel(
                                kernel,
                                kernel_pid,
                                path,
                                &mapped_host,
                            )?;
                            fs::symlink_metadata(&mapped_host.host_path).is_ok()
                        }
                        Err(error) => {
                            return Err(SidecarError::Io(format!(
                                "failed to inspect mapped guest path {} -> {}: {error}",
                                path,
                                mapped_host.host_path.display()
                            )));
                        }
                    };
                    if mapped_host_exists {
                        let context = format!("failed to update mapped guest path times {path}");
                        // Resolve the host target up front and hold the handle across
                        // the kernel update so the apply below operates on the verified
                        // fd. (The handle must stay alive: a `/proc/self/fd` path is
                        // only valid while its fd is open, and the macOS fd-relative
                        // path needs the live parent fd.)
                        let follow_handle = if follow_symlinks {
                            Some(open_mapped_runtime_beneath(
                                &mapped_host,
                                "fs.utimes",
                                O_PATH_ANCHOR,
                                Mode::empty(),
                            )?)
                        } else {
                            None
                        };
                        let parent_handle = if follow_symlinks {
                            None
                        } else {
                            Some(open_mapped_runtime_parent_beneath(
                                &mapped_host,
                                "fs.lutimes",
                            )?)
                        };
                        if kernel
                            .exists_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
                            .map_err(kernel_error)?
                        {
                            let result = if follow_symlinks {
                                kernel.utimes_spec(path, atime, mtime)
                            } else {
                                kernel.lutimes(path, atime, mtime)
                            };
                            if let Err(error) = result {
                                if error.code() != "ENOENT" {
                                    return Err(kernel_error(error));
                                }
                            }
                        }
                        if let Some(opened) = &follow_handle {
                            apply_host_path_utimens(
                                &opened.handle.proc_path(),
                                atime,
                                mtime,
                                true,
                                &context,
                            )?;
                        } else if let Some(parent) = &parent_handle {
                            apply_mapped_child_utimens(parent, atime, mtime, &context)?;
                        }
                        return Ok(Value::Null);
                    }
                }
                Some(MappedRuntimeHostAccess::ReadOnly(_)) => {
                    return Err(read_only_mapped_runtime_host_path_error(path));
                }
                None => {}
            }
            if follow_symlinks {
                kernel
                    .utimes_spec(path, atime, mtime)
                    .map_err(kernel_error)?;
            } else {
                kernel.lutimes(path, atime, mtime).map_err(kernel_error)?;
            };
            Ok(Value::Null)
        }
        "fs.futimesSync" => {
            let fd = javascript_sync_rpc_arg_u32(&request.args, 0, "filesystem futimes fd")?;
            let atime = parse_utime_arg(&request.args, 1, "filesystem futimes atime")?;
            let mtime = parse_utime_arg(&request.args, 2, "filesystem futimes mtime")?;
            kernel
                .futimes(EXECUTION_DRIVER_NAME, kernel_pid, fd, atime, mtime)
                .map(|()| Value::Null)
                .map_err(kernel_error)
        }
        _ => Err(SidecarError::InvalidState(format!(
            "unsupported JavaScript sync RPC method {}",
            request.method
        ))),
    }
}

fn kernel_fd_surfaces_stdio_event(
    kernel: &SidecarKernel,
    kernel_pid: u32,
    fd: u32,
) -> Result<bool, SidecarError> {
    let path = match fd {
        1 | 2 => kernel
            .fd_path(EXECUTION_DRIVER_NAME, kernel_pid, fd)
            .map_err(kernel_error)?,
        _ => return Ok(false),
    };
    Ok(matches!(
        (fd, path.as_str()),
        (1, "/dev/stdout") | (2, "/dev/stderr")
    ))
}

fn javascript_sync_rpc_path_arg(
    process: &ActiveProcess,
    args: &[Value],
    index: usize,
    label: &str,
) -> Result<String, SidecarError> {
    let path = javascript_sync_rpc_arg_str(args, index, label)?;
    Ok(normalize_process_filesystem_rpc_path(process, path))
}

fn normalize_process_filesystem_rpc_path(process: &ActiveProcess, path: &str) -> String {
    let host_path = Path::new(path);
    if host_path.is_absolute() {
        let normalized_host_path = normalize_host_path(host_path);
        if let Some(guest_path) =
            guest_path_from_runtime_host_mappings(process, &normalized_host_path)
        {
            return guest_path;
        }
        if let Some(sandbox_root) = process
            .env
            .get(EXECUTION_SANDBOX_ROOT_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|path| normalize_host_path(&path))
        {
            if let Ok(suffix) = normalized_host_path.strip_prefix(&sandbox_root) {
                let suffix = suffix.to_string_lossy();
                return normalize_path(&format!("/{}", suffix.trim_start_matches('/')));
            }
        }
    }
    path.to_owned()
}

fn guest_path_from_runtime_host_mappings(
    process: &ActiveProcess,
    host_path: &Path,
) -> Option<String> {
    runtime_guest_host_mappings(process)
        .into_iter()
        .filter_map(|(guest_path, host_root)| {
            host_path.strip_prefix(&host_root).ok().map(|suffix| {
                let suffix = suffix.to_string_lossy();
                normalize_path(&format!(
                    "{}/{}",
                    guest_path.trim_end_matches('/'),
                    suffix.trim_start_matches('/')
                ))
            })
        })
        .max_by_key(String::len)
}

fn runtime_guest_host_mappings(process: &ActiveProcess) -> Vec<(String, PathBuf)> {
    let Some(mappings) = process
        .env
        .get("AGENTOS_GUEST_PATH_MAPPINGS")
        .and_then(|value| serde_json::from_str::<Vec<RuntimeGuestPathMappingWire>>(value).ok())
    else {
        return Vec::new();
    };
    mappings
        .into_iter()
        .filter_map(|mapping| {
            if mapping.guest_path.is_empty() || mapping.host_path.is_empty() {
                return None;
            }
            let host_root = PathBuf::from(mapping.host_path);
            let normalized_host_root = if host_root.is_absolute() {
                normalize_host_path(&host_root)
            } else {
                normalize_host_path(&std::env::current_dir().ok()?.join(host_root))
            };
            Some((normalize_path(&mapping.guest_path), normalized_host_root))
        })
        .collect()
}

fn mirror_kernel_fd_contents_to_process_shadow(
    kernel: &mut SidecarKernel,
    process: &ActiveProcess,
    kernel_pid: u32,
    fd: u32,
) -> Result<(), SidecarError> {
    let path = kernel
        .fd_path(EXECUTION_DRIVER_NAME, kernel_pid, fd)
        .map_err(kernel_error)?;
    let path = normalize_process_filesystem_rpc_path(process, &path);
    mirror_kernel_path_to_process_shadow(kernel, process, kernel_pid, &path)
}

fn mirror_kernel_path_to_process_shadow(
    kernel: &mut SidecarKernel,
    process: &ActiveProcess,
    kernel_pid: u32,
    guest_path: &str,
) -> Result<(), SidecarError> {
    let Some(shadow_path) = resolve_process_guest_path_to_host(process, guest_path) else {
        return Ok(());
    };
    let bytes = kernel
        .read_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, guest_path)
        .map_err(kernel_error)?;
    write_process_shadow_file(&shadow_path, guest_path, &bytes)
}

fn write_process_shadow_file(
    shadow_path: &Path,
    guest_path: &str,
    bytes: &[u8],
) -> Result<(), SidecarError> {
    if let Some(parent) = shadow_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for {}: {error}",
                normalize_path(guest_path)
            ))
        })?;
    }
    match fs::symlink_metadata(shadow_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(shadow_path).map_err(|error| {
                SidecarError::Io(format!(
                    "failed to replace shadow symlink for {}: {error}",
                    normalize_path(guest_path)
                ))
            })?;
        }
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(shadow_path).map_err(|error| {
                SidecarError::Io(format!(
                    "failed to replace shadow directory for {}: {error}",
                    normalize_path(guest_path)
                ))
            })?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SidecarError::Io(format!(
                "failed to inspect shadow path for {}: {error}",
                normalize_path(guest_path)
            )));
        }
    }
    fs::write(shadow_path, bytes).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror kernel file {} into process shadow: {error}",
            normalize_path(guest_path)
        ))
    })
}

fn javascript_sync_rpc_stat_value(stat: VirtualStat) -> Value {
    let mut value = Map::with_capacity(18);
    value.insert("mode".to_string(), Value::from(stat.mode));
    value.insert("size".to_string(), Value::from(stat.size));
    value.insert("blocks".to_string(), Value::from(stat.blocks));
    value.insert("dev".to_string(), Value::from(stat.dev));
    value.insert("rdev".to_string(), Value::from(stat.rdev));
    value.insert("isDirectory".to_string(), Value::from(stat.is_directory));
    value.insert(
        "isSymbolicLink".to_string(),
        Value::from(stat.is_symbolic_link),
    );
    value.insert("atimeMs".to_string(), Value::from(stat.atime_ms));
    value.insert("atimeNsec".to_string(), Value::from(stat.atime_nsec));
    value.insert("mtimeMs".to_string(), Value::from(stat.mtime_ms));
    value.insert("mtimeNsec".to_string(), Value::from(stat.mtime_nsec));
    value.insert("ctimeMs".to_string(), Value::from(stat.ctime_ms));
    value.insert("ctimeNsec".to_string(), Value::from(stat.ctime_nsec));
    value.insert("birthtimeMs".to_string(), Value::from(stat.birthtime_ms));
    value.insert("ino".to_string(), Value::from(stat.ino));
    value.insert("nlink".to_string(), Value::from(stat.nlink));
    value.insert("uid".to_string(), Value::from(stat.uid));
    value.insert("gid".to_string(), Value::from(stat.gid));
    Value::Object(value)
}

fn javascript_sync_rpc_host_stat_value(metadata: &fs::Metadata) -> Value {
    let mut value = Map::with_capacity(15);
    value.insert("mode".to_string(), Value::from(metadata.mode()));
    value.insert("size".to_string(), Value::from(metadata.size()));
    value.insert("blocks".to_string(), Value::from(metadata.blocks()));
    value.insert("dev".to_string(), Value::from(metadata.dev()));
    value.insert("rdev".to_string(), Value::from(metadata.rdev()));
    value.insert("isDirectory".to_string(), Value::from(metadata.is_dir()));
    value.insert(
        "isSymbolicLink".to_string(),
        Value::from(metadata.file_type().is_symlink()),
    );
    value.insert(
        "atimeMs".to_string(),
        Value::from(metadata.atime() * 1000 + (metadata.atime_nsec() / 1_000_000)),
    );
    value.insert(
        "mtimeMs".to_string(),
        Value::from(metadata.mtime() * 1000 + (metadata.mtime_nsec() / 1_000_000)),
    );
    value.insert(
        "ctimeMs".to_string(),
        Value::from(metadata.ctime() * 1000 + (metadata.ctime_nsec() / 1_000_000)),
    );
    value.insert(
        "birthtimeMs".to_string(),
        Value::from(metadata.ctime() * 1000 + (metadata.ctime_nsec() / 1_000_000)),
    );
    value.insert("ino".to_string(), Value::from(metadata.ino()));
    value.insert("nlink".to_string(), Value::from(metadata.nlink()));
    value.insert("uid".to_string(), Value::from(metadata.uid()));
    value.insert("gid".to_string(), Value::from(metadata.gid()));
    Value::Object(value)
}

fn mapped_runtime_host_path(
    process: &ActiveProcess,
    guest_path: &str,
    writable: bool,
) -> Option<MappedRuntimeHostAccess> {
    if process_prefers_kernel_fs_sync_rpc(process) {
        return None;
    }

    let normalized = if guest_path.starts_with('/') {
        normalize_path(guest_path)
    } else {
        normalize_path(&format!(
            "{}/{}",
            process.guest_cwd.trim_end_matches('/'),
            guest_path
        ))
    };
    let mappings = process
        .env
        .get("AGENTOS_GUEST_PATH_MAPPINGS")
        .and_then(|value| serde_json::from_str::<Vec<RuntimeGuestPathMappingWire>>(value).ok())?;
    let mut sorted_mappings = mappings
        .into_iter()
        .filter_map(|mapping| {
            (!mapping.guest_path.is_empty() && !mapping.host_path.is_empty()).then_some((
                normalize_path(&mapping.guest_path),
                PathBuf::from(mapping.host_path),
            ))
        })
        .collect::<Vec<_>>();
    sorted_mappings.sort_by_key(|mapping| std::cmp::Reverse(mapping.0.len()));
    let readable_roots = runtime_host_access_roots(process, "AGENTOS_EXTRA_FS_READ_PATHS")?;
    let writable_roots = writable
        .then(|| runtime_host_access_roots(process, "AGENTOS_EXTRA_FS_WRITE_PATHS"))
        .flatten()
        .unwrap_or_default();

    for (guest_root, host_root) in sorted_mappings {
        if guest_root != "/"
            && normalized != guest_root
            && !normalized.starts_with(&format!("{guest_root}/"))
        {
            continue;
        }
        if guest_root == "/" && !normalized.starts_with('/') {
            continue;
        }

        let normalized_host_root = if host_root.is_absolute() {
            normalize_host_path(&host_root)
        } else {
            normalize_host_path(&std::env::current_dir().ok()?.join(host_root))
        };
        let suffix = if guest_root == "/" {
            normalized.trim_start_matches('/')
        } else {
            normalized
                .strip_prefix(&guest_root)
                .unwrap_or_default()
                .trim_start_matches('/')
        };
        let host_path = if suffix.is_empty() {
            normalized_host_root.clone()
        } else {
            normalized_host_root.join(suffix)
        };

        let is_asset_path = guest_root == PYTHON_PYODIDE_GUEST_ROOT
            || normalized == PYTHON_PYODIDE_GUEST_ROOT
            || normalized.starts_with(&format!("{PYTHON_PYODIDE_GUEST_ROOT}/"));
        let is_cache_path = guest_root == PYTHON_PYODIDE_CACHE_GUEST_ROOT
            || normalized == PYTHON_PYODIDE_CACHE_GUEST_ROOT
            || normalized.starts_with(&format!("{PYTHON_PYODIDE_CACHE_GUEST_ROOT}/"));
        if is_asset_path && !writable {
            return Some(MappedRuntimeHostAccess::Writable(MappedRuntimeHostPath {
                guest_path: normalized.clone(),
                host_root: normalized_host_root.clone(),
                host_path,
            }));
        }
        if is_cache_path {
            return Some(MappedRuntimeHostAccess::Writable(MappedRuntimeHostPath {
                guest_path: normalized.clone(),
                host_root: normalized_host_root.clone(),
                host_path,
            }));
        }

        let Some(read_root) = readable_roots
            .iter()
            .find(|root| path_is_within_root(&host_path, root))
            .cloned()
        else {
            continue;
        };
        if !writable {
            return Some(MappedRuntimeHostAccess::Writable(MappedRuntimeHostPath {
                guest_path: normalized.clone(),
                host_root: read_root.clone(),
                host_path,
            }));
        }
        if let Some(write_root) = writable_roots
            .iter()
            .find(|root| path_is_within_root(&host_path, root))
            .cloned()
        {
            return Some(MappedRuntimeHostAccess::Writable(MappedRuntimeHostPath {
                guest_path: normalized.clone(),
                host_root: write_root.clone(),
                host_path,
            }));
        }
        if guest_root != "/" {
            return Some(MappedRuntimeHostAccess::ReadOnly(MappedRuntimeHostPath {
                guest_path: normalized.clone(),
                host_root: read_root.clone(),
                host_path,
            }));
        }
    }

    None
}

fn mapped_runtime_host_path_for_read(
    process: &ActiveProcess,
    guest_path: &str,
) -> Option<MappedRuntimeHostPath> {
    match mapped_runtime_host_path(process, guest_path, false) {
        Some(MappedRuntimeHostAccess::Writable(mapped_host))
        | Some(MappedRuntimeHostAccess::ReadOnly(mapped_host)) => Some(mapped_host),
        None => None,
    }
}

fn process_shadow_host_path(process: &ActiveProcess, guest_path: &str) -> Option<PathBuf> {
    if process_prefers_kernel_fs_sync_rpc(process) {
        return None;
    }

    let normalized_guest_path = normalized_process_guest_path(process, guest_path);
    let normalized_guest_cwd = normalize_path(&process.guest_cwd);
    let mut host_root = normalize_host_path(&process.host_cwd);
    for _ in normalized_guest_cwd
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        host_root = host_root.parent()?.to_path_buf();
    }
    if normalized_guest_path == "/" {
        Some(host_root)
    } else {
        Some(host_root.join(normalized_guest_path.trim_start_matches('/')))
    }
}

fn normalized_process_guest_path(process: &ActiveProcess, guest_path: &str) -> String {
    if guest_path.starts_with('/') {
        normalize_path(guest_path)
    } else {
        normalize_path(&format!(
            "{}/{}",
            process.guest_cwd.trim_end_matches('/'),
            guest_path
        ))
    }
}

fn process_prefers_kernel_fs_sync_rpc(process: &ActiveProcess) -> bool {
    process.runtime == GuestRuntimeKind::WebAssembly
        && process
            .env
            .get(EXECUTION_SANDBOX_ROOT_ENV)
            .is_some_and(|value| !value.is_empty())
}

fn runtime_host_access_roots(process: &ActiveProcess, key: &str) -> Option<Vec<PathBuf>> {
    process
        .env
        .get(key)
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .map(|roots| {
            roots
                .into_iter()
                .map(PathBuf::from)
                .map(|root| normalize_host_path(&root))
                .collect()
        })
}

fn mapped_runtime_child_mount_basenames(process: &ActiveProcess, guest_path: &str) -> Vec<String> {
    let normalized = normalize_path(guest_path);
    let mappings = process
        .env
        .get("AGENTOS_GUEST_PATH_MAPPINGS")
        .and_then(|value| serde_json::from_str::<Vec<RuntimeGuestPathMappingWire>>(value).ok())
        .unwrap_or_default();
    let mut basenames = BTreeSet::new();
    for mapping in mappings {
        let guest_root = normalize_path(&mapping.guest_path);
        if guest_root == "/" || guest_root == normalized {
            continue;
        }
        if mapped_runtime_parent_path(&guest_root) == normalized {
            basenames.insert(mapped_runtime_basename(&guest_root));
        }
    }
    basenames.into_iter().collect()
}

fn mapped_runtime_parent_path(path: &str) -> String {
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

fn mapped_runtime_basename(path: &str) -> String {
    let normalized = normalize_path(path);
    Path::new(&normalized)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("/"))
}

fn read_only_mapped_runtime_host_path_error(guest_path: &str) -> SidecarError {
    SidecarError::Kernel(format!("EROFS: read-only filesystem: {guest_path}"))
}

#[cfg(not(target_os = "macos"))]
fn mapped_runtime_resolve_flags() -> ResolveFlag {
    ResolveFlag::RESOLVE_BENEATH | ResolveFlag::RESOLVE_NO_MAGICLINKS
}

/// Open `relative` strictly beneath the mapped mount root, returning an owned
/// raw fd. Linux resolves with `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)`
/// anchored on `root_dir`; macOS has no such syscall and resolves beneath
/// `host_root` with cap-std (see [`crate::macos_fs`]).
#[cfg(not(target_os = "macos"))]
fn mapped_runtime_open_fd(
    root_dir: &AnchoredFd,
    _host_root: &Path,
    relative: &Path,
    flags: OFlag,
    mode: Mode,
) -> Result<RawFd, Errno> {
    openat2(
        root_dir.as_raw_fd(),
        relative,
        OpenHow::new()
            .flags(flags | OFlag::O_CLOEXEC)
            .mode(mode)
            .resolve(mapped_runtime_resolve_flags()),
    )
}

#[cfg(target_os = "macos")]
fn mapped_runtime_open_fd(
    _root_dir: &AnchoredFd,
    host_root: &Path,
    relative: &Path,
    flags: OFlag,
    mode: Mode,
) -> Result<RawFd, Errno> {
    crate::macos_fs::resolve_beneath(host_root, relative, flags, mode)
}

fn mapped_runtime_relative_path(mapped: &MappedRuntimeHostPath) -> Result<PathBuf, SidecarError> {
    let normalized_root = normalize_host_path(&mapped.host_root);
    let normalized_path = normalize_host_path(&mapped.host_path);
    if !path_is_within_root(&normalized_path, &normalized_root) {
        return Err(mapped_runtime_host_path_escape_error(
            mapped,
            &normalized_path,
        ));
    }
    let relative = normalized_path
        .strip_prefix(&normalized_root)
        .map_err(|error| {
            SidecarError::InvalidState(format!(
                "failed to relativize mapped guest path {} ({} against {}): {error}",
                mapped.guest_path,
                normalized_path.display(),
                normalized_root.display()
            ))
        })?;
    Ok(if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative.to_path_buf()
    })
}

fn open_mapped_runtime_root_dir(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<AnchoredFd, SidecarError> {
    let fd = open(
        &mapped.host_root,
        OFlag::O_CLOEXEC | OFlag::O_DIRECTORY | OFlag::O_RDONLY,
        Mode::empty(),
    )
    .map_err(|error| {
        SidecarError::Io(format!(
            "{operation}: failed to open mapped host root {} for {}: {}",
            mapped.host_root.display(),
            mapped.guest_path,
            std::io::Error::from_raw_os_error(error as i32)
        ))
    })?;
    Ok(AnchoredFd { fd })
}

fn open_mapped_runtime_beneath(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
    flags: OFlag,
    mode: Mode,
) -> Result<MappedRuntimeOpenedPath, SidecarError> {
    let root_dir = open_mapped_runtime_root_dir(mapped, operation)?;
    let relative = mapped_runtime_relative_path(mapped)?;
    let open_mode = if flags.intersects(OFlag::O_CREAT | O_TMPFILE_FLAG) {
        mode
    } else {
        Mode::empty()
    };
    let fd = mapped_runtime_open_fd(&root_dir, &mapped.host_root, &relative, flags, open_mode)
        .map_err(|error| mapped_runtime_open_error(operation, mapped, error))?;
    let handle = AnchoredFd { fd };
    let host_path = mapped_runtime_host_path_from_fd(mapped, operation, &handle)?;
    Ok(MappedRuntimeOpenedPath { handle, host_path })
}

fn open_mapped_runtime_directory_beneath(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
    relative: &Path,
) -> Result<MappedRuntimeOpenedPath, SidecarError> {
    let root_dir = open_mapped_runtime_root_dir(mapped, operation)?;
    let fd = mapped_runtime_open_fd(
        &root_dir,
        &mapped.host_root,
        relative,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY,
        Mode::empty(),
    )
    .map_err(|error| mapped_runtime_open_error(operation, mapped, error))?;
    let handle = AnchoredFd { fd };
    let host_path = mapped_runtime_host_path_from_fd(mapped, operation, &handle)?;
    Ok(MappedRuntimeOpenedPath { handle, host_path })
}

fn open_mapped_runtime_parent_beneath(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<MappedRuntimeParentPath, SidecarError> {
    let relative = mapped_runtime_relative_path(mapped)?;
    let child_name = relative.file_name().ok_or_else(|| {
        SidecarError::InvalidState(format!(
            "{operation}: mapped guest path {} has no parent-relative basename",
            mapped.guest_path
        ))
    })?;
    let parent_relative = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = open_mapped_runtime_directory_beneath(mapped, operation, parent_relative)?;
    Ok(MappedRuntimeParentPath {
        directory: directory.handle,
        host_path: directory.host_path,
        child_name: child_name.to_os_string(),
    })
}

/// Platform-neutral lstat result. Lets the mapped-runtime lstat path produce the
/// same guest-facing stat value from either a `std::fs::Metadata` (Linux, and
/// the macOS root case) or a raw `fstatat` result (macOS fd-relative child
/// lstat), so the operation stays fd-relative on macOS without a `std::fs`
/// metadata handle.
struct HostStat {
    mode: u32,
    size: u64,
    blocks: u64,
    dev: u64,
    rdev: u64,
    is_directory: bool,
    is_symbolic_link: bool,
    atime_ms: i64,
    mtime_ms: i64,
    ctime_ms: i64,
    ino: u64,
    nlink: u64,
    uid: u32,
    gid: u32,
}

impl HostStat {
    #[cfg_attr(not(test), allow(dead_code))]
    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn to_value(&self) -> Value {
        json!({
            "mode": self.mode,
            "size": self.size,
            "blocks": self.blocks,
            "dev": self.dev,
            "rdev": self.rdev,
            "isDirectory": self.is_directory,
            "isSymbolicLink": self.is_symbolic_link,
            "atimeMs": self.atime_ms,
            "mtimeMs": self.mtime_ms,
            "ctimeMs": self.ctime_ms,
            "birthtimeMs": self.ctime_ms,
            "ino": self.ino,
            "nlink": self.nlink,
            "uid": self.uid,
            "gid": self.gid,
        })
    }
}

impl From<&fs::Metadata> for HostStat {
    fn from(metadata: &fs::Metadata) -> Self {
        Self {
            mode: metadata.mode(),
            size: metadata.size(),
            blocks: metadata.blocks(),
            dev: metadata.dev(),
            rdev: metadata.rdev(),
            is_directory: metadata.is_dir(),
            is_symbolic_link: metadata.file_type().is_symlink(),
            atime_ms: metadata.atime() * 1000 + (metadata.atime_nsec() / 1_000_000),
            mtime_ms: metadata.mtime() * 1000 + (metadata.mtime_nsec() / 1_000_000),
            ctime_ms: metadata.ctime() * 1000 + (metadata.ctime_nsec() / 1_000_000),
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }
}

#[cfg(target_os = "macos")]
impl HostStat {
    fn from_filestat(stat: &nix::sys::stat::FileStat) -> Self {
        use nix::sys::stat::SFlag;
        let fmt = stat.st_mode & SFlag::S_IFMT.bits();
        Self {
            mode: stat.st_mode as u32,
            size: stat.st_size as u64,
            blocks: stat.st_blocks as u64,
            dev: stat.st_dev as u64,
            rdev: stat.st_rdev as u64,
            is_directory: fmt == SFlag::S_IFDIR.bits(),
            is_symbolic_link: fmt == SFlag::S_IFLNK.bits(),
            atime_ms: stat.st_atime * 1000 + (stat.st_atime_nsec / 1_000_000),
            mtime_ms: stat.st_mtime * 1000 + (stat.st_mtime_nsec / 1_000_000),
            ctime_ms: stat.st_ctime * 1000 + (stat.st_ctime_nsec / 1_000_000),
            ino: stat.st_ino,
            nlink: stat.st_nlink as u64,
            uid: stat.st_uid,
            gid: stat.st_gid,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_lstat(parent: &MappedRuntimeParentPath) -> std::io::Result<HostStat> {
    Ok(HostStat::from(&fs::symlink_metadata(
        mapped_runtime_parent_child_path(parent),
    )?))
}
#[cfg(target_os = "macos")]
fn mapped_child_lstat(parent: &MappedRuntimeParentPath) -> std::io::Result<HostStat> {
    let stat = nix::sys::stat::fstatat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(errno_to_io)?;
    Ok(HostStat::from_filestat(&stat))
}

fn mapped_runtime_symlink_metadata(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<HostStat, SidecarError> {
    let relative = mapped_runtime_relative_path(mapped)?;
    if relative == Path::new(".") {
        return fs::symlink_metadata(&mapped.host_path)
            .map(|metadata| HostStat::from(&metadata))
            .map_err(|error| {
                SidecarError::Io(format!(
                    "failed to lstat mapped guest path {} -> {}: {error}",
                    mapped.guest_path,
                    mapped.host_path.display()
                ))
            });
    }

    let parent = open_mapped_runtime_parent_beneath(mapped, operation)?;
    let host_path = parent.host_path.join(&parent.child_name);
    mapped_child_lstat(&parent).map_err(|error| {
        SidecarError::Io(format!(
            "failed to lstat mapped guest path {} -> {}: {error}",
            mapped.guest_path,
            host_path.display()
        ))
    })
}

fn read_mapped_runtime_link(
    mapped: &MappedRuntimeHostPath,
    guest_path: &str,
    operation: &str,
) -> Result<PathBuf, SidecarError> {
    if mapped_runtime_relative_path(mapped)? == Path::new(".") {
        return fs::read_link(&mapped.host_path).map_err(|error| {
            SidecarError::Io(format!(
                "failed to read mapped guest symlink {} -> {}: {error}",
                guest_path,
                mapped.host_path.display()
            ))
        });
    }

    let parent = open_mapped_runtime_parent_beneath(mapped, operation)?;
    let host_path = parent.host_path.join(&parent.child_name);
    mapped_child_read_link(&parent).map_err(|error| {
        SidecarError::Io(format!(
            "failed to read mapped guest symlink {} -> {}: {error}",
            guest_path,
            host_path.display()
        ))
    })
}

fn mapped_runtime_host_path_from_fd(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
    fd: &AnchoredFd,
) -> Result<PathBuf, SidecarError> {
    // Linux reads the magic symlink `/proc/self/fd/N`; macOS recovers the path
    // with `fcntl(F_GETPATH)` (see [`crate::macos_fs::fd_real_path`]).
    #[cfg(not(target_os = "macos"))]
    let resolved = fs::read_link(fd.proc_path());
    #[cfg(target_os = "macos")]
    let resolved = crate::macos_fs::fd_real_path(fd.as_raw_fd());
    resolved.map_err(|error| {
        SidecarError::Io(format!(
            "{operation}: failed to resolve anchored mapped guest path {}: {error}",
            mapped.guest_path
        ))
    })
}

#[cfg(not(target_os = "macos"))]
fn mapped_runtime_parent_child_path(parent: &MappedRuntimeParentPath) -> PathBuf {
    parent.directory.proc_path().join(&parent.child_name)
}

// ---------------------------------------------------------------------------
// Mapped-runtime child operations.
//
// On Linux these operate on the resolved parent fd by appending the child to
// `/proc/self/fd/N`. macOS cannot append path components to `/dev/fd/N`, so it
// performs the same operations with fd-relative `*at` calls anchored on the
// resolved parent fd — TOCTOU-safe, mirroring the host_dir plugin.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn errno_to_io(error: Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error as i32)
}

#[cfg(not(target_os = "macos"))]
fn create_dir_at(dir: &AnchoredFd, name: &std::ffi::OsStr) -> std::io::Result<()> {
    fs::create_dir(dir.proc_path().join(name))
}
#[cfg(target_os = "macos")]
fn create_dir_at(dir: &AnchoredFd, name: &std::ffi::OsStr) -> std::io::Result<()> {
    nix::sys::stat::mkdirat(Some(dir.as_raw_fd()), name, Mode::from_bits_truncate(0o777))
        .map_err(errno_to_io)
}

fn mapped_child_create_dir(parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    create_dir_at(&parent.directory, parent.child_name.as_os_str())
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_is_dir(parent: &MappedRuntimeParentPath) -> std::io::Result<bool> {
    Ok(fs::symlink_metadata(mapped_runtime_parent_child_path(parent))?.is_dir())
}
#[cfg(target_os = "macos")]
fn mapped_child_is_dir(parent: &MappedRuntimeParentPath) -> std::io::Result<bool> {
    use nix::sys::stat::SFlag;
    let stat = nix::sys::stat::fstatat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(errno_to_io)?;
    Ok(stat.st_mode & SFlag::S_IFMT.bits() == SFlag::S_IFDIR.bits())
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_remove_dir(parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    fs::remove_dir(mapped_runtime_parent_child_path(parent))
}
#[cfg(target_os = "macos")]
fn mapped_child_remove_dir(parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    nix::unistd::unlinkat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
        nix::unistd::UnlinkatFlags::RemoveDir,
    )
    .map_err(errno_to_io)
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_remove_file(parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    fs::remove_file(mapped_runtime_parent_child_path(parent))
}
#[cfg(target_os = "macos")]
fn mapped_child_remove_file(parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    nix::unistd::unlinkat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(errno_to_io)
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_symlink(parent: &MappedRuntimeParentPath, target: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, mapped_runtime_parent_child_path(parent))
}
#[cfg(target_os = "macos")]
fn mapped_child_symlink(parent: &MappedRuntimeParentPath, target: &str) -> std::io::Result<()> {
    nix::unistd::symlinkat(
        target,
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
    )
    .map_err(errno_to_io)
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_read_link(parent: &MappedRuntimeParentPath) -> std::io::Result<PathBuf> {
    fs::read_link(mapped_runtime_parent_child_path(parent))
}
#[cfg(target_os = "macos")]
fn mapped_child_read_link(parent: &MappedRuntimeParentPath) -> std::io::Result<PathBuf> {
    nix::fcntl::readlinkat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
    )
    .map(PathBuf::from)
    .map_err(errno_to_io)
}

/// Set access/modification times on a mapped child without following symlinks
/// (lutimes). Linux operates on `/proc/self/fd/N/child`; macOS uses fd-relative
/// `utimensat` anchored on the resolved parent fd.
#[cfg(not(target_os = "macos"))]
fn apply_mapped_child_utimens(
    parent: &MappedRuntimeParentPath,
    atime: VirtualUtimeSpec,
    mtime: VirtualUtimeSpec,
    context: &str,
) -> Result<(), SidecarError> {
    apply_host_path_utimens(
        &mapped_runtime_parent_child_path(parent),
        atime,
        mtime,
        false,
        context,
    )
}
#[cfg(target_os = "macos")]
fn apply_mapped_child_utimens(
    parent: &MappedRuntimeParentPath,
    atime: VirtualUtimeSpec,
    mtime: VirtualUtimeSpec,
    context: &str,
) -> Result<(), SidecarError> {
    let existing = match (atime, mtime) {
        (VirtualUtimeSpec::Omit, _) | (_, VirtualUtimeSpec::Omit) => {
            let stat = nix::sys::stat::fstatat(
                Some(parent.directory.as_raw_fd()),
                parent.child_name.as_os_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| SidecarError::Io(format!("{context}: failed to stat: {error}")))?;
            Some((
                VirtualTimeSpec {
                    sec: stat.st_atime,
                    nsec: stat.st_atime_nsec.max(0) as u32,
                },
                VirtualTimeSpec {
                    sec: stat.st_mtime,
                    nsec: stat.st_mtime_nsec.max(0) as u32,
                },
            ))
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
        resolve_host_utime(atime, existing_atime),
        resolve_host_utime(mtime, existing_mtime),
    ];
    utimensat(
        Some(parent.directory.as_raw_fd()),
        parent.child_name.as_os_str(),
        &times[0],
        &times[1],
        UtimensatFlags::NoFollowSymlink,
    )
    .map_err(|error| SidecarError::Io(format!("{context}: failed to set times: {error}")))
}

#[cfg(not(target_os = "macos"))]
fn mapped_child_rename(
    source: &MappedRuntimeParentPath,
    destination: &MappedRuntimeParentPath,
) -> std::io::Result<()> {
    rename_mapped_host_path_with_fallback(
        &mapped_runtime_parent_child_path(source),
        &mapped_runtime_parent_child_path(destination),
    )
}
#[cfg(target_os = "macos")]
fn mapped_child_rename(
    source: &MappedRuntimeParentPath,
    destination: &MappedRuntimeParentPath,
) -> std::io::Result<()> {
    // Same-filesystem rename is fd-relative (TOCTOU-safe). A cross-device rename
    // (EXDEV) cannot be done fd-relative, so fall back to the path-based copy on
    // the resolved real paths, exactly as the Linux fallback does.
    match nix::fcntl::renameat(
        Some(source.directory.as_raw_fd()),
        source.child_name.as_os_str(),
        Some(destination.directory.as_raw_fd()),
        destination.child_name.as_os_str(),
    ) {
        Ok(()) => Ok(()),
        Err(Errno::EXDEV) => move_mapped_host_path_across_devices(
            &source.host_path.join(&source.child_name),
            &destination.host_path.join(&destination.child_name),
        ),
        Err(error) => Err(errno_to_io(error)),
    }
}

fn create_mapped_runtime_directory(
    parent: &MappedRuntimeParentPath,
    guest_path: &str,
    recursive: bool,
) -> Result<(), SidecarError> {
    match mapped_child_create_dir(parent) {
        Ok(()) => Ok(()),
        Err(error) if recursive && error.kind() == std::io::ErrorKind::AlreadyExists => {
            match mapped_child_is_dir(parent) {
                Ok(true) => Ok(()),
                Ok(false) => Err(SidecarError::Io(format!(
                    "failed to create mapped guest directory {} -> {}: file exists and is not a directory",
                    guest_path,
                    parent.host_path.join(&parent.child_name).display()
                ))),
                Err(metadata_error) => Err(SidecarError::Io(format!(
                    "failed to inspect existing mapped guest directory {} -> {}: {metadata_error}",
                    guest_path,
                    parent.host_path.join(&parent.child_name).display()
                ))),
            }
        }
        Err(error) => Err(SidecarError::Io(format!(
            "failed to create mapped guest directory {} -> {}: {error}",
            guest_path,
            parent.host_path.join(&parent.child_name).display()
        ))),
    }
}

fn create_mapped_runtime_root_directory(
    mapped: &MappedRuntimeHostPath,
    recursive: bool,
) -> Result<(), SidecarError> {
    let relative = mapped_runtime_relative_path(mapped)?;
    if relative != Path::new(".") {
        return Err(SidecarError::InvalidState(format!(
            "fs.mkdir: mapped guest path {} is not the mapped root",
            mapped.guest_path
        )));
    }

    if recursive {
        match fs::create_dir_all(&mapped.host_path) {
            Ok(()) => Ok(()),
            Err(error) => Err(SidecarError::Io(format!(
                "failed to create mapped guest directory {} -> {}: {error}",
                mapped.guest_path,
                mapped.host_path.display()
            ))),
        }
    } else {
        match fs::create_dir(&mapped.host_path) {
            Ok(()) => Ok(()),
            Err(error) => Err(SidecarError::Io(format!(
                "failed to create mapped guest directory {} -> {}: {error}",
                mapped.guest_path,
                mapped.host_path.display()
            ))),
        }
    }
}

fn ensure_mapped_runtime_parent_dirs(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<(), SidecarError> {
    let relative = mapped_runtime_relative_path(mapped)?;
    let Some(parent_relative) = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if parent_relative == Path::new(".") {
        return Ok(());
    }

    for index in 0..parent_relative.components().count() {
        let prefix = parent_relative
            .components()
            .take(index + 1)
            .collect::<PathBuf>();
        if open_mapped_runtime_directory_beneath(mapped, operation, &prefix).is_ok() {
            continue;
        }

        let prefix_parent = prefix
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let prefix_name = prefix.file_name().ok_or_else(|| {
            SidecarError::InvalidState(format!(
                "{operation}: invalid mapped guest directory prefix for {}",
                mapped.guest_path
            ))
        })?;
        let parent_dir = open_mapped_runtime_directory_beneath(mapped, operation, prefix_parent)?;
        create_dir_at(&parent_dir.handle, prefix_name).map_err(|error| {
            SidecarError::Io(format!(
                "{operation}: failed to create mapped guest parent {} under {}: {error}",
                mapped.guest_path,
                parent_dir.host_path.display()
            ))
        })?;
    }

    Ok(())
}

fn mapped_runtime_open_error(
    operation: &str,
    mapped: &MappedRuntimeHostPath,
    error: Errno,
) -> SidecarError {
    match error {
        Errno::EXDEV => mapped_runtime_host_path_escape_error(mapped, &mapped.host_path),
        other => SidecarError::Io(format!(
            "{operation}: failed to open mapped guest path {} beneath {}: {}",
            mapped.guest_path,
            mapped.host_root.display(),
            std::io::Error::from_raw_os_error(other as i32)
        )),
    }
}

fn mapped_runtime_host_path_escape_error(
    mapped: &MappedRuntimeHostPath,
    resolved: &Path,
) -> SidecarError {
    SidecarError::Io(format!(
        "mapped guest path {} escapes mapped host root {} via {}",
        mapped.guest_path,
        mapped.host_root.display(),
        resolved.display()
    ))
}

fn mapped_host_open_is_writable(flags: u32) -> bool {
    let access_mode = flags & libc::O_ACCMODE as u32;
    access_mode == libc::O_WRONLY as u32
        || access_mode == libc::O_RDWR as u32
        || flags & libc::O_APPEND as u32 != 0
        || flags & libc::O_CREAT as u32 != 0
        || flags & libc::O_TRUNC as u32 != 0
}

fn materialize_mapped_host_path_from_kernel(
    kernel: &mut SidecarKernel,
    kernel_pid: u32,
    guest_path: &str,
    mapped: &MappedRuntimeHostPath,
) -> Result<(), SidecarError> {
    let host_path = &mapped.host_path;
    match fs::symlink_metadata(host_path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SidecarError::Io(format!(
                "failed to inspect mapped host path for {} -> {}: {error}",
                guest_path,
                host_path.display()
            )));
        }
    }

    if !kernel
        .exists_for_process(EXECUTION_DRIVER_NAME, kernel_pid, guest_path)
        .map_err(kernel_error)?
    {
        return Ok(());
    }

    let stat = kernel
        .lstat_for_process(EXECUTION_DRIVER_NAME, kernel_pid, guest_path)
        .map_err(kernel_error)?;

    if stat.is_symbolic_link {
        let target = kernel
            .read_link_for_process(EXECUTION_DRIVER_NAME, kernel_pid, guest_path)
            .map_err(kernel_error)?;
        ensure_mapped_runtime_parent_dirs(mapped, "fs.materialize")?;
        let parent = open_mapped_runtime_parent_beneath(mapped, "fs.materialize")?;
        mapped_child_symlink(&parent, &target).map_err(|error| {
            SidecarError::Io(format!(
                "failed to materialize mapped guest symlink {} -> {} ({target}): {error}",
                guest_path,
                parent.host_path.join(&parent.child_name).display()
            ))
        })?;
        return Ok(());
    } else if stat.is_directory {
        if mapped_runtime_relative_path(mapped)? == Path::new(".") {
            create_mapped_runtime_root_directory(mapped, true)?;
        } else {
            ensure_mapped_runtime_parent_dirs(mapped, "fs.materialize")?;
            let parent = open_mapped_runtime_parent_beneath(mapped, "fs.materialize")?;
            create_mapped_runtime_directory(&parent, guest_path, true)?;
        }
    } else {
        let bytes = kernel
            .read_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, guest_path)
            .map_err(kernel_error)?;
        ensure_mapped_runtime_parent_dirs(mapped, "fs.materialize")?;
        let opened = open_mapped_runtime_beneath(
            mapped,
            "fs.materialize",
            OFlag::O_CREAT | OFlag::O_TRUNC | OFlag::O_WRONLY,
            Mode::from_bits_truncate((stat.mode & 0o7777) as _),
        )?;
        fs::write(opened.handle.proc_path(), bytes).map_err(|error| {
            SidecarError::Io(format!(
                "failed to materialize mapped guest file {} -> {}: {error}",
                guest_path,
                opened.host_path.display()
            ))
        })?;
    }

    let opened =
        open_mapped_runtime_beneath(mapped, "fs.materialize", O_PATH_ANCHOR, Mode::empty())?;
    fs::set_permissions(
        opened.handle.proc_path(),
        fs::Permissions::from_mode(stat.mode & 0o7777),
    )
    .map_err(|error| {
        SidecarError::Io(format!(
            "failed to set permissions for materialized mapped guest path {} -> {}: {error}",
            guest_path,
            opened.host_path.display()
        ))
    })?;

    Ok(())
}

fn open_mapped_host_fd(
    process: &mut ActiveProcess,
    host_path: PathBuf,
    guest_path: Option<String>,
    proc_path: PathBuf,
    flags: u32,
) -> Result<Value, SidecarError> {
    let access_mode = flags & libc::O_ACCMODE as u32;
    let mut options = OpenOptions::new();
    match access_mode {
        x if x == libc::O_WRONLY as u32 => {
            options.write(true);
        }
        x if x == libc::O_RDWR as u32 => {
            options.read(true).write(true);
        }
        _ => {
            options.read(true);
        }
    }
    if flags & libc::O_APPEND as u32 != 0 {
        options.append(true);
    }

    let masked_flags = flags
        & !(libc::O_ACCMODE as u32
            | libc::O_APPEND as u32
            | libc::O_CREAT as u32
            | libc::O_EXCL as u32
            | libc::O_TRUNC as u32);
    options.custom_flags(masked_flags as i32);

    let file = options.open(&proc_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to open mapped guest file {}: {error}",
            host_path.display()
        ))
    })?;
    let fd = process.allocate_mapped_host_fd(crate::state::ActiveMappedHostFd {
        file,
        path: host_path,
        guest_path,
    });
    Ok(json!(fd))
}

fn read_mapped_host_fd(
    mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    length: usize,
    position: Option<u64>,
) -> Result<Value, SidecarError> {
    let mut bytes = vec![0_u8; length];
    let read = match position {
        Some(offset) => mapped.file.read_at(&mut bytes, offset),
        None => mapped.file.read(&mut bytes),
    }
    .map_err(|error| {
        SidecarError::Io(format!(
            "failed to read mapped guest fd {fd} -> {}: {error}",
            mapped.path.display()
        ))
    })?;
    bytes.truncate(read);
    Ok(javascript_sync_rpc_bytes_value(&bytes))
}

fn write_mapped_host_fd(
    mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    contents: &[u8],
    position: Option<u64>,
) -> Result<Value, SidecarError> {
    let written = match position {
        Some(offset) => mapped.file.write_at(contents, offset),
        None => mapped.file.write(contents),
    }
    .map_err(|error| {
        SidecarError::Io(format!(
            "failed to write mapped guest fd {fd} -> {}: {error}",
            mapped.path.display()
        ))
    })?;
    Ok(json!(written))
}

fn write_all_mapped_host_fd(
    mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    contents: &[u8],
    position: Option<u64>,
) -> Result<usize, SidecarError> {
    let mut total = 0usize;
    while total < contents.len() {
        let write_position = position.map(|offset| offset.saturating_add(total as u64));
        let written = match write_position {
            Some(offset) => mapped.file.write_at(&contents[total..], offset),
            None => mapped.file.write(&contents[total..]),
        }
        .map_err(|error| {
            SidecarError::Io(format!(
                "failed to write mapped guest fd {fd} -> {}: {error}",
                mapped.path.display()
            ))
        })?;
        if written == 0 {
            return Err(SidecarError::Execution(format!(
                "EIO: filesystem write made no progress on mapped fd {fd}"
            )));
        }
        total = total.saturating_add(written);
    }
    Ok(total)
}

fn read_le_u32(payload: &[u8], offset: &mut usize, label: &str) -> Result<u32, SidecarError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| SidecarError::InvalidState(format!("filesystem {label} offset overflow")))?;
    let bytes = payload.get(*offset..end).ok_or_else(|| {
        SidecarError::InvalidState(format!("truncated filesystem {label} payload"))
    })?;
    *offset = end;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("slice length checked"),
    ))
}

fn decode_javascript_writev_raw_payload(payload: &[u8]) -> Result<Vec<&[u8]>, SidecarError> {
    let mut offset = 0usize;
    let count = read_le_u32(payload, &mut offset, "writev count")? as usize;
    let mut buffers = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_le_u32(payload, &mut offset, "writev buffer length")? as usize;
        let end = offset.checked_add(len).ok_or_else(|| {
            SidecarError::InvalidState(String::from("filesystem writev payload length overflow"))
        })?;
        let buffer = payload.get(offset..end).ok_or_else(|| {
            SidecarError::InvalidState(String::from("truncated filesystem writev payload"))
        })?;
        buffers.push(buffer);
        offset = end;
    }
    if offset != payload.len() {
        return Err(SidecarError::InvalidState(String::from(
            "filesystem writev payload has trailing bytes",
        )));
    }
    Ok(buffers)
}

fn rename_mapped_host_path(
    source: &str,
    source_host: Option<MappedRuntimeHostAccess>,
    destination: &str,
    destination_host: Option<MappedRuntimeHostAccess>,
) -> Result<Value, SidecarError> {
    match (source_host, destination_host) {
        (
            Some(MappedRuntimeHostAccess::Writable(source_host)),
            Some(MappedRuntimeHostAccess::Writable(destination_host)),
        ) => {
            if normalize_host_path(&source_host.host_root)
                != normalize_host_path(&destination_host.host_root)
            {
                return Err(SidecarError::Kernel(format!(
                    "EXDEV: invalid cross-device link: {source} -> {destination}"
                )));
            }
            let source_parent = open_mapped_runtime_parent_beneath(&source_host, "fs.rename")?;
            let destination_parent =
                open_mapped_runtime_parent_beneath(&destination_host, "fs.rename")?;
            let source_host_path = source_parent.host_path.join(&source_parent.child_name);
            let destination_host_path = destination_parent
                .host_path
                .join(&destination_parent.child_name);
            mapped_child_rename(&source_parent, &destination_parent)
                .map(|()| Value::Null)
                .map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to rename mapped guest path {} -> {} ({} -> {}): {error}",
                        source,
                        destination,
                        source_host_path.display(),
                        destination_host_path.display()
                    ))
                })
        }
        (Some(MappedRuntimeHostAccess::ReadOnly(_)), _) => {
            Err(read_only_mapped_runtime_host_path_error(source))
        }
        (_, Some(MappedRuntimeHostAccess::ReadOnly(_))) => {
            Err(read_only_mapped_runtime_host_path_error(destination))
        }
        _ => Err(SidecarError::Kernel(format!(
            "EXDEV: invalid cross-device link: {source} -> {destination}"
        ))),
    }
}

// On macOS the mapped rename is fd-relative (`renameat`); this path-based
// fallback is only used by the Linux mapped-rename helper.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn rename_mapped_host_path_with_fallback(source: &Path, destination: &Path) -> std::io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            move_mapped_host_path_across_devices(source, destination)
        }
        Err(error) => Err(error),
    }
}

fn move_mapped_host_path_across_devices(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    remove_existing_mapped_host_destination(destination)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        symlink(&target, destination)?;
        fs::remove_file(source)?;
        return Ok(());
    }

    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let source_child = entry.path();
            let destination_child = destination.join(entry.file_name());
            move_mapped_host_path_across_devices(&source_child, &destination_child)?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        fs::remove_dir(source)?;
        return Ok(());
    }

    fs::copy(source, destination)?;
    fs::set_permissions(destination, metadata.permissions())?;
    fs::remove_file(source)?;
    Ok(())
}

fn remove_existing_mapped_host_destination(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            fs::remove_file(path)
        }
        Ok(metadata) if metadata.is_dir() => fs::remove_dir(path),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn mapped_readdir_entry_is_directory(
    mapped_host: &MappedRuntimeHostPath,
    directory: &MappedRuntimeOpenedPath,
    guest_dir_path: &str,
    entry: &fs::DirEntry,
    name: &str,
) -> Option<bool> {
    let file_type = entry.file_type().ok()?;
    if !file_type.is_symlink() {
        return Some(file_type.is_dir());
    }

    let child = MappedRuntimeHostPath {
        guest_path: normalize_path(&format!(
            "{}/{}",
            guest_dir_path.trim_end_matches('/'),
            name
        )),
        host_root: mapped_host.host_root.clone(),
        host_path: directory.host_path.join(entry.file_name()),
    };
    let opened =
        open_mapped_runtime_beneath(&child, "fs.readdir entry", O_PATH_ANCHOR, Mode::empty())
            .ok()?;
    fs::metadata(opened.handle.proc_path())
        .map(|metadata| metadata.is_dir())
        .ok()
}

pub(crate) fn service_javascript_fs_readdir_entries(
    kernel: &mut SidecarKernel,
    process: &ActiveProcess,
    kernel_pid: u32,
    path: &str,
) -> Result<BTreeMap<String, bool>, SidecarError> {
    if let Some(MappedRuntimeHostAccess::Writable(mapped_host)) =
        mapped_runtime_host_path(process, path, false)
    {
        let mut typed: BTreeMap<String, bool> = BTreeMap::new();
        match open_mapped_runtime_beneath(
            &mapped_host,
            "fs.readdir",
            OFlag::O_DIRECTORY | OFlag::O_RDONLY,
            Mode::empty(),
        ) {
            Ok(directory) => {
                let readdir_path = directory.handle.readdir_path().map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to resolve mapped guest directory {} -> {}: {error}",
                        path,
                        directory.host_path.display()
                    ))
                })?;
                for entry in fs::read_dir(readdir_path).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to read mapped guest directory {} -> {}: {error}",
                        path,
                        directory.host_path.display()
                    ))
                })? {
                    let Ok(entry) = entry else { continue };
                    let Ok(name) = entry.file_name().into_string() else {
                        continue;
                    };
                    if let Some(is_dir) = mapped_readdir_entry_is_directory(
                        &mapped_host,
                        &directory,
                        path,
                        &entry,
                        &name,
                    ) {
                        typed.insert(name, is_dir);
                    }
                }
            }
            Err(_)
                if matches!(
                    fs::symlink_metadata(&mapped_host.host_path),
                    Err(ref metadata_error)
                        if metadata_error.kind() == std::io::ErrorKind::NotFound
                ) => {}
            Err(error) => return Err(error),
        }
        match kernel.read_dir_with_types_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path) {
            Ok(entries) => {
                for entry in entries {
                    typed.entry(entry.name).or_insert(entry.is_directory);
                }
            }
            Err(error) if matches!(error.code(), "ENOENT" | "ENOTDIR") => {}
            Err(error) => return Err(kernel_error(error)),
        }
        for name in mapped_runtime_child_mount_basenames(process, path) {
            typed.entry(name).or_insert(true);
        }
        return Ok(typed);
    }

    kernel
        .read_dir_with_types_for_process(EXECUTION_DRIVER_NAME, kernel_pid, path)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.name, entry.is_directory))
                .collect()
        })
        .map_err(kernel_error)
}

pub(crate) fn service_javascript_fs_readdir_raw_sync_rpc(
    kernel: &mut SidecarKernel,
    process: &ActiveProcess,
    kernel_pid: u32,
    request: &JavascriptSyncRpcRequest,
) -> Result<Vec<u8>, SidecarError> {
    let path = javascript_sync_rpc_path_arg(process, &request.args, 0, "filesystem readdir path")?;
    let entries =
        service_javascript_fs_readdir_entries(kernel, process, kernel_pid, path.as_str())?;
    encode_javascript_readdir_raw_payload(entries)
}

fn encode_javascript_readdir_raw_payload(
    entries: BTreeMap<String, bool>,
) -> Result<Vec<u8>, SidecarError> {
    let mut payload = Vec::new();
    for (name, is_dir) in entries
        .into_iter()
        .filter(|(name, _)| name != "." && name != "..")
    {
        let name = name.into_bytes();
        let name_len = u32::try_from(name.len()).map_err(|_| {
            SidecarError::InvalidState(String::from("filesystem readdir entry name too long"))
        })?;
        payload.push(u8::from(is_dir));
        payload.extend_from_slice(&name_len.to_le_bytes());
        payload.extend_from_slice(&name);
    }
    Ok(payload)
}

/// Like `javascript_sync_rpc_readdir_value` but carries each entry's
/// directory-ness as `{name, isDirectory}`. The guest's `normalizeReaddirEntries`
/// consumes these objects directly for `withFileTypes`, avoiding a per-entry stat
/// RPC, and extracts `.name` for the plain string form.
fn javascript_sync_rpc_readdir_typed_value(entries: BTreeMap<String, bool>) -> Value {
    json!(entries
        .into_iter()
        .filter(|(name, _)| name != "." && name != "..")
        .map(|(name, is_dir)| json!({ "name": name, "isDirectory": is_dir }))
        .collect::<Vec<_>>())
}

fn mirror_guest_file_write_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
    bytes: &[u8],
) -> Result<(), SidecarError> {
    let guest_path = normalize_path(guest_path);
    let shadow_path = if guest_path == "/" {
        vm.cwd.clone()
    } else {
        vm.cwd.join(guest_path.trim_start_matches('/'))
    };

    if let Some(parent) = shadow_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for {}: {error}",
                guest_path
            ))
        })?;
    }

    match fs::symlink_metadata(&shadow_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(&shadow_path).map_err(|error| {
                SidecarError::Io(format!(
                    "failed to replace shadow symlink for {}: {error}",
                    guest_path
                ))
            })?;
        }
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(&shadow_path).map_err(|error| {
                SidecarError::Io(format!(
                    "failed to replace shadow directory for {}: {error}",
                    guest_path
                ))
            })?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SidecarError::Io(format!(
                "failed to inspect shadow path for {}: {error}",
                guest_path
            )));
        }
    }
    fs::write(&shadow_path, bytes).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest file {} into shadow root: {error}",
            guest_path
        ))
    })?;

    let stat = vm.kernel.lstat(&guest_path).map_err(kernel_error)?;
    fs::set_permissions(&shadow_path, fs::Permissions::from_mode(stat.mode & 0o7777)).map_err(
        |error| {
            SidecarError::Io(format!(
                "failed to set shadow mode for {}: {error}",
                guest_path
            ))
        },
    )?;

    Ok(())
}

fn mirror_guest_directory_write_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
) -> Result<(), SidecarError> {
    let guest_path = normalize_path(guest_path);
    let shadow_path = shadow_host_path_for_guest(&vm.cwd, &guest_path);

    fs::create_dir_all(&shadow_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest directory {} into shadow root: {error}",
            guest_path
        ))
    })?;

    let stat = vm.kernel.lstat(&guest_path).map_err(kernel_error)?;
    fs::set_permissions(&shadow_path, fs::Permissions::from_mode(stat.mode & 0o7777)).map_err(
        |error| {
            SidecarError::Io(format!(
                "failed to set shadow mode for directory {}: {error}",
                guest_path
            ))
        },
    )?;

    Ok(())
}

fn ensure_guest_path_materialized_in_shadow(
    vm: &mut VmState,
    guest_path: &str,
) -> Result<PathBuf, SidecarError> {
    let guest_path = normalize_path(guest_path);
    let shadow_path = shadow_host_path_for_guest(&vm.cwd, &guest_path);
    if fs::symlink_metadata(&shadow_path).is_ok() {
        return Ok(shadow_path);
    }

    let stat = vm.kernel.lstat(&guest_path).map_err(kernel_error)?;
    if stat.is_symbolic_link {
        let target = vm.kernel.read_link(&guest_path).map_err(kernel_error)?;
        mirror_guest_symlink_to_shadow(vm, &guest_path, &target)?;
    } else if stat.is_directory {
        mirror_guest_directory_write_to_shadow(vm, &guest_path)?;
    } else {
        let bytes = vm.kernel.read_file(&guest_path).map_err(kernel_error)?;
        mirror_guest_file_write_to_shadow(vm, &guest_path, &bytes)?;
    }

    Ok(shadow_path)
}

fn mirror_guest_subtree_to_shadow(vm: &mut VmState, guest_path: &str) -> Result<(), SidecarError> {
    let guest_path = normalize_path(guest_path);
    ensure_guest_path_materialized_in_shadow(vm, &guest_path)?;
    let stat = vm.kernel.lstat(&guest_path).map_err(kernel_error)?;
    if !stat.is_directory || stat.is_symbolic_link {
        return Ok(());
    }

    let entries = vm
        .kernel
        .read_dir_recursive(&guest_path, None)
        .map_err(kernel_error)?;
    for entry in entries {
        ensure_guest_path_materialized_in_shadow(vm, &entry.path)?;
    }
    Ok(())
}

fn mirror_guest_symlink_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
    target: &str,
) -> Result<(), SidecarError> {
    let guest_path = normalize_path(guest_path);
    let shadow_path = shadow_host_path_for_guest(&vm.cwd, &guest_path);
    let shadow_target = shadow_symlink_target_for_guest(&vm.cwd, &guest_path, target);

    if let Some(parent) = shadow_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for symlink {}: {error}",
                guest_path
            ))
        })?;
    }

    remove_shadow_path_if_exists(&shadow_path, &guest_path)?;
    symlink(&shadow_target, &shadow_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest symlink {} into shadow root: {error}",
            guest_path
        ))
    })
}

fn mirror_guest_link_to_shadow(
    vm: &mut VmState,
    source_path: &str,
    destination_path: &str,
) -> Result<(), SidecarError> {
    let source_path = normalize_path(source_path);
    let destination_path = normalize_path(destination_path);
    let source_shadow_path = ensure_guest_path_materialized_in_shadow(vm, &source_path)?;
    let destination_shadow_path = shadow_host_path_for_guest(&vm.cwd, &destination_path);

    if let Some(parent) = destination_shadow_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for link {}: {error}",
                destination_path
            ))
        })?;
    }

    remove_shadow_path_if_exists(&destination_shadow_path, &destination_path)?;
    fs::hard_link(&source_shadow_path, &destination_shadow_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest link {} -> {} into shadow root: {error}",
            source_path, destination_path
        ))
    })
}

fn mirror_guest_chmod_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
    mode: u32,
) -> Result<(), SidecarError> {
    let shadow_path = ensure_guest_path_materialized_in_shadow(vm, guest_path)?;
    fs::set_permissions(&shadow_path, fs::Permissions::from_mode(mode & 0o7777)).map_err(|error| {
        SidecarError::Io(format!(
            "failed to set shadow mode for {}: {error}",
            normalize_path(guest_path)
        ))
    })
}

fn mirror_guest_utimes_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
    atime: VirtualUtimeSpec,
    mtime: VirtualUtimeSpec,
    follow_symlinks: bool,
) -> Result<(), SidecarError> {
    let shadow_path = ensure_guest_path_materialized_in_shadow(vm, guest_path)?;
    apply_host_path_utimens(
        &shadow_path,
        atime,
        mtime,
        follow_symlinks,
        &format!(
            "failed to mirror guest utimes for {} into shadow root",
            normalize_path(guest_path)
        ),
    )
}

fn mirror_guest_truncate_to_shadow(
    vm: &mut VmState,
    guest_path: &str,
    len: u64,
) -> Result<(), SidecarError> {
    let shadow_path = ensure_guest_path_materialized_in_shadow(vm, guest_path)?;
    OpenOptions::new()
        .write(true)
        .open(&shadow_path)
        .and_then(|file| file.set_len(len))
        .map_err(|error| {
            SidecarError::Io(format!(
                "failed to mirror guest truncate for {} into shadow root: {error}",
                normalize_path(guest_path)
            ))
        })
}

fn remove_guest_shadow_path(vm: &mut VmState, guest_path: &str) -> Result<(), SidecarError> {
    let guest_path = normalize_path(guest_path);
    let shadow_path = shadow_host_path_for_guest(&vm.cwd, &guest_path);
    remove_shadow_path_if_exists(&shadow_path, &guest_path)
}

fn rename_guest_shadow_path(
    vm: &mut VmState,
    from_path: &str,
    to_path: &str,
) -> Result<(), SidecarError> {
    let from_path = normalize_path(from_path);
    let to_path = normalize_path(to_path);
    let from_shadow_path = shadow_host_path_for_guest(&vm.cwd, &from_path);
    let to_shadow_path = shadow_host_path_for_guest(&vm.cwd, &to_path);

    if !from_shadow_path.exists() {
        remove_shadow_path_if_exists(&to_shadow_path, &to_path)?;
        return Ok(());
    }

    if let Some(parent) = to_shadow_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for rename {} -> {}: {error}",
                from_path, to_path
            ))
        })?;
    }

    remove_shadow_path_if_exists(&to_shadow_path, &to_path)?;
    fs::rename(&from_shadow_path, &to_shadow_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest rename {} -> {} into shadow root: {error}",
            from_path, to_path
        ))
    })?;

    Ok(())
}

fn remove_shadow_path_if_exists(shadow_path: &Path, guest_path: &str) -> Result<(), SidecarError> {
    match fs::symlink_metadata(shadow_path) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(shadow_path).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to remove shadow directory for {}: {error}",
                        guest_path
                    ))
                })?;
            } else {
                fs::remove_file(shadow_path).map_err(|error| {
                    SidecarError::Io(format!(
                        "failed to remove shadow path for {}: {error}",
                        guest_path
                    ))
                })?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SidecarError::Io(format!(
            "failed to inspect shadow path for {}: {error}",
            guest_path
        ))),
    }
}

fn sync_active_shadow_path_to_kernel(
    vm: &mut VmState,
    guest_path: &str,
) -> Result<(), SidecarError> {
    sync_active_process_host_writes_to_kernel(vm)?;
    let guest_path = normalize_path(guest_path);
    if is_protected_agentos_shadow_sync_path(&guest_path) {
        return Ok(());
    }
    let mut host_paths = active_process_shadow_host_paths_for_guest(vm, &guest_path);
    if host_paths.is_empty() && !vm.kernel.exists(&guest_path).unwrap_or(false) {
        host_paths.push(shadow_host_path_for_guest(&vm.cwd, &guest_path));
    }

    for host_path in host_paths {
        let metadata = match fs::symlink_metadata(&host_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(SidecarError::Io(format!(
                    "failed to stat host shadow path {}: {error}",
                    host_path.display()
                )));
            }
        };

        if metadata.file_type().is_symlink() {
            sync_host_symlink_to_kernel(vm, &guest_path, &host_path)?;
            return Ok(());
        }

        if metadata.is_dir() {
            sync_host_directory_to_kernel(vm, &guest_path, &metadata)?;
            return Ok(());
        }

        if metadata.is_file() {
            sync_host_file_to_kernel(vm, &guest_path, &host_path, &metadata)?;
            return Ok(());
        }
    }

    Ok(())
}

fn active_process_shadow_host_paths_for_guest(vm: &VmState, guest_path: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();

    for process in vm.active_processes.values() {
        if let Some(host_path) = resolve_process_guest_path_to_host(process, guest_path) {
            push_unique_host_path(&mut candidates, &mut seen, host_path);
        }
    }

    candidates
}

fn push_unique_host_path(
    candidates: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<PathBuf>,
    host_path: PathBuf,
) {
    if seen.insert(host_path.clone()) {
        candidates.push(host_path);
    }
}

fn shadow_host_path_for_guest(shadow_root: &Path, guest_path: &str) -> PathBuf {
    if guest_path == "/" {
        shadow_root.to_path_buf()
    } else {
        shadow_root.join(guest_path.trim_start_matches('/'))
    }
}

fn shadow_symlink_target_for_guest(shadow_root: &Path, guest_path: &str, target: &str) -> PathBuf {
    if !target.starts_with('/') {
        return PathBuf::from(target);
    }

    let link_shadow_path = shadow_host_path_for_guest(shadow_root, guest_path);
    let link_parent = link_shadow_path.parent().unwrap_or(shadow_root);
    let target_shadow_path = shadow_host_path_for_guest(shadow_root, target);
    relative_path_from(link_parent, &target_shadow_path)
}

fn relative_path_from(base_dir: &Path, target: &Path) -> PathBuf {
    let base_components: Vec<_> = base_dir.components().collect();
    let target_components: Vec<_> = target.components().collect();

    let mut shared_prefix = 0;
    while shared_prefix < base_components.len()
        && shared_prefix < target_components.len()
        && base_components[shared_prefix] == target_components[shared_prefix]
    {
        shared_prefix += 1;
    }

    let mut relative = PathBuf::new();
    for _ in shared_prefix..base_components.len() {
        relative.push("..");
    }
    for component in target_components.iter().skip(shared_prefix) {
        relative.push(component.as_os_str());
    }

    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn resolve_process_guest_path_to_host(
    process: &ActiveProcess,
    guest_path: &str,
) -> Option<PathBuf> {
    let normalized_guest_path = if guest_path.starts_with('/') {
        normalize_path(guest_path)
    } else {
        normalize_path(&format!(
            "{}/{}",
            process.guest_cwd.trim_end_matches('/'),
            guest_path
        ))
    };
    if let Some(host_path) =
        host_path_from_runtime_guest_mappings(&process.env, &normalized_guest_path)
    {
        return Some(host_path);
    }
    let normalized_guest_cwd = normalize_path(&process.guest_cwd);
    let mut host_root = process.host_cwd.clone();
    for _ in normalized_guest_cwd
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        host_root = host_root.parent()?.to_path_buf();
    }
    Some(shadow_host_path_for_guest(
        &host_root,
        &normalized_guest_path,
    ))
}

/// Removes the host shadow copy of `guest_path` after a kernel-direct guest
/// deletion so the exit-time shadow->kernel sync cannot resurrect it.
fn remove_process_shadow_path(
    process: &ActiveProcess,
    guest_path: &str,
) -> Result<(), SidecarError> {
    let Some(shadow_path) = resolve_process_guest_path_to_host(process, guest_path) else {
        return Ok(());
    };
    remove_shadow_path_if_exists(&shadow_path, guest_path)
}

/// Mirrors a kernel-direct guest rename into the host shadow tree. If the
/// source shadow entry is missing the stale destination copy is still removed
/// so the shadow walk cannot resurrect pre-rename content.
fn rename_process_shadow_path(
    process: &ActiveProcess,
    source: &str,
    destination: &str,
) -> Result<(), SidecarError> {
    let Some(source_shadow) = resolve_process_guest_path_to_host(process, source) else {
        return Ok(());
    };
    let Some(destination_shadow) = resolve_process_guest_path_to_host(process, destination) else {
        return Ok(());
    };

    if fs::symlink_metadata(&source_shadow).is_err() {
        return remove_shadow_path_if_exists(&destination_shadow, destination);
    }

    if let Some(parent) = destination_shadow.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SidecarError::Io(format!(
                "failed to create shadow parent for rename {source} -> {destination}: {error}"
            ))
        })?;
    }
    remove_shadow_path_if_exists(&destination_shadow, destination)?;
    fs::rename(&source_shadow, &destination_shadow).map_err(|error| {
        SidecarError::Io(format!(
            "failed to mirror guest rename {source} -> {destination} into shadow root: {error}"
        ))
    })
}

fn sync_host_directory_to_kernel(
    vm: &mut VmState,
    guest_path: &str,
    metadata: &fs::Metadata,
) -> Result<(), SidecarError> {
    vm.kernel.mkdir(guest_path, true).map_err(kernel_error)?;
    vm.kernel
        .chmod(guest_path, metadata.permissions().mode() & 0o7777)
        .map_err(kernel_error)?;
    Ok(())
}

fn sync_host_file_to_kernel(
    vm: &mut VmState,
    guest_path: &str,
    host_path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), SidecarError> {
    ensure_guest_parent_dir(vm, guest_path)?;
    let bytes = fs::read(host_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to read host shadow file {}: {error}",
            host_path.display()
        ))
    })?;
    vm.kernel
        .write_file(guest_path, bytes)
        .map_err(kernel_error)?;
    vm.kernel
        .chmod(guest_path, metadata.permissions().mode() & 0o7777)
        .map_err(kernel_error)?;
    Ok(())
}

fn sync_host_symlink_to_kernel(
    vm: &mut VmState,
    guest_path: &str,
    host_path: &Path,
) -> Result<(), SidecarError> {
    ensure_guest_parent_dir(vm, guest_path)?;
    let target = fs::read_link(host_path).map_err(|error| {
        SidecarError::Io(format!(
            "failed to read host shadow symlink {}: {error}",
            host_path.display()
        ))
    })?;

    let target = restore_guest_symlink_target_from_shadow(vm, guest_path, host_path, &target)
        .unwrap_or_else(|| target.to_string_lossy().into_owned());

    replace_guest_symlink(vm, guest_path, &target)
}

fn restore_guest_symlink_target_from_shadow(
    vm: &VmState,
    guest_path: &str,
    host_path: &Path,
    shadow_target: &Path,
) -> Option<String> {
    if shadow_target.is_absolute() {
        return None;
    }

    let existing_target = vm.kernel.read_link(guest_path).ok()?;
    if !existing_target.starts_with('/') {
        return None;
    }

    let host_parent = host_path.parent().unwrap_or(&vm.cwd);
    let resolved_host_target = normalize_host_path(&host_parent.join(shadow_target));
    let normalized_shadow_root = normalize_host_path(&vm.cwd);
    if resolved_host_target == normalized_shadow_root {
        return Some(String::from("/"));
    }

    resolved_host_target
        .strip_prefix(&normalized_shadow_root)
        .ok()
        .map(|suffix| format!("/{}", suffix.to_string_lossy().trim_start_matches('/')))
}

fn replace_guest_symlink(
    vm: &mut VmState,
    guest_path: &str,
    target: &str,
) -> Result<(), SidecarError> {
    if vm.kernel.symlink(target, guest_path).is_ok() {
        return Ok(());
    }

    if let Ok(existing_target) = vm.kernel.read_link(guest_path) {
        if existing_target == target {
            return Ok(());
        }
    }

    let _ = vm.kernel.remove_file(guest_path);
    let _ = vm.kernel.remove_dir(guest_path);
    vm.kernel
        .symlink(target, guest_path)
        .map_err(kernel_error)?;
    Ok(())
}

fn ensure_guest_parent_dir(vm: &mut VmState, guest_path: &str) -> Result<(), SidecarError> {
    let Some(parent) = Path::new(guest_path).parent() else {
        return Ok(());
    };
    let parent = parent.to_string_lossy();
    if parent.is_empty() || parent == "/" {
        return Ok(());
    }
    vm.kernel
        .mkdir(&normalize_path(&parent), true)
        .map_err(kernel_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        create_mapped_runtime_directory, create_mapped_runtime_root_directory,
        mapped_runtime_relative_path, mapped_runtime_symlink_metadata,
        materialize_mapped_host_path_from_kernel, open_mapped_runtime_parent_beneath,
        read_mapped_runtime_link, rename_mapped_host_path, MappedRuntimeHostAccess,
        MappedRuntimeHostPath, SidecarError,
    };
    use crate::execution::javascript_sync_rpc_error_code;
    use crate::state::{SidecarKernel, EXECUTION_DRIVER_NAME, JAVASCRIPT_COMMAND};
    use agentos_kernel::command_registry::CommandDriver;
    use agentos_kernel::kernel::{KernelVmConfig, SpawnOptions};
    use agentos_kernel::mount_table::MountTable;
    use agentos_kernel::permissions::Permissions;
    use agentos_kernel::vfs::MemoryFileSystem;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn writable_mapping(guest_path: &str, host_root: &str) -> MappedRuntimeHostAccess {
        let host_root = PathBuf::from(host_root);
        MappedRuntimeHostAccess::Writable(MappedRuntimeHostPath {
            guest_path: guest_path.to_owned(),
            host_path: host_root.join("file.txt"),
            host_root: host_root.clone(),
        })
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn test_kernel_with_process() -> (SidecarKernel, u32) {
        let mut config = KernelVmConfig::new("vm-mapped-materialize");
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        kernel
            .register_driver(CommandDriver::new(
                EXECUTION_DRIVER_NAME,
                [JAVASCRIPT_COMMAND],
            ))
            .expect("register execution driver");
        let handle = kernel
            .spawn_process(
                JAVASCRIPT_COMMAND,
                Vec::new(),
                SpawnOptions {
                    requester_driver: Some(String::from(EXECUTION_DRIVER_NAME)),
                    cwd: Some(String::from("/")),
                    ..SpawnOptions::default()
                },
            )
            .expect("spawn kernel process");
        (kernel, handle.pid())
    }

    #[test]
    fn rename_mapped_host_path_reports_exdev_for_cross_mount_guest_errno() {
        for (source_host, destination_host) in [
            (
                Some(writable_mapping(
                    "/mapped/file.txt",
                    "/tmp/secure-exec-mapped-source",
                )),
                None,
            ),
            (
                None,
                Some(writable_mapping(
                    "/mapped-dst/file.txt",
                    "/tmp/secure-exec-mapped-destination",
                )),
            ),
        ] {
            let error = rename_mapped_host_path(
                "/mapped/file.txt",
                source_host,
                "/kernel/file.txt",
                destination_host,
            )
            .expect_err("cross-mount rename should fail with EXDEV");
            assert!(
                matches!(error, SidecarError::Kernel(ref message) if message.starts_with("EXDEV:")),
                "expected EXDEV kernel error, got {error:?}"
            );
            assert_eq!(javascript_sync_rpc_error_code(&error), "EXDEV");
        }
    }

    #[test]
    fn mapped_runtime_parent_treats_single_segment_relative_paths_as_root_children() {
        let host_root = std::env::temp_dir().join(format!(
            "agentos-native-sidecar-fs-parent-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&host_root).expect("create mapped host root");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/workspace"),
            host_root: host_root.clone(),
            host_path: host_root.join("workspace"),
        };

        assert_eq!(
            mapped_runtime_relative_path(&mapped).expect("relative path"),
            PathBuf::from("workspace")
        );

        let parent = open_mapped_runtime_parent_beneath(&mapped, "test")
            .expect("open mapped parent for root child");
        // `host_path` is the resolved fd's real path, which is canonical (on
        // macOS the temp dir resolves through the `/private` firmlink), so
        // compare against the canonicalized root rather than the raw value.
        assert_eq!(
            parent.host_path,
            fs::canonicalize(&host_root).expect("canonicalize host root")
        );
        assert_eq!(parent.child_name.to_string_lossy(), "workspace");
    }

    #[test]
    fn mapped_runtime_root_lstat_uses_root_metadata_without_parent_basename() {
        let host_root = std::env::temp_dir().join(format!(
            "agentos-native-sidecar-fs-root-lstat-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&host_root).expect("create mapped host root");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/node_modules"),
            host_root: host_root.clone(),
            host_path: host_root.clone(),
        };

        let metadata = mapped_runtime_symlink_metadata(&mapped, "test").expect("lstat mapped root");
        assert!(metadata.is_dir(), "expected mapped root directory metadata");

        fs::remove_dir_all(&host_root).expect("remove mapped host root");
    }

    #[test]
    fn mapped_runtime_root_readlink_uses_root_path_without_parent_basename() {
        let host_parent = std::env::temp_dir().join(format!(
            "agentos-native-sidecar-fs-root-readlink-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        let host_target = host_parent.join("target");
        let host_link = host_parent.join("link");
        fs::create_dir_all(&host_target).expect("create mapped host target");
        std::os::unix::fs::symlink(&host_target, &host_link).expect("create mapped host link");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/"),
            host_root: host_link.clone(),
            host_path: host_link,
        };

        let target = read_mapped_runtime_link(&mapped, "/", "test").expect("read mapped root link");
        assert_eq!(target, host_target);

        fs::remove_dir_all(&host_parent).expect("remove mapped host parent");
    }

    #[test]
    fn recursive_mapped_directory_create_accepts_existing_directory() {
        let host_root = std::env::temp_dir().join(format!(
            "agentos-native-sidecar-fs-existing-dir-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        let existing_dir = host_root.join("workspace");
        fs::create_dir_all(&existing_dir).expect("create existing mapped directory");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/workspace"),
            host_root: host_root.clone(),
            host_path: existing_dir,
        };

        let parent = open_mapped_runtime_parent_beneath(&mapped, "test")
            .expect("open mapped parent for root child");
        create_mapped_runtime_directory(&parent, "/workspace", true)
            .expect("recursive mkdir should accept an existing directory");
        let non_recursive_error = create_mapped_runtime_directory(&parent, "/workspace", false)
            .expect_err("non-recursive mkdir should keep EEXIST behavior");
        assert!(
            matches!(non_recursive_error, SidecarError::Io(ref message) if message.contains("File exists")),
            "expected File exists error, got {non_recursive_error:?}"
        );

        fs::remove_dir_all(&host_root).expect("remove mapped host root");
    }

    #[test]
    fn recursive_mapped_root_directory_create_accepts_existing_directory() {
        let host_root = std::env::temp_dir().join(format!(
            "agentos-native-sidecar-fs-existing-root-dir-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&host_root).expect("create mapped host root");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/"),
            host_root: host_root.clone(),
            host_path: host_root.clone(),
        };

        create_mapped_runtime_root_directory(&mapped, true)
            .expect("recursive root mkdir should accept an existing directory");
        let non_recursive_error = create_mapped_runtime_root_directory(&mapped, false)
            .expect_err("non-recursive root mkdir should keep EEXIST behavior");
        assert!(
            matches!(non_recursive_error, SidecarError::Io(ref message) if message.contains("File exists")),
            "expected File exists error, got {non_recursive_error:?}"
        );

        fs::remove_dir_all(&host_root).expect("remove mapped host root");
    }

    #[test]
    fn materialize_mapped_host_path_does_not_follow_symlinked_parents() {
        let host_root = temp_dir("agentos-native-sidecar-fs-materialize-root");
        let outside = temp_dir("agentos-native-sidecar-fs-materialize-outside");
        std::os::unix::fs::symlink(&outside, host_root.join("link"))
            .expect("create escape symlink");

        let (mut kernel, pid) = test_kernel_with_process();
        kernel
            .write_file_for_process(
                EXECUTION_DRIVER_NAME,
                pid,
                "/workspace/link/out.txt",
                b"secret".to_vec(),
                Some(0o644),
            )
            .expect("seed guest file");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/workspace/link/out.txt"),
            host_root: host_root.clone(),
            host_path: host_root.join("link/out.txt"),
        };

        materialize_mapped_host_path_from_kernel(
            &mut kernel,
            pid,
            "/workspace/link/out.txt",
            &mapped,
        )
        .expect_err("symlinked parent must not be followed during materialization");

        assert!(
            !outside.join("out.txt").exists(),
            "materialization wrote through a symlinked mapped parent"
        );

        fs::remove_dir_all(&host_root).expect("remove mapped host root");
        fs::remove_dir_all(&outside).expect("remove outside dir");
    }

    #[test]
    fn materialize_mapped_host_path_writes_regular_files_beneath_root() {
        let host_root = temp_dir("agentos-native-sidecar-fs-materialize-file");
        let (mut kernel, pid) = test_kernel_with_process();
        kernel
            .write_file_for_process(
                EXECUTION_DRIVER_NAME,
                pid,
                "/workspace/out.txt",
                b"secret".to_vec(),
                Some(0o640),
            )
            .expect("seed guest file");
        let mapped = MappedRuntimeHostPath {
            guest_path: String::from("/workspace/out.txt"),
            host_root: host_root.clone(),
            host_path: host_root.join("out.txt"),
        };

        materialize_mapped_host_path_from_kernel(&mut kernel, pid, "/workspace/out.txt", &mapped)
            .expect("materialize regular mapped file");

        let host_path = host_root.join("out.txt");
        assert_eq!(
            fs::read(&host_path).expect("read materialized file"),
            b"secret"
        );
        assert_eq!(
            fs::metadata(&host_path)
                .expect("materialized metadata")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );

        fs::remove_dir_all(&host_root).expect("remove mapped host root");
    }

    // Companion to the execution-crate `faithful_pnpm_symlink_layout_*` host
    // test, but resolving through the *kernel VFS* via a read-only `host_dir`
    // mount at `/root/node_modules` — the real VM path. A faithful pnpm tree
    // (every package in its own `.pnpm/<pkg>@<ver>/node_modules/<pkg>` entry,
    // dependencies wired by symlink) must resolve purely by the standard
    // ancestor walk + realpath, with NO `.pnpm` store scanning, and must pick
    // the version the symlink points at — not an alphabetically-earlier decoy.
    #[test]
    fn faithful_pnpm_symlink_layout_resolves_through_kernel_vfs() {
        use super::{KernelModuleFsReader, ModuleResolveMode};
        use agentos_execution::{LocalModuleResolutionCache, ModuleResolver};
        use agentos_kernel::mount_table::{MountOptions, MountedVirtualFileSystem};
        use std::os::unix::fs::symlink;

        let node_modules = temp_dir("pnpm-vfs-node-modules").join("node_modules");
        let write = |relative: &str, contents: &str| {
            let path = node_modules.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
            fs::write(path, contents).expect("write fixture");
        };
        // pnpm always writes *relative* symlinks; the VFS mount follows them
        // with RESOLVE_BENEATH (absolute targets are treated as escaping, which
        // is also why pnpm never uses them). `relative_target` is the target
        // expressed relative to the link's own directory.
        let link = |relative_target: &str, link_relative: &str| {
            let link_path = node_modules.join(link_relative);
            fs::create_dir_all(link_path.parent().expect("link parent")).expect("create dirs");
            symlink(relative_target, link_path).expect("create symlink");
        };

        // consumer@1.0.0 in its store entry; imports `dep`.
        write(
            ".pnpm/consumer@1.0.0/node_modules/consumer/index.mjs",
            "import { wanted } from 'dep';\nexport default wanted;",
        );
        write(
            ".pnpm/consumer@1.0.0/node_modules/consumer/package.json",
            r#"{ "version": "1.0.0", "type": "module", "exports": { ".": "./index.mjs" } }"#,
        );
        // dep@2.0.0 — the correct version — in its own store entry.
        write(
            ".pnpm/dep@2.0.0/node_modules/dep/index.mjs",
            "export const wanted = 2;",
        );
        write(
            ".pnpm/dep@2.0.0/node_modules/dep/package.json",
            r#"{ "version": "2.0.0", "type": "module", "exports": { ".": "./index.mjs" } }"#,
        );
        // Decoy: an alphabetically-earlier store entry holding an incompatible dep@1.
        write(
            ".pnpm/aaa-other@1.0.0/node_modules/dep/index.js",
            "module.exports = 1;",
        );
        write(
            ".pnpm/aaa-other@1.0.0/node_modules/dep/package.json",
            r#"{ "version": "1.0.0", "main": "index.js" }"#,
        );
        // pnpm's sibling symlink: consumer's `dep` -> dep@2.0.0's store entry,
        // expressed relative to `.pnpm/consumer@1.0.0/node_modules/`.
        link(
            "../../dep@2.0.0/node_modules/dep",
            ".pnpm/consumer@1.0.0/node_modules/dep",
        );
        // Top-level symlink: node_modules/consumer -> consumer's store entry,
        // expressed relative to `node_modules/`.
        link(".pnpm/consumer@1.0.0/node_modules/consumer", "consumer");

        // Mount the tree read-only at /root/node_modules, exactly like the live VM.
        let mut config = KernelVmConfig::new("vm-pnpm-vfs");
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        let host_dir = crate::plugins::host_dir::HostDirFilesystem::new(&node_modules)
            .expect("create host_dir over node_modules");
        kernel
            .mount_boxed_filesystem(
                "/root/node_modules",
                Box::new(MountedVirtualFileSystem::new(host_dir)),
                MountOptions::new("host_dir").read_only(true),
            )
            .expect("mount node_modules read-only");

        let mut cache = LocalModuleResolutionCache::default();
        let mut resolver = ModuleResolver::new(
            KernelModuleFsReader {
                kernel: &mut kernel,
            },
            &mut cache,
        );

        // Importer is the top-level symlink path. The ancestor walk finds `dep`
        // via pnpm's sibling symlink in consumer's store dir (pointing at
        // dep@2.0.0) — no `.pnpm` scan. Resolution reads entirely through the VFS.
        let resolved = resolver.resolve_module(
            "dep",
            "/root/node_modules/consumer/index.mjs",
            ModuleResolveMode::Import,
        );
        assert_eq!(
            resolved.as_deref(),
            Some("/root/node_modules/.pnpm/consumer@1.0.0/node_modules/dep/index.mjs"),
            "must resolve dep@2.0.0 via the sibling symlink, not the aaa-other decoy",
        );

        // And the resolved source loads through the VFS too.
        let source = resolver
            .load_file("/root/node_modules/.pnpm/consumer@1.0.0/node_modules/dep/index.mjs")
            .expect("load resolved dep source via kernel VFS");
        assert_eq!(source, "export const wanted = 2;");

        fs::remove_dir_all(node_modules.parent().expect("temp parent")).expect("remove temp tree");
    }

    // Companion to the kernel-VFS test above, but resolving through the
    // `HostDirModuleReader` — the bridge-thread reader the live VM uses so module
    // resolution runs concurrently with the service loop instead of serializing
    // behind it. It reads the SAME read-only `host_dir` mount (anchored openat2,
    // escaping-symlink refusal) and must resolve the identical pnpm layout to the
    // identical guest path, with no `.pnpm` scanning and the symlink-pointed
    // version winning over the decoy.
    #[test]
    fn faithful_pnpm_symlink_layout_resolves_through_host_dir_module_reader() {
        use crate::plugins::host_dir::HostDirModuleReader;
        use agentos_execution::{LocalModuleResolutionCache, ModuleResolveMode, ModuleResolver};
        use std::os::unix::fs::symlink;

        let node_modules = temp_dir("pnpm-reader-node-modules").join("node_modules");
        let write = |relative: &str, contents: &str| {
            let path = node_modules.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
            fs::write(path, contents).expect("write fixture");
        };
        let link = |relative_target: &str, link_relative: &str| {
            let link_path = node_modules.join(link_relative);
            fs::create_dir_all(link_path.parent().expect("link parent")).expect("create dirs");
            symlink(relative_target, link_path).expect("create symlink");
        };

        write(
            ".pnpm/consumer@1.0.0/node_modules/consumer/index.mjs",
            "import { wanted } from 'dep';\nexport default wanted;",
        );
        write(
            ".pnpm/consumer@1.0.0/node_modules/consumer/package.json",
            r#"{ "version": "1.0.0", "type": "module", "exports": { ".": "./index.mjs" } }"#,
        );
        write(
            ".pnpm/dep@2.0.0/node_modules/dep/index.mjs",
            "export const wanted = 2;",
        );
        write(
            ".pnpm/dep@2.0.0/node_modules/dep/package.json",
            r#"{ "version": "2.0.0", "type": "module", "exports": { ".": "./index.mjs" } }"#,
        );
        write(
            ".pnpm/aaa-other@1.0.0/node_modules/dep/index.js",
            "module.exports = 1;",
        );
        write(
            ".pnpm/aaa-other@1.0.0/node_modules/dep/package.json",
            r#"{ "version": "1.0.0", "main": "index.js" }"#,
        );
        link(
            "../../dep@2.0.0/node_modules/dep",
            ".pnpm/consumer@1.0.0/node_modules/dep",
        );
        link(".pnpm/consumer@1.0.0/node_modules/consumer", "consumer");

        // The reader is anchored at the node_modules host root, mounted at the
        // guest convention `/root/node_modules` — exactly what build_module_reader
        // derives for the live VM.
        let reader = HostDirModuleReader::from_mounts([("/root/node_modules", &node_modules)])
            .expect("build host_dir module reader");
        let mut cache = LocalModuleResolutionCache::default();
        let mut resolver = ModuleResolver::new(reader, &mut cache);

        let resolved = resolver.resolve_module(
            "dep",
            "/root/node_modules/consumer/index.mjs",
            ModuleResolveMode::Import,
        );
        assert_eq!(
            resolved.as_deref(),
            Some("/root/node_modules/.pnpm/consumer@1.0.0/node_modules/dep/index.mjs"),
            "reader must resolve dep@2.0.0 via the sibling symlink, not the aaa-other decoy",
        );

        let source = resolver
            .load_file("/root/node_modules/.pnpm/consumer@1.0.0/node_modules/dep/index.mjs")
            .expect("load resolved dep source via host_dir reader");
        assert_eq!(source, "export const wanted = 2;");

        // Escaping-symlink refusal is preserved by the mount: a link pointing
        // outside the node_modules root must not read through it.
        let outside = temp_dir("pnpm-reader-outside");
        fs::create_dir_all(&outside).expect("create outside dir");
        fs::write(outside.join("escaped.js"), "module.exports = 'escaped';")
            .expect("write escape target");
        symlink(&outside, node_modules.join("escape-link")).expect("create escaping symlink");
        let escape_reader =
            HostDirModuleReader::from_mounts([("/root/node_modules", &node_modules)])
                .expect("build host_dir module reader");
        let mut escape_cache = LocalModuleResolutionCache::default();
        let mut escape_resolver = ModuleResolver::new(escape_reader, &mut escape_cache);
        let escaped = escape_resolver.load_file("/root/node_modules/escape-link/escaped.js");
        assert!(
            escaped.is_none(),
            "escaping symlink must not read through the mount",
        );

        fs::remove_dir_all(node_modules.parent().expect("temp parent")).expect("remove temp tree");
        fs::remove_dir_all(&outside).ok();
    }

    // Phase 0 perf gate: compare cold-start module resolution cost of the new
    // kernel-VFS path against the legacy host-direct path over a representative
    // node_modules closure. Run with:
    //   cargo test -p agentos-native-sidecar --lib module_resolution_vfs_vs_host_cold_start_perf -- --nocapture --ignored
    #[test]
    #[ignore = "perf microbenchmark; run explicitly with --ignored --nocapture"]
    fn module_resolution_vfs_vs_host_cold_start_perf() {
        use super::KernelModuleFsReader;
        use agentos_execution::javascript::ModuleResolutionTestHarness;
        use agentos_execution::{LocalModuleResolutionCache, ModuleResolveMode, ModuleResolver};
        use agentos_kernel::mount_table::{MountOptions, MountedVirtualFileSystem};
        use std::time::Instant;

        // Build a representative closure: a root entry that imports N packages,
        // each a scoped/unscoped package with its own package.json + nested dep.
        const PACKAGES: usize = 40;
        let root = temp_dir("perf-closure");
        let write = |relative: &str, contents: &str| {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
            fs::write(path, contents).expect("write");
        };

        let mut imports = Vec::new();
        for i in 0..PACKAGES {
            let pkg = format!("pkg{i}");
            write(
                &format!("node_modules/{pkg}/package.json"),
                &format!(r#"{{ "name": "{pkg}", "version": "1.0.0", "main": "lib/index.js" }}"#),
            );
            write(
                &format!("node_modules/{pkg}/lib/index.js"),
                "module.exports = require('./helper');",
            );
            write(
                &format!("node_modules/{pkg}/lib/helper.js"),
                "module.exports = 1;",
            );
            // a nested transitive dependency
            write(
                &format!("node_modules/{pkg}/node_modules/dep{i}/package.json"),
                &format!(r#"{{ "name": "dep{i}", "version": "1.0.0" }}"#),
            );
            write(
                &format!("node_modules/{pkg}/node_modules/dep{i}/index.js"),
                "module.exports = 2;",
            );
            imports.push(pkg);
        }
        write("index.js", "// root entry\n");

        let from = "/root/index.js";
        let iterations = 50usize;

        // --- Host-direct path (legacy) ---
        let host_start = Instant::now();
        for _ in 0..iterations {
            let mut harness = ModuleResolutionTestHarness::new(&root);
            for pkg in &imports {
                let _ = harness.resolve_require(pkg, from);
            }
        }
        let host_elapsed = host_start.elapsed();

        // --- Kernel-VFS path (new) ---
        // Mount the whole closure root so /root resolves through the VFS.
        let build_kernel = || {
            let mut config = KernelVmConfig::new("vm-perf");
            config.permissions = Permissions::allow_all();
            let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
            let host_dir = crate::plugins::host_dir::HostDirFilesystem::new(&root)
                .expect("host_dir over closure root");
            kernel
                .mount_boxed_filesystem(
                    "/root",
                    Box::new(MountedVirtualFileSystem::new(host_dir)),
                    MountOptions::new("host_dir").read_only(true),
                )
                .expect("mount /root");
            kernel
        };

        let vfs_start = Instant::now();
        for _ in 0..iterations {
            let mut kernel = build_kernel();
            let mut cache = LocalModuleResolutionCache::default();
            let mut resolver = ModuleResolver::new(
                KernelModuleFsReader {
                    kernel: &mut kernel,
                },
                &mut cache,
            );
            for pkg in &imports {
                let _ = resolver.resolve_module(pkg, from, ModuleResolveMode::Require);
            }
        }
        let vfs_elapsed = vfs_start.elapsed();

        // Exclude kernel-build cost from the VFS resolution figure by measuring
        // it separately, so the comparison is resolution-vs-resolution.
        let build_start = Instant::now();
        for _ in 0..iterations {
            let _kernel = build_kernel();
        }
        let build_elapsed = build_start.elapsed();
        let vfs_resolve_only = vfs_elapsed.saturating_sub(build_elapsed);

        let per_closure_host = host_elapsed / iterations as u32;
        let per_closure_vfs = vfs_elapsed / iterations as u32;
        let per_closure_vfs_resolve = vfs_resolve_only / iterations as u32;

        eprintln!("\n=== Phase 0 module-resolution cold-start perf ===");
        eprintln!("closure: {PACKAGES} packages, {iterations} cold iterations");
        eprintln!("host-direct : {host_elapsed:?} total | {per_closure_host:?} / closure");
        eprintln!(
            "kernel-VFS  : {vfs_elapsed:?} total | {per_closure_vfs:?} / closure (incl. mount build)"
        );
        eprintln!(
            "kernel-VFS  : {vfs_resolve_only:?} total | {per_closure_vfs_resolve:?} / closure (resolution only)"
        );
        eprintln!(
            "kernel build: {build_elapsed:?} total | {:?} / closure",
            build_elapsed / iterations as u32
        );
        let ratio = vfs_resolve_only.as_secs_f64() / host_elapsed.as_secs_f64().max(1e-9);
        eprintln!("ratio (vfs-resolve / host): {ratio:.2}x");

        fs::remove_dir_all(&root).expect("remove perf tree");
    }
}
