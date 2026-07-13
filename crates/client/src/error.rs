//! Error taxonomy for the Agent OS client SDK.
//!
//! Public methods return [`anyhow::Result`]; the typed [`ClientError`] is carried as the `source` so
//! callers can downcast. Filesystem errno decisions come from the kernel/sidecar.
//!
//! Hard rule (parity): JSON-RPC errors are NOT Rust `Err`. `prompt`, `cancel_session`,
//! `set_session_model`, `set_session_thought_level`, `respond_permission`, `raw_session_send`,
//! `raw_send`, and `set_session_mode` return a [`crate::json_rpc::JsonRpcResponse`] whose `error`
//! field may be populated (including `acp_timeout` and codex `-32601` fallbacks). Do not convert
//! those into `Err`.

use agentos_sidecar_client::{ProtocolCodecError, TransportError};

/// Typed error taxonomy for the client SDK.
#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    /// An SDK-spawned process with the given pid was not found.
    ///
    /// The message text matches the TypeScript `AgentOs` exactly (capital "P"). These strings are
    /// observable data (surfaced to callers), not logs, so the casing follows TS rather than the
    /// lowercase log convention.
    #[error("Process not found: {0}")]
    ProcessNotFound(u32),

    /// A shell with the given sidecar process id was not found.
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

    /// A cron schedule string could not be parsed/validated.
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),

    /// A one-shot (ISO-8601) cron schedule resolved to a time in the past.
    #[error("schedule is in the past: {0}")]
    PastSchedule(String),

    /// An explicit caller option cannot be represented on the sidecar protocol.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A bounded host event route evicted frames before this consumer observed them.
    #[error("event stream lagged and skipped {skipped} event(s)")]
    EventStreamLagged { skipped: u64 },

    /// The sidecar transport closed before a routed stream reached its terminal event.
    #[error("event stream closed before {context}")]
    EventStreamClosed { context: &'static str },

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

/// Convenience alias for results carrying a typed [`ClientError`].
pub type ClientResult<T> = std::result::Result<T, ClientError>;
