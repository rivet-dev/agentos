#![forbid(unsafe_code)]

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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rivetkit::prelude::*;
use rivetkit::{action, Actor, ActorConfig, Registry, Request, Response};
use rivetkit_core::inspector::InspectorTabEntry;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

pub use agentos_actor_contract::config::{
    AgentOsActorConfig, AgentOsActorConfigInput, HostedFilesystemBackend,
    HostedFilesystemBackendInput, HostedFilesystemConfig, HostedFilesystemConfigInput,
    HostedFilesystemMount, HostedFilesystemMountInput, HostedRootFilesystem,
    HostedRootFilesystemInput, PreviewPolicy, PreviewPolicyInput, RemotePackageSource,
    RemotePackageSourceInput,
};
pub use agentos_actor_contract::cron::{
    ActorCronJob, CronCancel, CronLaunchError, CronList, CronSchedule,
};
pub use agentos_actor_contract::events::{
    CronFiredEvent, ProcessExitEvent, ProcessOutputEvent, TerminalExitEvent, TerminalOutputEvent,
    VmBooted, VmLimitWarning, VmShutdown,
};
pub use agentos_actor_contract::filesystem::{
    ActorDirectoryEntry, ActorFileStat, FileBytes, FileContentInput, FilesystemDirectoryEntry,
    FilesystemExists, FilesystemExport, FilesystemListMounts, FilesystemMkdir, FilesystemMove,
    FilesystemReadFile, FilesystemReadFiles, FilesystemReadResult, FilesystemReaddir,
    FilesystemReaddirEntries, FilesystemReaddirRecursive, FilesystemRemove, FilesystemStat,
    FilesystemWriteEntry, FilesystemWriteFile, FilesystemWriteFiles, FilesystemWriteResult,
};
pub use agentos_actor_contract::language::{
    ActorCodeEvaluationResult, ActorCodeExecutionResult, ActorContextDescriptor, ActorContextId,
    ActorExecutionDescriptor, ActorExecutionError, ActorExecutionOutcome,
    ActorExecutionOutputOptions, ActorExecutionPtyOptions, ActorInlineExecutionOptions,
    ActorJavaScriptExecutionOptions, ActorJavaScriptModuleFormat, ActorLanguageExecutionOptions,
    ActorLanguageSpawnOptions, ActorNpmInstallOptions, ActorOutputCapture,
    ActorPythonInstallOptions, ActorTypeScriptCheckOptions, ActorTypeScriptCheckResult,
    ActorTypeScriptDiagnostic, ActorTypeScriptExecutionOptions, ContextsCreate, ContextsDelete,
    ContextsGet, ContextsList, ContextsReset, JavaScriptEvaluate, JavaScriptExecute,
    JavaScriptExecuteFile, JavaScriptNpmInstall, JavaScriptNpmRunPackage, JavaScriptNpmRunScript,
    JavaScriptSpawn, JavaScriptSpawnFile, PythonEvaluate, PythonExecute, PythonExecuteFile,
    PythonExecuteModule, PythonInstall, PythonSpawn, PythonSpawnFile, PythonSpawnModule,
    TypeScriptCheck, TypeScriptCheckProject, TypeScriptEvaluate, TypeScriptExecute,
    TypeScriptExecuteFile, TypeScriptSpawn, TypeScriptSpawnFile,
};
pub use agentos_actor_contract::lifecycle::*;
pub use agentos_actor_contract::network::{
    ActorFetchStreamChunk, ActorFetchStreamHead, ActorFetchStreamId, ActorHttpRequest,
    ActorHttpResponse, ActorPreview, NetworkFetch, NetworkFetchStreamCancel,
    NetworkFetchStreamRead, NetworkFetchStreamStart, NetworkPreviewCreate, NetworkPreviewExpire,
};
pub use agentos_actor_contract::process::{
    ActorExecOptions, ActorExecResult, ActorExitStatus, ActorOutputEvent, ActorOutputReplay,
    ActorProcessExit, ActorProcessId, ActorProcessInfo, ActorProcessTree, ActorProcessTreeNode,
    ActorSignal, ActorSpawnOptions, ActorTerminalExit, ActorTerminalId, ActorTerminalInfo,
    ActorTerminalOptions, ProcessGet, ProcessList, ProcessOutputRead, ProcessPtyResize, ProcessRun,
    ProcessSignal, ProcessSpawn, ProcessStdinClose, ProcessStdinWrite, ProcessTree, ProcessWait,
    TerminalClose, TerminalList, TerminalOpen, TerminalOutputRead, TerminalPtyResize,
    TerminalStdinWrite, TerminalWait,
};
pub use agentos_actor_contract::software::{
    ActorInstalledSoftware, SoftwareInstall, SoftwareList, SoftwareMutationResult,
    SoftwareUninstall,
};
pub use preload::{
    configure_process_preload, shutdown_process_preload, PreloadArtifact, PreloadBaselineReplaced,
    PreloadCoordinatorActor, PreloadCoordinatorConfig, PreloadCoordinatorConfigInput,
    PreloadCoordinatorCreateInput, PreloadCoordinatorStatus, PreloadGetPlan, PreloadPlan,
    PreloadProcessOptions, PreloadRecordUsage, PreloadReplaceBaseline, PreloadStatus,
    PreloadUsageAccepted, PreloadUsageObservation, ProcessPreloadReport,
    PRELOAD_COORDINATOR_ACTOR_KEY, PRELOAD_COORDINATOR_ACTOR_NAME, PRELOAD_PROTOCOL_VERSION,
};

use action_set::AgentOsActionSet;
use runtime::RuntimeController;

pub const ACTOR_NAME: &str = "agentOS";
const ACTION_CONCURRENCY_LIMIT: usize = 64;
const ACTOR_MESSAGE_SIZE_LIMIT: u32 = 1024 * 1024;

pub struct AgentOsActor {
    config: Mutex<ConfigSnapshot>,
    runtime: RuntimeController,
    action_admission: Arc<Semaphore>,
    config_mutation: Mutex<()>,
}

#[async_trait]
impl Actor for AgentOsActor {
    type State = AgentOsActorState;
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
    type Queue = ();
    type ConnParams = ();
    type ConnState = ();
    type Action = action::Raw;

    const HAS_DATABASE: bool = true;

    async fn create_state(_ctx: &Ctx<Self>, input: Self::Input) -> Result<Self::State> {
        let now = runtime::now_ms()?;
        let desired = AgentOsActorConfig::normalize(input.config.unwrap_or_default())
            .context("validate agentOS actor creation config")?;
        Ok(AgentOsActorState {
            config: ConfigSnapshot {
                revision: 1,
                desired,
                applied_revision: None,
                status: ConfigApplyState::Applying,
                issues: Vec::new(),
                created_at_ms: now,
                updated_at_ms: now,
            },
        })
    }

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let initial = ctx.state().config.clone();
        let durable = store::load_or_initialize(ctx, &initial)
            .await
            .context("initialize agentOS actor state store")?;
        ctx.set_state(AgentOsActorState {
            config: durable.clone(),
        });
        // The first agentOS actor created in a process performs the one bounded
        // advisory warm. Concurrent and later actors share its OnceCell result.
        let process_preload = preload::warm_process_once(ctx).await;
        let actor = Self {
            runtime: RuntimeController::new(ctx.actor_id(), durable.revision),
            config: Mutex::new(durable.clone()),
            action_admission: Arc::new(Semaphore::new(ACTION_CONCURRENCY_LIMIT)),
            config_mutation: Mutex::new(()),
        };
        actor
            .runtime
            .set_process_preload_report(&process_preload)
            .await;

        // A failed Core boot does not make vm.status unreachable. Persist
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
    fn admit_action(&self) -> Result<OwnedSemaphorePermit> {
        self.action_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow::anyhow!(
                    "limit_exceeded: actor action concurrency limit {ACTION_CONCURRENCY_LIMIT} reached; raise ACTION_CONCURRENCY_LIMIT"
                )
            })
    }

    async fn snapshot(&self) -> ConfigSnapshot {
        self.config.lock().await.clone()
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

pub fn registry() -> Registry {
    registry_with_inspector_tabs(None)
}

pub fn registry_with_inspector_tabs(inspector_root: Option<PathBuf>) -> Registry {
    let mut registry = Registry::new();
    let inspector_tabs = inspector_root.map_or_else(Vec::new, |root| {
        vec![
            inspector_tab("filesystem", "Filesystem", "folder-tree", &root),
            inspector_tab("processes", "Processes", "list-tree", &root),
            inspector_tab("terminal", "Terminal", "terminal", &root),
            inspector_tab("system", "System", "layer-group", &root),
            InspectorTabEntry::HideBuiltin {
                id: String::from("workflow"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("database"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("state"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("queue"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("schedules"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("connections"),
            },
            InspectorTabEntry::HideBuiltin {
                id: String::from("console"),
            },
        ]
    });
    registry.register_actor_with::<AgentOsActor>(
        ACTOR_NAME,
        ActorConfig {
            // Language/process actions permit up to five minutes of guest work.
            // Leave time for actor admission and response serialization.
            action_timeout: Duration::from_secs(6 * 60),
            max_incoming_message_size: ACTOR_MESSAGE_SIZE_LIMIT,
            max_outgoing_message_size: ACTOR_MESSAGE_SIZE_LIMIT,
            inspector_tabs,
            ..ActorConfig::default()
        },
    );
    registry.register_actor_with::<PreloadCoordinatorActor>(
        PRELOAD_COORDINATOR_ACTOR_NAME,
        ActorConfig {
            max_incoming_message_size: ACTOR_MESSAGE_SIZE_LIMIT,
            max_outgoing_message_size: ACTOR_MESSAGE_SIZE_LIMIT,
            ..ActorConfig::default()
        },
    );
    registry
}

fn inspector_tab(id: &str, label: &str, icon: &str, root: &std::path::Path) -> InspectorTabEntry {
    InspectorTabEntry::Custom {
        id: id.to_owned(),
        label: label.to_owned(),
        icon: Some(icon.to_owned()),
        root: root.to_owned(),
    }
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
