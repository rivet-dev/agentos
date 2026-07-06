use agentos_sidecar_client::wire::{
    self, AuthenticateRequest, ConnectionOwnership, OwnershipScope, ProtocolFrame, RequestFrame,
    RequestPayload, WireFrameCodec,
};
use agentos_sidecar_client::ProtocolCodecError;

#[test]
fn generated_wire_frame_codec_round_trips_authenticate() {
    let frame = authenticate_frame();
    let codec = WireFrameCodec::default();

    let encoded = codec.encode(&frame).expect("encode generated wire frame");
    let declared = u32::from_be_bytes(encoded[..4].try_into().expect("length prefix"));
    assert_eq!(declared as usize, encoded.len() - 4);

    let decoded = codec.decode(&encoded).expect("decode generated wire frame");

    assert_eq!(decoded, frame);
}

#[test]
fn generated_wire_frame_codec_rejects_oversized_payloads() {
    let frame = authenticate_frame();
    let codec = WireFrameCodec::new(8);

    let error = codec.encode(&frame).expect_err("oversized frame must fail");

    assert!(matches!(
        error,
        ProtocolCodecError::FrameTooLarge { size, max: 8 } if size > 8
    ));
}

#[test]
fn generated_wire_frame_codec_rejects_schema_mismatches() {
    let mut frame = authenticate_frame();
    let ProtocolFrame::RequestFrame(request) = &mut frame else {
        unreachable!("authenticate frame is a request")
    };
    request.schema.version = wire::PROTOCOL_VERSION + 1;

    let error = WireFrameCodec::default()
        .encode(&frame)
        .expect_err("schema mismatch must fail");

    assert!(matches!(
        error,
        ProtocolCodecError::UnsupportedSchema { version, .. } if version == wire::PROTOCOL_VERSION + 1
    ));
}

fn authenticate_frame() -> ProtocolFrame {
    ProtocolFrame::RequestFrame(RequestFrame {
        schema: wire::protocol_schema(),
        request_id: 1,
        ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
            connection_id: "conn-1".to_string(),
        }),
        payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
            client_name: "agentos-sidecar-client-test".to_string(),
            auth_token: "token".to_string(),
            protocol_version: wire::PROTOCOL_VERSION,
            bridge_version: 1,
        }),
    })
}
