use super::*;
use agentos_preload::PreloadReplaceBaseline;

#[tokio::test]
#[ignore = "explicit owned hosted public-client preload control; default maxUrlBytes4096;90s body"]
async fn public_rust_coordinator_client_retains_policy_error_and_saved_state() -> Result<()> {
    let fixture = support::Fixture::from_env()?;
    let client = Client::new(
        ClientConfig::new(&fixture.endpoint)
            .namespace(&fixture.namespace)
            .pool_name(&fixture.pool)
            .token(&fixture.token)
            .disable_metadata_lookup(true),
    );
    let mut id = None;
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let coordinator = client.get_or_create_typed::<PreloadCoordinatorActor>(PRELOAD_COORDINATOR_ACTOR_NAME,
            [format!("preload-client-rust-{}", uuid::Uuid::new_v4())], GetOrCreateOptions { create_with_input:Some(json!({})), ..Default::default() })?;
        id = Some(coordinator.inner().resolve().await?);
        let before = coordinator.call(PreloadStatus {}).await?;
        let url = format!("https://packages.gameinc.io/{}.aospkg", "x".repeat(4096));
        let action: PreloadReplaceBaseline = serde_json::from_value(json!({
            "protocolVersion":PRELOAD_PROTOCOL_VERSION,"artifacts":[{"url":url,"digest":format!("sha256:{}","a".repeat(64)),"size":1,"packageFormatVersion":1}]
        }))?;
        let error = coordinator.call(action).await.err().context("required preload policy rejection unexpectedly succeeded")?;
        // Qualified public Rust SDK exposes decoded metadata in anyhow text.
        // Do not invent a typed RivetError downcast unavailable in this client.
        let text = format!("{error:#}");
        for required in ["(user/limit_exceeded)".to_owned(), "\"setting\": String(\"preloadPolicy.maxUrlBytes\")".to_owned(),
            format!("\"observed\": Number({})",url.len()), "\"limit\": Number(4096)".to_owned(),
            "\"units\": String(\"bytes\")".to_owned(), "--preload-policy-json".to_owned()] {
            anyhow::ensure!(text.contains(&required),"public Rust preload rejection missing {required}");
        }
        anyhow::ensure!(coordinator.call(PreloadStatus {}).await?==before,"rejected action changed authoritative coordinator status/revision");
        Ok::<_,anyhow::Error>(())
    }).await.context("public Rust preload control exceeded90s").and_then(|r|r);
    client.dispose();
    let mut failures = result
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    if let Some(id) = id {
        match tokio::time::timeout(
            Duration::from_secs(5),
            fixture.request(
                reqwest::Method::DELETE,
                &format!("/actors/{id}?namespace={}", fixture.namespace),
                None,
                &fixture.token,
            ),
        )
        .await
        {
            Ok(Ok((status, _))) if status.is_success() => {}
            Ok(Ok((status, _))) => failures.push(format!("coordinator cleanup rejected:{status}")),
            Ok(Err(error)) => failures.push(format!("coordinator cleanup:{error:#}")),
            Err(error) => failures.push(format!("coordinator cleanup exceeded5s:{error}")),
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
    Ok(())
}
