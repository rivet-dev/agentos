use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use rivetkit::{Ctx, Handles};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::resolve_remote_software;
use crate::events::{VmBooted, VmShutdown};
use crate::runtime::VmStatusSnapshot;
use crate::software::{persist_snapshot, require_revision};
use crate::{store, AgentOsActor, AgentOsActorConfig, AgentOsActorConfigInput, ConfigSnapshot};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigGet {}

crate::register_action!(ConfigGet => ConfigSnapshot, "config.get");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigSet {
    pub config: AgentOsActorConfigInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

crate::register_action!(ConfigSet => ConfigSnapshot, "config.set");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigPatch {
    pub patch: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

crate::register_action!(ConfigPatch => ConfigSnapshot, "config.patch");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmStatus {}

crate::register_action!(VmStatus => VmStatusSnapshot, "vm.status");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmRestart {}

crate::register_action!(VmRestart => VmStatusSnapshot, "vm.restart");

impl Handles<ConfigGet> for AgentOsActor {
    type Future = BoxFuture<ConfigSnapshot>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ConfigGet) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            Ok(self.snapshot().await)
        })
    }
}

impl Handles<ConfigSet> for AgentOsActor {
    type Future = BoxFuture<ConfigSnapshot>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: ConfigSet) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;

            let _mutation = self.config_mutation.lock().await;
            let current = self.snapshot().await;
            require_revision(&current, action.expected_revision)?;
            let desired = normalize_and_resolve(action.config)
                .await
                .context("normalize replacement agentOS actor config")?;
            self.commit_config_locked(&ctx, &current, desired, ConfigCommitMode::Classify)
                .await
        })
    }
}

impl Handles<ConfigPatch> for AgentOsActor {
    type Future = BoxFuture<ConfigSnapshot>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: ConfigPatch) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let _mutation = self.config_mutation.lock().await;
            let current = self.snapshot().await;
            require_revision(&current, action.expected_revision)?;
            let input = merge_config_patch(&current.desired, action.patch)?;
            let desired = normalize_and_resolve(input)
                .await
                .context("normalize patched agentOS actor config")?;
            self.commit_config_locked(&ctx, &current, desired, ConfigCommitMode::Classify)
                .await
        })
    }
}

async fn normalize_and_resolve(input: AgentOsActorConfigInput) -> Result<AgentOsActorConfig> {
    let mut desired = AgentOsActorConfig::normalize(input)?;
    desired.software = resolve_remote_software(desired.software).await?;
    Ok(desired)
}

fn merge_config_patch(
    current: &AgentOsActorConfig,
    patch: Value,
) -> Result<AgentOsActorConfigInput> {
    if !patch.is_object() {
        anyhow::bail!("invalid_input: config.patch patch must be a JSON object");
    }
    let mut removals = Vec::new();
    collect_patch_removals(&patch, &mut Vec::new(), &mut removals);
    let mut document = serde_json::to_value(current).context("serialize current desired config")?;
    apply_merge_patch(&mut document, patch);
    let input = AgentOsActorConfigInput::deserialize(&document)
        .context("invalid_input: decode merged config input")?;
    // Validate erased members against the real serde DTOs as well. A null
    // removal of an unknown struct field must fail, while removing an absent
    // environment key or a known non-nullable field remains valid RFC 7386.
    // Probing a single null in an otherwise valid document distinguishes
    // unknown fields from valid fields whose input type rejects literal null.
    for path in removals {
        patch_parent_mut(&mut document, &path).insert(
            path.last().expect("removal has a field").clone(),
            Value::Null,
        );
        let probe = AgentOsActorConfigInput::deserialize(&document);
        patch_parent_mut(&mut document, &path).remove(path.last().expect("removal has a field"));
        if let Err(error) = probe {
            let message = error.to_string();
            if message.starts_with("unknown field ")
                || message.contains("did not match any variant of untagged enum")
            {
                return Err(error).context("invalid_input: validate config patch removal");
            }
        }
    }
    Ok(input)
}

fn collect_patch_removals(value: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if let Value::Object(object) = value {
        for (key, value) in object {
            path.push(key.clone());
            if value.is_null() {
                out.push(path.clone());
            } else {
                collect_patch_removals(value, path, out);
            }
            path.pop();
        }
    }
}

fn patch_parent_mut<'a>(
    document: &'a mut Value,
    path: &[String],
) -> &'a mut serde_json::Map<String, Value> {
    let mut parent = document;
    for key in &path[..path.len() - 1] {
        parent = parent.get_mut(key).expect("merge patch created the parent");
    }
    parent
        .as_object_mut()
        .expect("merge patch parent is an object")
}

fn apply_merge_patch(target: &mut Value, patch: Value) {
    let Value::Object(patch) = patch else {
        *target = patch;
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Default::default());
    }
    let target = target
        .as_object_mut()
        .expect("target was replaced with a JSON object");
    for (key, value) in patch {
        if value.is_null() {
            target.remove(&key);
        } else {
            apply_merge_patch(target.entry(key).or_insert(Value::Null), value);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigCommitMode {
    Classify,
    /// The changed field has already been applied live. Earlier pending
    /// configuration still requires its own VM restart.
    Live,
}

#[cfg(test)]
fn replacement_snapshot(
    current: &ConfigSnapshot,
    desired: AgentOsActorConfig,
    mode: ConfigCommitMode,
) -> Result<ConfigSnapshot> {
    let core_changed =
        mode == ConfigCommitMode::Classify && !current.desired.core_runtime_eq(&desired);
    replacement_snapshot_classified(current, desired, core_changed)
}

fn replacement_snapshot_classified(
    current: &ConfigSnapshot,
    desired: AgentOsActorConfig,
    core_changed: bool,
) -> Result<ConfigSnapshot> {
    let revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("actor config revision overflow"))?;
    let runtime_was_applied = current.applied_revision == Some(current.revision)
        && current.status == crate::ConfigApplyState::Ready;
    Ok(ConfigSnapshot {
        revision,
        desired,
        applied_revision: if core_changed {
            current.applied_revision
        } else if runtime_was_applied {
            Some(revision)
        } else {
            current.applied_revision
        },
        status: if core_changed {
            crate::ConfigApplyState::RestartRequired
        } else if runtime_was_applied {
            crate::ConfigApplyState::Ready
        } else {
            current.status
        },
        issues: if core_changed {
            Vec::new()
        } else {
            current.issues.clone()
        },
        created_at_ms: current.created_at_ms,
        updated_at_ms: crate::runtime::now_ms()?,
    })
}

impl AgentOsActor {
    pub(crate) async fn commit_config_locked(
        &self,
        ctx: &Ctx<Self>,
        current: &ConfigSnapshot,
        desired: AgentOsActorConfig,
        mode: ConfigCommitMode,
    ) -> Result<ConfigSnapshot> {
        if current.desired == desired {
            return Ok(current.clone());
        }
        let core_changed = if mode == ConfigCommitMode::Classify
            && !current.desired.core_runtime_eq(&desired)
        {
            if !current.desired.sidecar_comparison_covers_changes(&desired) {
                true
            } else if let Ok(vm) = self.runtime.vm().await {
                let before = current.desired.to_core_config(None, None, None);
                let after = desired.to_core_config(None, None, None);
                !agentos_client::actor_internals::vm_config_equivalent(&vm, &before, &after).await?
            } else {
                // No serving VM exists to resolve the live sidecar defaults.
                // Preserve the conservative replacement classification.
                true
            }
        } else {
            false
        };
        let next = replacement_snapshot_classified(current, desired, core_changed)?;
        // Reject an unreadable desired snapshot before changing durable state.
        crate::action_set::encode_action_output(&next)?;
        persist_snapshot(self, ctx, &next).await?;
        if next.applied_revision == Some(next.revision) {
            self.runtime
                .set_applied_config_revision(next.revision)
                .await;
        } else {
            self.runtime
                .set_desired_config_revision(next.revision)
                .await;
        }
        Ok(next)
    }
}

impl Handles<VmStatus> for AgentOsActor {
    type Future = BoxFuture<VmStatusSnapshot>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: VmStatus) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            Ok(self.runtime.status().await)
        })
    }
}

impl Handles<VmRestart> for AgentOsActor {
    type Future = BoxFuture<VmStatusSnapshot>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, _action: VmRestart) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let _mutation = self.config_mutation.lock().await;
            let before = self.runtime.status().await;
            self.runtime
                .stop("restart")
                .await
                .context("stop runtime for restart")?;
            if before.generation > 0 {
                ctx.emit(VmShutdown {
                    generation: before.generation,
                    reason: "restart".into(),
                    shutdown_at_ms: crate::runtime::now_ms()?,
                })?;
            }

            let desired = self.snapshot().await;
            let boot_result = self
                .runtime
                .boot(&ctx, &desired.desired, desired.revision)
                .await;
            let observed_status = self.runtime.status().await;
            self.mark_runtime_result(&ctx, &observed_status).await?;
            let status = boot_result.context("boot replacement runtime")?;
            self.pin_runtime_software(&ctx).await?;
            ctx.emit(VmBooted {
                generation: status.generation,
                config_revision: desired.revision,
                booted_at_ms: status
                    .last_boot_at_ms
                    .ok_or_else(|| anyhow::anyhow!("ready runtime has no boot timestamp"))?,
            })?;
            Ok(status)
        })
    }
}

impl AgentOsActor {
    pub(crate) async fn mark_runtime_result(
        &self,
        ctx: &Ctx<Self>,
        status: &VmStatusSnapshot,
    ) -> Result<()> {
        let mut snapshot = self.config.lock().await;
        snapshot.applied_revision = status.applied_config_revision;
        snapshot.status = if status.lifecycle == crate::VmLifecycleState::Ready {
            crate::ConfigApplyState::Ready
        } else {
            crate::ConfigApplyState::Failed
        };
        snapshot.issues = status.issues.clone();
        snapshot.updated_at_ms = crate::runtime::now_ms()?;
        ctx.set_state(crate::AgentOsActorState {
            config: snapshot.clone(),
        });
        store::persist(ctx, &snapshot).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigApplyState, PreviewPolicyInput};

    fn ready_snapshot(desired: AgentOsActorConfig) -> ConfigSnapshot {
        ConfigSnapshot {
            revision: 7,
            desired,
            applied_revision: Some(7),
            status: ConfigApplyState::Ready,
            issues: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn replacement_resets_omitted_fields_and_requires_restart_for_core_changes() {
        let current = ready_snapshot(
            AgentOsActorConfig::normalize(AgentOsActorConfigInput {
                environment: Some(std::collections::BTreeMap::from([(
                    String::from("TOKEN"),
                    String::from("value"),
                )])),
                high_resolution_time: Some(true),
                ..Default::default()
            })
            .expect("current config"),
        );
        let desired = AgentOsActorConfig::normalize(AgentOsActorConfigInput::default())
            .expect("default replacement");
        let next = replacement_snapshot(&current, desired, ConfigCommitMode::Classify)
            .expect("replacement");

        assert_eq!(next.revision, 8);
        assert_eq!(next.desired.environment, None);
        assert_eq!(next.desired.high_resolution_time, None);
        assert_eq!(next.applied_revision, Some(7));
        assert_eq!(next.status, ConfigApplyState::RestartRequired);
    }

    #[test]
    fn actor_only_preview_replacement_applies_without_restarting_core() {
        let current = ready_snapshot(AgentOsActorConfig::default());
        let desired = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            preview: Some(PreviewPolicyInput {
                max_active: Some(4),
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("preview replacement");
        assert!(current.desired.core_runtime_eq(&desired));

        let next = replacement_snapshot(&current, desired, ConfigCommitMode::Classify)
            .expect("replacement");
        assert_eq!(next.applied_revision, Some(8));
        assert_eq!(next.status, ConfigApplyState::Ready);
    }

    #[test]
    fn equivalent_desired_edit_preserves_pending_runtime_revision() {
        let ready = ready_snapshot(AgentOsActorConfig::default());
        let pending = replacement_snapshot_classified(
            &ready,
            AgentOsActorConfig {
                high_resolution_time: Some(true),
                ..ready.desired.clone()
            },
            true,
        )
        .unwrap();
        let mut desired = pending.desired.clone();
        desired.limits = Some(agentos_client::AgentOsLimits::default());
        let next = replacement_snapshot_classified(&pending, desired, false).unwrap();
        assert_eq!(next.revision, 9);
        assert_eq!(next.applied_revision, Some(7));
        assert_eq!(next.status, ConfigApplyState::RestartRequired);
    }

    #[test]
    fn live_software_change_preserves_pending_vm_configuration() {
        let ready = ready_snapshot(AgentOsActorConfig::default());
        let pending = replacement_snapshot(
            &ready,
            AgentOsActorConfig {
                high_resolution_time: Some(true),
                ..ready.desired.clone()
            },
            ConfigCommitMode::Classify,
        )
        .expect("pending configuration");
        let mut desired = pending.desired.clone();
        desired.software.push(crate::RemotePackageSource {
            url: String::from("https://example.com/package.aospkg"),
            digest: None,
            size: None,
            package_id: None,
        });

        let next = replacement_snapshot(&pending, desired.clone(), ConfigCommitMode::Live)
            .expect("live software update");
        assert_eq!(next.revision, 9);
        assert_eq!(next.applied_revision, Some(7));
        assert_eq!(next.status, ConfigApplyState::RestartRequired);

        let applied = replacement_snapshot(&ready, desired, ConfigCommitMode::Live)
            .expect("live update of an applied configuration");
        assert_eq!(applied.applied_revision, Some(8));
        assert_eq!(applied.status, ConfigApplyState::Ready);
    }

    #[test]
    fn revision_checks_conflict_and_unconditional_replacements_are_monotonic() {
        let current = ready_snapshot(AgentOsActorConfig::default());
        let error = require_revision(&current, Some(6)).expect_err("stale writer must fail");
        assert!(error.to_string().contains("revision_conflict"));
        require_revision(&current, Some(7)).expect("current writer");
        require_revision(&current, None).expect("unconditional writer");

        let first = replacement_snapshot(
            &current,
            AgentOsActorConfig {
                high_resolution_time: Some(true),
                ..Default::default()
            },
            ConfigCommitMode::Classify,
        )
        .expect("first replacement");
        let second = replacement_snapshot(
            &first,
            AgentOsActorConfig {
                environment: Some(std::collections::BTreeMap::new()),
                ..Default::default()
            },
            ConfigCommitMode::Classify,
        )
        .expect("last writer wins");
        assert_eq!(first.revision, 8);
        assert_eq!(second.revision, 9);
        assert_eq!(second.desired.environment, Some(Default::default()));
    }

    #[test]
    fn config_snapshot_uses_the_public_state_field() {
        let encoded = serde_json::to_value(ready_snapshot(AgentOsActorConfig::default()))
            .expect("serialize snapshot");
        assert_eq!(encoded["state"], "ready");
        assert!(encoded.get("status").is_none());
    }

    #[test]
    fn merge_patch_merges_objects_replaces_arrays_and_resets_nulls() {
        let current = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            environment: Some(std::collections::BTreeMap::from([
                (String::from("KEEP"), String::from("yes")),
                (String::from("CHANGE"), String::from("old")),
            ])),
            loopback_exempt_ports: Some(vec![3000, 4000]),
            high_resolution_time: Some(true),
            ..Default::default()
        })
        .expect("current config");

        let input = merge_config_patch(
            &current,
            serde_json::json!({
                "environment": { "CHANGE": "new" },
                "loopbackExemptPorts": [8080],
                "highResolutionTime": null
            }),
        )
        .expect("merge patch");
        let merged = AgentOsActorConfig::normalize(input).expect("normalize merged config");

        assert_eq!(
            merged.environment,
            Some(std::collections::BTreeMap::from([
                (String::from("CHANGE"), String::from("new")),
                (String::from("KEEP"), String::from("yes")),
            ]))
        );
        assert_eq!(merged.loopback_exempt_ports, vec![8080]);
        assert_eq!(merged.high_resolution_time, None);
    }

    #[test]
    fn merge_patch_rejects_non_objects_and_unknown_fields() {
        let current = AgentOsActorConfig::default();
        let error = merge_config_patch(&current, serde_json::json!([]))
            .expect_err("top-level array must fail");
        assert!(error.to_string().contains("must be a JSON object"));

        let error = merge_config_patch(&current, serde_json::json!({ "mystery": true }))
            .expect_err("unknown field must fail");
        assert!(format!("{error:#}").contains("unknown field"));

        let error = merge_config_patch(&current, serde_json::json!({ "mystery": null }))
            .expect_err("unknown field removal must fail");
        assert!(format!("{error:#}").contains("unknown field"));

        for patch in [
            serde_json::json!({ "preview": { "mystery": null } }),
            serde_json::json!({ "limits": { "mystery": null } }),
            serde_json::json!({ "user": { "mystery": null } }),
            serde_json::json!({ "filesystem": { "root": { "mystery": null } } }),
        ] {
            let error = merge_config_patch(&current, patch)
                .expect_err("nested unknown field removal must fail");
            assert!(format!("{error:#}").contains("unknown field"));
        }

        merge_config_patch(
            &current,
            serde_json::json!({
                "permissions": { "fs": { "rules": [], "mystery": null } }
            }),
        )
        .expect_err("unknown permission-rule field removal must fail");

        for patch in [
            serde_json::json!({ "limits": { "mystery": 1 } }),
            serde_json::json!({ "limits": { "resources": { "mystery": 1 } } }),
            serde_json::json!({ "permissions": { "mystery": "allow" } }),
        ] {
            merge_config_patch(&current, patch)
                .expect_err("unknown shared limit and permission fields must fail");
        }

        merge_config_patch(
            &current,
            serde_json::json!({
                "environment": { "ABSENT": null },
                "filesystem": { "mounts": null },
                "preview": { "maxTtlMs": null },
                "user": { "uid": null },
            }),
        )
        .expect("known field and arbitrary environment key removals must remain valid");
    }
}
