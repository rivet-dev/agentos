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

use agent_os_sidecar::protocol::ProtocolCodecError;

/// Typed error taxonomy for the client SDK.
#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    /// A filesystem path was not absolute (did not start with `/`).
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(String),

    /// A filesystem path was not in posix-normalized form.
    #[error("path must be normalized: {0}")]
    PathNotNormalized(String),

    /// A write was attempted against a read-only path (for example `/proc`).
    #[error("path is read-only: {0}")]
    PathReadOnly(String),

    /// An SDK-spawned process with the given pid was not found.
    #[error("process not found: {0}")]
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

/// Convenience alias for results carrying a typed [`ClientError`].
pub type ClientResult<T> = std::result::Result<T, ClientError>;
