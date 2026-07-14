//! Sidecar-owned bounded process-output capture shared by native and browser shells.

use crate::{SidecarCoreError, VmLimits};
use agentos_bridge::queue_tracker::{register_limit, QueueGauge, TrackedLimit};
use agentos_sidecar_protocol::protocol::{GuestRuntimeKind, RejectedResponse, StreamChannel};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const CAPTURED_OUTPUT_LIMIT_ERROR_CODE: &str = "ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED";
/// Caller-provided process ids are repeated in terminal frames and capture-overflow diagnostics.
/// Bounding them makes the captured-output frame budget independent of untrusted request size.
pub const MAX_PROCESS_ID_BYTES: usize = 1024;
/// Covers a maximum-size process id in both the terminal field and overflow message, plus BARE
/// framing, ownership, schema, error-code, configuration-path, and numeric metadata.
pub const CAPTURE_TERMINAL_FRAME_OVERHEAD_BYTES: usize = 4 * 1024;

pub fn validate_process_id(process_id: &str) -> Result<(), SidecarCoreError> {
    if process_id.is_empty() {
        return Err(SidecarCoreError::new(
            "execute process_id must not be empty",
        ));
    }
    if process_id.len() > MAX_PROCESS_ID_BYTES {
        return Err(SidecarCoreError::new(format!(
            "execute process_id is {} bytes; process ids must be at most {MAX_PROCESS_ID_BYTES} bytes",
            process_id.len()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureChunkOutcome {
    Forward,
    Suppress,
    LimitExceeded,
}

#[derive(Debug)]
pub struct CapturedOutputBudget {
    limit_bytes: usize,
    used_bytes: AtomicUsize,
    gauge: Arc<QueueGauge>,
}

impl CapturedOutputBudget {
    pub fn for_vm(limits: &VmLimits) -> Arc<Self> {
        Arc::new(Self::new(limits.max_captured_output_bytes))
    }

    fn new(limit_bytes: usize) -> Self {
        Self {
            limit_bytes,
            used_bytes: AtomicUsize::new(0),
            gauge: register_limit(TrackedLimit::VmCapturedOutputBytes, limit_bytes),
        }
    }

    fn try_reserve(&self, bytes: usize) -> bool {
        let mut current = self.used_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                self.gauge.observe_depth(usize::MAX);
                self.gauge.observe_depth(current);
                return false;
            };
            self.gauge.observe_depth(next);
            if next > self.limit_bytes {
                self.gauge.observe_depth(current);
                return false;
            }
            match self.used_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let previous = self.used_bytes.fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(previous >= bytes, "captured-output budget underflow");
        self.gauge.observe_depth(previous.saturating_sub(bytes));
    }

    #[cfg(test)]
    fn used_bytes(&self) -> usize {
        self.used_bytes.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOutputResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub error: Option<RejectedResponse>,
}

#[derive(Debug)]
pub struct CapturedOutputState {
    error: Option<RejectedResponse>,
    limit_bytes: usize,
    config_path: &'static str,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_gauge: Arc<QueueGauge>,
    stderr_gauge: Arc<QueueGauge>,
    vm_budget: Arc<CapturedOutputBudget>,
    reserved_bytes: usize,
}

impl CapturedOutputState {
    pub fn for_runtime(
        limits: &VmLimits,
        runtime: GuestRuntimeKind,
        vm_budget: Arc<CapturedOutputBudget>,
    ) -> Self {
        let (limit_bytes, config_path) = capture_limit_for_runtime(limits, runtime);
        Self::new(limit_bytes, config_path, vm_budget)
    }

    fn new(
        limit_bytes: usize,
        config_path: &'static str,
        vm_budget: Arc<CapturedOutputBudget>,
    ) -> Self {
        Self {
            error: None,
            limit_bytes,
            config_path,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_gauge: register_limit(TrackedLimit::ProcessCapturedOutputBytes, limit_bytes),
            stderr_gauge: register_limit(TrackedLimit::ProcessCapturedOutputBytes, limit_bytes),
            vm_budget,
            reserved_bytes: 0,
        }
    }

    pub fn record_chunk(
        &mut self,
        process_id: &str,
        channel: StreamChannel,
        chunk: &[u8],
    ) -> CaptureChunkOutcome {
        if self.error.is_some() {
            return CaptureChunkOutcome::Suppress;
        }

        let (stream, captured, gauge) = match channel {
            StreamChannel::Stdout => ("stdout", &mut self.stdout, &self.stdout_gauge),
            StreamChannel::Stderr => ("stderr", &mut self.stderr, &self.stderr_gauge),
        };
        let next_len = captured.len().saturating_add(chunk.len());
        gauge.observe_depth(next_len);
        if next_len > self.limit_bytes {
            self.error = Some(RejectedResponse {
                code: String::from(CAPTURED_OUTPUT_LIMIT_ERROR_CODE),
                message: format!(
                    "process {process_id} {stream} exceeded the captured output limit of {} bytes; raise {} to allow more captured output",
                    self.limit_bytes, self.config_path
                ),
            });
            return CaptureChunkOutcome::LimitExceeded;
        }
        if !self.vm_budget.try_reserve(chunk.len()) {
            self.error = Some(RejectedResponse {
                code: String::from(CAPTURED_OUTPUT_LIMIT_ERROR_CODE),
                message: format!(
                    "process {process_id} {stream} would exceed the VM captured output limit of {} bytes; raise limits.resources.maxCapturedOutputBytes (Rust: limits.resources.max_captured_output_bytes) or reduce concurrent captured executions",
                    self.vm_budget.limit_bytes
                ),
            });
            return CaptureChunkOutcome::LimitExceeded;
        }

        captured.extend_from_slice(chunk);
        self.reserved_bytes += chunk.len();
        CaptureChunkOutcome::Forward
    }

    pub fn into_result(mut self) -> CapturedOutputResult {
        CapturedOutputResult {
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            error: self.error.take(),
        }
    }
}

impl Drop for CapturedOutputState {
    fn drop(&mut self) {
        self.vm_budget.release(self.reserved_bytes);
    }
}

fn capture_limit_for_runtime(
    limits: &VmLimits,
    runtime: GuestRuntimeKind,
) -> (usize, &'static str) {
    match runtime {
        GuestRuntimeKind::JavaScript => (
            limits.js_runtime.captured_output_limit_bytes,
            "limits.jsRuntime.capturedOutputLimitBytes (Rust: limits.js_runtime.captured_output_limit_bytes)",
        ),
        GuestRuntimeKind::Python => (
            limits.python.output_buffer_max_bytes,
            "limits.python.outputBufferMaxBytes (Rust: limits.python.output_buffer_max_bytes)",
        ),
        GuestRuntimeKind::WebAssembly => (
            limits.wasm.captured_output_limit_bytes,
            "limits.wasm.capturedOutputLimitBytes (Rust: limits.wasm.captured_output_limit_bytes)",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_limit_is_per_stream_and_returns_a_typed_error() {
        let budget = Arc::new(CapturedOutputBudget::new(16));
        let mut capture = CapturedOutputState::new(
            8,
            "limits.jsRuntime.capturedOutputLimitBytes (Rust: limits.js_runtime.captured_output_limit_bytes)",
            Arc::clone(&budget),
        );

        assert_eq!(
            capture.record_chunk("process-1", StreamChannel::Stdout, b"12345678"),
            CaptureChunkOutcome::Forward
        );
        assert_eq!(
            capture.record_chunk("process-1", StreamChannel::Stderr, b"12345678"),
            CaptureChunkOutcome::Forward
        );
        assert_eq!(
            capture.record_chunk("process-1", StreamChannel::Stdout, b"9"),
            CaptureChunkOutcome::LimitExceeded
        );

        let result = capture.into_result();
        assert_eq!(
            budget.used_bytes(),
            0,
            "per-stream overflow must not charge the rejected chunk and terminal transfer releases active accounting"
        );
        let error = result
            .error
            .expect("overflow should record a terminal error");
        assert_eq!(error.code, CAPTURED_OUTPUT_LIMIT_ERROR_CODE);
        assert!(error.message.contains("limit of 8 bytes"));
        assert!(error
            .message
            .contains("limits.jsRuntime.capturedOutputLimitBytes"));
        assert!(error
            .message
            .contains("limits.js_runtime.captured_output_limit_bytes"));
    }

    #[test]
    fn runtime_specific_capture_limits_have_one_shared_mapping() {
        let mut limits = VmLimits::default();
        limits.js_runtime.captured_output_limit_bytes = 11;
        limits.python.output_buffer_max_bytes = 22;
        limits.wasm.captured_output_limit_bytes = 33;

        for (runtime, expected) in [
            (GuestRuntimeKind::JavaScript, 11),
            (GuestRuntimeKind::Python, 22),
            (GuestRuntimeKind::WebAssembly, 33),
        ] {
            let capture = CapturedOutputState::for_runtime(
                &limits,
                runtime,
                CapturedOutputBudget::for_vm(&limits),
            );
            assert_eq!(capture.limit_bytes, expected);
        }
    }

    #[test]
    fn concurrent_captures_share_and_release_the_vm_budget() {
        let mut limits = VmLimits::default();
        limits.js_runtime.captured_output_limit_bytes = 8;
        limits.max_captured_output_bytes = 10;
        let budget = CapturedOutputBudget::for_vm(&limits);
        let mut first = CapturedOutputState::for_runtime(
            &limits,
            GuestRuntimeKind::JavaScript,
            Arc::clone(&budget),
        );
        let mut second = CapturedOutputState::for_runtime(
            &limits,
            GuestRuntimeKind::JavaScript,
            Arc::clone(&budget),
        );

        assert_eq!(
            first.record_chunk("first", StreamChannel::Stdout, b"12345678"),
            CaptureChunkOutcome::Forward
        );
        assert_eq!(
            second.record_chunk("second", StreamChannel::Stdout, b"abc"),
            CaptureChunkOutcome::LimitExceeded
        );
        let error = second.into_result().error.expect("typed VM budget error");
        assert_eq!(error.code, CAPTURED_OUTPUT_LIMIT_ERROR_CODE);
        assert!(error
            .message
            .contains("limits.resources.maxCapturedOutputBytes"));
        assert_eq!(budget.used_bytes(), 8);

        drop(first);
        assert_eq!(budget.used_bytes(), 0);
        let mut third = CapturedOutputState::for_runtime(
            &limits,
            GuestRuntimeKind::JavaScript,
            Arc::clone(&budget),
        );
        assert_eq!(
            third.record_chunk("third", StreamChannel::Stdout, b"abcdefgh"),
            CaptureChunkOutcome::Forward
        );
    }

    #[test]
    fn process_id_bound_accounts_for_wire_terminal_metadata() {
        validate_process_id(&"p".repeat(MAX_PROCESS_ID_BYTES))
            .expect("maximum-size process id should be accepted");
        let error = validate_process_id(&"p".repeat(MAX_PROCESS_ID_BYTES + 1))
            .expect_err("oversize process id should be rejected");
        assert!(error.to_string().contains("at most 1024 bytes"));
    }
}
