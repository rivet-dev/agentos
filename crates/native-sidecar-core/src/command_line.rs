//! Sidecar-owned resolution of raw command lines.

/// A raw command line resolved to the command and argv the runtime should execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommandLine {
    pub command: String,
    pub args: Vec<String>,
}

/// Resolve a raw command line without asking an SDK client to parse shell syntax.
///
/// Plain whitespace-separated commands can execute directly. Shell syntax, shell
/// builtins, assignments, and reserved words preserve the original input as one
/// `sh -c` argument. `None` means the line was empty or whitespace-only.
pub fn resolve_command_line(command_line: &str) -> Option<ResolvedCommandLine> {
    let tokens = tokenize_shell_free_command(command_line);
    let requires_shell = command_requires_shell(command_line)
        || tokens.first().is_some_and(|command| {
            is_posix_shell_builtin(command) || shell_first_token_requires_shell(command)
        });

    if requires_shell {
        return Some(ResolvedCommandLine {
            command: String::from("sh"),
            args: vec![String::from("-c"), command_line.to_owned()],
        });
    }

    let (command, args) = tokens.split_first()?;
    Some(ResolvedCommandLine {
        command: command.clone(),
        args: args.to_vec(),
    })
}

fn tokenize_shell_free_command(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

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
            // The direct WASM implementation reports its root preopen on some
            // hosts; the guest shell observes the actual process cwd.
            | "pwd"
    )
}

fn shell_first_token_requires_shell(token: &str) -> bool {
    token.contains('=') || is_shell_reserved_word(token)
}

fn is_shell_reserved_word(token: &str) -> bool {
    matches!(
        token,
        "if" | "then"
            | "elif"
            | "else"
            | "fi"
            | "for"
            | "in"
            | "do"
            | "done"
            | "while"
            | "until"
            | "case"
            | "esac"
            | "{"
            | "}"
            | "!"
    )
}

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

#[cfg(test)]
mod tests {
    use super::{resolve_command_line, ResolvedCommandLine};

    #[test]
    fn plain_commands_resolve_to_direct_argv() {
        assert_eq!(
            resolve_command_line("cat /no/such/file"),
            Some(ResolvedCommandLine {
                command: String::from("cat"),
                args: vec![String::from("/no/such/file")],
            })
        );
    }

    #[test]
    fn shell_behavior_preserves_the_verbatim_line() {
        for line in [
            "echo a && echo b",
            "echo 'a b'",
            "echo $(whoami)",
            "FOO=bar env",
            "pwd",
        ] {
            assert_eq!(
                resolve_command_line(line),
                Some(ResolvedCommandLine {
                    command: String::from("sh"),
                    args: vec![String::from("-c"), line.to_owned()],
                }),
                "line {line:?}"
            );
        }
    }

    #[test]
    fn empty_lines_are_rejected() {
        assert_eq!(resolve_command_line(""), None);
        assert_eq!(resolve_command_line("   "), None);
    }
}
