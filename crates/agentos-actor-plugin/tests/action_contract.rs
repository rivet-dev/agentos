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
    let ts = include_str!("../../../packages/agentos/src/actor-actions.ts");
    let normalized_ts = normalize_ws(ts);

    for action in ACTION_CONTRACTS {
        let signature = normalize_ws(action.ts_signature);
        assert!(
            normalized_ts.contains(&signature),
            "packages/agentos/src/actor-actions.ts signature drifted for {}.\nexpected snippet: {}",
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

fn normalize_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Regression: rivetkit clients encode JS `undefined` as the JSON-compat
/// envelope `["$Undefined", 0]` (rivetkit `common/encoding.ts`). Explicitly
/// passing an omitted trailing options arg (`handle.exec(cmd, undefined)`)
/// or an explicitly-undefined options field (`{ env: undefined }`) must
/// decode instead of failing with an opaque positional-decode error.
#[test]
fn undefined_envelopes_decode_as_absent_options() {
    let undefined = CborValue::Array(vec![
        CborValue::Text(String::from("$Undefined")),
        CborValue::Integer(0.into()),
    ]);

    // exec("pwd", undefined)
    let args = encode_args(&CborValue::Array(vec![
        CborValue::Text(String::from("pwd")),
        undefined.clone(),
    ]));
    contract::decode_action_args("exec", &args).expect("exec with trailing undefined options");

    // exec("pwd", { env: undefined })
    let args = encode_args(&CborValue::Array(vec![
        CborValue::Text(String::from("pwd")),
        CborValue::Map(vec![(
            CborValue::Text(String::from("env")),
            undefined.clone(),
        )]),
    ]));
    contract::decode_action_args("exec", &args).expect("exec with undefined options field");

    // createSession("pi", undefined)
    let args = encode_args(&CborValue::Array(vec![
        CborValue::Text(String::from("pi")),
        undefined,
    ]));
    contract::decode_action_args("createSession", &args).expect("createSession with trailing undefined");
}

fn encode_args(value: &CborValue) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).expect("encode test args");
    bytes
}
