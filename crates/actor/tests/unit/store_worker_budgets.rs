use super::*;
use std::cell::Cell;

fn snapshot() -> ConfigSnapshot {
    ConfigSnapshot {
        revision: 7,
        desired: crate::AgentOsActorConfig::normalize(Default::default()).unwrap(),
        applied_revision: Some(7),
        status: crate::ConfigApplyState::Ready,
        issues: Vec::new(),
        created_at_ms: 10,
        updated_at_ms: 20,
    }
}

#[test]
fn saved_desired_bypasses_stale_create_input_and_missing_row_uses_initializer() {
    let saved = snapshot();
    let before = serde_json::to_vec(&saved).unwrap();
    let stale = crate::AgentOsActorConfigInput {
        environment: Some((0..257).map(|n| (format!("K{n}"), "v".into())).collect()),
        ..Default::default()
    };
    assert!(
        crate::AgentOsActorConfig::normalize(stale.clone())
            .unwrap_err()
            .to_string()
            .contains("actor.config.maxEnvironmentEntries"),
        "stale creation input independently fails actual new-worker validation"
    );
    let calls = Cell::new(0);
    let (loaded, created) = select_saved_or_initialize(Ok(Some(saved)), || {
        calls.set(calls.get() + 1);
        let desired = crate::AgentOsActorConfig::normalize(stale.clone())?;
        Ok(ConfigSnapshot {
            desired,
            ..snapshot()
        })
    })
    .unwrap();
    assert!(!created);
    assert_eq!(
        calls.get(),
        0,
        "saved desired document must win before creation input validation"
    );
    assert_eq!(serde_json::to_vec(&loaded).unwrap(), before);
    let (initial, created) = select_saved_or_initialize(Ok(None), || {
        calls.set(calls.get() + 1);
        Ok(snapshot())
    })
    .unwrap();
    assert!(created);
    assert_eq!(calls.get(), 1);
    assert_eq!(initial.revision, 7);
    let error = select_saved_or_initialize(Ok(None), || {
        calls.set(calls.get() + 1);
        let desired = crate::AgentOsActorConfig::normalize(stale)?;
        Ok(ConfigSnapshot {
            desired,
            ..snapshot()
        })
    })
    .unwrap_err();
    assert_eq!(calls.get(), 2);
    assert!(error
        .to_string()
        .contains("actor.config.maxEnvironmentEntries"));
    let error =
        select_saved_or_initialize(Err(anyhow::anyhow!("owning store read failure")), || {
            calls.set(calls.get() + 1);
            Ok(snapshot())
        })
        .unwrap_err();
    assert_eq!(
        calls.get(),
        2,
        "store errors must not fall back to initialization"
    );
    assert_eq!(error.to_string(), "owning store read failure");
}
