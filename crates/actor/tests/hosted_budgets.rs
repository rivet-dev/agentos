use agentos_actor::{
    ActorExecutionOutputOptions, ActorInlineExecutionOptions, ActorJavaScriptExecutionOptions,
    ActorJavaScriptModuleFormat, ActorLanguageExecutionOptions, ActorOutputCapture, AgentOsActor,
    ContextsCreate, ContextsDelete, FileContentInput, FilesystemReadFile, FilesystemWriteFile,
    JavaScriptExecute, VmLifecycleState, VmStatus, ACTOR_NAME,
};
use anyhow::{Context, Result};
use rivetkit::client::{Client, ClientConfig, GetOrCreateOptions};
use rivetkit::TypedClientExt;
use std::time::Duration;

// The pinned public Rust client decodes the wire metadata into Value, then
// exposes it through anyhow text, not a typed RivetError. Check that exact
// supported projection without pretending a structured downcast is available.
fn public_budget(
    error: anyhow::Error,
    setting: &str,
    observed: usize,
    limit: usize,
    remedy: &str,
) -> Result<()> {
    let message = format!("{error:#}");
    for token in [
        "(user/limit_exceeded)".to_owned(),
        format!("\"setting\": String(\"{setting}\")"),
        format!("\"observed\": Number({observed})"),
        format!("\"limit\": Number({limit})"),
        "\"units\": String(\"bytes\")".to_owned(),
        "\"remediation\": String(".to_owned(),
        remedy.to_owned(),
    ] {
        anyhow::ensure!(
            message.contains(&token),
            "public Rust budget projection omitted expected field {token}"
        );
    }
    Ok(())
}
async fn bridge_policy(
    actor: &rivetkit::TypedActorHandle<AgentOsActor>,
    raised: bool,
) -> Result<()> {
    let http = actor
        .call(JavaScriptExecute {
            source: include_str!("fixtures/hosted-http-policy.mjs")
                .replace("__RAISED__", if raised { "true" } else { "false" }),
            options: ActorJavaScriptExecutionOptions {
                inline: ActorInlineExecutionOptions {
                    process: ActorLanguageExecutionOptions {
                        timeout_ms: Some(15_000),
                        output: ActorExecutionOutputOptions {
                            capture: Some(ActorOutputCapture::All),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        })
        .await?;
    anyhow::ensure!(
        http.error.is_none() && http.exit_code == Some(0),
        "hosted HTTP policy execution failed: {:?}",
        http.error
    );
    anyhow::ensure!(
        http.stdout.as_ref().map(|v| v.0.as_slice()) == Some(&b"hosted-http-policy-complete\n"[..]),
        "hosted HTTP policy marker missing"
    );
    let context = actor
        .call(ContextsCreate {
            context_id: "hosted-bridge-policy".into(),
        })
        .await?;
    let body = async {
        for turn in 1..=3 {
            let source = include_str!("fixtures/hosted-readline-policy.cjs")
                .replace("__RAISED__", if raised { "true" } else { "false" })
                .replace("__TURN__", &turn.to_string());
            let result = actor
                .call(JavaScriptExecute {
                    source,
                    options: ActorJavaScriptExecutionOptions {
                        format: Some(ActorJavaScriptModuleFormat::CommonJs),
                        inline: ActorInlineExecutionOptions {
                            process: ActorLanguageExecutionOptions {
                                context: Some(context.context.clone()),
                                timeout_ms: Some(5_000),
                                output: ActorExecutionOutputOptions {
                                    capture: Some(ActorOutputCapture::All),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                })
                .await?;
            anyhow::ensure!(
                result.error.is_none() && result.exit_code == Some(0),
                "hosted retained policy failed at turn {turn}: {:?}",
                result.error
            );
            anyhow::ensure!(
                result.stdout.as_ref().map_or(0, |v| v.0.len()) <= 16 * 1024
                    && result.stderr.as_ref().map_or(0, |v| v.0.len()) <= 16 * 1024,
                "hosted retained output exceeded16KiB"
            );
            let expected = format!("hosted-bridge-turn-{turn}\n");
            anyhow::ensure!(
                result.stdout.as_ref().map(|v| v.0.as_slice()) == Some(expected.as_bytes()),
                "hosted retained policy marker missing at turn {turn}"
            );
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let cleanup = actor
        .call(ContextsDelete {
            context: context.context,
        })
        .await;
    match (body, cleanup) {
        (Ok(()), Ok(_)) => {}
        (body, cleanup) => {
            anyhow::bail!("hosted bridge policy body={body:?}; context cleanup={cleanup:?}")
        }
    }
    Ok(())
}

#[allow(dead_code)]
mod support;

#[tokio::test]
#[ignore = "explicit hosted budget integration: owned local engine and worker started with the selected startup policy"]
async fn hosted_budget_whole_file_rust() -> Result<()> {
    let fixture = support::Fixture::from_env()?;
    let case = std::env::var("AGENTOS_HOSTED_TEST_BUDGET_CASE")?;
    anyhow::ensure!(
        matches!(case.as_str(), "default" | "raised" | "vm-limited"),
        "unknown budget fixture case"
    );
    let sources = std::env::var("AGENTOS_HOSTED_TEST_SOFTWARE_JSON")?;
    anyhow::ensure!(
        sources.len() <= 64 * 1024,
        "fixture software JSON exceeds 64 KiB"
    );
    let software: serde_json::Value = serde_json::from_str(&sources)?;
    anyhow::ensure!(
        software
            .as_array()
            .is_some_and(|v| !v.is_empty() && v.len() <= 16),
        "fixture requires 1..16 software packages"
    );
    let mut config = serde_json::json!({"software": software, "filesystem": {"mounts": [
        {"path":"/workspace", "backend":{"type":"actor-sqlite"}, "readOnly":false}
    ]}});
    if case != "vm-limited" {
        config["permissions"] = serde_json::json!({"network":"allow"});
    }
    if case == "raised" {
        config["limits"] = serde_json::json!({"jsRuntime":{"maxReadlineHistoryEntries":2048},"http":{"maxChunkSizeLineBytes":32768}});
    }
    if case == "vm-limited" {
        config["limits"] = serde_json::json!({"resources":{"maxFilesystemBytes":1024 * 1024}});
    }
    let client = Client::new(
        ClientConfig::new(&fixture.endpoint)
            .namespace(&fixture.namespace)
            .pool_name(&fixture.pool)
            .token(&fixture.token)
            .disable_metadata_lookup(true),
    );
    let mut ids = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let actor = client.get_or_create_typed::<AgentOsActor>(
            ACTOR_NAME,
            [format!("budget-rust-{}", uuid::Uuid::new_v4())],
            GetOrCreateOptions {
                create_with_input: Some(serde_json::json!({"config":config})),
                ..Default::default()
            },
        )?;
        ids.push(actor.inner().resolve().await?);
        let status = actor.call(VmStatus {}).await?;
        anyhow::ensure!(
            status.lifecycle == VmLifecycleState::Ready,
            "budget fixture must boot before exercising transfer policy"
        );
        let transfer_bytes = if case == "vm-limited" {
            1536 * 1024
        } else {
            800 * 1024
        };
        let bytes = (0..transfer_bytes)
            .map(|i| ((i * 31 + 17) & 255) as u8)
            .collect::<Vec<_>>();
        let path = if case == "vm-limited" {
            "/tmp/budget-vm.bin"
        } else {
            "/workspace/budget.bin"
        };
        if case == "default" {
            let error = actor
                .call(FilesystemReadFile {
                    path: path.into(),
                    max_bytes: Some(768 * 1024 + 1),
                })
                .await
                .err()
                .context("default actor ceiling must reject a tiny public request")?;
            public_budget(
                error,
                "actor.filesystem.maxTransferBytes",
                768 * 1024 + 1,
                768 * 1024,
                "--actor-budgets-json",
            )?;
        }
        let write = actor
            .call(FilesystemWriteFile {
                path: path.into(),
                content: FileContentInput::Bytes(bytes.clone()),
            })
            .await;
        if case == "default" {
            let error = write
                .err()
                .context("default worker must reject the same above-default transfer")?;
            anyhow::ensure!(
                format!("{error:#}").contains("limit_exceeded")
                    || format!("{error:#}").contains("incoming_too_long"),
                "preserve actual default limit classification: {error:#}"
            );
            eprintln!("default Rust transfer rejected with actual classification: {error:#}");
        } else if case == "vm-limited" {
            let error = write.err().context(
                "raised actor policy must not override the smaller VM filesystem budget",
            )?;
            let message = format!("{error:#}");
            eprintln!(
                "owned VM quota Rust projection named_limit={} internal_masked={}",
                message.contains("limits.resources.maxFilesystemBytes"),
                message.contains("core/internal_error")
            );
            anyhow::ensure!(
                message.contains("limits.resources.maxFilesystemBytes"),
                "preserve owning VM limit diagnostic: {message}"
            );
            for token in [
                "(user/limit_exceeded)",
                "\"limit\": Object",
                "\"setting\": String(\"limits.resources.maxFilesystemBytes\")",
                "\"limit\": String(\"1048576\")",
                "\"units\": String(\"bytes\")",
                "\"observed\": String(\"",
                "\"remediation\": String(\"reduce filesystem usage",
            ] {
                anyhow::ensure!(
                    message.contains(token),
                    "public Rust quota projection omitted {token}"
                );
            }
            anyhow::ensure!(
                !message.contains("\"budget\":"),
                "VM quota must use metadata.limit only"
            );
            let observed_text = message
                .split("\"observed\": String(\"")
                .nth(1)
                .and_then(|tail| tail.split('"').next())
                .context("missing exact observed decimal string")?;
            let observed: u64 = observed_text
                .parse()
                .context("observed quota total is not u64")?;
            anyhow::ensure!(
                observed.to_string() == observed_text
                    && observed > 1024 * 1024
                    && observed >= bytes.len() as u64,
                "observed must be the exact canonical projected total above the VM limit"
            );
            anyhow::ensure!(
                !message.contains("actor.filesystem.maxTransferBytes"),
                "VM failure must not be relabeled as actor budget"
            );
        } else {
            write?;
            let read = actor
                .call(FilesystemReadFile {
                    path: path.into(),
                    max_bytes: Some(bytes.len()),
                })
                .await?;
            anyhow::ensure!(
                read.0 == bytes,
                "every byte must roundtrip above the unchanged default 768 KiB transfer cap"
            );
            let error = actor
                .call(FilesystemReadFile {
                    path: path.into(),
                    max_bytes: Some(bytes.len() - 1),
                })
                .await
                .err()
                .context("per-call read cap must reject the complete existing file")?;
            public_budget(
                error,
                "maxBytes",
                bytes.len(),
                bytes.len() - 1,
                "per-call maxBytes",
            )?;
            anyhow::ensure!(
                actor
                    .call(FilesystemReadFile {
                        path: path.into(),
                        max_bytes: Some(bytes.len())
                    })
                    .await?
                    .0
                    == bytes,
                "rejected read must preserve every stored byte"
            );
        }
        if case != "vm-limited" {
            bridge_policy(&actor, case == "raised").await?;
            if case == "raised" {
                let ordinary = client.get_or_create_typed::<AgentOsActor>(ACTOR_NAME, [format!("budget-rust-default-{}", uuid::Uuid::new_v4())], GetOrCreateOptions {
                    create_with_input: Some(serde_json::json!({"config":{"software":software,"permissions":{"network":"allow"}}})), ..Default::default()
                })?;
                ids.push(ordinary.inner().resolve().await?);
                bridge_policy(&ordinary, false).await?;
            }
        }
        anyhow::ensure!(
            actor.call(VmStatus {}).await?.lifecycle == VmLifecycleState::Ready,
            "VM must remain ready after rejected action"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("hosted budget Rust control exceeded its existing 90s fixture bound")
    .and_then(|v| v);
    client.dispose();
    let mut errors = result
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    for id in ids {
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
            Ok(Ok((status, _))) => {
                errors.push(format!("owned budget actor cleanup rejected: {status}"))
            }
            Ok(Err(error)) => errors.push(format!("owned budget actor cleanup failed: {error:#}")),
            Err(_) => errors.push("owned budget actor cleanup exceeded5s".into()),
        }
    }
    anyhow::ensure!(errors.is_empty(), "{}", errors.join("; "));
    Ok(())
}
