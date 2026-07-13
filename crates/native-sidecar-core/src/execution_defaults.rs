//! Sidecar-owned defaults for process execution requests.

use agentos_sidecar_protocol::protocol::ExecuteRequest;

/// Apply defaults that depend on the requested execution mode.
///
/// A PTY request without an explicit executable is the protocol representation
/// of opening the VM's default interactive shell. Streaming stdin is also the
/// natural PTY behavior. Explicit executable and stdin choices are preserved.
pub fn apply_execute_defaults(payload: &mut ExecuteRequest) {
    if payload.pty.is_none() {
        return;
    }

    if payload.command.is_none()
        && payload.shell_command.is_none()
        && payload.runtime.is_none()
        && payload.entrypoint.is_none()
    {
        payload.command = Some(String::from("sh"));
    }

    if payload.keep_stdin_open.is_none() {
        payload.keep_stdin_open = Some(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_sidecar_protocol::protocol::GuestRuntimeKind;
    use agentos_sidecar_protocol::wire::PtyOptions;

    fn request() -> ExecuteRequest {
        ExecuteRequest {
            process_id: Some(String::from("process-1")),
            command: None,
            shell_command: None,
            runtime: None,
            entrypoint: None,
            args: Vec::new(),
            env: None,
            cwd: None,
            wasm_permission_tier: None,
            pty: None,
            keep_stdin_open: None,
            timeout_ms: None,
        }
    }

    #[test]
    fn pty_without_executable_uses_sidecar_shell_defaults() {
        let mut payload = request();
        payload.pty = Some(PtyOptions {
            cols: Some(80),
            rows: Some(24),
        });

        apply_execute_defaults(&mut payload);

        assert_eq!(payload.command.as_deref(), Some("sh"));
        assert!(payload.args.is_empty());
        assert_eq!(payload.keep_stdin_open, Some(true));
    }

    #[test]
    fn explicit_terminal_choices_are_preserved() {
        let mut payload = request();
        payload.command = Some(String::from("bash"));
        payload.args = vec![String::from("--norc")];
        payload.pty = Some(PtyOptions {
            cols: None,
            rows: None,
        });
        payload.keep_stdin_open = Some(false);

        apply_execute_defaults(&mut payload);

        assert_eq!(payload.command.as_deref(), Some("bash"));
        assert_eq!(payload.args, ["--norc"]);
        assert_eq!(payload.keep_stdin_open, Some(false));
    }

    #[test]
    fn non_terminal_execution_does_not_gain_defaults() {
        let mut payload = request();
        payload.runtime = Some(GuestRuntimeKind::JavaScript);

        apply_execute_defaults(&mut payload);

        assert!(payload.command.is_none());
        assert_eq!(payload.keep_stdin_open, None);
    }
}
