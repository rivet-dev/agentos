//! Explicit HTTP qualification; direct RivetError serialization cannot prove masking behavior.
use agentos_actor::{AgentOsActor, VmLifecycleState, VmStatus, ACTOR_NAME};
use agentos_preload::{
    PreloadCoordinatorActor, PreloadStatus, PRELOAD_COORDINATOR_ACTOR_NAME,
    PRELOAD_PROTOCOL_VERSION,
};
use anyhow::{Context, Result};
use rivetkit::client::{Client, ClientConfig, GetOrCreateOptions};
use rivetkit::TypedClientExt;
use serde_json::{json, Value};
use std::time::Duration;
#[allow(dead_code)]
mod support;

async fn http_action(
    client: &Client,
    fixture: &support::Fixture,
    actor_name: &str,
    actor_id: &str,
    action: &str,
    input: Value,
) -> Result<(u16, Value)> {
    tokio::time::timeout(Duration::from_secs(10), async {
        // fetch() is the actor's /request API. Use the public resolved-ID gateway
        // URL to reach the framework action route without that prefix.
        let gateway = client
            .get_for_id(actor_name, actor_id, Default::default())?
            .gateway_url()?;
        let mut url = reqwest::Url::parse(&gateway).context("parse action gateway URL")?;
        anyhow::ensure!(
            url.origin() == reqwest::Url::parse(&fixture.endpoint)?.origin(),
            "action gateway escaped fixture origin"
        );
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("action gateway URL cannot hold path segments"))?
            .pop_if_empty()
            .push("action")
            .push(action);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()?;
        let bytes = serde_json::to_vec(&json!({"args":[input]}))?;
        let mut response = http
            .post(url)
            .header("x-rivet-target", "actor")
            .header("x-rivet-actor", actor_id)
            .header("x-rivet-namespace", &fixture.namespace)
            .header("x-rivet-token", &fixture.token)
            .header("x-rivet-encoding", "json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("send bounded action HTTP control")?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| {
                format!(
                    "read action HTTP control: status={status} bytes={}",
                    body.len()
                )
            })?
        {
            anyhow::ensure!(
                body.len().saturating_add(chunk.len()) <= 16 * 1024,
                "HTTP error control response exceeded16KiB"
            );
            body.extend_from_slice(&chunk);
        }
        let decoded = serde_json::from_slice(&body).with_context(|| {
            format!(
                "decode action HTTP JSON: status={status} bytes={}",
                body.len()
            )
        })?;
        Ok((status, decoded))
    })
    .await
    .context("HTTP action control exceeded10s")?
}

fn require_public_budget(
    status: u16,
    body: &Value,
    setting: &str,
    observed: u64,
    limit: u64,
    remedy: &str,
) -> Result<()> {
    // Check the actual HTTP representation, not a locally serialized RivetError.
    anyhow::ensure!(
        status == 400,
        "budget HTTP status was {status}; expected400"
    );
    anyhow::ensure!(
        body["group"] == "user" && body["code"] == "limit_exceeded",
        "wrong public budget group/code"
    );
    let budget = &body["metadata"]["budget"];
    anyhow::ensure!(
        budget["setting"] == setting
            && budget["observed"] == observed
            && budget["limit"] == limit
            && budget["units"] == "bytes",
        "wrong HTTP budget metadata: {budget}"
    );
    anyhow::ensure!(
        body["metadata"].as_object().is_some_and(|m| m.len() == 1)
            && budget.as_object().is_some_and(|m| m.len() == 5),
        "budget response contains unexpected metadata"
    );
    anyhow::ensure!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains(setting) && m.contains(remedy)),
        "HTTP budget message was masked or omitted remedy"
    );
    anyhow::ensure!(
        budget["remediation"]
            .as_str()
            .is_some_and(|m| m.contains(remedy)),
        "HTTP budget remediation absent"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "explicit owned hosted HTTP fixture; worker token budget1 and default preload URL4096;90s body"]
async fn hosted_http_budget_diagnostics_are_public_and_internal_errors_stay_masked() -> Result<()> {
    let fixture = support::Fixture::from_env()?;
    let client = Client::new(
        ClientConfig::new(&fixture.endpoint)
            .namespace(&fixture.namespace)
            .pool_name(&fixture.pool)
            .token(&fixture.token)
            .disable_metadata_lookup(true),
    );
    let mut ids = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let software_text = std::env::var("AGENTOS_HOSTED_TEST_SOFTWARE_JSON")?;
        anyhow::ensure!(software_text.len() <= 64*1024, "software fixture exceeds64KiB");
        let software: Value = serde_json::from_str(&software_text)?;
        anyhow::ensure!(software.as_array().is_some_and(|a| !a.is_empty() && a.len()<=16), "software fixture must contain1..16 entries");
        let actor = client.get_or_create_typed::<AgentOsActor>(ACTOR_NAME,
            [format!("public-budget-{}", uuid::Uuid::new_v4())], GetOrCreateOptions {
                create_with_input: Some(json!({"config":{"software":software,"filesystem":{"mounts":[{"path":"/workspace","backend":{"type":"actor-sqlite"},"readOnly":false}]}}})),
                ..Default::default()
            })?;
        let actor_http = actor.inner().resolve_handle().await?;
        ids.push(actor_http.actor_id().to_owned());
        anyhow::ensure!(actor.call(VmStatus {}).await?.lifecycle == VmLifecycleState::Ready, "fixture VM must be ready");
        let coordinator = client.get_or_create_typed::<PreloadCoordinatorActor>(PRELOAD_COORDINATOR_ACTOR_NAME,
            [format!("public-preload-{}", uuid::Uuid::new_v4())], GetOrCreateOptions {
                create_with_input: Some(json!({})), ..Default::default()
            })?;
        let coordinator_http = coordinator.inner().resolve_handle().await?;
        ids.push(coordinator_http.actor_id().to_owned());
        coordinator.call(PreloadStatus {}).await?;
        let (status, actor_error) = http_action(&client, &fixture, ACTOR_NAME, actor_http.actor_id(), "network.preview.create", json!({"port":8080,"ttlMs":60000})).await?;
        let url = format!("https://packages.gameinc.io/{}.aospkg", "x".repeat(4096));
        let (preload_status, preload_error) = http_action(&client, &fixture, PRELOAD_COORDINATOR_ACTOR_NAME, coordinator_http.actor_id(), "preload.replaceBaseline", json!({
            "protocolVersion":PRELOAD_PROTOCOL_VERSION,"artifacts":[{"url":url,"digest":format!("sha256:{}","a".repeat(64)),"size":1,"packageFormatVersion":1}]
        })).await?;
        // This real, untyped coordinator validation failure must still be masked.
        let (private_status, private_error) = http_action(&client, &fixture, PRELOAD_COORDINATOR_ACTOR_NAME, coordinator_http.actor_id(), "preload.getPlan", json!({
            "protocolVersion":0,"processId":"fixture","supportedPackageFormatVersions":[1]
        })).await?;
        // Record only deliberately public diagnostic fields; never credentials/actor identity.
        eprintln!("HTTP_BUDGET_CONTROL actor_status={status} actor_group={} actor_code={} actor_metadata={} preload_status={preload_status} preload_group={} preload_code={} preload_metadata={} private_status={private_status} private_group={} private_code={}",
            actor_error["group"], actor_error["code"], actor_error["metadata"], preload_error["group"], preload_error["code"], preload_error["metadata"], private_error["group"], private_error["code"]);
        let mut findings = Vec::new();
        for checked in [
            require_public_budget(status, &actor_error, "actor.network.maxPreviewTokenBytes", 32, 1, "--actor-budgets-json"),
            require_public_budget(preload_status, &preload_error, "preloadPolicy.maxUrlBytes", url.len() as u64, 4096, "--preload-policy-json"),
        ] { if let Err(error) = checked { findings.push(error.to_string()); } }
        if private_status != 500 || private_error["group"] != "core" || private_error["code"] != "internal_error" || private_error["message"] != rivet_error::INTERNAL_ERROR.default_message || !private_error["metadata"].is_null() {
            findings.push("untyped coordinator error changed its internal masking contract".into());
        }
        // Ordinary typed client calls still work after all rejected actions.
        anyhow::ensure!(actor.call(VmStatus {}).await?.lifecycle == VmLifecycleState::Ready, "fixture VM must be ready");
        coordinator.call(PreloadStatus {}).await?;
        anyhow::ensure!(findings.is_empty(), "{}", findings.join("; "));
        Ok::<_, anyhow::Error>(())
    }).await.context("hosted HTTP budget control exceeded90s").and_then(|r|r);
    client.dispose();
    let mut failures = result
        .err()
        .map(|e| vec![format!("{e:#}")])
        .unwrap_or_default();
    for id in ids {
        let cleanup = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.request(
                reqwest::Method::DELETE,
                &format!("/actors/{id}?namespace={}", fixture.namespace),
                None,
                &fixture.token,
            ),
        )
        .await;
        match cleanup {
            Ok(Ok((status, _))) if status.is_success() => {}
            Ok(Ok((status, _))) => failures.push(format!("actor cleanup rejected: {status}")),
            Ok(Err(error)) => failures.push(format!("actor cleanup failed: {error:#}")),
            Err(error) => failures.push(format!("actor cleanup exceeded5s: {error}")),
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
    Ok(())
}

#[path = "support/preload_clients.rs"]
mod preload_clients;
