use crate::backend::{HostCallReply, HostServiceError};
use crate::host::{ExecImportTiers, HostOperation, HostProcessContext};
use crate::memory::GuestMemoryError;
use crate::state::{HostState, PendingExecReplacement};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a host import needs from the engine running the guest that called it.
///
/// Everything else (descriptors, processes, signals, filesystem) is reached
/// through [`HostState::host`]. Implementations must not hold guest-memory
/// references across calls: every read and write re-resolves the memory.
pub trait WasmGuest {
    /// The engine's compiled module, carried in a pending exec replacement.
    type Module: Send + Sync + 'static;

    fn data(&self) -> &HostState<Self::Module>;
    fn data_mut(&mut self) -> &mut HostState<Self::Module>;

    /// Current byte length of the guest's exported `memory`.
    fn memory_size(&mut self) -> Result<usize, GuestMemoryError>;
    /// Copy a validated range out of guest memory.
    fn memory_read(&mut self, range: Range<usize>) -> Result<Vec<u8>, GuestMemoryError>;
    /// Copy bytes into a validated range of guest memory.
    fn memory_write(&mut self, range: Range<usize>, bytes: &[u8]) -> Result<(), GuestMemoryError>;

    /// Whether the guest exports `__wasi_signal_trampoline: (i32) -> ()`.
    fn has_signal_trampoline(&mut self) -> bool;
    /// Run the guest's signal handler for `signal` through its trampoline.
    /// An engine that calls guests synchronously runs the handler before
    /// returning and hands back a completed future.
    fn call_signal_trampoline(&mut self, signal: i32) -> BoxFuture<'_, crate::Result<()>>;

    /// Remaining deterministic fuel, when the engine meters it.
    fn fuel(&mut self) -> Result<u64, HostServiceError>;

    /// Compile an exec replacement image and check it before the kernel
    /// commits the exec: valid module, `_start: () -> ()`, and the permission
    /// tiers whose import sets it satisfies. `ERR_AGENTOS_WASM_INVALID_MODULE`
    /// marks bytes that are not a WebAssembly module.
    fn prepare_exec_replacement(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Self::Module, ExecImportTiers), HostServiceError>;
}

/// The engine's guest-thread group, when the guest runs WASI threads.
pub trait ThreadGroupHooks<M>: Send + Sync {
    fn request_process_exit(&self, code: i32) -> Result<(), HostServiceError>;
    fn request_exec(
        &self,
        replacement: Option<PendingExecReplacement<M>>,
    ) -> Result<(), HostServiceError>;
    fn is_shutting_down(&self) -> bool;
}

/// A host channel through an isolated worker process instead of direct
/// submission. Only the Wasmtime threaded executor uses one.
pub trait WorkerTransport: Send + Sync {
    fn process(&self) -> HostProcessContext;
    fn signal_pending(&self) -> bool;
    fn submit(
        &self,
        operation: HostOperation,
    ) -> BoxFuture<'_, Result<HostCallReply, HostServiceError>>;
    fn submit_adapter_call(
        &self,
        method: String,
        args: Vec<Value>,
        raw_bytes_args: HashMap<usize, Vec<u8>>,
    ) -> BoxFuture<'_, Result<HostCallReply, HostServiceError>>;
    fn publish_stderr(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<(), HostServiceError>>;
    fn report_group_exit(&self, code: i32) -> Result<(), HostServiceError>;
    fn report_group_failure(&self, error: HostServiceError) -> Result<(), HostServiceError>;
    fn sample_exec_cpu(&self) -> Result<ExecCpuSample, HostServiceError>;
    #[allow(clippy::too_many_arguments)]
    fn commit_exec(
        &self,
        open: Value,
        method: String,
        request: Value,
        remaining_cpu_ms: Option<u32>,
        cpu_sample: Option<ExecCpuSample>,
        remaining_fuel: Option<u64>,
    ) -> BoxFuture<'_, Result<HostCallReply, HostServiceError>>;
}

/// Both clocks are sampled by trusted worker code before its remaining-budget
/// snapshot. Monotonic time is shared with the parent across process boundaries.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ExecCpuSample {
    pub monotonic_ns: u64,
    pub process_cpu_ns: u64,
}
