//! Regression test: the ACP request timeout must not be too short (was a flat 120s).
//!
//! The original bug was a flat 120s ACP request timeout on the JS AcpClient that
//! long, multi-tool agent turns (`session/prompt`) routinely exceeded, causing the
//! turn to be aborted mid-flight. The fix bumped the `session/prompt` timeout to
//! 600s (600000ms). That timeout logic now lives in Rust, in the per-method
//! selector `request_timeout(method)` in `src/acp_extension.rs`.
//!
//! `request_timeout` is a private helper, so this test textually inlines the real
//! `acp_extension` source via `include!`. That makes the assertions below siblings of
//! the actual shipped `request_timeout`, so they exercise the production value (not a
//! copy). If a future refactor reverts `session/prompt` back to the old 120s default,
//! this build fails.
//!
//! NOTE: a separate test file is used (rather than editing the inline `#[cfg(test)]`
//! module or the existing `tests/acp_extension.rs`) to keep this regression guard
//! standalone and to avoid touching shared test files or production source.

// Pull the real production source in textually so we can call the private
// `request_timeout` helper as a sibling. `dead_code`/`unused` are silenced because we
// only exercise one helper out of the full module.
#[allow(dead_code, unused_imports, unused_variables, clippy::all)]
mod under_test {
    include!("../src/acp_extension.rs");
    // `Duration` is already imported by the included source above.

    /// `session/prompt` must use the patched 600s (600000ms) timeout, not the old 120s
    /// flat timeout that truncated long multi-tool agent turns.
    #[test]
    fn session_prompt_timeout_is_600s_not_120s() {
        let prompt_timeout = request_timeout("session/prompt");

        // Exactly the patched value.
        assert_eq!(
            prompt_timeout,
            Duration::from_secs(600),
            "session/prompt timeout must be 600s"
        );
        assert_eq!(
            u64::try_from(prompt_timeout.as_millis()).unwrap(),
            600_000,
            "session/prompt timeout must be 600000ms"
        );

        // And specifically NOT the regressed 120s flat timeout.
        assert_ne!(
            prompt_timeout,
            Duration::from_secs(120),
            "session/prompt must not regress to the old flat 120s ACP request timeout"
        );
    }

    /// The long-running `session/prompt` turn must get a strictly longer budget than the
    /// short control-method default, so a future refactor that collapses prompt back
    /// onto the default fails this assertion.
    #[test]
    fn session_prompt_exceeds_default_control_timeout() {
        let prompt_timeout = request_timeout("session/prompt");
        // An arbitrary non-overridden control method falls back to the default branch.
        let default_timeout = request_timeout("session/foo");

        assert_eq!(
            default_timeout,
            Duration::from_secs(120),
            "default (non-prompt) control-method timeout is expected to be 120s"
        );
        assert!(
            prompt_timeout > default_timeout,
            "session/prompt ({prompt_timeout:?}) must exceed the default control timeout ({default_timeout:?})"
        );
    }
}
