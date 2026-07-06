//! agentOS package projection.
//!
//! Packages are mounted directly from their uncompressed `package.tar` files
//! when available. Transition fixtures may still project an unpacked package
//! dir as a read-only package leaf. Tar packages stay on the zero-extraction
//! fast path: the VFS indexes headers once and returns mmap-backed byte ranges.
//! The projection also serves `bin/*`, `current`, manpage aliases, and
//! `provides.files` as virtual mounts; it never writes a physical symlink farm.
//!
//! The projection is deliberately granular. Each package version is a tar leaf
//! at `/opt/agentos/pkgs/<pkg>/<version>`, and each managed command/current
//! alias is its own root-symlink leaf. The containing dirs stay writable overlay
//! dirs so user-installed commands can coexist beside managed package entries.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::state::SidecarError;
use serde::Deserialize;
use vfs::posix::{normalize_path, TarFileSystem, VirtualFileSystem};

/// Root of the agentOS package tree inside the VM.
pub const OPT_AGENTOS_ROOT: &str = "/opt/agentos";
/// The symlink farm on `$PATH`.
pub const OPT_AGENTOS_BIN: &str = "/opt/agentos/bin";
const AGENT_SNAPSHOT_BUNDLE: &str = "dist/sdk-snapshot.js";
pub const DEFAULT_PACKAGE_TAR_NAME: &str = "package.tar";
pub const MAX_AGENTOS_PACKAGE_MOUNTS: usize = 4096;

/// A package to project, derived from `agentos-package.json` in a package dir or tar.
#[derive(Debug, Clone)]
pub struct PackageDescriptor {
    pub name: String,
    pub version: String,
    pub dir: String,
    pub tar_path: Option<String>,
    pub tar_digest: Option<String>,
    /// `bin/` command that speaks ACP, if this is an agent package.
    pub acp_entrypoint: Option<String>,
    pub snapshot: bool,
    pub provides: Option<PackageProvidesDescriptor>,
    pub commands: Vec<PackageCommandTarget>,
    pub man_pages: Vec<PackageManPageTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCommandTarget {
    pub command: String,
    pub entry: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManPageTarget {
    pub section: String,
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageLeafMount {
    Tar {
        guest_path: String,
        tar_path: String,
        digest: String,
        root: String,
    },
    HostDir {
        guest_path: String,
        host_path: String,
    },
    SingleSymlink {
        guest_path: String,
        target: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageProvidesDescriptor {
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub files: Vec<PackageProvidesFileDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageProvidesFileDescriptor {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Deserialize)]
struct AgentosPackageManifest {
    name: String,
    version: String,
    #[serde(default)]
    agent: Option<PackageAgentDescriptor>,
    #[serde(default)]
    provides: Option<PackageProvidesDescriptor>,
}

#[derive(Debug, Deserialize)]
struct PackageAgentDescriptor {
    #[serde(rename = "acpEntrypoint")]
    acp_entrypoint: String,
    #[serde(default)]
    snapshot: bool,
}

impl PackageDescriptor {
    fn from_parts(
        dir: String,
        tar_path: Option<String>,
        tar_digest: Option<String>,
        manifest: AgentosPackageManifest,
        commands: Vec<PackageCommandTarget>,
        man_pages: Vec<PackageManPageTarget>,
    ) -> Result<Self, SidecarError> {
        if manifest.name.is_empty() {
            return Err(SidecarError::InvalidState(format!(
                "agentos-package.json in {dir} is missing a valid \"name\""
            )));
        }
        if manifest.version.is_empty() {
            return Err(SidecarError::InvalidState(format!(
                "agentos-package.json in {dir} is missing a valid \"version\""
            )));
        }
        let (acp_entrypoint, snapshot) = match manifest.agent {
            Some(agent) => (Some(agent.acp_entrypoint), agent.snapshot),
            None => (None, false),
        };
        if acp_entrypoint
            .as_ref()
            .is_some_and(|entry| entry.is_empty())
        {
            return Err(SidecarError::InvalidState(format!(
                "agentos-package.json in {dir} has an empty agent.acpEntrypoint"
            )));
        }
        Ok(Self {
            name: manifest.name,
            version: manifest.version,
            dir,
            tar_path,
            tar_digest,
            acp_entrypoint,
            snapshot,
            provides: manifest.provides,
            commands,
            man_pages,
        })
    }

    fn tar_ref(&self) -> Result<Option<(&str, &str)>, SidecarError> {
        match (self.tar_path.as_deref(), self.tar_digest.as_deref()) {
            (Some(tar_path), Some(digest)) => Ok(Some((tar_path, digest))),
            (Some(_), None) => Err(SidecarError::InvalidState(format!(
                "package `{}` is missing tar digest",
                self.name
            ))),
            (None, Some(_)) => Err(SidecarError::InvalidState(format!(
                "package `{}` is missing {DEFAULT_PACKAGE_TAR_NAME}",
                self.name
            ))),
            (None, None) => Ok(None),
        }
    }
}

fn io_err(context: &str, error: std::io::Error) -> SidecarError {
    SidecarError::Io(format!("{context}: {error}"))
}

fn read_agentos_package_manifest(dir: &str) -> Result<AgentosPackageManifest, SidecarError> {
    let path = Path::new(dir).join("agentos-package.json");
    if !path.exists() {
        return Err(SidecarError::InvalidState(format!(
            "missing required agentos-package.json in package dir {dir}"
        )));
    }
    let text = fs::read_to_string(&path).map_err(|e| io_err("read agentos-package.json", e))?;
    serde_json::from_str(&text).map_err(|e| {
        SidecarError::InvalidState(format!("invalid agentos-package.json in {dir}: {e}"))
    })
}

/// Read the sidecar-owned package manifest from `<dir>/agentos-package.json`.
pub fn read_package_manifest(dir: &str) -> Result<PackageDescriptor, SidecarError> {
    let manifest = read_agentos_package_manifest(dir)?;
    let tar_path = package_tar_for_dir(dir);
    let tar_digest = tar_path
        .as_ref()
        .map(|path| digest_file(path))
        .transpose()?;
    PackageDescriptor::from_parts(
        dir.to_owned(),
        tar_path.map(|path| path.to_string_lossy().into_owned()),
        tar_digest,
        manifest,
        command_targets_from_dir(dir)?,
        man_pages_from_dir(dir)?,
    )
}

/// Read the first snapshot-enabled agent package's bundled SDK snapshot source.
pub fn read_agent_snapshot_bundle(
    package: &PackageDescriptor,
) -> Result<Option<String>, SidecarError> {
    if !package.snapshot {
        return Ok(None);
    }
    let path = Path::new(&package.dir).join(AGENT_SNAPSHOT_BUNDLE);
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| io_err("read agent snapshot bundle", e))
}

/// Read the package's `version` from `agentos-package.json`.
pub fn read_package_version(dir: &str) -> Result<String, SidecarError> {
    Ok(read_agentos_package_manifest(dir)?.version)
}

pub fn read_package_name(dir: &str) -> Result<String, SidecarError> {
    Ok(read_agentos_package_manifest(dir)?.name)
}

fn package_tar_for_dir(dir: &str) -> Option<PathBuf> {
    let tar = Path::new(dir).join(DEFAULT_PACKAGE_TAR_NAME);
    tar.is_file().then_some(tar)
}

/// Map each command name to its entry path relative to the package root.
fn command_targets_from_dir(dir: &str) -> Result<Vec<PackageCommandTarget>, SidecarError> {
    let pkg_json = Path::new(dir).join("package.json");
    if pkg_json.exists() {
        if let Ok(text) = fs::read_to_string(&pkg_json) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(targets) = command_targets_from_package_json(&value) {
                    return Ok(targets);
                }
            }
        }
    }

    let bin = Path::new(dir).join("bin");
    if !bin.is_dir() {
        return Ok(Vec::new());
    }
    let mut targets = Vec::new();
    for entry in fs::read_dir(&bin).map_err(|e| io_err("read bin/", e))? {
        let entry = entry.map_err(|e| io_err("read bin/ entry", e))?;
        if let Some(name) = entry.file_name().to_str() {
            if is_projectable_command_name(name) {
                targets.push(PackageCommandTarget {
                    command: name.to_owned(),
                    entry: format!("bin/{name}"),
                });
            }
        }
    }
    targets.sort_by(|a, b| a.command.cmp(&b.command));
    Ok(targets)
}

fn man_pages_from_dir(dir: &str) -> Result<Vec<PackageManPageTarget>, SidecarError> {
    let man = Path::new(dir).join("share").join("man");
    if !man.is_dir() {
        return Ok(Vec::new());
    }
    let mut pages = Vec::new();
    for section in fs::read_dir(&man).map_err(|e| io_err("read man/", e))? {
        let section = section.map_err(|e| io_err("man section", e))?;
        if !section.path().is_dir() {
            continue;
        }
        let Some(section_name) = section.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        for page in fs::read_dir(section.path()).map_err(|e| io_err("man pages", e))? {
            let page = page.map_err(|e| io_err("man page", e))?;
            if let Some(page_name) = page.file_name().to_str() {
                pages.push(PackageManPageTarget {
                    section: section_name.clone(),
                    page: page_name.to_owned(),
                });
            }
        }
    }
    pages.sort_by(|a, b| (&a.section, &a.page).cmp(&(&b.section, &b.page)));
    Ok(pages)
}

fn command_targets_from_package_json(
    value: &serde_json::Value,
) -> Option<Vec<PackageCommandTarget>> {
    match value.get("bin") {
        Some(serde_json::Value::String(path)) => {
            let name = value.get("name").and_then(|v| v.as_str())?;
            let unscoped = name.rsplit('/').next().unwrap_or(name).to_owned();
            Some(
                is_projectable_command_name(&unscoped)
                    .then(|| PackageCommandTarget {
                        command: unscoped,
                        entry: normalize_rel(path),
                    })
                    .into_iter()
                    .collect(),
            )
        }
        Some(serde_json::Value::Object(map)) => {
            let mut targets: Vec<PackageCommandTarget> = map
                .iter()
                .filter_map(|(name, path)| {
                    is_projectable_command_name(name)
                        .then(|| path.as_str())
                        .flatten()
                        .map(|path| PackageCommandTarget {
                            command: name.clone(),
                            entry: normalize_rel(path),
                        })
                })
                .collect();
            targets.sort_by(|a, b| a.command.cmp(&b.command));
            Some(targets)
        }
        _ => None,
    }
}

fn is_projectable_command_name(name: &str) -> bool {
    !name.starts_with('_') && !name.starts_with('.')
}

/// Strip a leading `./` so the resulting path is a clean in-package relative path.
fn normalize_rel(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_owned()
}

/// Derive command names for the package (sorted).
pub fn derive_commands(dir: &str) -> Result<Vec<String>, SidecarError> {
    Ok(command_targets_from_dir(dir)?
        .into_iter()
        .map(|target| target.command)
        .collect())
}

pub fn read_package_manifest_from_ref(
    dir: Option<&str>,
    tar: Option<&str>,
) -> Result<PackageDescriptor, SidecarError> {
    if let Some(tar) = tar.filter(|value| !value.is_empty()) {
        return read_package_manifest_from_tar(tar);
    }
    if let Some(dir) = dir.filter(|value| !value.is_empty()) {
        let path = Path::new(dir);
        if path.is_file() {
            return read_package_manifest_from_tar(dir);
        }
        if let Some(package_tar) = package_tar_for_dir(dir) {
            return read_package_manifest_from_tar_with_dir(&package_tar, dir.to_owned());
        }
        return read_package_manifest(dir);
    }
    Err(SidecarError::InvalidState(String::from(
        "package descriptor must include a package tar or dir",
    )))
}

fn read_package_manifest_from_tar(tar: &str) -> Result<PackageDescriptor, SidecarError> {
    read_package_manifest_from_tar_with_dir(Path::new(tar), tar.to_owned())
}

fn read_package_manifest_from_tar_with_dir(
    tar: &Path,
    dir: String,
) -> Result<PackageDescriptor, SidecarError> {
    let digest = digest_file(tar)?;
    let mut fs = TarFileSystem::open(tar, digest.clone())
        .map_err(|error| SidecarError::InvalidState(error.to_string()))?;
    let manifest_bytes = fs
        .read_file("/agentos-package.json")
        .map_err(|error| SidecarError::InvalidState(error.to_string()))?;
    let manifest =
        serde_json::from_slice::<AgentosPackageManifest>(&manifest_bytes).map_err(|error| {
            SidecarError::InvalidState(format!(
                "invalid agentos-package.json in {}: {error}",
                tar.display()
            ))
        })?;
    let commands = command_targets_from_tar(&mut fs)?;
    let man_pages = man_pages_from_tar(&mut fs)?;
    PackageDescriptor::from_parts(
        dir,
        Some(tar.to_string_lossy().into_owned()),
        Some(digest),
        manifest,
        commands,
        man_pages,
    )
}

fn command_targets_from_tar(
    fs: &mut TarFileSystem,
) -> Result<Vec<PackageCommandTarget>, SidecarError> {
    match fs.read_file("/package.json") {
        Ok(bytes) => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(targets) = command_targets_from_package_json(&value) {
                    return Ok(targets);
                }
            }
        }
        Err(error) if error.code() == "ENOENT" => {}
        Err(error) => return Err(SidecarError::InvalidState(error.to_string())),
    }

    let entries = match fs.read_dir("/bin") {
        Ok(entries) => entries,
        Err(error) if error.code() == "ENOENT" => return Ok(Vec::new()),
        Err(error) => return Err(SidecarError::InvalidState(error.to_string())),
    };
    let mut targets = entries
        .into_iter()
        .filter_map(|command| {
            is_projectable_command_name(&command).then(|| PackageCommandTarget {
                entry: format!("bin/{command}"),
                command,
            })
        })
        .collect::<Vec<_>>();
    targets.sort_by(|a, b| a.command.cmp(&b.command));
    Ok(targets)
}

fn man_pages_from_tar(fs: &mut TarFileSystem) -> Result<Vec<PackageManPageTarget>, SidecarError> {
    let sections = match fs.read_dir("/share/man") {
        Ok(entries) => entries,
        Err(error) if error.code() == "ENOENT" => return Ok(Vec::new()),
        Err(error) => return Err(SidecarError::InvalidState(error.to_string())),
    };
    let mut pages = Vec::new();
    for section in sections {
        let section_path = format!("/share/man/{section}");
        let Ok(stat) = fs.stat(&section_path) else {
            continue;
        };
        if !stat.is_directory {
            continue;
        }
        for page in fs
            .read_dir(&section_path)
            .map_err(|error| SidecarError::InvalidState(error.to_string()))?
        {
            pages.push(PackageManPageTarget {
                section: section.clone(),
                page,
            });
        }
    }
    pages.sort_by(|a, b| (&a.section, &a.page).cmp(&(&b.section, &b.page)));
    Ok(pages)
}

pub fn build_package_leaf_mounts(
    packages: &[PackageDescriptor],
    mount_at: &str,
) -> Result<Vec<PackageLeafMount>, SidecarError> {
    let mount_at = normalize_mount_root(mount_at);
    let mut mounts = Vec::new();
    let mut command_paths = HashSet::new();

    for package in packages {
        let commands = package
            .commands
            .iter()
            .map(|target| target.command.clone())
            .collect::<Vec<_>>();
        if let Some(acp) = &package.acp_entrypoint {
            if !commands.contains(acp) {
                return Err(SidecarError::InvalidState(format!(
                    "agent acpEntrypoint {acp:?} is not one of {}'s commands",
                    package.name
                )));
            }
        }

        let package_root = package_guest_root(&mount_at, &package.name);
        let version_path = normalize_path(&format!("{package_root}/{}", package.version));
        if let Some((tar_path, digest)) = package.tar_ref()? {
            push_mount(
                &mut mounts,
                PackageLeafMount::Tar {
                    guest_path: version_path,
                    tar_path: tar_path.to_owned(),
                    digest: digest.to_owned(),
                    root: String::from("/"),
                },
            )?;
        } else {
            push_mount(
                &mut mounts,
                PackageLeafMount::HostDir {
                    guest_path: version_path,
                    host_path: package.dir.clone(),
                },
            )?;
        }
        push_mount(
            &mut mounts,
            PackageLeafMount::SingleSymlink {
                guest_path: normalize_path(&format!("{package_root}/current")),
                target: package.version.clone(),
            },
        )?;

        for target in &package.commands {
            let guest_path = normalize_path(&format!("{mount_at}/bin/{}", target.command));
            if !command_paths.insert(guest_path.clone()) {
                return Err(SidecarError::InvalidState(format!(
                    "command {:?} is already provided by another package",
                    target.command
                )));
            }
            push_mount(
                &mut mounts,
                PackageLeafMount::SingleSymlink {
                    guest_path,
                    target: format!("../pkgs/{}/current/{}", package.name, target.entry),
                },
            )?;
        }

        for page in &package.man_pages {
            push_mount(
                &mut mounts,
                PackageLeafMount::SingleSymlink {
                    guest_path: normalize_path(&format!(
                        "{mount_at}/share/man/{}/{}",
                        page.section, page.page
                    )),
                    target: format!(
                        "../../../pkgs/{}/current/share/man/{}/{}",
                        package.name, page.section, page.page
                    ),
                },
            )?;
        }
    }

    Ok(mounts)
}

pub fn package_provides_file_mount(
    package: &PackageDescriptor,
    source: &str,
    target: &str,
) -> Result<Option<PackageLeafMount>, SidecarError> {
    if let Some((tar_path, digest)) = package.tar_ref()? {
        let root = normalize_package_source(source);
        let mut fs = TarFileSystem::open_at(tar_path, digest, &root)
            .map_err(|error| SidecarError::InvalidState(error.to_string()))?;
        match fs.stat("/") {
            Ok(stat) if stat.is_directory => Ok(Some(PackageLeafMount::Tar {
                guest_path: normalize_path(target),
                tar_path: tar_path.to_owned(),
                digest: digest.to_owned(),
                root,
            })),
            Ok(_) => Ok(None),
            Err(error) if error.code() == "ENOENT" => Err(SidecarError::InvalidState(format!(
                "package provides file source is missing: package `{}` source `{source}` target `{target}`",
                package.name
            ))),
            Err(error) => Err(SidecarError::InvalidState(error.to_string())),
        }
    } else {
        let host_path =
            Path::new(&package.dir).join(normalize_package_source(source).trim_start_matches('/'));
        match fs::metadata(&host_path) {
            Ok(metadata) if metadata.is_dir() => Ok(Some(PackageLeafMount::HostDir {
                guest_path: normalize_path(target),
                host_path: host_path.to_string_lossy().into_owned(),
            })),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(SidecarError::InvalidState(format!(
                    "package provides file source is missing: package `{}` source `{source}` target `{target}`",
                    package.name
                )))
            }
            Err(error) => Err(io_err("stat package provides file source", error)),
        }
    }
}

fn push_mount(
    mounts: &mut Vec<PackageLeafMount>,
    mount: PackageLeafMount,
) -> Result<(), SidecarError> {
    let observed = mounts.len() + 1;
    if observed > MAX_AGENTOS_PACKAGE_MOUNTS {
        return Err(SidecarError::InvalidState(format!(
            "agentos package mount count exceeded: {observed} mounts > {MAX_AGENTOS_PACKAGE_MOUNTS} mounts (raise via limits.agentosPackages.maxMounts)"
        )));
    }
    if observed * 100 / MAX_AGENTOS_PACKAGE_MOUNTS >= 80 {
        tracing::warn!(
            limit = "agentos_package_mounts",
            observed,
            capacity = MAX_AGENTOS_PACKAGE_MOUNTS,
            fill_percent = observed * 100 / MAX_AGENTOS_PACKAGE_MOUNTS,
            wired = "limits.agentosPackages.maxMounts",
            "agentos package mount count approaching configured limit"
        );
    }
    mounts.push(mount);
    Ok(())
}

fn normalize_mount_root(mount_at: &str) -> String {
    if mount_at.is_empty() {
        String::from(OPT_AGENTOS_ROOT)
    } else {
        normalize_path(mount_at)
    }
}

fn package_guest_root(mount_at: &str, name: &str) -> String {
    normalize_path(&format!("{mount_at}/pkgs/{name}"))
}

fn normalize_package_source(source: &str) -> String {
    if source.trim().is_empty() {
        String::from("/")
    } else {
        normalize_path(source)
    }
}

fn digest_file(path: impl AsRef<Path>) -> Result<String, SidecarError> {
    let bytes = fs::read(path.as_ref()).map_err(|error| io_err("read package tar", error))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
