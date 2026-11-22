//! The per-execution channel from host imports to the sidecar kernel.

use crate::backend::{direct_host_reply_channel, HostCallIdentity, HostCallReply, HostServiceError};
use crate::host::{HostOperation, HostProcessContext, ProcessHostCapabilitySet};
use crate::{ExecutionDiagnostics, QueuedWasmEvent, WorkerTransport};
use agentos_driver_tokio::accounting::ResourceLedger;
use agentos_executor_wasm_abi::{StartWasmExecutionRequest, WasmExecutionEvent};
use flume::Sender;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::Notify;

pub const DEFAULT_MAX_HOST_REPLY_BYTES: usize = 16 * 1024 * 1024;

/// One generation-bound direct waiter namespace shared by module loading and
/// every import issued by a Store. It owns no sidecar or kernel state.
#[derive(Clone)]
pub struct WasmHostClient {
    process: HostProcessContext,
    host: Option<ProcessHostCapabilitySet>,
    worker: Option<Arc<dyn WorkerTransport>>,
    next_call_id: Arc<AtomicU64>,
    max_host_reply_bytes: usize,
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    signal_pending: Arc<AtomicBool>,
    resources: Arc<ResourceLedger>,
    events: Sender<QueuedWasmEvent>,
    event_notify: Option<Arc<Notify>>,
    diagnostics: Option<Arc<ExecutionDiagnostics>>,
    active_cpu_origin_ns: Arc<OnceLock<u64>>,
}

impl WasmHostClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: ProcessHostCapabilitySet,
        max_host_reply_bytes: usize,
        cancelled: Arc<AtomicBool>,
        cancel_notify: Arc<Notify>,
        signal_pending: Arc<AtomicBool>,
        resources: Arc<ResourceLedger>,
        events: Sender<QueuedWasmEvent>,
        event_notify: Option<Arc<Notify>>,
    ) -> Self {
        Self {
            process: host.process(),
            host: Some(host),
            worker: None,
            next_call_id: Arc::new(AtomicU64::new(1)),
            max_host_reply_bytes,
            cancelled,
            cancel_notify,
            signal_pending,
            resources,
            events,
            event_notify,
            diagnostics: None,
            active_cpu_origin_ns: Arc::new(OnceLock::new()),
        }
    }

    pub fn new_worker(
        worker: Arc<dyn WorkerTransport>,
        max_host_reply_bytes: usize,
        cancelled: Arc<AtomicBool>,
        cancel_notify: Arc<Notify>,
        signal_pending: Arc<AtomicBool>,
        resources: Arc<ResourceLedger>,
        events: Sender<QueuedWasmEvent>,
    ) -> Self {
        Self {
            process: worker.process(),
            host: None,
            worker: Some(worker),
            next_call_id: Arc::new(AtomicU64::new(1)),
            max_host_reply_bytes,
            cancelled,
            cancel_notify,
            signal_pending,
            resources,
            events,
            event_notify: None,
            diagnostics: None,
            active_cpu_origin_ns: Arc::new(OnceLock::new()),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Arc<ExecutionDiagnostics>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Threaded execution owns an isolated worker process. Its process CPU
    /// clock includes every pthread, including threads between epoch checks.
    /// Direct execution shares a process with other VMs and uses its own thread.
    pub fn cpu_time_ns(&self) -> Result<u64, HostServiceError> {
        cpu_time_ns(self.worker.is_some())
    }

    pub fn check_active_cpu_budget(
        &self,
        limit_ms: Option<u32>,
    ) -> Result<(), HostServiceError> {
        if let (Some(limit_ms), Some(origin)) = (limit_ms, self.active_cpu_origin_ns.get()) {
            let observed_ns = self.cpu_time_ns()?.saturating_sub(*origin);
            let limit_ns = u64::from(limit_ms) * 1_000_000;
            if observed_ns >= limit_ns {
                return Err(active_cpu_limit_error(limit_ns, observed_ns));
            }
        }
        Ok(())
    }

    /// The CPU-clock origin shared by every Store of this execution, set by
    /// the first one. Exec replacements and extra threads keep the origin.
    pub fn active_cpu_origin(&self, candidate_ns: u64) -> u64 {
        *self.active_cpu_origin_ns.get_or_init(|| candidate_ns)
    }

    pub fn process(&self) -> HostProcessContext {
        self.process
    }

    pub fn max_host_reply_bytes(&self) -> usize {
        self.max_host_reply_bytes
    }

    pub fn canceled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn signal_pending(&self) -> bool {
        self.worker
            .as_ref()
            .map(|worker| worker.signal_pending())
            .unwrap_or_else(|| self.signal_pending.load(Ordering::Acquire))
    }

    pub async fn submit(
        &self,
        operation: HostOperation,
        retained_request_bytes: usize,
    ) -> Result<HostCallReply, HostServiceError> {
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.first_host_call();
        }
        if self.canceled() {
            return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled before host-call admission",
            ));
        }
        if let Some(worker) = self.worker.as_ref() {
            return worker.submit(operation).await;
        }
        let call_id = self
            .next_call_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call identity space is exhausted",
                )
            })?;
        let process = self.process;
        let identity = HostCallIdentity {
            generation: process.generation,
            pid: process.pid,
            call_id,
        };
        let host = self.host.as_ref().ok_or_else(|| {
            HostServiceError::new("EIO", "direct Wasmtime host capability is unavailable")
        })?;
        let admission = host.admit_request(retained_request_bytes)?;
        let (reply, receiver) = direct_host_reply_channel(identity, self.max_host_reply_bytes)?;
        host.submit(operation, reply, admission)?;
        tokio::select! {
            reply = receiver => reply,
            () = self.cancel_notify.notified() => Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while awaiting a host operation",
            )),
        }
    }

    /// Route an owned native ABI request through the shared compatibility
    /// decoder and then the common typed host-operation dispatcher. The reply
    /// capability is direct and call-specific; this path never uses V8's
    /// synchronous line protocol or scans an event stream for a response.
    pub async fn submit_adapter_call(
        &self,
        method: String,
        args: Vec<Value>,
        raw_bytes_args: HashMap<usize, Vec<u8>>,
    ) -> Result<HostCallReply, HostServiceError> {
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.first_host_call();
            diagnostics.first_guest_host_call();
            if matches!(method.as_str(), "__kernel_stdio_write" | "process.fd_write")
                && args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|fd| matches!(fd, 1 | 2))
            {
                diagnostics.first_output();
            }
        }
        if self.canceled() {
            return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled before adapter-call admission",
            ));
        }
        if let Some(worker) = self.worker.as_ref() {
            return worker
                .submit_adapter_call(method, args, raw_bytes_args)
                .await;
        }
        let call_id = self
            .next_call_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call identity space is exhausted",
                )
            })?;
        let process = self.process;
        let identity = HostCallIdentity {
            generation: process.generation,
            pid: process.pid,
            call_id,
        };
        let (reply, receiver) = direct_host_reply_channel(identity, self.max_host_reply_bytes)?;
        let request = agentos_executor_contract::HostRpcRequest {
            id: call_id,
            method,
            args,
            raw_bytes_args,
        };
        let retained_event_bytes = request
            .method
            .len()
            .checked_add(
                serde_json::to_vec(&request.args)
                    .map_err(|error| {
                        HostServiceError::new(
                            "ERR_AGENTOS_WASMTIME_HOST_CALL_ENCODING",
                            error.to_string(),
                        )
                    })?
                    .len(),
            )
            .and_then(|total| {
                request
                    .raw_bytes_args
                    .values()
                    .try_fold(total, |total, bytes| total.checked_add(bytes.len()))
            })
            .ok_or_else(|| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call retained byte accounting overflowed",
                )
            })?;
        let event = QueuedWasmEvent::new(
            &self.resources,
            Ok(WasmExecutionEvent::HostCall { request, reply }),
            retained_event_bytes,
        )?;
        tokio::select! {
            result = self.events.send_async(event) => result.map_err(|_| HostServiceError::new(
                "EPIPE",
                "Wasmtime host-call event receiver was dropped",
            ))?,
            () = self.cancel_notify.notified() => return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while admitting an adapter call",
            )),
        }
        if let Some(notify) = self.event_notify.as_ref() {
            notify.notify_one();
        }
        tokio::select! {
            reply = receiver => reply,
            () = self.cancel_notify.notified() => Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while awaiting an adapter call",
            )),
        }
    }

    pub async fn publish_stderr(&self, bytes: Vec<u8>) -> Result<(), HostServiceError> {
        if let Some(worker) = self.worker.as_ref() {
            return worker.publish_stderr(bytes).await;
        }
        let retained_bytes = bytes.len();
        let event = QueuedWasmEvent::new(
            &self.resources,
            Ok(WasmExecutionEvent::Stderr(bytes)),
            retained_bytes,
        )?;
        tokio::select! {
            result = self.events.send_async(event) => {
                result.map_err(|_| HostServiceError::new(
                    "EPIPE",
                    "Wasmtime stderr event receiver was dropped",
                ))?;
            }
            () = self.cancel_notify.notified() => return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while publishing stderr",
            )),
        }
        if let Some(notify) = self.event_notify.as_ref() {
            notify.notify_one();
        }
        Ok(())
    }

    pub fn worker_client(&self) -> Option<&Arc<dyn WorkerTransport>> {
        self.worker.as_ref()
    }

    pub fn report_thread_group_exit(&self, code: i32) -> Result<(), HostServiceError> {
        if let Some(worker) = self.worker.as_ref() {
            worker.report_group_exit(code)?;
        }
        Ok(())
    }

    pub fn report_thread_group_failure(&self, error: HostServiceError) {
        if let Some(worker) = self.worker.as_ref() {
            if let Err(report_error) = worker.report_group_failure(error) {
                eprintln!(
                    "{}: failed to report pthread group failure to parent: {}",
                    report_error.code, report_error.message
                );
            }
        }
    }
}

pub fn max_host_reply_bytes(
    request: &StartWasmExecutionRequest,
) -> Result<usize, HostServiceError> {
    request
        .limits
        .max_sync_rpc_response_line_bytes
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_LIMIT_CONFIG",
                "limits.reactor.maxBridgeResponseBytes does not fit this platform",
            )
        })
        .map(|value| value.unwrap_or(DEFAULT_MAX_HOST_REPLY_BYTES))
}

pub fn active_cpu_limit_error(limit_ns: u64, observed_ns: u64) -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT",
        "Wasmtime execution exhausted its aggregate active CPU budget; raise limits.wasm.activeCpuTimeLimitMs",
    )
    .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs", "limit": limit_ns / 1_000_000, "observed": observed_ns / 1_000_000 }))
}

#[cfg(unix)]
pub fn cpu_time_ns(process_wide: bool) -> Result<u64, HostServiceError> {
    let clock = if process_wide {
        nix::time::ClockId::CLOCK_PROCESS_CPUTIME_ID
    } else {
        nix::time::ClockId::CLOCK_THREAD_CPUTIME_ID
    };
    let value = nix::time::clock_gettime(clock).map_err(|error| {
        eprintln!("ERR_AGENTOS_WASMTIME_CPU_CLOCK: private clock diagnostic: {error}");
        HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
            "failed to read execution CPU clock for limits.wasm.activeCpuTimeLimitMs",
        )
        .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" }))
    })?;
    u64::try_from(value.tv_sec())
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000_000))
        .and_then(|seconds| {
            u64::try_from(value.tv_nsec())
                .ok()
                .and_then(|nanos| seconds.checked_add(nanos))
        })
        .ok_or_else(|| {
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
                "execution CPU clock for limits.wasm.activeCpuTimeLimitMs is outside its supported range",
            ).with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" }))
        })
}

#[cfg(not(unix))]
pub fn cpu_time_ns(_process_wide: bool) -> Result<u64, HostServiceError> {
    Err(HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
        "execution CPU clock for limits.wasm.activeCpuTimeLimitMs is unsupported on this platform",
    )
    .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" })))
}
