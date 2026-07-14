#![forbid(unsafe_code)]

//! Host-free core for the Agent OS ACP sidecar extension.
//!
//! This crate owns the host-independent ACP lifecycle, request/response codec,
//! and per-session model. It compiles to wasm32 so native and browser sidecars run
//! the same transitions, with each backend supplying process, filesystem, and
//! callback operations through the thin [`AcpHost`] seam.

use std::fmt;

use agentos_protocol::generated::v1::{AcpErrorResponse, AcpResponse};

pub mod behavior;
pub mod codec;
pub mod engine;
pub mod host;
pub mod json_rpc;
pub mod session;

pub use engine::{AcpCore, ResumeStep};
pub use host::{AcpHost, ProjectedAgentLaunch};
pub use session::AcpSessionRecord;

/// Host-free error type for the ACP core. Mirrors the wire `AcpErrorResponse`
/// shape (a stable string `code` + human message) so it round-trips identically
/// to the native sidecar's `SidecarError`-derived responses without depending on
/// the native error enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpCoreError {
    InvalidState(String),
    LimitExceeded(String),
    Unauthorized(String),
    Unsupported(String),
    Conflict(String),
    Execution(String),
}

impl AcpCoreError {
    /// Stable machine code, matching the native sidecar's `error_code` mapping so
    /// clients see identical codes regardless of backend.
    pub fn code(&self) -> &'static str {
        match self {
            AcpCoreError::InvalidState(_) => "invalid_state",
            AcpCoreError::LimitExceeded(_) => "limit_exceeded",
            AcpCoreError::Unauthorized(_) => "unauthorized",
            AcpCoreError::Unsupported(_) => "unsupported",
            AcpCoreError::Conflict(_) => "conflict",
            AcpCoreError::Execution(_) => "execution",
        }
    }
}

impl fmt::Display for AcpCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcpCoreError::InvalidState(message)
            | AcpCoreError::LimitExceeded(message)
            | AcpCoreError::Unauthorized(message)
            | AcpCoreError::Unsupported(message)
            | AcpCoreError::Conflict(message)
            | AcpCoreError::Execution(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AcpCoreError {}

/// The ACP extension namespace, re-exported for convenience.
pub use agentos_protocol::ACP_EXTENSION_NAMESPACE;

/// Build the wire error response for an ACP core error.
pub fn error_response(error: &AcpCoreError) -> AcpResponse {
    AcpResponse::AcpErrorResponse(AcpErrorResponse {
        code: error.code().to_string(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            AcpCoreError::InvalidState("x".into()).code(),
            "invalid_state"
        );
        assert_eq!(AcpCoreError::Unsupported("x".into()).code(), "unsupported");
        assert_eq!(
            AcpCoreError::LimitExceeded("x".into()).code(),
            "limit_exceeded"
        );
        assert_eq!(
            error_response(&AcpCoreError::Conflict("dup".into())),
            AcpResponse::AcpErrorResponse(AcpErrorResponse {
                code: "conflict".into(),
                message: "dup".into(),
            })
        );
    }
}
