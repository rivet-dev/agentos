//! WASM-compatible stub for codex-network-proxy.
//!
//! On WASI the host manages networking, so the network proxy is unnecessary.
//! All types are zero-size structs with no-op methods. This stub is not a
//! policy enforcement layer; guest egress must remain mediated by the VM kernel.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

// ---- ProxyError ----

/// Error type for proxy operations (stub — replaces anyhow::Error).
#[derive(Debug)]
pub struct ProxyError(String);

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProxyError {}

impl ProxyError {
    /// Create a new proxy error.
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

// ---- NetworkMode ----

/// Network operating mode (stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Full network access.
    Full,
}

impl Default for NetworkMode {
    fn default() -> Self {
        Self::Full
    }
}

// ---- NetworkProxyConfig ----

/// Proxy configuration (stub).
#[derive(Debug, Clone, Default)]
pub struct NetworkProxyConfig;

/// Extract host and port from a network address string (stub).
pub fn host_and_port_from_network_addr(_addr: &str) -> Option<(String, u16)> {
    None
}

// ---- NetworkPolicy types ----

/// Decision for a network request (stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDecision {
    /// Allow the request.
    Allow,
    /// Deny the request.
    Deny,
}

/// Source of a network decision (stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDecisionSource {
    /// Policy-based.
    Policy,
}

/// Network protocol (stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProtocol {
    /// TCP.
    Tcp,
}

/// Policy request arguments (stub).
#[derive(Debug, Clone)]
pub struct NetworkPolicyRequestArgs {
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
}

/// Policy request (stub).
#[derive(Debug, Clone)]
pub struct NetworkPolicyRequest;

/// Policy decision (stub).
#[derive(Debug, Clone)]
pub struct NetworkPolicyDecision {
    /// Decision.
    pub decision: NetworkDecision,
    /// Source.
    pub source: NetworkDecisionSource,
}

/// Trait for deciding network policy (stub).
pub trait NetworkPolicyDecider: Send + Sync + 'static {
    /// Decide whether to allow a request.
    fn decide(&self, request: &NetworkPolicyRequest) -> NetworkPolicyDecision;
}

// ---- Policy helpers ----

/// Normalize a hostname (stub — returns input unchanged).
pub fn normalize_host(host: &str) -> String {
    host.to_string()
}

// ---- Proxy env helpers ----

/// All proxy-related environment variable keys.
pub const ALL_PROXY_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// Environment variable key for allowing local binding.
pub const ALLOW_LOCAL_BINDING_ENV_KEY: &str = "CODEX_ALLOW_LOCAL_BINDING";

/// Default NO_PROXY value.
pub const DEFAULT_NO_PROXY_VALUE: &str = "";

/// NO_PROXY environment variable keys.
pub const NO_PROXY_ENV_KEYS: &[&str] = &["NO_PROXY", "no_proxy"];

/// Proxy URL environment variable keys.
pub const PROXY_URL_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// CLI arguments (stub).
#[derive(Debug, Clone, Default)]
pub struct Args;

/// Check whether proxy-URL env vars are present (stub — always false).
pub fn has_proxy_url_env_vars(_env: &HashMap<String, String>) -> bool {
    false
}

/// Get the proxy URL from env (stub — always None).
pub fn proxy_url_env_value<'a>(_env: &'a HashMap<String, String>) -> Option<&'a str> {
    None
}

// ---- NetworkProxy ----

/// Network proxy (zero-size stub for WASI).
#[derive(Clone)]
pub struct NetworkProxy;

impl fmt::Debug for NetworkProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkProxy").finish()
    }
}

impl PartialEq for NetworkProxy {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl NetworkProxy {
    /// Create a new builder (stub).
    pub fn builder() -> NetworkProxyBuilder {
        NetworkProxyBuilder
    }

    /// HTTP proxy address (stub — returns 127.0.0.1:0).
    pub fn http_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }

    /// SOCKS proxy address (stub — returns 127.0.0.1:0).
    pub fn socks_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }

    /// Get current config (stub).
    pub async fn current_cfg(&self) -> Result<NetworkProxyConfig, ProxyError> {
        Ok(NetworkProxyConfig)
    }

    /// Add allowed domain (stub — no-op).
    pub async fn add_allowed_domain(&self, _host: &str) -> Result<(), ProxyError> {
        Ok(())
    }

    /// Add denied domain (stub — no-op).
    pub async fn add_denied_domain(&self, _host: &str) -> Result<(), ProxyError> {
        Ok(())
    }

    /// Whether local binding is allowed (stub — always false).
    pub fn allow_local_binding(&self) -> bool {
        false
    }

    /// Allowed Unix sockets (stub — empty).
    pub fn allow_unix_sockets(&self) -> &[String] {
        &[]
    }

    /// Whether all Unix sockets are allowed (stub — false).
    pub fn dangerously_allow_all_unix_sockets(&self) -> bool {
        false
    }

    /// Apply proxy settings to environment (no-op on WASI).
    pub fn apply_to_env(&self, _env: &mut HashMap<String, String>) {}

    /// Run the proxy (stub — returns handle immediately).
    pub async fn run(&self) -> Result<NetworkProxyHandle, ProxyError> {
        Ok(NetworkProxyHandle)
    }
}

// ---- NetworkProxyBuilder ----

/// Builder for NetworkProxy (stub).
#[derive(Clone, Default)]
pub struct NetworkProxyBuilder;

impl NetworkProxyBuilder {
    /// Set state (stub — no-op).
    pub fn state(self, _state: Arc<NetworkProxyState>) -> Self {
        self
    }

    /// Set HTTP address (stub — no-op).
    pub fn http_addr(self, _addr: SocketAddr) -> Self {
        self
    }

    /// Set SOCKS address (stub — no-op).
    pub fn socks_addr(self, _addr: SocketAddr) -> Self {
        self
    }

    /// Set managed by codex flag (stub — no-op).
    pub fn managed_by_codex(self, _managed: bool) -> Self {
        self
    }

    /// Set policy decider (stub — no-op).
    pub fn policy_decider<D: NetworkPolicyDecider>(self, _decider: D) -> Self {
        self
    }

    /// Set policy decider from Arc (stub — no-op).
    pub fn policy_decider_arc(self, _decider: Arc<dyn NetworkPolicyDecider>) -> Self {
        self
    }

    /// Set blocked request observer (stub — no-op).
    pub fn blocked_request_observer<O: BlockedRequestObserver>(self, _observer: O) -> Self {
        self
    }

    /// Set blocked request observer from Arc (stub — no-op).
    pub fn blocked_request_observer_arc(self, _observer: Arc<dyn BlockedRequestObserver>) -> Self {
        self
    }

    /// Build the proxy (stub — returns immediately).
    pub async fn build(self) -> Result<NetworkProxy, ProxyError> {
        Ok(NetworkProxy)
    }
}

// ---- NetworkProxyHandle ----

/// Handle to a running proxy (stub).
#[derive(Debug)]
pub struct NetworkProxyHandle;

impl NetworkProxyHandle {
    /// Wait for the proxy to finish (stub — returns immediately).
    pub async fn wait(self) -> Result<(), ProxyError> {
        Ok(())
    }

    /// Shut down the proxy (stub — no-op).
    pub async fn shutdown(self) -> Result<(), ProxyError> {
        Ok(())
    }
}

// ---- Runtime types ----

/// Blocked request info (stub).
#[derive(Debug, Clone)]
pub struct BlockedRequest;

/// Blocked request arguments (stub).
#[derive(Debug, Clone)]
pub struct BlockedRequestArgs;

/// Observer for blocked requests (stub trait).
pub trait BlockedRequestObserver: Send + Sync + 'static {
    /// Called when a request is blocked.
    fn on_blocked(&self, request: &BlockedRequest);
}

/// Config reloader (stub).
#[derive(Debug)]
pub struct ConfigReloader;

/// Config state (stub).
#[derive(Debug, Clone, Default)]
pub struct ConfigState;

/// Network proxy state (stub).
#[derive(Debug, Clone, Default)]
pub struct NetworkProxyState;

// ---- State / audit types ----

/// Audit metadata for proxy decisions (stub).
#[derive(Debug, Clone, Default)]
pub struct NetworkProxyAuditMetadata {
    /// Session ID.
    pub session_id: Option<String>,
    /// Config source.
    pub config_source: Option<String>,
}

/// Error for proxy constraints (stub).
#[derive(Debug, Clone)]
pub struct NetworkProxyConstraintError;

impl fmt::Display for NetworkProxyConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "network proxy constraint error (stub)")
    }
}

impl std::error::Error for NetworkProxyConstraintError {}

/// Proxy constraints (stub).
#[derive(Debug, Clone, Default)]
pub struct NetworkProxyConstraints;

/// Partial network config (stub).
#[derive(Debug, Clone, Default)]
pub struct PartialNetworkConfig;

/// Partial proxy config (stub).
#[derive(Debug, Clone, Default)]
pub struct PartialNetworkProxyConfig;

/// Build config state (stub — returns default).
pub fn build_config_state() -> ConfigState {
    ConfigState
}

/// Validate policy against constraints (stub — always Ok).
pub fn validate_policy_against_constraints(
    _policy: &NetworkProxyConstraints,
    _config: &ConfigState,
) -> Result<(), NetworkProxyConstraintError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_env_helpers_do_not_expose_host_proxy_settings() {
        let mut env = HashMap::from([
            (
                "HTTP_PROXY".to_string(),
                "http://example.invalid:8080".to_string(),
            ),
            ("NO_PROXY".to_string(), "localhost".to_string()),
        ]);

        assert!(!has_proxy_url_env_vars(&env));
        assert_eq!(proxy_url_env_value(&env), None);

        NetworkProxy.apply_to_env(&mut env);

        assert_eq!(
            env.get("HTTP_PROXY").map(String::as_str),
            Some("http://example.invalid:8080")
        );
        assert_eq!(env.get("NO_PROXY").map(String::as_str), Some("localhost"));
    }

    #[test]
    fn proxy_stub_does_not_open_listening_ports() {
        let proxy = NetworkProxy;

        assert_eq!(proxy.http_addr(), SocketAddr::from(([127, 0, 0, 1], 0)));
        assert_eq!(proxy.socks_addr(), SocketAddr::from(([127, 0, 0, 1], 0)));
        assert!(!proxy.allow_local_binding());
        assert!(proxy.allow_unix_sockets().is_empty());
        assert!(!proxy.dangerously_allow_all_unix_sockets());
    }

    #[test]
    fn policy_helpers_remain_non_enforcing_stubs() {
        assert_eq!(normalize_host("Example.COM"), "Example.COM");
        assert!(
            validate_policy_against_constraints(&NetworkProxyConstraints, &ConfigState).is_ok()
        );
    }
}
