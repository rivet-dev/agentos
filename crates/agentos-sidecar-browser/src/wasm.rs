//! wasm-bindgen entry point for the Agent OS browser sidecar.
//!
//! Mirrors secure-exec's `BrowserSidecarWasm` (pushFrame/pollEvent over the shared
//! `BrowserWireDispatcher` + `BrowserJsBridge`) but registers the Agent OS ACP
//! `BrowserExtension` into the dispatcher's sidecar, so guest ACP/session traffic
//! is handled by the Agent OS wrapper while every kernel syscall still routes
//! through the converged secure-exec wasm kernel (the sole enforcement point).

use agentos_native_sidecar_browser::wire_dispatch::{BrowserWireDispatcher, BROWSER_SIDECAR_ID};
use agentos_native_sidecar_browser::BrowserJsBridge;
use js_sys::{Error as JsError, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{pending_frames, BrowserAcpExtension};

const INTERNAL_ACP_REQUEST_ID_START: i64 = i64::MAX - 1_000_000;

#[wasm_bindgen]
pub struct AgentOsBrowserSidecarWasm {
    dispatcher: BrowserWireDispatcher<BrowserJsBridge>,
    next_internal_acp_request_id: i64,
}

#[wasm_bindgen]
impl AgentOsBrowserSidecarWasm {
    #[wasm_bindgen(constructor)]
    pub fn new(host_bridge: Option<JsValue>) -> Result<AgentOsBrowserSidecarWasm, JsValue> {
        let mut dispatcher = BrowserWireDispatcher::new(BrowserJsBridge::new(host_bridge));
        dispatcher
            .sidecar_mut()
            .register_extension(Box::new(BrowserAcpExtension::new()))
            .map_err(js_error)?;
        Ok(Self {
            dispatcher,
            next_internal_acp_request_id: INTERNAL_ACP_REQUEST_ID_START,
        })
    }

    #[wasm_bindgen(getter, js_name = sidecarId)]
    pub fn sidecar_id(&self) -> String {
        String::from(BROWSER_SIDECAR_ID)
    }

    #[wasm_bindgen(js_name = pushFrame)]
    pub fn push_frame(&mut self, frame: Uint8Array) -> Result<JsValue, JsValue> {
        let bytes = frame.to_vec();
        let response = self
            .dispatcher
            .handle_request_bytes(&bytes)
            .map_err(js_error)?;
        Ok(Uint8Array::from(response.as_slice()).into())
    }

    #[wasm_bindgen(js_name = pollEvent)]
    pub fn poll_event(&mut self) -> Result<JsValue, JsValue> {
        match self.dispatcher.poll_event_bytes().map_err(js_error)? {
            Some(event) => Ok(Uint8Array::from(event.as_slice()).into()),
            None => Ok(JsValue::NULL),
        }
    }

    /// Inspect a sidecar-written response for the internal resumable ACP marker.
    /// The TypeScript routing loop deliberately receives only the opaque process id.
    #[wasm_bindgen(js_name = pendingResponseProcessId)]
    pub fn pending_response_process_id(&self, frame: Uint8Array) -> Result<JsValue, JsValue> {
        match pending_frames::pending_process_id(&frame.to_vec()).map_err(js_error)? {
            Some(process_id) => Ok(JsValue::from_str(&process_id)),
            None => Ok(JsValue::NULL),
        }
    }

    /// Read the sidecar-owned timeout for the currently awaited ACP phase.
    #[wasm_bindgen(js_name = pendingResponseTimeoutMs)]
    pub fn pending_response_timeout_ms(&self, frame: Uint8Array) -> Result<JsValue, JsValue> {
        match pending_frames::pending_timeout_ms(&frame.to_vec()).map_err(js_error)? {
            Some(timeout_ms) => Ok(JsValue::from_f64(f64::from(timeout_ms))),
            None => Ok(JsValue::NULL),
        }
    }

    /// Read the stable sidecar phase identity associated with the timeout.
    #[wasm_bindgen(js_name = pendingResponseTimeoutPhase)]
    pub fn pending_response_timeout_phase(&self, frame: Uint8Array) -> Result<JsValue, JsValue> {
        match pending_frames::pending_timeout_phase(&frame.to_vec()).map_err(js_error)? {
            Some(phase) => Ok(JsValue::from_str(&phase)),
            None => Ok(JsValue::NULL),
        }
    }

    /// Build the next authenticated DeliverAgentOutput frame using the original
    /// response's exact ownership. ACP and outer-frame serialization stay in Rust.
    #[wasm_bindgen(js_name = buildDeliverAgentOutputFrame)]
    pub fn build_deliver_agent_output_frame(
        &mut self,
        origin_response: Uint8Array,
        process_id: String,
        chunk: Uint8Array,
    ) -> Result<JsValue, JsValue> {
        let request_id = self.next_internal_acp_request_id;
        self.next_internal_acp_request_id = request_id
            .checked_sub(1)
            .ok_or_else(|| js_error("browser ACP internal request id space exhausted"))?;
        let frame = pending_frames::deliver_agent_output_frame(
            &origin_response.to_vec(),
            request_id,
            process_id,
            chunk.to_vec(),
        )
        .map_err(js_error)?;
        Ok(Uint8Array::from(frame.as_slice()).into())
    }

    /// Build an authenticated stderr-delivery request. TypeScript forwards only
    /// opaque bytes; Rust owns event identity, limits, and serialization.
    #[wasm_bindgen(js_name = buildDeliverAgentStderrFrame)]
    pub fn build_deliver_agent_stderr_frame(
        &mut self,
        origin_response: Uint8Array,
        process_id: String,
        chunk: Uint8Array,
    ) -> Result<JsValue, JsValue> {
        let request_id = self.next_internal_acp_request_id;
        self.next_internal_acp_request_id = request_id
            .checked_sub(1)
            .ok_or_else(|| js_error("browser ACP internal request id space exhausted"))?;
        let frame = pending_frames::deliver_agent_stderr_frame(
            &origin_response.to_vec(),
            request_id,
            process_id,
            chunk.to_vec(),
        )
        .map_err(js_error)?;
        Ok(Uint8Array::from(frame.as_slice()).into())
    }

    /// Build an authenticated abort request using the originating frame's exact
    /// connection/session/VM ownership. Rust maps the observed terminal fact to
    /// the protocol enum; the sidecar core owns cleanup and the final error.
    #[wasm_bindgen(js_name = buildAbortPendingFrame)]
    pub fn build_abort_pending_frame(
        &mut self,
        origin_response: Uint8Array,
        process_id: String,
        reason: String,
        exit_code: Option<i32>,
    ) -> Result<JsValue, JsValue> {
        let request_id = self.next_internal_acp_request_id;
        self.next_internal_acp_request_id = request_id
            .checked_sub(1)
            .ok_or_else(|| js_error("browser ACP internal request id space exhausted"))?;
        let frame = pending_frames::abort_pending_frame_for_reason(
            &origin_response.to_vec(),
            request_id,
            process_id,
            &reason,
            exit_code,
        )
        .map_err(js_error)?;
        Ok(Uint8Array::from(frame.as_slice()).into())
    }

    /// Restore the originating request id, schema, and ownership on the final ACP
    /// response before the browser transport exposes it to the ordinary TS client.
    #[wasm_bindgen(js_name = restorePendingResponse)]
    pub fn restore_pending_response(
        &self,
        origin_response: Uint8Array,
        completed_response: Uint8Array,
    ) -> Result<JsValue, JsValue> {
        let frame = pending_frames::restore_origin_response(
            &origin_response.to_vec(),
            &completed_response.to_vec(),
        )
        .map_err(js_error)?;
        Ok(Uint8Array::from(frame.as_slice()).into())
    }
}

fn js_error(error: impl ToString) -> JsValue {
    JsError::new(&error.to_string()).into()
}
