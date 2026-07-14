//! Error taxonomy for the Agent OS client SDK.
//!
//! Mirrors `spec.md` §4 / ADR-001 §4. Preserves the TypeScript SDK distinction so callers can still
//! discriminate path-guard violations from kernel errno failures. Public methods return
//! [`anyhow::Result`]; the typed [`ClientError`] is carried as the `source` so callers can downcast.
//!
//! Hard rule (parity): JSON-RPC errors are NOT Rust `Err`. `prompt`, `cancel_session`,
//! `set_session_model`, `set_session_thought_level`, `respond_permission`, `raw_session_send`,
//! `raw_send`, and `set_session_mode` return a [`crate::json_rpc::JsonRpcResponse`] whose `error`
//! field may be populated (including `acp_timeout` and codex `-32601` fallbacks). Do not convert
//! those into `Err`.

use agentos_sidecar_client::{ProtocolCodecError, TransportError};

/// Structured sidecar admission metadata kept behind one allocation so the
/// public [`ClientError`] remains cheap to return through every SDK method.
#[derive(Debug)]
pub struct ResourceLimitDetails {
    pub limit_name: Option<String>,
    pub configured_limit: Option<u64>,
    pub current_usage: Option<u64>,
    pub requested: Option<u64>,
    pub unit: Option<String>,
    pub scope: Option<String>,
    pub vm_id: Option<String>,
    pub session_generation: Option<u64>,
    pub capability_id: Option<u64>,
    pub operation: Option<String>,
    pub configuration_path: Option<String>,
    pub retryable: Option<bool>,
    pub errno: Option<String>,
}

/// Typed error taxonomy for the client SDK.
#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    /// A filesystem path was not absolute (did not start with `/`).
    ///
    /// The message text matches the TypeScript `AgentOs` exactly (capital "P"). These strings are
    /// observable data (they surface in `BatchWriteResult.error` / `BatchReadResult.error`), not
    /// logs, so the casing follows TS rather than the lowercase log convention.
    #[error("Path must be absolute: {0}")]
    PathNotAbsolute(String),

    /// A filesystem path was not in posix-normalized form.
    ///
    /// The message text matches the TypeScript `AgentOs` exactly (capital "P").
    #[error("Path must be normalized: {0}")]
    PathNotNormalized(String),

    /// A write was attempted against a read-only path (for example `/proc`).
    ///
    /// The message text matches the TypeScript `AgentOs` exactly (capital "P").
    #[error("Path is read-only: {0}")]
    PathReadOnly(String),

    /// An SDK-spawned process with the given pid was not found.
    ///
    /// The message text matches the TypeScript `AgentOs` exactly (capital "P"). These strings are
    /// observable data (surfaced to callers), not logs, so the casing follows TS rather than the
    /// lowercase log convention.
    #[error("Process not found: {0}")]
    ProcessNotFound(u32),

    /// A shell with the given synthetic `shell-N` id was not found.
    #[error("shell not found: {0}")]
    ShellNotFound(String),

    /// An ACP session with the given id was not found.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// A kernel/sidecar operation failed. The errno `code` string (`ENOENT`, `EEXIST`, `ENOTDIR`,
    /// `EACCES`, `EISDIR`, `ENOTEMPTY`, ...) is preserved verbatim for parity with the TypeScript
    /// `KernelError`.
    #[error("kernel error [{code}]: {message}")]
    Kernel { code: String, message: String },

    /// A sidecar policy/admission bound rejected an operation. Fields are
    /// copied from the lockstep wire response and never parsed from text.
    #[error("resource limit [{code}]: {message}")]
    ResourceLimit {
        code: String,
        message: String,
        details: Box<ResourceLimitDetails>,
    },

    /// A cron schedule string could not be parsed/validated.
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),

    /// A one-shot (ISO-8601) cron schedule resolved to a time in the past.
    #[error("schedule is in the past: {0}")]
    PastSchedule(String),

    /// A framing/codec failure on the sidecar transport.
    #[error("transport error: {0}")]
    Transport(#[from] ProtocolCodecError),

    /// A generic sidecar rejection or I/O failure with context.
    #[error("sidecar error: {0}")]
    Sidecar(String),
}

impl From<TransportError> for ClientError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Protocol(error) => ClientError::Transport(error),
            TransportError::Sidecar(message) => ClientError::Sidecar(message),
        }
    }
}

impl ClientError {
    pub(crate) fn from_rejection(
        rejection: agentos_sidecar_client::wire::RejectedResponse,
    ) -> Self {
        if rejection.code == "ERR_AGENTOS_RESOURCE_LIMIT"
            || rejection.code == "ERR_AGENTOS_OVERLOADED"
        {
            return Self::ResourceLimit {
                code: rejection.code,
                message: rejection.message,
                details: Box::new(ResourceLimitDetails {
                    limit_name: rejection.limit_name,
                    configured_limit: rejection.configured_limit,
                    current_usage: rejection.current_usage,
                    requested: rejection.requested,
                    unit: rejection.unit,
                    scope: rejection.scope,
                    vm_id: rejection.vm_id,
                    session_generation: rejection.session_generation,
                    capability_id: rejection.capability_id,
                    operation: rejection.operation,
                    configuration_path: rejection.configuration_path,
                    retryable: rejection.retryable,
                    errno: rejection.errno,
                }),
            };
        }
        Self::Kernel {
            code: rejection.code,
            message: rejection.message,
        }
    }

    /// Render this error the way the TypeScript `AgentOs` surfaces `err.message` into batch results
    /// (`BatchWriteResult.error` / `BatchReadResult.error`).
    ///
    /// The general [`Display`](std::fmt::Display) impl carries a human/log-oriented prefix
    /// (`kernel error [<code>]: ...`), but the batch surface is observable data that must match TS
    /// byte-for-byte. For kernel failures TS reports `KernelError.message`, which is
    /// `<code>: <message>` (and avoids doubling the code when the message already starts with it).
    /// Path-guard variants already carry the exact TS strings via their `Display` impl.
    pub fn batch_message(&self) -> String {
        match self {
            ClientError::Kernel { code, message } => {
                if message.starts_with(&format!("{code}:")) {
                    message.clone()
                } else {
                    format!("{code}: {message}")
                }
            }
            ClientError::ResourceLimit { code, message, .. } => {
                if message.starts_with(&format!("{code}:")) {
                    message.clone()
                } else {
                    format!("{code}: {message}")
                }
            }
            ClientError::PathNotAbsolute(_)
            | ClientError::PathNotNormalized(_)
            | ClientError::PathReadOnly(_)
            | ClientError::ProcessNotFound(_)
            | ClientError::ShellNotFound(_)
            | ClientError::SessionNotFound(_)
            | ClientError::InvalidSchedule(_)
            | ClientError::PastSchedule(_)
            | ClientError::Transport(_)
            | ClientError::Sidecar(_) => self.to_string(),
        }
    }
}

/// Convenience alias for results carrying a typed [`ClientError`].
pub type ClientResult<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_resource_limit_metadata_survives_rejection_conversion() {
        let error = ClientError::from_rejection(agentos_sidecar_client::wire::RejectedResponse {
            code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
            message: String::from("handle command bytes exceeded"),
            limit_name: Some(String::from("handleCommandBytes")),
            configured_limit: Some(4096),
            current_usage: Some(3072),
            requested: Some(2048),
            unit: Some(String::from("bytes")),
            scope: Some(String::from("vm")),
            vm_id: Some(String::from("vm-1")),
            session_generation: Some(3),
            capability_id: Some(11),
            operation: Some(String::from("socket.write")),
            configuration_path: Some(String::from("limits.reactor.maxHandleCommandBytes")),
            retryable: Some(true),
            errno: Some(String::from("EAGAIN")),
        });

        match error {
            ClientError::ResourceLimit { details, .. } => {
                assert_eq!(details.configured_limit, Some(4096));
                assert_eq!(details.current_usage, Some(3072));
                assert_eq!(details.requested, Some(2048));
                assert_eq!(
                    details.configuration_path.as_deref(),
                    Some("limits.reactor.maxHandleCommandBytes")
                );
                assert_eq!(details.retryable, Some(true));
                assert_eq!(details.errno.as_deref(), Some("EAGAIN"));
            }
            other => panic!("expected resource limit, got {other:?}"),
        }
    }
}
