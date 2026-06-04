//! Port-based virtual networking (`fetch`).
//!
//! Ported from `packages/core/src/agent-os.ts` `fetch`. Dispatches to a guest server on `port`
//! inside the kernel, never the host. The request URL host is discarded (only `pathname`+`search`
//! are used); the body is only attached for non-GET/HEAD methods; the response body is base64-decoded.
//! Fully buffered both directions. Wire path is the existing `VmFetch` request/response.

use anyhow::Result;
use bytes::Bytes;

use crate::agent_os::AgentOs;

impl AgentOs {
    /// Fetch from a guest server listening on `port` inside the VM.
    ///
    /// `path` is derived from the request URI's `pathname`+`search`; the host is ignored. The body
    /// is only sent for methods other than GET/HEAD. The response body is base64-decoded.
    pub async fn fetch(
        &self,
        _port: u16,
        _request: http::Request<Bytes>,
    ) -> Result<http::Response<Bytes>> {
        todo!("parity: fetch over VmFetch (port-based virtual net)")
    }
}
