//! Sidecar-owned wire helpers for the browser's resumable ACP routing loop.
//!
//! TypeScript treats the outer frame and ACP payload as opaque bytes. This module
//! owns decoding `AcpPendingResponse`, constructing authenticated
//! `AcpDeliverAgentOutputRequest` frames with the original ownership, and restoring
//! the originating request id on the completed response.

use agentos_native_sidecar_browser::wire_dispatch::BROWSER_MAX_FRAME_BYTES;
use agentos_protocol::generated::v1::{
    AcpAbortPendingRequest, AcpDeliverAgentOutputRequest, AcpDeliverAgentStderrRequest,
    AcpPendingAbortReason, AcpPendingResponse, AcpRequest, AcpResponse,
};
use agentos_sidecar_protocol::wire::{
    ExtEnvelope, ProtocolFrame, RequestFrame, RequestPayload, ResponseFrame, ResponsePayload,
    WireFrameCodec,
};

const ACP_NAMESPACE: &str = agentos_protocol::ACP_EXTENSION_NAMESPACE;

pub(crate) fn pending_process_id(bytes: &[u8]) -> Result<Option<String>, String> {
    let response = decode_response_frame(bytes)?;
    let ResponsePayload::ExtEnvelope(envelope) = response.payload else {
        return Ok(None);
    };
    if envelope.namespace != ACP_NAMESPACE {
        return Ok(None);
    }
    let response: AcpResponse = serde_bare::from_slice(&envelope.payload)
        .map_err(|error| format!("invalid ACP response: {error}"))?;
    Ok(match response {
        AcpResponse::AcpPendingResponse(AcpPendingResponse { process_id, .. }) => Some(process_id),
        _ => None,
    })
}

pub(crate) fn deliver_agent_stderr_frame(
    origin_response: &[u8],
    internal_request_id: i64,
    process_id: String,
    chunk: Vec<u8>,
) -> Result<Vec<u8>, String> {
    internal_request_frame(
        origin_response,
        internal_request_id,
        AcpRequest::AcpDeliverAgentStderrRequest(AcpDeliverAgentStderrRequest {
            process_id,
            chunk,
        }),
    )
}

pub(crate) fn pending_timeout_ms(bytes: &[u8]) -> Result<Option<u32>, String> {
    let response = decode_response_frame(bytes)?;
    let ResponsePayload::ExtEnvelope(envelope) = response.payload else {
        return Ok(None);
    };
    if envelope.namespace != ACP_NAMESPACE {
        return Ok(None);
    }
    let response: AcpResponse = serde_bare::from_slice(&envelope.payload)
        .map_err(|error| format!("invalid ACP response: {error}"))?;
    Ok(match response {
        AcpResponse::AcpPendingResponse(AcpPendingResponse { timeout_ms, .. }) => Some(timeout_ms),
        _ => None,
    })
}

pub(crate) fn pending_timeout_phase(bytes: &[u8]) -> Result<Option<String>, String> {
    let response = decode_response_frame(bytes)?;
    let ResponsePayload::ExtEnvelope(envelope) = response.payload else {
        return Ok(None);
    };
    if envelope.namespace != ACP_NAMESPACE {
        return Ok(None);
    }
    let response: AcpResponse = serde_bare::from_slice(&envelope.payload)
        .map_err(|error| format!("invalid ACP response: {error}"))?;
    Ok(match response {
        AcpResponse::AcpPendingResponse(AcpPendingResponse { timeout_phase, .. }) => {
            Some(timeout_phase)
        }
        _ => None,
    })
}

pub(crate) fn deliver_agent_output_frame(
    origin_response: &[u8],
    internal_request_id: i64,
    process_id: String,
    chunk: Vec<u8>,
) -> Result<Vec<u8>, String> {
    internal_request_frame(
        origin_response,
        internal_request_id,
        AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
            process_id,
            chunk,
        }),
    )
}

pub(crate) fn abort_pending_frame(
    origin_response: &[u8],
    internal_request_id: i64,
    process_id: String,
    reason: AcpPendingAbortReason,
    exit_code: Option<i32>,
) -> Result<Vec<u8>, String> {
    internal_request_frame(
        origin_response,
        internal_request_id,
        AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
            process_id,
            reason,
            exit_code,
        }),
    )
}

pub(crate) fn abort_pending_frame_for_reason(
    origin_response: &[u8],
    internal_request_id: i64,
    process_id: String,
    reason: &str,
    exit_code: Option<i32>,
) -> Result<Vec<u8>, String> {
    let reason = match reason {
        "agent_exited" => AcpPendingAbortReason::AgentExited,
        "interaction_timeout" => AcpPendingAbortReason::InteractionTimeout,
        "driver_failed" => AcpPendingAbortReason::DriverFailed,
        "caller_cancelled" => AcpPendingAbortReason::CallerCancelled,
        _ => return Err(format!("invalid ACP abort reason: {reason}")),
    };
    abort_pending_frame(
        origin_response,
        internal_request_id,
        process_id,
        reason,
        exit_code,
    )
}

fn internal_request_frame(
    origin_response: &[u8],
    internal_request_id: i64,
    request: AcpRequest,
) -> Result<Vec<u8>, String> {
    let origin = decode_response_frame(origin_response)?;
    let payload = serde_bare::to_vec(&request)
        .map_err(|error| format!("failed to encode internal ACP request: {error}"))?;
    encode_frame(ProtocolFrame::RequestFrame(RequestFrame {
        schema: origin.schema,
        request_id: internal_request_id,
        ownership: origin.ownership,
        payload: RequestPayload::ExtEnvelope(ExtEnvelope {
            namespace: ACP_NAMESPACE.to_string(),
            payload,
        }),
    }))
}

pub(crate) fn restore_origin_response(
    origin_response: &[u8],
    completed_response: &[u8],
) -> Result<Vec<u8>, String> {
    let origin = decode_response_frame(origin_response)?;
    let mut completed = decode_response_frame(completed_response)?;
    let ResponsePayload::ExtEnvelope(envelope) = &completed.payload else {
        return Err(String::from(
            "ACP delivery returned a non-extension response",
        ));
    };
    if envelope.namespace != ACP_NAMESPACE {
        return Err(String::from("ACP delivery returned a non-ACP response"));
    }
    completed.schema = origin.schema;
    completed.request_id = origin.request_id;
    completed.ownership = origin.ownership;
    encode_frame(ProtocolFrame::ResponseFrame(completed))
}

fn decode_response_frame(bytes: &[u8]) -> Result<ResponseFrame, String> {
    let codec = WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES);
    match codec
        .decode_message(bytes)
        .map_err(|error| error.to_string())?
    {
        ProtocolFrame::ResponseFrame(response) => Ok(response),
        _ => Err(String::from("expected browser sidecar response frame")),
    }
}

fn encode_frame(frame: ProtocolFrame) -> Result<Vec<u8>, String> {
    WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES)
        .encode_message(&frame)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_protocol::generated::v1::{AcpSessionCreatedResponse, AcpSessionResumedResponse};
    use agentos_sidecar_protocol::wire::{OwnershipScope, ProtocolSchema, VmOwnership};

    fn ownership(name: &str) -> OwnershipScope {
        OwnershipScope::VmOwnership(VmOwnership {
            connection_id: name.to_string(),
            session_id: format!("{name}-session"),
            vm_id: format!("{name}-vm"),
        })
    }

    #[test]
    fn builds_abort_with_exact_origin_ownership_and_sidecar_reason() {
        let origin_ownership = ownership("connection-a");
        let origin = response_bytes(
            42,
            origin_ownership.clone(),
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                process_id: String::from("acp-agent-7"),
                timeout_ms: 10_000,
                timeout_phase: String::from("session/prompt"),
            }),
        );
        let abort = abort_pending_frame(
            &origin,
            i64::MAX - 11,
            String::from("acp-agent-7"),
            AcpPendingAbortReason::InteractionTimeout,
            None,
        )
        .expect("build abort");
        let ProtocolFrame::RequestFrame(abort) = WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES)
            .decode_message(&abort)
            .expect("decode abort")
        else {
            panic!("expected request frame");
        };
        assert_eq!(abort.request_id, i64::MAX - 11);
        assert_eq!(abort.ownership, origin_ownership);
        let RequestPayload::ExtEnvelope(envelope) = abort.payload else {
            panic!("expected extension request");
        };
        let request: AcpRequest =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP abort");
        assert!(matches!(
            request,
            AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                process_id,
                reason: AcpPendingAbortReason::InteractionTimeout,
                exit_code: None,
            }) if process_id == "acp-agent-7"
        ));
    }

    #[test]
    fn builds_driver_failed_abort_from_the_browser_driver_reason() {
        let origin = response_bytes(
            42,
            ownership("connection-a"),
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                process_id: String::from("acp-agent-7"),
                timeout_ms: 10_000,
                timeout_phase: String::from("session/prompt"),
            }),
        );
        let abort = abort_pending_frame_for_reason(
            &origin,
            i64::MAX - 12,
            String::from("acp-agent-7"),
            "driver_failed",
            None,
        )
        .expect("build driver-failed abort");
        let ProtocolFrame::RequestFrame(abort) = WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES)
            .decode_message(&abort)
            .expect("decode abort")
        else {
            panic!("expected request frame");
        };
        let RequestPayload::ExtEnvelope(envelope) = abort.payload else {
            panic!("expected extension request");
        };
        let request: AcpRequest =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP abort");
        assert!(matches!(
            request,
            AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                reason: AcpPendingAbortReason::DriverFailed,
                ..
            })
        ));
    }

    fn response_bytes(
        request_id: i64,
        ownership: OwnershipScope,
        response: AcpResponse,
    ) -> Vec<u8> {
        let payload = serde_bare::to_vec(&response).expect("encode ACP response");
        encode_frame(ProtocolFrame::ResponseFrame(ResponseFrame {
            schema: ProtocolSchema {
                name: String::from("agentos-native-sidecar"),
                version: 7,
            },
            request_id,
            ownership,
            payload: ResponsePayload::ExtEnvelope(ExtEnvelope {
                namespace: ACP_NAMESPACE.to_string(),
                payload,
            }),
        }))
        .expect("encode response")
    }

    #[test]
    fn inspects_pending_and_builds_owned_delivery_without_ts_wire_constants() {
        let origin_ownership = ownership("connection-a");
        let origin = response_bytes(
            42,
            origin_ownership.clone(),
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                process_id: String::from("acp-agent-7"),
                timeout_ms: 30_000,
                timeout_phase: String::from("create.initialize"),
            }),
        );
        assert_eq!(
            pending_process_id(&origin).expect("inspect pending"),
            Some(String::from("acp-agent-7"))
        );
        assert_eq!(
            pending_timeout_ms(&origin).expect("inspect timeout"),
            Some(30_000)
        );
        assert_eq!(
            pending_timeout_phase(&origin).expect("inspect timeout phase"),
            Some(String::from("create.initialize"))
        );

        let delivery = deliver_agent_output_frame(
            &origin,
            i64::MAX - 10,
            String::from("acp-agent-7"),
            b"agent output\n".to_vec(),
        )
        .expect("build delivery");
        let codec = WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES);
        let ProtocolFrame::RequestFrame(delivery) =
            codec.decode_message(&delivery).expect("decode delivery")
        else {
            panic!("expected request frame");
        };
        assert_eq!(delivery.request_id, i64::MAX - 10);
        assert_eq!(delivery.ownership, origin_ownership);
        let RequestPayload::ExtEnvelope(envelope) = delivery.payload else {
            panic!("expected extension request");
        };
        let request: AcpRequest =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP delivery");
        let AcpRequest::AcpDeliverAgentOutputRequest(request) = request else {
            panic!("expected output delivery");
        };
        assert_eq!(request.process_id, "acp-agent-7");
        assert_eq!(request.chunk, b"agent output\n");

        let stderr_delivery = deliver_agent_stderr_frame(
            &origin,
            i64::MAX - 9,
            String::from("acp-agent-7"),
            b"adapter diagnostic\n".to_vec(),
        )
        .expect("build stderr delivery");
        let ProtocolFrame::RequestFrame(stderr_delivery) = codec
            .decode_message(&stderr_delivery)
            .expect("decode stderr delivery")
        else {
            panic!("expected stderr request frame");
        };
        assert_eq!(stderr_delivery.request_id, i64::MAX - 9);
        assert_eq!(stderr_delivery.ownership, ownership("connection-a"));
        let RequestPayload::ExtEnvelope(envelope) = stderr_delivery.payload else {
            panic!("expected extension request");
        };
        let request: AcpRequest =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP stderr delivery");
        let AcpRequest::AcpDeliverAgentStderrRequest(request) = request else {
            panic!("expected stderr delivery");
        };
        assert_eq!(request.process_id, "acp-agent-7");
        assert_eq!(request.chunk, b"adapter diagnostic\n");
    }

    #[test]
    fn restores_origin_id_and_ownership_on_completed_response() {
        let origin = response_bytes(
            42,
            ownership("origin"),
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                process_id: String::from("acp-agent-7"),
                timeout_ms: 600_000,
                timeout_phase: String::from("session/prompt"),
            }),
        );
        let completed = response_bytes(
            i64::MAX - 10,
            ownership("internal"),
            AcpResponse::AcpSessionResumedResponse(AcpSessionResumedResponse {
                session_id: String::from("session-1"),
                mode: String::from("native"),
                agent_type: String::from("echo"),
                process_id: String::from("acp-agent-7"),
                pid: None,
            }),
        );
        let restored = restore_origin_response(&origin, &completed).expect("restore response");
        let restored = decode_response_frame(&restored).expect("decode restored response");
        assert_eq!(restored.request_id, 42);
        assert_eq!(restored.ownership, ownership("origin"));
        let ResponsePayload::ExtEnvelope(envelope) = restored.payload else {
            panic!("expected ACP response");
        };
        let response: AcpResponse =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP response");
        assert!(matches!(
            response,
            AcpResponse::AcpSessionResumedResponse(AcpSessionResumedResponse { session_id, .. })
                if session_id == "session-1"
        ));

        let non_pending = response_bytes(
            43,
            ownership("origin"),
            AcpResponse::AcpSessionCreatedResponse(AcpSessionCreatedResponse {
                session_id: String::from("session-2"),
                agent_type: String::from("echo"),
                process_id: String::from("acp-agent-8"),
                pid: None,
                modes: None,
                config_options: Vec::new(),
                agent_capabilities: None,
                agent_info: None,
            }),
        );
        assert_eq!(
            pending_process_id(&non_pending).expect("inspect final"),
            None
        );
    }
}
