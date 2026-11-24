use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, SpawnOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::poll::{PollTargetEntry, POLLIN};
use agentos_vm_kernel::socket_table::{InetSocketAddress, SocketSpec};
use agentos_vm_kernel::vfs::MemoryFileSystem;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

#[test]
fn empty_udp_wakes_kernel_poll_and_socket_readiness() {
    for payload in [b"x".as_slice(), b"".as_slice()] {
        let mut config = KernelVmConfig::new("empty-udp-poll-wake");
        config.permissions = Permissions::allow_all();
        let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
        kernel
            .register_driver(CommandDriver::new("shell", ["sh"]))
            .unwrap();
        let process = kernel
            .spawn_process(
                "sh",
                Vec::new(),
                SpawnOptions {
                    requester_driver: Some("shell".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let pid = process.pid();
        let tx = kernel
            .socket_create("shell", pid, SocketSpec::udp())
            .unwrap();
        let rx = kernel
            .socket_create("shell", pid, SocketSpec::udp())
            .unwrap();
        kernel
            .socket_bind_inet("shell", pid, tx, InetSocketAddress::new("127.0.0.1", 51021))
            .unwrap();
        kernel
            .socket_bind_inet("shell", pid, rx, InetSocketAddress::new("127.0.0.1", 51022))
            .unwrap();
        let signals = Arc::new(AtomicUsize::new(0));
        let observed = signals.clone();
        kernel.set_socket_readiness_sink(Some(
            move |event: agentos_vm_kernel::socket_table::SocketReadiness| {
                assert_eq!(event.socket_id, rx);
                observed.fetch_add(1, Ordering::SeqCst);
            },
        ));
        let wait = kernel.poll_wait_handle();
        let generation = wait.snapshot();
        assert_eq!(
            kernel
                .poll_targets("shell", pid, vec![PollTargetEntry::socket(rx, POLLIN)], 0)
                .unwrap()
                .ready_count,
            0
        );
        assert_eq!(
            kernel
                .socket_send_to_inet_loopback(
                    "shell",
                    pid,
                    tx,
                    InetSocketAddress::new("127.0.0.1", 51022),
                    payload
                )
                .unwrap(),
            payload.len()
        );
        assert_eq!(
            signals.load(Ordering::SeqCst),
            1,
            "SocketTable wake is independent"
        );
        assert_eq!(
            kernel
                .poll_targets("shell", pid, vec![PollTargetEntry::socket(rx, POLLIN)], 0)
                .unwrap()
                .ready_count,
            1
        );
        assert!(
            wait.wait_for_change(generation, Some(Duration::ZERO)),
            "kernel poll generation must advance for payload length {}",
            payload.len()
        );
        let packet = kernel
            .socket_recv_datagram("shell", pid, rx, 1)
            .unwrap()
            .unwrap();
        assert_eq!(packet.payload(), payload);
    }
}
