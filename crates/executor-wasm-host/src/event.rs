use crate::backend::HostServiceError;
use agentos_driver_tokio::accounting::{Reservation, ResourceClass, ResourceLedger};
use agentos_executor_wasm_abi::{WasmExecutionError, WasmExecutionEvent};
use std::sync::Arc;

/// One queued executor event plus the ledger ownership for bytes retained by
/// that queue slot. The reservation is released when the event leaves the
/// Wasmtime queue; downstream output/host-call paths apply their own existing
/// retention accounting after that handoff.
pub struct QueuedWasmEvent {
    event: Result<WasmExecutionEvent, WasmExecutionError>,
    _retained_bytes: Option<Reservation>,
}

impl QueuedWasmEvent {
    pub fn new(
        resources: &Arc<ResourceLedger>,
        event: Result<WasmExecutionEvent, WasmExecutionError>,
        retained_bytes: usize,
    ) -> Result<Self, HostServiceError> {
        let retained_bytes = if retained_bytes == 0 {
            None
        } else {
            Some(
                resources
                    .reserve(ResourceClass::AsyncCompletionBytes, retained_bytes)
                    .map_err(|error| {
                        HostServiceError::new(
                            "ERR_AGENTOS_WASM_EVENT_BYTES_LIMIT",
                            error.to_string(),
                        )
                        .with_details(serde_json::json!({
                            "limitName": "limits.reactor.maxAsyncCompletionBytes",
                            "observed": retained_bytes,
                        }))
                    })?,
            )
        };
        Ok(Self {
            event,
            _retained_bytes: retained_bytes,
        })
    }

    pub fn into_event(self) -> Result<WasmExecutionEvent, WasmExecutionError> {
        self.event
    }
}
