use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentos_client::{AgentOs, InstalledSoftware, SidecarState};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{AgentOsActorConfig, RemotePackageSource};
use crate::preload::ProcessPreloadReport;

const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RUNTIME_ISSUES: usize = 32;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmLifecycleState {
    Initializing,
    Preloading,
    Booting,
    Ready,
    Degraded,
    Stopping,
    Failed,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmIssue {
    pub code: String,
    pub message: String,
    pub at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageStartupStatus {
    pub required_total: u32,
    pub required_ready: u32,
    pub optional_preload_total: u32,
    pub optional_preload_ready: u32,
    pub optional_preload_failed: u32,
    pub optional_preload_skipped: u32,
    pub optional_preload_warmed_bytes: u64,
    pub optional_preload_deadline_hit: bool,
    pub optional_preload_coordinator_available: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSidecarStatus {
    pub state: String,
    pub active_vm_count: u32,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmStatusSnapshot {
    pub lifecycle: VmLifecycleState,
    pub config_state: crate::ConfigApplyState,
    pub desired_config_revision: u64,
    pub applied_config_revision: Option<u64>,
    pub generation: u64,
    pub last_boot_at_ms: Option<i64>,
    pub last_shutdown_at_ms: Option<i64>,
    pub packages: PackageStartupStatus,
    pub issues: Vec<VmIssue>,
    pub core: Option<CoreSidecarStatus>,
}

pub(crate) struct RuntimeController {
    operation: Mutex<()>,
    state: Mutex<RuntimeState>,
}

struct RuntimeState {
    lifecycle: VmLifecycleState,
    desired_config_revision: u64,
    applied_config_revision: Option<u64>,
    generation: u64,
    last_boot_at_ms: Option<i64>,
    last_shutdown_at_ms: Option<i64>,
    packages: PackageStartupStatus,
    issues: VecDeque<VmIssue>,
    resolved_software: Vec<ResolvedSoftware>,
    vm: Option<AgentOs>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSoftware {
    pub(crate) url: String,
    pub(crate) installed: InstalledSoftware,
}

impl RuntimeController {
    pub(crate) fn new(_actor_id: impl Into<String>, desired_config_revision: u64) -> Self {
        Self {
            operation: Mutex::new(()),
            state: Mutex::new(RuntimeState {
                lifecycle: VmLifecycleState::Initializing,
                desired_config_revision,
                applied_config_revision: None,
                generation: 0,
                last_boot_at_ms: None,
                last_shutdown_at_ms: None,
                packages: PackageStartupStatus {
                    required_total: 0,
                    required_ready: 0,
                    optional_preload_total: 0,
                    optional_preload_ready: 0,
                    optional_preload_failed: 0,
                    optional_preload_skipped: 0,
                    optional_preload_warmed_bytes: 0,
                    optional_preload_deadline_hit: false,
                    optional_preload_coordinator_available: false,
                },
                issues: VecDeque::new(),
                resolved_software: Vec::new(),
                vm: None,
            }),
        }
    }

    pub(crate) async fn set_process_preload_report(&self, report: &ProcessPreloadReport) {
        let mut state = self.state.lock().await;
        state.packages.optional_preload_total = report.total;
        state.packages.optional_preload_ready = report.ready;
        state.packages.optional_preload_failed = report.failed;
        state.packages.optional_preload_skipped = report.skipped;
        state.packages.optional_preload_warmed_bytes = report.warmed_bytes;
        state.packages.optional_preload_deadline_hit = report.deadline_hit;
        state.packages.optional_preload_coordinator_available = report.coordinator_available;
    }

    pub(crate) async fn boot(
        &self,
        ctx: &rivetkit::Ctx<crate::AgentOsActor>,
        desired: &AgentOsActorConfig,
        revision: u64,
    ) -> Result<VmStatusSnapshot> {
        let _operation = self.operation.lock().await;
        self.stop_inner("replacement").await?;
        let generation = match crate::store::allocate_vm_generation(ctx).await {
            Ok(generation) => generation,
            Err(error) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Failed;
                push_issue(
                    &mut state,
                    VmIssue {
                        code: "vm_generation_allocation_failed".into(),
                        message: error.to_string(),
                        at_ms: now_ms()?,
                    },
                );
                return Err(error.context("reserve durable VM generation"));
            }
        };
        {
            let mut state = self.state.lock().await;
            state.lifecycle = VmLifecycleState::Booting;
            state.desired_config_revision = revision;
            state.generation = generation;
            state.packages.required_total = u32::try_from(desired.software.len())
                .context("required software count exceeds u32")?;
            state.packages.required_ready = 0;
            state.resolved_software.clear();
        }

        let binary_path = std::env::current_exe()
            .context("resolve the agentOS executable for internal sidecar mode")?
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow!("agentOS executable path is not valid UTF-8"))?;
        let database_namespace = String::from("agentos-vm");
        let config = desired.to_core_config(
            Some(binary_path),
            Some(agentos_client::VmSqliteDescriptor::HostCallback {
                namespace: database_namespace.clone(),
            }),
            Some(crate::store::rivet_sqlite_callback(
                ctx.clone(),
                database_namespace,
            )),
        );

        let deadline = tokio::time::Instant::now() + INITIALIZATION_TIMEOUT;
        let mut creation = tokio::spawn(AgentOs::create(config));
        let vm = match tokio::time::timeout_at(deadline, &mut creation).await {
            Ok(Ok(Ok(vm))) => vm,
            Ok(Ok(Err(error))) => {
                return self
                    .fail_boot(
                        "runtime_boot_failed",
                        anyhow!(error).context("create agentOS VM"),
                    )
                    .await;
            }
            Ok(Err(error)) => {
                return self
                    .fail_boot(
                        "runtime_boot_failed",
                        anyhow!(error).context("join agentOS VM creation"),
                    )
                    .await;
            }
            Err(_) => {
                // Do not cancel VM creation after it may have opened a sidecar VM.
                // Reap any late result, including a successful VM, in the background.
                tokio::spawn(async move {
                    match creation.await {
                        Ok(Ok(vm)) => {
                            if let Err(error) = vm.shutdown().await {
                                tracing::error!(?error, "clean up VM created after boot deadline");
                            }
                        }
                        Ok(Err(error)) => {
                            tracing::warn!(?error, "late VM creation failed after boot deadline");
                        }
                        Err(error) => {
                            tracing::error!(?error, "join late VM creation after boot deadline");
                        }
                    }
                });
                return self
                    .fail_boot(
                        "runtime_boot_timeout",
                        anyhow!(
                            "timeout: runtime initialization exceeded {}ms while creating the VM; late creation cleanup is pending",
                            INITIALIZATION_TIMEOUT.as_millis()
                        ),
                    )
                    .await;
            }
        };
        {
            let mut state = self.state.lock().await;
            state.lifecycle = VmLifecycleState::Preloading;
            // Own the VM before awaiting package installation so cancelling
            // the install future cannot drop its only cleanup handle.
            state.vm = Some(vm.clone());
        }

        let desired_software = desired.software.clone();
        let initialize = async {
            let mut resolved = Vec::with_capacity(desired_software.len());
            for source in desired_software {
                let installed = vm.install_software(source.to_core()).await?;
                resolved.push(ResolvedSoftware {
                    url: source.url,
                    installed,
                });
                let mut state = self.state.lock().await;
                state.packages.required_ready = state
                    .packages
                    .required_ready
                    .checked_add(1)
                    .ok_or_else(|| {
                        agentos_client::ClientError::Sidecar(String::from(
                            "required package readiness count overflow",
                        ))
                    })?;
            }
            Ok::<_, agentos_client::ClientError>(resolved)
        };

        match tokio::time::timeout_at(deadline, initialize).await {
            Ok(Ok(resolved_software)) => {
                let booted_at_ms = now_ms()?;
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Ready;
                state.applied_config_revision = Some(revision);
                state.last_boot_at_ms = Some(booted_at_ms);
                state.resolved_software = resolved_software;
                Ok(snapshot(&state))
            }
            Ok(Err(error)) => self
                .fail_boot(
                    "runtime_boot_failed",
                    anyhow!(error).context("install required VM packages"),
                )
                .await,
            Err(_) => self
                .fail_boot(
                    "runtime_boot_timeout",
                    anyhow!(
                        "timeout: runtime initialization exceeded {}ms while installing required packages; raise the actor initialization deadline",
                        INITIALIZATION_TIMEOUT.as_millis()
                    ),
                )
                .await,
        }
    }

    async fn fail_boot(
        &self,
        code: &'static str,
        error: anyhow::Error,
    ) -> Result<VmStatusSnapshot> {
        let cleanup = self.stop_inner("failed boot").await;
        let mut state = self.state.lock().await;
        state.lifecycle = if cleanup.is_ok() {
            VmLifecycleState::Failed
        } else {
            VmLifecycleState::Degraded
        };
        let error = match cleanup {
            Ok(()) => error,
            Err(cleanup_error) => error.context(format!("VM cleanup failed: {cleanup_error:#}")),
        };
        push_issue(
            &mut state,
            VmIssue {
                code: code.into(),
                message: error.to_string(),
                at_ms: now_ms()?,
            },
        );
        Err(error)
    }

    pub(crate) async fn stop(&self, reason: &str) -> Result<VmStatusSnapshot> {
        let _operation = self.operation.lock().await;
        self.stop_inner(reason).await?;
        Ok(self.status().await)
    }

    pub(crate) async fn install_software(
        &self,
        source: &RemotePackageSource,
    ) -> Result<InstalledSoftware> {
        let _operation = self.operation.lock().await;
        let vm = self.vm().await?;
        let installed = vm
            .install_software(source.to_core())
            .await
            .context("install software in agentOS Core")?;
        let mut state = self.state.lock().await;
        if !state
            .resolved_software
            .iter()
            .any(|entry| entry.installed.package_id == installed.package_id)
        {
            state.resolved_software.push(ResolvedSoftware {
                url: source.url.clone(),
                installed: installed.clone(),
            });
            state.packages.required_total = state.packages.required_total.saturating_add(1);
            state.packages.required_ready = state.packages.required_ready.saturating_add(1);
        }
        Ok(installed)
    }

    pub(crate) async fn uninstall_software(&self, package_id: &str) -> Result<InstalledSoftware> {
        let _operation = self.operation.lock().await;
        let vm = self.vm().await?;
        let removed = vm
            .uninstall_software(package_id)
            .await
            .context("uninstall software from agentOS Core")?;
        let mut state = self.state.lock().await;
        state
            .resolved_software
            .retain(|entry| entry.installed.package_id != package_id);
        state.packages.required_total = state.packages.required_total.saturating_sub(1);
        state.packages.required_ready = state.packages.required_ready.saturating_sub(1);
        Ok(removed)
    }

    pub(crate) async fn list_software(&self) -> Result<Vec<InstalledSoftware>> {
        let vm = self.vm().await?;
        Ok(vm.installed_software())
    }

    pub(crate) async fn resolved_software(&self) -> Vec<ResolvedSoftware> {
        self.state.lock().await.resolved_software.clone()
    }

    pub(crate) async fn set_applied_config_revision(&self, revision: u64) {
        let mut state = self.state.lock().await;
        state.desired_config_revision = revision;
        state.applied_config_revision = Some(revision);
    }

    pub(crate) async fn set_desired_config_revision(&self, revision: u64) {
        self.state.lock().await.desired_config_revision = revision;
    }

    async fn stop_inner(&self, reason: &str) -> Result<()> {
        let vm = {
            let mut state = self.state.lock().await;
            let Some(vm) = state.vm.take() else {
                return Ok(());
            };
            state.lifecycle = VmLifecycleState::Stopping;
            state.resolved_software.clear();
            state.packages.required_ready = 0;
            vm
        };

        match tokio::time::timeout(SHUTDOWN_TIMEOUT, vm.shutdown()).await {
            Ok(Ok(())) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Initializing;
                state.applied_config_revision = None;
                state.last_shutdown_at_ms = Some(now_ms()?);
                tracing::info!(%reason, "agentOS Core runtime stopped");
                Ok(())
            }
            Ok(Err(error)) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Degraded;
                state.vm = Some(vm);
                push_issue(
                    &mut state,
                    VmIssue {
                        code: "runtime_shutdown_failed".into(),
                        message: error.to_string(),
                        at_ms: now_ms()?,
                    },
                );
                Err(anyhow!(error).context("stop agentOS Core runtime"))
            }
            Err(_) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Degraded;
                state.vm = Some(vm);
                push_issue(
                    &mut state,
                    VmIssue {
                        code: "runtime_shutdown_timeout".into(),
                        message: format!(
                            "runtime shutdown exceeded the fixed actor deadline of {}ms; completion is unconfirmed",
                            SHUTDOWN_TIMEOUT.as_millis()
                        ),
                        at_ms: now_ms()?,
                    },
                );
                Err(anyhow!(
                    "timeout: runtime shutdown exceeded {}ms",
                    SHUTDOWN_TIMEOUT.as_millis()
                ))
            }
        }
    }

    pub(crate) async fn status(&self) -> VmStatusSnapshot {
        let state = self.state.lock().await;
        snapshot(&state)
    }

    pub(crate) async fn vm(&self) -> Result<AgentOs> {
        let state = self.state.lock().await;
        if state.lifecycle != VmLifecycleState::Ready {
            return Err(anyhow!(
                "not_ready: agentOS VM is {:?}; inspect vm.status and retry",
                state.lifecycle
            ));
        }
        state
            .vm
            .clone()
            .ok_or_else(|| anyhow!("not_ready: ready VM has no Core handle"))
    }

    pub(crate) async fn vm_at_generation(&self, generation: u64) -> Result<AgentOs> {
        let state = self.state.lock().await;
        if state.generation != generation {
            return Err(anyhow!(
                "stale_generation: handle generation {generation} does not match current generation {}",
                state.generation
            ));
        }
        if state.lifecycle != VmLifecycleState::Ready {
            return Err(anyhow!(
                "not_ready: agentOS VM is {:?}; inspect vm.status and retry",
                state.lifecycle
            ));
        }
        state
            .vm
            .clone()
            .ok_or_else(|| anyhow!("not_ready: ready VM has no Core handle"))
    }
}

fn snapshot(state: &RuntimeState) -> VmStatusSnapshot {
    let core = state.vm.as_ref().map(|vm| {
        let description = vm.sidecar().describe();
        CoreSidecarStatus {
            state: match description.state {
                SidecarState::Ready => "ready",
                SidecarState::Disposing => "disposing",
                SidecarState::Disposed => "disposed",
            }
            .into(),
            active_vm_count: description.active_vm_count,
        }
    });
    VmStatusSnapshot {
        lifecycle: state.lifecycle,
        config_state: match state.lifecycle {
            VmLifecycleState::Failed | VmLifecycleState::Degraded => {
                crate::ConfigApplyState::Failed
            }
            VmLifecycleState::Initializing
            | VmLifecycleState::Preloading
            | VmLifecycleState::Booting
            | VmLifecycleState::Stopping => crate::ConfigApplyState::Applying,
            VmLifecycleState::Ready
                if state.applied_config_revision != Some(state.desired_config_revision) =>
            {
                crate::ConfigApplyState::RestartRequired
            }
            VmLifecycleState::Ready => crate::ConfigApplyState::Ready,
        },
        desired_config_revision: state.desired_config_revision,
        applied_config_revision: state.applied_config_revision,
        generation: state.generation,
        last_boot_at_ms: state.last_boot_at_ms,
        last_shutdown_at_ms: state.last_shutdown_at_ms,
        packages: state.packages.clone(),
        issues: state.issues.iter().cloned().collect(),
        core,
    }
}

fn push_issue(state: &mut RuntimeState, issue: VmIssue) {
    if state.issues.len() == MAX_RUNTIME_ISSUES {
        state.issues.pop_front();
    }
    state.issues.push_back(issue);
}

pub(crate) fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_millis();
    i64::try_from(millis).context("system clock exceeds signed millisecond range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ready_runtime_reports_a_pending_desired_revision() {
        let runtime = RuntimeController::new("actor", 1);
        {
            let mut state = runtime.state.lock().await;
            state.lifecycle = VmLifecycleState::Ready;
            state.applied_config_revision = Some(1);
        }
        runtime.set_desired_config_revision(2).await;

        let status = runtime.status().await;
        assert_eq!(status.lifecycle, VmLifecycleState::Ready);
        assert_eq!(
            status.config_state,
            crate::ConfigApplyState::RestartRequired
        );
        assert_eq!(status.desired_config_revision, 2);
        assert_eq!(status.applied_config_revision, Some(1));
    }
}
