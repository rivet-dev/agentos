//! Root filesystem bootstrap and snapshot helpers extracted from vm.rs.

use crate::protocol::RootFilesystemEntry;
use crate::state::SidecarKernel;
use crate::VmError;

use agentos_vm_kernel::root_fs::{
    FilesystemEntry as KernelFilesystemEntry, RootFilesystemSnapshot,
};
use agentos_vm_kernel::vfs::VirtualFileSystem;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn root_snapshot_entry(entry: &KernelFilesystemEntry) -> RootFilesystemEntry {
    crate::core::root_snapshot_entry(entry)
}

pub(crate) fn root_snapshot_entries(snapshot: &RootFilesystemSnapshot) -> Vec<RootFilesystemEntry> {
    snapshot.entries.iter().map(root_snapshot_entry).collect()
}

pub(crate) fn root_snapshot_from_entries(
    entries: &[RootFilesystemEntry],
) -> Result<RootFilesystemSnapshot, VmError> {
    crate::core::root_snapshot_from_entries(entries)
        .map_err(|error| VmError::InvalidState(error.to_string()))
}

pub(crate) fn apply_root_filesystem_entry<F>(
    filesystem: &mut F,
    entry: &RootFilesystemEntry,
) -> Result<(), VmError>
where
    F: VirtualFileSystem,
{
    crate::core::apply_root_filesystem_entry(filesystem, entry)
        .map_err(|error| VmError::InvalidState(error.to_string()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KernelCommandInventory {
    pub(crate) names: BTreeSet<String>,
    pub(crate) search_roots: Vec<String>,
}

/// Enumerate legacy command mounts from the live kernel VFS for one immediate
/// registration/PATH rebuild. Nothing returned here is retained as pathname
/// authority; launch resolution revalidates the selected file in the kernel.
pub(crate) fn discover_kernel_commands(
    kernel: &mut SidecarKernel,
) -> Result<KernelCommandInventory, VmError> {
    let commands = discover_command_guest_paths(kernel)?;
    Ok(KernelCommandInventory {
        names: commands.keys().cloned().collect(),
        search_roots: commands
            .values()
            .filter_map(|path| path.rsplit_once('/').map(|(parent, _)| parent.to_owned()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    })
}

pub(crate) fn discover_command_guest_paths(
    kernel: &mut SidecarKernel,
) -> Result<BTreeMap<String, String>, VmError> {
    let mut command_guest_paths = BTreeMap::new();
    let command_roots = match kernel.read_dir_for_operator("/__agentos/commands") {
        Ok(roots) => roots,
        Err(error) if error.code() == "ENOENT" => return Ok(command_guest_paths),
        Err(error) => return Err(crate::service::kernel_error(error)),
    };

    let mut ordered_roots = command_roots
        .into_iter()
        .filter(|entry| !entry.is_empty() && entry.chars().all(|ch| ch.is_ascii_digit()))
        .collect::<Vec<_>>();
    ordered_roots.sort();

    for root in ordered_roots {
        let guest_root = format!("/__agentos/commands/{root}");
        let entries = kernel
            .read_dir_for_operator(&guest_root)
            .map_err(crate::service::kernel_error)?;

        for entry in entries {
            if entry.starts_with('.') || command_guest_paths.contains_key(&entry) {
                continue;
            }
            let guest_path = format!("{guest_root}/{entry}");
            let stat = kernel
                .stat_for_operator(&guest_path)
                .map_err(crate::service::kernel_error)?;
            if stat.is_directory {
                continue;
            }
            command_guest_paths.insert(entry, guest_path);
        }
    }

    Ok(command_guest_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_vm_kernel::kernel::KernelVmConfig;
    use agentos_vm_kernel::mount_table::MountTable;
    use agentos_vm_kernel::permissions::Permissions;
    use agentos_vm_kernel::vfs::MemoryFileSystem;

    fn test_kernel() -> SidecarKernel {
        let mut config = KernelVmConfig::new("vm-transient-command-discovery");
        config.permissions = Permissions::allow_all();
        SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config)
    }

    #[test]
    fn kernel_command_inventory_tracks_live_files_and_roots() {
        let mut kernel = test_kernel();
        kernel
            .mkdir("/__agentos/commands/001", true)
            .expect("create first command root");
        kernel
            .mkdir("/__agentos/commands/002/directory", true)
            .expect("create non-command directory");
        kernel
            .write_file(
                "/__agentos/commands/001/alpha",
                b"#!/usr/bin/env node\n".to_vec(),
            )
            .expect("write command");
        kernel
            .write_file(
                "/__agentos/commands/001/.hidden",
                b"#!/usr/bin/env node\n".to_vec(),
            )
            .expect("write hidden entry");

        let discovered = discover_kernel_commands(&mut kernel).unwrap();
        assert_eq!(discovered.names, BTreeSet::from([String::from("alpha")]));
        assert_eq!(
            discovered.search_roots,
            vec![String::from("/__agentos/commands/001")]
        );

        kernel
            .remove_file("/__agentos/commands/001/alpha")
            .expect("remove command after initial discovery");
        assert_eq!(
            discover_kernel_commands(&mut kernel).unwrap(),
            KernelCommandInventory::default(),
            "transient discovery must not retain deleted commands or roots"
        );
    }
}
