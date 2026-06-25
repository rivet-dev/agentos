//! Parse a `kernel.exec()` command line into a `(command, args)` pair for the sidecar.
//!
//! This mirrors the sidecar's child-process shell decision (`crates/sidecar/src/execution.rs`:
//! `tokenize_shell_free_command` / `command_requires_shell` / `is_posix_shell_builtin`) so the
//! top-level `exec` path makes the identical direct-spawn vs `sh -c` choice. A shell-free argv list
//! is spawned directly so the command keeps its real exit code (for example `cat /missing` reports
//! its own non-zero status); anything with shell syntax, or a POSIX shell builtin head, runs under
//! `sh -c <line>` with the original line passed as a single argv element, so there are no re-quoting
//! hazards. The sidecar still owns command lookup and host-path mapping for the resolved argv.

use anyhow::{bail, Result};

/// Split a command line on ASCII whitespace into non-empty tokens.
fn tokenize_shell_free_command(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether a command line contains any character that requires a real shell to interpret.
fn command_requires_shell(command: &str) -> bool {
    command.chars().any(|ch| {
        matches!(
            ch,
            '|' | '&'
                | ';'
                | '<'
                | '>'
                | '('
                | ')'
                | '$'
                | '`'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '~'
                | '\''
                | '"'
                | '\\'
                | '\n'
        )
    })
}

/// Whether a token should run under the shell even when it has no metacharacters.
fn is_posix_shell_builtin(command: &str) -> bool {
    matches!(
        command,
        "." | ":"
            | "break"
            | "cd"
            | "continue"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "times"
            | "trap"
            | "umask"
            | "unset"
            // Mirrors the TS sidecar client: direct wasm `pwd` reports the root
            // preopen on Darwin, while `sh -c pwd` observes the process cwd.
            | "pwd"
    )
}

/// Resolve an `exec` command line into the `(command, args)` pair to send to the sidecar.
///
/// Shell-free argv lists spawn directly; lines with shell syntax or a builtin head run under
/// `sh -c <line>`. An empty command line is an explicit error rather than a silent no-op.
pub(crate) fn resolve_exec_command(command: &str) -> Result<(String, Vec<String>)> {
    let tokens = tokenize_shell_free_command(command);
    let requires_shell = command_requires_shell(command)
        || tokens
            .first()
            .is_some_and(|head| is_posix_shell_builtin(head));
    if requires_shell {
        return Ok((
            String::from("sh"),
            vec![String::from("-c"), command.to_owned()],
        ));
    }
    let Some((head, args)) = tokens.split_first() else {
        bail!("exec: command must not be empty");
    };
    Ok((head.clone(), args.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::resolve_exec_command;

    /// A bare command with plain whitespace arguments takes the direct argv path.
    #[test]
    fn simple_command_splits_to_argv() {
        let (command, args) = resolve_exec_command("echo hello").unwrap();
        assert_eq!(command, "echo");
        assert_eq!(args, vec!["hello".to_string()]);
    }

    /// A single token with no arguments is a direct command with empty argv.
    #[test]
    fn single_token_is_direct() {
        let (command, args) = resolve_exec_command("echo").unwrap();
        assert_eq!(command, "echo");
        assert!(args.is_empty());
    }

    /// A non-zero-exit external command stays direct so it keeps its real exit code.
    #[test]
    fn missing_file_command_stays_direct() {
        let (command, args) = resolve_exec_command("cat /no/such/file").unwrap();
        assert_eq!(command, "cat");
        assert_eq!(args, vec!["/no/such/file".to_string()]);
    }

    /// Shell metacharacters route the whole line through `sh -c` as a single argv element.
    #[test]
    fn shell_syntax_wraps_in_sh_c() {
        for line in [
            "echo a && echo b",
            "echo hi > /tmp/x",
            "echo 'a b'",
            "ls *.txt",
            "a | b",
        ] {
            let (command, args) = resolve_exec_command(line).unwrap();
            assert_eq!(command, "sh", "line {line:?} should use sh -c");
            assert_eq!(args, vec!["-c".to_string(), line.to_string()]);
        }
    }

    /// A POSIX shell builtin head runs under `sh -c` even with no metacharacters.
    #[test]
    fn builtin_head_wraps_in_sh_c() {
        for line in ["cd /tmp", "pwd"] {
            let (command, args) = resolve_exec_command(line).unwrap();
            assert_eq!(command, "sh");
            assert_eq!(args, vec!["-c".to_string(), line.to_string()]);
        }
    }

    /// An empty or whitespace-only command line is an explicit error.
    #[test]
    fn empty_command_is_error() {
        assert!(resolve_exec_command("").is_err());
        assert!(resolve_exec_command("   ").is_err());
    }

    // ── Security: AOSCLIENT-P2-cmdline (N-009 guest exec line) ───────────────────────────────────
    //
    // Threat: an untrusted guest exec line that embeds command substitution, variable expansion,
    // or an embedded newline (command chaining) must be routed through `sh -c <line>` with the
    // ENTIRE original line as a SINGLE argv element. It must never be tokenized and direct-spawned
    // (which would mis-parse it) and the verbatim line must be preserved so the sidecar — not this
    // client — owns the shell decision and there are no re-quoting hazards. This asserts the
    // safeguard (`command_requires_shell`, command_line.rs:23) catches every such metachar.
    #[test]
    fn command_substitution_and_newline_metachars_route_through_sh_c_single_arg() {
        for line in [
            "echo `id`",      // backtick command substitution
            "echo $(whoami)", // $() command substitution
            "echo a$VAR",     // variable expansion
            "echo a\necho b", // embedded newline -> command chaining
            "echo $HOME",     // bare variable expansion
            "echo a;echo b",  // statement separator
        ] {
            let (command, args) = resolve_exec_command(line)
                .unwrap_or_else(|err| panic!("line {line:?} must resolve, got: {err}"));
            assert_eq!(
                command, "sh",
                "AOSCLIENT-P2-cmdline: line {line:?} contains shell metacharacters and must run \
                 under `sh`, not be direct-spawned"
            );
            assert_eq!(
                args,
                vec!["-c".to_string(), line.to_string()],
                "AOSCLIENT-P2-cmdline: line {line:?} must be passed to `sh -c` as a single \
                 verbatim argv element (no re-splitting / re-quoting)"
            );
        }
    }
}
