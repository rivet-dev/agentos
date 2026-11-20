#[test]
fn fetch_stream_bounds_accept_exact_padded_decoded_bytes() {
    for bytes in [
        vec![],
        vec![255],
        vec![0, 255],
        b"host".to_vec(),
        b"hosts".to_vec(),
    ] {
        let encoded = BASE64.encode(&bytes);
        let json = serde_json::json!({"body": encoded, "done": true}).to_string();
        let result = super::decode_fetch_stream_chunk(&json, bytes.len().max(1), 128)
            .expect("valid decoded payload within requested bound");
        assert_eq!(result.body, bytes);
        assert_eq!(
            result.body.capacity(),
            bytes.len(),
            "decode allocation exceeds exact payload length"
        );
        assert!(result.done);
    }
}

#[test]
fn fetch_stream_bounds_reject_decoded_oversize_and_malformed_data() {
    for (encoded, max) in [
        ("aG9zdHM=", 4),
        ("!!!!", 4),
        ("Zg=A", 4),
        ("Zg===", 4),
        ("Zh==", 1),
        ("Zg", 1),
    ] {
        let json = serde_json::json!({"body": encoded, "done": false}).to_string();
        assert!(
            super::decode_fetch_stream_chunk(&json, max, 128).is_err(),
            "accepted {encoded}"
        );
    }
}

#[test]
fn fetch_stream_bounds_enforce_response_frame_before_parsing_or_decoding() {
    let json = serde_json::json!({"body": "", "done": true, "extra": "x".repeat(64)}).to_string();
    let error = super::decode_fetch_stream_chunk(&json, 4, 32)
        .expect_err("response frame must stay bounded");
    assert!(
        error.to_string().contains("fetch stream response json"),
        "{error}"
    );
    let invalid = "!".repeat(33);
    let error = super::decode_fetch_stream_chunk(&invalid, usize::MAX, 32)
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("fetch stream response json"),
        "{error}"
    );
}

#[test]
fn fetch_stream_bounds_report_decoded_limit_without_padding_overhead() {
    let json = serde_json::json!({"body": "aG9zdHM=", "done": false}).to_string();
    let error = super::decode_fetch_stream_chunk(&json, 4, 128)
        .err()
        .unwrap();
    assert!(matches!(
        error.downcast_ref::<crate::ClientError>(),
        Some(crate::ClientError::Sidecar(_))
    ));
    assert!(
        error
            .to_string()
            .contains("fetch stream response body is 5 bytes, limit is 4"),
        "{error}"
    );
}
