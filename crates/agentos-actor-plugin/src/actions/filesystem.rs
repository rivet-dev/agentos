//! Filesystem actions. Each helper takes `&AgentOs` plus typed args
//! and delegates to the matching upstream `AgentOs::*` method.

use agentos_client::{
    AgentOs, DeleteOptions, DirEntry, FileContent, MkdirOptions, ReaddirRecursiveOptions,
    VirtualDirEntry, VirtualStat,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// `readFile(path)` — port of [`AgentOs::read_file`].
pub async fn read_file(vm: &AgentOs, path: &str) -> Result<Vec<u8>> {
    vm.read_file(path)
        .await
        .inspect_err(|error| tracing::error!(?error, path, "read_file failed"))
}

/// `writeFile(path, contents)` — port of [`AgentOs::write_file`].
pub async fn write_file(vm: &AgentOs, path: &str, contents: Vec<u8>) -> Result<()> {
    vm.write_file(path, FileContent::Bytes(contents))
        .await
        .inspect_err(|error| tracing::error!(?error, path, "write_file failed"))
}

/// `stat(path)` — port of [`AgentOs::stat`]. Returns the [`VirtualStat`]
/// structure directly; the rivetkit encoder handles cross-encoding
/// translation (bare / cbor / json) at the framework layer.
pub async fn stat(vm: &AgentOs, path: &str) -> Result<VirtualStat> {
    vm.stat(path).await
}

/// `mkdir(path)` — port of [`AgentOs::mkdir`]. Always recursive so the
/// JS shim's "create parent dirs if needed" expectation holds; the
/// driver tests rely on this.
pub async fn mkdir(vm: &AgentOs, path: &str) -> Result<()> {
    vm.mkdir(path, MkdirOptions { recursive: true }).await
}

/// `readdir(path)` — port of [`AgentOs::readdir`]. Returns the
/// (unsorted) child names, including `.` and `..`. Sorting / filtering
/// is up to the caller.
pub async fn readdir(vm: &AgentOs, path: &str) -> Result<Vec<String>> {
    vm.readdir(path).await
}

/// One typed directory entry returned by [`readdir_entries`]. Serializes as
/// `{ name, isDirectory, isSymbolicLink }`. No `size` — the fast path skips the
/// per-entry `stat`; callers that need a size `stat` the file when it is opened.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaddirEntryDto {
    pub name: String,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

impl From<VirtualDirEntry> for ReaddirEntryDto {
    fn from(entry: VirtualDirEntry) -> Self {
        Self {
            name: entry.name,
            is_directory: entry.is_directory,
            is_symbolic_link: entry.is_symbolic_link,
        }
    }
}

/// `readdirEntries(path)` — one round-trip directory listing with each child's
/// type, replacing `readdir` + a `stat` per entry (which wedges the actor on
/// large or virtual dirs). Routes through the kernel, so mounts list correctly.
///
/// Returns `None` (serialized as `null`) when `path` is not a listable directory
/// — i.e. it does not exist (`ENOENT`) or is a file (`ENOTDIR`). Callers (the
/// inspector's editable cwd) treat `null` as "not found / not a directory",
/// distinct from `Some([])` (an empty directory). Other kernel failures still
/// propagate as errors. This avoids surfacing an opaque RivetKit
/// `internal_error` for the common typo-a-path case, since a *successful* `null`
/// is not sanitized the way a rejection's message is.
pub async fn readdir_entries(vm: &AgentOs, path: &str) -> Result<Option<Vec<ReaddirEntryDto>>> {
    match vm.read_dir_with_types(path).await {
        Ok(entries) => Ok(Some(
            entries.into_iter().map(ReaddirEntryDto::from).collect(),
        )),
        Err(error) if is_not_a_listable_dir(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// True when a `read_dir_with_types` failure means "there is no directory here":
/// the kernel embeds the POSIX errno in the message (the wire `code` is a generic
/// `kernel_error`), reporting `ENOENT` for a missing path and `ENOTDIR` for a
/// path that resolves to a file. Any other errno is a real failure.
fn is_not_a_listable_dir(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let msg = cause.to_string();
        msg.contains("ENOENT") || msg.contains("ENOTDIR")
    })
}

/// `exists(path)` — port of [`AgentOs::exists`].
pub async fn exists(vm: &AgentOs, path: &str) -> Result<bool> {
    vm.exists(path).await
}

/// `move(from, to)` — port of [`AgentOs::move_path`]. Named `move_path`
/// in Rust because `move` is a keyword.
pub async fn move_path(vm: &AgentOs, from: &str, to: &str) -> Result<()> {
    vm.move_path(from, to).await
}

/// Options for `deleteFile`. TS sends `{ recursive?: boolean }`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOptionsArg {
    #[serde(default)]
    pub recursive: bool,
}

/// `deleteFile(path, options?)` — port of [`AgentOs::delete`]. Honors the
/// `recursive` option so directory deletes match JS semantics.
pub async fn delete_file(vm: &AgentOs, path: &str, recursive: bool) -> Result<()> {
    vm.delete(path, DeleteOptions { recursive }).await
}

/// `readdirRecursive(path)` — port of [`AgentOs::readdir_recursive`].
/// Returns every reachable entry with its type and size. Unbounded
/// depth; the JS shim passes no max-depth in the driver tests so this
/// arm defaults to `ReaddirRecursiveOptions::default()`.
pub async fn readdir_recursive(vm: &AgentOs, path: &str) -> Result<Vec<DirEntry>> {
    vm.readdir_recursive(path, ReaddirRecursiveOptions::default())
        .await
}

// ---------------------------------------------------------------------------
// Action argument / reply DTOs
// ---------------------------------------------------------------------------

/// Accept either a CBOR text string, a CBOR byte string (via `ByteBuf`), or
/// the `["$Uint8Array", base64]` wrapper that TS encoders emit when the
/// outer codec is JSON-compatible. Used by `writeFile`.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum WriteFileContent {
    String(String),
    Bytes(serde_bytes::ByteBuf),
    Wrapped(JsonCompatUint8Array),
}

impl WriteFileContent {
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::String(s) => s.into_bytes(),
            Self::Bytes(b) => b.into_vec(),
            Self::Wrapped(w) => w.bytes,
        }
    }
}

/// Deserializer for the `["$Uint8Array", base64]` envelope. Part of
/// [`WriteFileContent`]'s untagged enum so the same arms accept wrapped
/// bytes from the JSON encoder path.
pub struct JsonCompatUint8Array {
    bytes: Vec<u8>,
}

impl<'de> Deserialize<'de> for JsonCompatUint8Array {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
        let (tag, base64): (String, String) = Deserialize::deserialize(deserializer)?;
        if tag != "$Uint8Array" {
            return Err(serde::de::Error::custom(format!(
                "expected $Uint8Array wrapper, got {tag}"
            )));
        }
        let bytes = BASE64
            .decode(&base64)
            .map_err(|error| serde::de::Error::custom(format!("base64 decode: {error}")))?;
        Ok(Self { bytes })
    }
}
