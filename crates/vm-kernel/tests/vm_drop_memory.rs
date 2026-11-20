//! A disposed VM must give back the memory its kernel tables used.
//!
//! This test lives in its own binary because it reads the process-wide glibc
//! heap counters, which other tests running in parallel would disturb.

#![cfg(all(target_os = "linux", target_env = "gnu"))]

use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, SpawnOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::MemoryFileSystem;

#[repr(C)]
struct Mallinfo2 {
    arena: usize,
    ordblks: usize,
    smblks: usize,
    hblks: usize,
    hblkhd: usize,
    usmblks: usize,
    fsmblks: usize,
    uordblks: usize,
    fordblks: usize,
    keepcost: usize,
}

extern "C" {
    fn mallinfo2() -> Mallinfo2;
}

/// Bytes the allocator has handed out and not had back. Unlike RSS, this does
/// not include free memory that glibc keeps mapped.
fn heap_bytes_in_use() -> usize {
    // SAFETY: mallinfo2 has no preconditions and only reads allocator state.
    let info = unsafe { mallinfo2() };
    info.uordblks + info.hblkhd
}

/// One VM lifetime: a guest process with a PTY and a pipe, then dispose.
fn vm_lifetime(index: usize) {
    let mut config = KernelVmConfig::new(format!("vm-drop-memory-{index}"));
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    kernel
        .register_driver(CommandDriver::new("shell", ["sh"]))
        .expect("register shell");
    let process = kernel
        .spawn_process(
            "sh",
            Vec::new(),
            SpawnOptions {
                requester_driver: Some(String::from("shell")),
                ..SpawnOptions::default()
            },
        )
        .expect("spawn shell");
    kernel.open_pty("shell", process.pid()).expect("open pty");
    kernel.open_pipe("shell", process.pid()).expect("open pipe");
    kernel.dispose().expect("dispose VM");
}

const WARMUP_VMS: usize = 50;
const MEASURED_VMS: usize = 2_000;
// A VM whose tables are never freed keeps several KiB, so 2,000 of them keep
// many MiB. Allow 1 MiB for allocator bookkeeping that is not per VM.
const MAX_GROWTH_BYTES: usize = 1 << 20;

#[test]
fn disposed_vms_return_their_kernel_memory() {
    for index in 0..WARMUP_VMS {
        vm_lifetime(index);
    }
    let baseline = heap_bytes_in_use();

    for index in 0..MEASURED_VMS {
        vm_lifetime(WARMUP_VMS + index);
    }
    let growth = heap_bytes_in_use().saturating_sub(baseline);

    assert!(
        growth < MAX_GROWTH_BYTES,
        "heap in use grew by {growth} bytes over {MEASURED_VMS} disposed VMs \
         (limit {MAX_GROWTH_BYTES}); disposed VMs are not releasing kernel state"
    );
}
