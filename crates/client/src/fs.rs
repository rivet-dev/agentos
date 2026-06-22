//! Filesystem methods + path guards + supporting types + the in-process [`VirtualFileSystem`] mount
//! contract.
//!
//! Ported from `packages/core/src/agent-os.ts` (fs methods + `_assertSafeAbsolutePath` /
//! `_assertWritableAbsolutePath`), `runtime-compat.ts` (`VirtualStat`, `VirtualFileSystem`), and
//! `filesystem-snapshot.ts` (snapshot export types).
//!
//! Parity notes: every method runs the path guards first; `mkdir` recursive uses the WRITABLE guard,
//! non-recursive uses the SAFE guard. `writeFile` does NOT create parents; `writeFiles` DOES. Batch
//! methods NEVER reject (per-entry error strings). Snapshot wire format keeps octal-string `mode`
//! and `utf8`/`base64` content verbatim.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use secure_exec_client::wire::{
    self, GuestFilesystemCallRequest, GuestFilesystemOperation, GuestFilesystemResultResponse,
    GuestFilesystemStat, RootFilesystemEntry, RootFilesystemEntryEncoding, RootFilesystemEntryKind,
};

use crate::agent_os::AgentOs;
use crate::error::ClientError;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// `string | Uint8Array` file content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContent {
    Text(String),
    Bytes(Vec<u8>),
}

impl From<String> for FileContent {
    fn from(value: String) -> Self {
        FileContent::Text(value)
    }
}

impl From<&str> for FileContent {
    fn from(value: &str) -> Self {
        FileContent::Text(value.to_string())
    }
}

impl From<Vec<u8>> for FileContent {
    fn from(value: Vec<u8>) -> Self {
        FileContent::Bytes(value)
    }
}

impl From<&[u8]> for FileContent {
    fn from(value: &[u8]) -> Self {
        FileContent::Bytes(value.to_vec())
    }
}

/// An entry returned by `readdir_recursive`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: DirEntryType,
    pub size: u64,
}

/// The type of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirEntryType {
    File,
    Directory,
    Symlink,
}

/// Options for `readdir_recursive`. `max_depth` None = unlimited, Some(0) = immediate children only;
/// `exclude` matches basenames at any depth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaddirRecursiveOptions {
    pub max_depth: Option<u32>,
    pub exclude: Vec<String>,
}

/// A batch write entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchWriteEntry {
    pub path: String,
    pub content: FileContent,
}

/// Result of a single batch write (never an `Err`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchWriteResult {
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a single batch read (never an `Err`). `content` is None on failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReadResult {
    pub path: String,
    pub content: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// Options for `mkdir`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MkdirOptions {
    pub recursive: bool,
}

/// Options for `delete`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeleteOptions {
    pub recursive: bool,
}

/// Options for `mount_fs`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MountFsOptions {
    pub read_only: bool,
}

/// Stat result. 16 fields; `*_ms` time fields are `f64` (JS ms, possibly fractional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualStat {
    pub mode: u32,
    pub size: u64,
    pub blocks: u64,
    pub dev: u64,
    pub rdev: u64,
    #[serde(rename = "isDirectory")]
    pub is_directory: bool,
    #[serde(rename = "isSymbolicLink")]
    pub is_symbolic_link: bool,
    #[serde(rename = "atimeMs")]
    pub atime_ms: f64,
    #[serde(rename = "mtimeMs")]
    pub mtime_ms: f64,
    #[serde(rename = "ctimeMs")]
    pub ctime_ms: f64,
    #[serde(rename = "birthtimeMs")]
    pub birthtime_ms: f64,
    pub ino: u64,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
}

/// A registered in-process mount: the live [`VirtualFileSystem`] driver plus the `read_only` flag
/// forwarded from [`MountFsOptions`]. TS `mountFs` passes `{ readOnly }` through to
/// `kernel.mountFs`, which enforces read-only mount semantics; the flag is retained here so the
/// option is not structurally dropped before the mount-dispatch path consumes it.
#[derive(Clone)]
pub struct MountedFs {
    pub driver: Arc<dyn VirtualFileSystem>,
    pub read_only: bool,
}

/// A directory entry with a known type, returned by `read_dir_with_types` on the mount contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDirEntry {
    pub name: String,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

// ---------------------------------------------------------------------------
// Snapshot export wire types (octal-string mode, utf8/base64 content)
// ---------------------------------------------------------------------------

/// `{ kind: "snapshot-export"; source }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSnapshotExport {
    pub kind: SnapshotExportKind,
    pub source: FilesystemSnapshotExport,
}

/// The literal `"snapshot-export"` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotExportKind {
    #[serde(rename = "snapshot-export")]
    SnapshotExport,
}

/// `{ format: "agentos-filesystem-snapshot-v1"; filesystem: { entries } }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemSnapshotExport {
    pub format: String,
    pub filesystem: FilesystemSnapshotEntries,
}

/// `{ entries: FilesystemEntry[] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemSnapshotEntries {
    pub entries: Vec<FilesystemEntry>,
}

/// A single snapshot entry. `mode` is an OCTAL STRING (e.g. `"0755"`). `content` is utf8 or base64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: DirEntryType,
    pub mode: String,
    pub uid: u32,
    pub gid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<FilesystemEntryEncoding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Snapshot content encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesystemEntryEncoding {
    Utf8,
    Base64,
}

// ---------------------------------------------------------------------------
// VirtualFileSystem mount contract (in-process trait object for mount_fs)
// ---------------------------------------------------------------------------

/// The 25-method mount backend contract. A `mount_fs` driver implements this trait; it is a live
/// in-process object and cannot cross an RPC boundary.
///
/// TODO(parity: confirm exact method set/signatures against runtime-compat.ts before first impl).
#[async_trait]
pub trait VirtualFileSystem: Send + Sync {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    async fn read_text_file(&self, path: &str) -> Result<String>;
    async fn read_dir(&self, path: &str) -> Result<Vec<String>>;
    async fn read_dir_with_types(&self, path: &str) -> Result<Vec<VirtualDirEntry>>;
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<()>;
    async fn create_dir(&self, path: &str) -> Result<()>;
    async fn mkdir(&self, path: &str, recursive: bool) -> Result<()>;
    async fn exists(&self, path: &str) -> Result<bool>;
    async fn stat(&self, path: &str) -> Result<VirtualStat>;
    async fn lstat(&self, path: &str) -> Result<VirtualStat>;
    async fn remove_file(&self, path: &str) -> Result<()>;
    async fn remove_dir(&self, path: &str) -> Result<()>;
    async fn rename(&self, from: &str, to: &str) -> Result<()>;
    async fn realpath(&self, path: &str) -> Result<String>;
    async fn symlink(&self, target: &str, path: &str) -> Result<()>;
    async fn readlink(&self, path: &str) -> Result<String>;
    async fn link(&self, existing: &str, new_path: &str) -> Result<()>;
    async fn chmod(&self, path: &str, mode: u32) -> Result<()>;
    async fn chown(&self, path: &str, uid: u32, gid: u32) -> Result<()>;
    async fn utimes(&self, path: &str, atime_ms: f64, mtime_ms: f64) -> Result<()>;
    async fn truncate(&self, path: &str, len: u64) -> Result<()>;
    async fn pread(&self, path: &str, offset: u64, length: u64) -> Result<Vec<u8>>;
    async fn pwrite(&self, path: &str, offset: u64, data: &[u8]) -> Result<u64>;
}

// ---------------------------------------------------------------------------
// Path guards
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Posix-normalize a path the same way Node's `path.posix.normalize` does.
    ///
    /// Matches Node semantics: collapse `.`/`..` segments and duplicate separators, preserve a
    /// trailing slash when present, keep a leading slash for absolute paths, and return `.` for an
    /// empty result. Above-root `..` segments on an absolute path are discarded; on a relative path
    /// they are retained.
    pub(crate) fn posix_normalize(path: &str) -> String {
        if path.is_empty() {
            return String::from(".");
        }

        let is_absolute = path.starts_with('/');
        let trailing_slash = path.ends_with('/');

        let mut segments: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    match segments.last().copied() {
                        Some(last) if last != ".." => {
                            segments.pop();
                        }
                        Some(_) | None => {
                            // Retain leading `..` only on relative paths; on absolute paths the
                            // segment is silently discarded (cannot go above root).
                            if !is_absolute {
                                segments.push("..");
                            }
                        }
                    }
                }
                other => segments.push(other),
            }
        }

        let mut joined = segments.join("/");
        if joined.is_empty() {
            if is_absolute {
                return String::from("/");
            }
            return String::from(".");
        }

        if trailing_slash {
            joined.push('/');
        }
        if is_absolute {
            let mut absolute = String::from("/");
            absolute.push_str(&joined);
            absolute
        } else {
            joined
        }
    }

    /// Throws `PathNotAbsolute` if not absolute, `PathNotNormalized` if not in normalized form.
    pub(crate) fn assert_safe_absolute_path(path: &str) -> std::result::Result<(), ClientError> {
        if !path.starts_with('/') {
            return Err(ClientError::PathNotAbsolute(path.to_string()));
        }
        if Self::posix_normalize(path) != path {
            return Err(ClientError::PathNotNormalized(path.to_string()));
        }
        Ok(())
    }

    /// Runs the safe guard, then rejects writes to read-only paths.
    pub(crate) fn assert_writable_absolute_path(
        path: &str,
    ) -> std::result::Result<(), ClientError> {
        Self::assert_safe_absolute_path(path)?;
        if path == "/proc"
            || path.starts_with("/proc/")
            || path == "/etc/agentos"
            || path.starts_with("/etc/agentos/")
        {
            return Err(ClientError::PathReadOnly(path.to_string()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers (guest filesystem RPC + path joins)
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Render a batch-method error the way the TypeScript `AgentOs` surfaces `err.message` into
    /// `BatchWriteResult.error` / `BatchReadResult.error`. The error may be a bare [`ClientError`]
    /// (path guards) or an [`anyhow::Error`] wrapping one (kernel RPC failures via
    /// [`Self::guest_fs_call`]), so downcast to recover the exact TS message; otherwise fall back to
    /// the anyhow chain string.
    fn batch_error_message(err: &anyhow::Error) -> String {
        match err.downcast_ref::<ClientError>() {
            Some(client_error) => client_error.batch_message(),
            None => err.to_string(),
        }
    }

    /// Build the VM-scoped ownership for guest filesystem RPCs.
    fn fs_vm_scope(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
    }

    /// Posix `dirname` over an already-normalized absolute path (mirrors `posixPath.dirname`).
    fn posix_dirname(path: &str) -> String {
        match path.rfind('/') {
            None => String::from("."),
            Some(0) => String::from("/"),
            Some(idx) => path[..idx].to_string(),
        }
    }

    /// Join a parent directory with a child basename the way the TS fs code does (special-casing the
    /// root so it does not produce a leading `//`).
    fn join_child(dir: &str, child: &str) -> String {
        if dir == "/" {
            format!("/{child}")
        } else {
            format!("{dir}/{child}")
        }
    }

    /// Issue a single guest filesystem RPC and return the typed result, mapping a sidecar
    /// `Rejected` response into a [`ClientError::Kernel`] so the errno `code` survives for parity.
    async fn guest_fs_call(
        &self,
        request: GuestFilesystemCallRequest,
    ) -> Result<GuestFilesystemResultResponse> {
        let scope = self.fs_vm_scope();
        let response = self
            .transport()
            .request_wire(
                scope,
                wire::RequestPayload::GuestFilesystemCallRequest(request),
            )
            .await
            .context("guest filesystem call failed")?;
        match response {
            wire::ResponsePayload::GuestFilesystemResultResponse(result) => Ok(result),
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                Err(ClientError::Kernel { code, message }.into())
            }
            other => Err(anyhow::anyhow!(
                "unexpected response to guest filesystem call: {other:?}"
            )),
        }
    }

    /// A guest filesystem call carrying only an operation + path (the common case).
    fn fs_request(
        operation: GuestFilesystemOperation,
        path: impl Into<String>,
    ) -> GuestFilesystemCallRequest {
        GuestFilesystemCallRequest {
            operation,
            path: path.into(),
            destination_path: None,
            target: None,
            content: None,
            encoding: None,
            recursive: false,
            mode: None,
            uid: None,
            gid: None,
            atime_ms: None,
            mtime_ms: None,
            len: None,
            offset: None,
        }
    }

    /// Convert a wire [`GuestFilesystemStat`] into the public [`VirtualStat`] (`*_ms` widened to
    /// `f64` to match JS millisecond precision).
    fn virtual_stat_from(stat: GuestFilesystemStat) -> VirtualStat {
        VirtualStat {
            mode: stat.mode,
            size: stat.size,
            blocks: stat.blocks,
            dev: stat.dev,
            rdev: stat.rdev,
            is_directory: stat.is_directory,
            is_symbolic_link: stat.is_symbolic_link,
            atime_ms: stat.atime_ms as f64,
            mtime_ms: stat.mtime_ms as f64,
            ctime_ms: stat.ctime_ms as f64,
            birthtime_ms: stat.birthtime_ms as f64,
            ino: stat.ino,
            nlink: stat.nlink,
            uid: stat.uid,
            gid: stat.gid,
        }
    }

    // --- low-level kernel ops (each maps to one guest filesystem RPC) ---

    /// Mirrors TS `decodeGuestFilesystemContent`: a missing `content` field is a hard error
    /// (`sidecar returned no file content for <path>`, fail-by-default), `base64` is decoded, and
    /// any other/absent encoding is treated as utf8 bytes.
    async fn kernel_read_file(&self, path: &str) -> Result<Vec<u8>> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::ReadFile, path))
            .await?;
        let content = result
            .content
            .with_context(|| format!("sidecar returned no file content for {path}"))?;
        match result.encoding {
            Some(RootFilesystemEntryEncoding::Base64) => BASE64
                .decode(content.as_bytes())
                .context("decoding base64 file content"),
            Some(RootFilesystemEntryEncoding::Utf8) | None => Ok(content.into_bytes()),
        }
    }

    /// Mirrors TS `encodeGuestFilesystemContent`: string content is sent verbatim with NO `encoding`
    /// field (the sidecar defaults absent encoding to utf8); byte content is base64-encoded and
    /// carries `encoding: "base64"`.
    async fn kernel_write_file(&self, path: &str, content: &FileContent) -> Result<()> {
        let (encoded, encoding) = match content {
            FileContent::Text(text) => (text.clone(), None),
            FileContent::Bytes(bytes) => (
                BASE64.encode(bytes),
                Some(RootFilesystemEntryEncoding::Base64),
            ),
        };
        let mut request = Self::fs_request(GuestFilesystemOperation::WriteFile, path);
        request.content = Some(encoded);
        request.encoding = encoding;
        self.guest_fs_call(request).await?;
        Ok(())
    }

    /// Single-level directory creation. Mirrors TS `kernel.mkdir(path)` (no options), which the
    /// native client maps to the `create_dir` guest filesystem operation. This backs BOTH
    /// `AgentOs::mkdir` (non-recursive) and every `_mkdirp` component, so it always emits
    /// [`GuestFilesystemOperation::CreateDir`] (never `Mkdir`, which the native client reserves for
    /// the recursive `kernel.mkdir(path, { recursive: true })` shape that this code path never uses).
    async fn kernel_mkdir(&self, path: &str) -> Result<()> {
        self.guest_fs_call(Self::fs_request(GuestFilesystemOperation::CreateDir, path))
            .await?;
        Ok(())
    }

    async fn kernel_exists(&self, path: &str) -> Result<bool> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::Exists, path))
            .await?;
        Ok(result.exists.unwrap_or(false))
    }

    async fn kernel_readdir(&self, path: &str) -> Result<Vec<String>> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::ReadDir, path))
            .await?;
        Ok(result.entries.unwrap_or_default())
    }

    async fn kernel_stat(&self, path: &str) -> Result<VirtualStat> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::Stat, path))
            .await?;
        let stat = result.stat.context("stat response missing stat payload")?;
        Ok(Self::virtual_stat_from(stat))
    }

    async fn kernel_lstat(&self, path: &str) -> Result<VirtualStat> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::Lstat, path))
            .await?;
        let stat = result.stat.context("lstat response missing stat payload")?;
        Ok(Self::virtual_stat_from(stat))
    }

    async fn kernel_readlink(&self, path: &str) -> Result<String> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::ReadLink, path))
            .await?;
        result.target.context("readlink response missing target")
    }

    async fn kernel_symlink(&self, target: &str, path: &str) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Symlink, path);
        request.target = Some(target.to_string());
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_rename(&self, from: &str, to: &str) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Rename, from);
        request.destination_path = Some(to.to_string());
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_chmod(&self, path: &str, mode: u32) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Chmod, path);
        request.mode = Some(mode);
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_chown(&self, path: &str, uid: u32, gid: u32) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Chown, path);
        request.uid = Some(uid);
        request.gid = Some(gid);
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_remove_file(&self, path: &str) -> Result<()> {
        self.guest_fs_call(Self::fs_request(GuestFilesystemOperation::RemoveFile, path))
            .await?;
        Ok(())
    }

    async fn kernel_remove_dir(&self, path: &str) -> Result<()> {
        self.guest_fs_call(Self::fs_request(GuestFilesystemOperation::RemoveDir, path))
            .await?;
        Ok(())
    }

    /// Recursively create directories (`mkdir -p`). Uses the WRITABLE guard, then walks each path
    /// component and creates the ones that do not yet exist (mirrors TS `_mkdirp`).
    async fn mkdirp(&self, path: &str) -> Result<()> {
        Self::assert_writable_absolute_path(path)?;
        let mut current = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            current.push('/');
            current.push_str(part);
            if !self.kernel_exists(&current).await? {
                self.kernel_mkdir(&current).await?;
            }
        }
        Ok(())
    }

    /// Recursive copy preserving mode/uid/gid and replicating symlinks (mirrors TS `_copyPath`).
    /// Boxed because it recurses across an `async fn` boundary.
    fn copy_path<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Self::assert_writable_absolute_path(to)?;
            let stat = self.kernel_lstat(from).await?;
            if stat.is_symbolic_link {
                let target = self.kernel_readlink(from).await?;
                self.kernel_symlink(&target, to).await?;
                return Ok(());
            }
            if stat.is_directory {
                self.mkdirp(&Self::posix_dirname(to)).await?;
                if !self.kernel_exists(to).await? {
                    self.kernel_mkdir(to).await?;
                }
                self.kernel_chmod(to, stat.mode).await?;
                self.kernel_chown(to, stat.uid, stat.gid).await?;
                let entries = self.kernel_readdir(from).await?;
                for entry in entries {
                    if entry == "." || entry == ".." {
                        continue;
                    }
                    let from_path = Self::join_child(from, &entry);
                    let to_path = Self::join_child(to, &entry);
                    self.copy_path(&from_path, &to_path).await?;
                }
                return Ok(());
            }
            let content = self.kernel_read_file(from).await?;
            self.write_file(to, content).await?;
            self.kernel_chmod(to, stat.mode).await?;
            self.kernel_chown(to, stat.uid, stat.gid).await?;
            Ok(())
        })
    }

    /// `delete`, boxed so the recursive child walk can cross the `async fn` boundary.
    fn delete_inner<'a>(
        &'a self,
        path: &'a str,
        recursive: bool,
    ) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let stat = self.kernel_lstat(path).await?;
            if stat.is_directory {
                if recursive {
                    let entries = self.kernel_readdir(path).await?;
                    for entry in entries {
                        if entry == "." || entry == ".." {
                            continue;
                        }
                        let child = format!("{path}/{entry}");
                        // Mirror TS `delete` recursion, which re-runs the safe-path guard on each
                        // child via the public `this.delete(...)` call before recursing.
                        Self::assert_safe_absolute_path(&child)?;
                        self.delete_inner(&child, true).await?;
                    }
                }
                return self.kernel_remove_dir(path).await;
            }
            self.kernel_remove_file(path).await
        })
    }
}

// ---------------------------------------------------------------------------
// Filesystem methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Read a file's raw bytes (no decode).
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        Self::assert_safe_absolute_path(path)?;
        self.kernel_read_file(path).await
    }

    /// Write a file. Writable-path guard; does NOT auto-create parents; `Text` -> UTF-8.
    pub async fn write_file(&self, path: &str, content: impl Into<FileContent>) -> Result<()> {
        Self::assert_writable_absolute_path(path)?;
        let content = content.into();
        self.kernel_write_file(path, &content).await
    }

    /// Batch write. Sequential; never rejects (per-entry error); auto-creates parent dirs.
    pub async fn write_files(&self, entries: Vec<BatchWriteEntry>) -> Vec<BatchWriteResult> {
        let mut results = Vec::with_capacity(entries.len());
        for entry in entries {
            let outcome: Result<()> = async {
                Self::assert_writable_absolute_path(&entry.path)?;
                // Create parent directories as needed. TS slices off everything after the last `/`;
                // for a path like `/foo` this yields an empty parent which is skipped.
                if let Some(idx) = entry.path.rfind('/') {
                    let parent = &entry.path[..idx];
                    if !parent.is_empty() {
                        self.mkdirp(parent).await?;
                    }
                }
                self.kernel_write_file(&entry.path, &entry.content).await?;
                Ok(())
            }
            .await;
            match outcome {
                Ok(()) => results.push(BatchWriteResult {
                    path: entry.path,
                    success: true,
                    error: None,
                }),
                Err(err) => results.push(BatchWriteResult {
                    path: entry.path,
                    success: false,
                    error: Some(Self::batch_error_message(&err)),
                }),
            }
        }
        results
    }

    /// Batch read. Sequential; never rejects; `content` None on failure.
    pub async fn read_files(&self, paths: Vec<String>) -> Vec<BatchReadResult> {
        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            let outcome: Result<Vec<u8>> = async {
                Self::assert_safe_absolute_path(&path)?;
                self.kernel_read_file(&path).await
            }
            .await;
            match outcome {
                Ok(content) => results.push(BatchReadResult {
                    path,
                    content: Some(content),
                    error: None,
                }),
                Err(err) => results.push(BatchReadResult {
                    path,
                    content: None,
                    error: Some(Self::batch_error_message(&err)),
                }),
            }
        }
        results
    }

    /// Make a directory. Recursive -> writable guard + mkdirp; non-recursive -> safe guard + single
    /// level. The guard asymmetry is load-bearing.
    pub async fn mkdir(&self, path: &str, options: MkdirOptions) -> Result<()> {
        if options.recursive {
            return self.mkdirp(path).await;
        }
        Self::assert_writable_absolute_path(path)?;
        self.kernel_mkdir(path).await
    }

    /// List basenames (may include `.`/`..`).
    pub async fn readdir(&self, path: &str) -> Result<Vec<String>> {
        Self::assert_safe_absolute_path(path)?;
        self.kernel_readdir(path).await
    }

    /// Recursive BFS listing; symlinks recorded but NOT descended; a stat failure aborts the call.
    pub async fn readdir_recursive(
        &self,
        path: &str,
        options: ReaddirRecursiveOptions,
    ) -> Result<Vec<DirEntry>> {
        Self::assert_safe_absolute_path(path)?;
        let max_depth = options.max_depth;
        let exclude: std::collections::HashSet<&str> =
            options.exclude.iter().map(String::as_str).collect();
        let mut results: Vec<DirEntry> = Vec::new();

        // BFS queue of `(dir_path, current_depth)`.
        let mut queue: std::collections::VecDeque<(String, u32)> =
            std::collections::VecDeque::new();
        queue.push_back((path.to_string(), 0));

        while let Some((dir_path, depth)) = queue.pop_front() {
            let entries = self.kernel_readdir(&dir_path).await?;
            for name in entries {
                if name == "." || name == ".." {
                    continue;
                }
                if exclude.contains(name.as_str()) {
                    continue;
                }
                let full_path = Self::join_child(&dir_path, &name);
                let s = self.kernel_lstat(&full_path).await?;
                if s.is_symbolic_link {
                    results.push(DirEntry {
                        path: full_path,
                        entry_type: DirEntryType::Symlink,
                        size: s.size,
                    });
                } else if s.is_directory {
                    results.push(DirEntry {
                        path: full_path.clone(),
                        entry_type: DirEntryType::Directory,
                        size: s.size,
                    });
                    if max_depth.is_none() || depth < max_depth.unwrap() {
                        queue.push_back((full_path, depth + 1));
                    }
                } else {
                    results.push(DirEntry {
                        path: full_path,
                        entry_type: DirEntryType::File,
                        size: s.size,
                    });
                }
            }
        }

        Ok(results)
    }

    /// Stat (follows symlinks).
    pub async fn stat(&self, path: &str) -> Result<VirtualStat> {
        Self::assert_safe_absolute_path(path)?;
        self.kernel_stat(path).await
    }

    /// Existence check. Safe-path guard still errors; missing path -> false.
    pub async fn exists(&self, path: &str) -> Result<bool> {
        Self::assert_safe_absolute_path(path)?;
        self.kernel_exists(path).await
    }

    /// Export the root filesystem snapshot. Octal-string mode + utf8/base64 content verbatim.
    pub async fn snapshot_root_filesystem(&self) -> Result<RootSnapshotExport> {
        let scope = self.fs_vm_scope();
        let response = self
            .transport()
            .request_wire(scope, wire::RequestPayload::SnapshotRootFilesystemRequest)
            .await
            .context("snapshot root filesystem failed")?;
        let snapshot = match response {
            wire::ResponsePayload::RootFilesystemSnapshotResponse(snapshot) => snapshot,
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                return Err(ClientError::Kernel { code, message }.into());
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unexpected response to snapshot root filesystem: {other:?}"
                ));
            }
        };

        let entries = snapshot
            .entries
            .into_iter()
            .map(Self::snapshot_entry_from)
            .collect::<Result<Vec<_>>>()?;

        Ok(RootSnapshotExport {
            kind: SnapshotExportKind::SnapshotExport,
            source: FilesystemSnapshotExport {
                format: String::from("agentos-filesystem-snapshot-v1"),
                filesystem: FilesystemSnapshotEntries { entries },
            },
        })
    }

    /// Mount an in-process [`VirtualFileSystem`] driver. SYNC. Safe-path guard. The driver is a live
    /// trait object and cannot cross an RPC boundary, so it is registered (together with the
    /// `read_only` flag, mirroring TS `kernel.mountFs({ readOnly })`) in the in-process mount table
    /// keyed by its normalized guest path.
    pub fn mount_fs(
        &self,
        path: &str,
        driver: Arc<dyn VirtualFileSystem>,
        options: MountFsOptions,
    ) -> std::result::Result<(), ClientError> {
        Self::assert_safe_absolute_path(path)?;
        let _ = self.inner().in_process_mounts.insert(
            path.to_string(),
            MountedFs {
                driver,
                read_only: options.read_only,
            },
        );
        Ok(())
    }

    /// Unmount a previously mounted path. SYNC.
    pub fn unmount_fs(&self, path: &str) -> std::result::Result<(), ClientError> {
        Self::assert_safe_absolute_path(path)?;
        self.inner().in_process_mounts.remove(path);
        Ok(())
    }

    /// Move a path. `lstat(from)` no-follow; symlink/non-dir -> rename; real dir -> recursive copy
    /// (preserve mode/uid/gid/symlinks) + recursive delete. (TS `move`.)
    pub async fn move_path(&self, from: &str, to: &str) -> Result<()> {
        Self::assert_writable_absolute_path(from)?;
        Self::assert_writable_absolute_path(to)?;
        let source_stat = self.kernel_lstat(from).await?;
        if !source_stat.is_directory || source_stat.is_symbolic_link {
            return self.kernel_rename(from, to).await;
        }
        self.copy_path(from, to).await?;
        self.delete(from, DeleteOptions { recursive: true }).await
    }

    /// Delete a path. `lstat` to discriminate; recursive manually recurses children then `remove_dir`;
    /// non-recursive dir -> `remove_dir` (ENOTEMPTY if non-empty).
    pub async fn delete(&self, path: &str, options: DeleteOptions) -> Result<()> {
        Self::assert_writable_absolute_path(path)?;
        self.delete_inner(path, options.recursive).await
    }

    /// Convert a wire [`RootFilesystemEntry`] into the public snapshot [`FilesystemEntry`],
    /// preserving the octal-string `mode` and verbatim utf8/base64 `content`/`target`.
    ///
    /// Mirrors TS `convertSidecarRootSnapshotEntries` + `toSnapshotModeString` exactly:
    /// - `mode` falls back kind-dependently when absent (directory 0o755, symlink 0o777, file 0o644).
    /// - file entries ALWAYS carry `content` (defaulting to `""`) and `encoding` (defaulting to
    ///   `utf8`); directory/symlink entries carry neither.
    /// - symlink entries REQUIRE a `target`; a missing target is a hard error (fail-by-default),
    ///   matching the TS `throw`.
    fn snapshot_entry_from(entry: RootFilesystemEntry) -> Result<FilesystemEntry> {
        let entry_type = match entry.kind {
            RootFilesystemEntryKind::File => DirEntryType::File,
            RootFilesystemEntryKind::Directory => DirEntryType::Directory,
            RootFilesystemEntryKind::Symlink => DirEntryType::Symlink,
        };
        // Kind-dependent permission-bit fallback, then octal string with leading `0` masked to the
        // permission bits, matching TS `toSnapshotModeString`.
        let fallback_mode = match entry.kind {
            RootFilesystemEntryKind::Directory => 0o755,
            RootFilesystemEntryKind::Symlink => 0o777,
            RootFilesystemEntryKind::File => 0o644,
        };
        let mode = format!("0{:o}", entry.mode.unwrap_or(fallback_mode) & 0o7777);
        let uid = entry.uid.unwrap_or(0);
        let gid = entry.gid.unwrap_or(0);

        match entry.kind {
            RootFilesystemEntryKind::File => {
                let encoding = match entry.encoding {
                    Some(RootFilesystemEntryEncoding::Utf8) | None => FilesystemEntryEncoding::Utf8,
                    Some(RootFilesystemEntryEncoding::Base64) => FilesystemEntryEncoding::Base64,
                };
                Ok(FilesystemEntry {
                    path: entry.path,
                    entry_type,
                    mode,
                    uid,
                    gid,
                    content: Some(entry.content.unwrap_or_default()),
                    encoding: Some(encoding),
                    target: None,
                })
            }
            RootFilesystemEntryKind::Symlink => {
                let target = entry.target.with_context(|| {
                    format!(
                        "sidecar root snapshot for {} is missing a symlink target",
                        entry.path
                    )
                })?;
                Ok(FilesystemEntry {
                    path: entry.path,
                    entry_type,
                    mode,
                    uid,
                    gid,
                    content: None,
                    encoding: None,
                    target: Some(target),
                })
            }
            RootFilesystemEntryKind::Directory => Ok(FilesystemEntry {
                path: entry.path,
                entry_type,
                mode,
                uid,
                gid,
                content: None,
                encoding: None,
                target: None,
            }),
        }
    }
}
