use super::*;
use agentos_vm_host_interface::LocalVmHost;
use agentos_vm_kernel::{
    command_registry::CommandDriver, kernel::KernelVmConfig, mount_table::MountTable,
    permissions::Permissions, vfs::MemoryFileSystem,
};
use std::sync::{Arc, Mutex};

fn immediate(response: Result<HostServiceResponse, VmError>) -> Value {
    match response.unwrap() {
        HostServiceResponse::Json(value) => value,
        _ => panic!("unbound option operations must not activate native I/O"),
    }
}
fn error(response: Result<HostServiceResponse, VmError>) -> String {
    host_service_error_code(&response.err().expect("operation must reject"))
}

#[test]
fn managed_traffic_class_domains_types_and_failure_atomicity() {
    let driver = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
    let runtime = driver
        .handle()
        .scoped_for_vm(Arc::clone(driver.handle().resources()), 91031);
    let mut config = KernelVmConfig::new("traffic-class-test");
    config.permissions = Permissions::allow_all();
    let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
        .unwrap();
    let handle = kernel
        .create_virtual_process(
            EXECUTION_DRIVER_NAME,
            EXECUTION_DRIVER_NAME,
            WASM_COMMAND,
            vec![WASM_COMMAND.into()],
            Default::default(),
        )
        .unwrap();
    let mut process = ActiveProcess::new(
        handle.pid(),
        handle,
        runtime.clone(),
        Default::default(),
        agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
        GuestRuntimeKind::JavaScript,
        ActiveExecution::HostFunction(Default::default()),
    );
    let child_handle = kernel
        .create_virtual_process(
            EXECUTION_DRIVER_NAME,
            EXECUTION_DRIVER_NAME,
            WASM_COMMAND,
            vec![WASM_COMMAND.into()],
            Default::default(),
        )
        .unwrap();
    let mut child = ActiveProcess::new(
        child_handle.pid(),
        child_handle,
        runtime.clone(),
        Default::default(),
        agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
        GuestRuntimeKind::JavaScript,
        ActiveExecution::HostFunction(Default::default()),
    );
    let use_child = std::cell::Cell::new(false);
    let bridge = SharedBridge {
        inner: Arc::new(Mutex::new(LocalVmHost::default())),
        permissions: Arc::new(Mutex::new(BTreeMap::from([(
            "traffic-class-test".into(),
            serde_json::from_value(json!({"network":"deny"})).unwrap(),
        )]))),
        set_vm_permissions_history: Default::default(),
        set_vm_permissions_outcomes: Default::default(),
        emit_lifecycle_outcomes: Default::default(),
    };
    let paths = SocketPathContext {
        sandbox_root: Default::default(),
        unix_abstract_namespace: [0; 32],
        unix_socket_host_dir: Default::default(),
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
    };
    let dns = VmDnsConfig::default();
    let descriptions = Default::default();
    let readiness = Default::default();
    let capabilities = CapabilityRegistry::new(91031, Arc::clone(runtime.resources()));
    let mut call = |operation| {
        let _entered = runtime.tokio_handle().enter();
        service_managed_fd_network_operation(
            ManagedFdNetworkServiceContext {
                bridge: &bridge,
                vm_id: "traffic-class-test",
                dns: &dns,
                socket_paths: &paths,
                kernel: &mut kernel,
                kernel_readiness: Arc::clone(&readiness),
                process: if use_child.get() {
                    &mut child
                } else {
                    &mut process
                },
                capabilities: capabilities.clone(),
                managed_descriptions: Arc::clone(&descriptions),
                call_id: 1,
            },
            operation,
        )
    };
    for (domain, kind) in [
        (HostSocketDomain::Inet4, SocketKind::Stream),
        (HostSocketDomain::Inet6, SocketKind::Stream),
        (HostSocketDomain::Inet6, SocketKind::Datagram),
        (HostSocketDomain::Unix, SocketKind::Stream),
    ] {
        let fd = immediate(call(NetworkOperation::Socket {
            domain,
            kind,
            nonblocking: false,
            close_on_exec: false,
        }))["fd"]
            .as_u64()
            .unwrap() as u32;
        use_child.set(true);
        assert_eq!(
            error(call(NetworkOperation::GetOption {
                fd,
                name: SocketOptionName::Domain
            })),
            "EBADF"
        );
        assert_eq!(
            error(call(NetworkOperation::SetOption {
                fd,
                name: SocketOptionName::IpTypeOfService,
                value: SocketOptionValue::Integer(200)
            })),
            "EBADF"
        );
        use_child.set(false);
        let get = |name| NetworkOperation::GetOption { fd, name };
        let set = |name, value| NetworkOperation::SetOption { fd, name, value };
        assert_eq!(
            immediate(call(get(SocketOptionName::Domain))),
            json!(match domain {
                HostSocketDomain::Inet4 => 2,
                HostSocketDomain::Inet6 => 10,
                HostSocketDomain::Unix => 1,
            })
        );
        if domain == HostSocketDomain::Unix {
            assert_eq!(
                error(call(get(SocketOptionName::IpTypeOfService))),
                "EOPNOTSUPP"
            );
            assert_eq!(
                error(call(set(
                    SocketOptionName::IpTypeOfService,
                    SocketOptionValue::Integer(42)
                ))),
                "EOPNOTSUPP"
            );
        } else {
            immediate(call(set(
                SocketOptionName::IpTypeOfService,
                SocketOptionValue::Integer(42),
            )));
            let want = if kind == SocketKind::Stream { 40 } else { 42 };
            assert_eq!(
                immediate(call(get(SocketOptionName::IpTypeOfService))),
                json!(want)
            );
            assert_eq!(
                error(call(set(
                    SocketOptionName::IpTypeOfService,
                    SocketOptionValue::Bool(true)
                ))),
                "EINVAL"
            );
            assert_eq!(
                immediate(call(get(SocketOptionName::IpTypeOfService))),
                json!(want)
            );
            if domain == HostSocketDomain::Inet4 {
                assert_eq!(
                    error(call(set(
                        SocketOptionName::Ipv6TrafficClass,
                        SocketOptionValue::Integer(42)
                    ))),
                    "ENOPROTOOPT"
                );
                assert_eq!(
                    error(call(get(SocketOptionName::Ipv6TrafficClass))),
                    "EOPNOTSUPP"
                );
            } else {
                assert_eq!(
                    immediate(call(get(SocketOptionName::Ipv6TrafficClass))),
                    json!(0),
                    "IPv4 TOS must not change IPv6 traffic class"
                );
                immediate(call(set(
                    SocketOptionName::Ipv6TrafficClass,
                    SocketOptionValue::Integer(255),
                )));
                assert_eq!(
                    immediate(call(get(SocketOptionName::Ipv6TrafficClass))),
                    json!(if kind == SocketKind::Stream { 252 } else { 255 })
                );
                for invalid in [-2, 256] {
                    assert_eq!(
                        error(call(set(
                            SocketOptionName::Ipv6TrafficClass,
                            SocketOptionValue::Integer(invalid)
                        ))),
                        "EINVAL"
                    );
                }
                assert_eq!(
                    immediate(call(get(SocketOptionName::Ipv6TrafficClass))),
                    json!(if kind == SocketKind::Stream { 252 } else { 255 })
                );
                immediate(call(set(
                    SocketOptionName::Ipv6TrafficClass,
                    SocketOptionValue::Integer(-1),
                )));
                assert_eq!(
                    immediate(call(get(SocketOptionName::Ipv6TrafficClass))),
                    json!(0)
                );
                assert_eq!(
                    immediate(call(get(SocketOptionName::IpTypeOfService))),
                    json!(want)
                );
            }
        }
    }
    kernel.dispose().unwrap();
}
