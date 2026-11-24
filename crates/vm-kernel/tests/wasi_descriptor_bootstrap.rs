//! Actual kernel table/ownership controls for the per-execution WASI bootstrap.
use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::fd_table::{FD_CLOEXEC, F_SETFD, O_RDONLY};
use agentos_vm_kernel::kernel::{KernelProcessHandle, KernelVm, KernelVmConfig, SpawnOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::process_table::{
    ProcessPermissionTier, ProcessResourceLimit, ProcessResourceLimitKind, SigmaskHow, SignalSet,
    SIGTERM,
};
use agentos_vm_kernel::vfs::MemoryFileSystem;

fn process(tier: ProcessPermissionTier) -> (KernelVm<MemoryFileSystem>, KernelProcessHandle) {
    let mut config = KernelVmConfig::new("descriptor-bootstrap-test");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    kernel.mkdir("/workspace", true).expect("seed workspace");
    kernel
        .write_file("/source", b"abc".to_vec())
        .expect("seed file");
    kernel
        .register_driver(CommandDriver::new("wasm", ["bootstrap"]))
        .expect("register owning driver");
    let handle = kernel
        .spawn_process(
            "bootstrap",
            Vec::new(),
            SpawnOptions {
                requester_driver: Some("wasm".to_owned()),
                permission_tier: Some(tier),
                ..SpawnOptions::default()
            },
        )
        .expect("spawn process");
    (kernel, handle)
}

#[test]
fn initialized_roots_and_descriptor_identities_are_coherent_and_idempotent() {
    let (mut kernel, handle) = process(ProcessPermissionTier::Full);
    let first = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", handle.pid())
        .unwrap();
    assert!(!first.preopens.is_empty());
    for root in &first.preopens {
        let descriptor = first
            .descriptors
            .iter()
            .find(|entry| entry.fd == root.fd)
            .expect("root exists in the same descriptor view");
        assert_eq!(descriptor.rights_base, root.rights_base);
        assert_eq!(descriptor.rights_inheriting, root.rights_inheriting);
    }
    assert_eq!(
        kernel
            .initialize_wasi_descriptor_bootstrap("wasm", handle.pid())
            .unwrap(),
        first,
        "repeated bootstrap does not allocate roots or change open-description identity"
    );
}

#[test]
fn bootstrap_reads_current_table_after_earlier_snapshot_close_and_reuse() {
    let (mut kernel, handle) = process(ProcessPermissionTier::Full);
    let pid = handle.pid();
    let initial = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", pid)
        .unwrap();
    let old_root = initial.preopens[0].fd;
    let old_identity = initial
        .descriptors
        .iter()
        .find(|entry| entry.fd == old_root)
        .unwrap()
        .description_id;
    kernel.fd_close("wasm", pid, old_root).unwrap();
    let replacement = kernel
        .fd_open("wasm", pid, "/source", O_RDONLY, None)
        .unwrap();
    assert_eq!(
        replacement, old_root,
        "Linux reuses the lowest free descriptor"
    );
    let current = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", pid)
        .unwrap();
    assert!(
        !current.preopens.iter().any(|entry| entry.fd == replacement),
        "a reused ordinary descriptor must not regain its old capability-root identity"
    );
    let descriptor = current
        .descriptors
        .iter()
        .find(|entry| entry.fd == replacement)
        .unwrap();
    assert_ne!(descriptor.description_id, old_identity);
    assert_eq!(kernel.fd_read("wasm", pid, replacement, 3).unwrap(), b"abc");
}

#[test]
fn fork_half_preserves_alias_lifetime_flags_and_nonzero_mask() {
    let (mut kernel, parent) = process(ProcessPermissionTier::Full);
    let original = kernel
        .fd_open("wasm", parent.pid(), "/source", O_RDONLY, None)
        .unwrap();
    let alias = kernel.fd_dup("wasm", parent.pid(), original).unwrap();
    kernel
        .fd_fcntl("wasm", parent.pid(), original, F_SETFD, FD_CLOEXEC)
        .unwrap();
    parent
        .sigprocmask(
            SigmaskHow::Block,
            SignalSet::from_signals([SIGTERM]).unwrap(),
        )
        .unwrap();
    let child = kernel
        .spawn_process_with_process_group_preserving_cloexec(
            "bootstrap",
            Vec::new(),
            SpawnOptions {
                requester_driver: Some("wasm".to_owned()),
                parent_pid: Some(parent.pid()),
                ..SpawnOptions::default()
            },
            None,
        )
        .unwrap();
    let inherited = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", child.pid())
        .unwrap();
    let a = inherited
        .descriptors
        .iter()
        .find(|entry| entry.fd == original)
        .unwrap();
    let b = inherited
        .descriptors
        .iter()
        .find(|entry| entry.fd == alias)
        .unwrap();
    assert_eq!(a.description_id, b.description_id);
    assert_eq!(a.fd_flags & FD_CLOEXEC, FD_CLOEXEC);
    assert_eq!(b.fd_flags & FD_CLOEXEC, 0);
    assert!(child
        .sigprocmask(SigmaskHow::Block, SignalSet::empty())
        .unwrap()
        .contains(SIGTERM));
    kernel.fd_close("wasm", parent.pid(), original).unwrap();
    kernel.fd_close("wasm", parent.pid(), alias).unwrap();
    kernel.fd_close("wasm", child.pid(), original).unwrap();
    let current = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", child.pid())
        .unwrap();
    assert!(!current.descriptors.iter().any(|entry| entry.fd == original));
    assert_eq!(
        current
            .descriptors
            .iter()
            .find(|entry| entry.fd == alias)
            .unwrap()
            .description_id,
        b.description_id
    );
    assert_eq!(
        kernel.fd_read("wasm", child.pid(), alias, 3).unwrap(),
        b"abc"
    );
}

#[test]
fn bootstrap_cannot_initialize_another_driver_or_missing_process() {
    let (mut kernel, handle) = process(ProcessPermissionTier::Full);
    assert!(kernel
        .wasi_preopens("wasm", handle.pid())
        .unwrap()
        .is_empty());
    let wrong_driver = kernel
        .fd_snapshot("other-driver", handle.pid())
        .unwrap_err();
    assert_eq!(
        wrong_driver.code(),
        "EPERM",
        "existing driver ownership errno"
    );
    assert_eq!(
        kernel
            .initialize_wasi_descriptor_bootstrap("other-driver", handle.pid())
            .unwrap_err()
            .code(),
        wrong_driver.code()
    );
    let missing = kernel.fd_snapshot("wasm", u32::MAX).unwrap_err();
    assert_eq!(missing.code(), "ESRCH", "existing missing-process errno");
    assert_eq!(
        kernel
            .initialize_wasi_descriptor_bootstrap("wasm", u32::MAX)
            .unwrap_err()
            .code(),
        missing.code()
    );
    assert!(kernel
        .wasi_preopens("wasm", handle.pid())
        .unwrap()
        .is_empty());
}

#[test]
fn isolated_tier_keeps_capability_roots_empty() {
    let (mut kernel, handle) = process(ProcessPermissionTier::Isolated);
    let initial = kernel.fd_snapshot("wasm", handle.pid()).unwrap();
    let bootstrap = kernel
        .initialize_wasi_descriptor_bootstrap("wasm", handle.pid())
        .unwrap();
    assert!(bootstrap.preopens.is_empty());
    assert_eq!(bootstrap.descriptors, initial);
}

#[test]
fn preopen_resource_failure_is_returned_without_a_success_snapshot() {
    let (mut kernel, handle) = process(ProcessPermissionTier::Full);
    kernel
        .set_resource_limit(
            "wasm",
            handle.pid(),
            ProcessResourceLimitKind::OpenFiles,
            ProcessResourceLimit {
                soft: Some(3),
                hard: Some(3),
            },
        )
        .unwrap();
    assert_eq!(
        kernel
            .initialize_wasi_descriptor_bootstrap("wasm", handle.pid())
            .unwrap_err()
            .code(),
        "EMFILE"
    );
}
