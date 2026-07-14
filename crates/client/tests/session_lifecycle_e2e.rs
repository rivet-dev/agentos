//! Real-sidecar lifecycle regressions for connection-scoped wire sessions. These tests require no
//! guest command packages: they exercise failed VM initialization rollback and serialized shutdown.

mod common;

use agentos_client::config::{
    AgentOsConfig, AgentOsSidecarConfig, MountPlugin, RootFilesystemConfig, RootFilesystemKind,
};
use agentos_client::AgentOs;

#[tokio::test]
async fn failed_create_releases_session_capacity_and_concurrent_shutdown_is_idempotent() {
    if !common::require_sidecar(
        "failed_create_releases_session_capacity_and_concurrent_shutdown_is_idempotent",
    ) {
        return;
    }

    // This integration binary contains one test, and no sidecar has been spawned yet. Restricting
    // capacity to one makes a leaked OpenSession deterministic: the next create fails admission.
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var("AGENTOS_MAX_SESSIONS_PER_CONNECTION", "1");
    }

    let pool = String::from("rust-session-lifecycle-e2e");
    for attempt in 1..=3 {
        let result = AgentOs::create(AgentOsConfig {
            root_filesystem: RootFilesystemConfig {
                kind: RootFilesystemKind::Overlay,
                native_plugin: Some(MountPlugin {
                    id: String::from("invalid-overlay-plugin"),
                    config: None,
                }),
                ..Default::default()
            },
            sidecar: Some(AgentOsSidecarConfig::Shared {
                pool: Some(pool.clone()),
            }),
            ..Default::default()
        })
        .await;
        let Err(error) = result else {
            panic!("overlay root with a native plugin must fail client serialization");
        };
        assert!(
            error
                .to_string()
                .contains("rootFilesystem.nativePlugin requires type \"native\""),
            "attempt {attempt} must fail before OpenSession: {error}"
        );
    }

    // This input serializes successfully, then the real sidecar rejects it during initialization.
    // Repeating it proves that the post-open error path closes each newly allocated session.
    for attempt in 1..=3 {
        let result = AgentOs::create(AgentOsConfig {
            root_filesystem: RootFilesystemConfig {
                kind: RootFilesystemKind::Native,
                native_plugin: Some(MountPlugin {
                    id: String::from("missing-root-plugin"),
                    config: None,
                }),
                ..Default::default()
            },
            sidecar: Some(AgentOsSidecarConfig::Shared {
                pool: Some(pool.clone()),
            }),
            ..Default::default()
        })
        .await;
        let Err(error) = result else {
            panic!("unknown native root plugin must reject VM initialization");
        };
        assert!(
            !error.to_string().contains("session limit exceeded"),
            "attempt {attempt} observed leaked session capacity: {error}"
        );
    }

    // A valid create against the same connection proves all failed attempts authoritatively closed
    // their sessions. Concurrent callers then exercise the production shutdown mutex and the
    // sidecar's idempotent CloseSession response, not the unit-only ShutdownAttempt helper.
    let os = AgentOs::create(AgentOsConfig {
        sidecar: Some(AgentOsSidecarConfig::Shared {
            pool: Some(pool.clone()),
        }),
        ..Default::default()
    })
    .await
    .expect("failed creates must leave capacity for a valid VM");
    assert_eq!(os.sidecar().describe().active_vm_count, 1);

    let (a, b, c) = tokio::join!(os.shutdown(), os.shutdown(), os.shutdown());
    a.expect("first concurrent shutdown");
    b.expect("second concurrent shutdown");
    c.expect("third concurrent shutdown");
    os.shutdown()
        .await
        .expect("later shutdown remains idempotent");
    assert_eq!(
        os.sidecar().describe().active_vm_count,
        0,
        "confirmed shutdown must release its VM lease exactly once"
    );
}
