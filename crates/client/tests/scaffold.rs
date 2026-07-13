//! Integration test scaffold for `agentos-client`.
//!
//! Per repo rules, integration tests live under `tests/` (one module per SDK module, real
//! sidecar/kernel/fs, no mocks). The actual per-module suites land alongside their method
//! implementations. This file only asserts the crate's public surface is wired so the test target
//! compiles before any method bodies exist.

use agentos_client::SHELL_DISPOSE_TIMEOUT_MS;

#[test]
fn constants_are_exported() {
    assert_eq!(SHELL_DISPOSE_TIMEOUT_MS, 5_000);
}
