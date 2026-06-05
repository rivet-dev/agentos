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

use agent_os_sidecar::protocol::{
    OwnershipScope, RejectedResponse, RequestPayload, ResponsePayload, VmFetchRequest,
};

use crate::agent_os::AgentOs;
use crate::error::ClientError;

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
        let (parts, body) = request.into_parts();

        // Only `pathname`+`search` are carried on the wire; the host/authority is discarded, matching
        // the TS `${url.pathname}${url.search}`. A missing path defaults to "/".
        let path = match parts.uri.path_and_query() {
            Some(pq) => pq.as_str().to_owned(),
            None => "/".to_owned(),
        };

        let method = parts.method.as_str().to_owned();

        // Headers serialized as a JSON object (TS `Object.fromEntries(headers.entries())`). A repeated
        // header name keeps the last value, matching JS object semantics where later keys overwrite.
        let mut header_map: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in parts.headers.iter() {
            header_map.insert(
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            );
        }
        let headers_json =
            serde_json::to_string(&header_map).context("serializing fetch request headers")?;

        // Body is only attached for methods other than GET/HEAD (TS `request.method !== "GET" && ...`).
        let wire_body = if method == "GET" || method == "HEAD" {
            None
        } else {
            Some(String::from_utf8_lossy(&body).into_owned())
        };

        let response = self
            .transport()
            .request(
                self.vm_fetch_ownership(),
                RequestPayload::VmFetch(VmFetchRequest {
                    port,
                    method,
                    path,
                    headers_json,
                    body: wire_body,
                }),
            )
            .await?;

        let response_json = match response {
            ResponsePayload::VmFetchResult(result) => result.response_json,
            ResponsePayload::Rejected(RejectedResponse { code, message }) => {
                return Err(ClientError::Kernel { code, message }.into());
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "fetch: unexpected response {other:?}"
                ))
                .into());
            }
        };

        let payload: VmFetchResponsePayload =
            serde_json::from_str(&response_json).context("parsing vm_fetch response json")?;

        // Base64-decode the response body (TS `Buffer.from(body ?? "", "base64")`). An absent body is
        // an empty body.
        let decoded_body = match payload.body {
            Some(encoded) => Bytes::from(
                BASE64
                    .decode(encoded.as_bytes())
                    .context("decoding base64 fetch response body")?,
            ),
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
            http_response.extensions_mut().insert(FetchStatusText(status_text));
        }

        Ok(http_response)
    }

    /// The VM-scoped ownership used for the `VmFetch` wire request.
    fn vm_fetch_ownership(&self) -> OwnershipScope {
        OwnershipScope::vm(self.connection_id(), self.wire_session_id(), self.vm_id())
    }
}

/// The wire `statusText`, stashed in [`http::Response`] extensions so callers can recover the TS
/// `Response.statusText` value (the `http` crate has no dedicated status-text field).
#[derive(Debug, Clone)]
pub struct FetchStatusText(pub String);
