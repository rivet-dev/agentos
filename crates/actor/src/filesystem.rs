use std::sync::Arc;

use agentos_actor_contract::filesystem::*;
#[cfg(test)]
use agentos_client::{DirEntry, DirEntryType, VirtualStat};
use agentos_client::{
    MkdirOptions, MountInfo, ReaddirRecursiveOptions, RemoveOptions, RootSnapshotExport,
};
use anyhow::{bail, Result};
use rivetkit::{Ctx, Handles};

use crate::actions::BoxFuture;
use crate::AgentOsActor;

const MAX_PATH_BYTES: usize = 4 * 1024;
const MAX_BATCH_PATHS: usize = 128;
const MAX_TRANSFER_BYTES: usize = 768 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 4_096;
const MAX_DIRECTORY_RESULT_BYTES: usize = 512 * 1024;
const MAX_RECURSION_DEPTH: u32 = 64;

crate::register_contract_action!(FilesystemReadFile);
crate::register_contract_action!(FilesystemWriteFile);
crate::register_contract_action!(FilesystemReadFiles);
crate::register_contract_action!(FilesystemWriteFiles);
crate::register_contract_action!(FilesystemStat);
crate::register_contract_action!(FilesystemMkdir);
crate::register_contract_action!(FilesystemReaddir);
crate::register_contract_action!(FilesystemReaddirEntries);
crate::register_contract_action!(FilesystemReaddirRecursive);
crate::register_contract_action!(FilesystemExists);
crate::register_contract_action!(FilesystemMove);
crate::register_contract_action!(FilesystemRemove);
crate::register_contract_action!(FilesystemExport);
crate::register_contract_action!(FilesystemListMounts);

impl Handles<FilesystemReadFile> for AgentOsActor {
    type Future = BoxFuture<FileBytes>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemReadFile) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            let max_bytes = transfer_limit(action.max_bytes)?;
            let content = self.runtime.vm().await?.read_file(&action.path).await?;
            validate_bytes("filesystem.readFile result", content.len(), max_bytes)?;
            Ok(FileBytes(content))
        })
    }
}

impl Handles<FilesystemWriteFile> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemWriteFile) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            validate_bytes(
                "filesystem.writeFile content",
                action.content.byte_len(),
                MAX_TRANSFER_BYTES,
            )?;
            self.runtime
                .vm()
                .await?
                .write_file(&action.path, action.content)
                .await
        })
    }
}

impl Handles<FilesystemReadFiles> for AgentOsActor {
    type Future = BoxFuture<Vec<FilesystemReadResult>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemReadFiles) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_paths(&action.paths)?;
            let max_bytes = transfer_limit(action.max_bytes)?;
            let vm = self.runtime.vm().await?;
            let read_results = vm.read_files(action.paths).await;
            let mut total = 0usize;
            let mut results = Vec::with_capacity(read_results.len());
            for result in read_results {
                match result.content {
                    Some(content) => {
                        total = total.checked_add(content.len()).ok_or_else(|| {
                            anyhow::anyhow!(
                                "limit_exceeded: filesystem.readFiles byte count overflow"
                            )
                        })?;
                        validate_bytes("filesystem.readFiles result", total, max_bytes)?;
                        results.push(FilesystemReadResult {
                            path: result.path,
                            content: Some(FileBytes(content)),
                            error: result.error,
                        });
                    }
                    None => results.push(FilesystemReadResult {
                        path: result.path,
                        content: None,
                        error: result.error,
                    }),
                }
            }
            Ok(results)
        })
    }
}

impl Handles<FilesystemWriteFiles> for AgentOsActor {
    type Future = BoxFuture<Vec<FilesystemWriteResult>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemWriteFiles) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_count(
                "filesystem.writeFiles entries",
                action.entries.len(),
                MAX_BATCH_PATHS,
            )?;
            let mut total = 0usize;
            for entry in &action.entries {
                validate_path(&entry.path)?;
                total = total.checked_add(entry.content.byte_len()).ok_or_else(|| {
                    anyhow::anyhow!("limit_exceeded: filesystem.writeFiles byte count overflow")
                })?;
            }
            validate_bytes("filesystem.writeFiles content", total, MAX_TRANSFER_BYTES)?;
            Ok(self.runtime.vm().await?.write_files(action.entries).await)
        })
    }
}

impl Handles<FilesystemStat> for AgentOsActor {
    type Future = BoxFuture<ActorFileStat>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemStat) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            self.runtime.vm().await?.stat(&action.path).await
        })
    }
}

impl Handles<FilesystemMkdir> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemMkdir) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
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

impl Handles<FilesystemReaddir> for AgentOsActor {
    type Future = BoxFuture<Vec<String>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemReaddir) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            let entries = self.runtime.vm().await?.readdir(&action.path).await?;
            validate_directory_output(entries.len(), entries.iter().map(String::len).sum())?;
            Ok(entries)
        })
    }
}

impl Handles<FilesystemReaddirEntries> for AgentOsActor {
    type Future = BoxFuture<Vec<FilesystemDirectoryEntry>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemReaddirEntries) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            let entries = self
                .runtime
                .vm()
                .await?
                .readdir_entries(&action.path)
                .await?;
            validate_directory_output(
                entries.len(),
                entries.iter().map(|entry| entry.name.len()).sum(),
            )?;
            Ok(entries)
        })
    }
}

impl Handles<FilesystemReaddirRecursive> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorDirectoryEntry>>;

    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: FilesystemReaddirRecursive,
    ) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            if action
                .max_depth
                .is_some_and(|depth| depth > MAX_RECURSION_DEPTH)
            {
                bail!(
                    "limit_exceeded: filesystem.readdirRecursive maxDepth exceeds {MAX_RECURSION_DEPTH}; lower maxDepth"
                );
            }
            validate_count(
                "filesystem.readdirRecursive exclude",
                action.exclude.len(),
                MAX_BATCH_PATHS,
            )?;
            for value in &action.exclude {
                validate_string("filesystem.readdirRecursive exclude entry", value)?;
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
            )?;
            Ok(entries)
        })
    }
}

impl Handles<FilesystemExists> for AgentOsActor {
    type Future = BoxFuture<bool>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemExists) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
            self.runtime.vm().await?.exists(&action.path).await
        })
    }
}

impl Handles<FilesystemMove> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemMove) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.from)?;
            validate_path(&action.to)?;
            self.runtime
                .vm()
                .await?
                .move_path(&action.from, &action.to)
                .await
        })
    }
}

impl Handles<FilesystemRemove> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemRemove) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_path(&action.path)?;
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

impl Handles<FilesystemExport> for AgentOsActor {
    type Future = BoxFuture<RootSnapshotExport>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: FilesystemExport) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let max_bytes = transfer_limit(action.max_bytes)?;
            self.runtime
                .vm()
                .await?
                .export_root_filesystem(max_bytes)
                .await
        })
    }
}

impl Handles<FilesystemListMounts> for AgentOsActor {
    type Future = BoxFuture<Vec<MountInfo>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: FilesystemListMounts) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let mounts = self.runtime.vm().await?.list_mounts().await?;
            validate_count(
                "filesystem.listMounts result",
                mounts.len(),
                MAX_BATCH_PATHS,
            )?;
            Ok(mounts)
        })
    }
}

fn transfer_limit(requested: Option<usize>) -> Result<usize> {
    let limit = requested.unwrap_or(MAX_TRANSFER_BYTES);
    if limit == 0 || limit > MAX_TRANSFER_BYTES {
        bail!(
            "limit_exceeded: maxBytes must be between 1 and {MAX_TRANSFER_BYTES}; large-file streaming is not implemented"
        );
    }
    Ok(limit)
}

fn validate_paths(paths: &[String]) -> Result<()> {
    validate_count("filesystem path count", paths.len(), MAX_BATCH_PATHS)?;
    for path in paths {
        validate_path(path)?;
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    validate_string("filesystem path", path)
}

fn validate_string(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES {
        bail!("limit_exceeded: {label} must contain 1..={MAX_PATH_BYTES} bytes; reduce the value");
    }
    Ok(())
}

fn validate_count(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual}, limit is {limit}; reduce the collection");
    }
    Ok(())
}

fn validate_bytes(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual} bytes, limit is {limit}; reduce the payload");
    }
    Ok(())
}

fn validate_directory_output(entry_count: usize, encoded_name_bytes: usize) -> Result<()> {
    validate_count(
        "filesystem directory result entries",
        entry_count,
        MAX_DIRECTORY_ENTRIES,
    )?;
    validate_bytes(
        "filesystem directory result paths",
        encoded_name_bytes,
        MAX_DIRECTORY_RESULT_BYTES,
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
        assert!(validate_paths(&vec!["/a".into(); MAX_BATCH_PATHS]).is_ok());
        assert!(validate_paths(&vec!["/a".into(); MAX_BATCH_PATHS + 1]).is_err());
    }

    #[test]
    fn byte_content_uses_a_cbor_byte_string() {
        let mut encoded = Vec::new();
        ciborium::into_writer(&FileBytes(vec![1, 2, 3]), &mut encoded).expect("encode bytes");
        assert_eq!(encoded, vec![0x43, 1, 2, 3]);
    }

    #[test]
    fn file_metadata_names_byte_counts() {
        let stat: ActorFileStat = VirtualStat {
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
        };
        let encoded = serde_json::to_value(stat).expect("encode file stat");
        assert_eq!(encoded["sizeBytes"], 42);
        assert!(encoded.get("size").is_none());

        let entry: ActorDirectoryEntry = DirEntry {
            path: "/file".into(),
            entry_type: DirEntryType::File,
            size: 42,
        };
        let encoded = serde_json::to_value(entry).expect("encode directory entry");
        assert_eq!(encoded["path"], "/file");
        assert_eq!(encoded["type"], "file");
        assert_eq!(encoded["sizeBytes"], 42);
        assert!(encoded.get("size").is_none());
        assert!(encoded.get("entryType").is_none());
    }
}
