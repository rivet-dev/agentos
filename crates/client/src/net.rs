//! Port-based virtual networking (`fetch`).
//!
//! Ported from `packages/core/src/agent-os.ts` `fetch`. Dispatches to a guest server on `port`
//! inside the kernel, never the host. The request URL host is discarded (only `pathname`+`search`
//! are used); the body is only attached for non-GET/HEAD methods; the response body is base64-decoded.
//! Fully buffered both directions. Wire path is the existing `VmFetch` request/response.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use bytes::Bytes;
use serde::Deserialize;

use agentos_sidecar_client::wire;

use crate::agent_os::AgentOs;
use crate::error::ClientError;

/// Maximum fully buffered fetch component size. `VmFetch` is a single request/response frame, so
/// keeping this at the default frame size prevents fetch-specific buffers from growing just because
/// a sidecar was configured with a larger transport frame limit for another API.
const VM_FETCH_BUFFER_LIMIT_BYTES: usize = agentos_sidecar_client::wire::DEFAULT_MAX_FRAME_BYTES;

/// The shape of the JSON string returned in [`VmFetchResponse::response_json`], mirroring the TS
/// `{ status, statusText?, headers?: [k,v][], body?: base64 }` payload.
#[derive(Debug, Deserialize)]
struct VmFetchResponsePayload {
    status: u16,
    #[serde(rename = "statusText", default)]
    status_text: Option<String>,
    #[serde(default)]
    headers: Option<Vec<(String, String)>>,
    /// Base64-encoded response body.
    #[serde(default)]
    body: Option<String>,
}

impl AgentOs {
    /// Fetch from a guest server listening on `port` inside the VM.
    ///
    /// `path` is derived from the request URI's `pathname`+`search`; the host is ignored. The body
    /// is only sent for methods other than GET/HEAD. The response body is base64-decoded.
    pub async fn fetch(
        &self,
        port: u16,
        request: http::Request<Bytes>,
    ) -> Result<http::Response<Bytes>> {
        let buffer_limit = self.fetch_buffer_limit();
        let (parts, body) = request.into_parts();

        // Only `pathname`+`search` are carried on the wire; the host/authority is discarded, matching
        // the TS `${url.pathname}${url.search}`. A missing path defaults to "/".
        let path = match parts.uri.path_and_query() {
            Some(pq) => {
                ensure_fetch_component_within_limit(
                    "fetch request path",
                    pq.as_str().len(),
                    buffer_limit,
                )?;
                pq.as_str().to_owned()
            }
            None => "/".to_owned(),
        };

        let method = parts.method.as_str().to_owned();

        // Headers serialized as a JSON object (TS `Object.fromEntries(headers.entries())`). A repeated
        // header name keeps the last value, matching JS object semantics where later keys overwrite.
        let mut header_map: BTreeMap<String, String> = BTreeMap::new();
        let mut raw_header_bytes = 0usize;
        for (name, value) in parts.headers.iter() {
            raw_header_bytes = raw_header_bytes
                .saturating_add(name.as_str().len())
                .saturating_add(value.as_bytes().len());
            header_map.insert(
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            );
        }
        ensure_fetch_component_within_limit(
            "fetch request headers",
            raw_header_bytes,
            buffer_limit,
        )?;
        let headers_json =
            serde_json::to_string(&header_map).context("serializing fetch request headers")?;
        ensure_fetch_component_within_limit(
            "fetch request headers json",
            headers_json.len(),
            buffer_limit,
        )?;

        // Body is only attached for methods other than GET/HEAD (TS `request.method !== "GET" && ...`).
        let wire_body = if method == "GET" || method == "HEAD" {
            None
        } else {
            ensure_fetch_component_within_limit("fetch request body", body.len(), buffer_limit)?;
            let body = String::from_utf8_lossy(&body).into_owned();
            ensure_fetch_component_within_limit(
                "fetch request body text",
                body.len(),
                buffer_limit,
            )?;
            Some(body)
        };
        ensure_fetch_request_payload_within_limit(
            &method,
            &path,
            &headers_json,
            wire_body.as_deref(),
            buffer_limit,
        )?;

        let response = self
            .transport()
            .request_wire_bounded(
                self.vm_fetch_ownership(),
                wire::RequestPayload::VmFetchRequest(wire::VmFetchRequest {
                    port,
                    method,
                    path,
                    headers_json,
                    body: wire_body,
                }),
                buffer_limit,
            )
            .await?;

        let response_json = match response {
            wire::ResponsePayload::VmFetchResponse(result) => result.response_json,
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                return Err(ClientError::Kernel { code, message }.into());
            }
            other => {
                return Err(
                    ClientError::Sidecar(format!("fetch: unexpected response {other:?}")).into(),
                );
            }
        };
        ensure_fetch_component_within_limit(
            "fetch response json",
            response_json.len(),
            buffer_limit,
        )?;

        let payload: VmFetchResponsePayload =
            serde_json::from_str(&response_json).context("parsing vm_fetch response json")?;

        // Base64-decode the response body (TS `Buffer.from(body ?? "", "base64")`). An absent body is
        // an empty body.
        let decoded_body = match payload.body {
            Some(encoded) => {
                ensure_fetch_base64_body_within_limit(&encoded, buffer_limit)?;
                Bytes::from(
                    BASE64
                        .decode(encoded.as_bytes())
                        .context("decoding base64 fetch response body")?,
                )
            }
            None => Bytes::new(),
        };

        let status = http::StatusCode::from_u16(payload.status)
            .context("fetch: invalid response status code")?;

        let mut builder = http::Response::builder().status(status);
        for (key, value) in payload.headers.unwrap_or_default() {
            builder = builder.header(key, value);
        }

        let mut http_response = builder
            .body(decoded_body)
            .context("building fetch response")?;

        // `statusText` has no slot in `http::Response`; carry it on the extensions so a caller can
        // recover it, matching the TS `Response.statusText`.
        if let Some(status_text) = payload.status_text {
            http_response
                .extensions_mut()
                .insert(FetchStatusText(status_text));
        }

        Ok(http_response)
    }

    /// The VM-scoped ownership used for the `VmFetch` wire request.
    fn vm_fetch_ownership(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
    }

    fn fetch_buffer_limit(&self) -> usize {
        self.transport()
            .max_frame_bytes()
            .min(VM_FETCH_BUFFER_LIMIT_BYTES)
    }
}

/// The wire `statusText`, stashed in [`http::Response`] extensions so callers can recover the TS
/// `Response.statusText` value (the `http` crate has no dedicated status-text field).
#[derive(Debug, Clone)]
pub struct FetchStatusText(pub String);

fn ensure_fetch_component_within_limit(
    component: &str,
    size: usize,
    limit: usize,
) -> Result<(), ClientError> {
    if size > limit {
        return Err(ClientError::Sidecar(format!(
            "{component} is {size} bytes, limit is {limit}"
        )));
    }
    Ok(())
}

fn ensure_fetch_base64_body_within_limit(encoded: &str, limit: usize) -> Result<(), ClientError> {
    ensure_fetch_component_within_limit("fetch response body base64", encoded.len(), limit)?;
    ensure_fetch_component_within_limit(
        "fetch response body",
        base64_decoded_upper_bound(encoded.len()),
        limit,
    )
}

fn ensure_fetch_request_payload_within_limit(
    method: &str,
    path: &str,
    headers_json: &str,
    body: Option<&str>,
    limit: usize,
) -> Result<(), ClientError> {
    let size = method
        .len()
        .saturating_add(path.len())
        .saturating_add(headers_json.len())
        .saturating_add(body.map(str::len).unwrap_or_default());
    ensure_fetch_component_within_limit("fetch request payload", size, limit)
}

fn base64_decoded_upper_bound(encoded_len: usize) -> usize {
    encoded_len.saturating_add(3) / 4 * 3
}

#[cfg(test)]
mod tests {
    use super::{
        base64_decoded_upper_bound, ensure_fetch_base64_body_within_limit,
        ensure_fetch_component_within_limit, ensure_fetch_request_payload_within_limit,
        VM_FETCH_BUFFER_LIMIT_BYTES,
    };

    #[test]
    fn fetch_component_limit_rejects_oversized_buffers() {
        assert!(ensure_fetch_component_within_limit("component", 8, 8).is_ok());

        let error =
            ensure_fetch_component_within_limit("component", 9, 8).expect_err("limit violation");
        assert!(
            error.to_string().contains("component is 9 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn fetch_component_limit_rejects_expanded_request_text() {
        let replacement = String::from_utf8_lossy(&[0xff]).into_owned();
        assert_eq!(replacement.len(), 3);

        let error = ensure_fetch_component_within_limit("fetch request body text", 3, 2)
            .expect_err("expanded body text should exceed limit");
        assert!(
            error
                .to_string()
                .contains("fetch request body text is 3 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn fetch_request_payload_limit_rejects_aggregate_oversize() {
        let error =
            ensure_fetch_request_payload_within_limit("POST", "/abc", "{}", Some("body"), 8)
                .expect_err("aggregate request payload should exceed limit");
        assert!(
            error
                .to_string()
                .contains("fetch request payload is 14 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn fetch_base64_guard_bounds_decoded_response_size() {
        assert_eq!(base64_decoded_upper_bound(4), 3);
        assert!(ensure_fetch_base64_body_within_limit("AAAA", 4).is_ok());

        let error = ensure_fetch_base64_body_within_limit("AAAA", 2)
            .expect_err("encoded body should exceed limit");
        assert!(
            error
                .to_string()
                .contains("fetch response body base64 is 4 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn fetch_buffer_limit_is_fixed_to_default_frame_size() {
        assert_eq!(
            VM_FETCH_BUFFER_LIMIT_BYTES,
            agentos_sidecar_client::wire::DEFAULT_MAX_FRAME_BYTES
        );
    }

    // ── Security: AOSCLIENT-P3-fetch (N-010 guest-server VmFetch response) ───────────────────────
    //
    // Threat: a guest server controls the `VmFetch` RESPONSE JSON that the client parses. A hostile
    // server returns an out-of-range status (70000 / 0), a malformed base64 body ("!!!"), or an
    // over-limit base64 body. Each must be handled as a clean `Err` on the client — never a panic
    // (a panic in the shared host process is cross-tenant DoS, F.4). This is a regression guard for
    // the parse path in `AgentOs::fetch`: `serde_json::from_str` -> `StatusCode::from_u16` ->
    // `ensure_fetch_base64_body_within_limit` -> `BASE64.decode`.
    use super::{VmFetchResponsePayload, BASE64};
    use base64::Engine as _;

    /// A status that overflows u16 (70000) must fail JSON deserialization of the response payload,
    /// not panic. `status` is typed `u16`, so serde rejects the out-of-range value.
    #[test]
    fn vm_fetch_response_overflowing_status_fails_deserialization_without_panic() {
        let json = r#"{"status":70000}"#;
        let parsed: Result<VmFetchResponsePayload, _> = serde_json::from_str(json);
        assert!(
            parsed.is_err(),
            "AOSCLIENT-P3-fetch: status 70000 overflows u16 and must fail to deserialize, not panic"
        );
    }

    /// A status of 0 deserializes (it is a valid u16) but must be rejected by
    /// `http::StatusCode::from_u16`, mirroring the `fetch` status construction, without panic.
    #[test]
    fn vm_fetch_response_zero_status_is_rejected_by_status_code_without_panic() {
        let json = r#"{"status":0}"#;
        let payload: VmFetchResponsePayload =
            serde_json::from_str(json).expect("status 0 is a valid u16 and should deserialize");
        let status = http::StatusCode::from_u16(payload.status);
        assert!(
            status.is_err(),
            "AOSCLIENT-P3-fetch: status code 0 must be rejected by StatusCode::from_u16, not panic"
        );
    }

    /// A malformed base64 body ("!!!") must produce a decode `Err`, never a panic.
    #[test]
    fn vm_fetch_response_malformed_base64_body_errors_without_panic() {
        // First the size guard passes for a tiny body, so we reach the decode step the way
        // `fetch` does.
        ensure_fetch_base64_body_within_limit("!!!", VM_FETCH_BUFFER_LIMIT_BYTES)
            .expect("tiny body is within the limit");
        let decoded = BASE64.decode("!!!".as_bytes());
        assert!(
            decoded.is_err(),
            "AOSCLIENT-P3-fetch: malformed base64 response body \"!!!\" must error on decode, not panic"
        );
    }

    /// An over-limit base64 body must be rejected by the size guard BEFORE any allocation/decode,
    /// without panic.
    #[test]
    fn vm_fetch_response_over_limit_base64_body_is_rejected_before_decode() {
        // An encoded length strictly greater than the limit trips the guard on the encoded size.
        let oversized = "A".repeat(VM_FETCH_BUFFER_LIMIT_BYTES + 4);
        let result = ensure_fetch_base64_body_within_limit(&oversized, VM_FETCH_BUFFER_LIMIT_BYTES);
        let error = result.expect_err(
            "AOSCLIENT-P3-fetch: an over-limit base64 body must be rejected before decode",
        );
        assert!(
            error.to_string().contains("fetch response body base64"),
            "AOSCLIENT-P3-fetch: over-limit base64 body must be rejected by the size guard, got: {error}"
        );
    }
}
