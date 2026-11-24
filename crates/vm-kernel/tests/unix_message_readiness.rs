#![cfg(not(target_arch = "wasm32"))]

use agentos_vm_kernel::admission::{ResourceClass, ResourceLedger, ResourceLimit};
use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelProcessHandle, KernelVm, KernelVmConfig, SpawnOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::poll::{PollTargetEntry, POLLOUT};
use agentos_vm_kernel::socket_table::{
    InetSocketAddress, SocketId, SocketSpec, SocketType, DATAGRAM_METADATA_BYTES,
};
use agentos_vm_kernel::vfs::MemoryFileSystem;
use std::sync::Arc;
use std::time::Duration;

enum UnixPair {
    Fd(u32, u32),
    Socket(SocketId, SocketId),
}

impl UnixPair {
    fn create(kernel: &mut KernelVm<MemoryFileSystem>, pid: u32, fd_route: bool) -> Self {
        if fd_route {
            let (writer, reader) = kernel
                .fd_socketpair("shell", pid, SocketType::Datagram, true, false)
                .unwrap();
            Self::Fd(writer, reader)
        } else {
            let writer = kernel
                .socket_create("shell", pid, SocketSpec::unix_datagram())
                .unwrap();
            let reader = kernel
                .socket_create("shell", pid, SocketSpec::unix_datagram())
                .unwrap();
            kernel
                .socket_connect_pair("shell", pid, writer, reader)
                .unwrap();
            Self::Socket(writer, reader)
        }
    }

    fn pollout(&self, kernel: &KernelVm<MemoryFileSystem>, pid: u32) -> bool {
        let target = match self {
            Self::Fd(writer, _) => PollTargetEntry::fd(*writer, POLLOUT),
            Self::Socket(writer, _) => PollTargetEntry::socket(*writer, POLLOUT),
        };
        let result = kernel.poll_targets("shell", pid, vec![target], 0).unwrap();
        result.targets[0].revents.intersects(POLLOUT)
    }

    fn empty_write(
        &self,
        kernel: &mut KernelVm<MemoryFileSystem>,
        pid: u32,
    ) -> Result<usize, String> {
        match self {
            Self::Fd(writer, _) => kernel.fd_write("shell", pid, *writer, b""),
            Self::Socket(writer, _) => kernel.socket_write("shell", pid, *writer, b""),
        }
        .map_err(|error| error.code().to_owned())
    }

    fn read(&self, kernel: &mut KernelVm<MemoryFileSystem>, pid: u32) -> Option<Vec<u8>> {
        match self {
            Self::Fd(_, reader) => {
                kernel.fd_read_with_timeout_result("shell", pid, *reader, 8, Some(Duration::ZERO))
            }
            Self::Socket(_, reader) => kernel.socket_read("shell", pid, *reader, 8),
        }
        .unwrap()
    }

    fn close(self, kernel: &mut KernelVm<MemoryFileSystem>, pid: u32) {
        match self {
            Self::Fd(writer, reader) => {
                kernel.fd_close("shell", pid, writer).unwrap();
                kernel.fd_close("shell", pid, reader).unwrap();
            }
            Self::Socket(writer, reader) => {
                kernel.socket_close("shell", pid, writer).unwrap();
                kernel.socket_close("shell", pid, reader).unwrap();
            }
        }
    }
}

fn fixture(
    byte_limit: usize,
) -> (
    KernelVm<MemoryFileSystem>,
    KernelProcessHandle,
    Arc<ResourceLedger>,
) {
    let mut config = KernelVmConfig::new("unix-message-readiness");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    let ledger = Arc::new(ResourceLedger::root(
        "unix-message-readiness",
        [
            (
                ResourceClass::BufferedBytes,
                ResourceLimit::new(byte_limit, "limits.resources.maxSocketBufferedBytes"),
            ),
            (
                ResourceClass::Datagrams,
                ResourceLimit::new(4, "limits.resources.maxSocketDatagramQueueLen"),
            ),
            (
                ResourceClass::UdpBytes,
                ResourceLimit::new(2048, "limits.udp.maxBufferedBytes"),
            ),
            (
                ResourceClass::UdpDatagrams,
                ResourceLimit::new(1, "limits.udp.maxBufferedDatagrams"),
            ),
        ],
    ));
    kernel
        .set_socket_resource_ledger(Arc::clone(&ledger))
        .unwrap();
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
    (kernel, process, ledger)
}

#[test]
fn unix_message_pollout_respects_minimum_metadata_admission() {
    let mut failures = Vec::new();
    for fd_route in [true, false] {
        let (mut kernel, process, ledger) = fixture(DATAGRAM_METADATA_BYTES - 1);
        let pid = process.pid();
        let baseline_fds = kernel.resource_snapshot().open_fds;
        let pair = UnixPair::create(&mut kernel, pid, fd_route);
        let pollout = pair.pollout(&kernel, pid);
        let write = pair.empty_write(&mut kernel, pid);
        // Capture readiness and admission before either conformance assertion.
        pair.close(&mut kernel, pid);
        assert_eq!(kernel.resource_snapshot().open_fds, baseline_fds);
        assert_eq!(kernel.resource_snapshot().sockets, 0);
        assert_eq!(ledger.usage(ResourceClass::BufferedBytes).used, 0);
        assert_eq!(ledger.usage(ResourceClass::Datagrams).used, 0);
        println!(
            "unix_message_readiness {}",
            serde_json::json!({
                "case":"minimum-metadata", "fdRoute":fd_route,
                "budget":DATAGRAM_METADATA_BYTES-1,"minimumCharge":DATAGRAM_METADATA_BYTES,
                "pollout":pollout,"emptyWrite":write,"cleanupAssertionsPassed":true
            })
        );
        if write != Err("EAGAIN".to_owned()) || pollout {
            failures.push(format!(
                "fd_route={fd_route}: pollout={pollout}, write={write:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn unix_message_pollout_is_independent_of_udp_only_message_limit() {
    let mut failures = Vec::new();
    for fd_route in [true, false] {
        let (mut kernel, process, ledger) = fixture(4096);
        let pid = process.pid();
        let baseline_fds = kernel.resource_snapshot().open_fds;
        let pair = UnixPair::create(&mut kernel, pid, fd_route);
        let initially_ready = pair.pollout(&kernel, pid);
        let tx = kernel
            .socket_create("shell", pid, SocketSpec::udp())
            .unwrap();
        let rx = kernel
            .socket_create("shell", pid, SocketSpec::udp())
            .unwrap();
        kernel
            .socket_bind_inet("shell", pid, tx, InetSocketAddress::new("127.0.0.1", 53280))
            .unwrap();
        kernel
            .socket_bind_inet("shell", pid, rx, InetSocketAddress::new("127.0.0.1", 53281))
            .unwrap();
        kernel
            .socket_send_to_inet_loopback(
                "shell",
                pid,
                tx,
                InetSocketAddress::new("127.0.0.1", 53281),
                b"",
            )
            .unwrap();
        let udp_only_exhausted = !ledger.capacity_available(ResourceClass::UdpDatagrams, 1)
            && ledger.capacity_available(ResourceClass::Datagrams, 1)
            && ledger.capacity_available(ResourceClass::BufferedBytes, DATAGRAM_METADATA_BYTES)
            && ledger.capacity_available(ResourceClass::UdpBytes, DATAGRAM_METADATA_BYTES);
        let pollout_with_udp_full = pair.pollout(&kernel, pid);
        let write = pair.empty_write(&mut kernel, pid);
        let readback = write.is_ok().then(|| pair.read(&mut kernel, pid));
        assert_eq!(ledger.usage(ResourceClass::UdpDatagrams).used, 1);
        assert!(kernel
            .socket_recv_datagram("shell", pid, rx, 1)
            .unwrap()
            .unwrap()
            .payload()
            .is_empty());
        let pollout_after_udp_drain = pair.pollout(&kernel, pid);
        pair.close(&mut kernel, pid);
        kernel.socket_close("shell", pid, tx).unwrap();
        kernel.socket_close("shell", pid, rx).unwrap();
        assert_eq!(kernel.resource_snapshot().open_fds, baseline_fds);
        assert_eq!(kernel.resource_snapshot().sockets, 0);
        for class in [
            ResourceClass::BufferedBytes,
            ResourceClass::Datagrams,
            ResourceClass::UdpBytes,
            ResourceClass::UdpDatagrams,
        ] {
            assert_eq!(ledger.usage(class).used, 0);
        }
        println!(
            "unix_message_readiness {}",
            serde_json::json!({
                "case":"udp-only-limit", "fdRoute":fd_route,"initiallyReady":initially_ready,
                "onlyUdpDatagramsExhausted":udp_only_exhausted,"polloutWithUdpFull":pollout_with_udp_full,
                "emptyWrite":write,"readback":readback,"polloutAfterUdpDrain":pollout_after_udp_drain,
                "cleanupAssertionsPassed":true
            })
        );
        if !initially_ready
            || !udp_only_exhausted
            || !pollout_with_udp_full
            || write != Ok(0)
            || readback != Some(Some(Vec::new()))
            || !pollout_after_udp_drain
        {
            failures.push(format!("fd_route={fd_route}: initial={initially_ready}, udp_only={udp_only_exhausted}, pollout={pollout_with_udp_full}, write={write:?}, read={readback:?}, drained={pollout_after_udp_drain}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
