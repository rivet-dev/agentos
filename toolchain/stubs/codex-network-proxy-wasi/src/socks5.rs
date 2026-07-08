//! wasm32-wasip1 stub: SOCKS5 proxy server inert on wasi (host-brokered policy).
use std::net::SocketAddr;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;

use anyhow::Result;

use crate::network_policy::NetworkPolicyDecider;
use crate::state::NetworkProxyState;

pub async fn run_socks5(
    _state: Arc<NetworkProxyState>,
    _addr: SocketAddr,
    _policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
    _enable_socks5_udp: bool,
) -> Result<()> {
    Ok(())
}

pub async fn run_socks5_with_std_listener(
    _state: Arc<NetworkProxyState>,
    _listener: StdTcpListener,
    _policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
    _enable_socks5_udp: bool,
) -> Result<()> {
    Ok(())
}
