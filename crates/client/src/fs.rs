//! Filesystem methods and supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (fs methods), `runtime.ts` (`VirtualStat`), and
//! `filesystem-snapshot.ts` (snapshot export types).
//!
//! Snapshot wire format keeps octal-string `mode` and `utf8`/`base64` content verbatim. Path
//! normalization, cwd resolution, and read-only policy belong to the sidecar/kernel.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use agentos_sidecar_client::wire::{
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

/// Options for `readdir_recursive`. `max_depth` None = unlimited, Some(0) = immediate children only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaddirRecursiveOptions {
    pub max_depth: Option<u32>,
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

/// A directory entry with a known type, returned by `read_dir_with_types`.
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
// Internal helpers (guest filesystem RPC + path joins)
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Build the VM-scoped ownership for guest filesystem RPCs.
    fn fs_vm_scope(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
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
            recursive: None,
            max_depth: None,
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

    /// Missing content or encoding is a malformed sidecar response. The sidecar always supplies
    /// its selected utf8/base64 encoding for a read response.
    async fn kernel_read_file(&self, path: &str) -> Result<Vec<u8>> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::ReadFile, path))
            .await?;
        let content = result
            .content
            .with_context(|| format!("sidecar returned no file content for {path}"))?;
        match result
            .encoding
            .with_context(|| format!("sidecar returned no file encoding for {path}"))?
        {
            RootFilesystemEntryEncoding::Base64 => BASE64
                .decode(content.as_bytes())
                .context("decoding base64 file content"),
            RootFilesystemEntryEncoding::Utf8 => Ok(content.into_bytes()),
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

    async fn kernel_mkdir(&self, path: &str, recursive: bool) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Mkdir, path);
        request.recursive = recursive.then_some(true);
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_exists(&self, path: &str) -> Result<bool> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::Exists, path))
            .await?;
        require_exists_payload(result.exists, path)
    }

    async fn kernel_readdir(&self, path: &str) -> Result<Vec<wire::GuestDirEntry>> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::ReadDir, path))
            .await?;
        require_entries_payload(result.entries, "directory", path)
    }

    async fn kernel_readdir_recursive(
        &self,
        path: &str,
        max_depth: Option<u32>,
    ) -> Result<Vec<wire::GuestDirEntry>> {
        let mut request = Self::fs_request(GuestFilesystemOperation::ReadDirRecursive, path);
        request.max_depth = max_depth;
        let result = self.guest_fs_call(request).await?;
        require_entries_payload(result.entries, "recursive directory", path)
    }

    async fn kernel_stat(&self, path: &str) -> Result<VirtualStat> {
        let result = self
            .guest_fs_call(Self::fs_request(GuestFilesystemOperation::Stat, path))
            .await?;
        let stat = result.stat.context("stat response missing stat payload")?;
        Ok(Self::virtual_stat_from(stat))
    }

    async fn kernel_remove_path(&self, path: &str, recursive: bool) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Remove, path);
        request.recursive = recursive.then_some(true);
        self.guest_fs_call(request).await?;
        Ok(())
    }

    async fn kernel_move_path(&self, from: &str, to: &str) -> Result<()> {
        let mut request = Self::fs_request(GuestFilesystemOperation::Move, from);
        request.destination_path = Some(to.to_string());
        self.guest_fs_call(request).await?;
        Ok(())
    }
}

fn require_exists_payload(value: Option<bool>, path: &str) -> Result<bool> {
    value.with_context(|| format!("sidecar returned no exists result for {path}"))
}

fn require_entries_payload(
    value: Option<Vec<wire::GuestDirEntry>>,
    operation: &str,
    path: &str,
) -> Result<Vec<wire::GuestDirEntry>> {
    value.with_context(|| format!("sidecar returned no {operation} entries for {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_response_does_not_invent_missing_linux_metadata() {
        let error = AgentOs::snapshot_entry_from(RootFilesystemEntry {
            path: String::from("/workspace/file.txt"),
            kind: RootFilesystemEntryKind::File,
            mode: None,
            uid: Some(501),
            gid: Some(20),
            content: Some(String::from("hello")),
            encoding: Some(RootFilesystemEntryEncoding::Utf8),
            target: None,
            executable: false,
        })
        .expect_err("missing sidecar metadata must fail");

        assert_eq!(
            error.to_string(),
            "sidecar root snapshot for /workspace/file.txt is missing mode"
        );
    }

    #[test]
    fn filesystem_responses_do_not_invent_missing_results() {
        assert_eq!(
            require_exists_payload(None, "/missing")
                .expect_err("missing exists payload must fail")
                .to_string(),
            "sidecar returned no exists result for /missing"
        );
        assert_eq!(
            require_entries_payload(None, "directory", "/empty")
                .expect_err("missing directory payload must fail")
                .to_string(),
            "sidecar returned no directory entries for /empty"
        );
    }
}

// ---------------------------------------------------------------------------
// Filesystem methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Read a file's raw bytes (no decode).
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.kernel_read_file(path).await
    }

    /// Write a file. Does not auto-create parents; `Text` becomes UTF-8.
    pub async fn write_file(&self, path: &str, content: impl Into<FileContent>) -> Result<()> {
        let content = content.into();
        self.kernel_write_file(path, &content).await
    }

    /// Make a directory through the sidecar's native mkdir primitive.
    pub async fn mkdir(&self, path: &str, options: MkdirOptions) -> Result<()> {
        self.kernel_mkdir(path, options.recursive).await
    }

    /// List basenames (may include `.`/`..`).
    pub async fn readdir(&self, path: &str) -> Result<Vec<String>> {
        Ok(self
            .kernel_readdir(path)
            .await?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    /// List directory entries with the types returned by the sidecar/kernel.
    pub(crate) async fn acp_read_dir_with_types(&self, path: &str) -> Result<Vec<VirtualDirEntry>> {
        Ok(self
            .kernel_readdir(path)
            .await?
            .into_iter()
            .filter(|entry| entry.name != "." && entry.name != "..")
            .map(|entry| VirtualDirEntry {
                name: entry.name,
                is_directory: entry.is_directory,
                is_symbolic_link: entry.is_symbolic_link,
            })
            .collect())
    }

    /// Typed directory listing. `.`/`..` are filtered.
    pub async fn read_dir_with_types(&self, path: &str) -> Result<Vec<VirtualDirEntry>> {
        self.acp_read_dir_with_types(path).await
    }

    /// Recursive BFS listing; symlinks recorded but NOT descended; a stat failure aborts the call.
    pub async fn readdir_recursive(
        &self,
        path: &str,
        options: ReaddirRecursiveOptions,
    ) -> Result<Vec<DirEntry>> {
        let entries = self
            .kernel_readdir_recursive(path, options.max_depth)
            .await?;
        Ok(entries
            .into_iter()
            .map(|entry| DirEntry {
                path: entry.path,
                entry_type: if entry.is_symbolic_link {
                    DirEntryType::Symlink
                } else if entry.is_directory {
                    DirEntryType::Directory
                } else {
                    DirEntryType::File
                },
                size: entry.size,
            })
            .collect())
    }

    /// Stat (follows symlinks).
    pub async fn stat(&self, path: &str) -> Result<VirtualStat> {
        self.kernel_stat(path).await
    }

    /// Existence check. A missing path returns false.
    pub async fn exists(&self, path: &str) -> Result<bool> {
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

    /// Move a path through the sidecar primitive. The kernel attempts rename first, then falls back
    /// to recursive copy+remove on EXDEV.
    pub async fn move_path(&self, from: &str, to: &str) -> Result<()> {
        self.kernel_move_path(from, to).await
    }

    /// Delete a path through the sidecar primitive. Non-recursive directory deletes preserve
    /// ENOTEMPTY semantics.
    pub async fn delete(&self, path: &str, options: DeleteOptions) -> Result<()> {
        self.kernel_remove_path(path, options.recursive).await
    }

    /// Convert a wire [`RootFilesystemEntry`] into the public snapshot [`FilesystemEntry`],
    /// preserving the octal-string `mode` and verbatim utf8/base64 `content`/`target`.
    ///
    /// The sidecar snapshots live Linux state and therefore must supply mode/uid/gid for every
    /// entry, content/encoding for files, and a target for symlinks. Missing response fields are a
    /// malformed sidecar response, not a client-owned opportunity to invent filesystem defaults.
    fn snapshot_entry_from(entry: RootFilesystemEntry) -> Result<FilesystemEntry> {
        let snapshot_path = entry.path.clone();
        let entry_type = match entry.kind {
            RootFilesystemEntryKind::File => DirEntryType::File,
            RootFilesystemEntryKind::Directory => DirEntryType::Directory,
            RootFilesystemEntryKind::Symlink => DirEntryType::Symlink,
        };
        let mode = format!(
            "0{:o}",
            entry.mode.with_context(|| {
                format!("sidecar root snapshot for {snapshot_path} is missing mode")
            })? & 0o7777
        );
        let uid = entry
            .uid
            .with_context(|| format!("sidecar root snapshot for {snapshot_path} is missing uid"))?;
        let gid = entry
            .gid
            .with_context(|| format!("sidecar root snapshot for {snapshot_path} is missing gid"))?;

        match entry.kind {
            RootFilesystemEntryKind::File => {
                let encoding = match entry.encoding.with_context(|| {
                    format!("sidecar root snapshot for {snapshot_path} is missing file encoding")
                })? {
                    RootFilesystemEntryEncoding::Utf8 => FilesystemEntryEncoding::Utf8,
                    RootFilesystemEntryEncoding::Base64 => FilesystemEntryEncoding::Base64,
                };
                Ok(FilesystemEntry {
                    path: entry.path,
                    entry_type,
                    mode,
                    uid,
                    gid,
                    content: Some(entry.content.with_context(|| {
                        format!("sidecar root snapshot for {snapshot_path} is missing file content")
                    })?),
                    encoding: Some(encoding),
                    target: None,
                })
            }
            RootFilesystemEntryKind::Symlink => {
                let target = entry.target.with_context(|| {
                    format!("sidecar root snapshot for {snapshot_path} is missing a symlink target")
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
