use agentos_actor_contract::{classify_public_error, ActionBudgetViolation};
use rivet_error::RivetError;

#[test]
fn typed_budget_projection_excludes_private_reason_and_outer_context() {
    let error = anyhow::Error::new(ActionBudgetViolation {
        setting: "actor.network.maxPreviewTokenBytes",
        observed: 32,
        limit: 1,
        units: "bytes",
        reason: "PRIVATE_REASON_SENTINEL".into(),
    })
    .context("PRIVATE_OUTER_SENTINEL");
    let classified = classify_public_error(error);
    let public = RivetError::extract(&classified);
    assert_eq!(public.group(), "user");
    assert_eq!(public.code(), "limit_exceeded");
    let wire = serde_json::to_value(public).unwrap();
    assert_eq!(
        wire["meta"],
        serde_json::json!({"budget": {
            "setting":"actor.network.maxPreviewTokenBytes", "observed":32, "limit":1,
            "units":"bytes", "remediation":"lower the per-call request or raise network.maxPreviewTokenBytes in --actor-budgets-json and restart this worker"
        }})
    );
    assert!(!wire.to_string().contains("PRIVATE_"));
    assert!(wire["message"]
        .as_str()
        .unwrap()
        .contains("--actor-budgets-json"));
}

#[test]
fn untyped_prefixes_and_internal_errors_do_not_gain_public_user_group() {
    for error in [
        anyhow::anyhow!("limit_exceeded: PRIVATE_PREFIX_SENTINEL"),
        anyhow::anyhow!("PRIVATE_INTERNAL_SENTINEL"),
    ] {
        let classified = classify_public_error(error);
        let public = RivetError::extract(&classified);
        assert_ne!(public.group(), "user");
    }
}

fn quota_error(
    setting: &str,
    unit: &str,
    limit: u64,
    observed: u64,
) -> agentos_client::ClientError {
    agentos_client::ClientError::ResourceLimit {
        code: "ERR_AGENTOS_RESOURCE_LIMIT".into(),
        message: "PRIVATE_NATIVE_MESSAGE".into(),
        details: Box::new(agentos_client::ResourceLimitDetails {
            limit_name: Some(setting.into()),
            configuration_path: Some(setting.into()),
            configured_limit: Some(limit),
            requested: Some(observed),
            unit: Some(unit.into()),
            errno: Some("ENOSPC".into()),
            vm_id: Some("PRIVATE_VM".into()),
            session_generation: Some(17),
            capability_id: Some(23),
            operation: Some("PRIVATE_OPERATION".into()),
            scope: Some("PRIVATE_SCOPE".into()),
            ..Default::default()
        }),
    }
}

#[test]
fn typed_filesystem_quota_is_public_exact_and_excludes_private_context() {
    for (setting, unit, limit, observed) in [
        (
            "limits.resources.maxFilesystemBytes",
            "bytes",
            1048576,
            1572864,
        ),
        ("limits.resources.maxInodeCount", "inodes", 17, 18),
        (
            "limits.resources.maxFilesystemBytes",
            "bytes",
            (1u64 << 53) + 1,
            (1u64 << 53) + 2,
        ),
        (
            "limits.resources.maxInodeCount",
            "inodes",
            u64::MAX - 1,
            u64::MAX,
        ),
    ] {
        let classified = classify_public_error(
            anyhow::Error::new(quota_error(setting, unit, limit, observed))
                .context("PRIVATE_OUTER"),
        );
        let public = RivetError::extract(&classified);
        assert_eq!(public.group(), "user");
        assert_eq!(public.code(), "limit_exceeded");
        let serialized = serde_json::to_string(&public).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            wire["meta"],
            serde_json::json!({"limit": {
                "setting":setting, "limit":limit.to_string(), "observed":observed.to_string(), "units":unit,
                "remediation":format!("reduce filesystem usage or raise {setting} in the VM configuration"),
            }})
        );
        assert!(!serialized.contains("PRIVATE_"));
        assert!(wire["meta"].get("budget").is_none());
        assert!(wire["message"].as_str().unwrap().contains(setting));
    }
}

#[test]
fn partial_mismatched_or_nonquota_errors_do_not_gain_public_user_group() {
    let base = || quota_error("limits.resources.maxFilesystemBytes", "bytes", 10, 11);
    for variant in 0..12 {
        let mut error = base();
        let agentos_client::ClientError::ResourceLimit { code, details, .. } = &mut error else {
            unreachable!()
        };
        match variant {
            0 => *code = "ERR_AGENTOS_OVERLOADED".into(),
            1 => details.errno = Some("ENOBUFS".into()),
            2 => details.current_usage = Some(0),
            3 => details.limit_name = Some("PRIVATE_UNKNOWN_SETTING".into()),
            4 => details.configuration_path = Some("limits.resources.maxInodeCount".into()),
            5 => details.unit = Some("inodes".into()),
            6 => details.configured_limit = None,
            7 => details.requested = None,
            8 => details.requested = Some(10),
            9 => details.limit_name = None,
            10 => details.configuration_path = None,
            11 => details.errno = None,
            _ => unreachable!(),
        }
        let classified = classify_public_error(anyhow::Error::new(error));
        assert_ne!(
            RivetError::extract(&classified).group(),
            "user",
            "variant {variant}"
        );
    }
    for error in [
        agentos_client::ClientError::Kernel {
            code: "ENOSPC".into(),
            message: "PRIVATE_BACKEND".into(),
        },
        agentos_client::ClientError::Sidecar("limit_exceeded: PRIVATE_UNTYPED".into()),
    ] {
        assert_ne!(
            RivetError::extract(&classify_public_error(anyhow::Error::new(error))).group(),
            "user"
        );
    }
}
