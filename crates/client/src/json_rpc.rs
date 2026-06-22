//! JSON-RPC 2.0 types used by the ACP session layer.
//!
//! Ported from `packages/core/src/json-rpc.ts`. `result`/`params`/`data` are opaque JSON
//! (`serde_json::Value`). JSON-RPC errors are NOT Rust `Err`; session methods return a
//! [`JsonRpcResponse`] whose `error` field may be populated.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON-RPC id: a number, a string, or null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
    Null,
}

/// A JSON-RPC 2.0 response. `result` and `error` are mutually exclusive in practice but both are
/// optional on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<JsonRpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object. `data` may carry an [`AcpTimeoutErrorData`] or arbitrary JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Structured `data` for an ACP timeout error (`kind: "acp_timeout"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpTimeoutErrorData {
    pub kind: String,
    pub method: String,
    pub id: Option<JsonRpcId>,
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: f64,
    #[serde(default, rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killed: Option<bool>,
    #[serde(
        default,
        rename = "transportState",
        skip_serializing_if = "Option::is_none"
    )]
    pub transport_state: Option<String>,
    #[serde(rename = "recentActivity")]
    pub recent_activity: Vec<String>,
}

/// Structured `data` for an "unknown session" error (`kind: "unknown_session"`).
///
/// Mirrors `UnknownSessionErrorData` in `packages/core/src/json-rpc.ts`. The
/// sidecar normalizes an adapter's native "no such session" error from
/// `session/load` into this shape so resume orchestration can distinguish "the
/// store didn't survive the wake — fall through to a fresh session" from a
/// transport/timeout error (which must propagate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownSessionErrorData {
    pub kind: String,
    /// Optional metadata. The discriminator is `kind` alone — the sidecar's
    /// normalized error carries only `kind`, so this stays optional to keep the
    /// sidecar and client contracts aligned with the TS mirror.
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<String>,
}

/// Whether a JSON-RPC error's `data` is an [`UnknownSessionErrorData`]
/// (discriminated by `kind == "unknown_session"`; `sessionId` is optional).
/// Mirrors the TS `isUnknownSessionErrorData()` discriminator.
pub fn is_unknown_session(error: &JsonRpcError) -> bool {
    error
        .data
        .as_ref()
        .and_then(|data| data.as_object())
        .is_some_and(|data| data.get("kind").and_then(Value::as_str) == Some("unknown_session"))
}

/// A JSON-RPC 2.0 notification (no id). `params` is opaque JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
