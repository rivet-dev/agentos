//! wasm32-wasip1 stub: the in-guest HTTP proxy server is inert on wasi (the VM kernel
//! brokers network policy host-side). Signatures match the real crate; bodies no-op.
use std::net::SocketAddr;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;

use anyhow::Result;

use crate::network_policy::NetworkPolicyDecider;
use crate::state::NetworkProxyState;

pub async fn run_http_proxy(
    _state: Arc<NetworkProxyState>,
    _addr: SocketAddr,
    _policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
) -> Result<()> {
    Ok(())
}

pub async fn run_http_proxy_with_std_listener(
    _state: Arc<NetworkProxyState>,
    _listener: StdTcpListener,
    _policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
) -> Result<()> {
    Ok(())
}
