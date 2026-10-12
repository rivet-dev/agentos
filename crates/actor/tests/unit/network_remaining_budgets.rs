use super::*;
fn budgets(value: serde_json::Value) -> crate::HostedActorBudgets {
    crate::HostedActorBudgets::normalize(serde_json::from_value(value).unwrap()).unwrap()
}
fn violation(error: &anyhow::Error, setting: &str) {
    let value = error
        .chain()
        .find_map(|e| e.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
        .expect("typed budget failure");
    assert_eq!(value.setting, setting);
    assert!(value.to_string().contains("--actor-budgets-json"));
}

#[test]
fn network_request_and_response_converters_use_configured_budgets() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"network":{"maxHttpPathBytes":20000,"maxHttpMethodBytes":64,"maxHttpHeaders":256,"maxHttpHeaderBytes":131072,"maxHttpBodyBytes":786432,"maxHttpResponseBytes":1048576}}),
    );
    let request = ActorHttpRequest {
        port: 80,
        path: "/".into(),
        method: "GET".into(),
        headers: BTreeMap::new(),
        body: None,
    };
    let variants = [
        (
            ActorHttpRequest {
                path: format!("/{}", "p".repeat(16384)),
                ..request.clone()
            },
            "actor.network.maxHttpPathBytes",
        ),
        (
            ActorHttpRequest {
                method: "M".repeat(33),
                ..request.clone()
            },
            "actor.network.maxHttpMethodBytes",
        ),
        (
            ActorHttpRequest {
                headers: (0..129).map(|n| (format!("x-{n}"), "v".into())).collect(),
                ..request.clone()
            },
            "actor.network.maxHttpHeaders",
        ),
        (
            ActorHttpRequest {
                headers: BTreeMap::from([("x".into(), "v".repeat(65536))]),
                ..request.clone()
            },
            "actor.network.maxHttpHeaderBytes",
        ),
        (
            ActorHttpRequest {
                body: Some(FileContentInput::Bytes(vec![17; 524289])),
                ..request
            },
            "actor.network.maxHttpBodyBytes",
        ),
    ];
    for (request, setting) in variants {
        violation(&request.validate(&default).unwrap_err(), setting);
        request.validate(&raised).unwrap();
    }
    let response = HttpResponse {
        status: 200,
        status_text: "OK".into(),
        headers: Vec::new(),
        body: vec![29; 786433],
    };
    violation(
        &actor_http_response(response.clone(), &default).unwrap_err(),
        "actor.network.maxHttpResponseBytes",
    );
    assert_eq!(
        actor_http_response(response.clone(), &raised)
            .unwrap()
            .body
            .0,
        response.body
    );
}

#[test]
fn stream_selection_preserves_caller_default_and_real_converters_use_worker_caps() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"network":{"maxStreamChunkBytes":262144,"maxStreamIdBytes":512,"maxStreamLifetimeMs":3600001,"maxPreviewTokenBytes":256}}),
    );
    assert_eq!(
        select_stream_bytes(None, &raised).unwrap(),
        65536,
        "no new default-read knob"
    );
    assert_eq!(select_stream_bytes(Some(1024), &raised).unwrap(), 1024);
    violation(
        &select_stream_bytes(Some(131073), &default).unwrap_err(),
        "actor.network.maxStreamChunkBytes",
    );
    assert_eq!(select_stream_bytes(Some(131073), &raised).unwrap(), 131073);
    let lower = budgets(serde_json::json!({"network":{"maxStreamChunkBytes":32768}}));
    violation(
        &select_stream_bytes(None, &lower).unwrap_err(),
        "actor.network.maxStreamChunkBytes",
    );
    assert_eq!(select_stream_bytes(Some(16384), &lower).unwrap(), 16384);
    let chunk = HttpStreamChunk {
        body: vec![7; 131073],
        done: false,
    };
    violation(
        &actor_stream_chunk(chunk.clone(), &default).unwrap_err(),
        "actor.network.maxStreamChunkBytes",
    );
    assert_eq!(
        actor_stream_chunk(chunk.clone(), &raised).unwrap().body.0,
        chunk.body
    );
    let head = HttpStreamHead {
        stream_id: "s".repeat(257),
        status: 200,
        status_text: "OK".into(),
        headers: Vec::new(),
    };
    violation(
        &actor_stream_head(1, head.clone(), &default).unwrap_err(),
        "actor.network.maxStreamIdBytes",
    );
    assert_eq!(
        actor_stream_head(1, head, &raised)
            .unwrap()
            .stream
            .generation,
        1
    );
    assert_eq!(stream_expiration_at(1000, &raised).unwrap(), 3601001);
    assert!(stream_expiration_at(i64::MAX, &raised).is_err());
    assert!(!stream_is_expired(3601001, 1000, &raised));
    assert!(stream_is_expired(3601001, 3601001, &raised));
    violation(
        &validate_token(&"a".repeat(129), &default).unwrap_err(),
        "actor.network.maxPreviewTokenBytes",
    );
    validate_token(&"a".repeat(129), &raised).unwrap();
    let too_small = budgets(serde_json::json!({"network":{"maxPreviewTokenBytes":1}}));
    violation(
        &generated_preview_token(&too_small).unwrap_err(),
        "actor.network.maxPreviewTokenBytes",
    );
    let token = generated_preview_token(&default).unwrap();
    assert_eq!(token.len(), 32);
    validate_token(&token, &default).unwrap();
    assert!(validate_token("../control", &raised).is_err());
}

#[test]
fn lower_saved_preview_choices_remain_distinct_from_worker_maxima() {
    let owner = budgets(serde_json::json!({"preview":{"maxTtlMs":172800000,"maxActive":256}}));
    let saved = crate::PreviewPolicy {
        default_ttl_ms: 1000,
        max_ttl_ms: 2000,
        max_active: 1,
    };
    assert_eq!(select_preview_ttl(saved, None, &owner).unwrap(), 1000);
    let error = select_preview_ttl(saved, Some(2001), &owner).unwrap_err();
    violation(&error, "config.preview.maxTtlMs");
    assert!(error
        .to_string()
        .contains("if the desired value exceeds the worker"));
    assert!(!error
        .to_string()
        .contains("raise config.preview.maxTtlMs in --actor-budgets-json"));
    let capacity = preview_capacity(saved, &owner);
    capacity.check("active previews", 1).unwrap();
    violation(
        &capacity.check("active previews", 2).unwrap_err(),
        "config.preview.maxActive",
    );
    let selected = crate::PreviewPolicy {
        default_ttl_ms: 1000,
        max_ttl_ms: 172800000,
        max_active: 256,
    };
    assert_eq!(
        select_preview_ttl(selected, Some(86400001), &owner).unwrap(),
        86400001
    );
    preview_capacity(selected, &owner)
        .check("active previews", 129)
        .unwrap();
}

#[tokio::test]
async fn rejected_stream_head_awaits_cancellation_of_the_exact_native_owner() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let owner = budgets(serde_json::json!({"network":{"maxStreamIdBytes":1}}));
    let live = Rc::new(RefCell::new(std::collections::BTreeSet::from([
        String::from("7:19"),
    ])));
    let native = HttpStreamHead {
        stream_id: "7:19".into(),
        status: 200,
        status_text: "OK".into(),
        headers: Vec::new(),
    };
    let cleanup = Rc::clone(&live);
    let error = publish_stream_head(7, native, &owner, move |raw_id| async move {
        tokio::task::yield_now().await;
        assert_eq!(raw_id, "7:19");
        assert!(cleanup.borrow_mut().remove(&raw_id));
        Ok(())
    })
    .await
    .unwrap_err();
    violation(&error, "actor.network.maxStreamIdBytes");
    assert!(
        live.borrow().is_empty(),
        "rejection must await owned cancellation before returning"
    );
    let owner = budgets(serde_json::json!({}));
    let valid = HttpStreamHead {
        stream_id: "7:20".into(),
        status: 200,
        status_text: "OK".into(),
        headers: Vec::new(),
    };
    let result = publish_stream_head(7, valid, &owner, |_| {
        std::future::ready(Err(anyhow::anyhow!(
            "success must publish without cancelling its live stream"
        )))
    })
    .await
    .unwrap();
    assert_eq!(result.stream.stream_id, "7:20");
}

#[tokio::test]
async fn rejected_stream_head_preserves_primary_and_reports_cancellation_failure() {
    let owner = budgets(serde_json::json!({"network":{"maxStreamIdBytes":1}}));
    let raw_id = std::sync::Arc::new(std::sync::Mutex::new(None));
    let observed = std::sync::Arc::clone(&raw_id);
    let native = HttpStreamHead {
        stream_id: "8:21".into(),
        status: 200,
        status_text: "OK".into(),
        headers: Vec::new(),
    };
    let error = publish_stream_head(8, native, &owner, move |id| async move {
        *observed.lock().unwrap() = Some(id);
        anyhow::bail!("synthetic native cancellation failure")
    })
    .await
    .unwrap_err();
    violation(&error, "actor.network.maxStreamIdBytes");
    assert_eq!(raw_id.lock().unwrap().as_deref(), Some("8:21"));
    assert!(format!("{error:#}").contains("synthetic native cancellation failure"));
    let classified = crate::action_set::classify_public_error(error);
    let classified = classified
        .downcast_ref::<rivet_error::RivetError>()
        .unwrap();
    let rivet_error::RivetErrorKind::Dynamic { code, .. } = &classified.kind else {
        panic!("typed public error");
    };
    assert_eq!(
        code, "limit_exceeded",
        "cleanup context must not replace primary classification"
    );
}

#[tokio::test]
#[ignore = "explicit native HTTP ownership integration; requires a captured sidecar and owned process controller"]
async fn rejected_stream_head_retires_the_actual_native_owner() -> anyhow::Result<()> {
    use agentos_client::config::{AgentOsConfig, PatternPermissions, PermissionMode, Permissions};
    use agentos_client::{AgentOs, ClientError, LanguageSpawnOptions};
    use std::time::Duration;

    fn require_budget(error: &anyhow::Error) -> anyhow::Result<()> {
        let violation = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
            .context("stream rejection lost its typed budget violation")?;
        anyhow::ensure!(
            violation.setting == "actor.network.maxStreamIdBytes",
            "stream rejected by a different budget: {error:#}"
        );
        Ok(())
    }

    let binary = std::env::var("AGENTOS_SIDECAR_BIN")
        .context("native stream ownership control requires AGENTOS_SIDECAR_BIN")?;
    anyhow::ensure!(
        std::path::Path::new(&binary).is_file(),
        "native sidecar is missing"
    );
    let mut owned_vm = None;
    let operation = tokio::time::timeout(Duration::from_secs(25), async {
        let vm = AgentOs::create(AgentOsConfig {
            defaults_profile: Some(agentos_vm_config::VmDefaultsProfile::Secure),
            sidecar_binary_path: Some(binary),
            permissions: Some(Permissions {
                network: Some(PatternPermissions::Mode(PermissionMode::Deny)),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await?;
        owned_vm = Some(vm.clone());
        vm.spawn_javascript(
            r#"
import http from 'node:http';
import fs from 'node:fs';
const openResponses = new Set();
const server = http.createServer((request, response) => {
  if (request.url === '/stream') {
    openResponses.add(response);
    response.once('close', () => openResponses.delete(response));
    response.writeHead(200, { 'content-type': 'text/plain' });
    response.write('begin');
  } else {
    response.end(String(openResponses.size));
  }
});
server.listen(31425, '127.0.0.1', () => fs.writeFileSync('/tmp/stream-owner-ready', 'ready'));
"#,
            LanguageSpawnOptions::default(),
        )
        .await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match vm.read_file("/tmp/stream-owner-ready").await {
                    Ok(bytes) => {
                        anyhow::ensure!(bytes == b"ready", "unexpected server readiness marker");
                        return Ok::<_, anyhow::Error>(());
                    }
                    Err(error) => {
                        anyhow::ensure!(
                            matches!(error.downcast_ref::<ClientError>(), Some(ClientError::Kernel { code, .. }) if code == "ENOENT"),
                            "server readiness failed: {error:#}"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .context("native HTTP server did not become ready within 5 seconds")??;

        let request = || HttpRequest {
            port: 31425,
            path: "/stream".into(),
            method: "GET".into(),
            headers: BTreeMap::new(),
            body: None,
        };
        let constrained = budgets(serde_json::json!({"network":{"maxStreamIdBytes":1}}));

        // The former handler converted the registered native head directly.
        // Its rejected reply left an owner that can still deliver real bytes.
        let before = vm.http_request_stream_start(request()).await?;
        let before_id = before.stream_id.clone();
        let original_error = actor_stream_head(1, before, &constrained).err()
            .context("negative control must reject the generated native ID")?;
        require_budget(&original_error)?;
        let retained = vm.http_request_stream_read(&before_id, 5).await?;
        anyhow::ensure!(retained.body == b"begin" && !retained.done,
            "the negative control must retain the actual native stream");
        vm.http_request_stream_cancel(&before_id).await?;

        let head = vm.http_request_stream_start(request()).await?;
        let raw_id = head.stream_id.clone();
        let cancel_vm = &vm;
        let expected_id = &raw_id;
        let error = publish_stream_head(1, head, &constrained, |id| async move {
            anyhow::ensure!(&id == expected_id, "cancel the exact native owner");
            cancel_vm.http_request_stream_cancel(&id).await
        })
        .await
        .err().context("the configured reply cap must still reject")?;
        require_budget(&error)?;
        let retired = vm.http_request_stream_read(&raw_id, 5).await.err()
            .context("rejected reply must retire its actual native stream before returning")?;
        anyhow::ensure!(format!("{retired:#}").contains("ERR_AGENTOS_VM_FETCH_STREAM_NOT_FOUND"),
            "preserve the native closed-owner diagnostic: {retired:#}");

        // A later valid publication proves the rejection did not close the VM
        // or unrelated HTTP ownership to make the missing-stream check pass.
        let normal = budgets(serde_json::json!({}));
        let next = vm.http_request_stream_start(request()).await?;
        let next = publish_stream_head(1, next, &normal, |_| async {
            anyhow::bail!("valid stream publication must not cancel")
        }).await?;
        let chunk = vm.http_request_stream_read(&next.stream.stream_id, 5).await?;
        anyhow::ensure!(chunk.body == b"begin" && !chunk.done, "later stream must remain usable");
        vm.http_request_stream_cancel(&next.stream.stream_id).await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = vm.http_request(HttpRequest { path: "/status".into(), ..request() }).await?;
                anyhow::ensure!(status.status == 200, "server status probe failed");
                if status.body == b"0" {
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.context("native cancellation did not close the guest responses within 3 seconds")??;
        Ok::<_, anyhow::Error>(())
    }).await.context("native HTTP ownership control exceeded 25 seconds").and_then(|result| result);

    let mut failures = operation
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    if let Some(vm) = owned_vm {
        match tokio::time::timeout(Duration::from_secs(5), vm.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(format!("VM cleanup: {error:#}")),
            Err(error) => failures.push(format!("VM cleanup deadline: {error}")),
        }
    }
    if let Err(error) =
        agentos_client::actor_internals::shutdown_runtime_shared_sidecars(Duration::from_secs(5))
            .await
    {
        failures.push(format!("sidecar owner cleanup: {error:#}"));
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
    Ok(())
}
