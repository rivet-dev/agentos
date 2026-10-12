use crate::budgets::{HostedActorBudgets, ValidationBound};
use std::sync::Arc;

use agentos_client::{
    BatchWriteEntry, DirEntry, DirEntryType, FileContent, MkdirOptions, MountInfo,
    ReaddirRecursiveOptions, RemoveOptions, RootSnapshotExport, VirtualStat,
};
use anyhow::{bail, Result};
use rivetkit::{Ctx, Handler};
use serde::{Deserialize, Serialize};

use crate::AgentOsActor;

#[cfg(test)]
const MAX_BATCH_PATHS: usize = 128;
#[cfg(test)]
const MAX_TRANSFER_BYTES: usize = 768 * 1024;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileBytes(
    #[serde(with = "serde_bytes")]
    #[cfg_attr(feature = "contract", ts(type = "Uint8Array"))]
    pub Vec<u8>,
);

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileContentInput {
    Text(String),
    Bytes(
        #[serde(with = "serde_bytes")]
        #[cfg_attr(feature = "contract", ts(type = "Uint8Array"))]
        Vec<u8>,
    ),
}

impl FileContentInput {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::Text(value) => value.len(),
            Self::Bytes(value) => value.len(),
        }
    }

    pub(crate) fn into_core(self) -> FileContent {
        match self {
            Self::Text(value) => FileContent::Text(value),
            Self::Bytes(value) => FileContent::Bytes(value),
        }
    }
}

macro_rules! path_action {
    ($name:ident, $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub path: String,
        }

        crate::register_action!($name => $output, $wire_name);
    };
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemReadFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

crate::register_action!(FilesystemReadFile => FileBytes, "filesystem.readFile");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemWriteFile {
    pub path: String,
    pub content: FileContentInput,
}

crate::register_action!(FilesystemWriteFile => (), "filesystem.writeFile");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemReadFiles {
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

crate::register_action!(FilesystemReadFiles => Vec<FilesystemReadResult>, "filesystem.readFiles");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemReadResult {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<FileBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemWriteFiles {
    pub entries: Vec<FilesystemWriteEntry>,
}

crate::register_action!(FilesystemWriteFiles => Vec<FilesystemWriteResult>, "filesystem.writeFiles");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemWriteEntry {
    pub path: String,
    pub content: FileContentInput,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemWriteResult {
    pub path: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

path_action!(FilesystemStat, ActorFileStat, "filesystem.stat");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorFileStat {
    pub mode: u32,
    pub size_bytes: u64,
    pub blocks: u64,
    pub dev: u64,
    pub rdev: u64,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub atime_ms: f64,
    pub mtime_ms: f64,
    pub ctime_ms: f64,
    pub birthtime_ms: f64,
    pub ino: u64,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
}

impl From<VirtualStat> for ActorFileStat {
    fn from(stat: VirtualStat) -> Self {
        Self {
            mode: stat.mode,
            size_bytes: stat.size,
            blocks: stat.blocks,
            dev: stat.dev,
            rdev: stat.rdev,
            is_directory: stat.is_directory,
            is_symbolic_link: stat.is_symbolic_link,
            atime_ms: stat.atime_ms,
            mtime_ms: stat.mtime_ms,
            ctime_ms: stat.ctime_ms,
            birthtime_ms: stat.birthtime_ms,
            ino: stat.ino,
            nlink: stat.nlink,
            uid: stat.uid,
            gid: stat.gid,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMkdir {
    pub path: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub recursive: bool,
}

crate::register_action!(FilesystemMkdir => (), "filesystem.mkdir");

path_action!(FilesystemReaddir, Vec<String>, "filesystem.readdir");
path_action!(
    FilesystemReaddirEntries,
    Vec<FilesystemDirectoryEntry>,
    "filesystem.readdirEntries"
);

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemDirectoryEntry {
    pub name: String,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemReaddirRecursive {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub exclude: Vec<String>,
}

crate::register_action!(FilesystemReaddirRecursive => Vec<ActorDirectoryEntry>, "filesystem.readdirRecursive");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorDirectoryEntry {
    pub path: String,
    #[serde(rename = "type")]
    #[cfg_attr(feature = "contract", ts(rename = "type"))]
    pub entry_type: DirEntryType,
    pub size_bytes: u64,
}

impl From<DirEntry> for ActorDirectoryEntry {
    fn from(entry: DirEntry) -> Self {
        Self {
            path: entry.path,
            entry_type: entry.entry_type,
            size_bytes: entry.size,
        }
    }
}

path_action!(FilesystemExists, bool, "filesystem.exists");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMove {
    pub from: String,
    pub to: String,
}

crate::register_action!(FilesystemMove => (), "filesystem.move");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemRemove {
    pub path: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub recursive: bool,
}

crate::register_action!(FilesystemRemove => (), "filesystem.remove");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemExport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

crate::register_action!(FilesystemExport => RootSnapshotExport, "filesystem.export");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemListMounts {}

crate::register_action!(FilesystemListMounts => Vec<MountInfo>, "filesystem.listMounts");

impl Handler<FilesystemReadFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReadFile,
    ) -> impl std::future::Future<Output = anyhow::Result<FileBytes>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            let max_bytes = self.selected_transfer_limit(action.max_bytes)?;
            let content = self.runtime.vm().await?.read_file(&action.path).await?;
            self.validate_transfer_bytes("filesystem.readFile result", content.len(), max_bytes)?;
            Ok(FileBytes(content))
        })
    }
}

impl Handler<FilesystemWriteFile> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemWriteFile,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            self.validate_transfer_bytes(
                "filesystem.writeFile content",
                action.content.byte_len(),
                self.selected_transfer_limit(None)?,
            )?;
            self.runtime
                .vm()
                .await?
                .write_file(&action.path, action.content.into_core())
                .await
        })
    }
}

impl Handler<FilesystemReadFiles> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReadFiles,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<FilesystemReadResult>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_paths(&action.paths, &self.budgets)?;
            let max_bytes = self.selected_transfer_limit(action.max_bytes)?;
            let vm = self.runtime.vm().await?;
            let mut total = 0usize;
            let mut results = Vec::with_capacity(action.paths.len());
            for path in action.paths {
                match vm.read_file(&path).await {
                    Ok(content) => {
                        total = total.checked_add(content.len()).ok_or_else(|| {
                            anyhow::anyhow!(
                                "limit_exceeded: filesystem.readFiles byte count overflow"
                            )
                        })?;
                        self.validate_transfer_bytes(
                            "filesystem.readFiles result",
                            total,
                            max_bytes,
                        )?;
                        results.push(FilesystemReadResult {
                            path,
                            content: Some(FileBytes(content)),
                            error: None,
                        });
                    }
                    Err(error) => results.push(FilesystemReadResult {
                        path,
                        content: None,
                        error: Some(error.to_string()),
                    }),
                }
            }
            Ok(results)
        })
    }
}

impl Handler<FilesystemWriteFiles> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemWriteFiles,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<FilesystemWriteResult>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_count(
                "filesystem.writeFiles entries",
                action.entries.len(),
                self.budgets.filesystem.max_batch_paths,
            )?;
            let mut total = 0usize;
            let mut entries = Vec::with_capacity(action.entries.len());
            for entry in action.entries {
                validate_path(&entry.path, &self.budgets)?;
                total = total.checked_add(entry.content.byte_len()).ok_or_else(|| {
                    anyhow::anyhow!("limit_exceeded: filesystem.writeFiles byte count overflow")
                })?;
                entries.push(BatchWriteEntry {
                    path: entry.path,
                    content: entry.content.into_core(),
                });
            }
            self.validate_transfer_bytes(
                "filesystem.writeFiles content",
                total,
                self.selected_transfer_limit(None)?,
            )?;
            Ok(self
                .runtime
                .vm()
                .await?
                .write_files(entries)
                .await
                .into_iter()
                .map(|result| FilesystemWriteResult {
                    path: result.path,
                    success: result.success,
                    error: result.error,
                })
                .collect())
        })
    }
}

impl Handler<FilesystemStat> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemStat,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorFileStat>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            Ok(self.runtime.vm().await?.stat(&action.path).await?.into())
        })
    }
}

impl Handler<FilesystemMkdir> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemMkdir,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            self.runtime
                .vm()
                .await?
                .mkdir(
                    &action.path,
                    MkdirOptions {
                        recursive: action.recursive,
                    },
                )
                .await
        })
    }
}

impl Handler<FilesystemReaddir> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReaddir,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<String>>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            let entries = self.runtime.vm().await?.readdir(&action.path).await?;
            validate_directory_output(
                entries.len(),
                entries.iter().map(String::len).sum(),
                &self.budgets,
            )?;
            Ok(entries)
        })
    }
}

impl Handler<FilesystemReaddirEntries> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReaddirEntries,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<FilesystemDirectoryEntry>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            let entries = self
                .runtime
                .vm()
                .await?
                .readdir_entries(&action.path)
                .await?;
            validate_directory_output(
                entries.len(),
                entries.iter().map(|entry| entry.name.len()).sum(),
                &self.budgets,
            )?;
            Ok(entries
                .into_iter()
                .map(|entry| FilesystemDirectoryEntry {
                    name: entry.name,
                    is_directory: entry.is_directory,
                    is_symbolic_link: entry.is_symbolic_link,
                })
                .collect())
        })
    }
}

impl Handler<FilesystemReaddirRecursive> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReaddirRecursive,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<ActorDirectoryEntry>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            validate_recursive_depth(action.max_depth, &self.budgets)?;
            validate_count(
                "filesystem.readdirRecursive exclude",
                action.exclude.len(),
                self.budgets.filesystem.max_batch_paths,
            )?;
            for value in &action.exclude {
                validate_string(
                    "filesystem.readdirRecursive exclude entry",
                    value,
                    &self.budgets,
                )?;
            }
            let entries = self
                .runtime
                .vm()
                .await?
                .readdir_recursive(
                    &action.path,
                    ReaddirRecursiveOptions {
                        max_depth: action.max_depth,
                        exclude: action.exclude,
                    },
                )
                .await?;
            validate_directory_output(
                entries.len(),
                entries.iter().map(|entry| entry.path.len()).sum(),
                &self.budgets,
            )?;
            Ok(entries.into_iter().map(ActorDirectoryEntry::from).collect())
        })
    }
}

impl Handler<FilesystemExists> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemExists,
    ) -> impl std::future::Future<Output = anyhow::Result<bool>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            self.runtime.vm().await?.exists(&action.path).await
        })
    }
}

impl Handler<FilesystemMove> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemMove,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.from, &self.budgets)?;
            validate_path(&action.to, &self.budgets)?;
            self.runtime
                .vm()
                .await?
                .move_path(&action.from, &action.to)
                .await
        })
    }
}

impl Handler<FilesystemRemove> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemRemove,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path, &self.budgets)?;
            self.runtime
                .vm()
                .await?
                .remove(
                    &action.path,
                    RemoveOptions {
                        recursive: action.recursive,
                    },
                )
                .await
        })
    }
}

impl Handler<FilesystemExport> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemExport,
    ) -> impl std::future::Future<Output = anyhow::Result<RootSnapshotExport>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let max_bytes = self.selected_transfer_limit(action.max_bytes)?;
            self.runtime
                .vm()
                .await?
                .export_root_filesystem(max_bytes)
                .await
        })
    }
}

impl Handler<FilesystemListMounts> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _action: FilesystemListMounts,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<MountInfo>>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let mounts = self.runtime.vm().await?.list_mounts().await?;
            validate_count(
                "filesystem.listMounts result",
                mounts.len(),
                self.budgets.filesystem.max_batch_paths,
            )?;
            Ok(mounts)
        })
    }
}

impl AgentOsActor {
    pub(crate) fn selected_transfer_limit(&self, requested: Option<usize>) -> Result<usize> {
        self.budgets.transfer_limit(requested)
    }
    pub(crate) fn validate_transfer_bytes(
        &self,
        label: &'static str,
        observed: usize,
        limit: usize,
    ) -> Result<()> {
        self.budgets.check_transfer(label, observed, limit)
    }
}

#[cfg(test)]
fn transfer_limit(requested: Option<usize>) -> Result<usize> {
    let limit = requested.unwrap_or(MAX_TRANSFER_BYTES);
    if limit == 0 || limit > MAX_TRANSFER_BYTES {
        bail!(
            "limit_exceeded: maxBytes must be between 1 and {MAX_TRANSFER_BYTES}; large-file streaming is not implemented"
        );
    }
    Ok(limit)
}

fn validate_paths(paths: &[String], budgets: &crate::HostedActorBudgets) -> Result<()> {
    validate_count(
        "filesystem path count",
        paths.len(),
        budgets.filesystem.max_batch_paths,
    )?;
    for path in paths {
        validate_path(path, budgets)?;
    }
    Ok(())
}

fn validate_path(path: &str, budgets: &crate::HostedActorBudgets) -> Result<()> {
    validate_string("filesystem path", path, budgets)
}

fn validate_string(label: &str, value: &str, budgets: &crate::HostedActorBudgets) -> Result<()> {
    if value.is_empty() {
        bail!("limit_exceeded: {label} must not be empty");
    }
    budgets.path_bound().check(label, value.len())
}

fn validate_count(label: &str, actual: usize, limit: ValidationBound) -> Result<()> {
    limit.check(label, actual)
}

fn validate_bytes(label: &str, actual: usize, limit: ValidationBound) -> Result<()> {
    limit.check(label, actual)
}

fn validate_directory_output(
    entry_count: usize,
    encoded_name_bytes: usize,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    validate_count(
        "filesystem directory result entries",
        entry_count,
        budgets.filesystem.max_directory_entries,
    )?;
    validate_bytes(
        "filesystem directory result paths",
        encoded_name_bytes,
        budgets.filesystem.max_directory_result_bytes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_and_collection_limits_are_bounded() {
        assert_eq!(
            transfer_limit(None).expect("default limit"),
            MAX_TRANSFER_BYTES
        );
        assert!(transfer_limit(Some(0)).is_err());
        assert!(transfer_limit(Some(MAX_TRANSFER_BYTES + 1)).is_err());
        assert!(validate_paths(
            &vec!["/a".into(); MAX_BATCH_PATHS],
            &crate::HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_ok());
        assert!(validate_paths(
            &vec!["/a".into(); MAX_BATCH_PATHS + 1],
            &crate::HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
    }

    #[test]
    fn byte_content_uses_a_cbor_byte_string() {
        let mut encoded = Vec::new();
        ciborium::into_writer(&FileBytes(vec![1, 2, 3]), &mut encoded).expect("encode bytes");
        assert_eq!(encoded, vec![0x43, 1, 2, 3]);
    }

    #[test]
    fn file_metadata_names_byte_counts() {
        let stat = ActorFileStat::from(VirtualStat {
            mode: 0,
            size: 42,
            blocks: 0,
            dev: 0,
            rdev: 0,
            is_directory: false,
            is_symbolic_link: false,
            atime_ms: 0.0,
            mtime_ms: 0.0,
            ctime_ms: 0.0,
            birthtime_ms: 0.0,
            ino: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
        });
        let encoded = serde_json::to_value(stat).expect("encode file stat");
        assert_eq!(encoded["sizeBytes"], 42);
        assert!(encoded.get("size").is_none());

        let entry = ActorDirectoryEntry::from(DirEntry {
            path: "/file".into(),
            entry_type: DirEntryType::File,
            size: 42,
        });
        let encoded = serde_json::to_value(entry).expect("encode directory entry");
        assert_eq!(encoded["path"], "/file");
        assert_eq!(encoded["type"], "file");
        assert_eq!(encoded["sizeBytes"], 42);
        assert!(encoded.get("size").is_none());
        assert!(encoded.get("entryType").is_none());
    }
}

#[cfg(test)]
#[path = "../tests/unit/filesystem_worker_budgets.rs"]
mod worker_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/filesystem_remaining_budgets.rs"]
mod remaining_budget_tests;

fn validate_recursive_depth(selected: Option<u32>, budgets: &HostedActorBudgets) -> Result<()> {
    if let Some(depth) = selected {
        budgets
            .filesystem
            .max_recursive_depth
            .check("filesystem.readdirRecursive maxDepth", depth as usize)?;
    }
    Ok(())
}
