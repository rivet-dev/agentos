use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub(crate) use agentos_actor_contract::network::*;
use anyhow::{bail, Context, Result};
use rivetkit::{Ctx, Handles, Request, Response};

use crate::actions::BoxFuture;
use crate::config::MIN_PREVIEW_TTL_MS;
use crate::runtime::now_ms;
use crate::store::{self, PreviewLease};
use crate::{AgentOsActor, FileContentInput};

const PREVIEW_PREFIX: &str = "/preview/";

crate::register_contract_action!(NetworkFetch);
crate::register_contract_action!(NetworkFetchStreamStart);
crate::register_contract_action!(NetworkFetchStreamRead);
crate::register_contract_action!(NetworkFetchStreamCancel);
crate::register_contract_action!(NetworkPreviewCreate);
crate::register_contract_action!(NetworkPreviewExpire);

impl Handles<NetworkFetch> for AgentOsActor {
    type Future = BoxFuture<ActorHttpResponse>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: NetworkFetch) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_http_request(&action.request)?;
            actor_http_response(
                self.runtime
                    .vm()
                    .await?
                    .http_request(action.request)
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
            validate_http_request(&action.request)?;
            let status = self.runtime.status().await;
            let head = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .http_request_stream_start(action.request)
                .await?;
            actor_stream_head(status.generation, now_ms()?, head)
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
    validate_http_request(&request)?;
    let response = actor.runtime.vm().await?.http_request(request).await?;
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
        assert!(validate_http_request(&request).is_err());
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
