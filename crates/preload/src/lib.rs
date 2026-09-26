use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_client::{
    validate_package_source, AgentOs, AgentOsPackageSession, AgentOsSidecar, InstalledSoftware,
    PackageResolverOptions, PackageSource, DEFAULT_MAX_PACKAGE_BYTES,
};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use rivetkit::{action, Action, Actor, Ctx, Handles, TypedClientExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OnceCell};

mod metrics;

pub const PRELOAD_COORDINATOR_ACTOR_NAME: &str = "agentOS-preload-coordinator";
pub const PRELOAD_COORDINATOR_ACTOR_KEY: &str = "global";
pub const PRELOAD_PROTOCOL_VERSION: u32 = 1;

const PACKAGE_FORMAT_VERSION: u32 = 1;
const MAX_URL_BYTES: usize = 4 * 1024;
const MAX_PROCESS_ID_BYTES: usize = 128;
const MAX_SUPPORTED_FORMATS: usize = 8;
const MAX_PLAN_ENTRIES_HARD: u32 = 512;
const MAX_PLAN_BYTES_HARD: u64 = 64 * 1024 * 1024 * 1024;
const MAX_CANDIDATES_HARD: u32 = 4_096;
const MAX_TRACKED_PROCESSES_HARD: u32 = 2_048;
const MAX_BATCH_OBSERVATIONS_HARD: u32 = 128;
const MAX_PLAN_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_OBSERVATION_WINDOW_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_OBSERVATION_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
const MAX_USAGE_COUNT_PER_OBSERVATION: u64 = 1_000_000;
const MAX_CANDIDATE_SCORE: u64 = 1_000_000_000_000;
const SCORE_HALF_LIFE_MS: i64 = 6 * 60 * 60 * 1_000;
const FLUSH_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_PROCESS_STARTUP_DEADLINE_MS: u64 = 5 * 60 * 1_000;
const MAX_PROCESS_PLAN_READ_TIMEOUT_MS: u64 = 60 * 1_000;
const MAX_PROCESS_FLUSH_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_PROCESS_ACTION_TIMEOUT_MS: u64 = 60 * 1_000;

type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadProcessOptions {
    pub startup_deadline_ms: u64,
    pub plan_read_timeout_ms: u64,
    pub max_plan_entries: usize,
    pub max_plan_bytes: u64,
    pub warm_concurrency: usize,
    pub max_observation_entries: usize,
    pub flush_interval_ms: u64,
    pub action_timeout_ms: u64,
}

impl Default for PreloadProcessOptions {
    fn default() -> Self {
        Self {
            startup_deadline_ms: 10_000,
            plan_read_timeout_ms: 1_500,
            max_plan_entries: 64,
            max_plan_bytes: 2 * 1024 * 1024 * 1024,
            warm_concurrency: 4,
            max_observation_entries: 128,
            flush_interval_ms: 5 * 60 * 1_000,
            action_timeout_ms: 2_000,
        }
    }
}

impl PreloadProcessOptions {
    pub fn validate(&self) -> Result<()> {
        if self.startup_deadline_ms == 0
            || self.plan_read_timeout_ms == 0
            || self.max_plan_entries == 0
            || self.max_plan_bytes == 0
            || self.warm_concurrency == 0
            || self.max_observation_entries == 0
            || self.flush_interval_ms == 0
            || self.action_timeout_ms == 0
        {
            bail!(
                "preload process limits must be greater than zero; raise the matching PreloadProcessOptions field"
            );
        }
        if self.plan_read_timeout_ms > self.startup_deadline_ms {
            bail!("PreloadProcessOptions.plan_read_timeout_ms cannot exceed startup_deadline_ms");
        }
        if self.max_plan_entries > MAX_PLAN_ENTRIES_HARD as usize {
            bail!(
                "PreloadProcessOptions.max_plan_entries exceeds the coordinator protocol limit of {MAX_PLAN_ENTRIES_HARD}"
            );
        }
        if self.max_plan_bytes > MAX_PLAN_BYTES_HARD {
            bail!(
                "PreloadProcessOptions.max_plan_bytes exceeds the coordinator protocol limit of {MAX_PLAN_BYTES_HARD}"
            );
        }
        if self.max_observation_entries > MAX_BATCH_OBSERVATIONS_HARD as usize {
            bail!(
                "PreloadProcessOptions.max_observation_entries exceeds the coordinator batch limit of {MAX_BATCH_OBSERVATIONS_HARD}"
            );
        }
        if self.startup_deadline_ms > MAX_PROCESS_STARTUP_DEADLINE_MS {
            bail!(
                "PreloadProcessOptions.startup_deadline_ms exceeds {MAX_PROCESS_STARTUP_DEADLINE_MS}"
            );
        }
        if self.plan_read_timeout_ms > MAX_PROCESS_PLAN_READ_TIMEOUT_MS {
            bail!(
                "PreloadProcessOptions.plan_read_timeout_ms exceeds {MAX_PROCESS_PLAN_READ_TIMEOUT_MS}"
            );
        }
        if self.warm_concurrency > MAX_PLAN_ENTRIES_HARD as usize {
            bail!("PreloadProcessOptions.warm_concurrency exceeds {MAX_PLAN_ENTRIES_HARD}");
        }
        if self.flush_interval_ms > MAX_PROCESS_FLUSH_INTERVAL_MS {
            bail!(
                "PreloadProcessOptions.flush_interval_ms exceeds {MAX_PROCESS_FLUSH_INTERVAL_MS}"
            );
        }
        if self.action_timeout_ms > MAX_PROCESS_ACTION_TIMEOUT_MS {
            bail!(
                "PreloadProcessOptions.action_timeout_ms exceeds {MAX_PROCESS_ACTION_TIMEOUT_MS}"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPreloadReport {
    pub plan_revision: Option<u64>,
    pub total: u32,
    pub ready: u32,
    pub failed: u32,
    pub skipped: u32,
    pub warmed_bytes: u64,
    pub deadline_hit: bool,
    pub coordinator_available: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadCoordinatorCreateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<PreloadCoordinatorConfigInput>,
    #[serde(default)]
    pub baseline: Vec<PreloadArtifact>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadCoordinatorConfigInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_plan_entries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_plan_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_candidates: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tracked_processes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_batch_observations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_actions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadCoordinatorConfig {
    pub max_plan_entries: u32,
    pub max_plan_bytes: u64,
    pub max_candidates: u32,
    pub max_tracked_processes: u32,
    pub max_batch_observations: u32,
    pub max_concurrent_actions: u32,
    pub plan_ttl_ms: u64,
}

impl PreloadCoordinatorConfig {
    fn normalize(input: PreloadCoordinatorConfigInput) -> Result<Self> {
        let config = Self {
            max_plan_entries: input.max_plan_entries.unwrap_or(64),
            max_plan_bytes: input.max_plan_bytes.unwrap_or(2 * 1024 * 1024 * 1024),
            max_candidates: input.max_candidates.unwrap_or(1_024),
            max_tracked_processes: input.max_tracked_processes.unwrap_or(512),
            max_batch_observations: input.max_batch_observations.unwrap_or(128),
            max_concurrent_actions: input.max_concurrent_actions.unwrap_or(64),
            plan_ttl_ms: input.plan_ttl_ms.unwrap_or(30 * 60 * 1_000),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        validate_positive_bound(
            "maxPlanEntries",
            u64::from(self.max_plan_entries),
            u64::from(MAX_PLAN_ENTRIES_HARD),
        )?;
        validate_positive_bound("maxPlanBytes", self.max_plan_bytes, MAX_PLAN_BYTES_HARD)?;
        validate_positive_bound(
            "maxCandidates",
            u64::from(self.max_candidates),
            u64::from(MAX_CANDIDATES_HARD),
        )?;
        validate_positive_bound(
            "maxTrackedProcesses",
            u64::from(self.max_tracked_processes),
            u64::from(MAX_TRACKED_PROCESSES_HARD),
        )?;
        validate_positive_bound(
            "maxBatchObservations",
            u64::from(self.max_batch_observations),
            u64::from(MAX_BATCH_OBSERVATIONS_HARD),
        )?;
        validate_positive_bound(
            "maxConcurrentActions",
            u64::from(self.max_concurrent_actions),
            512,
        )?;
        validate_positive_bound("planTtlMs", self.plan_ttl_ms, MAX_PLAN_TTL_MS)
    }
}

fn validate_positive_bound(name: &str, value: u64, maximum: u64) -> Result<()> {
    if value == 0 || value > maximum {
        bail!(
            "preload coordinator {name} must be in 1..={maximum}; change the coordinator creation config"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadArtifact {
    pub url: String,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default = "package_format_version")]
    pub package_format_version: u32,
}

fn package_format_version() -> u32 {
    PACKAGE_FORMAT_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadGetPlan {
    pub protocol_version: u32,
    pub process_id: String,
    pub supported_package_format_versions: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
}

impl Action for PreloadGetPlan {
    type Output = PreloadPlan;

    const NAME: &'static str = "preload.getPlan";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadPlan {
    pub protocol_version: u32,
    pub revision: u64,
    pub generated_at_ms: i64,
    pub expires_at_ms: i64,
    pub artifacts: Vec<PreloadArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadUsageObservation {
    pub url: String,
    pub digest: String,
    pub size: u64,
    pub count: u64,
    pub last_observed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadRecordUsage {
    pub protocol_version: u32,
    pub process_id: String,
    pub window_id: u64,
    pub window_started_at_ms: i64,
    pub window_ended_at_ms: i64,
    pub observations: Vec<PreloadUsageObservation>,
    #[serde(default)]
    pub dropped_observations: u64,
}

impl Action for PreloadRecordUsage {
    type Output = PreloadUsageAccepted;

    const NAME: &'static str = "preload.recordUsage";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadUsageAccepted {
    pub revision: u64,
    pub accepted: bool,
    pub duplicate_or_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadReplaceBaseline {
    pub protocol_version: u32,
    pub artifacts: Vec<PreloadArtifact>,
}

impl Action for PreloadReplaceBaseline {
    type Output = PreloadBaselineReplaced;

    const NAME: &'static str = "preload.replaceBaseline";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadBaselineReplaced {
    pub revision: u64,
    pub artifact_count: u32,
    pub predicted_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreloadStatus;

impl Action for PreloadStatus {
    type Output = PreloadCoordinatorStatus;

    const NAME: &'static str = "preload.status";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadCoordinatorStatus {
    pub protocol_version: u32,
    pub revision: u64,
    pub baseline_entries: u32,
    pub candidate_entries: u32,
    pub tracked_processes: u32,
    pub accepted_usage_batches: u64,
    pub duplicate_or_stale_batches: u64,
    pub reported_dropped_observations: u64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadCoordinatorState {
    config: PreloadCoordinatorConfig,
    revision: u64,
    baseline: Vec<PreloadArtifact>,
    candidates: Vec<PreloadCandidate>,
    process_windows: Vec<PreloadProcessCursor>,
    accepted_usage_batches: u64,
    duplicate_or_stale_batches: u64,
    reported_dropped_observations: u64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreloadCandidate {
    artifact: PreloadArtifact,
    score: u64,
    last_observed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreloadProcessCursor {
    process_id: String,
    window_id: u64,
    last_observed_at_ms: i64,
}

pub struct PreloadCoordinatorActor {
    mutation: Mutex<()>,
    action_admission: Arc<tokio::sync::Semaphore>,
    action_concurrency_limit: u32,
}

#[async_trait]
impl Actor for PreloadCoordinatorActor {
    type State = PreloadCoordinatorState;
    type Input = PreloadCoordinatorCreateInput;
    type Actions = (
        PreloadGetPlan,
        PreloadRecordUsage,
        PreloadReplaceBaseline,
        PreloadStatus,
    );
    type Events = ();
    type Queue = ();
    type ConnParams = ();
    type ConnState = ();
    type Action = action::Raw;

    async fn create_state(_ctx: &Ctx<Self>, input: Self::Input) -> Result<Self::State> {
        let now = now_ms()?;
        let config = PreloadCoordinatorConfig::normalize(input.config.unwrap_or_default())?;
        let baseline = normalize_artifacts(input.baseline, &config)?;
        Ok(PreloadCoordinatorState {
            config,
            revision: 1,
            baseline,
            candidates: Vec::new(),
            process_windows: Vec::new(),
            accepted_usage_batches: 0,
            duplicate_or_stale_batches: 0,
            reported_dropped_observations: 0,
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        validate_coordinator_state(&ctx.state())?;
        let action_concurrency_limit = ctx.state().config.max_concurrent_actions;
        Ok(Self {
            mutation: Mutex::new(()),
            action_admission: Arc::new(tokio::sync::Semaphore::new(
                action_concurrency_limit as usize,
            )),
            action_concurrency_limit,
        })
    }
}

fn validate_coordinator_state(state: &PreloadCoordinatorState) -> Result<()> {
    state.config.validate()?;
    if state.revision == 0 {
        bail!("preload coordinator durable revision must be greater than zero");
    }
    if state.baseline.len() > state.config.max_plan_entries as usize
        || state.candidates.len() > state.config.max_candidates as usize
        || state.process_windows.len() > state.config.max_tracked_processes as usize
    {
        bail!(
            "preload coordinator durable state exceeds its configured collection limits; repair or recreate the coordinator actor"
        );
    }
    let mut baseline_digests = BTreeSet::new();
    for artifact in &state.baseline {
        validate_artifact(artifact)?;
        if !baseline_digests.insert(artifact.digest.clone()) {
            bail!("preload coordinator durable baseline contains a duplicate digest");
        }
    }
    if predicted_artifact_bytes(&state.baseline)? > state.config.max_plan_bytes {
        bail!("preload coordinator durable baseline exceeds maxPlanBytes");
    }
    let mut candidate_digests = BTreeSet::new();
    for candidate in &state.candidates {
        validate_artifact(&candidate.artifact)?;
        if candidate.score > MAX_CANDIDATE_SCORE
            || !candidate_digests.insert(candidate.artifact.digest.clone())
        {
            bail!("preload coordinator durable candidate set is invalid");
        }
    }
    let mut process_ids = BTreeSet::new();
    for cursor in &state.process_windows {
        validate_process_id(&cursor.process_id)?;
        if cursor.window_id == 0 || !process_ids.insert(cursor.process_id.clone()) {
            bail!("preload coordinator durable process cursor set is invalid");
        }
    }
    Ok(())
}

impl PreloadCoordinatorActor {
    fn admit_action(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        let permit = self
            .action_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow!(
                    "limit_exceeded: preload coordinator action concurrency reached {}; raise config.maxConcurrentActions at coordinator creation",
                    self.action_concurrency_limit
                )
            })?;
        let observed = self
            .action_concurrency_limit
            .saturating_sub(self.action_admission.available_permits() as u32);
        if observed.saturating_mul(100) / self.action_concurrency_limit >= 80 {
            tracing::warn!(
                limit = "preload_coordinator_concurrent_actions",
                observed,
                capacity = self.action_concurrency_limit,
                configuration_path = "PreloadCoordinatorConfigInput.maxConcurrentActions",
                "preload coordinator action concurrency approaching configured limit"
            );
        }
        Ok(permit)
    }
}

impl Handles<PreloadGetPlan> for PreloadCoordinatorActor {
    type Future = BoxFuture<PreloadPlan>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: PreloadGetPlan) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_protocol(action.protocol_version)?;
            validate_process_id(&action.process_id)?;
            if action.supported_package_format_versions.len() > MAX_SUPPORTED_FORMATS {
                bail!(
                    "limit_exceeded: supportedPackageFormatVersions has {} entries; maximum is {MAX_SUPPORTED_FORMATS}",
                    action.supported_package_format_versions.len()
                );
            }
            let now = now_ms()?;
            let state = ctx.state();
            build_plan(&state, &action, now)
        })
    }
}

impl Handles<PreloadRecordUsage> for PreloadCoordinatorActor {
    type Future = BoxFuture<PreloadUsageAccepted>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: PreloadRecordUsage) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let _mutation = self.mutation.lock().await;
            let now = now_ms()?;
            let mut state = ctx.state().clone();
            let output = apply_usage(&mut state, action, now)?;
            ctx.set_state(state);
            Ok(output)
        })
    }
}

impl Handles<PreloadReplaceBaseline> for PreloadCoordinatorActor {
    type Future = BoxFuture<PreloadBaselineReplaced>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: PreloadReplaceBaseline) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let _mutation = self.mutation.lock().await;
            validate_protocol(action.protocol_version)?;
            let mut state = ctx.state().clone();
            let baseline = normalize_artifacts(action.artifacts, &state.config)?;
            let predicted_bytes = predicted_artifact_bytes(&baseline)?;
            state.baseline = baseline;
            state.revision = checked_revision(state.revision)?;
            state.updated_at_ms = now_ms()?;
            let output = PreloadBaselineReplaced {
                revision: state.revision,
                artifact_count: u32::try_from(state.baseline.len())
                    .context("baseline artifact count exceeds u32")?,
                predicted_bytes,
            };
            ctx.set_state(state);
            Ok(output)
        })
    }
}

impl Handles<PreloadStatus> for PreloadCoordinatorActor {
    type Future = BoxFuture<PreloadCoordinatorStatus>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, _action: PreloadStatus) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let state = ctx.state();
            Ok(PreloadCoordinatorStatus {
                protocol_version: PRELOAD_PROTOCOL_VERSION,
                revision: state.revision,
                baseline_entries: u32::try_from(state.baseline.len())
                    .context("baseline count exceeds u32")?,
                candidate_entries: u32::try_from(state.candidates.len())
                    .context("candidate count exceeds u32")?,
                tracked_processes: u32::try_from(state.process_windows.len())
                    .context("tracked process count exceeds u32")?,
                accepted_usage_batches: state.accepted_usage_batches,
                duplicate_or_stale_batches: state.duplicate_or_stale_batches,
                reported_dropped_observations: state.reported_dropped_observations,
                updated_at_ms: state.updated_at_ms,
            })
        })
    }
}

fn validate_protocol(version: u32) -> Result<()> {
    if version != PRELOAD_PROTOCOL_VERSION {
        bail!(
            "unsupported preload protocol version {version}; expected {PRELOAD_PROTOCOL_VERSION}"
        );
    }
    Ok(())
}

fn validate_process_id(process_id: &str) -> Result<()> {
    if process_id.is_empty() || process_id.len() > MAX_PROCESS_ID_BYTES {
        bail!(
            "processId must contain 1..={MAX_PROCESS_ID_BYTES} bytes; generate one bounded identifier per process"
        );
    }
    Ok(())
}

fn validate_artifact(artifact: &PreloadArtifact) -> Result<()> {
    if artifact.url.len() > MAX_URL_BYTES {
        bail!(
            "preload artifact URL exceeds {MAX_URL_BYTES} bytes; publish the package at a shorter stable URL"
        );
    }
    if artifact.package_format_version != PACKAGE_FORMAT_VERSION {
        bail!(
            "unsupported package format version {}; expected {PACKAGE_FORMAT_VERSION}",
            artifact.package_format_version
        );
    }
    if artifact.size == Some(0) || artifact.size.is_some_and(|size| size > MAX_PLAN_BYTES_HARD) {
        bail!("preload artifact size must be in 1..={MAX_PLAN_BYTES_HARD} when provided");
    }
    validate_package_source(&PackageSource::Url {
        url: artifact.url.clone(),
        expected_digest: Some(artifact.digest.clone()),
    })?;
    Ok(())
}

fn normalize_artifacts(
    artifacts: Vec<PreloadArtifact>,
    config: &PreloadCoordinatorConfig,
) -> Result<Vec<PreloadArtifact>> {
    if artifacts.len() > config.max_plan_entries as usize {
        bail!(
            "limit_exceeded: baseline has {} entries; maximum is {}; raise config.maxPlanEntries at coordinator creation",
            artifacts.len(),
            config.max_plan_entries
        );
    }
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        validate_artifact(&artifact)?;
        if seen.insert(artifact.digest.clone()) {
            normalized.push(artifact);
        }
    }
    let bytes = predicted_artifact_bytes(&normalized)?;
    if bytes > config.max_plan_bytes {
        bail!(
            "limit_exceeded: baseline predicts {bytes} bytes; maximum is {}; raise config.maxPlanBytes at coordinator creation",
            config.max_plan_bytes
        );
    }
    if normalized.len() * 100 / config.max_plan_entries as usize >= 80
        || bytes.saturating_mul(100) / config.max_plan_bytes >= 80
    {
        tracing::warn!(
            limit = "preload_coordinator_baseline",
            observed_entries = normalized.len(),
            capacity_entries = config.max_plan_entries,
            observed_bytes = bytes,
            capacity_bytes = config.max_plan_bytes,
            configuration_path = "PreloadCoordinatorConfigInput",
            "preload coordinator baseline approaching configured plan limits"
        );
    }
    Ok(normalized)
}

fn predicted_size(artifact: &PreloadArtifact) -> u64 {
    artifact.size.unwrap_or(DEFAULT_MAX_PACKAGE_BYTES)
}

fn predicted_artifact_bytes(artifacts: &[PreloadArtifact]) -> Result<u64> {
    artifacts.iter().try_fold(0u64, |total, artifact| {
        total
            .checked_add(predicted_size(artifact))
            .ok_or_else(|| anyhow!("preload predicted byte total overflow"))
    })
}

fn build_plan(
    state: &PreloadCoordinatorState,
    action: &PreloadGetPlan,
    now: i64,
) -> Result<PreloadPlan> {
    let max_entries = action
        .max_entries
        .unwrap_or(state.config.max_plan_entries)
        .min(state.config.max_plan_entries);
    let max_bytes = action
        .max_bytes
        .unwrap_or(state.config.max_plan_bytes)
        .min(state.config.max_plan_bytes);
    if max_entries == 0 || max_bytes == 0 {
        bail!("preload plan maxEntries and maxBytes must be greater than zero");
    }

    let supports_package_format = action
        .supported_package_format_versions
        .contains(&PACKAGE_FORMAT_VERSION);
    let mut artifacts = Vec::with_capacity(max_entries as usize);
    let mut seen = BTreeSet::new();
    let mut bytes = 0u64;
    if supports_package_format {
        for artifact in &state.baseline {
            append_plan_artifact(
                &mut artifacts,
                &mut seen,
                &mut bytes,
                artifact,
                max_entries,
                max_bytes,
            )?;
        }

        let mut candidates = state.candidates.clone();
        candidates.sort_by(|left, right| {
            decayed_score(right.score, right.last_observed_at_ms, now)
                .cmp(&decayed_score(left.score, left.last_observed_at_ms, now))
                .then_with(|| right.last_observed_at_ms.cmp(&left.last_observed_at_ms))
                .then_with(|| left.artifact.digest.cmp(&right.artifact.digest))
        });
        for candidate in candidates {
            append_plan_artifact(
                &mut artifacts,
                &mut seen,
                &mut bytes,
                &candidate.artifact,
                max_entries,
                max_bytes,
            )?;
        }
    }

    Ok(PreloadPlan {
        protocol_version: PRELOAD_PROTOCOL_VERSION,
        revision: state.revision,
        generated_at_ms: now,
        expires_at_ms: now
            .checked_add(i64::try_from(state.config.plan_ttl_ms).context("plan TTL exceeds i64")?)
            .ok_or_else(|| anyhow!("preload plan expiry overflow"))?,
        artifacts,
    })
}

fn append_plan_artifact(
    output: &mut Vec<PreloadArtifact>,
    seen: &mut BTreeSet<String>,
    bytes: &mut u64,
    artifact: &PreloadArtifact,
    max_entries: u32,
    max_bytes: u64,
) -> Result<()> {
    if output.len() >= max_entries as usize || seen.contains(&artifact.digest) {
        return Ok(());
    }
    let candidate_bytes = bytes
        .checked_add(predicted_size(artifact))
        .ok_or_else(|| anyhow!("preload plan byte total overflow"))?;
    if candidate_bytes > max_bytes {
        return Ok(());
    }
    *bytes = candidate_bytes;
    seen.insert(artifact.digest.clone());
    output.push(artifact.clone());
    Ok(())
}

fn apply_usage(
    state: &mut PreloadCoordinatorState,
    action: PreloadRecordUsage,
    now: i64,
) -> Result<PreloadUsageAccepted> {
    validate_protocol(action.protocol_version)?;
    validate_process_id(&action.process_id)?;
    if action.observations.len() > state.config.max_batch_observations as usize {
        bail!(
            "limit_exceeded: usage batch has {} observations; maximum is {}; raise config.maxBatchObservations at coordinator creation",
            action.observations.len(),
            state.config.max_batch_observations
        );
    }
    if action.window_id == 0 {
        bail!("preload usage windowId must be greater than zero");
    }
    if action.window_started_at_ms > action.window_ended_at_ms
        || action
            .window_ended_at_ms
            .saturating_sub(action.window_started_at_ms)
            > MAX_OBSERVATION_WINDOW_MS
        || action.window_ended_at_ms > now.saturating_add(MAX_OBSERVATION_CLOCK_SKEW_MS)
    {
        bail!(
            "preload usage observation window is invalid or exceeds {MAX_OBSERVATION_WINDOW_MS}ms"
        );
    }

    let mut normalized = BTreeMap::<String, PreloadUsageObservation>::new();
    for observation in action.observations {
        if observation.count == 0 || observation.count > MAX_USAGE_COUNT_PER_OBSERVATION {
            bail!(
                "preload usage count must be in 1..={MAX_USAGE_COUNT_PER_OBSERVATION}; reduce the process flush interval"
            );
        }
        if observation.last_observed_at_ms < action.window_started_at_ms
            || observation.last_observed_at_ms > action.window_ended_at_ms
        {
            bail!("preload observation timestamp must fall inside its window");
        }
        let artifact = PreloadArtifact {
            url: observation.url.clone(),
            digest: observation.digest.clone(),
            size: Some(observation.size),
            package_format_version: PACKAGE_FORMAT_VERSION,
        };
        validate_artifact(&artifact)?;
        match normalized.get_mut(&observation.digest) {
            Some(existing) => {
                existing.count = existing
                    .count
                    .checked_add(observation.count)
                    .ok_or_else(|| anyhow!("preload usage count overflow"))?;
                if existing.count > MAX_USAGE_COUNT_PER_OBSERVATION {
                    bail!(
                        "coalesced preload usage count exceeds {MAX_USAGE_COUNT_PER_OBSERVATION}"
                    );
                }
                if observation.last_observed_at_ms >= existing.last_observed_at_ms {
                    existing.url = observation.url;
                    existing.size = observation.size;
                    existing.last_observed_at_ms = observation.last_observed_at_ms;
                }
            }
            None => {
                normalized.insert(observation.digest.clone(), observation);
            }
        }
    }

    if let Some(cursor) = state
        .process_windows
        .iter_mut()
        .find(|cursor| cursor.process_id == action.process_id)
    {
        if action.window_id <= cursor.window_id {
            state.duplicate_or_stale_batches = state.duplicate_or_stale_batches.saturating_add(1);
            return Ok(PreloadUsageAccepted {
                revision: state.revision,
                accepted: false,
                duplicate_or_stale: true,
            });
        }
        cursor.window_id = action.window_id;
        cursor.last_observed_at_ms = action.window_ended_at_ms;
    } else {
        if state.process_windows.len() >= state.config.max_tracked_processes as usize {
            state.process_windows.sort_by(|left, right| {
                left.last_observed_at_ms
                    .cmp(&right.last_observed_at_ms)
                    .then_with(|| left.process_id.cmp(&right.process_id))
            });
            state.process_windows.remove(0);
        }
        state.process_windows.push(PreloadProcessCursor {
            process_id: action.process_id,
            window_id: action.window_id,
            last_observed_at_ms: action.window_ended_at_ms,
        });
    }
    if state.process_windows.len() * 100 / state.config.max_tracked_processes as usize >= 80 {
        tracing::warn!(
            limit = "preload_coordinator_tracked_processes",
            observed = state.process_windows.len(),
            capacity = state.config.max_tracked_processes,
            configuration_path = "PreloadCoordinatorConfigInput.maxTrackedProcesses",
            "preload coordinator process cursor set approaching configured limit"
        );
    }

    for observation in normalized.into_values() {
        let artifact = PreloadArtifact {
            url: observation.url,
            digest: observation.digest,
            size: Some(observation.size),
            package_format_version: PACKAGE_FORMAT_VERSION,
        };
        if let Some(candidate) = state
            .candidates
            .iter_mut()
            .find(|candidate| candidate.artifact.digest == artifact.digest)
        {
            let score_time = candidate
                .last_observed_at_ms
                .max(observation.last_observed_at_ms);
            candidate.score =
                decayed_score(candidate.score, candidate.last_observed_at_ms, score_time)
                    .saturating_add(observation.count)
                    .min(MAX_CANDIDATE_SCORE);
            if observation.last_observed_at_ms >= candidate.last_observed_at_ms {
                candidate.last_observed_at_ms = observation.last_observed_at_ms;
                candidate.artifact = artifact;
            }
        } else {
            state.candidates.push(PreloadCandidate {
                artifact,
                score: observation.count.min(MAX_CANDIDATE_SCORE),
                last_observed_at_ms: observation.last_observed_at_ms,
            });
        }
    }
    prune_candidates(state, now);
    state.accepted_usage_batches = state.accepted_usage_batches.saturating_add(1);
    state.reported_dropped_observations = state
        .reported_dropped_observations
        .saturating_add(action.dropped_observations);
    state.revision = checked_revision(state.revision)?;
    state.updated_at_ms = now;
    Ok(PreloadUsageAccepted {
        revision: state.revision,
        accepted: true,
        duplicate_or_stale: false,
    })
}

fn decayed_score(score: u64, last_observed_at_ms: i64, now: i64) -> u64 {
    let elapsed = now.saturating_sub(last_observed_at_ms).max(0);
    let half_lives = (elapsed / SCORE_HALF_LIFE_MS).min(63) as u32;
    score >> half_lives
}

fn prune_candidates(state: &mut PreloadCoordinatorState, now: i64) {
    state.candidates.sort_by(|left, right| {
        decayed_score(right.score, right.last_observed_at_ms, now)
            .cmp(&decayed_score(left.score, left.last_observed_at_ms, now))
            .then_with(|| right.last_observed_at_ms.cmp(&left.last_observed_at_ms))
            .then_with(|| left.artifact.digest.cmp(&right.artifact.digest))
    });
    state
        .candidates
        .truncate(state.config.max_candidates as usize);
    if state.candidates.len() * 100 / state.config.max_candidates as usize >= 80 {
        tracing::warn!(
            limit = "preload_coordinator_candidates",
            observed = state.candidates.len(),
            capacity = state.config.max_candidates,
            configuration_path = "PreloadCoordinatorConfigInput.maxCandidates",
            "preload coordinator candidate set approaching configured limit"
        );
    }
}

fn checked_revision(revision: u64) -> Result<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| anyhow!("preload coordinator revision overflow"))
}

#[derive(Debug, Clone)]
struct BufferedObservation {
    url: String,
    digest: String,
    size: u64,
    count: u64,
    last_observed_at_ms: i64,
}

struct ObservationBuffer {
    started_at_ms: i64,
    entries: BTreeMap<String, BufferedObservation>,
    dropped: u64,
}

struct ProcessPreloadService {
    options: PreloadProcessOptions,
    process_id: String,
    warm: OnceCell<ProcessPreloadReport>,
    // Retain the worker-owned handle before opening a package session. Session
    // initialization is cancellable, so the session OnceCell alone cannot own
    // child cleanup during worker shutdown.
    sidecar: OnceLock<Arc<AgentOsSidecar>>,
    // Serialize first handle publication with terminal worker shutdown. No
    // initializer may publish a new child owner after shutdown returned.
    shutting_down: Mutex<bool>,
    package_session: OnceCell<AgentOsPackageSession>,
    client: OnceLock<rivetkit::client::Client>,
    flush_task_started: AtomicBool,
    observations: Mutex<ObservationBuffer>,
    next_window_id: AtomicU64,
}

#[derive(Debug, PartialEq, Eq)]
struct OptionalPreloadOutcome {
    ready: u32,
    failed: u32,
    skipped: u32,
    warmed_bytes: u64,
    deadline_hit: bool,
    pending_digests: BTreeSet<String>,
}

/// Dropping the stream on deadline stops waiting for optional acquisitions.
/// The sidecar bounds outstanding downloads independently; an already-sent
/// request may finish after this worker has moved on.
async fn collect_optional_preloads<S, E>(
    futures: S,
    mut pending_digests: BTreeSet<String>,
    deadline: tokio::time::Instant,
) -> OptionalPreloadOutcome
where
    S: Stream<Item = (String, std::result::Result<u64, E>)>,
    E: std::fmt::Debug,
{
    let total = u32::try_from(pending_digests.len()).unwrap_or(u32::MAX);
    let mut ready = 0u32;
    let mut failed = 0u32;
    let mut warmed_bytes = 0u64;
    let mut deadline_hit = false;
    tokio::pin!(futures);
    loop {
        if pending_digests.is_empty() {
            break;
        }
        // timeout_at polls its inner future before checking the timer. Do not
        // start another acquisition after a deadline that has already elapsed.
        if tokio::time::Instant::now() >= deadline {
            deadline_hit = true;
            break;
        }
        let next = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                deadline_hit = true;
                break;
            }
            next = futures.next() => next,
        };
        match next {
            Some((digest, Ok(size))) => {
                pending_digests.remove(&digest);
                ready = ready.saturating_add(1);
                warmed_bytes = warmed_bytes.saturating_add(size);
            }
            Some((digest, Err(error))) => {
                pending_digests.remove(&digest);
                failed = failed.saturating_add(1);
                tracing::warn!(?error, %digest, "optional package preload acquisition failed");
            }
            None => break,
        }
    }
    OptionalPreloadOutcome {
        ready,
        failed,
        skipped: total.saturating_sub(ready.saturating_add(failed)),
        warmed_bytes,
        deadline_hit,
        pending_digests,
    }
}

static PROCESS_PRELOAD: OnceLock<Arc<ProcessPreloadService>> = OnceLock::new();

pub fn configure_process_preload(options: PreloadProcessOptions) -> Result<()> {
    options.validate()?;
    if let Some(service) = PROCESS_PRELOAD.get() {
        if service.options == options {
            return Ok(());
        }
        bail!(
            "process preload service is already configured differently; configure PreloadProcessOptions before starting the actor registry"
        );
    }
    let service = Arc::new(ProcessPreloadService::new(options.clone())?);
    if PROCESS_PRELOAD.set(service).is_err() {
        return configure_process_preload(options);
    }
    Ok(())
}

fn process_preload_service() -> Result<Arc<ProcessPreloadService>> {
    if PROCESS_PRELOAD.get().is_none() {
        configure_process_preload(PreloadProcessOptions::default())?;
    }
    PROCESS_PRELOAD
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("process preload service initialization did not publish a service"))
}

impl ProcessPreloadService {
    fn new(options: PreloadProcessOptions) -> Result<Self> {
        options.validate()?;
        Ok(Self {
            options,
            process_id: uuid::Uuid::new_v4().to_string(),
            warm: OnceCell::new(),
            sidecar: OnceLock::new(),
            shutting_down: Mutex::new(false),
            package_session: OnceCell::new(),
            client: OnceLock::new(),
            flush_task_started: AtomicBool::new(false),
            observations: Mutex::new(ObservationBuffer {
                started_at_ms: now_ms()?,
                entries: BTreeMap::new(),
                dropped: 0,
            }),
            next_window_id: AtomicU64::new(1),
        })
    }

    async fn sidecar(&self) -> Result<&Arc<AgentOsSidecar>> {
        let shutting_down = self.shutting_down.lock().await;
        if *shutting_down {
            bail!("process preload service is shutting down");
        }
        if let Some(sidecar) = self.sidecar.get() {
            return Ok(sidecar);
        }
        let binary_path = std::env::current_exe()
            .context("resolve agentOS executable for preload sidecar")?
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow!("agentOS executable path is not valid UTF-8"))?;
        let sidecar = AgentOs::get_shared_sidecar(None, Some(binary_path)).await?;
        // Publish before any operation that can spawn or await the child,
        // while shutdown is excluded by the lifecycle lock.
        self.sidecar
            .set(sidecar)
            .map_err(|_| anyhow!("process preload sidecar was already initialized"))?;
        self.sidecar
            .get()
            .ok_or_else(|| anyhow!("process preload sidecar was not retained"))
    }

    async fn package_session(&self) -> Result<&AgentOsPackageSession> {
        self.package_session
            .get_or_try_init(|| async {
                Ok::<_, anyhow::Error>(self.sidecar().await?.open_package_session().await?)
            })
            .await
    }

    async fn shutdown(&self) -> Result<()> {
        let mut shutting_down = self.shutting_down.lock().await;
        *shutting_down = true;
        if let Some(sidecar) = self.sidecar.get() {
            // Keep the owner reachable on cancellation or failure so a later
            // shutdown retries reaping. Incomplete session initialization does
            // not affect ownership, and the terminal flag prevents reopening.
            sidecar.dispose().await?;
        }
        Ok(())
    }

    fn attach_client(self: &Arc<Self>, client: rivetkit::client::Client) {
        if self.client.set(client).is_err() {
            tracing::debug!("process preload service already retained its first RivetKit client");
        }
        if self
            .flush_task_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let service = Arc::clone(self);
            tokio::spawn(async move { service.flush_loop().await });
        }
    }

    async fn warm_once(
        self: &Arc<Self>,
        client: Result<rivetkit::client::Client>,
    ) -> ProcessPreloadReport {
        self.warm
            .get_or_init(|| async {
                let report = match client {
                    Ok(client) => {
                        self.attach_client(client.clone());
                        match self.run_warm(client).await {
                            Ok(report) => report,
                            Err(error) => {
                                tracing::warn!(
                                    ?error,
                                    "optional process package preload failed; actor startup will continue"
                                );
                                ProcessPreloadReport::default()
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            ?error,
                            "actor client unavailable for optional process preload"
                        );
                        ProcessPreloadReport::default()
                    }
                };
                if let Err(error) = metrics::record_preload_report(&report) {
                    tracing::error!(?error, "record process preload metrics");
                }
                record_process_cache_metrics().await;
                report
            })
            .await
            .clone()
    }

    async fn run_warm(&self, client: rivetkit::client::Client) -> Result<ProcessPreloadReport> {
        let started = Instant::now();
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.options.startup_deadline_ms);
        let coordinator = client.get_or_create_typed_default::<PreloadCoordinatorActor>(
            PRELOAD_COORDINATOR_ACTOR_NAME,
            [PRELOAD_COORDINATOR_ACTOR_KEY],
        )?;
        let plan = match tokio::time::timeout(
            Duration::from_millis(self.options.plan_read_timeout_ms),
            coordinator.call(PreloadGetPlan {
                protocol_version: PRELOAD_PROTOCOL_VERSION,
                process_id: self.process_id.clone(),
                supported_package_format_versions: vec![PACKAGE_FORMAT_VERSION],
                max_entries: Some(
                    u32::try_from(self.options.max_plan_entries)
                        .context("preload process max entries exceeds u32")?,
                ),
                max_bytes: Some(self.options.max_plan_bytes),
            }),
        )
        .await
        {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => {
                tracing::warn!(?error, "preload coordinator plan read failed");
                return Ok(ProcessPreloadReport::default());
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = self.options.plan_read_timeout_ms,
                    configuration_path = "PreloadProcessOptions.plan_read_timeout_ms",
                    "preload coordinator plan read timed out"
                );
                return Ok(ProcessPreloadReport::default());
            }
        };
        let artifacts = self.validate_plan(plan.clone())?;
        let total = u32::try_from(artifacts.len()).context("preload plan length exceeds u32")?;
        let pending_digests: BTreeSet<_> = artifacts
            .iter()
            .map(|artifact| artifact.digest.clone())
            .collect();
        let session = match tokio::time::timeout_at(deadline, self.package_session()).await {
            Ok(Ok(session)) => session,
            Ok(Err(error)) => {
                tracing::warn!(?error, "optional process package session could not start");
                return Ok(ProcessPreloadReport {
                    plan_revision: Some(plan.revision),
                    total,
                    failed: total,
                    coordinator_available: true,
                    ..ProcessPreloadReport::default()
                });
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = self.options.startup_deadline_ms,
                    configuration_path = "PreloadProcessOptions.startup_deadline_ms",
                    pending_digests = ?pending_digests,
                    "optional package preload deadline expired while opening its sidecar session"
                );
                return Ok(ProcessPreloadReport {
                    plan_revision: Some(plan.revision),
                    total,
                    skipped: total,
                    deadline_hit: true,
                    coordinator_available: true,
                    ..ProcessPreloadReport::default()
                });
            }
        };
        let mut options = hosted_package_options();
        options.download_timeout_ms = self.options.startup_deadline_ms;
        options.connect_timeout_ms = self
            .options
            .plan_read_timeout_ms
            .min(self.options.startup_deadline_ms);
        let futures = stream::iter(artifacts.into_iter().map(|artifact| {
            let options = options.clone();
            async move {
                let digest = artifact.digest.clone();
                let remaining_ms = deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                let result = session
                    .acquire(
                        PackageSource::Url {
                            url: artifact.url,
                            expected_digest: Some(artifact.digest),
                        },
                        true,
                        Some(remaining_ms.max(1)),
                        Some(&options),
                    )
                    .await
                    .map(|package| package.size_bytes);
                (digest, result)
            }
        }))
        .buffer_unordered(self.options.warm_concurrency);
        let outcome = collect_optional_preloads(futures, pending_digests, deadline).await;
        if outcome.deadline_hit {
            tracing::warn!(
                timeout_ms = self.options.startup_deadline_ms,
                configuration_path = "PreloadProcessOptions.startup_deadline_ms",
                completed = outcome.ready.saturating_add(outcome.failed),
                total,
                pending_digests = ?outcome.pending_digests,
                "optional process package preload reached its startup deadline"
            );
        }
        tracing::info!(
            plan_revision = plan.revision,
            total,
            ready = outcome.ready,
            failed = outcome.failed,
            skipped = outcome.skipped,
            warmed_bytes = outcome.warmed_bytes,
            elapsed_ms = started.elapsed().as_millis(),
            "process package preload completed"
        );
        Ok(ProcessPreloadReport {
            plan_revision: Some(plan.revision),
            total,
            ready: outcome.ready,
            failed: outcome.failed,
            skipped: outcome.skipped,
            warmed_bytes: outcome.warmed_bytes,
            deadline_hit: outcome.deadline_hit,
            coordinator_available: true,
        })
    }

    fn validate_plan(&self, plan: PreloadPlan) -> Result<Vec<PreloadArtifact>> {
        validate_protocol(plan.protocol_version)?;
        let now = now_ms()?;
        if plan.revision == 0
            || plan.generated_at_ms > plan.expires_at_ms
            || plan.expires_at_ms <= now
            || plan.expires_at_ms.saturating_sub(plan.generated_at_ms) > MAX_PLAN_TTL_MS as i64
        {
            bail!("preload coordinator returned a stale or malformed plan");
        }
        if plan.artifacts.len() > self.options.max_plan_entries {
            bail!(
                "preload coordinator returned {} entries; process limit is {}; raise PreloadProcessOptions.max_plan_entries",
                plan.artifacts.len(),
                self.options.max_plan_entries
            );
        }
        let mut seen = BTreeSet::new();
        let mut bytes = 0u64;
        for artifact in &plan.artifacts {
            validate_artifact(artifact)?;
            if !seen.insert(artifact.digest.clone()) {
                bail!("preload coordinator returned a duplicate artifact digest");
            }
            bytes = bytes
                .checked_add(predicted_size(artifact))
                .ok_or_else(|| anyhow!("preload plan byte total overflow"))?;
            if bytes > self.options.max_plan_bytes {
                bail!(
                    "preload coordinator plan predicts {bytes} bytes; process limit is {}; raise PreloadProcessOptions.max_plan_bytes",
                    self.options.max_plan_bytes
                );
            }
        }
        Ok(plan.artifacts)
    }

    async fn observe(&self, url: &str, software: &InstalledSoftware) {
        let now = match now_ms() {
            Ok(now) => now,
            Err(error) => {
                tracing::error!(?error, "failed to timestamp package usage observation");
                return;
            }
        };
        let mut buffer = self.observations.lock().await;
        if let Some(existing) = buffer.entries.get_mut(&software.digest) {
            existing.count = existing
                .count
                .saturating_add(1)
                .min(MAX_USAGE_COUNT_PER_OBSERVATION);
            existing.last_observed_at_ms = now;
            existing.url = url.to_owned();
            existing.size = software.size_bytes;
            return;
        }
        if buffer.entries.len() >= self.options.max_observation_entries {
            buffer.dropped = buffer.dropped.saturating_add(1);
            tracing::warn!(
                limit = "process_preload_observation_entries",
                observed = buffer.entries.len(),
                capacity = self.options.max_observation_entries,
                configuration_path = "PreloadProcessOptions.max_observation_entries",
                "process package usage observation buffer is full"
            );
            return;
        }
        buffer.entries.insert(
            software.digest.clone(),
            BufferedObservation {
                url: url.to_owned(),
                digest: software.digest.clone(),
                size: software.size_bytes,
                count: 1,
                last_observed_at_ms: now,
            },
        );
        if buffer.entries.len() * 100 / self.options.max_observation_entries >= 80 {
            tracing::warn!(
                limit = "process_preload_observation_entries",
                observed = buffer.entries.len(),
                capacity = self.options.max_observation_entries,
                configuration_path = "PreloadProcessOptions.max_observation_entries",
                "process package usage observation buffer approaching configured limit"
            );
        }
    }

    async fn flush_loop(self: Arc<Self>) {
        let jitter_bound = (self.options.flush_interval_ms / 10).max(1);
        let jitter_ms = u64::from(std::process::id()) % jitter_bound;
        let interval =
            Duration::from_millis(self.options.flush_interval_ms.saturating_add(jitter_ms));
        loop {
            tokio::time::sleep(interval).await;
            record_process_cache_metrics().await;
            if let Err(error) = self.flush_once().await {
                tracing::warn!(?error, "dropping process package usage observation window");
            }
        }
    }

    async fn flush_once(&self) -> Result<()> {
        let Some(client) = self.client.get().cloned() else {
            return Ok(());
        };
        let ended_at_ms = now_ms()?;
        let (started_at_ms, entries, dropped) = {
            let mut buffer = self.observations.lock().await;
            if buffer.entries.is_empty() && buffer.dropped == 0 {
                buffer.started_at_ms = ended_at_ms;
                return Ok(());
            }
            (
                std::mem::replace(&mut buffer.started_at_ms, ended_at_ms),
                std::mem::take(&mut buffer.entries),
                std::mem::take(&mut buffer.dropped),
            )
        };
        let window_id = self
            .next_window_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow!("process preload observation window id exhausted"))?;
        let action = PreloadRecordUsage {
            protocol_version: PRELOAD_PROTOCOL_VERSION,
            process_id: self.process_id.clone(),
            window_id,
            window_started_at_ms: started_at_ms,
            window_ended_at_ms: ended_at_ms,
            observations: entries
                .into_values()
                .map(|entry| PreloadUsageObservation {
                    url: entry.url,
                    digest: entry.digest,
                    size: entry.size,
                    count: entry.count,
                    last_observed_at_ms: entry.last_observed_at_ms,
                })
                .collect(),
            dropped_observations: dropped,
        };
        let observation_count = action.observations.len();
        let coalesced_uses = action.observations.iter().fold(0u64, |total, observation| {
            total.saturating_add(observation.count)
        });

        let coordinator = client.get_or_create_typed_default::<PreloadCoordinatorActor>(
            PRELOAD_COORDINATOR_ACTOR_NAME,
            [PRELOAD_COORDINATOR_ACTOR_KEY],
        )?;
        let mut last_error = None;
        for attempt in 0..=1 {
            match tokio::time::timeout(
                Duration::from_millis(self.options.action_timeout_ms),
                coordinator.call(action.clone()),
            )
            .await
            {
                Ok(Ok(output)) => {
                    tracing::info!(
                        window_id,
                        observation_count,
                        coalesced_uses,
                        dropped_observations = dropped,
                        coordinator_revision = output.revision,
                        accepted = output.accepted,
                        duplicate_or_stale = output.duplicate_or_stale,
                        "flushed process package usage observations"
                    );
                    return Ok(());
                }
                Ok(Err(error)) => last_error = Some(anyhow!(error)),
                Err(_) => {
                    last_error = Some(anyhow!(
                        "preload usage action exceeded {}ms; raise PreloadProcessOptions.action_timeout_ms",
                        self.options.action_timeout_ms
                    ));
                }
            }
            if attempt == 0 {
                tokio::time::sleep(FLUSH_RETRY_DELAY).await;
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("preload usage flush failed without an error")))
    }
}

async fn record_process_cache_metrics() {
    let result = async {
        let service = process_preload_service()?;
        let Some(session) = service.package_session.get() else {
            return Ok(());
        };
        let stats = session.cache_stats().await?;
        metrics::record_process_cache_stats(stats).await
    }
    .await;
    if let Err(error) = result {
        tracing::error!(?error, "sample process package cache metrics");
    }
}

pub async fn acquire_required_package(
    source: PackageSource,
    timeout_ms: Option<u64>,
) -> Result<InstalledSoftware> {
    let service = process_preload_service()?;
    let options = hosted_package_options();
    Ok(service
        .package_session()
        .await?
        .acquire(source, false, timeout_ms, Some(&options))
        .await?)
}

pub async fn shutdown_process_preload() -> Result<()> {
    if let Some(service) = PROCESS_PRELOAD.get() {
        service.shutdown().await?;
    }
    Ok(())
}

pub async fn warm_process_once(client: Result<rivetkit::client::Client>) -> ProcessPreloadReport {
    let service = match process_preload_service() {
        Ok(service) => service,
        Err(error) => {
            tracing::error!(?error, "initialize process preload service");
            return ProcessPreloadReport::default();
        }
    };
    service.warm_once(client).await
}

pub async fn observe_software_usage(url: &str, software: &InstalledSoftware) {
    match process_preload_service() {
        Ok(service) => service.observe(url, software).await,
        Err(error) => tracing::error!(?error, "record process package usage"),
    }
}

/// Package resolver policy shared by required installs and advisory warming in
/// hosted actor processes.
pub fn hosted_package_options() -> PackageResolverOptions {
    let mut options = PackageResolverOptions::default();
    if std::env::var("AGENTOS_ALLOW_INSECURE_LOCAL_PACKAGE_HTTP").as_deref() == Ok("1") {
        options.allow_insecure_local_http = true;
    }
    options
}

fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_millis();
    i64::try_from(millis).context("system clock exceeds signed millisecond range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivetkit::ActionSet;

    #[tokio::test]
    async fn shutdown_disposes_owner_without_an_initialized_package_session() {
        let service = ProcessPreloadService::new(PreloadProcessOptions::default()).unwrap();
        let sidecar = AgentOs::create_sidecar(None).await.unwrap();
        assert!(service.sidecar.set(sidecar.clone()).is_ok());
        assert!(service.package_session.get().is_none());

        service.shutdown().await.unwrap();
        assert_eq!(
            sidecar.describe().state,
            agentos_client::SidecarState::Disposed
        );
        assert!(service.package_session().await.is_err());
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_excludes_a_racing_first_sidecar_initializer() {
        let service = ProcessPreloadService::new(PreloadProcessOptions::default()).unwrap();
        let gate = service.shutting_down.lock().await;
        let shutdown = service.shutdown();
        tokio::pin!(shutdown);
        assert!(futures::poll!(&mut shutdown).is_pending());
        let opening = service.sidecar();
        tokio::pin!(opening);
        assert!(futures::poll!(&mut opening).is_pending());
        // Tokio's FIFO mutex admits shutdown first. The waiting initializer
        // must then fail without publishing a handle or starting any child.
        drop(gate);
        shutdown.await.unwrap();
        assert!(opening.await.is_err());
        assert!(service.sidecar.get().is_none());
        assert!(service.package_session.get().is_none());
    }

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn artifact(byte: char, size: u64) -> PreloadArtifact {
        PreloadArtifact {
            url: format!("https://packages.example.test/{byte}.aospkg"),
            digest: digest(byte),
            size: Some(size),
            package_format_version: PACKAGE_FORMAT_VERSION,
        }
    }

    fn state(now: i64) -> PreloadCoordinatorState {
        PreloadCoordinatorState {
            config: PreloadCoordinatorConfig::normalize(PreloadCoordinatorConfigInput {
                max_plan_entries: Some(3),
                max_plan_bytes: Some(30),
                max_candidates: Some(3),
                max_tracked_processes: Some(2),
                max_batch_observations: Some(3),
                max_concurrent_actions: Some(4),
                plan_ttl_ms: Some(1_000),
            })
            .unwrap(),
            revision: 1,
            baseline: vec![artifact('a', 10)],
            candidates: Vec::new(),
            process_windows: Vec::new(),
            accepted_usage_batches: 0,
            duplicate_or_stale_batches: 0,
            reported_dropped_observations: 0,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    fn usage(
        process: &str,
        window_id: u64,
        values: &[(char, u64)],
        now: i64,
    ) -> PreloadRecordUsage {
        PreloadRecordUsage {
            protocol_version: PRELOAD_PROTOCOL_VERSION,
            process_id: process.to_owned(),
            window_id,
            window_started_at_ms: now - 100,
            window_ended_at_ms: now,
            observations: values
                .iter()
                .map(|(byte, count)| PreloadUsageObservation {
                    url: artifact(*byte, 10).url,
                    digest: digest(*byte),
                    size: 10,
                    count: *count,
                    last_observed_at_ms: now,
                })
                .collect(),
            dropped_observations: 0,
        }
    }

    #[test]
    fn coordinator_actions_are_internal_and_dotted() {
        assert_eq!(
            <<PreloadCoordinatorActor as Actor>::Actions as ActionSet<PreloadCoordinatorActor>>::entries()
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>(),
            [
                "preload.getPlan",
                "preload.recordUsage",
                "preload.replaceBaseline",
                "preload.status",
            ]
        );
    }

    #[test]
    fn baseline_is_reserved_and_candidates_are_ranked_deterministically() {
        let now = 10_000_000;
        let mut state = state(now);
        apply_usage(
            &mut state,
            usage("process-1", 1, &[('b', 2), ('c', 7), ('d', 4)], now),
            now,
        )
        .unwrap();
        let plan = build_plan(
            &state,
            &PreloadGetPlan {
                protocol_version: PRELOAD_PROTOCOL_VERSION,
                process_id: String::from("reader"),
                supported_package_format_versions: vec![PACKAGE_FORMAT_VERSION],
                max_entries: None,
                max_bytes: None,
            },
            now,
        )
        .unwrap();
        assert_eq!(
            plan.artifacts
                .into_iter()
                .map(|entry| entry.digest)
                .collect::<Vec<_>>(),
            [digest('a'), digest('c'), digest('d')]
        );
    }

    #[test]
    fn duplicate_and_reordered_windows_do_not_change_ranking() {
        let now = 20_000_000;
        let mut state = state(now);
        assert!(
            apply_usage(&mut state, usage("process-1", 2, &[('b', 4)], now), now)
                .unwrap()
                .accepted
        );
        let revision = state.revision;
        let score = state.candidates[0].score;
        let duplicate =
            apply_usage(&mut state, usage("process-1", 2, &[('b', 100)], now), now).unwrap();
        let reordered =
            apply_usage(&mut state, usage("process-1", 1, &[('b', 100)], now), now).unwrap();
        assert!(!duplicate.accepted);
        assert!(!reordered.accepted);
        assert_eq!(state.revision, revision);
        assert_eq!(state.candidates[0].score, score);
        assert_eq!(state.duplicate_or_stale_batches, 2);
    }

    #[test]
    fn delayed_other_process_does_not_regress_candidate_recency() {
        let now = 25_000_000;
        let mut state = state(now);
        apply_usage(&mut state, usage("process-1", 1, &[('b', 4)], now), now).unwrap();
        let original_recency = state.candidates[0].last_observed_at_ms;
        apply_usage(
            &mut state,
            usage("process-2", 1, &[('b', 2)], now - 1_000),
            now,
        )
        .unwrap();

        assert_eq!(state.candidates[0].last_observed_at_ms, original_recency);
        assert_eq!(state.candidates[0].score, 6);
    }

    #[test]
    fn candidate_and_process_maps_are_bounded() {
        let now = 30_000_000;
        let mut state = state(now);
        apply_usage(
            &mut state,
            usage("process-1", 1, &[('b', 1), ('c', 2), ('d', 3)], now),
            now,
        )
        .unwrap();
        apply_usage(
            &mut state,
            usage("process-2", 1, &[('e', 4)], now + 1),
            now + 1,
        )
        .unwrap();
        apply_usage(
            &mut state,
            usage("process-3", 1, &[('f', 5)], now + 2),
            now + 2,
        )
        .unwrap();
        assert_eq!(state.candidates.len(), 3);
        assert_eq!(state.process_windows.len(), 2);
        assert!(state
            .process_windows
            .iter()
            .all(|cursor| cursor.process_id != "process-1"));
    }

    #[test]
    fn process_options_reject_unbounded_or_incoherent_limits() {
        let options = PreloadProcessOptions {
            plan_read_timeout_ms: PreloadProcessOptions::default().startup_deadline_ms + 1,
            ..PreloadProcessOptions::default()
        };
        assert!(options.validate().is_err());
        let options = PreloadProcessOptions {
            max_observation_entries: MAX_BATCH_OBSERVATIONS_HARD as usize + 1,
            ..PreloadProcessOptions::default()
        };
        assert!(options.validate().is_err());
    }

    #[tokio::test]
    async fn optional_preload_counts_completed_acquisitions() {
        let futures = stream::iter(vec![
            (digest('a'), Ok::<u64, &'static str>(10)),
            (digest('b'), Err::<u64, &'static str>("unavailable")),
        ]);
        let outcome = collect_optional_preloads(
            futures,
            BTreeSet::from([digest('a'), digest('b')]),
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await;
        assert_eq!(outcome.ready, 1);
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.skipped, 0);
        assert_eq!(outcome.warmed_bytes, 10);
        assert!(!outcome.deadline_hit);
        assert!(outcome.pending_digests.is_empty());
    }

    #[tokio::test]
    async fn expired_preload_deadline_does_not_poll_new_acquisitions() {
        let started = Arc::new(AtomicBool::new(false));
        let mark_started = Arc::clone(&started);
        let futures = stream::once(async move {
            mark_started.store(true, Ordering::Release);
            (digest('a'), Ok::<u64, &'static str>(10))
        });
        let outcome = collect_optional_preloads(
            futures,
            BTreeSet::from([digest('a')]),
            tokio::time::Instant::now(),
        )
        .await;
        assert!(!started.load(Ordering::Acquire));
        assert!(outcome.deadline_hit);
        assert_eq!(outcome.ready, 0);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(outcome.pending_digests, BTreeSet::from([digest('a')]));
    }

    #[tokio::test]
    async fn optional_preload_deadline_drops_unneeded_waiter() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_on_drop = Arc::clone(&cancelled);
        let futures = stream::once(async move {
            let _guard = DropFlag(cancelled_on_drop);
            let result = std::future::pending::<std::result::Result<u64, &'static str>>().await;
            (digest('c'), result)
        });
        let outcome = collect_optional_preloads(
            futures,
            BTreeSet::from([digest('c')]),
            tokio::time::Instant::now() + Duration::from_millis(20),
        )
        .await;
        assert!(outcome.deadline_hit);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(outcome.pending_digests, BTreeSet::from([digest('c')]));
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn process_observations_coalesce_and_drop_distinct_overflow() {
        let options = PreloadProcessOptions {
            max_observation_entries: 1,
            ..PreloadProcessOptions::default()
        };
        let service = ProcessPreloadService::new(options).unwrap();
        let first = InstalledSoftware {
            package_id: digest('a'),
            digest: digest('a'),
            size_bytes: 10,
            package_name: String::from("first"),
            version: String::from("1"),
            commands: Vec::new(),
        };
        let second = InstalledSoftware {
            package_id: digest('b'),
            digest: digest('b'),
            size_bytes: 20,
            package_name: String::from("second"),
            version: String::from("1"),
            commands: Vec::new(),
        };

        service
            .observe("https://packages.example.test/a.aospkg", &first)
            .await;
        service
            .observe("https://packages.example.test/a.aospkg", &first)
            .await;
        service
            .observe("https://packages.example.test/b.aospkg", &second)
            .await;

        let buffer = service.observations.lock().await;
        assert_eq!(buffer.entries.len(), 1);
        assert_eq!(buffer.entries.get(&digest('a')).unwrap().count, 2);
        assert_eq!(buffer.dropped, 1);
    }
}
