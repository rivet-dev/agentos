//! Transport-safe filesystem action DTOs.
//!
//! These types are shared by the hosted contract and its Rust adapter. They
//! contain no Rivet lifecycle behavior or filesystem policy.

use agentos_client::{
    BatchWriteEntry, BatchWriteResult, DirEntry, FileContent, MountInfo, RootSnapshotExport,
    VirtualDirEntry, VirtualStat,
};
use serde::{Deserialize, Serialize};

pub use agentos_client::{
    BatchWriteEntry as FilesystemWriteEntry, BatchWriteResult as FilesystemWriteResult,
    DirEntry as ActorDirectoryEntry, FileContent as FileContentInput,
    VirtualDirEntry as FilesystemDirectoryEntry, VirtualStat as ActorFileStat,
};

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

macro_rules! path_action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub path: String,
        }

        action!($name => $output, $wire_name);
    };
}

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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemReadFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

action!(FilesystemReadFile => FileBytes, "filesystem.readFile");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemWriteFile {
    pub path: String,
    pub content: FileContent,
}

action!(FilesystemWriteFile => (), "filesystem.writeFile");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemReadFiles {
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

action!(FilesystemReadFiles => Vec<FilesystemReadResult>, "filesystem.readFiles");

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
    pub entries: Vec<BatchWriteEntry>,
}

action!(FilesystemWriteFiles => Vec<BatchWriteResult>, "filesystem.writeFiles");

path_action!(FilesystemStat => VirtualStat, "filesystem.stat");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMkdir {
    pub path: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub recursive: bool,
}

action!(FilesystemMkdir => (), "filesystem.mkdir");

path_action!(FilesystemReaddir => Vec<String>, "filesystem.readdir");
path_action!(FilesystemReaddirEntries => Vec<VirtualDirEntry>, "filesystem.readdirEntries");

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

action!(FilesystemReaddirRecursive => Vec<DirEntry>, "filesystem.readdirRecursive");

path_action!(FilesystemExists => bool, "filesystem.exists");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMove {
    pub from: String,
    pub to: String,
}

action!(FilesystemMove => (), "filesystem.move");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemRemove {
    pub path: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub recursive: bool,
}

action!(FilesystemRemove => (), "filesystem.remove");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemExport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

action!(FilesystemExport => RootSnapshotExport, "filesystem.export");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemListMounts {}

action!(FilesystemListMounts => Vec<MountInfo>, "filesystem.listMounts");
