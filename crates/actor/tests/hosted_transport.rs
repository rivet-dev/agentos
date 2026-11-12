use std::time::Duration;

use agentos_actor::{
    ActorProcessId, AgentOsActor, ConfigGet, ConfigPatch, FileContentInput, FilesystemReadFile,
    FilesystemWriteFile, ProcessOutputRead, ACTOR_NAME,
};
use anyhow::{Context, Result};
use rivetkit::client::{Client, ClientConfig, GetOrCreateOptions};
use rivetkit::TypedClientExt;

#[allow(dead_code)]
mod support;

#[tokio::test]
#[ignore = "explicit hosted authentication integration: owned3.0.7 engine and worker"]
async fn native_hosted_jwt_scope_signature_and_expiry_match_http_and_websocket() -> Result<()> {
    use agentos_actor::VmStatus;
    use base64::Engine;
    use rivetkit::{TypedActorConnection, TypedActorHandle};
    let fixture = support::Fixture::from_env()?;
    let owner = Client::new(
        ClientConfig::new(&fixture.endpoint)
            .namespace(&fixture.namespace)
            .pool_name(&fixture.pool)
            .token(&fixture.token)
            .disable_metadata_lookup(true),
    );
    let mut ids = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        for _ in 0..2 {
            let actor = owner.get_or_create_typed::<AgentOsActor>(ACTOR_NAME,
                [format!("jwt-rust-{}", uuid::Uuid::new_v4())], owned_actor_input()?)?;
            ids.push(actor.inner().resolve().await?);
            actor.call(VmStatus {}).await?;
        }
        let grant = serde_json::json!([{"resource":"actor_gateway","target":{"id":ids[0]},"operations":["read"]}]);
        let (valid, _) = fixture.issue_token(grant.clone(), 300).await?;
        let (expired, expires_ts) = fixture.issue_token(grant, 1).await?;
        let (prefix, signature) = valid.rsplit_once('.').context("issued JWT has no signature")?;
        let mut signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature)?;
        anyhow::ensure!(!signature.is_empty(), "issued JWT signature is empty");
        signature[0] ^= 1;
        let invalid = format!("{prefix}.{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature));
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
        let delay = u64::try_from((i128::from(expires_ts) + 30_000 - i128::try_from(now)? + 1100).max(0))?;
        // Published3.0.7 verifies until exp + PROTOCOL_CLOCK_SKEW(30s).
        anyhow::ensure!(delay <= 35_000, "expiry control exceeds35s including published30s clock skew");
        tokio::time::sleep(Duration::from_millis(delay)).await;
        for (token, expected, id) in [(&valid, None, &ids[0]),
            (&invalid, Some("invalid_token"), &ids[0]),
            (&expired, Some("token_expired"), &ids[0]),
            (&valid, Some("insufficient_permissions"), &ids[1])] {
            let client = Client::new(ClientConfig::new(&fixture.endpoint).namespace(&fixture.namespace)
                .pool_name(&fixture.pool).token(token).disable_metadata_lookup(true));
            let checked = async {
                let actor = TypedActorHandle::<AgentOsActor>::new(client.get_for_id(ACTOR_NAME, id, Default::default())?);
                check_authorization(actor.call(VmStatus {}), expected).await?;
                let connection: TypedActorConnection<AgentOsActor> = actor.connect();
                let checked = check_authorization(connection.call(VmStatus {}), expected).await;
                tokio::time::timeout(Duration::from_secs(5), connection.disconnect()).await
                    .context("owned JWT connection cleanup exceeded5s")?;
                checked
            }.await;
            client.dispose();
            checked?;
        }
        Ok::<_, anyhow::Error>(())
    }).await.context("owned Rust JWT controls exceeded90s").and_then(|value| value);
    owner.dispose();
    let mut errors = result
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    for id in ids {
        match fixture
            .request(
                reqwest::Method::DELETE,
                &format!("/actors/{id}?namespace={}", fixture.namespace),
                None,
                &fixture.token,
            )
            .await
        {
            Ok((status, _)) if status.is_success() => {}
            Ok((status, body)) => errors.push(format!(
                "owned JWT actor cleanup failed: {status}; {}",
                cleanup_response_diagnostic(&body)
            )),
            Err(error) => errors.push(format!("owned JWT actor cleanup: {error:#}")),
        }
    }
    anyhow::ensure!(errors.is_empty(), "{}", errors.join("; "));
    Ok(())
}

// Only machine-code fields are safe to retain: messages, metadata, and IDs may contain
// credentials or fixture-specific identifiers. This projection is always below 2KiB.
fn cleanup_response_diagnostic(body: &serde_json::Value) -> serde_json::Value {
    let error = body
        .get("error")
        .filter(|value| value.is_object())
        .unwrap_or(body);
    let safe_identifier = |field: &str| {
        error
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| {
                value.len() <= 128
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
            })
            .unwrap_or("<omitted>")
            .to_owned()
    };
    serde_json::json!({"group": safe_identifier("group"), "code": safe_identifier("code")})
}

async fn check_authorization<T>(
    operation: impl std::future::Future<Output = Result<T>>,
    code: Option<&str>,
) -> Result<()> {
    let result = tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .context(
            "owned authorization control exceeded10s; timeout is not authentication evidence",
        )?;
    match (result, code) {
        (Ok(_), None) => Ok(()),
        (Err(error), Some(code)) => {
            anyhow::ensure!(
                format!("{error:#}").contains(code),
                "expected authorization code {code}, got {error:#}"
            );
            Ok(())
        }
        (Err(error), None) => Err(error),
        (Ok(_), Some(code)) => {
            anyhow::bail!("forbidden operation unexpectedly succeeded; expected {code}")
        }
    }
}

/// Requires an owned local control plane/worker; its owner removes all test state.
#[tokio::test]
#[ignore = "explicit hosted integration: requires a matching native worker and local control plane"]
async fn native_hosted_transport_preserves_bytes_and_exact_integer_errors() -> Result<()> {
    anyhow::ensure!(
        std::env::var("AGENTOS_HOSTED_TEST_REQUIRE_LOCAL").as_deref() == Ok("1"),
        "set AGENTOS_HOSTED_TEST_REQUIRE_LOCAL=1 only for an owned local hosted fixture"
    );
    let client = Client::new(
        ClientConfig::new(std::env::var("AGENTOS_HOSTED_TEST_ENDPOINT")?)
            .namespace(std::env::var("AGENTOS_HOSTED_TEST_NAMESPACE")?)
            .pool_name(std::env::var("AGENTOS_HOSTED_TEST_POOL")?)
            .disable_metadata_lookup(true)
            .token(std::env::var("AGENTOS_HOSTED_TEST_TOKEN")?),
    );
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let actor = client.get_or_create_typed::<AgentOsActor>(
            ACTOR_NAME,
            [format!("transport-rust-{}", uuid::Uuid::new_v4())],
            owned_actor_input()?,
        )?;
        let bytes = vec![0, 255, 128, 65, 10];
        actor
            .call(FilesystemWriteFile {
                path: "/workspace/transport.bin".into(),
                content: FileContentInput::Bytes(bytes.clone()),
            })
            .await?;
        let read = actor
            .call(FilesystemReadFile {
                path: "/workspace/transport.bin".into(),
                max_bytes: Some(16),
            })
            .await?;
        anyhow::ensure!(
            read.0 == bytes,
            "binary output changed across hosted transport"
        );
        let before = actor.call(ConfigGet {}).await?;
        let after = actor.call(ConfigPatch {
            patch: serde_json::json!({ "environment": { "HOSTED_TRANSPORT_CONTROL": "checked" } }),
            expected_revision: Some(before.revision),
        }).await?;
        anyhow::ensure!(
            after.revision == before.revision + 1,
            "revision did not advance once"
        );
        let conflict = actor
            .call(ConfigPatch {
                patch: serde_json::json!({}),
                expected_revision: Some(9_007_199_254_740_993),
            })
            .await
            .expect_err("exact revision must reject a mismatch");
        anyhow::ensure!(
            format!("{conflict:#}").contains("revision_conflict"),
            "wrong revision error: {conflict:#}"
        );
        let stale = actor
            .call(ProcessOutputRead {
                process: ActorProcessId {
                    generation: 9_007_199_254_740_993,
                    pid: 1,
                },
                after: Some(u64::MAX),
                max_events: Some(1),
                max_bytes: Some(1024),
            })
            .await
            .expect_err("exact stale generation must reject the handle");
        anyhow::ensure!(
            format!("{stale:#}").contains("stale_generation"),
            "wrong generation error: {stale:#}"
        );
        for patch in [
            serde_json::json!({ "sidecarBinaryPath": "/not-a-host-input" }),
            serde_json::json!({ "bindings": { "synthetic": {} } }),
            serde_json::json!({ "software": [{ "path": "/not-a-host-package" }] }),
        ] {
            let error = actor
                .call(ConfigPatch {
                    patch,
                    expected_revision: None,
                })
                .await
                .expect_err("host-only configuration must be rejected");
            anyhow::ensure!(
                format!("{error:#}").contains("invalid_input"),
                "wrong hosted rejection: {error:#}"
            );
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("timeout: hosted Rust transport exceeded its 90 second fixture budget")?;
    client.dispose();
    result
}

fn owned_actor_input() -> Result<GetOrCreateOptions> {
    let input = std::env::var("AGENTOS_HOSTED_TEST_SOFTWARE_JSON")?;
    anyhow::ensure!(
        input.len() <= 64 * 1024,
        "fixture software JSON exceeds64KiB"
    );
    let software: serde_json::Value = serde_json::from_str(&input)?;
    let entries = software
        .as_array()
        .context("fixture software must be an array")?;
    anyhow::ensure!(
        !entries.is_empty() && entries.len() <= 16,
        "fixture software must have1..16 entries"
    );
    Ok(GetOrCreateOptions {
        create_with_input: Some(serde_json::json!({
            "config": { "software": software, "filesystem": { "mounts": [
                { "path": "/workspace", "backend": { "type": "actor-sqlite" }, "readOnly": false }
            ] } }
        })),
        ..Default::default()
    })
}

#[tokio::test]
#[ignore = "explicit hosted integration: owned local worker, matching software and durable actor SQLite"]
async fn native_hosted_retained_language_stream_cancel_and_restart() -> Result<()> {
    use agentos_actor::{
        ActorExecutionOutputOptions, ActorInlineExecutionOptions, ActorJavaScriptExecutionOptions,
        ActorLanguageExecutionOptions, ActorOutputCapture, ActorSignal, ContextsCreate,
        ContextsDelete, JavaScriptEvaluate, JavaScriptExecute, ProcessSignal, ProcessSpawn,
        ProcessWait, VmRestart, VmStatus,
    };
    anyhow::ensure!(
        std::env::var("AGENTOS_HOSTED_TEST_REQUIRE_LOCAL").as_deref() == Ok("1"),
        "explicit owned hosted fixture required"
    );
    let client = Client::new(
        ClientConfig::new(std::env::var("AGENTOS_HOSTED_TEST_ENDPOINT")?)
            .namespace(std::env::var("AGENTOS_HOSTED_TEST_NAMESPACE")?)
            .pool_name(std::env::var("AGENTOS_HOSTED_TEST_POOL")?)
            .token(std::env::var("AGENTOS_HOSTED_TEST_TOKEN")?)
            .disable_metadata_lookup(true),
    );
    let result =
        tokio::time::timeout(Duration::from_secs(90), async {
            let actor = client.get_or_create_typed::<AgentOsActor>(
                ACTOR_NAME,
                [format!("lifecycle-rust-{}", uuid::Uuid::new_v4())],
                owned_actor_input()?,
            )?;
            let context = actor
                .call(ContextsCreate {
                    context_id: "retained-control".into(),
                })
                .await?;
            let options = ActorJavaScriptExecutionOptions {
                inline: ActorInlineExecutionOptions {
                    process: ActorLanguageExecutionOptions {
                        context: Some(context.context.clone()),
                        timeout_ms: Some(10_000),
                        output: ActorExecutionOutputOptions {
                            capture: Some(ActorOutputCapture::All),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            };
            let execute = actor
                .call(JavaScriptExecute {
                    source: "globalThis.__hostedValue = 41".into(),
                    options: options.clone(),
                })
                .await?;
            anyhow::ensure!(
                execute.error.is_none(),
                "retained execution failed: {:?}",
                execute.error
            );
            let evaluated = actor
                .call(JavaScriptEvaluate {
                    expression: "globalThis.__hostedValue + 1".into(),
                    options,
                })
                .await?;
            anyhow::ensure!(
                evaluated.value == Some(serde_json::json!(42)),
                "retained value changed: {:?}",
                evaluated.value
            );
            actor
                .call(ContextsDelete {
                    context: context.context,
                })
                .await?;
            let process =
                actor
                    .call(ProcessSpawn {
                        command: "node".into(),
                        args: vec!["-e".into(),
            "process.stdout.write(Buffer.from([0,255,128]));setInterval(()=>{},1000)".into()],
                        options: Default::default(),
                    })
                    .await?;
            let end = tokio::time::Instant::now() + Duration::from_secs(10);
            let page = loop {
                let page = actor
                    .call(ProcessOutputRead {
                        process,
                        after: None,
                        max_events: Some(16),
                        max_bytes: Some(1024),
                    })
                    .await?;
                let stdout = page
                    .events
                    .iter()
                    .filter(|event| event.stream == agentos_client::ProcessStream::Stdout)
                    .flat_map(|event| event.data.0.iter().copied())
                    .collect::<Vec<_>>();
                if stdout == [0, 255, 128] {
                    break page;
                }
                anyhow::ensure!(
                    tokio::time::Instant::now() < end,
                    "stream output deadline exceeded"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            anyhow::ensure!(!page.events.is_empty(), "stream was empty");
            actor
                .call(ProcessSignal {
                    process,
                    signal: ActorSignal::Term,
                })
                .await?;
            let exit = actor.call(ProcessWait { process }).await?;
            anyhow::ensure!(
                exit.status.signal.map(|signal| signal.number()) == Some(15),
                "wrong cancelled process status: {:?}",
                exit.status
            );
            actor
                .call(FilesystemWriteFile {
                    path: "/workspace/durable.bin".into(),
                    content: FileContentInput::Bytes(vec![0, 255, 128, 42]),
                })
                .await?;
            let old = actor.call(VmStatus {}).await?;
            let restarted = actor.call(VmRestart {}).await?;
            anyhow::ensure!(
                restarted.generation > old.generation,
                "restart did not advance generation"
            );
            let bytes = actor
                .call(FilesystemReadFile {
                    path: "/workspace/durable.bin".into(),
                    max_bytes: Some(16),
                })
                .await?;
            anyhow::ensure!(
                bytes.0 == [0, 255, 128, 42],
                "durable bytes changed after restart"
            );
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("timeout: hosted Rust lifecycle exceeded90s")?;
    client.dispose();
    result
}
