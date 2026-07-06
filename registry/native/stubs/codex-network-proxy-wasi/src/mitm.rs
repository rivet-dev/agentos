//! wasm32-wasip1 stub: MITM TLS interception is inert on wasi (host brokers TLS).
use anyhow::Result;

#[derive(Debug)]
pub struct MitmState;

impl MitmState {
    pub(crate) fn new(_allow_upstream_proxy: bool) -> Result<Self> {
        Ok(MitmState)
    }
}
