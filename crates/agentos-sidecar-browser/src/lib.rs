#![forbid(unsafe_code)]

//! Agent OS browser sidecar wrapper.

use agentos_sidecar_wrapper::AcpExtension;
use secure_exec_sidecar_browser::{
    BrowserExtension, BrowserExtensionContext, BrowserSidecar, BrowserSidecarBridge,
    BrowserSidecarConfig, BrowserSidecarError,
};

pub struct BrowserAcpExtension {
    _native: AcpExtension,
}

impl BrowserAcpExtension {
    pub fn new() -> Self {
        Self {
            _native: AcpExtension::new(),
        }
    }
}

impl Default for BrowserAcpExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserExtension for BrowserAcpExtension {
    fn namespace(&self) -> &str {
        agentos_protocol::ACP_EXTENSION_NAMESPACE
    }

    fn handle_request(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        payload: &[u8],
    ) -> Result<Vec<u8>, BrowserSidecarError> {
        let _ = context;
        let _ = payload;
        Err(BrowserSidecarError::InvalidState(String::from(
            "browser ACP extension request dispatch reached the Agent OS wrapper, but browser ACP handlers are not wired yet",
        )))
    }
}

pub fn extensions() -> Vec<Box<dyn BrowserExtension>> {
    vec![Box::new(BrowserAcpExtension::new())]
}

pub fn browser_sidecar<B>(
    bridge: B,
    config: BrowserSidecarConfig,
) -> Result<BrowserSidecar<B>, BrowserSidecarError>
where
    B: BrowserSidecarBridge,
    <B as secure_exec_bridge::BridgeTypes>::Error: std::fmt::Debug,
{
    BrowserSidecar::with_extensions(bridge, config, extensions())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_protocol::ACP_EXTENSION_NAMESPACE;

    #[test]
    fn browser_extensions_register_acp_namespace() {
        let extensions = extensions();

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].namespace(), ACP_EXTENSION_NAMESPACE);
    }

    #[test]
    fn browser_acp_extension_reaches_wrapper_request_hook() {
        let extension = BrowserAcpExtension::new();
        let mut host = NullBrowserExtensionHost;
        let mut context = BrowserExtensionContext::new(&mut host);

        let error = extension
            .handle_request(&mut context, b"acp-request")
            .expect_err("browser ACP handlers are not wired yet");
        assert!(error
            .to_string()
            .contains("browser ACP extension request dispatch reached the Agent OS wrapper"));
    }

    struct NullBrowserExtensionHost;

    impl secure_exec_sidecar_browser::BrowserExtensionHost for NullBrowserExtensionHost {
        fn write_file(
            &mut self,
            _vm_id: &str,
            _path: &str,
            _contents: Vec<u8>,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn read_file(&mut self, _vm_id: &str, _path: &str) -> Result<Vec<u8>, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn mkdir(
            &mut self,
            _vm_id: &str,
            _path: &str,
            _recursive: bool,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn read_dir(
            &mut self,
            _vm_id: &str,
            _path: &str,
        ) -> Result<Vec<String>, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn create_javascript_context(
            &mut self,
            _request: secure_exec_bridge::CreateJavascriptContextRequest,
        ) -> Result<secure_exec_bridge::GuestContextHandle, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn create_wasm_context(
            &mut self,
            _request: secure_exec_bridge::CreateWasmContextRequest,
        ) -> Result<secure_exec_bridge::GuestContextHandle, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn start_execution(
            &mut self,
            _request: secure_exec_bridge::StartExecutionRequest,
        ) -> Result<secure_exec_bridge::StartedExecution, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn write_stdin(
            &mut self,
            _request: secure_exec_bridge::WriteExecutionStdinRequest,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn close_stdin(
            &mut self,
            _request: secure_exec_bridge::ExecutionHandleRequest,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn kill_execution(
            &mut self,
            _request: secure_exec_bridge::KillExecutionRequest,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn poll_execution_event(
            &mut self,
            _request: secure_exec_bridge::PollExecutionEventRequest,
        ) -> Result<Option<secure_exec_bridge::ExecutionEvent>, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }
    }
}
