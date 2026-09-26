//! Error taxonomy for the Agent OS client SDK.
//!
//! Mirrors `spec.md` §4 / ADR-001 §4. Preserves the TypeScript SDK distinction so callers can still
//! discriminate path-guard violations from kernel errno failures. Public methods return
//! [`anyhow::Result`]; the typed [`ClientError`] is carried as the `source` so callers can downcast.
//!
use agentos_sidecar_client::{ProtocolCodecError, TransportError};

/// Structured sidecar admission metadata kept behind one allocation so the
/// public [`ClientError`] remains cheap to return through every SDK method.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
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
#[derive(thiserror::Error, Debug, Clone)]
pub enum ClientError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
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

    /// The execution deadline expired after the guest's exit was confirmed.
    #[error("timeout: process {process_id} exceeded its execution deadline and was stopped")]
    ExecutionTimedOut { process_id: String },

    /// A sidecar operation exceeded its deadline. Unlike ExecutionTimedOut,
    /// this does not assert successful guest termination or resource cleanup.
    #[error("timeout: {message}")]
    OperationTimedOut {
        message: String,
        details: Box<ResourceLimitDetails>,
    },

    /// Observation or cleanup failed without proof of the guest's exit.
    #[error("termination_failed: process {process_id} could not be confirmed stopped: {reason}")]
    TerminationFailed { process_id: String, reason: String },

    /// A shell with the given synthetic `shell-N` id was not found.
    #[error("shell not found: {0}")]
    ShellNotFound(String),

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

    /// A package source failed validation before any guest-visible mutation.
    #[error("invalid package source: {0}")]
    InvalidPackageSource(String),

    /// Remote package acquisition failed under the configured bounded policy.
    #[error("package download failed: {0}")]
    PackageDownload(String),

    /// A package exceeded the configured acquisition byte limit.
    #[error(
        "package is {observed} bytes; limit is {limit}; raise package_resolver.max_package_bytes"
    )]
    PackageTooLarge { observed: u64, limit: u64 },

    /// The acquired bytes did not match the caller-pinned content identity.
    #[error("package digest mismatch: expected {expected}, got {actual}")]
    PackageDigestMismatch { expected: String, actual: String },

    /// A downloaded file was not a bounded valid `.aospkg` container.
    #[error("invalid .aospkg: {0}")]
    InvalidPackageFormat(String),

    /// Trusted host I/O failed while acquiring or staging a package.
    #[error("package I/O failed: {0}")]
    PackageIo(String),

    /// A package acquisition failed in the sidecar. The wire code is kept
    /// separately from the message because not every local package error can
    /// be reconstructed from a generic rejection without parsing its text.
    /// Preserve the same rejection metadata available to TypeScript callers.
    #[error("package acquisition [{code}]: {message}")]
    PackageAcquisition {
        code: String,
        message: String,
        details: Box<ResourceLimitDetails>,
    },

    /// The process-local immutable package cache cannot admit another object
    /// without evicting a package still pinned by a live VM.
    #[error(
        "package cache capacity exceeded: requested {requested} bytes with {current} of {limit} bytes in use; raise ProcessPackageCacheOptions.max_bytes or uninstall pinned software"
    )]
    PackageCacheCapacity {
        requested: u64,
        current: u64,
        limit: u64,
    },

    #[error(
        "package cache entry limit exceeded: {current} of {limit} entries are in use; raise ProcessPackageCacheOptions.max_entries or uninstall pinned software"
    )]
    PackageCacheEntryCapacity { current: usize, limit: usize },

    /// Too many distinct acquisitions are waiting in the process. The bound
    /// prevents unique untrusted URLs from growing the single-flight table.
    #[error(
        "package cache pending acquisition limit {limit} reached; raise ProcessPackageCacheOptions.max_pending_acquisitions"
    )]
    PackageCachePendingLimit { limit: usize },

    /// A process cache was already initialized with different operator-owned
    /// limits. Process-global configuration is first-write-once by design.
    #[error("package cache is already configured differently: {0}")]
    PackageCacheConfiguration(String),

    /// An exact content-addressed installed package was not present.
    #[error("software package not found: {0}")]
    SoftwareNotFound(String),

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
            || rejection.code == "timeout"
            || matches!(
                rejection.operation.as_deref(),
                Some("package.acquire" | "package.cache_stats")
            )
        {
            let details = Box::new(ResourceLimitDetails {
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
            });
            if rejection.code == "timeout" {
                return Self::OperationTimedOut {
                    message: rejection.message,
                    details,
                };
            }
            if rejection.code == "ERR_AGENTOS_RESOURCE_LIMIT"
                || rejection.code == "ERR_AGENTOS_OVERLOADED"
            {
                return Self::ResourceLimit {
                    code: rejection.code,
                    message: rejection.message,
                    details,
                };
            }
            return Self::PackageAcquisition {
                code: rejection.code,
                message: rejection.message,
                details,
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
            | ClientError::InvalidConfig(_)
            | ClientError::ProcessNotFound(_)
            | ClientError::ExecutionTimedOut { .. }
            | ClientError::OperationTimedOut { .. }
            | ClientError::TerminationFailed { .. }
            | ClientError::ShellNotFound(_)
            | ClientError::InvalidSchedule(_)
            | ClientError::PastSchedule(_)
            | ClientError::InvalidPackageSource(_)
            | ClientError::PackageDownload(_)
            | ClientError::PackageTooLarge { .. }
            | ClientError::PackageDigestMismatch { .. }
            | ClientError::InvalidPackageFormat(_)
            | ClientError::PackageIo(_)
            | ClientError::PackageAcquisition { .. }
            | ClientError::PackageCacheCapacity { .. }
            | ClientError::PackageCacheEntryCapacity { .. }
            | ClientError::PackageCachePendingLimit { .. }
            | ClientError::PackageCacheConfiguration(_)
            | ClientError::SoftwareNotFound(_)
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

    #[test]
    fn sidecar_package_rejection_is_not_misclassified_as_a_kernel_error() {
        for operation in ["package.acquire", "package.cache_stats"] {
            for code in [
                "invalid_package_source",
                "invalid_package_format",
                "package_digest_mismatch",
                "package_download_failed",
                "package_io_failed",
                "package_cache_configuration",
                "package_acquisition_failed",
            ] {
                let error =
                    ClientError::from_rejection(agentos_sidecar_client::wire::RejectedResponse {
                        code: code.into(),
                        message: "specific package failure".into(),
                        operation: Some(operation.into()),
                        limit_name: None,
                        configured_limit: None,
                        current_usage: None,
                        requested: None,
                        unit: None,
                        scope: Some("session".into()),
                        vm_id: None,
                        session_generation: None,
                        capability_id: None,
                        configuration_path: None,
                        retryable: Some(false),
                        errno: Some("EIO".into()),
                    });
                let ClientError::PackageAcquisition {
                    code: actual,
                    message,
                    details,
                } = error
                else {
                    panic!("expected a typed package acquisition error");
                };
                assert_eq!(actual, code);
                assert_eq!(message, "specific package failure");
                assert_eq!(details.operation.as_deref(), Some(operation));
                assert_eq!(details.scope.as_deref(), Some("session"));
                assert_eq!(details.retryable, Some(false));
                assert_eq!(details.errno.as_deref(), Some("EIO"));
            }
        }
    }

    #[test]
    fn package_rejections_keep_limit_timeout_precedence_and_non_package_errors_unchanged() {
        for code in [
            "ERR_AGENTOS_RESOURCE_LIMIT",
            "ERR_AGENTOS_OVERLOADED",
            "timeout",
            "invalid_package_source",
        ] {
            for operation in ["package.acquire", "filesystem.read"] {
                let error =
                    ClientError::from_rejection(agentos_sidecar_client::wire::RejectedResponse {
                        code: code.into(),
                        message: "wire failure".into(),
                        limit_name: Some("exampleLimit".into()),
                        configured_limit: Some(10),
                        current_usage: Some(8),
                        requested: Some(4),
                        unit: Some("bytes".into()),
                        scope: Some("session".into()),
                        vm_id: None,
                        session_generation: None,
                        capability_id: None,
                        operation: Some(operation.into()),
                        configuration_path: Some("example.limit".into()),
                        retryable: Some(true),
                        errno: Some("EAGAIN".into()),
                    });
                let details = match (code, operation, error) {
                    ("timeout", _, ClientError::OperationTimedOut { details, .. }) => details,
                    (
                        "ERR_AGENTOS_RESOURCE_LIMIT" | "ERR_AGENTOS_OVERLOADED",
                        _,
                        ClientError::ResourceLimit { details, .. },
                    ) => details,
                    (
                        "invalid_package_source",
                        "package.acquire",
                        ClientError::PackageAcquisition { details, .. },
                    ) => details,
                    (
                        "invalid_package_source",
                        "filesystem.read",
                        ClientError::Kernel { code, message },
                    ) => {
                        assert_eq!(code, "invalid_package_source");
                        assert_eq!(message, "wire failure");
                        continue;
                    }
                    (_, _, other) => panic!("unexpected rejection classification: {other:?}"),
                };
                assert_eq!(details.limit_name.as_deref(), Some("exampleLimit"));
                assert_eq!(details.configured_limit, Some(10));
                assert_eq!(details.current_usage, Some(8));
                assert_eq!(details.requested, Some(4));
                assert_eq!(details.configuration_path.as_deref(), Some("example.limit"));
                assert_eq!(details.retryable, Some(true));
                assert_eq!(details.errno.as_deref(), Some("EAGAIN"));
            }
        }
    }
}
