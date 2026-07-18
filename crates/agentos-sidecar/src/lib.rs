#![forbid(unsafe_code)]

//! Agent OS native sidecar wrapper.

mod acp;
mod session_store;

pub use acp::AcpExtension;

pub fn extensions() -> Vec<Box<dyn agentos_native_sidecar::Extension>> {
    vec![Box::new(AcpExtension::new())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_protocol::ACP_EXTENSION_NAMESPACE;

    #[test]
    fn extensions_register_acp_namespace() {
        let extensions = extensions();

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].namespace(), ACP_EXTENSION_NAMESPACE);
    }
}
