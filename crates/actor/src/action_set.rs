use std::sync::Arc;

pub(crate) use agentos_actor_contract::classify_public_error;
#[cfg(test)]
use agentos_actor_contract::{
    decode_action_input as decode_contract_input, normalize_client_tags, MAX_ACTION_INPUT_DEPTH,
    MAX_ACTION_INPUT_NODES,
};
use agentos_actor_contract::{dispatch_typed as dispatch_contract_action, DispatchFuture};
use anyhow::Result;
#[cfg(test)]
use rivet_error::{RivetError, RivetErrorKind};
use rivetkit::{Action, ActionEntry, ActionSet, Ctx, Handles};

use crate::AgentOsActor;

/// Product-owned typed action registry.
///
/// This is intentionally independent of RivetKit's tuple implementations so
/// the agentOS contract can grow without turning API groups into raw router
/// actions. Step 12 consumes this same registry for prototype TypeScript
/// binding generation.
pub struct AgentOsActionSet;

pub(crate) fn dispatch_typed<A>(
    actor: Arc<AgentOsActor>,
    ctx: Ctx<AgentOsActor>,
    args: &[u8],
) -> DispatchFuture
where
    A: Action,
    AgentOsActor: Handles<A>,
{
    dispatch_contract_action::<AgentOsActor, A>(
        actor,
        ctx,
        args,
        crate::ACTOR_MESSAGE_SIZE_LIMIT as usize,
    )
}

pub(crate) struct RegisteredAction {
    pub(crate) name: &'static str,
    pub(crate) dispatch: fn(Arc<AgentOsActor>, Ctx<AgentOsActor>, &[u8]) -> DispatchFuture,
    #[cfg(feature = "contract")]
    pub(crate) contract: fn() -> agentos_actor_contract::schema::ActionContract,
    #[cfg(feature = "contract")]
    pub(crate) collect_types: fn(&mut agentos_actor_contract::schema::TypeCollector),
}

inventory::collect!(RegisteredAction);

#[cfg(feature = "contract")]
pub(crate) fn contract_for<A>() -> agentos_actor_contract::schema::ActionContract
where
    A: Action + ts_rs::TS,
    A::Output: ts_rs::TS,
{
    agentos_actor_contract::schema::ActionContract {
        name: A::NAME,
        public: !A::NAME.starts_with("__"),
        input: agentos_actor_contract::schema::input::<A>(),
        output: agentos_actor_contract::schema::output::<A::Output>(),
    }
}

#[cfg(feature = "contract")]
pub(crate) fn collect_types_for<A>(types: &mut agentos_actor_contract::schema::TypeCollector)
where
    A: Action + ts_rs::TS + 'static,
    A::Output: ts_rs::TS + 'static,
{
    if !A::NAME.starts_with("__") {
        types.collect::<A>();
        types.collect::<A::Output>();
    }
}

#[macro_export]
macro_rules! register_action {
    ($action:ty => $output:ty, $name:literal) => {
        impl rivetkit::Action for $action {
            type Output = $output;
            const NAME: &'static str = $name;
        }

        inventory::submit! {
            $crate::action_set::RegisteredAction {
                name: $name,
                dispatch: $crate::action_set::dispatch_typed::<$action>,
                #[cfg(feature = "contract")]
                contract: $crate::action_set::contract_for::<$action>,
                #[cfg(feature = "contract")]
                collect_types: $crate::action_set::collect_types_for::<$action>,
            }
        }
    };
}

#[cfg(test)]
fn decode_action_input<A: serde::de::DeserializeOwned>(args: &[u8]) -> Result<A> {
    decode_contract_input(args, crate::ACTOR_MESSAGE_SIZE_LIMIT as usize)
}

pub(crate) fn encode_action_output(output: &impl serde::Serialize) -> Result<Vec<u8>> {
    agentos_actor_contract::encode_action_output(output, crate::ACTOR_MESSAGE_SIZE_LIMIT as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_scalar_tags_decode_without_integer_or_binary_loss() {
        #[derive(Debug, serde::Deserialize)]
        struct MixedInput {
            after: u64,
            content: crate::FileContentInput,
        }
        // Produced by the installed RivetKit 2.3.10 encodeCborCompat with:
        // [{after: 9007199254740993n, content: new Uint8Array([0, 1, 255])}]
        let bytes = hex::decode("81b90002656166746572826724426967496e74703930303731393932353437343039393367636f6e74656e74826b2455696e74384172726179644141482f").unwrap();
        assert!(rivetkit::action::decode_positional::<MixedInput>(&bytes).is_err());
        let decoded: MixedInput = decode_action_input(&bytes).unwrap();
        assert_eq!(decoded.after, 9_007_199_254_740_993);
        assert!(
            matches!(decoded.content, crate::FileContentInput::Bytes(bytes) if bytes == [0, 1, 255])
        );

        let input = serde_json::json!([{
            "process": { "generation": ["$BigInt", "9007199254740993"], "pid": 7 },
            "after": ["$BigInt", "18446744073709551615"]
        }]);
        let mut encoded = Vec::new();
        ciborium::into_writer(&input, &mut encoded).unwrap();
        let decoded: crate::ProcessOutputRead = decode_action_input(&encoded).unwrap();
        assert_eq!(decoded.process.generation, 9_007_199_254_740_993);
        assert_eq!(decoded.after, Some(u64::MAX));
    }

    #[test]
    fn native_binary_action_inputs_still_decode() {
        let action = crate::FilesystemWriteFile {
            path: "/tmp/data".into(),
            content: crate::FileContentInput::Bytes(vec![0, 1, 255]),
        };
        let encoded = rivetkit::action::encode_positional(&action).unwrap();
        let decoded: crate::FilesystemWriteFile = decode_action_input(&encoded).unwrap();
        assert_eq!(decoded, action);
    }

    #[test]
    fn typescript_undefined_properties_and_escaped_arrays_preserve_defaults() {
        // Installed encodeCborCompat([{command:"echo", options:undefined,
        // args:["$BigInt", "123"]}]). The literal argument array is escaped.
        let encoded = hex::decode("81b9000367636f6d6d616e64646563686f676f7074696f6e73826a24556e646566696e656400646172677382682424426967496e7463313233").unwrap();
        let decoded: crate::ProcessRun = decode_action_input(&encoded).unwrap();
        assert_eq!(decoded.command, "echo");
        assert_eq!(decoded.args, ["$BigInt", "123"]);
        assert_eq!(decoded.options, crate::ActorExecOptions::default());

        let value = serde_json::json!([{
            "first": ["$Undefined", 0],
            "second": "preserved",
            "third": ["$Undefined", 0],
            "nested": { "env": ["$Undefined", 0], "cwd": "/tmp" },
            "array": [["$Undefined", 0], ["$$Undefined", 0], ["$$$custom", 1]]
        }]);
        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();
        let decoded: serde_json::Value = decode_action_input(&encoded).unwrap();
        assert_eq!(
            decoded,
            serde_json::json!([{
                "second": "preserved",
                "nested": { "cwd": "/tmp" },
                "array": [null, ["$Undefined", 0], ["$$custom", 1]]
            }])
        );
    }

    #[test]
    fn typescript_array_buffer_uses_the_bounded_binary_path() {
        // Installed encodeCborCompat([{path:"/tmp/binary",
        // content:new Uint8Array([0, 1, 255]).buffer}]).
        let encoded = hex::decode("81b9000264706174686b2f746d702f62696e61727967636f6e74656e74826c244172726179427566666572644141482f").unwrap();
        let decoded: crate::FilesystemWriteFile = decode_action_input(&encoded).unwrap();
        assert_eq!(decoded.path, "/tmp/binary");
        assert!(
            matches!(decoded.content, crate::FileContentInput::Bytes(bytes) if bytes == [0, 1, 255])
        );
    }

    #[test]
    fn malformed_client_scalar_tags_fail_with_typed_bounded_errors() {
        for value in [
            serde_json::json!(["$BigInt"]),
            serde_json::json!(["$BigInt", 1]),
            serde_json::json!(["$BigInt", "+1"]),
            serde_json::json!(["$BigInt", "18446744073709551616"]),
            serde_json::json!(["$Uint8Array", "invalid!"]),
            serde_json::json!(["$Uint8Array", []]),
            serde_json::json!(["$ArrayBuffer", "invalid!"]),
            serde_json::json!(["$Undefined", 1]),
            serde_json::json!(["$Undefined", "0"]),
        ] {
            let mut encoded = Vec::new();
            ciborium::into_writer(&value, &mut encoded).unwrap();
            let error = decode_action_input::<serde_json::Value>(&encoded).unwrap_err();
            assert!(error.to_string().starts_with("invalid_input:"));
        }
        let mut oversized = ciborium::Value::Array(vec![
            ciborium::Value::Text("$Uint8Array".into()),
            ciborium::Value::Text("A".repeat(1024 * 1024 + 4)),
        ]);
        let mut remaining_nodes = MAX_ACTION_INPUT_NODES;
        let error = normalize_client_tags(&mut oversized, 0, &mut remaining_nodes).unwrap_err();
        let error = classify_public_error(error.context("decode action args"));
        let error = error.downcast_ref::<RivetError>().unwrap();
        assert!(
            matches!(&error.kind, RivetErrorKind::Dynamic { code, .. } if code == "limit_exceeded")
        );

        let mut nested = ciborium::Value::Null;
        for _ in 0..=MAX_ACTION_INPUT_DEPTH {
            nested = ciborium::Value::Array(vec![nested]);
        }
        let mut remaining_nodes = MAX_ACTION_INPUT_NODES;
        assert!(normalize_client_tags(&mut nested, 0, &mut remaining_nodes)
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded:"));
        assert!(normalize_client_tags(&mut ciborium::Value::Null, 0, &mut 0)
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded:"));
        let oversized = vec![0; crate::ACTOR_MESSAGE_SIZE_LIMIT as usize + 1];
        assert!(decode_action_input::<()>(&oversized)
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded:"));
    }

    #[test]
    fn encoded_action_limits_include_batch_metadata_and_json_escaping() {
        let entries = (0..128)
            .map(|_| {
                serde_json::json!({
                    "path": "p".repeat(4096),
                    "content": "x".repeat(6144),
                    "error": null,
                })
            })
            .collect::<Vec<_>>();
        assert!(encode_action_output(&entries)
            .expect_err("batch metadata must count toward the envelope")
            .to_string()
            .starts_with("limit_exceeded:"));
        assert!(encode_action_output(&"\u{0000}".repeat(200_000))
            .expect_err("JSON escaping must count toward the envelope")
            .to_string()
            .starts_with("limit_exceeded:"));
    }

    #[test]
    fn binary_transfers_fit_bare_output_and_require_binary_transport() {
        let encoded = encode_action_output(&crate::FileBytes(vec![0; 768 * 1024]))
            .expect("the documented binary transfer size fits the BARE envelope");
        let bytes: serde_bytes::ByteBuf = ciborium::from_reader(encoded.as_slice()).unwrap();
        assert_eq!(bytes.len(), 768 * 1024);
        assert!(ciborium::from_reader::<serde_json::Value, _>(encoded.as_slice()).is_err());
    }

    #[test]
    fn typed_core_errors_keep_public_error_codes_through_context() {
        for (error, expected) in [
            (
                agentos_client::ClientError::OperationTimedOut {
                    message: "VM SQLite close exceeded its deadline".into(),
                    details: Box::new(agentos_client::ResourceLimitDetails {
                        operation: Some("vm.dispose".into()),
                        configured_limit: Some(5_000),
                        ..Default::default()
                    }),
                },
                "timeout",
            ),
            (
                agentos_client::ClientError::ExecutionTimedOut {
                    process_id: "p".into(),
                },
                "timeout",
            ),
            (
                agentos_client::ClientError::TerminationFailed {
                    process_id: "p".into(),
                    reason: "missing exit".into(),
                },
                "termination_failed",
            ),
            (agentos_client::ClientError::ProcessNotFound(1), "not_found"),
            (
                agentos_client::ClientError::ShellNotFound("shell-1".into()),
                "not_found",
            ),
            (
                agentos_client::ClientError::PackageCachePendingLimit { limit: 1 },
                "limit_exceeded",
            ),
            (
                agentos_client::ClientError::InvalidPackageSource("invalid".into()),
                "invalid_input",
            ),
        ] {
            let error =
                classify_public_error(anyhow::Error::new(error).context("actor action failed"));
            let error = error
                .downcast_ref::<RivetError>()
                .expect("public typed error");
            let RivetErrorKind::Dynamic { code, .. } = &error.kind else {
                panic!("expected a dynamic agentOS error");
            };
            assert_eq!(code, expected);
        }
    }

    #[test]
    fn vm_disposal_timeout_exposes_structured_limit_metadata() {
        let error = agentos_client::ClientError::OperationTimedOut {
            message: "SQLite close was not confirmed".into(),
            details: Box::new(agentos_client::ResourceLimitDetails {
                limit_name: Some("reactor.shutdownDeadlineMs".into()),
                configured_limit: Some(5_000),
                unit: Some("milliseconds".into()),
                vm_id: Some("vm-test".into()),
                operation: Some("vm.dispose".into()),
                configuration_path: Some("limits.reactor.shutdownDeadlineMs".into()),
                retryable: Some(false),
                errno: Some("ETIMEDOUT".into()),
                ..Default::default()
            }),
        };
        let error = classify_public_error(anyhow::Error::new(error).context("stop VM"));
        let public = error.downcast_ref::<RivetError>().expect("public timeout");
        assert!(matches!(
            &public.kind,
            RivetErrorKind::Dynamic { code, .. } if code == "timeout"
        ));
        let metadata: serde_json::Value =
            serde_json::from_str(public.meta.as_ref().expect("structured metadata").get())
                .expect("decode metadata");
        assert_eq!(metadata["limit"]["limitName"], "reactor.shutdownDeadlineMs");
        assert_eq!(metadata["limit"]["configuredLimit"], 5_000);
        assert_eq!(metadata["limit"]["unit"], "milliseconds");
        assert_eq!(metadata["limit"]["vmId"], "vm-test");
        assert_eq!(metadata["limit"]["operation"], "vm.dispose");
        assert_eq!(
            metadata["limit"]["configurationPath"],
            "limits.reactor.shutdownDeadlineMs"
        );
        assert_eq!(metadata["limit"]["retryable"], false);
        assert_eq!(metadata["limit"]["errno"], "ETIMEDOUT");
    }

    #[test]
    fn resource_limit_metadata_uses_camel_case_and_actor_deadlines_have_no_limit() {
        let error = agentos_client::ClientError::ResourceLimit {
            code: "ERR_AGENTOS_RESOURCE_LIMIT".into(),
            message: "command queue full".into(),
            details: Box::new(agentos_client::ResourceLimitDetails {
                current_usage: Some(123),
                requested: Some(45),
                session_generation: Some(6),
                capability_id: Some(7),
                ..Default::default()
            }),
        };
        let error = classify_public_error(anyhow::Error::new(error).context("invoke VM"));
        let public = error.downcast_ref::<RivetError>().expect("public limit");
        assert!(matches!(
            &public.kind,
            RivetErrorKind::Dynamic { code, .. } if code == "limit_exceeded"
        ));
        let metadata: serde_json::Value =
            serde_json::from_str(public.meta.as_ref().unwrap().get()).unwrap();
        assert_eq!(metadata["limit"]["currentUsage"], 123);
        assert_eq!(metadata["limit"]["requested"], 45);
        assert_eq!(metadata["limit"]["sessionGeneration"], 6);
        assert_eq!(metadata["limit"]["capabilityId"], 7);
        assert!(metadata["limit"]
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.contains('_')));
        assert!(metadata["limit"]["configurationPath"].is_null());

        let error = classify_public_error(anyhow::anyhow!(
            "timeout: runtime shutdown exceeded 10000ms"
        ));
        let public = error.downcast_ref::<RivetError>().expect("public timeout");
        let metadata: serde_json::Value =
            serde_json::from_str(public.meta.as_ref().unwrap().get()).unwrap();
        assert!(metadata.get("limit").is_none());
    }

    #[test]
    fn stream_expiration_and_package_resolution_timeout_are_typed() {
        for (message, expected) in [
            ("timeout: runtime shutdown exceeded 10000ms", "timeout"),
            ("fetch_stream_expired: stream lifetime elapsed", "expired"),
            (
                "config_package_resolution_timeout: required software resolution expired",
                "timeout",
            ),
        ] {
            let error =
                classify_public_error(anyhow::anyhow!(message).context("actor action failed"));
            let error = error
                .downcast_ref::<RivetError>()
                .expect("public typed error");
            let RivetErrorKind::Dynamic { code, .. } = &error.kind else {
                panic!("expected a dynamic agentOS error");
            };
            assert_eq!(code, expected);
        }
    }

    #[test]
    fn sidecar_package_validation_failures_remain_public_invalid_input() {
        for code in [
            "invalid_package_source",
            "invalid_package_format",
            "package_digest_mismatch",
        ] {
            let error = classify_public_error(
                anyhow::Error::new(agentos_client::ClientError::PackageAcquisition {
                    code: code.into(),
                    message: "invalid artifact".into(),
                    details: Box::new(agentos_client::error::ResourceLimitDetails {
                        operation: Some("package.acquire".into()),
                        errno: Some("EINVAL".into()),
                        ..Default::default()
                    }),
                })
                .context("software installation failed"),
            );
            let error = error
                .downcast_ref::<RivetError>()
                .expect("public typed error");
            assert!(matches!(
                &error.kind,
                RivetErrorKind::Dynamic { code, .. } if code == "invalid_input"
            ));
            let metadata: serde_json::Value =
                serde_json::from_str(error.meta.as_ref().unwrap().get()).unwrap();
            assert_eq!(metadata["package"]["operation"], "package.acquire");
            assert_eq!(metadata["package"]["errno"], "EINVAL");
            assert_eq!(metadata["package"]["code"], code);
        }
    }

    #[test]
    fn operator_cache_and_acquisition_io_failures_are_not_caller_validation_errors() {
        let mut errors = vec![agentos_client::ClientError::PackageCacheConfiguration(
            "operator cache is already configured differently".into(),
        )];
        errors.extend(
            [
                "package_cache_configuration",
                "package_download_failed",
                "package_io_failed",
                "package_acquisition_failed",
            ]
            .map(|code| agentos_client::ClientError::PackageAcquisition {
                code: code.into(),
                message: "operator or acquisition failure".into(),
                details: Box::default(),
            }),
        );
        for error in errors {
            let error =
                classify_public_error(anyhow::Error::new(error).context("install software"));
            assert!(error.downcast_ref::<RivetError>().is_none());
            assert!(error
                .downcast_ref::<agentos_client::ClientError>()
                .is_some());
        }
    }
}

#[cfg(feature = "contract")]
pub(crate) fn contract() -> Vec<agentos_actor_contract::schema::ActionContract> {
    let mut actions = inventory::iter::<RegisteredAction>
        .into_iter()
        .map(|action| (action.contract)())
        .collect::<Vec<_>>();
    actions.sort_by_key(|action| action.name);
    actions
}

#[cfg(feature = "contract")]
pub(crate) fn collect_contract_types(types: &mut agentos_actor_contract::schema::TypeCollector) {
    for action in inventory::iter::<RegisteredAction> {
        (action.collect_types)(types);
    }
}

impl ActionSet<AgentOsActor> for AgentOsActionSet {
    fn entries() -> Vec<ActionEntry<AgentOsActor>> {
        let mut actions = inventory::iter::<RegisteredAction>
            .into_iter()
            .collect::<Vec<_>>();
        actions.sort_by_key(|action| action.name);
        actions
            .into_iter()
            .map(|action| ActionEntry::new(action.name))
            .collect()
    }

    fn dispatch(
        actor: Arc<AgentOsActor>,
        ctx: Ctx<AgentOsActor>,
        name: &str,
        args: &[u8],
    ) -> Option<DispatchFuture> {
        inventory::iter::<RegisteredAction>
            .into_iter()
            .find(|action| action.name == name)
            .map(|action| (action.dispatch)(actor, ctx, args))
    }
}
