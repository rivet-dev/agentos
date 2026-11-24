#![forbid(unsafe_code)]

mod budgets;
pub use budgets::{
    configure_process_actor_budgets, HostedActorBudgetOptions, HostedActorBudgets,
    HostedAdmissionBudgetOptions, HostedCodecBudgetOptions, HostedConfigBudgetOptions,
    HostedCronBudgetOptions, HostedExecutionBudgetOptions, HostedFilesystemBudgetOptions,
    HostedLanguageBudgetOptions, HostedNetworkBudgetOptions, HostedPathBudgetOptions,
    HostedPreviewBudgetOptions, HostedProcessBudgetOptions, HostedRetentionBudgetOptions,
    HostedTransportBudgetOptions,
};
mod action_set;
mod actions;
mod config;
#[cfg(feature = "contract")]
pub mod contract;
mod cron;
mod events;
mod filesystem;
mod language;
mod network;
mod preload;
mod process;
mod runtime;
mod software;
mod store;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rivetkit::prelude::*;
use rivetkit::{action, Actor, ActorConfig, Registry, Request, Response};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

pub use actions::{ConfigGet, ConfigPatch, ConfigSet, VmRestart, VmStatus};
pub use config::{
    AgentOsActorConfig, AgentOsActorConfigInput, HostedFilesystemBackend,
    HostedFilesystemBackendInput, HostedFilesystemConfig, HostedFilesystemConfigInput,
    HostedFilesystemMount, HostedFilesystemMountInput, HostedRootFilesystem,
    HostedRootFilesystemInput, PreviewPolicy, PreviewPolicyInput, RemotePackageSource,
    RemotePackageSourceInput,
};
pub use cron::*;
pub use events::{
    CronFiredEvent, ProcessExitEvent, ProcessOutputEvent, TerminalExitEvent, TerminalOutputEvent,
    VmBooted, VmLimitWarning, VmShutdown,
};
pub use filesystem::{
    FileBytes, FileContentInput, FilesystemDirectoryEntry, FilesystemExists, FilesystemExport,
    FilesystemListMounts, FilesystemMkdir, FilesystemMove, FilesystemReadFile, FilesystemReadFiles,
    FilesystemReadResult, FilesystemReaddir, FilesystemReaddirEntries, FilesystemReaddirRecursive,
    FilesystemRemove, FilesystemStat, FilesystemWriteEntry, FilesystemWriteFile,
    FilesystemWriteFiles, FilesystemWriteResult,
};
pub use language::*;
pub use network::*;
pub use preload::{
    configure_preload_validation_policy, configure_process_preload, shutdown_process_preload,
    PreloadArtifact, PreloadBaselineReplaced, PreloadCoordinatorActor, PreloadCoordinatorConfig,
    PreloadCoordinatorConfigInput, PreloadCoordinatorCreateInput, PreloadCoordinatorStatus,
    PreloadGetPlan, PreloadPlan, PreloadProcessOptions, PreloadRecordUsage, PreloadReplaceBaseline,
    PreloadStatus, PreloadUsageAccepted, PreloadUsageObservation, PreloadValidationPolicy,
    ProcessPreloadReport, PRELOAD_COORDINATOR_ACTOR_KEY, PRELOAD_COORDINATOR_ACTOR_NAME,
    PRELOAD_PROTOCOL_VERSION,
};
pub use process::*;
pub use runtime::{
    CoreSidecarStatus, PackageStartupStatus, VmIssue, VmLifecycleState, VmStatusSnapshot,
};
pub use software::{SoftwareInstall, SoftwareList, SoftwareMutationResult, SoftwareUninstall};

use action_set::AgentOsActionSet;
use runtime::RuntimeController;

pub const ACTOR_NAME: &str = "agentOS";
const ACTION_CONCURRENCY_LIMIT: usize = 64;
const ACTOR_MESSAGE_SIZE_LIMIT: u32 = 1024 * 1024;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOsActorCreateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<AgentOsActorConfigInput>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplyState {
    Applying,
    Ready,
    RestartRequired,
    Failed,
}

impl ConfigApplyState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Applying => "applying",
            Self::Ready => "ready",
            Self::RestartRequired => "restart_required",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for ConfigApplyState {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "applying" => Ok(Self::Applying),
            "ready" => Ok(Self::Ready),
            "restart_required" => Ok(Self::RestartRequired),
            "failed" => Ok(Self::Failed),
            _ => bail!("invalid actor config apply state {value:?}"),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub revision: u64,
    pub desired: AgentOsActorConfig,
    pub applied_revision: Option<u64>,
    #[serde(rename = "state")]
    pub status: ConfigApplyState,
    pub issues: Vec<VmIssue>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub struct AgentOsActor {
    runtime: RuntimeController,
    budgets: Arc<HostedActorBudgets>,
    action_admission: Arc<Semaphore>,
    config_mutation: Mutex<()>,
}

#[async_trait]
impl Actor for AgentOsActor {
    type Input = AgentOsActorCreateInput;
    type Actions = AgentOsActionSet;
    type Events = (
        VmBooted,
        VmShutdown,
        VmLimitWarning,
        ProcessOutputEvent,
        ProcessExitEvent,
        TerminalOutputEvent,
        TerminalExitEvent,
        CronFiredEvent,
    );
    type ConnParams = ();
    type Action = action::Raw;

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let budgets = budgets::process_actor_budgets();
        // Existing desired state is authoritative; stale create input is only
        // normalized when the migrated store has no saved configuration row.
        let durable = store::load_or_initialize(ctx, || {
            let desired = AgentOsActorConfig::normalize_with_budgets(
                ctx.input()?.config.unwrap_or_default(),
                &budgets,
            )
            .context("validate agentOS actor creation config")?;
            let now = runtime::now_ms()?;
            Ok(ConfigSnapshot {
                revision: 1,
                desired,
                applied_revision: None,
                status: ConfigApplyState::Applying,
                issues: Vec::new(),
                created_at_ms: now,
                updated_at_ms: now,
            })
        })
        .await
        .context("initialize agentOS actor state store")?;
        let actor = Self::with_budgets(
            RuntimeController::new_with_budgets(
                ctx.actor_id(),
                durable.revision,
                Arc::clone(&budgets),
            ),
            budgets,
        );
        // A rejected restored policy or failed Core boot keeps vm.status reachable. Persist
        // the failure and let the actor start in the failed lifecycle state.
        match actor
            .runtime
            .boot(ctx, &durable.desired, durable.revision)
            .await
        {
            Ok(status) => {
                actor.pin_runtime_software(ctx).await?;
                actor.mark_runtime_result(ctx, &status).await?;
            }
            Err(error) => {
                tracing::error!(?error, actor_id = %ctx.actor_id(), "agentOS runtime boot failed");
                let status = actor.runtime.status().await;
                actor.mark_runtime_result(ctx, &status).await?;
            }
        }
        Ok(actor)
    }

    async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        let status = self.runtime.status().await;
        if status.lifecycle == VmLifecycleState::Ready {
            ctx.emit(VmBooted {
                generation: status.generation,
                config_revision: status.applied_config_revision.ok_or_else(|| {
                    anyhow::anyhow!("ready runtime has no applied config revision")
                })?,
                booted_at_ms: status
                    .last_boot_at_ms
                    .ok_or_else(|| anyhow::anyhow!("ready runtime has no boot timestamp"))?,
            })?;
        }
        Ok(())
    }

    async fn on_fetch(self: Arc<Self>, ctx: Ctx<Self>, request: Request) -> Result<Response> {
        network::handle_preview_fetch(self, ctx, request).await
    }

    async fn on_sleep(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        self.shutdown(&ctx, "sleep").await
    }

    async fn on_destroy(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        self.shutdown(&ctx, "destroy").await
    }
}

impl AgentOsActor {
    fn with_budgets(runtime: RuntimeController, budgets: Arc<HostedActorBudgets>) -> Self {
        Self {
            runtime: runtime.with_budgets(Arc::clone(&budgets)),
            action_admission: Arc::new(Semaphore::new(budgets.max_concurrent_actions)),
            budgets,
            config_mutation: Mutex::new(()),
        }
    }

    fn encode_budgeted_output(&self, output: &impl serde::Serialize) -> Result<Vec<u8>> {
        agentos_actor_contract::encode_action_output_with_limits(output, self.budgets.codec)
    }

    fn admit_action(&self) -> Result<OwnedSemaphorePermit> {
        let permit = self
            .action_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| agentos_actor_contract::ActionBudgetViolation {
                setting: "actor.admission.maxConcurrentActions",
                observed: self.budgets.max_concurrent_actions.saturating_add(1),
                limit: self.budgets.max_concurrent_actions,
                units: "actions",
                reason: "all actor action permits are occupied; no waiting queue is admitted"
                    .into(),
            })?;
        agentos_actor_contract::warn_near_budget(
            "actor.admission.maxConcurrentActions",
            self.budgets
                .max_concurrent_actions
                .saturating_sub(self.action_admission.available_permits()),
            self.budgets.max_concurrent_actions,
            "actions",
        );
        Ok(permit)
    }

    async fn snapshot(&self, ctx: &Ctx<Self>) -> Result<ConfigSnapshot> {
        store::load(ctx).await
    }

    async fn shutdown(&self, ctx: &Ctx<Self>, reason: &str) -> Result<()> {
        let before = self.runtime.status().await;
        self.runtime.stop(reason).await?;
        if before.generation > 0 {
            ctx.emit(VmShutdown {
                generation: before.generation,
                reason: reason.into(),
                shutdown_at_ms: runtime::now_ms()?,
            })?;
        }
        Ok(())
    }
}

fn hosted_actor_config_with_budgets(budgets: &HostedActorBudgets) -> ActorConfig {
    ActorConfig {
        has_database: true,
        action_timeout: Duration::from_secs(6 * 60),
        max_incoming_message_size: budgets.max_message_bytes,
        max_outgoing_message_size: budgets.max_message_bytes,
        ..ActorConfig::default()
    }
}

/// RivetKit registration configuration using the same frozen policy as actor dispatch.
/// Custom registries should use this builder rather than copy the hosted defaults.
pub fn hosted_actor_config() -> ActorConfig {
    hosted_actor_config_with_budgets(&budgets::process_actor_budgets())
}

// The coordinator shares only the transport owner; its framework defaults and
// action deadline remain distinct from the VM actor's policy.
fn preload_coordinator_config_with_budgets(budgets: &HostedActorBudgets) -> ActorConfig {
    ActorConfig {
        has_database: true,
        max_incoming_message_size: budgets.max_message_bytes,
        max_outgoing_message_size: budgets.max_message_bytes,
        ..ActorConfig::default()
    }
}

pub fn registry() -> Registry {
    let budgets = budgets::process_actor_budgets();
    let mut registry = Registry::new();
    registry.register_actor_with::<AgentOsActor>(
        ACTOR_NAME,
        hosted_actor_config_with_budgets(&budgets),
    );
    registry.register_actor_with::<PreloadCoordinatorActor>(
        PRELOAD_COORDINATOR_ACTOR_NAME,
        preload_coordinator_config_with_budgets(&budgets),
    );
    registry
}

#[cfg(test)]
mod tests {
    use rivetkit::{ActionSet, EventSet};

    use super::*;

    #[test]
    fn actor_name_and_initial_contract_are_fixed() {
        assert_eq!(ACTOR_NAME, "agentOS");
        let actions = <<AgentOsActor as Actor>::Actions as ActionSet<AgentOsActor>>::entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        let mut expected_actions = vec![
            "config.get",
            "config.set",
            "config.patch",
            "vm.status",
            "vm.restart",
            "filesystem.readFile",
            "filesystem.writeFile",
            "filesystem.readFiles",
            "filesystem.writeFiles",
            "filesystem.stat",
            "filesystem.mkdir",
            "filesystem.readdir",
            "filesystem.readdirEntries",
            "filesystem.readdirRecursive",
            "filesystem.exists",
            "filesystem.move",
            "filesystem.remove",
            "filesystem.export",
            "filesystem.listMounts",
            "process.run",
            "process.spawn",
            "process.get",
            "process.list",
            "process.tree",
            "process.wait",
            "process.signal",
            "process.stdin.write",
            "process.stdin.close",
            "process.pty.resize",
            "process.output.read",
            "terminal.open",
            "terminal.list",
            "terminal.output.read",
            "terminal.stdin.write",
            "terminal.pty.resize",
            "terminal.wait",
            "terminal.close",
            "contexts.create",
            "contexts.get",
            "contexts.list",
            "contexts.reset",
            "contexts.delete",
            "javascript.execute",
            "javascript.evaluate",
            "javascript.executeFile",
            "javascript.spawn",
            "javascript.spawnFile",
            "javascript.npm.install",
            "javascript.npm.runScript",
            "javascript.npm.runPackage",
            "typescript.execute",
            "typescript.evaluate",
            "typescript.executeFile",
            "typescript.spawn",
            "typescript.spawnFile",
            "typescript.check",
            "typescript.checkProject",
            "python.execute",
            "python.evaluate",
            "python.executeFile",
            "python.executeModule",
            "python.spawn",
            "python.spawnFile",
            "python.spawnModule",
            "python.install",
            "network.fetch",
            "network.fetchStream.start",
            "network.fetchStream.read",
            "network.fetchStream.cancel",
            "network.preview.create",
            "network.preview.expire",
            "cron.schedule",
            "cron.list",
            "cron.cancel",
            "__agentos.cron.invoke",
            "software.install",
            "software.uninstall",
            "software.list",
        ];
        expected_actions.sort_unstable();
        assert_eq!(actions, expected_actions);
        assert_eq!(
            <AgentOsActor as Actor>::Events::entries()
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>(),
            [
                "vm.booted",
                "vm.shutdown",
                "vm.limitWarning",
                "process.output",
                "process.exit",
                "terminal.output",
                "terminal.exit",
                "cron.fired",
            ]
        );
    }
}

#[cfg(test)]
#[path = "../tests/unit/hosted_budgets.rs"]
mod hosted_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/preload_registration_policy.rs"]
mod preload_registration_policy_tests;
