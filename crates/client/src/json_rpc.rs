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

/// A JSON-RPC 2.0 notification (no id). `params` is opaque JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A sequenced session notification. Synthetic events use NEGATIVE sequence numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequencedEvent {
    #[serde(rename = "sequenceNumber")]
    pub sequence_number: i64,
    pub notification: JsonRpcNotification,
}
