use std::collections::BTreeSet;

use agentos_actor_plugin::actions::contract::{
    self, ActionContract, EventContract, ReplyShape, ACTION_CONTRACTS, EVENT_CONTRACTS,
};
use ciborium::Value as CborValue;

#[test]
fn dispatcher_arms_have_contract_rows() {
    let dispatcher = include_str!("../src/actions/mod.rs");
    let dispatch = dispatcher
        .split("pub(crate) async fn dispatch")
        .nth(1)
        .expect("dispatch function exists");
    let dispatch_arms = dispatch_arm_names(dispatch);
    let contract_names = contract_names();

    assert_eq!(
        dispatch_arms, contract_names,
        "dispatcher arms and ACTION_CONTRACTS drifted"
    );
}

#[test]
fn client_arg_payloads_decode_for_every_action() {
    for action in ACTION_CONTRACTS {
        let variants = contract::encoded_client_arg_variants(action.name)
            .unwrap_or_else(|error| panic!("{} arg fixture build failed: {error}", action.name));
        assert!(
            !variants.is_empty(),
            "{} must have at least one arg fixture",
            action.name
        );
        for (index, payload) in variants.iter().enumerate() {
            contract::decode_action_args(action.name, payload).unwrap_or_else(|error| {
                panic!(
                    "{} arg fixture #{index} did not decode from a client-shaped payload: {error}",
                    action.name
                )
            });
        }
    }
}

#[test]
fn unknown_action_name_rejects_at_contract_decode_layer() {
    let error = contract::decode_action_args("definitelyNotAnAction", b"not-cbor")
        .expect_err("unknown actions must reject before dispatch");
    assert!(
        error
            .to_string()
            .contains("unknown action definitelyNotAnAction"),
        "unexpected unknown-action error: {error}"
    );
}

#[test]
fn malformed_arg_payloads_reject_for_every_action_contract() {
    for action in ACTION_CONTRACTS {
        if contract::decode_action_args(action.name, b"not-cbor").is_ok() {
            panic!("{} accepted a malformed non-CBOR arg payload", action.name);
        }
    }
}

#[test]
fn invalid_client_arg_shapes_reject_for_every_action_contract() {
    for action in ACTION_CONTRACTS {
        let variants =
            contract::encoded_invalid_client_arg_variants(action.name).unwrap_or_else(|error| {
                panic!("{} invalid arg fixture build failed: {error}", action.name)
            });
        assert!(
            !variants.is_empty(),
            "{} must have at least one invalid arg fixture",
            action.name
        );
        for (case, payload) in variants {
            if contract::decode_action_args(action.name, &payload).is_ok() {
                panic!(
                    "{} accepted invalid client-shaped arg payload: {case}",
                    action.name
                );
            }
        }
    }
}

#[test]
fn reply_payload_shapes_match_contract_for_every_action() {
    for action in ACTION_CONTRACTS {
        let encoded = contract::encode_sample_reply(action.name)
            .unwrap_or_else(|error| panic!("{} reply fixture build failed: {error}", action.name));
        let decoded = contract::decode_reply_value(&encoded).unwrap_or_else(|error| {
            panic!("{} reply did not decode as CBOR: {error}", action.name)
        });
        assert_reply_shape(action, &decoded);
    }
}

#[test]
fn event_payload_shapes_match_contract_for_every_event() {
    for event in EVENT_CONTRACTS {
        let encoded = contract::encode_sample_event(event.name)
            .unwrap_or_else(|error| panic!("{} event fixture build failed: {error}", event.name));
        let decoded = contract::decode_reply_value(&encoded)
            .unwrap_or_else(|error| panic!("{} event did not decode as CBOR: {error}", event.name));
        let args = match decoded {
            CborValue::Array(args) => args,
            other => panic!(
                "{} event must encode handler args array, got {other:?}",
                event.name
            ),
        };
        assert_eq!(args.len(), 1, "{} event handler arg count", event.name);
        assert_event_payload_shape(event, &args[0]);
    }
}

#[test]
fn agent_crashed_event_includes_core_agent_exit_fields() {
    let encoded = contract::encode_sample_event("agentCrashed").unwrap();
    let decoded = contract::decode_reply_value(&encoded).unwrap();
    let CborValue::Array(args) = decoded else {
        panic!("agentCrashed event must encode handler args array");
    };
    let Some(CborValue::Map(payload)) = args.first() else {
        panic!("agentCrashed event arg must be an object");
    };
    let event = object_field(payload, "event");
    assert_object_keys(
        "agentCrashed.event",
        event,
        &[
            "sessionId",
            "agentType",
            "processId",
            "exitCode",
            "restart",
            "restartCount",
            "maxRestarts",
        ],
    );
    assert_eq!(
        object_field(payload, "sessionId"),
        object_field(event_map(event), "sessionId")
    );
    assert_eq!(
        object_field(event_map(event), "processId"),
        &CborValue::Text("proc-1".to_owned())
    );
}

#[test]
fn create_session_reply_is_bare_string_regression() {
    let encoded = contract::encode_sample_reply("createSession").unwrap();
    let decoded = contract::decode_reply_value(&encoded).unwrap();

    assert_eq!(
        decoded,
        CborValue::Text("session-1".to_owned()),
        "createSession must reply the bare session id string, not {{ sessionId }}"
    );
}

#[test]
fn ts_action_interface_matches_rust_contract_fixture() {
    let ts = contract::render_actor_actions_ts();
    let normalized_ts = normalize_ws(&ts);

    for action in ACTION_CONTRACTS {
        let signature = normalize_ws(action.ts_signature);
        assert!(
            normalized_ts.contains(&signature),
            "generated actor-actions signature drifted for {}.\nexpected snippet: {}",
            action.name,
            action.ts_signature
        );
    }
}

#[test]
fn ts_event_interface_matches_rust_contract_fixture() {
    let ts = include_str!("../../../packages/agentos/src/types.ts");
    let normalized_ts = normalize_ws(ts);

    for event in EVENT_CONTRACTS {
        let signature = normalize_ws(event.ts_signature);
        assert!(
            normalized_ts.contains(&signature),
            "packages/agentos/src/types.ts event signature drifted for {}.\nexpected snippet: {}",
            event.name,
            event.ts_signature
        );
    }
}

#[test]
fn ts_dto_field_names_match_rust_contract_fixture() {
    let actor_actions = contract::render_actor_actions_ts();
    let actor_types = include_str!("../../../packages/agentos/src/types.ts");
    let core_agent_os = include_str!("../../../packages/core/src/agent-os.ts");
    let core_runtime = include_str!("../../../packages/core/src/runtime.ts");
    let core_session = include_str!("../../../packages/core/src/agent-session-types.ts");

    let fixtures = [
        (
            "VirtualStat",
            "packages/core/src/runtime.ts",
            core_runtime,
            "export interface VirtualStat { mode: number; size: number; blocks: number; dev: number; rdev: number; isDirectory: boolean; isSymbolicLink: boolean; atimeMs: number; mtimeMs: number; ctimeMs: number; birthtimeMs: number; ino: number; nlink: number; uid: number; gid: number; }",
        ),
        (
            "ExecResult",
            "packages/core/src/runtime.ts",
            core_runtime,
            "export interface ExecResult { exitCode: number; stdout: string; stderr: string; }",
        ),
        (
            "ProcessInfo",
            "packages/core/src/runtime.ts",
            core_runtime,
            "export interface ProcessInfo { pid: number; ppid: number; pgid: number; sid: number; driver: string; command: string; args: string[]; cwd: string; status: \"running\" | \"exited\"; exitCode: number | null; startTime: number; exitTime: number | null; }",
        ),
        (
            "AgentExitEvent",
            "packages/core/src/agent-os.ts",
            core_agent_os,
            "export interface AgentExitEvent { sessionId: string; agentType: string;",
        ),
        (
            "AgentExitEvent.pid",
            "packages/core/src/agent-os.ts",
            core_agent_os,
            "pid: number | null;",
        ),
        (
            "AgentExitEvent.restartBudget",
            "packages/core/src/agent-os.ts",
            core_agent_os,
            "restart: AgentRestartOutcome; /** Restarts consumed for this session so far. */ restartCount: number; /** Per-session restart budget. */ maxRestarts: number;",
        ),
        (
            "SpawnedProcessInfo",
            "packages/core/src/agent-os.ts",
            core_agent_os,
            "export interface SpawnedProcessInfo { pid: number; command: string; args: string[]; running: boolean; exitCode: number | null;",
        ),
        (
            "PermissionRequest",
            "packages/core/src/agent-session-types.ts",
            core_session,
            "export interface PermissionRequest { permissionId: string; description?: string; params: Record<string, unknown>; }",
        ),
        (
            "PermissionReply",
            "packages/core/src/agent-session-types.ts",
            core_session,
            "export type PermissionReply = \"once\" | \"always\" | \"reject\";",
        ),
        (
            "VmFetchResponse",
            "generated actor-actions",
            &actor_actions,
            "export interface VmFetchResponse { status: number; statusText: string; headers: Record<string, string>; body: Uint8Array; }",
        ),
        (
            "WriteFileResult",
            "generated actor-actions",
            &actor_actions,
            "export interface WriteFileResult { path: string; success: boolean; error?: string; }",
        ),
        (
            "ReadFileResult",
            "generated actor-actions",
            &actor_actions,
            "export interface ReadFileResult { path: string; content?: Uint8Array; error?: string; }",
        ),
        (
            "PersistedSessionRecord",
            "packages/agentos/src/types.ts",
            actor_types,
            "export interface PersistedSessionRecord { sessionId: string; agentType: string; createdAt: number; status: \"running\" | \"idle\"; }",
        ),
        (
            "PersistedSessionEvent",
            "packages/agentos/src/types.ts",
            actor_types,
            "export interface PersistedSessionEvent { sessionId: string; seq: number; event: JsonRpcNotification; createdAt: number; }",
        ),
        (
            "SerializableCronEvent",
            "packages/agentos/src/types.ts",
            actor_types,
            "export type SerializableCronEvent = | { type: \"cron:fire\"; jobId: string; time: number } | { type: \"cron:complete\"; jobId: string; time: number; durationMs: number } | { type: \"cron:error\"; jobId: string; time: number; error: string };",
        ),
        (
            "SerializableCronJobInfo",
            "packages/agentos/src/types.ts",
            actor_types,
            "export interface SerializableCronJobInfo { id: string; schedule: string; overlap: \"allow\" | \"skip\" | \"queue\"; lastRun?: number; nextRun?: number; }",
        ),
    ];

    for (dto, path, source, snippet) in fixtures {
        assert_source_contains(dto, path, source, snippet);
    }
}

fn dispatch_arm_names(source: &str) -> BTreeSet<&str> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix('"')?;
            let (name, rest) = rest.split_once('"')?;
            if rest.trim_start().starts_with("=>") {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

fn contract_names() -> BTreeSet<&'static str> {
    ACTION_CONTRACTS.iter().map(|action| action.name).collect()
}

fn assert_reply_shape(action: &ActionContract, value: &CborValue) {
    match action.reply_shape {
        ReplyShape::Unit => assert_eq!(value, &CborValue::Null, "{} reply", action.name),
        ReplyShape::String => {
            assert!(matches!(value, CborValue::Text(_)), "{} reply", action.name)
        }
        ReplyShape::Bool => {
            assert!(matches!(value, CborValue::Bool(_)), "{} reply", action.name)
        }
        ReplyShape::Number => {
            assert!(
                matches!(value, CborValue::Integer(_) | CborValue::Float(_)),
                "{} reply",
                action.name
            )
        }
        ReplyShape::Uint8Array => assert_uint8_array(action.name, value),
        ReplyShape::Array => {
            assert!(
                matches!(value, CborValue::Array(_)),
                "{} reply",
                action.name
            )
        }
        ReplyShape::NullableArray => {
            assert!(
                matches!(value, CborValue::Array(_) | CborValue::Null),
                "{} reply",
                action.name
            )
        }
        ReplyShape::Object(expected) => assert_object_keys(action.name, value, expected),
    }
}

fn assert_event_payload_shape(event: &EventContract, value: &CborValue) {
    match event.payload_shape {
        ReplyShape::Object(expected) => assert_object_keys(event.name, value, expected),
        other => panic!("{} event uses unsupported shape {other:?}", event.name),
    }
}

fn assert_uint8_array(action: &str, value: &CborValue) {
    let CborValue::Array(items) = value else {
        panic!("{action} reply must be a JSON-compatible Uint8Array wrapper");
    };
    assert_eq!(items.len(), 2, "{action} Uint8Array wrapper arity");
    assert_eq!(
        items.first(),
        Some(&CborValue::Text("$Uint8Array".to_owned())),
        "{action} Uint8Array tag"
    );
    assert!(
        matches!(items.get(1), Some(CborValue::Text(_))),
        "{action} Uint8Array base64 payload"
    );
}

fn assert_object_keys(action: &str, value: &CborValue, expected: &[&str]) {
    let CborValue::Map(entries) = value else {
        panic!("{action} reply must be an object");
    };
    let actual: BTreeSet<&str> = entries
        .iter()
        .map(|(key, _)| match key {
            CborValue::Text(key) => key.as_str(),
            other => panic!("{action} object key must be text, got {other:?}"),
        })
        .collect();
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(actual, expected, "{action} reply object keys");
}

fn event_map(value: &CborValue) -> &Vec<(CborValue, CborValue)> {
    let CborValue::Map(entries) = value else {
        panic!("expected object, got {value:?}");
    };
    entries
}

fn object_field<'a>(entries: &'a [(CborValue, CborValue)], field: &str) -> &'a CborValue {
    entries
        .iter()
        .find_map(|(key, value)| match key {
            CborValue::Text(key) if key == field => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing object field {field}"))
}

fn normalize_ws(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace(", )", ")")
}

fn assert_source_contains(dto: &str, path: &str, source: &str, snippet: &str) {
    let normalized_source = normalize_ws(source);
    let normalized_snippet = normalize_ws(snippet);
    assert!(
        normalized_source.contains(&normalized_snippet),
        "{path} DTO field drift for {dto}.\nexpected snippet: {snippet}"
    );
}
