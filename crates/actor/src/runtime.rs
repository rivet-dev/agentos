use crate::budgets::{HostedActorBudgets, ValidationBound};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentos_client::{AgentOs, InstalledSoftware, SidecarState};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{AgentOsActorConfig, RemotePackageSource};
use crate::preload::ProcessPreloadReport;

const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

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
    budgets: Arc<HostedActorBudgets>,
    operation: Mutex<()>,
    state: Mutex<RuntimeState>,
    pending_creation: Mutex<Option<PendingVmCreation>>,
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
    vm: Option<Arc<RuntimeVmOwner>>,
}

type VmCreationFuture =
    Pin<Box<dyn Future<Output = Result<Arc<RuntimeVmOwner>, agentos_client::ClientError>> + Send>>;

enum PendingVmCreation {
    Running(VmCreationFuture),
    Created(Arc<RuntimeVmOwner>),
}

#[derive(Debug)]
enum CreationWaitError {
    Failed(agentos_client::ClientError),
    DeadlineElapsed,
}

/// Keeps a completed VM owned through publication and cancellation boundaries.
/// Explicit shutdown retains this owner in state until Core confirms completion.
struct RuntimeVmOwner {
    vm: AgentOs,
    shutdown_confirmed: AtomicBool,
    #[cfg(test)]
    shutdown_gate: Option<Arc<TestShutdownGate>>,
}

impl RuntimeVmOwner {
    fn new(vm: AgentOs) -> Self {
        Self {
            vm,
            shutdown_confirmed: AtomicBool::new(false),
            #[cfg(test)]
            shutdown_gate: None,
        }
    }

    async fn shutdown(&self) -> Result<(), agentos_client::ClientError> {
        #[cfg(test)]
        if let Some(gate) = &self.shutdown_gate {
            gate.started.notify_one();
            let permit = gate
                .release
                .acquire()
                .await
                .expect("test shutdown gate remains open");
            permit.forget();
        }
        self.vm.shutdown().await?;
        self.shutdown_confirmed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for RuntimeVmOwner {
    fn drop(&mut self) {
        if self.shutdown_confirmed.load(Ordering::SeqCst) {
            return;
        }
        // Actor::create can be cancelled before the actor's lifecycle hooks exist.
        // Match Core's creation guard: retain the VM in bounded host cleanup,
        // with the terminal worker sidecar drain as the final ownership boundary.
        let vm = self.vm.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    match tokio::time::timeout(SHUTDOWN_TIMEOUT, vm.shutdown()).await {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => tracing::error!(%error, "cleanup of an abandoned hosted VM failed; worker sidecar drain remains required"),
                        Err(_) => tracing::error!(deadline_ms = SHUTDOWN_TIMEOUT.as_millis(), "cleanup of an abandoned hosted VM timed out; worker sidecar drain remains required"),
                    }
                });
            }
            Err(error) => {
                tracing::error!(%error, "hosted VM owner dropped outside its runtime; worker sidecar drain remains required")
            }
        }
    }
}

#[cfg(test)]
struct TestShutdownGate {
    started: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSoftware {
    pub(crate) url: String,
    pub(crate) installed: InstalledSoftware,
}

impl RuntimeController {
    #[cfg(test)]
    pub(crate) fn new(actor_id: impl Into<String>, desired_config_revision: u64) -> Self {
        Self::new_with_budgets(
            actor_id,
            desired_config_revision,
            Arc::new(
                HostedActorBudgets::normalize(Default::default()).expect("built-in actor budgets"),
            ),
        )
    }

    pub(crate) fn new_with_budgets(
        _actor_id: impl Into<String>,
        desired_config_revision: u64,
        budgets: Arc<HostedActorBudgets>,
    ) -> Self {
        Self {
            budgets,
            operation: Mutex::new(()),
            pending_creation: Mutex::new(None),
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

    // Bind the immutable actor owner during construction, before runtime use.
    pub(crate) fn with_budgets(mut self, budgets: Arc<HostedActorBudgets>) -> Self {
        self.budgets = budgets;
        self
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
        self.admit_boot_policy(desired, revision).await?;
        // Only admitted configuration may activate advisory preload or a VM.
        // Completed warming is shared process-wide; canceled initialization may retry.
        let process_preload = crate::preload::warm_process_once(ctx).await;
        self.set_process_preload_report(&process_preload).await;
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
                    self.budgets.retention.max_runtime_issues,
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
        self.begin_creation(Box::pin(async move {
            AgentOs::create(config)
                .await
                .map(|vm| Arc::new(RuntimeVmOwner::new(vm)))
        }))
        .await?;
        let owner = match self.finish_pending_creation(deadline).await {
            Ok(Some(owner)) => owner,
            Ok(None) => return self.fail_boot("runtime_boot_failed", anyhow!("VM creation completed without an owned result")).await,
            Err(CreationWaitError::Failed(error)) => return self.fail_boot("runtime_boot_failed", anyhow!(error).context("create agentOS VM")).await,
            Err(CreationWaitError::DeadlineElapsed) => return self.fail_boot(
                "runtime_boot_timeout",
                anyhow!("timeout: runtime initialization exceeded {}ms while creating the VM; creation remains owned for cleanup", INITIALIZATION_TIMEOUT.as_millis()),
            ).await,
        };
        let vm = owner.vm.clone();

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

    async fn begin_creation(&self, creation: VmCreationFuture) -> Result<()> {
        let mut pending = self.pending_creation.lock().await;
        if pending.is_some() {
            return Err(anyhow!(
                "not_ready: previous hosted VM creation still requires cleanup"
            ));
        }
        *pending = Some(PendingVmCreation::Running(creation));
        Ok(())
    }

    async fn finish_pending_creation(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<Option<Arc<RuntimeVmOwner>>, CreationWaitError> {
        let mut pending = self.pending_creation.lock().await;
        if let Some(PendingVmCreation::Running(creation)) = pending.as_mut() {
            match tokio::time::timeout_at(deadline, creation.as_mut()).await {
                Ok(Ok(owner)) => *pending = Some(PendingVmCreation::Created(owner)),
                Ok(Err(error)) => {
                    pending.take();
                    return Err(CreationWaitError::Failed(error));
                }
                Err(_) => return Err(CreationWaitError::DeadlineElapsed),
            }
        }
        let Some(PendingVmCreation::Created(owner)) = pending.as_ref() else {
            return Ok(None);
        };
        let owner = owner.clone();
        // The pending slot owns the completed VM if this lock acquisition is cancelled.
        let mut state = self.state.lock().await;
        state.vm = Some(owner.clone());
        state.lifecycle = VmLifecycleState::Preloading;
        pending.take();
        Ok(Some(owner))
    }

    // Called under boot's existing operation lock, before generation allocation
    // or any preload/VM activation. The existing failure owner retains cleanup.
    async fn admit_boot_policy(&self, desired: &AgentOsActorConfig, revision: u64) -> Result<()> {
        match desired.validate_restored_budgets(&self.budgets) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.set_desired_config_revision(revision).await;
                self.fail_boot("runtime_configuration_rejected", error)
                    .await
                    .map(|_| ())
            }
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
                message: format!("{error:#}"),
                at_ms: now_ms()?,
            },
            self.budgets.retention.max_runtime_issues,
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
        self.stop_inner_at(reason, tokio::time::Instant::now() + SHUTDOWN_TIMEOUT)
            .await
    }

    async fn stop_inner_at(&self, reason: &str, deadline: tokio::time::Instant) -> Result<()> {
        match self.finish_pending_creation(deadline).await {
            Ok(_) => {}
            Err(CreationWaitError::Failed(error)) => {
                // Creation failed without publishing a VM; there is no guest to stop.
                tracing::warn!(%error, "cancelled hosted VM creation finished with an error");
            }
            Err(CreationWaitError::DeadlineElapsed) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Degraded;
                push_issue(
                    &mut state,
                    VmIssue {
                        code: "runtime_shutdown_timeout".into(),
                        message:
                            "pending VM creation remains owned; shutdown completion is unconfirmed"
                                .into(),
                        at_ms: now_ms()?,
                    },
                    self.budgets.retention.max_runtime_issues,
                );
                return Err(anyhow!(
                    "timeout: pending VM creation could not be reaped within {}ms",
                    SHUTDOWN_TIMEOUT.as_millis()
                ));
            }
        }
        let owner = {
            let mut state = self.state.lock().await;
            let Some(owner) = state.vm.clone() else {
                return Ok(());
            };
            state.lifecycle = VmLifecycleState::Stopping;
            state.resolved_software.clear();
            state.packages.required_ready = 0;
            owner
        };
        match tokio::time::timeout_at(deadline, owner.shutdown()).await {
            Ok(Ok(())) => {
                let mut state = self.state.lock().await;
                state.vm.take();
                state.lifecycle = VmLifecycleState::Initializing;
                state.applied_config_revision = None;
                state.last_shutdown_at_ms = Some(now_ms()?);
                tracing::info!(%reason, "agentOS Core runtime stopped");
                Ok(())
            }
            Ok(Err(error)) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Degraded;
                push_issue(
                    &mut state,
                    VmIssue {
                        code: "runtime_shutdown_failed".into(),
                        message: error.to_string(),
                        at_ms: now_ms()?,
                    },
                    self.budgets.retention.max_runtime_issues,
                );
                Err(anyhow!(error).context("stop agentOS Core runtime"))
            }
            Err(_) => {
                let mut state = self.state.lock().await;
                state.lifecycle = VmLifecycleState::Degraded;
                push_issue(&mut state, VmIssue {
                    code: "runtime_shutdown_timeout".into(),
                    message: format!("runtime shutdown exceeded the fixed actor deadline of {}ms; completion is unconfirmed", SHUTDOWN_TIMEOUT.as_millis()),
                    at_ms: now_ms()?,
                }, self.budgets.retention.max_runtime_issues);
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
            .as_ref()
            .map(|owner| owner.vm.clone())
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
            .as_ref()
            .map(|owner| owner.vm.clone())
            .ok_or_else(|| anyhow!("not_ready: ready VM has no Core handle"))
    }
}

fn snapshot(state: &RuntimeState) -> VmStatusSnapshot {
    let core = state.vm.as_ref().map(|vm| {
        let description = vm.vm.sidecar().describe();
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

fn push_issue(state: &mut RuntimeState, issue: VmIssue, capacity: ValidationBound) {
    let observed = state.issues.len().saturating_add(1);
    agentos_actor_contract::warn_near_budget(
        capacity.setting,
        observed,
        capacity.maximum,
        capacity.units,
    );
    while state.issues.len() >= capacity.maximum {
        state.issues.pop_front();
        tracing::warn!(
            setting = capacity.setting,
            observed,
            limit = capacity.maximum,
            units = capacity.units,
            "evicting oldest runtime issue; runtime operation is unaffected"
        );
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
    #[tokio::test]
    async fn pending_creation_deadline_keeps_its_owned_slot_for_retry() {
        let runtime = RuntimeController::new("actor", 1);
        runtime
            .begin_creation(Box::pin(std::future::pending()))
            .await
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1);
        let error = runtime
            .stop_inner_at("synthetic pending creation", deadline)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("pending VM creation could not be reaped"));
        assert!(matches!(
            *runtime.pending_creation.lock().await,
            Some(PendingVmCreation::Running(_))
        ));
        assert_eq!(runtime.status().await.lifecycle, VmLifecycleState::Degraded);
    }

    async fn native_lifecycle_sidecar() -> Result<Arc<agentos_client::AgentOsSidecar>> {
        anyhow::ensure!(
            std::env::var("AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE").as_deref() == Ok("1"),
            "set AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE=1 for the explicit native lifecycle phase"
        );
        let binary = std::env::var("AGENTOS_SIDECAR_BIN")
            .context("pin AGENTOS_SIDECAR_BIN for native hosted lifecycle tests")?;
        anyhow::ensure!(
            !binary.is_empty(),
            "AGENTOS_SIDECAR_BIN must name the pinned native artifact"
        );
        Ok(AgentOs::create_sidecar(None).await?)
    }

    async fn native_lifecycle_vm(
        sidecar: Arc<agentos_client::AgentOsSidecar>,
    ) -> Result<AgentOs, agentos_client::ClientError> {
        AgentOs::create(agentos_client::AgentOsConfig {
            sidecar: Some(agentos_client::AgentOsSidecarConfig::Explicit { handle: sidecar }),
            ..Default::default()
        })
        .await
    }

    #[tokio::test]
    #[ignore = "explicit native lifecycle phase: uses one pinned sidecar and real VM, no guest workload"]
    async fn cancelled_creation_waiter_leaves_vm_owned_until_stop_confirms_shutdown() -> Result<()>
    {
        let sidecar = native_lifecycle_sidecar().await?;
        let runtime = Arc::new(RuntimeController::new("actor", 1));
        let completed = Arc::new(tokio::sync::Notify::new());
        let completion = completed.clone();
        let source = sidecar.clone();
        let (release, released) = tokio::sync::oneshot::channel();
        runtime
            .begin_creation(Box::pin(async move {
                let owner = Arc::new(RuntimeVmOwner::new(native_lifecycle_vm(source).await?));
                completion.notify_one();
                released.await.map_err(|error| {
                    agentos_client::ClientError::Sidecar(format!(
                        "test creation gate closed: {error}"
                    ))
                })?;
                Ok(owner)
            }))
            .await?;
        let waiter = tokio::spawn({
            let runtime = runtime.clone();
            async move {
                runtime
                    .finish_pending_creation(tokio::time::Instant::now() + INITIALIZATION_TIMEOUT)
                    .await
            }
        });
        tokio::time::timeout(INITIALIZATION_TIMEOUT, completed.notified())
            .await
            .context("native VM creation did not reach gate")?;
        waiter.abort();
        let cancelled = match waiter.await {
            Err(error) => error,
            Ok(_) => return Err(anyhow!("cancelled creation waiter unexpectedly completed")),
        };
        anyhow::ensure!(
            cancelled.is_cancelled(),
            "creation waiter failed: {cancelled}"
        );
        assert_eq!(sidecar.describe().active_vm_count, 1);
        assert!(runtime.pending_creation.lock().await.is_some());
        release
            .send(())
            .map_err(|_| anyhow!("creation continuation lost its owning pending slot"))?;
        runtime.stop("cancelled boot").await?;
        assert!(runtime.pending_creation.lock().await.is_none());
        assert!(runtime.state.lock().await.vm.is_none());
        assert_eq!(sidecar.describe().active_vm_count, 0);
        sidecar.dispose().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit native lifecycle phase: uses one pinned sidecar and real VM, no guest workload"]
    async fn cancelled_shutdown_preserves_actual_state_owner_for_retry() -> Result<()> {
        let sidecar = native_lifecycle_sidecar().await?;
        let vm = native_lifecycle_vm(sidecar.clone()).await?;
        let gate = Arc::new(TestShutdownGate {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        });
        let runtime = Arc::new(RuntimeController::new("actor", 1));
        runtime.state.lock().await.vm = Some(Arc::new(RuntimeVmOwner {
            vm,
            shutdown_confirmed: AtomicBool::new(false),
            shutdown_gate: Some(gate.clone()),
        }));
        let waiter = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.stop("gated shutdown").await }
        });
        tokio::time::timeout(SHUTDOWN_TIMEOUT, gate.started.notified())
            .await
            .context("shutdown did not reach gate")?;
        waiter.abort();
        let cancelled = waiter.await.expect_err("shutdown waiter must cancel");
        anyhow::ensure!(
            cancelled.is_cancelled(),
            "shutdown waiter failed: {cancelled}"
        );
        assert!(
            runtime.state.lock().await.vm.is_some(),
            "cancelled shutdown lost its cleanup owner"
        );
        assert_eq!(sidecar.describe().active_vm_count, 1);
        gate.release.add_permits(1);
        runtime.stop("retry cancelled shutdown").await?;
        assert!(runtime.state.lock().await.vm.is_none());
        assert_eq!(sidecar.describe().active_vm_count, 0);
        sidecar.dispose().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit native lifecycle phase: uses one pinned sidecar and real VM, no guest workload"]
    async fn cancelled_created_vm_publication_keeps_pending_owner() -> Result<()> {
        let sidecar = native_lifecycle_sidecar().await?;
        let owner = Arc::new(RuntimeVmOwner::new(
            native_lifecycle_vm(sidecar.clone()).await?,
        ));
        let runtime = RuntimeController::new("actor", 1);
        runtime
            .begin_creation(Box::pin(async move { Ok(owner) }))
            .await?;
        let state_gate = runtime.state.lock().await;
        {
            let publication = runtime
                .finish_pending_creation(tokio::time::Instant::now() + INITIALIZATION_TIMEOUT);
            tokio::pin!(publication);
            assert!(futures::poll!(&mut publication).is_pending());
        }
        assert!(matches!(
            *runtime.pending_creation.lock().await,
            Some(PendingVmCreation::Created(_))
        ));
        assert_eq!(sidecar.describe().active_vm_count, 1);
        drop(state_gate);
        runtime.stop("cancelled publication").await?;
        assert!(runtime.state.lock().await.vm.is_none());
        assert_eq!(sidecar.describe().active_vm_count, 0);
        sidecar.dispose().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit native lifecycle phase: uses one pinned sidecar and real VM, no guest workload"]
    async fn dropping_unregistered_actor_controller_reaps_completed_vm() -> Result<()> {
        let sidecar = native_lifecycle_sidecar().await?;
        let runtime = RuntimeController::new("actor", 1);
        runtime.state.lock().await.vm = Some(Arc::new(RuntimeVmOwner::new(
            native_lifecycle_vm(sidecar.clone()).await?,
        )));
        assert_eq!(sidecar.describe().active_vm_count, 1);
        drop(runtime);
        tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            while sidecar.describe().active_vm_count != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("abandoned actor VM cleanup did not confirm shutdown")?;
        sidecar.dispose().await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/runtime_remaining_budgets.rs"]
mod remaining_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/restored_boot_policy.rs"]
mod restored_boot_policy_tests;
