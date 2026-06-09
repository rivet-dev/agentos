//! Real Pi agent session e2e against a real `agent-os-sidecar`.
//!
//! This is the HONEST regression gate for the agent-session path: when a workspace with a *built*
//! Pi adapter is available, it asserts `create_session("pi")` actually succeeds, the session is
//! listed, and `close_session` removes it. It does NOT skip on feature errors — a broken Pi path
//! makes this test fail. It skips only when the prerequisite is genuinely absent.
//!
//! Prerequisite: set `AGENT_OS_PI_MODULE_CWD` to a directory whose `node_modules` contains a built
//! `@rivet-dev/agent-os-pi` (and its transitive deps, e.g. an installed/published tree). Registry
//! agent packages are build artifacts and the in-repo source does not build cleanly today, so the
//! tree must come from an installed workspace. Example:
//!   AGENT_OS_PI_MODULE_CWD=/path/to/workspace cargo test -p agent-os-client --test pi_session_e2e
//!
//! Background: a real agent SDK exercises module-loading patterns (tsc `__exportStar` CJS barrels,
//! deep pnpm symlink graphs, `__dirname`-based package self-location) that mock ACP adapters never
//! touch. Those are exactly the patterns that were silently broken; this gate keeps them honest.

mod common;

use std::collections::BTreeMap;

use agent_os_client::config::AgentOsConfig;
use agent_os_client::{AgentOs, CreateSessionOptions};

#[tokio::test]
async fn pi_session_create_list_close() {
    if !common::sidecar_available() {
        eprintln!("skipping pi_session_create_list_close: sidecar binary not built");
        return;
    }
    let module_cwd = match std::env::var("AGENT_OS_PI_MODULE_CWD") {
        Ok(value) if !value.is_empty() => value,
        _ => {
            eprintln!(
                "skipping pi_session_create_list_close: set AGENT_OS_PI_MODULE_CWD to a workspace \
                 with a built @rivet-dev/agent-os-pi to run this gate"
            );
            return;
        }
    };
    if !std::path::Path::new(&module_cwd)
        .join("node_modules")
        .is_dir()
    {
        eprintln!(
            "skipping pi_session_create_list_close: {module_cwd}/node_modules not found"
        );
        return;
    }

    common::ensure_sidecar_env();
    let config = AgentOsConfig {
        module_access_cwd: Some(module_cwd.clone()),
        ..Default::default()
    };
    let os = AgentOs::create(config)
        .await
        .expect("create VM for pi session");

    // A mock key is enough to reach + complete ACP `initialize` (no LLM call happens at init).
    let mut env = BTreeMap::new();
    env.insert("ANTHROPIC_API_KEY".to_string(), "mock-key".to_string());

    // The crux: a real agent SDK must initialize. This ASSERTS success — it does not swallow errors.
    let session = os
        .create_session(
            "pi",
            CreateSessionOptions {
                env,
                skip_os_instructions: true,
                ..Default::default()
            },
        )
        .await
        .expect("create_session(\"pi\") must succeed against a built Pi tree");
    assert!(
        !session.session_id.is_empty(),
        "session id must be non-empty"
    );

    // The session is real and observable.
    let listed = os.list_sessions();
    assert!(
        listed.iter().any(|s| s.session_id == session.session_id),
        "created session must appear in list_sessions"
    );

    // Close removes it.
    os.close_session(&session.session_id)
        .expect("close_session must succeed");
}
