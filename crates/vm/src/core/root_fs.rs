use crate::core::ca::default_ca_snapshot;
use crate::core::services::default_services_snapshot;
use agentos_sidecar_protocol::protocol::{
    RootFilesystemDescriptor as ProtocolRootFilesystemDescriptor,
    RootFilesystemEntry as ProtocolRootFilesystemEntry,
    RootFilesystemEntryEncoding as ProtocolRootFilesystemEntryEncoding,
    RootFilesystemEntryKind as ProtocolRootFilesystemEntryKind,
    RootFilesystemLowerDescriptor as ProtocolRootFilesystemLowerDescriptor,
    RootFilesystemMode as ProtocolRootFilesystemMode,
    SnapshotRootFilesystemLower as ProtocolSnapshotRootFilesystemLower,
};
use agentos_vfs_core::posix::usage::RootFilesystemResourceLimits;
use agentos_vm_config as vm_config;
use agentos_vm_host_interface::FilesystemSnapshot;
use agentos_vm_kernel::mount_table::MountTable;
use agentos_vm_kernel::root_fs::{
    decode_snapshot_with_import_limits, is_supported_root_filesystem_snapshot_format,
    FilesystemEntry, FilesystemEntryKind, RootFileSystem,
    RootFilesystemDescriptor as KernelRootFilesystemDescriptor, RootFilesystemImportLimits,
    RootFilesystemMode as KernelRootFilesystemMode, RootFilesystemSnapshot,
};
use agentos_vm_kernel::vfs::{normalize_path, VirtualFileSystem};
use base64::Engine;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarCoreError {
    filesystem_quota: Option<agentos_vm_kernel::resource_accounting::FilesystemQuotaViolation>,
    code: Option<String>,
    message: String,
}

impl SidecarCoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            filesystem_quota: None,
            code: None,
            message: message.into(),
        }
    }

    /// Construct a stable typed error for an executor-facing operation.
    ///
    /// The code is transported independently from the diagnostic message so
    /// adapters never need to recover errno by parsing `Display` output.
    pub fn typed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            filesystem_quota: None,
            code: Some(code.into()),
            message: message.into(),
        }
    }

    pub(crate) fn from_kernel(error: agentos_vm_kernel::kernel::KernelError) -> Self {
        let mut mapped = Self::typed(error.code(), error.message());
        mapped.filesystem_quota = error.filesystem_quota();
        mapped
    }

    pub(crate) fn filesystem_quota(
        &self,
    ) -> Option<agentos_vm_kernel::resource_accounting::FilesystemQuotaViolation> {
        self.filesystem_quota
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SidecarCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.code {
            Some(code) => write!(f, "{code}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl Error for SidecarCoreError {}

pub fn root_filesystem_descriptor_from_config(
    config: &vm_config::RootFilesystemConfig,
) -> Result<KernelRootFilesystemDescriptor, SidecarCoreError> {
    root_filesystem_descriptor_from_config_with_import_limits(
        config,
        &RootFilesystemImportLimits::default(),
    )
}

fn root_filesystem_descriptor_from_config_with_import_limits(
    config: &vm_config::RootFilesystemConfig,
    import_limits: &RootFilesystemImportLimits,
) -> Result<KernelRootFilesystemDescriptor, SidecarCoreError> {
    Ok(KernelRootFilesystemDescriptor {
        mode: root_filesystem_mode_from_config(config.mode),
        disable_default_base_layer: config.disable_default_base_layer,
        lowers: config
            .lowers
            .iter()
            .map(|lower| root_filesystem_lower_from_config(lower, import_limits))
            .collect::<Result<Vec<_>, _>>()?,
        bootstrap_entries: config
            .bootstrap_entries
            .iter()
            .map(root_filesystem_entry_from_config)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub fn root_filesystem_protocol_descriptor_from_config(
    config: &vm_config::RootFilesystemConfig,
) -> ProtocolRootFilesystemDescriptor {
    ProtocolRootFilesystemDescriptor {
        mode: match config.mode {
            vm_config::RootFilesystemMode::Ephemeral => ProtocolRootFilesystemMode::Ephemeral,
            vm_config::RootFilesystemMode::ReadOnly => ProtocolRootFilesystemMode::ReadOnly,
        },
        disable_default_base_layer: config.disable_default_base_layer,
        lowers: config
            .lowers
            .iter()
            .map(root_filesystem_protocol_lower_from_config)
            .collect(),
        bootstrap_entries: config
            .bootstrap_entries
            .iter()
            .map(root_filesystem_protocol_entry_from_config)
            .collect(),
    }
}

pub fn build_root_filesystem(
    config: &vm_config::RootFilesystemConfig,
    limits: &impl RootFilesystemResourceLimits,
) -> Result<RootFileSystem, SidecarCoreError> {
    build_root_filesystem_with_loaded_snapshot(config, None, limits)
}

pub fn build_root_filesystem_with_loaded_snapshot(
    config: &vm_config::RootFilesystemConfig,
    loaded_snapshot: Option<&FilesystemSnapshot>,
    limits: &impl RootFilesystemResourceLimits,
) -> Result<RootFileSystem, SidecarCoreError> {
    let import_limits = RootFilesystemImportLimits::from_resource_limits(limits);
    let mut descriptor = if let Some(restored) = supported_loaded_snapshot(loaded_snapshot) {
        KernelRootFilesystemDescriptor {
            mode: root_filesystem_mode_from_config(config.mode),
            disable_default_base_layer: true,
            lowers: vec![
                decode_snapshot_with_import_limits(&restored.bytes, &import_limits).map_err(
                    |error| {
                        SidecarCoreError::new(format!("decode restored root filesystem: {error}"))
                    },
                )?,
            ],
            bootstrap_entries: config
                .bootstrap_entries
                .iter()
                .map(root_filesystem_entry_from_config)
                .collect::<Result<Vec<_>, _>>()?,
        }
    } else {
        root_filesystem_descriptor_from_config_with_import_limits(config, &import_limits)?
    };
    // Restore ignores configured lowers; fill its missing directories later
    // from the caller seed, without inserting fresh defaults into persisted data.
    if descriptor.disable_default_base_layer && supported_loaded_snapshot(loaded_snapshot).is_none()
    {
        let represented = descriptor
            .lowers
            .iter()
            .flat_map(|lower| lower.entries.iter())
            .chain(descriptor.bootstrap_entries.iter())
            .map(|entry| normalize_path(&entry.path))
            .collect::<std::collections::BTreeSet<_>>();
        let mut entries = AGENTOS_BOOTSTRAP_DIRECTORIES
            .iter()
            .filter(|path| !represented.contains(**path))
            .map(|path| {
                let mut entry = FilesystemEntry::directory(*path);
                let (mode, uid, gid) = agentos_directory_metadata(path);
                entry.mode = mode;
                entry.uid = uid;
                entry.gid = gid;
                entry
            })
            .collect::<Vec<_>>();
        if !represented.contains("/usr/bin/env") {
            entries.push(FilesystemEntry::file("/usr/bin/env", vec![0]));
        }
        if !entries.is_empty() {
            descriptor.lowers.push(RootFilesystemSnapshot { entries });
        }
    }

    // This is the final (lowest-precedence) AgentOS lower. Overlay lookup checks
    // user lowers first, and bootstrap entries are applied to the upper, so an
    // explicit trust store always replaces these defaults.
    let bootstrap_replaces_bundle = descriptor
        .bootstrap_entries
        .iter()
        .any(|entry| normalize_path(&entry.path) == crate::core::ca::CA_CERTIFICATES_GUEST_PATH);
    let bootstrap_replaces_cert_pem = descriptor
        .bootstrap_entries
        .iter()
        .any(|entry| normalize_path(&entry.path) == crate::core::ca::CA_CERTIFICATES_SYMLINK_PATH);
    // `write_file` follows a lower-layer symlink during copy-up. When an
    // explicit bootstrap node replaces one of the trust paths, remove that
    // exact lower entry first so a regular file replaces the symlink itself
    // instead of overwriting its target. This also handles restored snapshots
    // that captured an older default CA layout.
    for lower in &mut descriptor.lowers {
        lower.entries.retain(|entry| {
            let path = normalize_path(&entry.path);
            !(bootstrap_replaces_bundle && path == crate::core::ca::CA_CERTIFICATES_GUEST_PATH
                || bootstrap_replaces_cert_pem
                    && path == crate::core::ca::CA_CERTIFICATES_SYMLINK_PATH)
        });
    }
    descriptor.lowers.push(default_services_snapshot());
    descriptor.lowers.push(default_ca_snapshot(
        bootstrap_replaces_bundle,
        bootstrap_replaces_cert_pem,
    ));
    RootFileSystem::from_descriptor_with_import_limits(descriptor, &import_limits)
        .map_err(|error| SidecarCoreError::new(format!("build root filesystem: {error}")))
}

/// Complete product directories on the selected root before the VM is published.
/// This is agentOS product policy, not an embedded-kernel or native-host default.
pub(crate) fn complete_agentos_root_directories(
    mounts: &mut MountTable,
    config: &vm_config::RootFilesystemConfig,
    native_root: bool,
) -> Result<(), SidecarCoreError> {
    if native_root {
        return Ok(());
    }
    let root = mounts
        .root_virtual_filesystem_mut::<RootFileSystem>()
        .ok_or_else(|| {
            SidecarCoreError::new("agentOS root filesystem unavailable during bootstrap")
        })?;
    for path in AGENTOS_BOOTSTRAP_DIRECTORIES {
        match root.lstat(path) {
            Ok(_) => continue, // Preserve all existing nodes, including dangling symlinks.
            Err(error) if error.code() == "ENOENT" => {}
            Err(error) => return Err(SidecarCoreError::typed(error.code(), error.to_string())),
        }
        let seed = config.lowers.iter().find_map(|lower| match lower {
            vm_config::RootFilesystemLowerDescriptor::Snapshot { entries } => {
                entries.iter().find(|entry| entry.path == *path)
            }
            vm_config::RootFilesystemLowerDescriptor::BundledBaseFilesystem => None,
        });
        let (default_mode, default_uid, default_gid) = agentos_directory_metadata(path);
        let mode = seed
            .filter(|entry| entry.kind == vm_config::RootFilesystemEntryKind::Directory)
            .and_then(|entry| entry.mode)
            .unwrap_or(default_mode);
        let uid = seed.and_then(|entry| entry.uid).unwrap_or(default_uid);
        let gid = seed.and_then(|entry| entry.gid).unwrap_or(default_gid);
        root.mkdir(path, true)
            .map_err(|error| SidecarCoreError::typed(error.code(), error.to_string()))?;
        root.chmod(path, mode)
            .map_err(|error| SidecarCoreError::typed(error.code(), error.to_string()))?;
        root.chown(path, uid, gid)
            .map_err(|error| SidecarCoreError::typed(error.code(), error.to_string()))?;
    }
    Ok(())
}

fn agentos_directory_metadata(path: &str) -> (u32, u32, u32) {
    match path {
        "/tmp" | "/var/tmp" => (0o1777, 0, 0),
        "/root" => (0o711, 0, 0),
        "/sys" | "/var/empty" => (0o555, 0, 0),
        "/home/agentos" => (0o2755, 1000, 1000),
        "/workspace" => (0o755, 1000, 1000),
        "/var/lock" | "/var/run" => (0o777, 0, 0),
        _ => (0o755, 0, 0),
    }
}

const AGENTOS_BOOTSTRAP_DIRECTORIES: &[&str] = &[
    "/dev",
    "/proc",
    "/tmp",
    "/bin",
    "/lib",
    "/sbin",
    "/boot",
    "/etc",
    "/root",
    "/run",
    "/srv",
    "/sys",
    "/opt",
    "/mnt",
    "/media",
    "/home",
    "/home/agentos",
    "/workspace",
    "/usr",
    "/usr/bin",
    "/usr/games",
    "/usr/include",
    "/usr/lib",
    "/usr/libexec",
    "/usr/man",
    "/usr/local",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/share",
    "/usr/share/man",
    "/var",
    "/var/cache",
    "/var/empty",
    "/var/lib",
    "/var/lock",
    "/var/log",
    "/var/run",
    "/var/spool",
    "/var/tmp",
    "/etc/agentos",
];

pub fn build_root_mount_table(
    config: &vm_config::RootFilesystemConfig,
    limits: &impl RootFilesystemResourceLimits,
) -> Result<MountTable, SidecarCoreError> {
    Ok(MountTable::new(build_root_filesystem(config, limits)?))
}

pub fn build_root_mount_table_with_loaded_snapshot(
    config: &vm_config::RootFilesystemConfig,
    loaded_snapshot: Option<&FilesystemSnapshot>,
    limits: &impl RootFilesystemResourceLimits,
) -> Result<MountTable, SidecarCoreError> {
    Ok(MountTable::new(build_root_filesystem_with_loaded_snapshot(
        config,
        loaded_snapshot,
        limits,
    )?))
}

pub fn root_filesystem_mode_from_config(
    mode: vm_config::RootFilesystemMode,
) -> KernelRootFilesystemMode {
    match mode {
        vm_config::RootFilesystemMode::Ephemeral => KernelRootFilesystemMode::Ephemeral,
        vm_config::RootFilesystemMode::ReadOnly => KernelRootFilesystemMode::ReadOnly,
    }
}

pub fn protocol_root_filesystem_mode(mode: ProtocolRootFilesystemMode) -> KernelRootFilesystemMode {
    match mode {
        ProtocolRootFilesystemMode::Ephemeral => KernelRootFilesystemMode::Ephemeral,
        ProtocolRootFilesystemMode::ReadOnly => KernelRootFilesystemMode::ReadOnly,
    }
}

fn supported_loaded_snapshot(snapshot: Option<&FilesystemSnapshot>) -> Option<&FilesystemSnapshot> {
    snapshot.filter(|snapshot| is_supported_root_filesystem_snapshot_format(&snapshot.format))
}

fn root_filesystem_lower_from_config(
    lower: &vm_config::RootFilesystemLowerDescriptor,
    import_limits: &RootFilesystemImportLimits,
) -> Result<RootFilesystemSnapshot, SidecarCoreError> {
    match lower {
        vm_config::RootFilesystemLowerDescriptor::Snapshot { entries } => {
            Ok(RootFilesystemSnapshot {
                entries: entries
                    .iter()
                    .map(root_filesystem_entry_from_config)
                    .collect::<Result<Vec<_>, _>>()?,
            })
        }
        vm_config::RootFilesystemLowerDescriptor::BundledBaseFilesystem => Ok(
            agentos_vm_kernel::root_fs::load_bundled_base_snapshot_with_limits(import_limits)
                .map_err(|error| {
                    SidecarCoreError::new(format!("load bundled base filesystem lower: {error}"))
                })?,
        ),
    }
}

fn root_filesystem_entry_from_config(
    entry: &vm_config::RootFilesystemEntry,
) -> Result<FilesystemEntry, SidecarCoreError> {
    let mode = entry.mode.unwrap_or(match entry.kind {
        vm_config::RootFilesystemEntryKind::File => {
            if entry.executable {
                0o755
            } else {
                0o644
            }
        }
        vm_config::RootFilesystemEntryKind::Directory => 0o755,
        vm_config::RootFilesystemEntryKind::Symlink => 0o777,
    });

    let content = match entry.content.as_ref() {
        Some(content) => match entry.encoding {
            Some(vm_config::RootFilesystemEntryEncoding::Base64) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(content)
                    .map_err(|error| {
                        SidecarCoreError::new(format!(
                            "invalid base64 root filesystem content for {}: {error}",
                            entry.path
                        ))
                    })?,
            ),
            Some(vm_config::RootFilesystemEntryEncoding::Utf8) | None => {
                Some(content.as_bytes().to_vec())
            }
        },
        None => None,
    };

    Ok(FilesystemEntry {
        path: normalize_path(&entry.path),
        kind: match entry.kind {
            vm_config::RootFilesystemEntryKind::File => FilesystemEntryKind::File,
            vm_config::RootFilesystemEntryKind::Directory => FilesystemEntryKind::Directory,
            vm_config::RootFilesystemEntryKind::Symlink => FilesystemEntryKind::Symlink,
        },
        mode,
        uid: entry.uid.unwrap_or(0),
        gid: entry.gid.unwrap_or(0),
        content,
        target: entry.target.clone(),
    })
}

fn root_filesystem_protocol_lower_from_config(
    lower: &vm_config::RootFilesystemLowerDescriptor,
) -> ProtocolRootFilesystemLowerDescriptor {
    match lower {
        vm_config::RootFilesystemLowerDescriptor::Snapshot { entries } => {
            ProtocolRootFilesystemLowerDescriptor::SnapshotRootFilesystemLower(
                ProtocolSnapshotRootFilesystemLower {
                    entries: entries
                        .iter()
                        .map(root_filesystem_protocol_entry_from_config)
                        .collect(),
                },
            )
        }
        vm_config::RootFilesystemLowerDescriptor::BundledBaseFilesystem => {
            ProtocolRootFilesystemLowerDescriptor::BundledBaseFilesystemLower
        }
    }
}

fn root_filesystem_protocol_entry_from_config(
    entry: &vm_config::RootFilesystemEntry,
) -> ProtocolRootFilesystemEntry {
    ProtocolRootFilesystemEntry {
        path: entry.path.clone(),
        kind: match entry.kind {
            vm_config::RootFilesystemEntryKind::File => ProtocolRootFilesystemEntryKind::File,
            vm_config::RootFilesystemEntryKind::Directory => {
                ProtocolRootFilesystemEntryKind::Directory
            }
            vm_config::RootFilesystemEntryKind::Symlink => ProtocolRootFilesystemEntryKind::Symlink,
        },
        mode: entry.mode,
        uid: entry.uid,
        gid: entry.gid,
        content: entry.content.clone(),
        encoding: entry.encoding.map(|encoding| match encoding {
            vm_config::RootFilesystemEntryEncoding::Utf8 => {
                ProtocolRootFilesystemEntryEncoding::Utf8
            }
            vm_config::RootFilesystemEntryEncoding::Base64 => {
                ProtocolRootFilesystemEntryEncoding::Base64
            }
        }),
        target: entry.target.clone(),
        executable: entry.executable,
    }
}

pub fn root_snapshot_entry(entry: &FilesystemEntry) -> ProtocolRootFilesystemEntry {
    let (content, encoding) = entry
        .content
        .clone()
        .map(snapshot_entry_content)
        .map(|(content, encoding)| (Some(content), Some(encoding)))
        .unwrap_or((None, None));

    ProtocolRootFilesystemEntry {
        path: entry.path.clone(),
        kind: match entry.kind {
            FilesystemEntryKind::File => ProtocolRootFilesystemEntryKind::File,
            FilesystemEntryKind::Directory => ProtocolRootFilesystemEntryKind::Directory,
            FilesystemEntryKind::Symlink => ProtocolRootFilesystemEntryKind::Symlink,
        },
        mode: Some(entry.mode),
        uid: Some(entry.uid),
        gid: Some(entry.gid),
        content,
        encoding,
        target: entry.target.clone(),
        executable: entry.mode & 0o111 != 0,
    }
}

/// Convert a protocol root-filesystem entry into a kernel `FilesystemEntry`, decoding
/// content (utf8/base64) and applying per-kind mode defaults. Shared by native and
/// browser bootstrap + snapshot paths.
pub fn convert_root_filesystem_entry(
    entry: &ProtocolRootFilesystemEntry,
) -> Result<FilesystemEntry, SidecarCoreError> {
    let mode = entry.mode.unwrap_or(match entry.kind {
        ProtocolRootFilesystemEntryKind::File => {
            if entry.executable {
                0o755
            } else {
                0o644
            }
        }
        ProtocolRootFilesystemEntryKind::Directory => 0o755,
        ProtocolRootFilesystemEntryKind::Symlink => 0o777,
    });

    let content = match entry.content.as_ref() {
        Some(content) => match entry.encoding {
            Some(ProtocolRootFilesystemEntryEncoding::Base64) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(content)
                    .map_err(|error| {
                        SidecarCoreError::new(format!(
                            "invalid base64 root filesystem content for {}: {error}",
                            entry.path
                        ))
                    })?,
            ),
            Some(ProtocolRootFilesystemEntryEncoding::Utf8) | None => {
                Some(content.as_bytes().to_vec())
            }
        },
        None => None,
    };

    Ok(FilesystemEntry {
        path: normalize_path(&entry.path),
        kind: match entry.kind {
            ProtocolRootFilesystemEntryKind::File => FilesystemEntryKind::File,
            ProtocolRootFilesystemEntryKind::Directory => FilesystemEntryKind::Directory,
            ProtocolRootFilesystemEntryKind::Symlink => FilesystemEntryKind::Symlink,
        },
        mode,
        uid: entry.uid.unwrap_or(0),
        gid: entry.gid.unwrap_or(0),
        content,
        target: entry.target.clone(),
    })
}

/// Build a kernel snapshot from protocol entries (shared by native + browser).
pub fn root_snapshot_from_entries(
    entries: &[ProtocolRootFilesystemEntry],
) -> Result<RootFilesystemSnapshot, SidecarCoreError> {
    Ok(RootFilesystemSnapshot {
        entries: entries
            .iter()
            .map(convert_root_filesystem_entry)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

/// Write one root-filesystem bootstrap entry (file/dir/symlink) into a VFS, creating
/// parent dirs and applying deterministic mode/uid/gid defaults (chmod/chown for
/// non-symlinks). Shared by native (raw root FS) and browser (kernel `filesystem_mut`).
pub fn apply_root_filesystem_entry<F: VirtualFileSystem>(
    filesystem: &mut F,
    entry: &ProtocolRootFilesystemEntry,
) -> Result<(), SidecarCoreError> {
    let kernel_entry = convert_root_filesystem_entry(entry)?;

    let parent = parent_directory(&kernel_entry.path);
    if parent != "/" && !filesystem.exists(&parent) {
        filesystem.mkdir(&parent, true).map_err(vfs_err)?;
    }

    match kernel_entry.kind {
        FilesystemEntryKind::Directory => filesystem
            .mkdir(&kernel_entry.path, true)
            .map_err(vfs_err)?,
        FilesystemEntryKind::File => filesystem
            .write_file(&kernel_entry.path, kernel_entry.content.unwrap_or_default())
            .map_err(vfs_err)?,
        FilesystemEntryKind::Symlink => filesystem
            .symlink(
                kernel_entry.target.as_deref().ok_or_else(|| {
                    SidecarCoreError::new(format!(
                        "root filesystem bootstrap for symlink {} requires a target",
                        entry.path
                    ))
                })?,
                &kernel_entry.path,
            )
            .map_err(vfs_err)?,
    }

    if !matches!(kernel_entry.kind, FilesystemEntryKind::Symlink) {
        filesystem
            .chmod(&kernel_entry.path, kernel_entry.mode)
            .map_err(vfs_err)?;
        filesystem
            .chown(&kernel_entry.path, kernel_entry.uid, kernel_entry.gid)
            .map_err(vfs_err)?;
    }

    Ok(())
}

fn vfs_err<E: fmt::Display>(error: E) -> SidecarCoreError {
    SidecarCoreError::new(error.to_string())
}

fn parent_directory(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(index) => path[..index].to_string(),
    }
}

fn snapshot_entry_content(content: Vec<u8>) -> (String, ProtocolRootFilesystemEntryEncoding) {
    match String::from_utf8(content) {
        Ok(text) => (text, ProtocolRootFilesystemEntryEncoding::Utf8),
        Err(error) => (
            base64::engine::general_purpose::STANDARD.encode(error.into_bytes()),
            ProtocolRootFilesystemEntryEncoding::Base64,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ca::{
        CA_CERTIFICATES_BUNDLE, CA_CERTIFICATES_GUEST_PATH, CA_CERTIFICATES_SYMLINK_PATH,
        CA_CERTIFICATES_SYMLINK_TARGET,
    };
    use crate::core::services::{BASELINE_SERVICES, SERVICES_GUEST_PATH};
    use agentos_vm_kernel::resource_accounting::ResourceLimits;
    use agentos_vm_kernel::vfs::VirtualFileSystem;

    fn completed_product_root(config: &vm_config::RootFilesystemConfig) -> MountTable {
        let limits = ResourceLimits::default();
        let mut mounts = build_root_mount_table(config, &limits).expect("build root");
        complete_agentos_root_directories(&mut mounts, config, false).expect("complete root");
        mounts
    }

    #[test]
    fn product_bootstrap_completes_default_root_without_changing_existing_metadata() {
        let config = vm_config::RootFilesystemConfig::default();
        let mut mounts = completed_product_root(&config);
        for path in AGENTOS_BOOTSTRAP_DIRECTORIES {
            assert!(mounts.stat(path).expect(path).is_directory, "{path}");
        }
        assert!(
            mounts.lstat("/var/run").unwrap().is_symbolic_link,
            "bundled symlink is preserved"
        );
        assert_eq!(mounts.lstat("/tmp").unwrap().mode & 0o7777, 0o1777);
        assert_eq!(mounts.lstat("/workspace").unwrap().uid, 1000);
    }

    #[test]
    fn no_base_product_scaffold_gets_special_metadata_without_client_seed() {
        let config = vm_config::RootFilesystemConfig {
            disable_default_base_layer: true,
            ..Default::default()
        };
        let mut mounts = completed_product_root(&config);
        assert_eq!(mounts.lstat("/tmp").unwrap().mode & 0o7777, 0o1777);
        assert_eq!(mounts.lstat("/var/empty").unwrap().mode & 0o7777, 0o555);
        let workspace = mounts.lstat("/workspace").unwrap();
        assert_eq!((workspace.uid, workspace.gid), (1000, 1000));
        assert_eq!(mounts.read_file("/usr/bin/env").unwrap(), vec![0]);
    }

    #[test]
    fn explicit_complete_no_base_root_does_not_pay_duplicate_default_import_quota() {
        let mut entries = AGENTOS_BOOTSTRAP_DIRECTORIES
            .iter()
            .map(|path| vm_config::RootFilesystemEntry {
                path: path.to_string(),
                kind: vm_config::RootFilesystemEntryKind::Directory,
                mode: Some(0o755),
                uid: Some(0),
                gid: Some(0),
                content: None,
                encoding: None,
                target: None,
                executable: true,
            })
            .collect::<Vec<_>>();
        entries.push(vm_config::RootFilesystemEntry {
            path: "/usr/bin/env".into(),
            kind: vm_config::RootFilesystemEntryKind::File,
            mode: Some(0o644),
            uid: Some(0),
            gid: Some(0),
            content: Some("AA==".into()),
            encoding: Some(vm_config::RootFilesystemEntryEncoding::Base64),
            target: None,
            executable: false,
        });
        let config = vm_config::RootFilesystemConfig {
            disable_default_base_layer: true,
            lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot { entries }],
            ..Default::default()
        };
        let limits = ResourceLimits {
            max_inode_count: Some(AGENTOS_BOOTSTRAP_DIRECTORIES.len() + 16),
            max_filesystem_bytes: Some(
                (CA_CERTIFICATES_BUNDLE.len()
                    + CA_CERTIFICATES_SYMLINK_TARGET.len()
                    + BASELINE_SERVICES.len()
                    + 1) as u64,
            ),
            ..Default::default()
        };
        let mut root = build_root_filesystem(&config, &limits)
            .expect("existing complete root fits original entry/content budget");
        assert_eq!(root.read_file("/usr/bin/env").unwrap(), vec![0]);
    }

    #[test]
    fn restored_directory_gap_uses_caller_seed_metadata() {
        let restored = RootFilesystemSnapshot {
            entries: vec![FilesystemEntry::directory("/")],
        };
        let loaded = FilesystemSnapshot {
            format: String::from(agentos_vm_kernel::root_fs::ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
            bytes: agentos_vm_kernel::root_fs::encode_snapshot(&restored).unwrap(),
        };
        let config = vm_config::RootFilesystemConfig {
            lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                entries: vec![vm_config::RootFilesystemEntry {
                    path: "/workspace".into(),
                    kind: vm_config::RootFilesystemEntryKind::Directory,
                    mode: Some(0o770),
                    uid: Some(42),
                    gid: Some(43),
                    content: None,
                    encoding: None,
                    target: None,
                    executable: true,
                }],
            }],
            ..Default::default()
        };
        let mut mounts = build_root_mount_table_with_loaded_snapshot(
            &config,
            Some(&loaded),
            &ResourceLimits::default(),
        )
        .unwrap();
        complete_agentos_root_directories(&mut mounts, &config, false).unwrap();
        let stat = mounts.lstat("/workspace").unwrap();
        assert_eq!((stat.mode & 0o7777, stat.uid, stat.gid), (0o770, 42, 43));
        assert!(!mounts.exists("/usr/bin/env"));
    }

    #[test]
    fn product_bootstrap_preserves_custom_directory_and_dangling_symlink() {
        let config = vm_config::RootFilesystemConfig {
            disable_default_base_layer: true,
            lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                entries: vec![
                    vm_config::RootFilesystemEntry {
                        path: "/workspace".into(),
                        kind: vm_config::RootFilesystemEntryKind::Directory,
                        mode: Some(0o770),
                        uid: Some(42),
                        gid: Some(43),
                        content: None,
                        encoding: None,
                        target: None,
                        executable: true,
                    },
                    vm_config::RootFilesystemEntry {
                        path: "/boot".into(),
                        kind: vm_config::RootFilesystemEntryKind::Symlink,
                        mode: Some(0o777),
                        uid: Some(0),
                        gid: Some(0),
                        content: None,
                        encoding: None,
                        target: Some("/missing".into()),
                        executable: false,
                    },
                ],
            }],
            ..Default::default()
        };
        let mounts = completed_product_root(&config);
        let workspace = mounts.lstat("/workspace").unwrap();
        assert_eq!(
            (workspace.mode & 0o7777, workspace.uid, workspace.gid),
            (0o770, 42, 43)
        );
        assert!(mounts.lstat("/boot").unwrap().is_symbolic_link);
        assert_eq!(mounts.lstat("/var/tmp").unwrap().mode & 0o7777, 0o1777);
    }

    #[test]
    fn product_bootstrap_does_not_mutate_native_roots() {
        let config = vm_config::RootFilesystemConfig::default();
        let mut mounts = build_root_mount_table(&config, &ResourceLimits::default()).unwrap();
        let before = mounts.root_usage().unwrap();
        complete_agentos_root_directories(&mut mounts, &config, true).unwrap();
        assert_eq!(mounts.root_usage().unwrap(), before);
        assert!(mounts.lstat("/boot").is_err());
    }

    #[test]
    fn readonly_product_bootstrap_is_completed_before_seal() {
        let config = vm_config::RootFilesystemConfig {
            mode: vm_config::RootFilesystemMode::ReadOnly,
            ..Default::default()
        };
        let mut mounts = completed_product_root(&config);
        let root = mounts
            .root_virtual_filesystem_mut::<RootFileSystem>()
            .unwrap();
        root.finish_bootstrap();
        assert!(root.lstat("/boot").unwrap().is_directory);
        assert_eq!(
            root.write_file("/new", vec![1]).unwrap_err().code(),
            "EROFS"
        );
    }

    #[test]
    fn completed_root_still_enters_kernel_driver_quota_admission() {
        use agentos_vm_kernel::command_registry::CommandDriver;
        use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig};
        let config = vm_config::RootFilesystemConfig::default();
        let mut mounts = completed_product_root(&config);
        let usage = mounts.root_usage().unwrap();
        let mut kernel_config = KernelVmConfig::new("bootstrap-quota");
        kernel_config.resources.max_inode_count = Some(usage.inode_count);
        let mut kernel = KernelVm::new(mounts, kernel_config);
        let error = kernel
            .register_driver_for_operator(CommandDriver::new("test", ["new-command"]))
            .unwrap_err();
        assert_eq!(error.code(), "ENOSPC");
        assert!(!kernel.exists_for_operator("/bin/new-command").unwrap());
    }

    #[test]
    fn no_base_scaffold_preserves_explicit_environment_stub_content() {
        let config = vm_config::RootFilesystemConfig {
            disable_default_base_layer: true,
            lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                entries: vec![vm_config::RootFilesystemEntry {
                    path: "/usr/bin/env".into(),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: Some(0o750),
                    uid: Some(42),
                    gid: Some(43),
                    content: Some("custom-env".into()),
                    encoding: None,
                    target: None,
                    executable: true,
                }],
            }],
            ..Default::default()
        };
        let mut root = build_root_filesystem(&config, &ResourceLimits::default()).unwrap();
        assert_eq!(
            root.read_file("/usr/bin/env").unwrap(),
            b"custom-env".to_vec()
        );
        let stat = root.lstat("/usr/bin/env").unwrap();
        assert_eq!((stat.mode & 0o7777, stat.uid, stat.gid), (0o750, 42, 43));
    }

    #[test]
    fn restored_scaffold_preserves_directory_metadata_without_fresh_env_sentinel() {
        let mut directory = FilesystemEntry::directory("/tmp");
        directory.mode = 0o770;
        directory.uid = 42;
        directory.gid = 43;
        let snapshot = RootFilesystemSnapshot {
            entries: vec![directory],
        };
        let loaded = FilesystemSnapshot {
            format: String::from(agentos_vm_kernel::root_fs::ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
            bytes: agentos_vm_kernel::root_fs::encode_snapshot(&snapshot).unwrap(),
        };
        let root = build_root_filesystem_with_loaded_snapshot(
            &Default::default(),
            Some(&loaded),
            &ResourceLimits::default(),
        )
        .unwrap();
        let stat = root.lstat("/tmp").unwrap();
        assert_eq!((stat.mode & 0o7777, stat.uid, stat.gid), (0o770, 42, 43));
        assert!(
            !root.exists("/usr/bin/env"),
            "restoring state must not inject a fresh environment sentinel"
        );
    }

    #[test]
    fn builds_root_filesystem_from_snapshot_lower_and_bootstrap_upper() {
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                    entries: vec![vm_config::RootFilesystemEntry {
                        path: String::from("/workspace/value.txt"),
                        kind: vm_config::RootFilesystemEntryKind::File,
                        mode: None,
                        uid: None,
                        gid: None,
                        content: Some(String::from("lower")),
                        encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                        target: None,
                        executable: false,
                    }],
                }],
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: String::from("/workspace/value.txt"),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(String::from("upper")),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }],
                ..vm_config::RootFilesystemConfig::default()
            },
            &ResourceLimits::default(),
        )
        .expect("build root filesystem");

        assert_eq!(
            root.read_file("/workspace/value.txt")
                .expect("read merged value"),
            b"upper".to_vec()
        );
    }

    #[test]
    fn installs_default_linux_services_database() {
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                ..vm_config::RootFilesystemConfig::default()
            },
            &ResourceLimits::default(),
        )
        .expect("build root filesystem");

        assert_eq!(
            root.read_file(SERVICES_GUEST_PATH)
                .expect("read default services"),
            BASELINE_SERVICES.as_bytes()
        );
    }

    #[test]
    fn decodes_base64_root_filesystem_entries() {
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: String::from("/bin/tool"),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(String::from("dG9vbA==")),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Base64),
                    target: None,
                    executable: true,
                }],
                ..vm_config::RootFilesystemConfig::default()
            },
            &ResourceLimits::default(),
        )
        .expect("build root filesystem");

        assert_eq!(root.read_file("/bin/tool").expect("read file"), b"tool");
        assert_eq!(
            root.stat("/bin/tool").expect("stat file").mode & 0o777,
            0o755
        );
    }

    #[test]
    fn serializes_root_snapshot_entries_as_utf8_or_base64() {
        let text = root_snapshot_entry(&FilesystemEntry {
            path: String::from("/workspace/text.txt"),
            kind: FilesystemEntryKind::File,
            mode: 0o755,
            uid: 501,
            gid: 20,
            content: Some(b"hello".to_vec()),
            target: None,
        });

        assert_eq!(text.path, "/workspace/text.txt");
        assert_eq!(text.kind, ProtocolRootFilesystemEntryKind::File);
        assert_eq!(text.mode, Some(0o755));
        assert_eq!(text.uid, Some(501));
        assert_eq!(text.gid, Some(20));
        assert_eq!(text.content.as_deref(), Some("hello"));
        assert_eq!(
            text.encoding,
            Some(ProtocolRootFilesystemEntryEncoding::Utf8)
        );
        assert!(text.executable);

        let binary = root_snapshot_entry(&FilesystemEntry {
            path: String::from("/workspace/binary.bin"),
            kind: FilesystemEntryKind::File,
            mode: 0o644,
            uid: 0,
            gid: 0,
            content: Some(vec![0xff, 0x00]),
            target: None,
        });

        assert_eq!(binary.content.as_deref(), Some("/wA="));
        assert_eq!(
            binary.encoding,
            Some(ProtocolRootFilesystemEntryEncoding::Base64)
        );
        assert!(!binary.executable);
    }

    #[test]
    fn builds_protocol_descriptor_from_config_without_normalizing_optional_fields() {
        let descriptor =
            root_filesystem_protocol_descriptor_from_config(&vm_config::RootFilesystemConfig {
                mode: vm_config::RootFilesystemMode::ReadOnly,
                disable_default_base_layer: true,
                lowers: vec![
                    vm_config::RootFilesystemLowerDescriptor::BundledBaseFilesystem,
                    vm_config::RootFilesystemLowerDescriptor::Snapshot {
                        entries: vec![vm_config::RootFilesystemEntry {
                            path: String::from("relative/lower.txt"),
                            kind: vm_config::RootFilesystemEntryKind::File,
                            mode: None,
                            uid: None,
                            gid: None,
                            content: Some(String::from("lower")),
                            encoding: None,
                            target: None,
                            executable: false,
                        }],
                    },
                ],
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: String::from("/bin/tool"),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: Some(0o700),
                    uid: Some(1000),
                    gid: Some(1000),
                    content: Some(String::from("dG9vbA==")),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Base64),
                    target: None,
                    executable: true,
                }],
            });

        assert_eq!(descriptor.mode, ProtocolRootFilesystemMode::ReadOnly);
        assert!(descriptor.disable_default_base_layer);
        assert_eq!(descriptor.lowers.len(), 2);
        assert!(matches!(
            descriptor.lowers[0],
            ProtocolRootFilesystemLowerDescriptor::BundledBaseFilesystemLower
        ));
        let ProtocolRootFilesystemLowerDescriptor::SnapshotRootFilesystemLower(snapshot) =
            &descriptor.lowers[1]
        else {
            panic!("expected snapshot lower");
        };
        assert_eq!(snapshot.entries[0].path, "relative/lower.txt");
        assert_eq!(snapshot.entries[0].mode, None);
        assert_eq!(snapshot.entries[0].encoding, None);

        let entry = &descriptor.bootstrap_entries[0];
        assert_eq!(entry.path, "/bin/tool");
        assert_eq!(entry.kind, ProtocolRootFilesystemEntryKind::File);
        assert_eq!(entry.mode, Some(0o700));
        assert_eq!(entry.uid, Some(1000));
        assert_eq!(entry.gid, Some(1000));
        assert_eq!(entry.content.as_deref(), Some("dG9vbA=="));
        assert_eq!(
            entry.encoding,
            Some(ProtocolRootFilesystemEntryEncoding::Base64)
        );
        assert!(entry.executable);
    }

    #[test]
    fn maps_root_filesystem_modes_to_kernel_modes() {
        assert_eq!(
            root_filesystem_mode_from_config(vm_config::RootFilesystemMode::Ephemeral),
            KernelRootFilesystemMode::Ephemeral
        );
        assert_eq!(
            root_filesystem_mode_from_config(vm_config::RootFilesystemMode::ReadOnly),
            KernelRootFilesystemMode::ReadOnly
        );
        assert_eq!(
            protocol_root_filesystem_mode(ProtocolRootFilesystemMode::Ephemeral),
            KernelRootFilesystemMode::Ephemeral
        );
        assert_eq!(
            protocol_root_filesystem_mode(ProtocolRootFilesystemMode::ReadOnly),
            KernelRootFilesystemMode::ReadOnly
        );
    }

    #[test]
    fn restored_snapshot_replaces_config_lowers_but_preserves_bootstrap_entries() {
        let restored = RootFilesystemSnapshot {
            entries: vec![FilesystemEntry::file(
                "/workspace/restored.txt",
                b"restored",
            )],
        };
        let loaded_snapshot = FilesystemSnapshot {
            format: String::from(agentos_vm_kernel::root_fs::ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
            bytes: agentos_vm_kernel::root_fs::encode_snapshot(&restored)
                .expect("encode restored snapshot"),
        };
        let mut root = build_root_filesystem_with_loaded_snapshot(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                    entries: vec![vm_config::RootFilesystemEntry {
                        path: String::from("/workspace/ignored-lower.txt"),
                        kind: vm_config::RootFilesystemEntryKind::File,
                        mode: None,
                        uid: None,
                        gid: None,
                        content: Some(String::from("ignored")),
                        encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                        target: None,
                        executable: false,
                    }],
                }],
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: String::from("/workspace/bootstrap.txt"),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(String::from("bootstrap")),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }],
                ..vm_config::RootFilesystemConfig::default()
            },
            Some(&loaded_snapshot),
            &ResourceLimits::default(),
        )
        .expect("build root filesystem from restored snapshot");

        assert_eq!(
            root.read_file("/workspace/restored.txt")
                .expect("read restored file"),
            b"restored".to_vec()
        );
        assert_eq!(
            root.read_file("/workspace/bootstrap.txt")
                .expect("read bootstrap file"),
            b"bootstrap".to_vec()
        );
        assert!(
            root.read_file("/workspace/ignored-lower.txt").is_err(),
            "restored snapshots should replace configured lowers"
        );
    }

    #[test]
    fn installs_default_ca_in_read_only_roots_before_bootstrap_finishes() {
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig {
                mode: vm_config::RootFilesystemMode::ReadOnly,
                disable_default_base_layer: true,
                ..vm_config::RootFilesystemConfig::default()
            },
            &ResourceLimits::default(),
        )
        .expect("build read-only root filesystem");

        assert_eq!(
            root.read_file(CA_CERTIFICATES_GUEST_PATH)
                .expect("read default CA bundle"),
            CA_CERTIFICATES_BUNDLE
        );
        assert_eq!(
            root.read_link(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("read default CA symlink"),
            CA_CERTIFICATES_SYMLINK_TARGET
        );
        assert!(
            root.exists("/usr/bin/env"),
            "adding the CA lower must preserve the kernel's minimal root scaffold"
        );

        root.finish_bootstrap();
        assert_eq!(
            root.read_file(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("read CA through symlink after read-only lock"),
            CA_CERTIFICATES_BUNDLE
        );
        assert!(
            root.write_file(CA_CERTIFICATES_GUEST_PATH, b"replacement")
                .is_err(),
            "read-only root must lock the seeded trust store with all other writes"
        );
    }

    #[test]
    fn installs_default_ca_in_ephemeral_bundled_roots() {
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig::default(),
            &ResourceLimits::default(),
        )
        .expect("build default ephemeral root filesystem");

        assert_eq!(
            root.read_file(CA_CERTIFICATES_GUEST_PATH)
                .expect("read default CA bundle from bundled root"),
            CA_CERTIFICATES_BUNDLE
        );
        assert_eq!(
            root.read_file(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("read default CA through bundled-root symlink"),
            CA_CERTIFICATES_BUNDLE
        );
    }

    #[test]
    fn explicit_ca_entries_replace_defaults_across_lower_and_bootstrap_layers() {
        let custom_bundle = "custom lower bundle\n";
        let custom_cert_pem = "custom bootstrap cert.pem\n";
        let mut root = build_root_filesystem(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                lowers: vec![vm_config::RootFilesystemLowerDescriptor::Snapshot {
                    entries: vec![vm_config::RootFilesystemEntry {
                        path: CA_CERTIFICATES_GUEST_PATH.to_string(),
                        kind: vm_config::RootFilesystemEntryKind::File,
                        mode: None,
                        uid: None,
                        gid: None,
                        content: Some(custom_bundle.to_string()),
                        encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                        target: None,
                        executable: false,
                    }],
                }],
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: CA_CERTIFICATES_SYMLINK_PATH.to_string(),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(custom_cert_pem.to_string()),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }],
                ..vm_config::RootFilesystemConfig::default()
            },
            &ResourceLimits::default(),
        )
        .expect("build root filesystem with custom trust");

        assert_eq!(
            root.read_file(CA_CERTIFICATES_GUEST_PATH)
                .expect("read custom lower bundle"),
            custom_bundle.as_bytes()
        );
        assert_eq!(
            root.read_file(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("read regular custom cert.pem"),
            custom_cert_pem.as_bytes()
        );
        assert!(
            !root
                .lstat(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("lstat custom cert.pem")
                .is_symbolic_link,
            "a regular custom cert.pem must replace the default symlink"
        );
    }

    #[test]
    fn restored_snapshot_ca_replaces_default_ca() {
        let restored_bundle = b"restored trust bundle\n";
        let restored = RootFilesystemSnapshot {
            entries: vec![
                FilesystemEntry::file(CA_CERTIFICATES_GUEST_PATH, restored_bundle),
                FilesystemEntry::symlink(
                    CA_CERTIFICATES_SYMLINK_PATH,
                    CA_CERTIFICATES_SYMLINK_TARGET,
                ),
            ],
        };
        let loaded_snapshot = FilesystemSnapshot {
            format: String::from(agentos_vm_kernel::root_fs::ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
            bytes: agentos_vm_kernel::root_fs::encode_snapshot(&restored)
                .expect("encode restored snapshot"),
        };
        let mut root = build_root_filesystem_with_loaded_snapshot(
            &vm_config::RootFilesystemConfig {
                disable_default_base_layer: true,
                bootstrap_entries: vec![vm_config::RootFilesystemEntry {
                    path: CA_CERTIFICATES_SYMLINK_PATH.to_string(),
                    kind: vm_config::RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some("restored custom cert.pem\n".to_string()),
                    encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }],
                ..vm_config::RootFilesystemConfig::default()
            },
            Some(&loaded_snapshot),
            &ResourceLimits::default(),
        )
        .expect("build root filesystem from restored snapshot");

        assert_eq!(
            root.read_file(CA_CERTIFICATES_GUEST_PATH)
                .expect("read restored CA bundle"),
            restored_bundle
        );
        assert_eq!(
            root.read_file(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("read restored custom regular cert.pem"),
            b"restored custom cert.pem\n"
        );
        assert!(
            !root
                .lstat(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("lstat restored custom cert.pem")
                .is_symbolic_link
        );
    }
}
