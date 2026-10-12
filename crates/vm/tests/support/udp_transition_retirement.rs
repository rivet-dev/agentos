//! Actual managed service/transfer/retirement with a gated native completion.
//! No guest executor or process is launched by this fixture.
use super::*;
use crate::executor::backend::{DirectHostReplyTarget, HostCallIdentity};
use crate::protocol::{RequestFrame, RequestPayload, ResponsePayload};
use crate::state::{ConnectionState, SessionState};
use agentos_vm_host_interface::LocalVmHost;

#[derive(Default)]
struct ReplyTarget {
    result: Mutex<Option<Result<HostCallReply, HostServiceError>>>,
}
impl DirectHostReplyTarget for ReplyTarget {
    fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
        Ok(true)
    }
    fn respond(
        &self,
        _: u64,
        _: bool,
        result: Result<HostCallReply, HostServiceError>,
    ) -> Result<(), HostServiceError> {
        *self.result.lock().unwrap() = Some(result);
        Ok(())
    }
}
fn reply(vm: &VmState, process: &str, call_id: u64) -> (DirectHostReplyHandle, Arc<ReplyTarget>) {
    let target = Arc::new(ReplyTarget::default());
    let reply = DirectHostReplyHandle::new(
        HostCallIdentity {
            generation: vm.generation,
            pid: vm.active_processes[process].kernel_pid,
            call_id,
        },
        target.clone(),
        4096,
    )
    .unwrap();
    reply.claim().unwrap();
    (reply, target)
}
fn dispatch(
    sidecar: &mut VmManager<LocalVmHost>,
    vm_id: &str,
    process: &str,
    call_id: u64,
    operation: NetworkOperation,
) -> Result<HostServiceResponse, VmError> {
    let mut vm = sidecar.vms.get_mut(vm_id).unwrap();
    let paths = build_socket_path_context(&vm)?;
    let vm = &mut *vm;
    let runtime = vm.runtime_context.clone();
    let _enter = runtime.tokio_handle().enter();
    service_managed_fd_network_operation(
        ManagedFdNetworkServiceContext {
            bridge: &sidecar.bridge,
            vm_id,
            dns: &vm.dns,
            socket_paths: &paths,
            kernel: &mut vm.kernel,
            kernel_readiness: vm.kernel_socket_readiness.clone(),
            process: vm.active_processes.get_mut(process).unwrap(),
            capabilities: vm.capabilities.clone(),
            managed_descriptions: vm.managed_host_net_descriptions.clone(),
            call_id,
        },
        operation,
    )
}
async fn settle(
    sidecar: &mut VmManager<LocalVmHost>,
    vm_id: &str,
    process: &str,
    call_id: u64,
    response: HostServiceResponse,
) -> Value {
    match response {
        HostServiceResponse::Json(value) => value,
        HostServiceResponse::Deferred { receiver, .. } => {
            let result = tokio::time::timeout(Duration::from_secs(2), receiver)
                .await
                .unwrap()
                .unwrap();
            let (reply, target) = reply(&sidecar.vms.get(vm_id).unwrap(), process, call_id);
            sidecar
                .handle_host_call_completion(
                    vm_id,
                    process,
                    crate::state::HostCallCompletion { reply, result },
                )
                .unwrap();
            let result = target.result.lock().unwrap().take().unwrap().unwrap();
            match result {
                HostCallReply::Json(value) => value,
                _ => panic!("JSON completion required"),
            }
        }
        _ => panic!("unexpected service response"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn udp_pending_connect_initiator_retirement_allows_alias_recovery() {
    tokio::task::LocalSet::new().run_until(async {
        let config = crate::VmManagerConfig::default();
        let driver = agentos_driver_tokio::TokioDriver::process(&config.runtime).unwrap();
        let mut sidecar = VmManager::with_config_extensions_driver_and_executors(
            LocalVmHost::default(), config, Vec::new(), driver.handle(),
            crate::ExecutorRegistry::empty().with(crate::ExecutorKind::NodeV8),
        ).unwrap();
        sidecar.connections.insert("udp-owner-test".into(), ConnectionState { auth_token: String::new(), sessions: BTreeSet::from(["session".into()]) });
        sidecar.sessions.insert("session".into(), SessionState {
            connection_id: "udp-owner-test".into(),
            placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(crate::protocol::SidecarPlacementShared { pool: None }),
            metadata: BTreeMap::new(), vm_ids: BTreeSet::new(),
        });
        let payload = crate::protocol::CreateVmRequest::legacy_test_config(
            crate::protocol::GuestRuntimeKind::JavaScript, Default::default(), Default::default(),
            Some(crate::wire::PermissionsPolicy::allow_all()),
        );
        let request = RequestFrame::new(1, OwnershipScope::session("udp-owner-test", "session"), RequestPayload::CreateVm(payload.clone()));
        let created = sidecar.create_vm(&request, payload).await.unwrap();
        let ResponsePayload::VmCreated(created) = created.response.payload else { panic!("VM creation") };
        let vm_id = created.vm_id;
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(250))).unwrap();
        let peer_port = peer.local_addr().unwrap().port();
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            vm.create_loopback_exempt_ports.insert(peer_port);
            for name in ["initiator", "survivor", "unrelated"] {
                let handle = vm.kernel.create_virtual_process(EXECUTION_DRIVER_NAME, EXECUTION_DRIVER_NAME,
                    JAVASCRIPT_COMMAND, vec![JAVASCRIPT_COMMAND.into()], Default::default()).unwrap();
                let process = ActiveProcess::new(handle.pid(), handle, vm.runtime_context.clone(), vm.limits.clone(),
                    agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS, GuestRuntimeKind::JavaScript,
                    ActiveExecution::HostFunction(Default::default()));
                vm.active_processes.insert(name.into(), process);
            }
        }
        let response = dispatch(&mut sidecar, &vm_id, "initiator", 1, NetworkOperation::Socket {
            domain: HostSocketDomain::Inet4, kind: SocketKind::Datagram, nonblocking: false, close_on_exec: false,
        }).unwrap();
        let fd = settle(&mut sidecar, &vm_id, "initiator", 1, response).await["fd"].as_u64().unwrap() as u32;
        let description_id;
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            let vm = &mut *vm;
            let process = vm.active_processes.get_mut("initiator").unwrap();
            description_id = vm.kernel.fd_description_identity(EXECUTION_DRIVER_NAME, process.kernel_pid, fd).unwrap().0;
            let description = vm.managed_host_net_descriptions.lock().unwrap()[&description_id].clone();
            let resource = crate::execution::child_process::prepare_managed_transferred_host_net_resource(
                &mut vm.kernel, process, description_id, fd, &description, "pending UDP owner transfer").unwrap();
            let transferred = vm.kernel.fd_transfer(EXECUTION_DRIVER_NAME, process.kernel_pid, fd).unwrap();
            let survivor = vm.active_processes.get_mut("survivor").unwrap();
            vm.kernel.fd_install_transfer_at(EXECUTION_DRIVER_NAME, survivor.kernel_pid, fd, 0, &transferred).unwrap();
            let crate::execution::child_process::TransferredHostNetSocket::Udp { socket, .. } = resource else { panic!("UDP transfer") };
            survivor.udp_sockets.insert("transferred".into(), socket);
            vm.managed_host_net_descriptions.lock().unwrap().get_mut(&description_id).unwrap().routes.insert(
                survivor.kernel_pid, ManagedHostNetRoute::UdpSocket("transferred".into()));
        }
        let limit = PayloadLimit::new("test.udp.retirement", 256).unwrap();
        let address = || SocketAddress::Inet { host: BoundedString::try_new("127.0.0.1".into(), &limit).unwrap(), port: peer_port };
        // A second process has its own pending transition. Retiring the first
        // initiator must not authorize, cancel, or complete this unrelated one.
        let response = dispatch(&mut sidecar, &vm_id, "unrelated", 10, NetworkOperation::Socket {
            domain: HostSocketDomain::Inet4, kind: SocketKind::Datagram, nonblocking: false, close_on_exec: false,
        }).unwrap();
        let other_fd = settle(&mut sidecar, &vm_id, "unrelated", 10, response).await["fd"].as_u64().unwrap() as u32;
        let response = dispatch(&mut sidecar, &vm_id, "unrelated", 11, NetworkOperation::Connect {
            fd: other_fd, address: address(), deadline_ms: Some(1000),
        }).unwrap();
        let HostServiceResponse::Deferred { receiver, .. } = response else { panic!("unrelated native transition must defer") };
        let other_result = tokio::time::timeout(Duration::from_secs(2), receiver).await.unwrap().unwrap();
        assert!(other_result.is_ok());
        let (other_reply, other_target) = reply(&sidecar.vms.get(&vm_id).unwrap(), "unrelated", 11);
        let response = dispatch(&mut sidecar, &vm_id, "initiator", 2, NetworkOperation::Connect { fd, address: address(), deadline_ms: Some(1000) }).unwrap();
        let HostServiceResponse::Deferred { receiver, .. } = response else { panic!("actual native completion must defer") };
        let native_result = tokio::time::timeout(Duration::from_secs(2), receiver).await.unwrap().unwrap();
        assert!(native_result.is_ok());
        let (late_reply, late_target) = reply(&sidecar.vms.get(&vm_id).unwrap(), "initiator", 2);
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            VmManager::finish_active_process_exit_in_vm(&sidecar.bridge, &mut vm, &vm_id, "initiator", 0).unwrap().unwrap();
            assert!(!vm.active_processes.contains_key("initiator"));
            assert!(vm.active_processes.contains_key("survivor"));
            assert!(vm.managed_host_net_descriptions.lock().unwrap().contains_key(&description_id));
        }
        let other_error = dispatch(&mut sidecar, &vm_id, "unrelated", 12, NetworkOperation::Connect {
            fd: other_fd, address: SocketAddress::Unspecified, deadline_ms: Some(1000),
        }).err().expect("another process transition must remain pending");
        assert_eq!(host_service_error_code(&other_error), "EALREADY");
        sidecar.handle_host_call_completion(&vm_id, "unrelated", crate::state::HostCallCompletion {
            reply: other_reply, result: other_result,
        }).unwrap();
        assert!(other_target.result.lock().unwrap().as_ref().unwrap().is_ok());
        let error = dispatch(&mut sidecar, &vm_id, "survivor", 3, NetworkOperation::Send {
            fd, bytes: BoundedBytes::try_new(vec![99], &limit).unwrap(), flags: 0, address: None, deadline_ms: None,
        }).err().expect("uncommitted peer must remain fenced");
        eprintln!("survivor send after initiator retirement: {}", host_service_error_code(&error));
        assert_eq!(host_service_error_code(&error), "ECANCELED");
        let recovery = dispatch(&mut sidecar, &vm_id, "survivor", 4, NetworkOperation::Connect {
            fd, address: SocketAddress::Unspecified, deadline_ms: Some(1000),
        });
        assert!(recovery.is_ok(), "surviving alias must be able to recover after its pending transition owner exits: {:?}", recovery.err());
        settle(&mut sidecar, &vm_id, "survivor", 4, recovery.unwrap()).await;
        let response = dispatch(&mut sidecar, &vm_id, "survivor", 5, NetworkOperation::Connect { fd, address: address(), deadline_ms: Some(1000) }).unwrap();
        settle(&mut sidecar, &vm_id, "survivor", 5, response).await;
        // Deliver the retired owner's completion only after the survivor has
        // committed a later transition. It cannot overwrite that newer state.
        sidecar.handle_host_call_completion(&vm_id, "initiator", crate::state::HostCallCompletion { reply: late_reply, result: native_result }).unwrap();
        assert_eq!(late_target.result.lock().unwrap().as_ref().unwrap().as_ref().unwrap_err().code, "ESTALE");
        let response = dispatch(&mut sidecar, &vm_id, "survivor", 6, NetworkOperation::Send {
            fd, bytes: BoundedBytes::try_new(vec![42], &limit).unwrap(), flags: 0, address: None, deadline_ms: None,
        }).unwrap();
        settle(&mut sidecar, &vm_id, "survivor", 6, response).await;
        let mut received = [0u8; 2];
        let (count, _) = peer.recv_from(&mut received).unwrap(); assert_eq!(&received[..count], &[42]);
        let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
        VmManager::finish_active_process_exit_in_vm(&sidecar.bridge, &mut vm, &vm_id, "survivor", 0).unwrap().unwrap();
        VmManager::finish_active_process_exit_in_vm(&sidecar.bridge, &mut vm, &vm_id, "unrelated", 0).unwrap().unwrap();
    }).await;
}
