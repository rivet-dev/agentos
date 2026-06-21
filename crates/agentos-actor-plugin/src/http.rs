//! HTTP preview proxy — plugin-side port of `rivetkit-agent-os::run::proxy_preview`.
//!
//! The host forwards a `RuntimeEvent::Http` as an `AbiEventTag::Http` event
//! whose payload is a CBOR [`HttpReqWire`]; the plugin replies (`reply_ok`) with
//! a CBOR [`HttpRespWire`]. These wire structs MUST stay field-identical to the
//! host's (`rivetkit-core::actor::native_plugin`) so CBOR round-trips.
//!
//! A `/preview/{token}/...` request resolves the token to a guest loopback port
//! (persisted in `agent_os_preview_tokens`) and forwards the remainder to that
//! port via [`AgentOs::fetch`]. An unmatched path, an unknown/expired token, or
//! a VM that is not up all reply `404` — which, like rivetkit's
//! `http.reply_status(404)`, is a successful HTTP response, not a reply error.

use std::collections::HashMap;
use std::io::Cursor;

use agent_os_client::AgentOs;
use bytes::Bytes;

use crate::actions::preview;
use crate::host_ctx::HostCtx;

/// HTTP request forwarded from the host (`Http` tag payload), CBOR-encoded.
/// Field-identical to the host's `HttpReqWire`.
#[derive(serde::Deserialize)]
struct HttpReqWire {
    method: String,
    uri: String,
    headers: HashMap<String, String>,
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
}

/// HTTP response the plugin returns in its reply, CBOR-encoded. Field-identical
/// to the host's `HttpRespWire`.
#[derive(serde::Serialize)]
struct HttpRespWire {
    status: u16,
    headers: HashMap<String, String>,
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
}

fn encode_resp(status: u16, headers: HashMap<String, String>, body: Vec<u8>) -> Vec<u8> {
    let wire = HttpRespWire {
        status,
        headers,
        body,
    };
    let mut out = Vec::new();
    let _ = ciborium::into_writer(&wire, &mut out);
    out
}

/// A bodyless response with `status` (the `/preview` 404 path + error cases).
fn status_response(status: u16) -> Vec<u8> {
    encode_resp(status, HashMap::new(), Vec::new())
}

/// Proxy a `/preview/{token}/...` request to the guest port the token was issued
/// for. Returns the CBOR `HttpRespWire` bytes to hand to `reply_ok`.
pub(crate) async fn proxy_preview(
    host: &HostCtx,
    vm: Option<&AgentOs>,
    req_bytes: &[u8],
) -> Vec<u8> {
    let req: HttpReqWire = match ciborium::from_reader(Cursor::new(req_bytes)) {
        Ok(req) => req,
        Err(_) => return status_response(400),
    };

    // The forwarded uri is request-target form; take just the path.
    let path = req
        .uri
        .parse::<http::Uri>()
        .map(|uri| uri.path().to_owned())
        .unwrap_or_else(|_| req.uri.clone());

    let Some(rest) = path.strip_prefix("/preview/") else {
        return status_response(404);
    };
    let (token, forward_path) = match rest.split_once('/') {
        Some((token, tail)) => (token.to_owned(), format!("/{tail}")),
        None => (rest.to_owned(), "/".to_owned()),
    };

    let port = match preview::resolve(host, &token).await {
        Ok(Some(port)) => port,
        _ => return status_response(404),
    };
    let Some(vm) = vm else {
        return status_response(404);
    };

    let mut builder = http::Request::builder()
        .method(req.method.as_str())
        .uri(&forward_path);
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let forwarded = match builder.body(Bytes::from(req.body)) {
        Ok(forwarded) => forwarded,
        Err(_) => return status_response(400),
    };

    match vm.fetch(port, forwarded).await {
        Ok(response) => {
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_owned(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect();
            let body = response.into_body().to_vec();
            encode_resp(status, headers, body)
        }
        Err(_) => status_response(502),
    }
}
