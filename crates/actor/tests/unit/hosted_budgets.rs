use super::*;
use base64::Engine;

fn actor(options: serde_json::Value) -> AgentOsActor {
    let options = serde_json::from_value(options).unwrap();
    AgentOsActor::with_budgets(
        RuntimeController::new("hosted-budget-control", 1),
        Arc::new(HostedActorBudgets::normalize(options).unwrap()),
    )
}
fn raised() -> AgentOsActor {
    actor(serde_json::json!({
        "admission": {"maxConcurrentActions": 80},
        "transport": {"maxMessageBytes": 8 * 1024 * 1024},
        "codec": {"maxBinaryBytes": 2 * 1024 * 1024, "maxInputValues": 2 * 1024 * 1024},
        "filesystem": {"maxTransferBytes": 2 * 1024 * 1024}
    }))
}
fn violation(error: &anyhow::Error, setting: &'static str) {
    let typed = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
        .expect("actor rejects with its typed owning budget");
    assert_eq!(typed.setting, setting);
    assert!(error.to_string().contains("--actor-budgets-json"));
}

#[test]
fn default_budget_options_are_compatible_and_invalid_overrides_reject() {
    let empty: HostedActorBudgetOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(
        HostedActorBudgets::normalize(empty.clone()).unwrap(),
        HostedActorBudgets::normalize(Default::default()).unwrap()
    );
    let resolved = HostedActorBudgets::normalize(empty).unwrap();
    assert_eq!(resolved.max_concurrent_actions, 64);
    assert_eq!(resolved.max_message_bytes, 1024 * 1024);
    assert_eq!(resolved.codec.max_binary_bytes, 768 * 1024);
    assert_eq!(resolved.max_transfer_bytes, 768 * 1024);
    for json in [
        serde_json::json!({"admission":{"maxConcurrentActions":0}}),
        serde_json::json!({"transport":{"maxMessageBytes":0}}),
        serde_json::json!({"transport":{"maxMessageBytes":1024}}),
        serde_json::json!({"transport":{"maxMessageBytes":u64::MAX}}),
        serde_json::json!({"codec":{"maxBinaryBytes":u64::MAX}}),
        serde_json::json!({"filesystem":{"maxTransferBytes":0}}),
    ] {
        assert!(HostedActorBudgets::normalize(serde_json::from_value(json).unwrap()).is_err());
    }
    assert_eq!(
        HostedActorBudgets::normalize(
            serde_json::from_value(
                serde_json::json!({"transport":{"maxMessageBytes":33 * 1024 * 1024}})
            )
            .unwrap()
        )
        .unwrap()
        .max_message_bytes,
        33 * 1024 * 1024
    );
    assert!(
        serde_json::from_str::<HostedActorBudgetOptions>(r#"{"filesystem":{"unknown":2}}"#)
            .is_err()
    );
    let options: HostedActorBudgetOptions =
        serde_json::from_str(r#"{"codec":{"maxBinaryBytes":2097152}}"#).unwrap();
    assert_eq!(
        serde_json::from_value::<HostedActorBudgetOptions>(serde_json::to_value(&options).unwrap())
            .unwrap(),
        options
    );
}

#[test]
fn production_actor_admission_uses_owner_and_returns_all_permits() {
    let default = actor(serde_json::json!({}));
    let permits = (0..64)
        .map(|_| default.admit_action().unwrap())
        .collect::<Vec<_>>();
    let default_error = default.admit_action().unwrap_err();
    drop(permits);
    assert_eq!(default.action_admission.available_permits(), 64);
    let configured = raised();
    let permits = (0..65)
        .map(|_| {
            configured
                .admit_action()
                .expect("configured actor admits a real 65th action")
        })
        .collect::<Vec<_>>();
    assert_eq!(configured.action_admission.available_permits(), 15);
    drop(permits);
    assert_eq!(configured.action_admission.available_permits(), 80);
    violation(&default_error, "actor.admission.maxConcurrentActions");
}

#[test]
fn production_registration_config_uses_owner_and_preserves_deadline() {
    let configured = raised();
    let config = hosted_actor_config_with_budgets(&configured.budgets);
    assert_eq!(config.max_incoming_message_size, 8 * 1024 * 1024);
    assert_eq!(config.max_outgoing_message_size, 8 * 1024 * 1024);
    assert_eq!(config.action_timeout, Duration::from_secs(6 * 60));
}

#[test]
fn production_dispatch_policy_decodes_native_and_client_tag_binary() {
    let bytes = (0..800 * 1024)
        .map(|index| ((index * 31 + 17) & 255) as u8)
        .collect::<Vec<_>>();
    let action = FilesystemWriteFile {
        path: "/tmp/above-default.bin".into(),
        content: FileContentInput::Bytes(bytes.clone()),
    };
    // Rust client encoding preserves JSON-shaped numeric arrays, not native CBOR bytes.
    let rust_array = rivetkit::action::encode_positional(&action).unwrap();
    let array_value: ciborium::Value = ciborium::from_reader(rust_array.as_slice()).unwrap();
    let argument = &array_value.as_array().unwrap()[0];
    let content = argument
        .as_map()
        .unwrap()
        .iter()
        .find(|(key, _)| key.as_text() == Some("content"))
        .unwrap()
        .1
        .as_array()
        .unwrap();
    assert_eq!(content.len(), bytes.len());
    for (value, expected) in content.iter().zip(&bytes) {
        assert_eq!(
            value.as_integer().map(i128::from),
            Some(i128::from(*expected))
        );
    }
    // This separate fixture exercises supported native CBOR codec input, not client transport.
    let native_value = ciborium::Value::Array(vec![ciborium::Value::Map(vec![
        (
            ciborium::Value::Text("path".into()),
            ciborium::Value::Text(action.path.clone()),
        ),
        (
            ciborium::Value::Text("content".into()),
            ciborium::Value::Bytes(bytes.clone()),
        ),
    ])]);
    let mut native = Vec::new();
    ciborium::into_writer(&native_value, &mut native).unwrap();
    assert!(native.len() < 1024 * 1024);
    assert!(rust_array.len() > 1024 * 1024);
    assert!(rust_array.len() < 2 * 1024 * 1024);

    let mut tagged = Vec::new();
    ciborium::into_writer(&serde_json::json!([{
        "path": action.path, "content": ["$Uint8Array", base64::engine::general_purpose::STANDARD.encode(&bytes)]
    }]), &mut tagged).unwrap();
    let default = actor(serde_json::json!({}));
    let configured = raised();
    for encoded in [&native, &tagged, &rust_array] {
        let decoded: FilesystemWriteFile = agentos_actor_contract::decode_action_input_with_limits(
            encoded,
            action_set::actor_codec_limits(&configured),
        )
        .expect("actual dispatcher policy admits raised binary input");
        assert_eq!(decoded.content, FileContentInput::Bytes(bytes.clone()));
        let error = agentos_actor_contract::decode_action_input_with_limits::<FilesystemWriteFile>(
            encoded,
            action_set::actor_codec_limits(&default),
        )
        .unwrap_err();
        let typed = error
            .downcast_ref::<agentos_actor_contract::ActionBudgetViolation>()
            .unwrap();
        assert!(matches!(
            typed.setting,
            "actor.codec.maxBinaryBytes"
                | "actor.transport.maxMessageBytes"
                | "actor.codec.maxInputValues"
        ));
    }
    let values_not_raised = actor(
        serde_json::json!({"transport":{"maxMessageBytes":8 * 1024 * 1024},"codec":{"maxBinaryBytes":2 * 1024 * 1024}}),
    );
    let error = agentos_actor_contract::decode_action_input_with_limits::<FilesystemWriteFile>(
        &rust_array,
        action_set::actor_codec_limits(&values_not_raised),
    )
    .unwrap_err();
    violation(&error, "actor.codec.maxInputValues");
    let binary_only = actor(serde_json::json!({"codec":{"maxBinaryBytes":2 * 1024 * 1024}}));
    let error = agentos_actor_contract::decode_action_input_with_limits::<FilesystemWriteFile>(
        &tagged,
        action_set::actor_codec_limits(&binary_only),
    )
    .unwrap_err();
    violation(&error, "actor.transport.maxMessageBytes");
}

#[test]
fn production_precommit_encoder_and_transfer_guards_use_same_owner() {
    let default = actor(serde_json::json!({}));
    let configured = raised();
    let response = "x".repeat(1024 * 1024 + 17);
    violation(
        &default.encode_budgeted_output(&response).unwrap_err(),
        "actor.transport.maxMessageBytes",
    );
    let encoded = configured.encode_budgeted_output(&response).unwrap();
    let roundtrip: String = ciborium::from_reader(encoded.as_slice()).unwrap();
    assert_eq!(roundtrip, response);
    let bytes = 800 * 1024;
    let default_limit = default.selected_transfer_limit(None).unwrap();
    violation(
        &default
            .validate_transfer_bytes("file content", bytes, default_limit)
            .unwrap_err(),
        "actor.filesystem.maxTransferBytes",
    );
    let configured_limit = configured.selected_transfer_limit(None).unwrap();
    configured
        .validate_transfer_bytes("file content", bytes, configured_limit)
        .unwrap();
    assert_eq!(
        configured.selected_transfer_limit(Some(1024)).unwrap(),
        1024
    );
    assert!(configured
        .validate_transfer_bytes("caller-selected limit", 1025, 1024)
        .is_err());
    let caller_error = configured
        .validate_transfer_bytes("caller cap", 1025, 1024)
        .unwrap_err();
    assert_eq!(
        caller_error
            .downcast_ref::<agentos_actor_contract::ActionBudgetViolation>()
            .unwrap()
            .setting,
        "maxBytes"
    );
    assert!(!caller_error
        .to_string()
        .contains("raise maxBytes in --actor-budgets-json"));
    assert!(configured
        .validate_transfer_bytes(
            "worker cap cannot be bypassed",
            2 * 1024 * 1024 + 1,
            usize::MAX
        )
        .is_err());
    assert!(configured.selected_transfer_limit(Some(0)).is_err());
    assert!(configured
        .selected_transfer_limit(Some(2 * 1024 * 1024 + 1))
        .is_err());
}

#[derive(Clone)]
struct WarningObserver(Arc<std::sync::Mutex<Vec<std::collections::BTreeMap<String, String>>>>);
struct WarningFields(std::collections::BTreeMap<String, String>);
impl tracing::field::Visit for WarningFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().into(), value.to_string());
    }
}
impl tracing::Subscriber for WarningObserver {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = WarningFields(Default::default());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn admission_warning_reports_actual_occupancy_and_worker_remediation() {
    let records = Arc::new(std::sync::Mutex::new(Vec::new()));
    tracing::subscriber::with_default(WarningObserver(Arc::clone(&records)), || {
        let configured = raised();
        let permits = (0..64)
            .map(|_| configured.admit_action().unwrap())
            .collect::<Vec<_>>();
        drop(permits);
    });
    assert!(
        records.lock().unwrap().iter().any(|fields| {
            fields
                .get("setting")
                .is_some_and(|v| v == "actor.admission.maxConcurrentActions")
                && fields
                    .get("observed")
                    .and_then(|v| v.parse::<usize>().ok())
                    .is_some_and(|n| (64..=80).contains(&n))
                && fields.get("limit").is_some_and(|v| v == "80")
                && fields.get("units").is_some_and(|v| v == "actions")
                && fields
                    .get("message")
                    .is_some_and(|v| v.contains("--actor-budgets-json"))
        }),
        "warning must expose actual occupancy, configured capacity, units and operator remediation"
    );
}

#[test]
fn typed_budget_failure_keeps_public_limit_code_and_machine_quantities() {
    let default = actor(serde_json::json!({}));
    let limit = default.selected_transfer_limit(None).unwrap();
    let error = default
        .validate_transfer_bytes("file content", limit + 1, limit)
        .unwrap_err();
    let public = action_set::classify_public_error(error);
    let public = public.downcast_ref::<rivet_error::RivetError>().unwrap();
    assert!(
        matches!(&public.kind, rivet_error::RivetErrorKind::Dynamic { code, .. } if code == "limit_exceeded")
    );
    let metadata: serde_json::Value =
        serde_json::from_str(public.meta.as_ref().unwrap().get()).unwrap();
    assert_eq!(
        metadata["budget"]["setting"],
        "actor.filesystem.maxTransferBytes"
    );
    assert_eq!(metadata["budget"]["observed"], limit + 1);
    assert_eq!(metadata["budget"]["limit"], limit);
    assert_eq!(metadata["budget"]["units"], "bytes");
}

#[test]
fn production_dispatch_policy_raises_depth_and_value_count_and_warns_once() {
    let configured =
        actor(serde_json::json!({"codec":{"maxInputDepth":80,"maxInputValues":140000}}));
    let default = actor(serde_json::json!({}));
    let mut nested = serde_json::Value::Null;
    for _ in 0..70 {
        nested = serde_json::Value::Array(vec![nested]);
    }
    let shallow_many = serde_json::Value::Array(vec![serde_json::Value::Null; 132000]);
    for (value, setting) in [
        (nested, "actor.codec.maxInputDepth"),
        (shallow_many, "actor.codec.maxInputValues"),
    ] {
        let encoded = rivetkit::action::encode_positional(&value).unwrap();
        let records = Arc::new(std::sync::Mutex::new(Vec::new()));
        let decoded: serde_json::Value =
            tracing::subscriber::with_default(WarningObserver(Arc::clone(&records)), || {
                agentos_actor_contract::decode_action_input_with_limits(
                    &encoded,
                    action_set::actor_codec_limits(&configured),
                )
            })
            .expect("production-selected raised codec policy admits formerly rejected input");
        let records = records.lock().unwrap();
        let depth_warnings: Vec<_> = records
            .iter()
            .filter(|fields| {
                fields
                    .get("setting")
                    .is_some_and(|s| s == "actor.codec.maxInputDepth")
            })
            .collect();
        if setting == "actor.codec.maxInputDepth" {
            assert_eq!(
                depth_warnings.len(),
                1,
                "one peak warning, not one warning per recursive node"
            );
            let fields = depth_warnings[0];
            assert_eq!(fields.get("limit").unwrap(), "80");
            assert_eq!(fields.get("units").unwrap(), "levels");
            assert!((70..=71).contains(&fields.get("observed").unwrap().parse::<usize>().unwrap()));
        } else {
            assert!(depth_warnings.is_empty());
        }
        drop(records);
        assert_eq!(decoded, value);
        let error = agentos_actor_contract::decode_action_input_with_limits::<serde_json::Value>(
            &encoded,
            action_set::actor_codec_limits(&default),
        )
        .unwrap_err();
        violation(&error, setting);
    }
    for (value, setting) in [
        (
            serde_json::Value::Array(vec![serde_json::Value::Null; 140001]),
            "actor.codec.maxInputValues",
        ),
        (
            {
                let mut value = serde_json::Value::Null;
                for _ in 0..81 {
                    value = serde_json::Value::Array(vec![value]);
                }
                value
            },
            "actor.codec.maxInputDepth",
        ),
    ] {
        let encoded = rivetkit::action::encode_positional(&value).unwrap();
        let error = agentos_actor_contract::decode_action_input_with_limits::<serde_json::Value>(
            &encoded,
            action_set::actor_codec_limits(&configured),
        )
        .unwrap_err();
        violation(&error, setting);
    }
}
