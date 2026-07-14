use agentos_kernel::vfs::{
    normalize_path, MemoryFileSystem, SingleSymlinkFileSystem, VirtualFileSystem, VirtualTimeSpec,
    VirtualUtimeSpec,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use vfs::package_format::generated::v1::{self, TarEntryKind};
use vfs::package_format::versioned::{decode_mount_index, decode_package_manifest};
use vfs::package_format::{parse_aospkg_header, validate_mount_range};

pub const MAX_BROWSER_PROJECTED_PACKAGES_PER_VM: usize = 4_096;
/// Aggregate retained package-source bound across all package-link requests.
/// The wire codec independently enforces its exact per-frame bound, including
/// the request envelope, before package projection begins.
pub const MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM: usize = 64 * 1024 * 1024;
/// Bounds copied file content after package and `provides.files` projection.
/// The entry-count and mount-count limits separately bound metadata work.
pub const MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM: usize = 128 * 1024 * 1024;
pub const MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM: usize = 200_000;
pub const MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM: usize = 4_096;
pub const DEFAULT_BROWSER_PACKAGES_MOUNT_ROOT: &str = "/opt/agentos";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProjectedCommand {
    pub name: String,
    pub guest_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProjectedPackageAgent {
    pub id: String,
    pub acp_entrypoint: String,
    pub adapter_entrypoint: String,
    pub snapshot: bool,
    pub env: BTreeMap<String, String>,
    pub launch_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProjectedPackage {
    pub name: String,
    pub version: String,
    pub commands: Vec<String>,
    pub projected_commands: Vec<BrowserProjectedCommand>,
    pub agent: Option<BrowserProjectedPackageAgent>,
    pub applied_mounts: usize,
    pub provided_env: BTreeMap<String, String>,
    pub snapshot_bundle_path: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PreparedBrowserPackage {
    pub projection: BrowserProjectedPackage,
    pub mounts: Vec<PreparedBrowserPackageMount>,
    pub source_bytes: Vec<u8>,
    pub index_entries: usize,
    pub materialized_bytes: usize,
}

#[derive(Debug)]
pub(crate) enum PreparedBrowserPackageMount {
    Files {
        guest_path: String,
        filesystem: MemoryFileSystem,
    },
    Symlink {
        guest_path: String,
        filesystem: SingleSymlinkFileSystem,
    },
}

impl PreparedBrowserPackageMount {
    pub fn guest_path(&self) -> &str {
        match self {
            Self::Files { guest_path, .. } | Self::Symlink { guest_path, .. } => guest_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowserPackageProjectionError {
    Invalid(String),
    LimitExceeded {
        limit: &'static str,
        capacity: usize,
        how_to_raise: &'static str,
    },
}

impl BrowserPackageProjectionError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    fn limit(limit: &'static str, capacity: usize, how_to_raise: &'static str) -> Self {
        Self::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        }
    }
}

impl fmt::Display for BrowserPackageProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::LimitExceeded {
                limit,
                capacity,
                how_to_raise,
            } => write!(
                f,
                "browser sidecar limit {limit} reached at configured capacity {capacity}; {how_to_raise}"
            ),
        }
    }
}

pub(crate) fn prepare_aospkg_bytes(
    bytes: Vec<u8>,
    mount_root: &str,
) -> Result<PreparedBrowserPackage, BrowserPackageProjectionError> {
    let header = parse_aospkg_header(&bytes)
        .map_err(|error| invalid_package("parse header", error.to_string()))?;
    let manifest = decode_package_manifest(&bytes[header.manifest.clone()])
        .map_err(|error| invalid_package("decode manifest", error.to_string()))?;
    let index = decode_mount_index(&bytes[header.index.clone()])
        .map_err(|error| invalid_package("decode mount index", error.to_string()))?;

    validate_leaf("package name", &manifest.name)?;
    validate_leaf("package version", &manifest.version)?;
    validate_sorted_index(&index.tar_entries)?;

    let materialized_bytes =
        projected_materialized_bytes(&index.tar_entries, manifest.provides.as_ref())?;
    if materialized_bytes > MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM {
        return Err(BrowserPackageProjectionError::limit(
            "max_projected_package_materialized_bytes_per_vm",
            MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM,
            "raise the browser package projection materialized-byte limit in the sidecar",
        ));
    }
    let expected_mounts = 2usize
        .checked_add(manifest.commands.len())
        .and_then(|count| count.checked_add(manifest.man_pages.len()))
        .and_then(|count| {
            count.checked_add(
                manifest
                    .provides
                    .as_ref()
                    .map_or(0, |provides| provides.files.len()),
            )
        })
        .ok_or_else(|| {
            BrowserPackageProjectionError::limit(
                "max_projected_package_mounts_per_vm",
                MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM,
                "raise the browser package projection mount limit in the sidecar",
            )
        })?;
    if expected_mounts > MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM {
        return Err(BrowserPackageProjectionError::limit(
            "max_projected_package_mounts_per_vm",
            MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM,
            "raise the browser package projection mount limit in the sidecar",
        ));
    }

    let index_entries = index.tar_entries.len();
    let indexed_paths = index
        .tar_entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.kind.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut command_names = BTreeSet::new();
    for command in &manifest.commands {
        validate_leaf("package command", &command.command)?;
        validate_relative_entry(&command.command, &command.entry)?;
        if !command_names.insert(command.command.clone()) {
            return Err(BrowserPackageProjectionError::invalid(format!(
                "duplicate package command {:?}",
                command.command
            )));
        }
        let entry_path = format!("/{}", command.entry);
        match indexed_paths.get(entry_path.as_str()) {
            Some(TarEntryKind::File | TarEntryKind::Symlink) => {}
            Some(TarEntryKind::Directory) => {
                return Err(BrowserPackageProjectionError::invalid(format!(
                    "package command {:?} targets directory {:?}",
                    command.command, command.entry
                )));
            }
            None => {
                return Err(BrowserPackageProjectionError::invalid(format!(
                    "package command {:?} targets missing entry {:?}",
                    command.command, command.entry
                )));
            }
        }
    }

    let agent = prepare_agent(&manifest, &command_names, mount_root)?;
    let package_name = manifest.name.clone();
    let package_version = manifest.version.clone();
    let package_root = format!("{mount_root}/pkgs/{package_name}");
    let version_path = format!("{package_root}/{package_version}");
    let filesystem = materialize_mount_filesystem(&bytes, &header, &index.tar_entries, "/")?;

    let mut mounts = vec![PreparedBrowserPackageMount::Files {
        guest_path: version_path,
        filesystem,
    }];
    mounts.push(PreparedBrowserPackageMount::Symlink {
        guest_path: format!("{package_root}/current"),
        filesystem: SingleSymlinkFileSystem::new(package_version.clone()),
    });
    for command in &manifest.commands {
        mounts.push(PreparedBrowserPackageMount::Symlink {
            guest_path: format!("{mount_root}/bin/{}", command.command),
            filesystem: SingleSymlinkFileSystem::new(format!(
                "../pkgs/{package_name}/current/{}",
                command.entry
            )),
        });
    }
    for page in &manifest.man_pages {
        validate_leaf("man page section", &page.section)?;
        validate_leaf("man page name", &page.page)?;
        let source = format!("/share/man/{}/{}", page.section, page.page);
        if !indexed_paths.contains_key(source.as_str()) {
            return Err(BrowserPackageProjectionError::invalid(format!(
                "package man page target is missing: {source}"
            )));
        }
        mounts.push(PreparedBrowserPackageMount::Symlink {
            guest_path: format!("{mount_root}/share/man/{}/{}", page.section, page.page),
            filesystem: SingleSymlinkFileSystem::new(format!(
                "../../../pkgs/{package_name}/current/share/man/{}/{}",
                page.section, page.page
            )),
        });
    }
    let provided_env = manifest
        .provides
        .as_ref()
        .map(|provides| provides.env.clone().into_iter().collect())
        .unwrap_or_default();
    append_provides_file_mounts(
        &mut mounts,
        &bytes,
        &header,
        &index.tar_entries,
        manifest.provides.as_ref(),
        &package_name,
    )?;

    let projected_commands = command_names
        .iter()
        .map(|name| BrowserProjectedCommand {
            name: name.clone(),
            guest_path: format!("{mount_root}/bin/{name}"),
        })
        .collect();
    let applied_mounts = mounts.len();

    Ok(PreparedBrowserPackage {
        projection: BrowserProjectedPackage {
            name: package_name,
            version: package_version,
            commands: command_names.into_iter().collect(),
            projected_commands,
            agent,
            applied_mounts,
            provided_env,
            snapshot_bundle_path: manifest.snapshot_bundle_path,
        },
        mounts,
        source_bytes: bytes,
        index_entries,
        materialized_bytes,
    })
}

fn projected_materialized_bytes(
    entries: &[v1::TarEntry],
    provides: Option<&v1::ProvidesBlock>,
) -> Result<usize, BrowserPackageProjectionError> {
    let sum_files = |source_root: &str| {
        entries
            .iter()
            .filter(|entry| matches!(entry.kind, TarEntryKind::File))
            .filter(|entry| entry_relative_to_source(entry, source_root).is_some())
            .try_fold(0usize, |total, entry| {
                let size = usize::try_from(entry.size).map_err(|_| {
                    BrowserPackageProjectionError::invalid(format!(
                        "package entry size does not fit memory for {}",
                        entry.path
                    ))
                })?;
                total.checked_add(size).ok_or_else(|| {
                    BrowserPackageProjectionError::invalid(
                        "package materialized file byte count overflowed",
                    )
                })
            })
    };

    let mut total = sum_files("/")?;
    if let Some(provides) = provides {
        for file in &provides.files {
            let source = normalize_provides_source(&file.source)?;
            total = total.checked_add(sum_files(&source)?).ok_or_else(|| {
                BrowserPackageProjectionError::invalid(
                    "package materialized file byte count overflowed",
                )
            })?;
            if total > MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM {
                return Err(BrowserPackageProjectionError::limit(
                    "max_projected_package_materialized_bytes_per_vm",
                    MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM,
                    "raise the browser package projection materialized-byte limit in the sidecar",
                ));
            }
        }
    }
    Ok(total)
}

fn prepare_agent(
    manifest: &v1::PackageManifest,
    commands: &BTreeSet<String>,
    mount_root: &str,
) -> Result<Option<BrowserProjectedPackageAgent>, BrowserPackageProjectionError> {
    let Some(agent) = &manifest.agent else {
        return Ok(None);
    };
    validate_leaf("agent ACP entrypoint", &agent.acp_entrypoint)?;
    if !commands.contains(&agent.acp_entrypoint) {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "agent acpEntrypoint {:?} is not one of {}'s commands",
            agent.acp_entrypoint, manifest.name
        )));
    }
    Ok(Some(BrowserProjectedPackageAgent {
        id: manifest.name.clone(),
        acp_entrypoint: agent.acp_entrypoint.clone(),
        adapter_entrypoint: format!("{mount_root}/bin/{}", agent.acp_entrypoint),
        snapshot: agent.snapshot,
        env: agent.env.clone().into_iter().collect(),
        launch_args: agent.launch_args.clone(),
    }))
}

fn materialize_mount_filesystem(
    bytes: &[u8],
    header: &vfs::package_format::AospkgHeader,
    entries: &[v1::TarEntry],
    source_root: &str,
) -> Result<MemoryFileSystem, BrowserPackageProjectionError> {
    let mut filesystem = MemoryFileSystem::new();
    for source_entry in entries {
        let Some(entry) = entry_relative_to_source(source_entry, source_root) else {
            continue;
        };
        validate_index_path(&entry.path)?;
        ensure_parent_directories(&mut filesystem, &entry.path)?;
        let _mtime_ms = u64::try_from(entry.mtime)
            .map_err(|_| {
                BrowserPackageProjectionError::invalid(format!(
                    "negative package entry mtime for {}",
                    entry.path
                ))
            })?
            .checked_mul(1_000)
            .ok_or_else(|| {
                BrowserPackageProjectionError::invalid(format!(
                    "package entry mtime overflows milliseconds for {}",
                    entry.path
                ))
            })?;
        match entry.kind {
            TarEntryKind::Directory => {
                if entry.path == "/" {
                    filesystem
                        .chmod("/", entry.mode)
                        .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
                } else {
                    filesystem
                        .create_dir_with_mode(&entry.path, Some(entry.mode))
                        .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
                }
                filesystem
                    .chown(&entry.path, entry.uid, entry.gid)
                    .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
            }
            TarEntryKind::File => {
                let range = validate_mount_range(header, entry.offset, entry.size)
                    .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
                filesystem
                    .write_file_with_mode(&entry.path, bytes[range].to_vec(), Some(entry.mode))
                    .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
                filesystem
                    .chown(&entry.path, entry.uid, entry.gid)
                    .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
            }
            TarEntryKind::Symlink => {
                let target = entry.link_target.as_deref().ok_or_else(|| {
                    BrowserPackageProjectionError::invalid(format!(
                        "missing linkTarget for package symlink {}",
                        entry.path
                    ))
                })?;
                filesystem
                    .symlink_with_metadata(target, &entry.path, entry.mode, entry.uid, entry.gid)
                    .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
            }
        }
        filesystem
            .utimes_spec(
                &entry.path,
                VirtualUtimeSpec::Set(VirtualTimeSpec {
                    sec: entry.mtime,
                    nsec: 0,
                }),
                VirtualUtimeSpec::Set(VirtualTimeSpec {
                    sec: entry.mtime,
                    nsec: 0,
                }),
                !matches!(entry.kind, TarEntryKind::Symlink),
            )
            .map_err(|error| invalid_entry(&entry.path, error.to_string()))?;
    }
    Ok(filesystem)
}

fn append_provides_file_mounts(
    mounts: &mut Vec<PreparedBrowserPackageMount>,
    bytes: &[u8],
    header: &vfs::package_format::AospkgHeader,
    entries: &[v1::TarEntry],
    provides: Option<&v1::ProvidesBlock>,
    package_name: &str,
) -> Result<(), BrowserPackageProjectionError> {
    let Some(provides) = provides else {
        return Ok(());
    };
    let mut targets = BTreeSet::new();
    for file in &provides.files {
        let source = normalize_provides_source(&file.source)?;
        let target = validate_absolute_mount_path("package provides target", &file.target)?;
        if target == "/" {
            return Err(BrowserPackageProjectionError::invalid(format!(
                "package {package_name:?} provides file target must not replace the VM root"
            )));
        }
        if !targets.insert(target.clone()) {
            return Err(BrowserPackageProjectionError::invalid(format!(
                "package {package_name:?} has duplicate provides file target {target:?}"
            )));
        }
        let exact_kind = entries
            .iter()
            .find(|entry| entry.path == source)
            .map(|entry| &entry.kind);
        let child_prefix = if source == "/" {
            String::from("/")
        } else {
            format!("{source}/")
        };
        let has_children = entries
            .iter()
            .any(|entry| entry.path.starts_with(&child_prefix) && entry.path != source);
        match exact_kind {
            Some(TarEntryKind::File | TarEntryKind::Symlink) => {
                tracing::warn!(
                    package = package_name,
                    source,
                    target,
                    "package provides file source is not a directory; skipping"
                );
                continue;
            }
            Some(TarEntryKind::Directory) => {}
            None if has_children || source == "/" => {}
            None => {
                return Err(BrowserPackageProjectionError::invalid(format!(
                    "package provides file source is missing: package {package_name:?} source {:?} target {target:?}",
                    file.source
                )));
            }
        }
        mounts.push(PreparedBrowserPackageMount::Files {
            guest_path: target,
            filesystem: materialize_mount_filesystem(bytes, header, entries, &source)?,
        });
    }
    Ok(())
}

fn entry_relative_to_source(entry: &v1::TarEntry, source_root: &str) -> Option<v1::TarEntry> {
    if source_root == "/" {
        return Some(entry.clone());
    }
    if entry.path == source_root {
        let mut entry = entry.clone();
        entry.path = String::from("/");
        return Some(entry);
    }
    let suffix = entry.path.strip_prefix(&format!("{source_root}/"))?;
    let mut entry = entry.clone();
    entry.path = format!("/{suffix}");
    Some(entry)
}

fn normalize_provides_source(source: &str) -> Result<String, BrowserPackageProjectionError> {
    if source.trim().is_empty() {
        return Ok(String::from("/"));
    }
    let candidate = if source.starts_with('/') {
        source.to_owned()
    } else {
        format!("/{source}")
    };
    if normalize_path(&candidate) != candidate {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "package provides source must be a canonical path inside the package, got {source:?}"
        )));
    }
    Ok(candidate)
}

pub(crate) fn normalize_packages_mount_root(
    mount_root: Option<&str>,
) -> Result<String, BrowserPackageProjectionError> {
    match mount_root {
        None | Some("") => Ok(String::from(DEFAULT_BROWSER_PACKAGES_MOUNT_ROOT)),
        Some(value) => {
            let value = validate_absolute_mount_path("packages mount root", value)?;
            if value == "/" {
                return Err(BrowserPackageProjectionError::invalid(
                    "packages mount root must not replace the VM root",
                ));
            }
            Ok(value)
        }
    }
}

fn validate_absolute_mount_path(
    label: &str,
    path: &str,
) -> Result<String, BrowserPackageProjectionError> {
    if path.is_empty()
        || !path.starts_with('/')
        || normalize_path(path) != path
        || path
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "{label} must be a non-empty canonical absolute path, got {path:?}"
        )));
    }
    Ok(path.to_owned())
}

fn ensure_parent_directories(
    filesystem: &mut MemoryFileSystem,
    path: &str,
) -> Result<(), BrowserPackageProjectionError> {
    let components = path
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let mut parent = String::new();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        parent.push('/');
        parent.push_str(component);
        match filesystem.lstat(&parent) {
            Ok(stat) if stat.is_directory && !stat.is_symbolic_link => {}
            Ok(_) => {
                return Err(BrowserPackageProjectionError::invalid(format!(
                    "package entry parent is not a directory: {parent}"
                )));
            }
            Err(error) if error.code() == "ENOENT" => filesystem
                .create_dir_with_mode(&parent, Some(0o040755))
                .map_err(|error| invalid_entry(path, error.to_string()))?,
            Err(error) => return Err(invalid_entry(path, error.to_string())),
        }
    }
    Ok(())
}

fn validate_sorted_index(entries: &[v1::TarEntry]) -> Result<(), BrowserPackageProjectionError> {
    validate_index_entry_count(entries.len())?;
    for pair in entries.windows(2) {
        if pair[0].path >= pair[1].path {
            return Err(BrowserPackageProjectionError::invalid(format!(
                ".aospkg mount index is not sorted by canonical path: {:?} before {:?}",
                pair[0].path, pair[1].path
            )));
        }
    }
    Ok(())
}

fn validate_index_entry_count(entry_count: usize) -> Result<(), BrowserPackageProjectionError> {
    if entry_count > MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM {
        return Err(BrowserPackageProjectionError::limit(
            "max_projected_package_entries_per_vm",
            MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM,
            "raise the browser package projection entry limit in the sidecar",
        ));
    }
    Ok(())
}

fn validate_index_path(path: &str) -> Result<(), BrowserPackageProjectionError> {
    if path.is_empty() || !path.starts_with('/') || normalize_path(path) != path {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "non-canonical package index path {path:?}"
        )));
    }
    Ok(())
}

fn validate_leaf(label: &str, value: &str) -> Result<(), BrowserPackageProjectionError> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.contains('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "{label} must be one non-empty canonical path component, got {value:?}"
        )));
    }
    Ok(())
}

fn validate_relative_entry(
    command: &str,
    entry: &str,
) -> Result<(), BrowserPackageProjectionError> {
    if entry.is_empty()
        || entry.starts_with('/')
        || entry
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
        || normalize_path(entry) != format!("/{entry}")
    {
        return Err(BrowserPackageProjectionError::invalid(format!(
            "package command {command:?} has non-canonical relative entry {entry:?}"
        )));
    }
    Ok(())
}

fn invalid_package(stage: &str, detail: String) -> BrowserPackageProjectionError {
    BrowserPackageProjectionError::invalid(format!("invalid .aospkg: {stage}: {detail}"))
}

fn invalid_entry(path: &str, detail: String) -> BrowserPackageProjectionError {
    BrowserPackageProjectionError::invalid(format!(
        "invalid .aospkg mount entry {path:?}: {detail}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_index_entry_overflow_is_a_typed_limit_without_materializing_entries() {
        let error = validate_index_entry_count(
            MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM.saturating_add(1),
        )
        .expect_err("entry capacity plus one must be rejected");

        assert_eq!(
            error,
            BrowserPackageProjectionError::LimitExceeded {
                limit: "max_projected_package_entries_per_vm",
                capacity: MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM,
                how_to_raise: "raise the browser package projection entry limit in the sidecar",
            }
        );
        assert!(error
            .to_string()
            .contains("raise the browser package projection entry limit in the sidecar"));
    }
}
