//! Integration test scaffold for `agentos-client`.
//!
//! Per repo rules, integration tests live under `tests/` (one module per SDK module, real
//! sidecar/kernel/fs, no mocks). The actual per-module suites land alongside their method
//! implementations. This file only asserts the crate's public surface is wired so the test target
//! compiles before any method bodies exist.

use agentos_client::{
    ACP_PROTOCOL_VERSION, CLOSED_SESSION_ID_RETENTION_LIMIT, CRON_JOB_LIMIT, PERMISSION_TIMEOUT_MS,
    SHELL_DISPOSE_TIMEOUT_MS, VM_READY_TIMEOUT_MS,
};

#[test]
fn constants_are_exported() {
    assert_eq!(ACP_PROTOCOL_VERSION, 1);
    assert_eq!(PERMISSION_TIMEOUT_MS, 120_000);
    assert_eq!(CLOSED_SESSION_ID_RETENTION_LIMIT, 2048);
    assert_eq!(SHELL_DISPOSE_TIMEOUT_MS, 5_000);
    assert_eq!(VM_READY_TIMEOUT_MS, 10_000);
    assert_eq!(CRON_JOB_LIMIT, 1024);
}
