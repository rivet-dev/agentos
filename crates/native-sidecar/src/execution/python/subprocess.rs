use super::super::*;

impl<B> NativeSidecar<B>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(in crate::execution) async fn handle_python_subprocess_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: PythonVfsRpcRequest,
    ) -> Result<(), SidecarError> {
        let Some(command) = request.command.clone() else {
            return self.respond_python_rpc(
                vm_id,
                process_id,
                request.id,
                Err(SidecarError::InvalidState(String::from(
                    "python subprocessRun requires a command",
                ))),
            );
        };
        let (internal_bootstrap_env, cwd) = {
            let Some(vm) = self.vms.get(vm_id) else {
                return Ok(());
            };
            let Some(process) = vm.active_processes.get(process_id) else {
                return Ok(());
            };
            let virtual_home = guest_virtual_home(vm);
            let cwd = request.cwd.clone().or_else(|| {
                guest_runtime_path_for_host_path(
                    &vm.guest_env,
                    &virtual_home,
                    &vm.host_cwd,
                    &process.host_cwd.to_string_lossy(),
                )
            });
            (
                sanitize_javascript_child_process_internal_bootstrap_env(&vm.guest_env),
                cwd,
            )
        };
        let result = self
            .begin_javascript_child_process_sync(
                vm_id,
                process_id,
                JavascriptChildProcessSpawnRequest {
                    command,
                    args: request.args.clone(),
                    options: JavascriptChildProcessSpawnOptions {
                        cwd,
                        env: request.env.clone(),
                        input: None,
                        internal_bootstrap_env,
                        shell: request.shell,
                        detached: false,
                        stdio: vec![
                            String::from("pipe"),
                            String::from("pipe"),
                            String::from("pipe"),
                        ],
                        timeout: None,
                        kill_signal: None,
                        ..JavascriptChildProcessSpawnOptions::default()
                    },
                },
                request.max_buffer,
                PendingChildProcessSyncCompletion::Python {
                    request_id: request.id,
                },
            )
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.respond_python_rpc(vm_id, process_id, request.id, Err(error)),
        }
    }
}
