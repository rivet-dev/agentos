use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use agentos_client::InstalledSoftware;
use anyhow::{anyhow, bail, Result};
use rivetkit::{Ctx, Handles};
use serde::{Deserialize, Serialize};

use crate::actions::ConfigCommitMode;
use crate::config::{
    normalize_remote_source, RemotePackageSource, RemotePackageSourceInput, MAX_REMOTE_SOFTWARE,
};
use crate::{store, AgentOsActor, AgentOsActorState, ConfigSnapshot};

type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoftwareInstall {
    pub source: RemotePackageSourceInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

crate::register_action!(SoftwareInstall => SoftwareMutationResult, "software.install");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoftwareUninstall {
    pub package_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

crate::register_action!(SoftwareUninstall => SoftwareMutationResult, "software.uninstall");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoftwareList {}

crate::register_action!(SoftwareList => Vec<ActorInstalledSoftware>, "software.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorInstalledSoftware {
    pub package_id: String,
    pub digest: String,
    pub size_bytes: u64,
    pub package_name: String,
    pub version: String,
    pub commands: Vec<String>,
}

impl From<InstalledSoftware> for ActorInstalledSoftware {
    fn from(value: InstalledSoftware) -> Self {
        Self {
            package_id: value.package_id,
            digest: value.digest,
            size_bytes: value.size_bytes,
            package_name: value.package_name,
            version: value.version,
            commands: value.commands,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareMutationResult {
    pub software: Option<ActorInstalledSoftware>,
    pub source: RemotePackageSource,
    pub config: ConfigSnapshot,
}

impl Handles<SoftwareInstall> for AgentOsActor {
    type Future = BoxFuture<SoftwareMutationResult>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: SoftwareInstall) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let source = normalize_remote_source(action.source)?;
            let _mutation = self.config_mutation.lock().await;
            let current = self.snapshot().await;
            require_revision(&current, action.expected_revision)?;
            let installed =
                crate::preload::acquire_required_package(source.to_core(), None).await?;
            let resolved_source = RemotePackageSource::resolved(source.url.clone(), &installed);
            if current
                .desired
                .software
                .iter()
                .any(|entry| entry.package_id.as_deref() == Some(installed.package_id.as_str()))
            {
                crate::preload::observe_software_usage(&source.url, &installed).await;
                return Ok(SoftwareMutationResult {
                    software: Some(installed.into()),
                    source: resolved_source,
                    config: current,
                });
            }

            if current.desired.software.len() >= MAX_REMOTE_SOFTWARE {
                bail!(
                    "limit_exceeded: software has {} entries; maximum is {MAX_REMOTE_SOFTWARE}; uninstall a package before adding another",
                    current.desired.software.len()
                );
            }

            let mut desired = current.desired.clone();
            desired.software.push(resolved_source.clone());
            let live = can_apply_software_live(
                &current,
                self.runtime.status().await.lifecycle == crate::VmLifecycleState::Ready,
            );
            if live {
                self.runtime.install_software(&resolved_source).await?;
            }
            let next = match self
                .commit_config_locked(&ctx, &current, desired, software_commit_mode(live))
                .await
            {
                Ok(next) => next,
                Err(error) if live => {
                    return match self.runtime.uninstall_software(&installed.package_id).await {
                    Ok(_) => Err(error.context("persist software installation")),
                    Err(rollback_error) => Err(error.context(format!(
                        "persist software installation; Core rollback also failed: {rollback_error:#}"
                    ))),
                };
                }
                Err(error) => return Err(error),
            };
            crate::preload::observe_software_usage(&source.url, &installed).await;
            Ok(SoftwareMutationResult {
                software: Some(installed.into()),
                source: resolved_source,
                config: next,
            })
        })
    }
}

impl Handles<SoftwareUninstall> for AgentOsActor {
    type Future = BoxFuture<SoftwareMutationResult>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: SoftwareUninstall) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_package_id(&action.package_id)?;
            let _mutation = self.config_mutation.lock().await;
            let current = self.snapshot().await;
            require_revision(&current, action.expected_revision)?;
            let index = current
                .desired
                .software
                .iter()
                .position(|source| source.package_id.as_deref() == Some(&action.package_id))
                .ok_or_else(|| anyhow!("software_not_found: {}", action.package_id))?;
            let removed_source = current.desired.software[index].clone();
            let live = can_apply_software_live(
                &current,
                self.runtime.status().await.lifecycle == crate::VmLifecycleState::Ready,
            );
            let removed = if live {
                Some(self.runtime.uninstall_software(&action.package_id).await?)
            } else {
                None
            };

            let mut desired = current.desired.clone();
            desired.software.remove(index);
            let next = match self
                .commit_config_locked(&ctx, &current, desired, software_commit_mode(live))
                .await
            {
                Ok(next) => next,
                Err(error) if live => {
                    return match self.runtime.install_software(&removed_source).await {
                        Ok(_) => Err(error.context("persist software uninstall")),
                        Err(rollback_error) => Err(error.context(format!(
                        "persist software uninstall; Core rollback also failed: {rollback_error:#}"
                    ))),
                    };
                }
                Err(error) => return Err(error),
            };
            Ok(SoftwareMutationResult {
                software: removed.map(Into::into),
                source: removed_source,
                config: next,
            })
        })
    }
}

fn can_apply_software_live(current: &ConfigSnapshot, vm_ready: bool) -> bool {
    vm_ready
        && current.status == crate::ConfigApplyState::Ready
        && current.applied_revision == Some(current.revision)
}

fn software_commit_mode(live: bool) -> ConfigCommitMode {
    if live {
        ConfigCommitMode::Live
    } else {
        ConfigCommitMode::Classify
    }
}

impl Handles<SoftwareList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorInstalledSoftware>>;

    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: SoftwareList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            Ok(self
                .runtime
                .list_software()
                .await?
                .into_iter()
                .map(Into::into)
                .collect())
        })
    }
}

impl AgentOsActor {
    pub(crate) async fn pin_runtime_software(&self, ctx: &Ctx<Self>) -> Result<()> {
        let resolved = self.runtime.resolved_software().await;
        let mut next = self.snapshot().await;
        if resolved.len() != next.desired.software.len() {
            bail!(
                "runtime resolved {} packages for {} desired sources",
                resolved.len(),
                next.desired.software.len()
            );
        }
        let mut changed = false;
        for (source, resolved) in next.desired.software.iter_mut().zip(resolved) {
            if source.url != resolved.url {
                bail!("runtime package resolution order does not match desired software");
            }
            let pinned = RemotePackageSource::resolved(source.url.clone(), &resolved.installed);
            if *source != pinned {
                *source = pinned;
                changed = true;
            }
        }
        if changed {
            persist_snapshot(self, ctx, &next).await?;
        }
        for resolved in self.runtime.resolved_software().await {
            crate::preload::observe_software_usage(&resolved.url, &resolved.installed).await;
        }
        Ok(())
    }
}

pub(crate) async fn persist_snapshot(
    actor: &AgentOsActor,
    ctx: &Ctx<AgentOsActor>,
    snapshot: &ConfigSnapshot,
) -> Result<()> {
    store::persist(ctx, snapshot).await?;
    *actor.config.lock().await = snapshot.clone();
    ctx.set_state(AgentOsActorState {
        config: snapshot.clone(),
    });
    Ok(())
}

pub(crate) fn require_revision(snapshot: &ConfigSnapshot, expected: Option<u64>) -> Result<()> {
    if let Some(expected) = expected {
        if expected != snapshot.revision {
            bail!(
                "revision_conflict: expected revision {expected}, current revision is {}",
                snapshot.revision
            );
        }
    }
    Ok(())
}

fn validate_package_id(package_id: &str) -> Result<()> {
    let Some(digest) = package_id.strip_prefix("sha256:") else {
        bail!("invalid_input: packageId must use the sha256:<64 lowercase hex> form");
    };
    if digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || digest.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        bail!("invalid_input: packageId must use the sha256:<64 lowercase hex> form");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_id_is_content_addressed() {
        assert!(validate_package_id(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_package_id("package-name").is_err());
    }

    #[test]
    fn hosted_software_byte_count_names_its_unit() {
        let installed = ActorInstalledSoftware::from(InstalledSoftware {
            package_id: format!("sha256:{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
            size_bytes: 123,
            package_name: "example".into(),
            version: "1".into(),
            commands: Vec::new(),
        });
        let encoded = serde_json::to_value(installed).expect("encode hosted software");
        assert_eq!(encoded["sizeBytes"], 123);
        assert!(encoded.get("size").is_none());
    }

    #[test]
    fn software_mutations_use_desired_state_when_vm_is_failed_or_pending() {
        let mut snapshot = ConfigSnapshot {
            revision: 2,
            desired: Default::default(),
            applied_revision: Some(2),
            status: crate::ConfigApplyState::Ready,
            issues: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert!(can_apply_software_live(&snapshot, true));
        assert!(!can_apply_software_live(&snapshot, false));
        snapshot.applied_revision = Some(1);
        assert!(!can_apply_software_live(&snapshot, true));
        for state in [
            crate::ConfigApplyState::RestartRequired,
            crate::ConfigApplyState::Failed,
        ] {
            snapshot.status = state;
            assert!(!can_apply_software_live(&snapshot, true));
            assert_eq!(software_commit_mode(false), ConfigCommitMode::Classify);
        }
        let result = SoftwareMutationResult {
            software: None,
            source: RemotePackageSource {
                url: "https://example.com/unavailable.aospkg".into(),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                package_id: Some(format!("sha256:{}", "a".repeat(64))),
                size: Some(1),
            },
            config: snapshot,
        };
        let encoded = crate::action_set::encode_action_output(&result)
            .expect("uninstall can return the source without requiring package download metadata");
        assert!(!encoded.is_empty());
    }

    #[test]
    fn actor_source_cannot_deserialize_a_path_variant() {
        let error = serde_json::from_value::<RemotePackageSourceInput>(serde_json::json!({
            "url": "https://example.com/tool.aospkg",
            "path": "/tmp/tool.aospkg"
        }))
        .expect_err("host path must be rejected");
        assert!(error.to_string().contains("unknown field `path`"));
    }
}
