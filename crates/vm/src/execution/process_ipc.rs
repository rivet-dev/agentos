//! Kernel descriptor transport for Node IPC, independent of fd0/1/2.
use super::*;
use crate::executor::host::{ProcessIpcInterest, ProcessIpcOperation, ProcessIpcTarget};
use crate::state::{VmHandle, WeakVmHandle};
use agentos_vm_kernel::fd_table::{FD_CLOEXEC, F_SETFD};
use agentos_vm_kernel::poll::{PollEvents, PollFd, POLLIN, POLLNVAL, POLLOUT};
use agentos_vm_kernel::resource_accounting::DEFAULT_MAX_OPEN_FDS;
use agentos_vm_kernel::socket_table::SocketType;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, Clone)]
struct Endpoint {
    fd: u32,
    description: u64,
    generation: u64,
    serialization: String,
}

struct IpcWait {
    reply: DirectHostReplyHandle,
    id: u64,
    revision: Cell<u64>,
    interests: RefCell<Vec<ProcessIpcInterest>>,
    closed: Cell<bool>,
    changed: Arc<tokio::sync::Notify>,
    wake: IpcWake,
    task: RefCell<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for IpcWait {
    fn drop(&mut self) {
        self.closed.set(true);
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
        if !self.reply.is_terminal() {
            if let Err(error) = self
                .reply
                .fail(HostServiceError::new("EINTR", "IPC waiter retired"))
            {
                tracing::error!(%error,"ERR_AGENTOS_IPC_REPLY_RETIREMENT");
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct ProcessIpcState {
    parent: Option<Endpoint>,
    children: BTreeMap<String, Endpoint>,
    wait: Option<Rc<IpcWait>>,
    metadata_warned: bool,
}

impl Drop for ProcessIpcState {
    fn drop(&mut self) {
        self.cancel_wait();
    }
}

impl ProcessIpcState {
    fn metadata_count(&self) -> usize {
        self.children
            .len()
            .saturating_add(usize::from(self.parent.is_some()))
    }

    fn observe_metadata(&mut self, limit: usize, pid: u32) {
        let observed = self.metadata_count();
        // ceil(80%): avoid overflow even for a caller-supplied usize bound.
        let near_limit = limit > 0 && observed >= limit - limit / 5;
        if near_limit && !self.metadata_warned {
            tracing::warn!(
                pid,
                observed,
                limit,
                resource = "ipcEndpointMetadata",
                config_path = "limits.resources.maxOpenFds",
                "IPC retained endpoint metadata is nearing limits.resources.maxOpenFds; close unused IPC channels or raise limits.resources.maxOpenFds"
            );
        }
        self.metadata_warned = near_limit;
    }

    pub(super) fn cancel_wait(&mut self) {
        if let Some(wait) = self.wait.take() {
            wait.closed.set(true);
            if let Some(task) = wait.task.borrow_mut().take() {
                task.abort();
            }
            if !wait.reply.is_terminal() {
                if let Err(error) = wait
                    .reply
                    .fail(HostServiceError::new("EINTR", "IPC owner retired"))
                {
                    tracing::error!(%error, "ERR_AGENTOS_IPC_REPLY_RETIREMENT");
                }
            }
            wait.changed.notify_one();
        }
    }
}

fn metadata_limit(kernel: &SidecarKernel) -> usize {
    kernel
        .resource_limits()
        .max_open_fds
        .unwrap_or(DEFAULT_MAX_OPEN_FDS)
}

fn metadata_admission_error(pid: u32, count: usize, limit: usize) -> VmError {
    let message = format!(
        "IPC retained endpoint metadata for PID {pid} would exceed limits.resources.maxOpenFds ({limit}) with {} records; close unused IPC channels or raise limits.resources.maxOpenFds",
        count.saturating_add(1)
    );
    let details = json!({
        "resource": "ipcEndpointMetadata",
        "ownerPid": pid,
        "limitName": "limits.resources.maxOpenFds",
        "configPath": "limits.resources.maxOpenFds",
        "limit": limit,
        "observed": count.saturating_add(1),
    });
    VmError::Host(HostServiceError::new("EMFILE", message).with_details(details))
}

fn endpoint<'a>(state: &'a ProcessIpcState, child: Option<&str>) -> Option<&'a Endpoint> {
    match child {
        Some(id) => state.children.get(id),
        None => state.parent.as_ref(),
    }
}

fn recorded_endpoint<'a>(
    process: &'a ActiveProcess,
    target: &ProcessIpcTarget,
) -> Result<&'a Endpoint, VmError> {
    endpoint(&process.ipc, target.child_id.as_ref().map(|id| id.as_str()))
        .filter(|endpoint| endpoint.generation == target.generation)
        .ok_or_else(|| VmError::host("ESTALE", "IPC channel generation is no longer owned"))
}

fn checked_endpoint<'a>(
    kernel: &SidecarKernel,
    process: &'a ActiveProcess,
    target: &ProcessIpcTarget,
) -> Result<&'a Endpoint, VmError> {
    let endpoint = recorded_endpoint(process, target)?;
    let identity = kernel
        .fd_description_identity(EXECUTION_DRIVER_NAME, process.kernel_pid, endpoint.fd)
        .map_err(kernel_error)?
        .0;
    if identity != endpoint.description {
        return Err(VmError::host(
            "ESTALE",
            "IPC descriptor was closed or replaced",
        ));
    }
    Ok(endpoint)
}

fn close_matching(
    kernel: &mut SidecarKernel,
    pid: u32,
    endpoint: &Endpoint,
) -> Result<(), VmError> {
    match kernel.fd_description_identity(EXECUTION_DRIVER_NAME, pid, endpoint.fd) {
        Ok((identity, _)) if identity == endpoint.description => kernel
            .fd_close(EXECUTION_DRIVER_NAME, pid, endpoint.fd)
            .map_err(kernel_error),
        Ok(_) => Ok(()),
        Err(error) if matches!(error.code(), "EBADF" | "ESRCH") => Ok(()),
        Err(error) => Err(kernel_error(error)),
    }
}

/// Captured outside the spawn async body. Errors drop its VM borrows before
/// rollback; cancellation occurs only at an await with no VM borrow retained.
pub(super) struct PendingProcessIpc {
    vm: Option<WeakVmHandle>,
    endpoints: Vec<(u32, Endpoint)>,
    metadata_limit: usize,
}

type IpcStartupFuture = Pin<Box<dyn Future<Output = Result<Value, VmError>>>>;

/// Drop the startup future (and all its VM borrows) before rolling descriptors
/// back. This order also applies when an admitted startup is cancelled.
struct IpcStartup {
    future: Option<IpcStartupFuture>,
    guard: Rc<RefCell<PendingProcessIpc>>,
}

impl Future for IpcStartup {
    type Output = Result<Value, VmError>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let state = self.get_mut();
        let result = std::task::ready!(state
            .future
            .as_mut()
            .expect("IPC startup future")
            .as_mut()
            .poll(cx));
        state.future.take();
        if let Err(cleanup) = state.guard.borrow_mut().rollback() {
            let original = result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default();
            return std::task::Poll::Ready(Err(VmError::host(
                "ERR_AGENTOS_IPC_ROLLBACK",
                format!("{original}; {cleanup}"),
            )));
        }
        std::task::Poll::Ready(result)
    }
}

impl Drop for IpcStartup {
    fn drop(&mut self) {
        self.future.take();
        if let Err(error) = self.guard.borrow_mut().rollback() {
            tracing::error!(%error,"ERR_AGENTOS_IPC_ROLLBACK");
        }
    }
}

pub(super) fn finish_startup(
    future: IpcStartupFuture,
    guard: Rc<RefCell<PendingProcessIpc>>,
) -> IpcStartupFuture {
    Box::pin(IpcStartup {
        future: Some(future),
        guard,
    })
}

impl PendingProcessIpc {
    pub(super) fn new(vm: Option<WeakVmHandle>) -> Self {
        Self {
            vm,
            endpoints: Vec::new(),
            metadata_limit: DEFAULT_MAX_OPEN_FDS,
        }
    }

    pub(super) fn install(
        &mut self,
        kernel: &mut SidecarKernel,
        parent_pid: u32,
        child_pid: u32,
        options: &ProcessLaunchOptions,
    ) -> Result<(), VmError> {
        let slots = options
            .stdio
            .iter()
            .enumerate()
            .filter(|(_, mode)| mode.as_str() == "ipc")
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        if slots.is_empty() {
            return Ok(());
        }
        if slots.len() != 1 || slots[0] < 3 {
            return Err(VmError::host(
                "EINVAL",
                "IPC requires exactly one stdio entry above fd2",
            ));
        }
        let serialization = options.ipc_serialization.as_deref().unwrap_or("json");
        if !matches!(serialization, "json" | "advanced") {
            return Err(VmError::host("EINVAL", "invalid IPC serialization mode"));
        }
        // Match the finite descriptor-table bound, including its None fallback.
        // Ordinary fd_close can free descriptors while IPC records remain owned.
        self.metadata_limit = metadata_limit(kernel);
        let mut aliases: Vec<(u32, Endpoint)> = Vec::new();
        let prepared = (|| {
            let (parent_fd, peer_fd) = kernel
                .fd_socketpair(
                    EXECUTION_DRIVER_NAME,
                    parent_pid,
                    SocketType::Stream,
                    true,
                    true,
                )
                .map_err(kernel_error)?;
            let record = |pid, fd, generation| -> Result<Endpoint, VmError> {
                Ok(Endpoint {
                    fd,
                    description: kernel
                        .fd_description_identity(EXECUTION_DRIVER_NAME, pid, fd)
                        .map_err(kernel_error)?
                        .0,
                    generation,
                    serialization: serialization.to_owned(),
                })
            };
            let mut parent = record(parent_pid, parent_fd, 0)?;
            parent.generation = parent.description;
            aliases.push((parent_pid, parent.clone()));
            aliases.push((parent_pid, record(parent_pid, peer_fd, parent.generation)?));
            let transfer = kernel
                .fd_transfer(EXECUTION_DRIVER_NAME, parent_pid, peer_fd)
                .map_err(kernel_error)?;
            let child_fd = kernel
                .fd_install_transfer(EXECUTION_DRIVER_NAME, child_pid, &transfer, true)
                .map_err(kernel_error)?;
            drop(transfer);
            let mut child = Endpoint {
                fd: child_fd,
                description: kernel
                    .fd_description_identity(EXECUTION_DRIVER_NAME, child_pid, child_fd)
                    .map_err(kernel_error)?
                    .0,
                generation: parent.generation,
                serialization: serialization.to_owned(),
            };
            aliases.push((child_pid, child.clone()));
            close_matching(kernel, parent_pid, &aliases[1].1)?;
            let slot = u32::try_from(slots[0])
                .map_err(|_| VmError::host("EINVAL", "IPC slot exceeds u32"))?;
            if child_fd != slot {
                kernel
                    .fd_dup2(EXECUTION_DRIVER_NAME, child_pid, child_fd, slot)
                    .map_err(kernel_error)?;
                child.fd = slot;
                aliases.push((child_pid, child.clone()));
                kernel
                    .fd_close(EXECUTION_DRIVER_NAME, child_pid, child_fd)
                    .map_err(kernel_error)?;
            }
            kernel
                .fd_fcntl(
                    EXECUTION_DRIVER_NAME,
                    child_pid,
                    child.fd,
                    F_SETFD,
                    FD_CLOEXEC,
                )
                .map_err(kernel_error)?;
            Ok((parent, child))
        })();
        match prepared {
            Ok((parent, child)) => {
                self.endpoints = vec![(parent_pid, parent), (child_pid, child)];
                Ok(())
            }
            Err(error) => {
                let mut errors = Vec::new();
                for (pid, endpoint) in aliases.iter().rev() {
                    if let Err(cleanup) = close_matching(kernel, *pid, endpoint) {
                        errors.push(cleanup.to_string());
                    }
                }
                if errors.is_empty() {
                    Err(error)
                } else {
                    Err(VmError::host(
                        "ERR_AGENTOS_IPC_ROLLBACK",
                        format!("{error}; {}", errors.join("; ")),
                    ))
                }
            }
        }
    }

    pub(super) fn rollback(&mut self) -> Result<(), VmError> {
        if self.endpoints.is_empty() {
            return Ok(());
        }
        let Some(vm) = self.vm.as_ref().and_then(WeakVmHandle::upgrade) else {
            self.endpoints.clear();
            return Ok(());
        };
        vm.try_command("roll back IPC setup", |vm| {
            let mut errors = Vec::new();
            self.endpoints.retain(|(pid, endpoint)| {
                match close_matching(&mut vm.kernel, *pid, endpoint) {
                    Ok(()) => false,
                    Err(error) => {
                        errors.push(error.to_string());
                        true
                    }
                }
            });
            if errors.is_empty() {
                Ok(())
            } else {
                Err(VmError::host("ERR_AGENTOS_IPC_ROLLBACK", errors.join("; ")))
            }
        })
    }

    pub(super) fn commit(
        &mut self,
        parent: &mut ActiveProcess,
        child: &mut ActiveProcess,
        child_id: &str,
    ) -> Result<(), VmError> {
        if self.endpoints.is_empty() {
            return Ok(());
        }
        if parent.ipc.children.contains_key(child_id) || child.ipc.parent.is_some() {
            return Err(VmError::host(
                "EBUSY",
                "IPC endpoint owner is already registered",
            ));
        }
        for owner in [&mut *parent, &mut *child] {
            owner
                .ipc
                .observe_metadata(self.metadata_limit, owner.kernel_pid);
            let count = owner.ipc.metadata_count();
            if count >= self.metadata_limit {
                return Err(metadata_admission_error(
                    owner.kernel_pid,
                    count,
                    self.metadata_limit,
                ));
            }
        }
        let (_, child_endpoint) = self.endpoints.pop().expect("prepared child IPC endpoint");
        let (_, parent_endpoint) = self.endpoints.pop().expect("prepared parent IPC endpoint");
        child.ipc.parent = Some(child_endpoint);
        parent
            .ipc
            .children
            .insert(child_id.to_owned(), parent_endpoint);
        parent
            .ipc
            .observe_metadata(self.metadata_limit, parent.kernel_pid);
        child
            .ipc
            .observe_metadata(self.metadata_limit, child.kernel_pid);
        if let Some(wait) = parent.ipc.wait.clone() {
            wait.changed.notify_one();
        }
        Ok(())
    }
}

impl Drop for PendingProcessIpc {
    fn drop(&mut self) {
        if let Err(error) = self.rollback() {
            tracing::error!(%error,"ERR_AGENTOS_IPC_ROLLBACK");
        }
    }
}

#[cfg(test)]
#[path = "../../tests/private_process_ipc/mod.rs"]
mod tests;

fn caller_mut<'a>(
    vm: &'a mut VmState,
    root: &str,
    path: &[String],
    generation: u64,
    pid: u32,
) -> Result<(&'a mut SidecarKernel, &'a mut ActiveProcess), VmError> {
    if vm.generation != generation {
        return Err(VmError::host("ESTALE", "IPC execution generation expired"));
    }
    let mut process = vm
        .active_processes
        .get_mut(root)
        .ok_or_else(|| VmError::host("ESTALE", "IPC process retired"))?;
    for segment in path {
        process = process
            .child_processes
            .get_mut(segment)
            .ok_or_else(|| VmError::host("ESTALE", "IPC descendant retired"))?;
    }
    if process.kernel_pid != pid {
        return Err(VmError::host("ESTALE", "IPC execution PID changed"));
    }
    Ok((&mut vm.kernel, process))
}

fn validate_interests(
    kernel: &SidecarKernel,
    process: &ActiveProcess,
    interests: &[ProcessIpcInterest],
) -> Result<(), VmError> {
    if interests.is_empty()
        || interests.len() > process.ipc.children.len() + usize::from(process.ipc.parent.is_some())
    {
        return Err(VmError::host(
            "EINVAL",
            "IPC interest set exceeds owned channels",
        ));
    }
    let mut seen = BTreeSet::new();
    for interest in interests {
        let endpoint = checked_endpoint(kernel, process, &interest.target)?;
        if !seen.insert(endpoint.description) {
            return Err(VmError::host("EINVAL", "duplicate IPC poll endpoint"));
        }
    }
    Ok(())
}

fn execute_operation(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    operation: ProcessIpcOperation,
) -> Result<Value, VmError> {
    match operation {
        ProcessIpcOperation::Info { child_id } => Ok(endpoint(&process.ipc, child_id.as_ref().map(|id| id.as_str())).map(|endpoint| json!({"generation":endpoint.generation.to_string(),"serialization":endpoint.serialization})).unwrap_or(Value::Null)),
        ProcessIpcOperation::Read { target, max_bytes } => {
            if max_bytes.get() == 0 || max_bytes.get() > 64 * 1024 { return Err(VmError::host("E2BIG", "IPC read exceeds 64KiB")); }
            let fd = checked_endpoint(kernel, process, &target)?.fd;
            match kernel.fd_read_with_timeout_result(EXECUTION_DRIVER_NAME, process.kernel_pid, fd, max_bytes.get(), Some(Duration::ZERO)) {
                Ok(Some(bytes)) => Ok(json!({"dataBase64":base64::engine::general_purpose::STANDARD.encode(bytes)})),
                Ok(None) => Ok(json!({"done":true})),
                Err(error) if matches!(error.code(), "EAGAIN" | "EWOULDBLOCK") => Ok(Value::Null),
                Err(error) => Err(kernel_error(error)),
            }
        }
        ProcessIpcOperation::Write { target, chunk } => {
            if chunk.as_slice().len() > 64 * 1024 { return Err(VmError::host("E2BIG", "IPC write exceeds 64KiB")); }
            let fd = checked_endpoint(kernel, process, &target)?.fd;
            match kernel.fd_write_nonblocking(EXECUTION_DRIVER_NAME, process.kernel_pid, fd, chunk.as_slice()) {
                Ok(written) => Ok(json!({"written":written})),
                Err(error) if matches!(error.code(), "EAGAIN" | "EWOULDBLOCK") => Ok(json!({"written":0})),
                Err(error) => Err(kernel_error(error)),
            }
        }
        ProcessIpcOperation::Close { target } => {
            // Ownership is the record and its exact generation. An ordinary
            // close/replacement must still retire that record without touching
            // a new description installed at the reusable descriptor number.
            let endpoint = recorded_endpoint(process, &target)?.clone();
            close_matching(kernel, process.kernel_pid, &endpoint)?;
            match target.child_id { Some(id) => {process.ipc.children.remove(id.as_str());}, None => {process.ipc.parent=None;} }
            process.ipc.observe_metadata(metadata_limit(kernel), process.kernel_pid);
            if let Some(wait) = process.ipc.wait.clone() { wait.changed.notify_one(); }
            Ok(Value::Null)
        }
        ProcessIpcOperation::Update { poll_id, expected_revision, interests } => {
            validate_interests(kernel, process, interests.as_slice())?;
            let wait = process.ipc.wait.clone().filter(|wait| !wait.closed.get() && wait.id == poll_id && wait.revision.get() == expected_revision)
                .ok_or_else(|| VmError::host("ESTALE", "IPC poll ownership changed"))?;
            let revision = expected_revision.checked_add(1).ok_or_else(|| VmError::host("EOVERFLOW", "IPC interest revision exhausted"))?;
            *wait.interests.borrow_mut() = interests.as_slice().to_vec();
            wait.revision.set(revision);
            wait.changed.notify_one();
            Ok(json!({"revision":revision.to_string()}))
        }
        ProcessIpcOperation::Poll { .. } => Err(VmError::host("EINVAL", "IPC poll requires its owned waiter")),
    }
}

/// A notifier task retains no VM/process state or V8 capability. The direct
/// reply and bounded interest list remain owned by the process on the actor.
#[derive(Clone)]
pub(super) struct IpcWake {
    sender: tokio::sync::mpsc::Sender<ProcessEventEnvelope>,
    notify: Arc<tokio::sync::Notify>,
    connection_id: String,
    session_id: String,
    vm_id: String,
    root: String,
    path: Vec<String>,
}

impl IpcWake {
    async fn publish(self) {
        let envelope = ProcessEventEnvelope {
            connection_id: self.connection_id,
            session_id: self.session_id,
            vm_id: self.vm_id,
            process_id: self.root,
            child_path: self.path,
            event: ActiveExecutionEvent::DeferredPosixPollWake,
        };
        self.notify.notify_waiters();
        self.notify.notify_one();
        if let Err(error) = self.sender.send(envelope).await {
            tracing::error!(%error,"ERR_AGENTOS_IPC_WAKE_DROPPED");
            return;
        }
        self.notify.notify_waiters();
        self.notify.notify_one();
    }
}

pub(super) fn wake_lane<B>(
    sidecar: &VmManager<B>,
    vm_id: &str,
    root: &str,
    path: Vec<String>,
) -> Result<IpcWake, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let vm = sidecar
        .vms
        .get(vm_id)
        .ok_or_else(|| missing_vm_error(vm_id))?;
    Ok(IpcWake {
        sender: sidecar.process_event_sender.clone(),
        notify: Arc::clone(&sidecar.process_event_notify),
        connection_id: vm.connection_id.clone(),
        session_id: vm.session_id.clone(),
        vm_id: vm_id.to_owned(),
        root: root.to_owned(),
        path,
    })
}

fn settle_poll(
    process: &mut ActiveProcess,
    wait: &IpcWait,
    result: Result<Value, VmError>,
) -> Result<(), VmError> {
    let settled = match result {
        Ok(value) => match wait.reply.succeed_json(value) {
            Ok(()) => Ok(()),
            Err(error) if !wait.reply.is_terminal() => wait.reply.fail(error),
            Err(error) => Err(error),
        },
        Err(error) => wait.reply.fail(host_service_error(&error)),
    };
    process.ipc.cancel_wait();
    settled.map_err(VmError::from)
}

fn service(kernel: &mut SidecarKernel, process: &mut ActiveProcess) -> Result<(), VmError> {
    let Some(wait) = process.ipc.wait.clone() else {
        return Ok(());
    };
    if wait.closed.get() || wait.reply.is_terminal() {
        process.ipc.cancel_wait();
        return Ok(());
    }
    if let Some(task) = wait.task.borrow_mut().take() {
        task.abort();
    }
    let handle = kernel.poll_wait_handle();
    let observed = handle.snapshot();
    let mut ready = Vec::new();
    for interest in wait.interests.borrow().iter() {
        let endpoint = match checked_endpoint(kernel, process, &interest.target) {
            Ok(endpoint) => endpoint,
            Err(error) if matches!(error_code(&error), "ESTALE" | "EBADF" | "ESRCH") => {
                ready.push(json!({"childId":interest.target.child_id.as_ref().map(|id|id.as_str()),"generation":interest.target.generation.to_string(),"events":POLLNVAL.bits()}));
                continue;
            }
            Err(error) => {
                return settle_poll(process, &wait, Err(error));
            }
        };
        let result = match kernel.poll_fds(
            EXECUTION_DRIVER_NAME,
            process.kernel_pid,
            vec![PollFd {
                fd: endpoint.fd,
                events: if interest.writable {
                    POLLIN | POLLOUT
                } else {
                    POLLIN
                },
                revents: PollEvents::empty(),
            }],
            0,
        ) {
            Ok(result) => result,
            Err(error) => return settle_poll(process, &wait, Err(kernel_error(error))),
        };
        if result.ready_count > 0 {
            ready.push(json!({"childId":interest.target.child_id.as_ref().map(|id|id.as_str()),"generation":interest.target.generation.to_string(),"events":result.fds[0].revents.bits()}));
        }
    }
    if !ready.is_empty() {
        return settle_poll(
            process,
            &wait,
            Ok(json!({"revision":wait.revision.get().to_string(),"ready":ready})),
        );
    }
    let changed = Arc::clone(&wait.changed);
    let wake = wait.wake.clone();
    match process
        .runtime_context
        .spawn(agentos_driver_tokio::TaskClass::Vm, async move {
            tokio::select! {_=handle.wait_for_change_async(observed)=>{},_=changed.notified()=>{}}
            wake.publish().await;
        }) {
        Ok(task) => *wait.task.borrow_mut() = Some(task),
        Err(error) => {
            wait.reply
                .fail(host_service_error(&VmError::from(error)))
                .map_err(VmError::from)?;
            process.ipc.cancel_wait();
        }
    }
    Ok(())
}

pub(super) fn dispatch(
    vm: VmHandle,
    root: String,
    path: Vec<String>,
    operation: ProcessIpcOperation,
    reply: DirectHostReplyHandle,
    wake: Result<IpcWake, VmError>,
) -> Pin<Box<dyn Future<Output = Result<(), VmError>>>> {
    let identity = reply.identity();
    let result = (|| {
        let wake = wake?;
        vm.try_command("admit owned IPC operation", |vm| {
            let (kernel, process) =
                caller_mut(vm, &root, &path, identity.generation, identity.pid)?;
            if let ProcessIpcOperation::Poll {
                poll_id,
                ref interests,
            } = operation
            {
                validate_interests(kernel, process, interests.as_slice())?;
                if process
                    .ipc
                    .wait
                    .as_ref()
                    .is_some_and(|wait| !wait.closed.get() && !wait.reply.is_terminal())
                {
                    return Err(VmError::host("EBUSY", "process already owns an IPC poll"));
                }
                if !reply.claim().map_err(VmError::from)? {
                    return Ok(());
                }
                process.ipc.wait = Some(Rc::new(IpcWait {
                    reply: reply.clone(),
                    id: poll_id,
                    revision: Cell::new(0),
                    interests: RefCell::new(interests.as_slice().to_vec()),
                    closed: Cell::new(false),
                    changed: Arc::new(tokio::sync::Notify::new()),
                    wake,
                    task: RefCell::new(None),
                }));
                service(kernel, process)
            } else {
                if !reply.claim().map_err(VmError::from)? {
                    return Ok(());
                }
                let result = execute_operation(kernel, process, operation);
                match result {
                    Ok(value) => reply.succeed_json(value),
                    Err(error) => reply.fail(host_service_error(&error)),
                }
                .map_err(VmError::from)?;
                service(kernel, process)
            }
        })
    })();
    // Setup/recheck settles or parks its direct reply, then releases the VM
    // operation permit immediately. Idle readiness never retains this future.
    drop(vm);
    Box::pin(async move {
        if let Err(error) = result {
            if !reply.is_terminal() {
                reply
                    .fail(host_service_error(&error))
                    .map_err(VmError::from)?;
            }
        }
        Ok(())
    })
}

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(super) fn recheck_process_ipc(
        &mut self,
        vm_id: &str,
        root: &str,
        path: &[String],
    ) -> Result<(), VmError> {
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        let generation = vm.generation;
        let vm = &mut *vm;
        let Some(mut process) = vm.active_processes.get_mut(root) else {
            return Ok(());
        };
        for part in path {
            let Some(child) = process.child_processes.get_mut(part) else {
                return Ok(());
            };
            process = child;
        }
        process.apply_runtime_controls()?;
        if let Some(wait) = process.ipc.wait.as_ref() {
            if wait.reply.identity().generation != generation
                || wait.reply.identity().pid != process.kernel_pid
            {
                process.ipc.cancel_wait();
                return Ok(());
            }
        }
        service(&mut vm.kernel, process)
    }
}
