fn mixed_udp_paths() -> SocketPathContext {
    SocketPathContext {
        sandbox_root: PathBuf::from("/tmp/agentos-mixed-udp-test"),
        unix_abstract_namespace: [0; 32],
        unix_socket_host_dir: PathBuf::from("/tmp/agentos-mixed-udp-test/unix"),
        unix_bound_addresses: Default::default(),
        host_net_transfer_descriptions: Default::default(),
        mounts: vec![],
        listen_policy: Default::default(),
        loopback_exempt_ports: Default::default(),
        tcp_loopback_guest_to_host_ports: Default::default(),
        http_loopback_targets: Default::default(),
        http2_loopback_targets: Default::default(),
        udp_loopback_guest_to_host_ports: Default::default(),
        udp_loopback_host_to_guest_ports: Default::default(),
        used_tcp_guest_ports: Default::default(),
        used_udp_guest_ports: Default::default(),
    }
}

#[test]
fn managed_udp_collector_routes_only_kernel_dualstack_wildcards() {
    use agentos_vm_host_interface::LocalVmHost;
    use agentos_vm_kernel::{
        command_registry::CommandDriver, kernel::KernelVmConfig, mount_table::MountTable,
        permissions::Permissions, vfs::MemoryFileSystem,
    };
    for wildcard in [true, false] {
        let driver = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
        let runtime = driver.handle();
        let mut config = KernelVmConfig::new("mixed-udp");
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        kernel
            .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
            .unwrap();
        let process_handle = kernel
            .create_virtual_process(
                EXECUTION_DRIVER_NAME,
                EXECUTION_DRIVER_NAME,
                WASM_COMMAND,
                vec![WASM_COMMAND.into()],
                Default::default(),
            )
            .unwrap();
        let mut process = ActiveProcess::new(
            process_handle.pid(),
            process_handle,
            runtime.clone(),
            Default::default(),
            agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
            GuestRuntimeKind::JavaScript,
            ActiveExecution::HostFunction(Default::default()),
        );
        let mut receiver = ActiveUdpSocket::new(
            &mut kernel,
            process.kernel_pid,
            UdpFamily::Ipv6,
            Arc::clone(runtime.resources()),
            runtime.clone(),
            reactor_io_limits(&crate::limits::VmLimits::default()),
        )
        .unwrap();
        let mut paths = mixed_udp_paths();
        receiver
            .bind(
                &mut kernel,
                process.kernel_pid,
                Some(if wildcard { "::" } else { "::1" }),
                41096,
                &paths,
            )
            .unwrap();
        let receiver_id = receiver.kernel_socket_id.unwrap();
        // A native owner may coexist, but the mixed-family VM route must never
        // borrow its host port. No actual owner task/socket is spawned here.
        receiver.update_connection(|state| {
            state.native_local_addr = Some("[::1]:42096".parse().unwrap())
        });
        process.udp_sockets.insert("receiver".into(), receiver);
        {
            let collect = |paths: &mut SocketPathContext| {
                crate::execution::process::collect_socket_port_state(
                    &kernel,
                    "root",
                    &process,
                    &mut paths.tcp_loopback_guest_to_host_ports,
                    &mut paths.http_loopback_targets,
                    &mut paths.http2_loopback_targets,
                    &mut paths.udp_loopback_guest_to_host_ports,
                    &mut paths.udp_loopback_host_to_guest_ports,
                    &mut paths.used_tcp_guest_ports,
                    &mut paths.used_udp_guest_ports,
                );
            };
            collect(&mut paths);
            assert_eq!(
                paths.translate_udp_loopback_port(SocketFamily::Ipv4, 41096),
                wildcard.then_some(41096)
            );
            assert_eq!(
                paths.translate_udp_loopback_port(SocketFamily::Ipv4, 42096),
                None
            );
            assert!(paths.loopback_exempt_ports.is_empty());
            if wildcard {
                // Existing explicit IPv4 route before collection retains priority;
                // a later explicit insertion likewise overrides the fallback.
                paths
                    .udp_loopback_guest_to_host_ports
                    .insert((SocketFamily::Ipv4, 41096), 43096);
                collect(&mut paths);
                assert_eq!(
                    paths.translate_udp_loopback_port(SocketFamily::Ipv4, 41096),
                    Some(43096)
                );
                paths
                    .udp_loopback_guest_to_host_ports
                    .remove(&(SocketFamily::Ipv4, 41096));
                collect(&mut paths);
                paths
                    .udp_loopback_guest_to_host_ports
                    .insert((SocketFamily::Ipv4, 41096), 43096);
                assert_eq!(
                    paths.translate_udp_loopback_port(SocketFamily::Ipv4, 41096),
                    Some(43096)
                );
            }
        }
        // Positive integration must use fresh output of the real collector.
        let mut paths = mixed_udp_paths();
        crate::execution::process::collect_socket_port_state(
            &kernel,
            "root",
            &process,
            &mut paths.tcp_loopback_guest_to_host_ports,
            &mut paths.http_loopback_targets,
            &mut paths.http2_loopback_targets,
            &mut paths.udp_loopback_guest_to_host_ports,
            &mut paths.udp_loopback_host_to_guest_ports,
            &mut paths.used_tcp_guest_ports,
            &mut paths.used_udp_guest_ports,
        );
        let bridge = SharedBridge {
            inner: Arc::new(Mutex::new(LocalVmHost::default())),
            permissions: Arc::new(Mutex::new(BTreeMap::from([(
                "mixed-udp".into(),
                crate::core::permissions::resolve_profile_permissions_policy(
                    agentos_vm_config::VmDefaultsProfile::AgentOs,
                    Some(serde_json::from_value(json!({"network":"deny"})).unwrap()),
                ),
            )]))),
            set_vm_permissions_history: Default::default(),
            set_vm_permissions_outcomes: Default::default(),
            emit_lifecycle_outcomes: Default::default(),
        };
        let mut sender = ActiveUdpSocket::new(
            &mut kernel,
            process.kernel_pid,
            UdpFamily::Ipv4,
            Arc::clone(runtime.resources()),
            runtime.clone(),
            reactor_io_limits(&crate::limits::VmLimits::default()),
        )
        .unwrap();
        let dns = VmDnsConfig::default();
        let send = |sender: &mut ActiveUdpSocket,
                    kernel: &mut SidecarKernel,
                    context: &SocketPathContext| {
            sender.send_to(ActiveUdpSendToRequest {
                bridge: &bridge,
                kernel,
                kernel_pid: process.kernel_pid,
                vm_id: "mixed-udp",
                dns: &dns,
                host: "127.0.0.2",
                port: 41096,
                context,
                contents: &[0, 255, 128],
            })
        };
        let result = send(&mut sender, &mut kernel, &paths);
        if wildcard {
            assert!(matches!(
                result.unwrap(),
                ActiveUdpSendResult::Immediate { written: 3, .. }
            ));
            let received = kernel
                .socket_recv_datagram(EXECUTION_DRIVER_NAME, process.kernel_pid, receiver_id, 3)
                .unwrap()
                .unwrap();
            assert_eq!(received.payload(), &[0, 255, 128]);
        } else {
            assert_eq!(
                host_service_error_code(&result.err().unwrap()),
                "EACCES",
                "specific IPv6 binding does not own IPv4"
            );
        }
        assert_eq!(
            host_service_error_code(
                &send(&mut sender, &mut kernel, &mixed_udp_paths())
                    .err()
                    .unwrap()
            ),
            "EACCES",
            "empty inventory grants nothing (not a full cross-VM test)"
        );
        process
            .udp_sockets
            .get_mut("receiver")
            .unwrap()
            .kernel_socket_id = None;
        let mut no_kernel = mixed_udp_paths();
        crate::execution::process::collect_socket_port_state(
            &kernel,
            "root",
            &process,
            &mut no_kernel.tcp_loopback_guest_to_host_ports,
            &mut no_kernel.http_loopback_targets,
            &mut no_kernel.http2_loopback_targets,
            &mut no_kernel.udp_loopback_guest_to_host_ports,
            &mut no_kernel.udp_loopback_host_to_guest_ports,
            &mut no_kernel.used_tcp_guest_ports,
            &mut no_kernel.used_udp_guest_ports,
        );
        assert!(
            !no_kernel.udp_loopback_port_allowed(SocketFamily::Ipv4, 41096),
            "native-only wildcard grants no cross-family host route"
        );
    }
}
