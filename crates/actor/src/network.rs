use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use agentos_client::{HttpRequest, HttpResponse, HttpStreamChunk, HttpStreamHead};
use anyhow::{bail, Context, Result};
use rivetkit::{Ctx, Handles, Request, Response};
use serde::{Deserialize, Serialize};

use crate::actions::BoxFuture;
use crate::config::MIN_PREVIEW_TTL_MS;
use crate::runtime::now_ms;
use crate::store::{self, PreviewLease};
use crate::{AgentOsActor, FileBytes, FileContentInput};

const MAX_HTTP_PATH_BYTES: usize = 16 * 1024;
const MAX_HTTP_METHOD_BYTES: usize = 32;
const MAX_HTTP_HEADERS: usize = 128;
const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 512 * 1024;
const MAX_HTTP_RESPONSE_BYTES: usize = 768 * 1024;
const MAX_STREAM_CHUNK_BYTES: u32 = 128 * 1024;
const DEFAULT_STREAM_CHUNK_BYTES: u32 = 64 * 1024;
const MAX_STREAM_ID_BYTES: usize = 256;
const MAX_STREAM_LIFETIME_MS: i64 = 60 * 60 * 1_000;
const MAX_PREVIEW_TOKEN_BYTES: usize = 128;
const PREVIEW_PREFIX: &str = "/preview/";

fn default_http_method() -> String {
    String::from("GET")
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorHttpRequest {
    pub port: u16,
    pub path: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default = "default_http_method")]
    pub method: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FileContentInput>,
}

impl ActorHttpRequest {
    fn validate(&self) -> Result<()> {
        if self.port == 0 {
            bail!("invalid_input: network port must be between 1 and 65535");
        }
        validate_nonempty_bytes("HTTP path", &self.path, MAX_HTTP_PATH_BYTES)?;
        if !self.path.starts_with('/') {
            bail!("invalid_input: HTTP path must start with '/'");
        }
        validate_nonempty_bytes("HTTP method", &self.method, MAX_HTTP_METHOD_BYTES)?;
        if !self
            .method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            let uppercase = self.method.to_ascii_uppercase();
            if !uppercase
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
            {
                bail!("invalid_input: HTTP method contains invalid bytes");
            }
        }
        validate_headers(&self.headers)?;
        if let Some(body) = &self.body {
            validate_limit("HTTP body", body.byte_len(), MAX_HTTP_BODY_BYTES)?;
        }
        Ok(())
    }

    fn into_core(self) -> HttpRequest {
        HttpRequest {
            port: self.port,
            path: self.path,
            method: self.method,
            headers: self.headers,
            body: self.body.map(file_content_into_bytes),
        }
    }
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

crate::register_action!(NetworkFetch => ActorHttpResponse, "network.fetch");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamStart {
    pub request: ActorHttpRequest,
}

crate::register_action!(NetworkFetchStreamStart => ActorFetchStreamHead, "network.fetchStream.start");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamRead {
    pub stream: ActorFetchStreamId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
}

crate::register_action!(NetworkFetchStreamRead => ActorFetchStreamChunk, "network.fetchStream.read");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkFetchStreamCancel {
    pub stream: ActorFetchStreamId,
}

crate::register_action!(NetworkFetchStreamCancel => (), "network.fetchStream.cancel");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkPreviewCreate {
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

crate::register_action!(NetworkPreviewCreate => ActorPreview, "network.preview.create");

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

crate::register_action!(NetworkPreviewExpire => bool, "network.preview.expire");

impl Handles<NetworkFetch> for AgentOsActor {
    type Future = BoxFuture<ActorHttpResponse>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: NetworkFetch) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.request.validate()?;
            actor_http_response(
                self.runtime
                    .vm()
                    .await?
                    .http_request(action.request.into_core())
                    .await?,
            )
        })
    }
}

impl Handles<NetworkFetchStreamStart> for AgentOsActor {
    type Future = BoxFuture<ActorFetchStreamHead>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: NetworkFetchStreamStart) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.request.validate()?;
            let status = self.runtime.status().await;
            let head = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .http_request_stream_start(action.request.into_core())
                .await?;
            actor_stream_head(status.generation, head)
        })
    }
}

impl Handles<NetworkFetchStreamRead> for AgentOsActor {
    type Future = BoxFuture<ActorFetchStreamChunk>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: NetworkFetchStreamRead) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_stream(&action.stream)?;
            let max_bytes = action.max_bytes.unwrap_or(DEFAULT_STREAM_CHUNK_BYTES);
            if max_bytes == 0 || max_bytes > MAX_STREAM_CHUNK_BYTES {
                bail!(
                    "limit_exceeded: fetch stream maxBytes must be between 1 and {MAX_STREAM_CHUNK_BYTES}"
                );
            }
            let vm = self
                .runtime
                .vm_at_generation(action.stream.generation)
                .await?;
            if action.stream.expires_at_ms <= now_ms()? {
                vm.http_request_stream_cancel(&action.stream.stream_id)
                    .await
                    .context("cancel expired fetch stream")?;
                bail!(
                    "fetch_stream_expired: stream exceeded its {MAX_STREAM_LIFETIME_MS}ms absolute lifetime"
                );
            }
            let chunk = vm
                .http_request_stream_read(&action.stream.stream_id, max_bytes)
                .await?;
            actor_stream_chunk(chunk)
        })
    }
}

impl Handles<NetworkFetchStreamCancel> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: NetworkFetchStreamCancel) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_stream(&action.stream)?;
            self.runtime
                .vm_at_generation(action.stream.generation)
                .await?
                .http_request_stream_cancel(&action.stream.stream_id)
                .await?;
            Ok(())
        })
    }
}

impl Handles<NetworkPreviewCreate> for AgentOsActor {
    type Future = BoxFuture<ActorPreview>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: NetworkPreviewCreate) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            if action.port == 0 {
                bail!("invalid_input: preview port must be between 1 and 65535");
            }
            self.runtime.vm().await?;
            let preview = self.snapshot().await.desired.preview;
            let ttl_ms = action.ttl_ms.unwrap_or(preview.default_ttl_ms);
            if !(MIN_PREVIEW_TTL_MS..=preview.max_ttl_ms).contains(&ttl_ms) {
                bail!(
                    "limit_exceeded: preview ttlMs must be between {MIN_PREVIEW_TTL_MS} and {}; raise config.preview.maxTtlMs up to its actor maximum",
                    preview.max_ttl_ms
                );
            }
            let created_at_ms = now_ms()?;
            let expires_at_ms = created_at_ms
                .checked_add(i64::try_from(ttl_ms).context("preview TTL exceeds signed range")?)
                .context("preview expiration overflow")?;
            let token = uuid::Uuid::new_v4().simple().to_string();
            let lease = PreviewLease {
                token: token.clone(),
                port: action.port,
                expires_at_ms,
            };
            store::create_preview(
                &ctx,
                &lease,
                created_at_ms,
                usize::try_from(preview.max_active).context("preview maxActive exceeds usize")?,
            )
            .await?;
            Ok(ActorPreview {
                path: format!("{PREVIEW_PREFIX}{token}/"),
                token,
                port: action.port,
                expires_at_ms,
            })
        })
    }
}

impl Handles<NetworkPreviewExpire> for AgentOsActor {
    type Future = BoxFuture<bool>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: NetworkPreviewExpire) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_token(&action.token)?;
            store::expire_preview(&ctx, &action.token).await
        })
    }
}

pub(crate) async fn handle_preview_fetch(
    actor: Arc<AgentOsActor>,
    ctx: Ctx<AgentOsActor>,
    request: Request,
) -> Result<Response> {
    let path = request.uri().path();
    let Some(remainder) = path.strip_prefix(PREVIEW_PREFIX) else {
        return Response::from_parts(404, HashMap::new(), Vec::new());
    };
    let (token, guest_path) = remainder
        .split_once('/')
        .map(|(token, path)| (token, format!("/{path}")))
        .unwrap_or((remainder, String::from("/")));
    if validate_token(token).is_err() {
        return Response::from_parts(404, HashMap::new(), Vec::new());
    }
    let Some(lease) = store::load_preview(&ctx, token, now_ms()?).await? else {
        return Response::from_parts(404, HashMap::new(), Vec::new());
    };
    validate_limit(
        "preview request body",
        request.body().len(),
        MAX_HTTP_BODY_BYTES,
    )?;
    let mut guest_path = guest_path;
    if let Some(query) = request.uri().query() {
        guest_path.push('?');
        guest_path.push_str(query);
    }
    let headers = request
        .headers()
        .iter()
        .filter(|(name, _)| !is_hop_by_hop_header(name.as_str()))
        .map(|(name, value)| {
            value
                .to_str()
                .with_context(|| format!("preview request header {name} is not text"))
                .map(|value| (name.to_string(), value.to_owned()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let request = ActorHttpRequest {
        port: lease.port,
        path: guest_path,
        method: request.method().to_string(),
        headers,
        body: Some(FileContentInput::Bytes(request.body().clone())),
    };
    request.validate()?;
    let response = actor
        .runtime
        .vm()
        .await?
        .http_request(request.into_core())
        .await?;
    validate_limit(
        "preview response body",
        response.body.len(),
        MAX_HTTP_RESPONSE_BYTES,
    )?;

    let mut outgoing = http::Response::builder()
        .status(response.status)
        .body(response.body)?;
    for (name, value) in response.headers {
        if is_hop_by_hop_header(&name) || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let name = name
            .parse::<http::header::HeaderName>()
            .with_context(|| format!("invalid preview response header name {name:?}"))?;
        let value = value
            .parse::<http::header::HeaderValue>()
            .context("invalid preview response header value")?;
        outgoing.headers_mut().append(name, value);
    }
    outgoing.headers_mut().insert(
        http::header::CACHE_CONTROL,
        http::header::HeaderValue::from_static("no-store"),
    );
    Ok(Response::from(outgoing))
}

fn actor_http_response(response: HttpResponse) -> Result<ActorHttpResponse> {
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

fn actor_stream_head(generation: u64, head: HttpStreamHead) -> Result<ActorFetchStreamHead> {
    validate_nonempty_bytes("fetch stream id", &head.stream_id, MAX_STREAM_ID_BYTES)?;
    let header_bytes = head.headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    validate_limit("HTTP response headers", header_bytes, MAX_HTTP_HEADER_BYTES)?;
    Ok(ActorFetchStreamHead {
        stream: ActorFetchStreamId {
            generation,
            stream_id: head.stream_id,
            expires_at_ms: now_ms()?
                .checked_add(MAX_STREAM_LIFETIME_MS)
                .context("fetch stream expiration overflow")?,
        },
        status: head.status,
        status_text: head.status_text,
        headers: head.headers,
    })
}

fn actor_stream_chunk(chunk: HttpStreamChunk) -> Result<ActorFetchStreamChunk> {
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

fn validate_headers(headers: &BTreeMap<String, String>) -> Result<()> {
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

fn validate_stream(stream: &ActorFetchStreamId) -> Result<()> {
    if stream.generation == 0 {
        bail!("invalid_input: fetch stream generation must be greater than zero");
    }
    if stream.expires_at_ms <= 0 {
        bail!("invalid_input: fetch stream expiration must be greater than zero");
    }
    validate_nonempty_bytes("fetch stream id", &stream.stream_id, MAX_STREAM_ID_BYTES)
}

fn validate_token(token: &str) -> Result<()> {
    validate_nonempty_bytes("preview token", token, MAX_PREVIEW_TOKEN_BYTES)?;
    if !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid_input: preview token contains invalid bytes");
    }
    Ok(())
}

fn validate_nonempty_bytes(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} cannot be empty");
    }
    validate_limit(label, value.len(), max)
}

fn validate_limit(label: &str, actual: usize, max: usize) -> Result<()> {
    if actual > max {
        bail!("limit_exceeded: {label} is {actual}; maximum is {max}");
    }
    Ok(())
}

fn file_content_into_bytes(content: FileContentInput) -> Vec<u8> {
    match content {
        FileContentInput::Text(value) => value.into_bytes(),
        FileContentInput::Bytes(value) => value,
    }
}

fn is_hop_by_hop_header(name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_network_request_limits_are_bounded() {
        let request = ActorHttpRequest {
            port: 0,
            path: String::from("/"),
            method: String::from("GET"),
            headers: BTreeMap::new(),
            body: None,
        };
        assert!(request.validate().is_err());
        assert!(validate_limit(
            "chunk",
            MAX_STREAM_CHUNK_BYTES as usize + 1,
            MAX_STREAM_CHUNK_BYTES as usize
        )
        .is_err());
    }

    #[test]
    fn preview_tokens_and_hop_headers_are_restricted() {
        assert!(validate_token("../../control").is_err());
        assert!(validate_token("0123456789abcdef").is_ok());
        assert!(is_hop_by_hop_header("Transfer-Encoding"));
        assert!(!is_hop_by_hop_header("set-cookie"));
    }
}
