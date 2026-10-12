use crate::budgets::{HostedActorBudgets, ValidationBound};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use agentos_client::{HttpRequest, HttpResponse, HttpStreamChunk, HttpStreamHead};
use anyhow::{bail, Context, Result};
use rivetkit::{Ctx, Handler, Request, Response};
use serde::{Deserialize, Serialize};

use crate::config::MIN_PREVIEW_TTL_MS;
use crate::runtime::now_ms;
use crate::store::{self, PreviewLease};
use crate::{AgentOsActor, FileBytes, FileContentInput};

#[cfg(test)]
const MAX_STREAM_CHUNK_BYTES: u32 = 128 * 1024;
const DEFAULT_STREAM_CHUNK_BYTES: u32 = 64 * 1024;
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
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        if self.port == 0 {
            bail!("invalid_input: network port must be between 1 and 65535");
        }
        validate_nonempty_bytes("HTTP path", &self.path, budgets.network.max_http_path_bytes)?;
        if !self.path.starts_with('/') {
            bail!("invalid_input: HTTP path must start with '/'");
        }
        validate_nonempty_bytes(
            "HTTP method",
            &self.method,
            budgets.network.max_http_method_bytes,
        )?;
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
        validate_headers(&self.headers, budgets)?;
        if let Some(body) = &self.body {
            validate_limit(
                "HTTP body",
                body.byte_len(),
                budgets.network.max_http_body_bytes,
            )?;
        }
        Ok(())
    }

    fn into_core(self) -> HttpRequest {
        HttpRequest {
            port: self.port,
            path: self.path,
            method: self.method,
            headers: self.headers,
            body: self.body.map(FileContentInput::into_core),
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

impl Handler<NetworkFetch> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: NetworkFetch,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorHttpResponse>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.request.validate(budgets)?;
            actor_http_response(
                self.runtime
                    .vm()
                    .await?
                    .http_request(action.request.into_core())
                    .await?,
                budgets,
            )
        })
    }
}

impl Handler<NetworkFetchStreamStart> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: NetworkFetchStreamStart,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorFetchStreamHead>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.request.validate(budgets)?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let head = vm
                .http_request_stream_start(action.request.into_core())
                .await?;
            publish_stream_head(status.generation, head, budgets, |raw_id| async move {
                vm.http_request_stream_cancel(&raw_id).await
            })
            .await
        })
    }
}

impl Handler<NetworkFetchStreamRead> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: NetworkFetchStreamRead,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorFetchStreamChunk>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_stream(&action.stream, budgets)?;
            let max_bytes = select_stream_bytes(action.max_bytes, budgets)?;
            let vm = self
                .runtime
                .vm_at_generation(action.stream.generation)
                .await?;
            if stream_is_expired(action.stream.expires_at_ms, now_ms()?, budgets) {
                vm.http_request_stream_cancel(&action.stream.stream_id)
                    .await
                    .context("cancel expired fetch stream")?;
                bail!(
                    "fetch_stream_expired: stream exceeded its absolute lifetime; raise network.maxStreamLifetimeMs in --actor-budgets-json and restart this worker before opening a new stream"
                );
            }
            let chunk = vm
                .http_request_stream_read(&action.stream.stream_id, max_bytes)
                .await?;
            actor_stream_chunk(chunk, budgets)
        })
    }
}

impl Handler<NetworkFetchStreamCancel> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: NetworkFetchStreamCancel,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_stream(&action.stream, budgets)?;
            self.runtime
                .vm_at_generation(action.stream.generation)
                .await?
                .http_request_stream_cancel(&action.stream.stream_id)
                .await?;
            Ok(())
        })
    }
}

impl Handler<NetworkPreviewCreate> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: NetworkPreviewCreate,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorPreview>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            if action.port == 0 {
                bail!("invalid_input: preview port must be between 1 and 65535");
            }
            self.runtime.vm().await?;
            let preview = self.snapshot(&ctx).await?.desired.preview;
            let ttl_ms = select_preview_ttl(preview, action.ttl_ms, budgets)?;
            let created_at_ms = now_ms()?;
            let expires_at_ms = created_at_ms
                .checked_add(i64::try_from(ttl_ms).context("preview TTL exceeds signed range")?)
                .context("preview expiration overflow")?;
            let token = generated_preview_token(budgets)?;
            let lease = PreviewLease {
                token: token.clone(),
                port: action.port,
                expires_at_ms,
            };
            store::create_preview(
                &ctx,
                &lease,
                created_at_ms,
                preview_capacity(preview, budgets),
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

impl Handler<NetworkPreviewExpire> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: NetworkPreviewExpire,
    ) -> impl std::future::Future<Output = anyhow::Result<bool>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_token(&action.token, budgets)?;
            store::expire_preview(&ctx, &action.token).await
        })
    }
}

pub(crate) async fn handle_preview_fetch(
    actor: Arc<AgentOsActor>,
    ctx: Ctx<AgentOsActor>,
    request: Request,
) -> Result<Response> {
    let budgets = actor.budgets.as_ref();
    let path = request.uri().path();
    let Some(remainder) = path.strip_prefix(PREVIEW_PREFIX) else {
        return Response::from_parts(404, HashMap::new(), Vec::new());
    };
    let (token, guest_path) = remainder
        .split_once('/')
        .map(|(token, path)| (token, format!("/{path}")))
        .unwrap_or((remainder, String::from("/")));
    if let Err(error) = validate_token(token, budgets) {
        if let Some(limit) = error.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>() {
            tracing::warn!(setting=limit.setting, observed=limit.observed, limit=limit.limit, units=limit.units, %error, "preview token rejected by worker budget");
        }
        return Response::from_parts(404, HashMap::new(), Vec::new());
    }
    let Some(lease) = store::load_preview(&ctx, token, now_ms()?).await? else {
        return Response::from_parts(404, HashMap::new(), Vec::new());
    };
    validate_limit(
        "preview request body",
        request.body().len(),
        budgets.network.max_http_body_bytes,
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
    request.validate(budgets)?;
    let response = actor
        .runtime
        .vm()
        .await?
        .http_request(request.into_core())
        .await?;
    validate_limit(
        "preview response body",
        response.body.len(),
        budgets.network.max_http_response_bytes,
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

fn actor_http_response(
    response: HttpResponse,
    budgets: &HostedActorBudgets,
) -> Result<ActorHttpResponse> {
    let header_bytes = response
        .headers
        .iter()
        .fold(0usize, |total, (name, value)| {
            total.saturating_add(name.len()).saturating_add(value.len())
        });
    validate_limit(
        "HTTP response headers",
        header_bytes,
        budgets.network.max_http_header_bytes,
    )?;
    validate_limit(
        "HTTP response body",
        response.body.len(),
        budgets.network.max_http_response_bytes,
    )?;
    Ok(ActorHttpResponse {
        status: response.status,
        status_text: response.status_text,
        headers: response.headers,
        body: FileBytes(response.body),
    })
}

fn actor_stream_head(
    generation: u64,
    head: HttpStreamHead,
    budgets: &HostedActorBudgets,
) -> Result<ActorFetchStreamHead> {
    validate_nonempty_bytes(
        "fetch stream id",
        &head.stream_id,
        budgets.network.max_stream_id_bytes,
    )?;
    let header_bytes = head.headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    validate_limit(
        "HTTP response headers",
        header_bytes,
        budgets.network.max_http_header_bytes,
    )?;
    Ok(ActorFetchStreamHead {
        stream: ActorFetchStreamId {
            generation,
            stream_id: head.stream_id,
            expires_at_ms: stream_expiration_at(now_ms()?, budgets)?,
        },
        status: head.status,
        status_text: head.status_text,
        headers: head.headers,
    })
}

fn actor_stream_chunk(
    chunk: HttpStreamChunk,
    budgets: &HostedActorBudgets,
) -> Result<ActorFetchStreamChunk> {
    validate_limit(
        "fetch stream chunk",
        chunk.body.len(),
        budgets.network.max_stream_chunk_bytes,
    )?;
    Ok(ActorFetchStreamChunk {
        body: FileBytes(chunk.body),
        done: chunk.done,
    })
}

fn validate_headers(
    headers: &BTreeMap<String, String>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    budgets
        .network
        .max_http_headers
        .check("HTTP headers", headers.len())?;
    let bytes = headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    validate_limit("HTTP headers", bytes, budgets.network.max_http_header_bytes)
}

fn validate_stream(stream: &ActorFetchStreamId, budgets: &HostedActorBudgets) -> Result<()> {
    if stream.generation == 0 {
        bail!("invalid_input: fetch stream generation must be greater than zero");
    }
    if stream.expires_at_ms <= 0 {
        bail!("invalid_input: fetch stream expiration must be greater than zero");
    }
    validate_nonempty_bytes(
        "fetch stream id",
        &stream.stream_id,
        budgets.network.max_stream_id_bytes,
    )
}

fn validate_token(token: &str, budgets: &HostedActorBudgets) -> Result<()> {
    validate_nonempty_bytes(
        "preview token",
        token,
        budgets.network.max_preview_token_bytes,
    )?;
    if !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid_input: preview token contains invalid bytes");
    }
    Ok(())
}

fn validate_nonempty_bytes(label: &str, value: &str, max: ValidationBound) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} cannot be empty");
    }
    max.check(label, value.len())
}

fn validate_limit(label: &str, actual: usize, max: ValidationBound) -> Result<()> {
    max.check(label, actual)
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

fn select_stream_bytes(requested: Option<u32>, budgets: &HostedActorBudgets) -> Result<u32> {
    let selected = requested.unwrap_or(DEFAULT_STREAM_CHUNK_BYTES);
    if selected == 0 {
        bail!("limit_exceeded: fetch stream maxBytes must be positive");
    }
    budgets
        .network
        .max_stream_chunk_bytes
        .check("fetch stream maxBytes", selected as usize)?;
    Ok(selected)
}

fn preview_capacity(
    preview: crate::PreviewPolicy,
    budgets: &HostedActorBudgets,
) -> ValidationBound {
    ValidationBound {
        setting: if (preview.max_active as usize) < budgets.preview.max_active.maximum {
            "config.preview.maxActive"
        } else {
            budgets.preview.max_active.setting
        },
        maximum: preview.max_active as usize,
        units: "previews",
    }
}

fn select_preview_ttl(
    preview: crate::PreviewPolicy,
    requested: Option<u64>,
    budgets: &HostedActorBudgets,
) -> Result<u64> {
    let selected = requested.unwrap_or(preview.default_ttl_ms);
    if selected < MIN_PREVIEW_TTL_MS {
        bail!("limit_exceeded: preview ttlMs is below its minimum TTL");
    }
    let bound = ValidationBound {
        setting: if (preview.max_ttl_ms as usize) < budgets.preview.max_ttl_ms.maximum {
            "config.preview.maxTtlMs"
        } else {
            budgets.preview.max_ttl_ms.setting
        },
        maximum: preview.max_ttl_ms as usize,
        units: "milliseconds",
    };
    bound.check(
        "preview ttlMs",
        usize::try_from(selected).context("preview TTL exceeds platform representation")?,
    )?;
    Ok(selected)
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
        assert!(request
            .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
            .is_err());
        assert!(validate_limit(
            "chunk",
            MAX_STREAM_CHUNK_BYTES as usize + 1,
            HostedActorBudgets::normalize(Default::default())
                .unwrap()
                .network
                .max_stream_chunk_bytes
        )
        .is_err());
    }

    #[test]
    fn preview_tokens_and_hop_headers_are_restricted() {
        assert!(validate_token(
            "../../control",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
        assert!(validate_token(
            "0123456789abcdef",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_ok());
        assert!(is_hop_by_hop_header("Transfer-Encoding"));
        assert!(!is_hop_by_hop_header("set-cookie"));
    }
}

fn stream_expiration_at(started_at_ms: i64, budgets: &HostedActorBudgets) -> Result<i64> {
    started_at_ms
        .checked_add(budgets.network.max_stream_lifetime_ms.maximum as i64)
        .context(
            "invalid_input: actor.network.maxStreamLifetimeMs exceeds expiration representation",
        )
}

#[cfg(test)]
#[path = "../tests/unit/network_remaining_budgets.rs"]
mod remaining_budget_tests;

fn stream_is_expired(expires_at_ms: i64, now: i64, budgets: &HostedActorBudgets) -> bool {
    let bound = budgets.network.max_stream_lifetime_ms;
    let remaining = usize::try_from(expires_at_ms.saturating_sub(now).max(0)).unwrap_or(usize::MAX);
    agentos_actor_contract::warn_near_budget(
        bound.setting,
        bound.maximum.saturating_sub(remaining),
        bound.maximum,
        bound.units,
    );
    expires_at_ms <= now
}

async fn publish_stream_head<F, C>(
    generation: u64,
    head: HttpStreamHead,
    budgets: &HostedActorBudgets,
    cancel: C,
) -> Result<ActorFetchStreamHead>
where
    C: FnOnce(String) -> F,
    F: std::future::Future<Output = Result<()>>,
{
    let raw_id = head.stream_id.clone();
    match actor_stream_head(generation, head, budgets) {
        Ok(head) => Ok(head),
        Err(primary) => match cancel(raw_id.clone()).await {
            Ok(()) => Err(primary),
            Err(cleanup) => {
                tracing::error!(stream_id=%raw_id, %primary, %cleanup, "failed to cancel rejected native fetch stream; owning VM remains responsible for shutdown");
                let context = format!("stream head rejected ({primary}); native stream {raw_id:?} cancellation also failed ({cleanup:#})");
                Err(primary.context(context))
            }
        },
    }
}

fn generated_preview_token(budgets: &HostedActorBudgets) -> Result<String> {
    let token = uuid::Uuid::new_v4().simple().to_string();
    validate_token(&token, budgets)?;
    Ok(token)
}
