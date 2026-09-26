//! Opaque fixtures for sidecar transport tests. No mutable VM state escapes.
use crate::executor::backend::{
    DirectHostReplyHandle, DirectHostReplyTarget, HostCallIdentity, HostCallReply, HostServiceError,
};
use crate::state::*;
use crate::VmManager;
use agentos_vm_host_interface::LocalVmHost;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

pub fn register_session(manager: &mut VmManager<LocalVmHost>, connection: &str, session: &str) {
    manager.connections.insert(
        connection.into(),
        ConnectionState {
            auth_token: String::new(),
            sessions: BTreeSet::from([session.into()]),
        },
    );
    manager.sessions.insert(
        session.into(),
        SessionState {
            connection_id: connection.into(),
            placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                crate::protocol::SidecarPlacementShared { pool: None },
            ),
            metadata: BTreeMap::new(),
            vm_ids: BTreeSet::new(),
        },
    );
}

pub fn contains_vm(manager: &VmManager<LocalVmHost>, vm_id: &str) -> bool {
    manager.vms.contains_key(vm_id)
}

#[derive(Clone)]
pub struct ProcessEvents(HostFunctionExecution, u64, u32);
impl ProcessEvents {
    fn enqueue(&self, event: ActiveExecutionEvent) {
        assert!(crate::execution::send_host_function_process_event(
            &self.0.cancelled,
            &self.0.pending_events,
            &self.0.event_overflow_reason,
            &self.0.pending_event_bytes,
            &self.0.pending_event_count_limit,
            &self.0.pending_event_bytes_limit,
            &self.0.vm_pending_event_bytes_budget,
            event,
        ));
        self.0.event_notify.notify_one();
    }
    pub fn stdout(&self, bytes: &[u8]) {
        use crate::executor::backend::{ExecutionEvent, OutputStream, PayloadLimit};
        self.enqueue(ActiveExecutionEvent::Common(
            ExecutionEvent::output(
                OutputStream::Stdout,
                bytes.to_vec(),
                &PayloadLimit::new("fixtureOutputBytes", 4096).expect("output limit"),
            )
            .expect("bounded output"),
        ));
    }
    pub fn runtime_fault(&self) {
        let limit = crate::executor::backend::PayloadLimit::new("fixtureFaultBytes", 4096)
            .expect("fault limit");
        let event = crate::executor::backend::ExecutionEvent::runtime_fault(
            HostServiceError::new("ERR_FIXTURE_FAULT", "fixture runtime fault"),
            &limit,
        )
        .expect("bounded fault");
        self.enqueue(ActiveExecutionEvent::Common(event));
    }
    pub fn exit(&self, code: i32) {
        self.enqueue(ActiveExecutionEvent::Common(
            crate::executor::backend::ExecutionEvent::Exited(
                crate::executor::backend::ExecutionExit::Exited(code),
            ),
        ));
    }
    pub fn signal_exit(&self, signal: i32) {
        self.enqueue(ActiveExecutionEvent::Common(
            crate::executor::backend::ExecutionEvent::Exited(
                crate::executor::backend::ExecutionExit::Signaled {
                    signal,
                    core_dumped: false,
                },
            ),
        ));
    }
    pub fn is_empty(&self) -> bool {
        self.0
            .pending_events
            .lock()
            .expect("fixture event queue")
            .is_empty()
    }
    pub fn stale_completion(&self) -> oneshot::Receiver<Result<HostCallReply, HostServiceError>> {
        let (tx, rx) = oneshot::channel();
        let reply = DirectHostReplyHandle::new(
            HostCallIdentity {
                generation: self.1.wrapping_add(1),
                pid: self.2,
                call_id: 777,
            },
            Arc::new(Reply(Mutex::new(Some(tx)))),
            4096,
        )
        .expect("fixture reply");
        self.enqueue(ActiveExecutionEvent::HostCallCompletion(
            HostCallCompletion {
                reply,
                result: Ok(serde_json::Value::Null),
            },
        ));
        rx
    }
    pub fn rpc(
        &self,
        id: u64,
        method: &str,
        args: Vec<serde_json::Value>,
    ) -> oneshot::Receiver<Result<HostCallReply, HostServiceError>> {
        let (tx, rx) = oneshot::channel();
        let reply = DirectHostReplyHandle::new(
            HostCallIdentity {
                generation: self.1,
                pid: self.2,
                call_id: id,
            },
            Arc::new(Reply(Mutex::new(Some(tx)))),
            4096,
        )
        .expect("fixture reply");
        self.enqueue(ActiveExecutionEvent::HostRpcRequest(ExecutionHostCall {
            request: crate::executor::HostRpcRequest {
                id,
                method: method.into(),
                args,
                raw_bytes_args: Default::default(),
            },
            reply,
        }));
        rx
    }
}
struct Reply(Mutex<Option<oneshot::Sender<Result<HostCallReply, HostServiceError>>>>);
impl DirectHostReplyTarget for Reply {
    fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
        Ok(true)
    }
    fn respond(
        &self,
        _: u64,
        _: bool,
        result: Result<HostCallReply, HostServiceError>,
    ) -> Result<(), HostServiceError> {
        if let Some(sender) = self.0.lock().expect("fixture reply mutex").take() {
            // Tests may intentionally discard the waiter while checking queue progress.
            if sender.send(result).is_err() {
                tracing::debug!("fixture waiter dropped");
            }
        }
        Ok(())
    }
}
fn process(
    vm: &mut VmState,
    parent: Option<u32>,
    notify: Arc<tokio::sync::Notify>,
) -> (ActiveProcess, ProcessEvents) {
    let handle = vm
        .kernel
        .create_virtual_process(
            EXECUTION_DRIVER_NAME,
            EXECUTION_DRIVER_NAME,
            JAVASCRIPT_COMMAND,
            vec![JAVASCRIPT_COMMAND.into()],
            agentos_vm_kernel::kernel::VirtualProcessOptions {
                parent_pid: parent,
                env: vm.guest_env.clone(),
                ..Default::default()
            },
        )
        .expect("fixture kernel process");
    let execution = HostFunctionExecution::with_event_notify(
        notify.clone(),
        agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
    );
    let producer = ProcessEvents(execution.clone(), vm.generation, handle.pid());
    let control = ActiveProcess::attach_runtime_control_before_start(&handle, notify.clone())
        .expect("fixture runtime control");
    let process = ActiveProcess::new_with_attached_runtime_control(
        handle.pid(),
        handle,
        vm.runtime_context.clone(),
        vm.limits.clone(),
        agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
        crate::protocol::GuestRuntimeKind::JavaScript,
        ActiveExecution::HostFunction(execution),
        control,
        notify,
    );
    (process, producer)
}
pub fn insert_process(
    manager: &mut VmManager<LocalVmHost>,
    vm_id: &str,
    id: &str,
    detached: bool,
) -> ProcessEvents {
    let notify = manager.process_event_notify();
    let mut vm = manager.vms.get_mut(vm_id).expect("fixture VM");
    let (process, producer) = process(&mut vm, None, notify);
    vm.active_processes.insert(id.into(), process);
    if detached {
        vm.detached_child_processes.insert(id.into());
    }
    producer
}
pub fn insert_child(
    manager: &mut VmManager<LocalVmHost>,
    vm_id: &str,
    root_id: &str,
    id: &str,
) -> ProcessEvents {
    let notify = manager.process_event_notify();
    let mut vm = manager.vms.get_mut(vm_id).expect("fixture VM");
    let parent = vm.active_processes[root_id].kernel_pid;
    let (process, producer) = process(&mut vm, Some(parent), notify);
    vm.active_processes
        .get_mut(root_id)
        .expect("fixture root")
        .child_processes
        .insert(id.into(), process);
    producer
}
pub fn capture_child(
    manager: &mut VmManager<LocalVmHost>,
    vm_id: &str,
    root_id: &str,
    child_id: &str,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static {
    let mut vm = manager.vms.get_mut(vm_id).expect("fixture VM");
    let count = VmPendingBudgetReservation::try_new(vm.pending_child_sync_count_budget.clone(), 1)
        .expect("fixture count budget");
    let bytes =
        VmPendingBudgetReservation::try_new(vm.pending_child_sync_bytes_budget.clone(), 2050)
            .expect("fixture byte budget");
    let root = vm.active_processes.get_mut(root_id).expect("fixture root");
    let pid = root.child_processes[child_id].kernel_pid;
    let (tx, rx) = oneshot::channel();
    root.pending_child_process_sync.insert(
        child_id.into(),
        PendingChildProcessSync {
            pid,
            stdout: Vec::new(),
            stderr: Vec::new(),
            max_buffer: 1024,
            deadline: None,
            timeout_signal: "SIGTERM".into(),
            kill_sent: false,
            timed_out: false,
            max_buffer_exceeded: false,
            completion: PendingChildProcessSyncCompletion::Javascript(tx),
            _count_reservation: count,
            _bytes_reservation: bytes,
        },
    );
    async move {
        rx.await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("{e:?}"))
    }
}
