//! Transport-safe networking and preview action DTOs.

use std::collections::BTreeMap;

use agentos_client::{HttpRequest, HttpResponse, HttpStreamChunk, HttpStreamHead};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::filesystem::FileBytes;

pub use agentos_client::HttpRequest as ActorHttpRequest;

pub const MAX_HTTP_PATH_BYTES: usize = 16 * 1024;
pub const MAX_HTTP_METHOD_BYTES: usize = 32;
pub const MAX_HTTP_HEADERS: usize = 128;
pub const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 512 * 1024;
pub const MAX_HTTP_RESPONSE_BYTES: usize = 768 * 1024;
pub const MAX_STREAM_CHUNK_BYTES: u32 = 128 * 1024;
pub const DEFAULT_STREAM_CHUNK_BYTES: u32 = 64 * 1024;
pub const MAX_STREAM_ID_BYTES: usize = 256;
pub const MAX_STREAM_LIFETIME_MS: i64 = 60 * 60 * 1_000;
pub const MAX_PREVIEW_TOKEN_BYTES: usize = 128;

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorHttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: FileBytes,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorFetchStreamId {
    pub generation: u64,
    pub stream_id: String,
    pub expires_at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorFetchStreamHead {
    pub stream: ActorFetchStreamId,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorFetchStreamChunk {
    pub body: FileBytes,
    pub done: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetch {
    pub request: ActorHttpRequest,
}

action!(NetworkFetch => ActorHttpResponse, "network.fetch");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamStart {
    pub request: ActorHttpRequest,
}

action!(NetworkFetchStreamStart => ActorFetchStreamHead, "network.fetchStream.start");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamRead {
    pub stream: ActorFetchStreamId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
}

action!(NetworkFetchStreamRead => ActorFetchStreamChunk, "network.fetchStream.read");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamCancel {
    pub stream: ActorFetchStreamId,
}

action!(NetworkFetchStreamCancel => (), "network.fetchStream.cancel");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkPreviewCreate {
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

action!(NetworkPreviewCreate => ActorPreview, "network.preview.create");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorPreview {
    pub token: String,
    pub path: String,
    pub port: u16,
    pub expires_at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkPreviewExpire {
    pub token: String,
}

action!(NetworkPreviewExpire => bool, "network.preview.expire");

pub fn actor_http_response(response: HttpResponse) -> Result<ActorHttpResponse> {
    let header_bytes = response
        .headers
        .iter()
        .fold(0usize, |total, (name, value)| {
            total.saturating_add(name.len()).saturating_add(value.len())
        });
    validate_limit("HTTP response headers", header_bytes, MAX_HTTP_HEADER_BYTES)?;
    validate_limit(
        "HTTP response body",
        response.body.len(),
        MAX_HTTP_RESPONSE_BYTES,
    )?;
    Ok(ActorHttpResponse {
        status: response.status,
        status_text: response.status_text,
        headers: response.headers,
        body: FileBytes(response.body),
    })
}

pub fn actor_stream_head(
    generation: u64,
    now_ms: i64,
    head: HttpStreamHead,
) -> Result<ActorFetchStreamHead> {
    validate_nonempty_bytes("fetch stream id", &head.stream_id, MAX_STREAM_ID_BYTES)?;
    let header_bytes = head.headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    validate_limit("HTTP response headers", header_bytes, MAX_HTTP_HEADER_BYTES)?;
    Ok(ActorFetchStreamHead {
        stream: ActorFetchStreamId {
            generation,
            stream_id: head.stream_id,
            expires_at_ms: now_ms
                .checked_add(MAX_STREAM_LIFETIME_MS)
                .context("fetch stream expiration overflow")?,
        },
        status: head.status,
        status_text: head.status_text,
        headers: head.headers,
    })
}

pub fn actor_stream_chunk(chunk: HttpStreamChunk) -> Result<ActorFetchStreamChunk> {
    validate_limit(
        "fetch stream chunk",
        chunk.body.len(),
        MAX_STREAM_CHUNK_BYTES as usize,
    )?;
    Ok(ActorFetchStreamChunk {
        body: FileBytes(chunk.body),
        done: chunk.done,
    })
}

pub fn validate_headers(headers: &BTreeMap<String, String>) -> Result<()> {
    if headers.len() > MAX_HTTP_HEADERS {
        bail!(
            "limit_exceeded: HTTP headers has {} entries; maximum is {MAX_HTTP_HEADERS}",
            headers.len()
        );
    }
    let bytes = headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    validate_limit("HTTP headers", bytes, MAX_HTTP_HEADER_BYTES)
}

pub fn validate_http_request(request: &HttpRequest) -> Result<()> {
    if request.port == 0 {
        bail!("invalid_input: network port must be between 1 and 65535");
    }
    validate_nonempty_bytes("HTTP path", &request.path, MAX_HTTP_PATH_BYTES)?;
    if !request.path.starts_with('/') {
        bail!("invalid_input: HTTP path must start with '/'");
    }
    validate_nonempty_bytes("HTTP method", &request.method, MAX_HTTP_METHOD_BYTES)?;
    if !request
        .method
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
    {
        let uppercase = request.method.to_ascii_uppercase();
        if !uppercase
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            bail!("invalid_input: HTTP method contains invalid bytes");
        }
    }
    validate_headers(&request.headers)?;
    if let Some(body) = &request.body {
        validate_limit("HTTP body", body.byte_len(), MAX_HTTP_BODY_BYTES)?;
    }
    Ok(())
}

pub fn validate_stream(stream: &ActorFetchStreamId) -> Result<()> {
    if stream.generation == 0 {
        bail!("invalid_input: fetch stream generation must be greater than zero");
    }
    if stream.expires_at_ms <= 0 {
        bail!("invalid_input: fetch stream expiration must be greater than zero");
    }
    validate_nonempty_bytes("fetch stream id", &stream.stream_id, MAX_STREAM_ID_BYTES)
}

pub fn validate_token(token: &str) -> Result<()> {
    validate_nonempty_bytes("preview token", token, MAX_PREVIEW_TOKEN_BYTES)?;
    if !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid_input: preview token contains invalid bytes");
    }
    Ok(())
}

pub fn validate_nonempty_bytes(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} cannot be empty");
    }
    validate_limit(label, value.len(), max)
}

pub fn validate_limit(label: &str, actual: usize, max: usize) -> Result<()> {
    if actual > max {
        bail!("limit_exceeded: {label} is {actual}; maximum is {max}");
    }
    Ok(())
}

pub fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
