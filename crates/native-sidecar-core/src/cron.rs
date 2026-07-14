use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr as _;

use agentos_sidecar_protocol::wire::{
    CancelCronJobRequest, CompleteCronRunRequest, CronAlarm, CronCancelledResponse, CronEventKind,
    CronEventRecord, CronJobEntry, CronJobsResponse, CronOverlap, CronRun,
    CronRunCompletedResponse, CronScheduledResponse, CronStateImportedResponse, CronWakeResponse,
    ScheduleCronRequest, WakeCronRequest,
};
use chrono::{
    DateTime, Datelike as _, Duration as ChronoDuration, NaiveDate, NaiveDateTime, TimeZone as _,
    Utc, Weekday,
};
use croner::Cron;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CRON_JOBS: usize = 1_024;
pub const MAX_ACTIVE_CRON_RUNS: usize = 4_096;
pub const MAX_CRON_ID_BYTES: usize = 256;
pub const MAX_CRON_SCHEDULE_BYTES: usize = 1_024;
pub const MAX_CRON_ACTION_BYTES: usize = 1024 * 1024;
pub const MAX_CRON_STATE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CRON_ERROR_BYTES: usize = 64 * 1024;
const CRON_STATE_VERSION: u32 = 1;

/// Action payload stored by the sidecar scheduler. Clients serialize explicit
/// caller input, but only the sidecar interprets executable actions. Callback
/// ids remain opaque host routes because their closures cannot cross the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum CronAction {
    Session {
        #[serde(rename = "agentType")]
        agent_type: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Value>,
    },
    Exec {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
    Callback {
        #[serde(rename = "callbackId")]
        callback_id: String,
    },
}

pub fn decode_cron_action(action: &str) -> Result<CronAction, CronSchedulerError> {
    serde_json::from_str(action).map_err(|error| {
        CronSchedulerError::InvalidArgument(format!("invalid cron action: {error}"))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronSchedulerError {
    InvalidSchedule(String),
    PastSchedule(String),
    InvalidArgument(String),
    JobLimit,
    UnknownRun(String),
    CounterExhausted(&'static str),
}

impl fmt::Display for CronSchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchedule(schedule) => {
                write!(f, "[invalid_schedule] invalid cron schedule: {schedule}")
            }
            Self::PastSchedule(schedule) => {
                write!(f, "[past_schedule] one-shot cron schedule is in the past: {schedule}")
            }
            Self::InvalidArgument(message) => f.write_str(message),
            Self::JobLimit => write!(
                f,
                "cron job limit exceeded: at most {MAX_CRON_JOBS} jobs can be scheduled per VM; cancel a job before adding another"
            ),
            Self::UnknownRun(run_id) => write!(f, "unknown or completed cron run: {run_id}"),
            Self::CounterExhausted(counter) => {
                write!(f, "cron {counter} counter exhausted; recreate the VM")
            }
        }
    }
}

impl std::error::Error for CronSchedulerError {}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum ParsedSchedule {
    Date(u64),
    Cron(ParsedCron),
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum ParsedCron {
    Standard(Cron),
    /// `LW` is part of the established JavaScript croner grammar but is not
    /// accepted by the Rust croner crate. Keep this small compatibility case
    /// in the one sidecar parser instead of copying a parser into each client.
    LastWeekday {
        base: Cron,
        day_of_week: Option<Cron>,
    },
}

impl ParsedCron {
    fn next_after(&self, now: &DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Cron expressions have whole-second precision. The parser otherwise
        // preserves the caller's millisecond fraction in returned occurrences.
        let whole_second_ms = now.timestamp_millis().div_euclid(1_000) * 1_000;
        let now = Utc.timestamp_millis_opt(whole_second_ms).single()?;
        match self {
            Self::Standard(cron) => cron.find_next_occurrence(&now, false).ok(),
            Self::LastWeekday { base, day_of_week } => {
                let mut cursor = now;
                // The date predicate is constant within a day. Jump to the
                // next day after a miss so wildcard seconds cannot turn one
                // lookup into millions of iterations.
                for _ in 0..(366 * 8) {
                    let candidate = base.find_next_occurrence(&cursor, false).ok()?;
                    let matches_day_of_week = day_of_week
                        .as_ref()
                        .is_some_and(|cron| cron.is_time_matching(&candidate).unwrap_or(false));
                    if is_last_weekday(candidate.date_naive()) || matches_day_of_week {
                        return Some(candidate);
                    }
                    let next_date = candidate.date_naive().succ_opt()?;
                    cursor = DateTime::<Utc>::from_naive_utc_and_offset(
                        next_date.and_hms_opt(0, 0, 0)?,
                        Utc,
                    ) - ChronoDuration::seconds(1);
                }
                None
            }
        }
    }
}

impl ParsedSchedule {
    fn next_after(&self, now_ms: u64) -> Option<u64> {
        match self {
            Self::Date(timestamp_ms) => (*timestamp_ms > now_ms).then_some(*timestamp_ms),
            Self::Cron(cron) => {
                let now_ms = i64::try_from(now_ms).ok()?;
                let now = Utc.timestamp_millis_opt(now_ms).single()?;
                let next = cron.next_after(&now)?;
                u64::try_from(next.timestamp_millis()).ok()
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CronJobState {
    revision: u64,
    schedule: String,
    parsed: ParsedSchedule,
    action: String,
    overlap: CronOverlap,
    last_run_ms: Option<u64>,
    next_run_ms: Option<u64>,
    run_count: u64,
    running_count: u32,
    queued: bool,
}

#[derive(Debug, Clone)]
struct ActiveCronRun {
    job_id: String,
    job_revision: u64,
    started_at_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CronStateSnapshot {
    version: u32,
    alarm_generation: u64,
    next_job_revision: u64,
    next_run_id: u64,
    jobs: Vec<CronJobSnapshot>,
    active_runs: Vec<ActiveCronRunSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CronJobSnapshot {
    id: String,
    revision: u64,
    schedule: String,
    action: String,
    overlap: CronOverlap,
    last_run_ms: Option<u64>,
    next_run_ms: Option<u64>,
    run_count: u64,
    queued: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveCronRunSnapshot {
    run_id: String,
    job_id: String,
    job_revision: u64,
    started_at_ms: u64,
}

/// Sidecar-owned cron state machine. It deliberately owns no timer: hosts arm
/// the absolute [`CronAlarm`] and forward its generation through [`wake`]. This
/// lets a native process use a normal timer while a durable actor uses its own
/// wake primitive without duplicating schedule policy.
#[derive(Debug, Default)]
pub struct CronScheduler {
    jobs: BTreeMap<String, CronJobState>,
    active_runs: BTreeMap<String, ActiveCronRun>,
    alarm_generation: u64,
    next_job_revision: u64,
    next_run_id: u64,
}

impl CronScheduler {
    pub fn schedule(
        &mut self,
        request: ScheduleCronRequest,
        now_ms: u64,
    ) -> Result<CronScheduledResponse, CronSchedulerError> {
        validate_schedule_request(&request)?;
        let parsed = parse_schedule(&request.schedule)?;
        let next_run_ms = parsed.next_after(now_ms);
        if matches!(parsed, ParsedSchedule::Date(_)) && next_run_ms.is_none() {
            return Err(CronSchedulerError::PastSchedule(request.schedule));
        }

        let id = request
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        validate_id(&id)?;
        if !self.jobs.contains_key(&id) && self.jobs.len() >= MAX_CRON_JOBS {
            return Err(CronSchedulerError::JobLimit);
        }
        let replaced_action_bytes = self.jobs.get(&id).map_or(0, |job| job.action.len());
        let total_action_bytes = self
            .total_action_bytes()
            .saturating_sub(replaced_action_bytes)
            .saturating_add(request.action.len());
        if total_action_bytes > MAX_CRON_STATE_BYTES {
            return Err(CronSchedulerError::InvalidArgument(format!(
                "cron action registry would exceed {MAX_CRON_STATE_BYTES} bytes; cancel jobs or reduce action payloads before scheduling another job"
            )));
        }

        let before = self.next_alarm_ms();
        let revision = self.allocate_job_revision()?;
        self.jobs.insert(
            id.clone(),
            CronJobState {
                revision,
                schedule: request.schedule,
                parsed,
                action: request.action,
                overlap: request.overlap.unwrap_or(CronOverlap::Allow),
                last_run_ms: None,
                next_run_ms,
                run_count: 0,
                running_count: 0,
                queued: false,
            },
        );
        self.refresh_alarm_generation(before)?;

        Ok(CronScheduledResponse {
            id,
            alarm: self.alarm(),
        })
    }

    pub fn list(&self) -> CronJobsResponse {
        CronJobsResponse {
            jobs: self
                .jobs
                .iter()
                .map(|(id, state)| CronJobEntry {
                    id: id.clone(),
                    schedule: state.schedule.clone(),
                    action: state.action.clone(),
                    overlap: state.overlap.clone(),
                    last_run_ms: state.last_run_ms,
                    next_run_ms: state.next_run_ms,
                    run_count: state.run_count,
                    running: state.running_count > 0,
                })
                .collect(),
            alarm: self.alarm(),
        }
    }

    pub fn cancel(
        &mut self,
        request: CancelCronJobRequest,
    ) -> Result<CronCancelledResponse, CronSchedulerError> {
        validate_id(&request.id)?;
        let before = self.next_alarm_ms();
        let cancelled = self.jobs.remove(&request.id).is_some();
        self.refresh_alarm_generation(before)?;
        Ok(CronCancelledResponse {
            id: request.id,
            cancelled,
            alarm: self.alarm(),
        })
    }

    pub fn wake(
        &mut self,
        request: WakeCronRequest,
        now_ms: u64,
    ) -> Result<CronWakeResponse, CronSchedulerError> {
        if request.generation != self.alarm_generation {
            return Ok(CronWakeResponse {
                alarm: self.alarm(),
                runs: Vec::new(),
                events: Vec::new(),
            });
        }

        let before = self.next_alarm_ms();
        let due_ids = self
            .jobs
            .iter()
            .filter_map(|(id, state)| {
                state
                    .next_run_ms
                    .filter(|next_run_ms| *next_run_ms <= now_ms)
                    .map(|_| id.clone())
            })
            .collect::<Vec<_>>();

        let mut runs = Vec::new();
        let mut events = Vec::new();
        for id in due_ids {
            let Some(mut state) = self.jobs.remove(&id) else {
                continue;
            };

            // Coalesce any number of missed occurrences into this one wake and
            // advance from `now`, rather than replaying an unbounded backlog.
            state.next_run_ms = state.parsed.next_after(now_ms);
            match state.overlap.clone() {
                CronOverlap::Skip if state.running_count > 0 => {}
                CronOverlap::Queue if state.running_count > 0 => state.queued = true,
                CronOverlap::Allow | CronOverlap::Skip | CronOverlap::Queue => {
                    self.start_run(&id, &mut state, now_ms, &mut runs, &mut events)?;
                }
            }
            self.jobs.insert(id, state);
        }
        self.refresh_alarm_generation(before)?;

        Ok(CronWakeResponse {
            alarm: self.alarm(),
            runs,
            events,
        })
    }

    pub fn complete(
        &mut self,
        request: CompleteCronRunRequest,
        now_ms: u64,
    ) -> Result<CronRunCompletedResponse, CronSchedulerError> {
        let active = self
            .active_runs
            .remove(&request.run_id)
            .ok_or_else(|| CronSchedulerError::UnknownRun(request.run_id.clone()))?;

        let mut events = vec![completion_event(&active, now_ms, request.error.as_deref())];
        let mut runs = Vec::new();
        if let Some(mut state) = self.jobs.remove(&active.job_id) {
            if state.revision == active.job_revision {
                state.running_count = state.running_count.saturating_sub(1);
                if state.queued && state.running_count == 0 {
                    state.queued = false;
                    self.start_run(&active.job_id, &mut state, now_ms, &mut runs, &mut events)?;
                }
            }
            self.jobs.insert(active.job_id, state);
        }

        Ok(CronRunCompletedResponse {
            alarm: self.alarm(),
            runs,
            events,
        })
    }

    pub fn alarm(&self) -> CronAlarm {
        CronAlarm {
            generation: self.alarm_generation,
            next_alarm_ms: self.next_alarm_ms(),
        }
    }

    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }

    pub fn total_action_bytes(&self) -> usize {
        self.jobs.values().map(|job| job.action.len()).sum()
    }

    /// Serialize scheduler truth for opaque host persistence. The format is
    /// private to the lockstep sidecar protocol; hosts must store it verbatim.
    pub fn export_state(&self) -> Result<String, CronSchedulerError> {
        let snapshot = CronStateSnapshot {
            version: CRON_STATE_VERSION,
            alarm_generation: self.alarm_generation,
            next_job_revision: self.next_job_revision,
            next_run_id: self.next_run_id,
            jobs: self
                .jobs
                .iter()
                .map(|(id, state)| CronJobSnapshot {
                    id: id.clone(),
                    revision: state.revision,
                    schedule: state.schedule.clone(),
                    action: state.action.clone(),
                    overlap: state.overlap.clone(),
                    last_run_ms: state.last_run_ms,
                    next_run_ms: state.next_run_ms,
                    run_count: state.run_count,
                    queued: state.queued,
                })
                .collect(),
            active_runs: self
                .active_runs
                .iter()
                .map(|(run_id, run)| ActiveCronRunSnapshot {
                    run_id: run_id.clone(),
                    job_id: run.job_id.clone(),
                    job_revision: run.job_revision,
                    started_at_ms: run.started_at_ms,
                })
                .collect(),
        };
        let state = serde_json::to_string(&snapshot).map_err(|error| {
            CronSchedulerError::InvalidArgument(format!(
                "failed to encode sidecar cron state: {error}"
            ))
        })?;
        if state.len() > MAX_CRON_STATE_BYTES {
            return Err(CronSchedulerError::InvalidArgument(format!(
                "encoded cron state exceeds {MAX_CRON_STATE_BYTES} bytes; cancel jobs or reduce action payloads before persisting the VM"
            )));
        }
        Ok(state)
    }

    /// Restore a sidecar-produced snapshot. In-flight runs are returned for
    /// at-least-once delivery after a cold wake; obsolete runs belonging to a
    /// cancelled or replaced job are intentionally discarded.
    pub fn import_state(
        &mut self,
        state: &str,
    ) -> Result<CronStateImportedResponse, CronSchedulerError> {
        if !self.jobs.is_empty() || !self.active_runs.is_empty() {
            return Err(CronSchedulerError::InvalidArgument(String::from(
                "cron state can only be imported into an empty sidecar scheduler",
            )));
        }
        if state.len() > MAX_CRON_STATE_BYTES {
            return Err(CronSchedulerError::InvalidArgument(format!(
                "cron state exceeds {MAX_CRON_STATE_BYTES} bytes"
            )));
        }
        let snapshot: CronStateSnapshot = serde_json::from_str(state).map_err(|error| {
            CronSchedulerError::InvalidArgument(format!(
                "cron state is not a valid sidecar snapshot: {error}"
            ))
        })?;
        if snapshot.version != CRON_STATE_VERSION {
            return Err(CronSchedulerError::InvalidArgument(format!(
                "unsupported cron state version {}; expected {CRON_STATE_VERSION}",
                snapshot.version
            )));
        }
        if snapshot.jobs.len() > MAX_CRON_JOBS {
            return Err(CronSchedulerError::JobLimit);
        }
        if snapshot.active_runs.len() > MAX_ACTIVE_CRON_RUNS {
            return Err(CronSchedulerError::InvalidArgument(format!(
                "cron state exceeds the {MAX_ACTIVE_CRON_RUNS} active-run limit"
            )));
        }

        let mut jobs = BTreeMap::new();
        let mut total_action_bytes = 0usize;
        let mut max_job_revision = 0u64;
        for job in snapshot.jobs {
            validate_id(&job.id)?;
            validate_schedule_request(&ScheduleCronRequest {
                id: Some(job.id.clone()),
                schedule: job.schedule.clone(),
                action: job.action.clone(),
                overlap: Some(job.overlap.clone()),
            })?;
            let parsed = parse_schedule(&job.schedule)?;
            total_action_bytes = total_action_bytes
                .checked_add(job.action.len())
                .ok_or_else(|| {
                    CronSchedulerError::InvalidArgument(String::from(
                        "cron action registry byte count overflowed",
                    ))
                })?;
            if total_action_bytes > MAX_CRON_STATE_BYTES {
                return Err(CronSchedulerError::InvalidArgument(format!(
                    "cron action registry exceeds {MAX_CRON_STATE_BYTES} bytes"
                )));
            }
            max_job_revision = max_job_revision.max(job.revision);
            let id = job.id.clone();
            if jobs
                .insert(
                    id.clone(),
                    CronJobState {
                        revision: job.revision,
                        schedule: job.schedule,
                        parsed,
                        action: job.action,
                        overlap: job.overlap,
                        last_run_ms: job.last_run_ms,
                        next_run_ms: job.next_run_ms,
                        run_count: job.run_count,
                        running_count: 0,
                        queued: job.queued,
                    },
                )
                .is_some()
            {
                return Err(CronSchedulerError::InvalidArgument(format!(
                    "cron state contains duplicate job id {id}"
                )));
            }
        }
        if snapshot.next_job_revision < max_job_revision {
            return Err(CronSchedulerError::InvalidArgument(String::from(
                "cron state job-revision counter is behind a stored job",
            )));
        }

        let mut active_runs = BTreeMap::new();
        let mut runs = Vec::new();
        for active in snapshot.active_runs {
            validate_id(&active.run_id)?;
            validate_id(&active.job_id)?;
            let Some(job) = jobs.get_mut(&active.job_id) else {
                continue;
            };
            if job.revision != active.job_revision {
                continue;
            }
            job.running_count = job
                .running_count
                .checked_add(1)
                .ok_or(CronSchedulerError::CounterExhausted("running-count"))?;
            runs.push(CronRun {
                run_id: active.run_id.clone(),
                job_id: active.job_id.clone(),
                action: job.action.clone(),
            });
            if active_runs
                .insert(
                    active.run_id.clone(),
                    ActiveCronRun {
                        job_id: active.job_id,
                        job_revision: active.job_revision,
                        started_at_ms: active.started_at_ms,
                    },
                )
                .is_some()
            {
                return Err(CronSchedulerError::InvalidArgument(format!(
                    "cron state contains duplicate run id {}",
                    active.run_id
                )));
            }
        }

        self.jobs = jobs;
        self.active_runs = active_runs;
        self.alarm_generation = snapshot.alarm_generation;
        self.next_job_revision = snapshot.next_job_revision;
        self.next_run_id = snapshot.next_run_id;
        Ok(CronStateImportedResponse {
            alarm: self.alarm(),
            runs,
            events: Vec::new(),
        })
    }

    fn start_run(
        &mut self,
        job_id: &str,
        state: &mut CronJobState,
        now_ms: u64,
        runs: &mut Vec<CronRun>,
        events: &mut Vec<CronEventRecord>,
    ) -> Result<(), CronSchedulerError> {
        if self.active_runs.len() >= MAX_ACTIVE_CRON_RUNS {
            events.push(CronEventRecord {
                kind: CronEventKind::Error,
                job_id: job_id.to_string(),
                time_ms: now_ms,
                duration_ms: None,
                error: Some(format!(
                    "cron active-run limit exceeded: at most {MAX_ACTIVE_CRON_RUNS} runs can be active per VM; shorten jobs or use overlap=skip/queue"
                )),
            });
            return Ok(());
        }

        let run_id = self.allocate_run_id()?;
        state.run_count = state
            .run_count
            .checked_add(1)
            .ok_or(CronSchedulerError::CounterExhausted("job run-count"))?;
        state.running_count = state
            .running_count
            .checked_add(1)
            .ok_or(CronSchedulerError::CounterExhausted("running-count"))?;
        state.last_run_ms = Some(now_ms);
        self.active_runs.insert(
            run_id.clone(),
            ActiveCronRun {
                job_id: job_id.to_string(),
                job_revision: state.revision,
                started_at_ms: now_ms,
            },
        );
        runs.push(CronRun {
            run_id,
            job_id: job_id.to_string(),
            action: state.action.clone(),
        });
        events.push(CronEventRecord {
            kind: CronEventKind::Fire,
            job_id: job_id.to_string(),
            time_ms: now_ms,
            duration_ms: None,
            error: None,
        });
        Ok(())
    }

    fn next_alarm_ms(&self) -> Option<u64> {
        self.jobs.values().filter_map(|job| job.next_run_ms).min()
    }

    fn refresh_alarm_generation(
        &mut self,
        previous_next_alarm_ms: Option<u64>,
    ) -> Result<(), CronSchedulerError> {
        if previous_next_alarm_ms != self.next_alarm_ms() {
            self.alarm_generation = self
                .alarm_generation
                .checked_add(1)
                .ok_or(CronSchedulerError::CounterExhausted("alarm-generation"))?;
        }
        Ok(())
    }

    fn allocate_job_revision(&mut self) -> Result<u64, CronSchedulerError> {
        self.next_job_revision = self
            .next_job_revision
            .checked_add(1)
            .ok_or(CronSchedulerError::CounterExhausted("job-revision"))?;
        Ok(self.next_job_revision)
    }

    fn allocate_run_id(&mut self) -> Result<String, CronSchedulerError> {
        self.next_run_id = self
            .next_run_id
            .checked_add(1)
            .ok_or(CronSchedulerError::CounterExhausted("run-id"))?;
        Ok(format!("cron-run-{}", self.next_run_id))
    }
}

fn completion_event(active: &ActiveCronRun, now_ms: u64, error: Option<&str>) -> CronEventRecord {
    CronEventRecord {
        kind: if error.is_some() {
            CronEventKind::Error
        } else {
            CronEventKind::Complete
        },
        job_id: active.job_id.clone(),
        time_ms: now_ms,
        duration_ms: Some(now_ms.saturating_sub(active.started_at_ms)),
        error: error.map(|error| truncate_utf8(error, MAX_CRON_ERROR_BYTES)),
    }
}

fn validate_schedule_request(request: &ScheduleCronRequest) -> Result<(), CronSchedulerError> {
    if request.schedule.len() > MAX_CRON_SCHEDULE_BYTES {
        return Err(CronSchedulerError::InvalidArgument(format!(
            "cron schedule exceeds {MAX_CRON_SCHEDULE_BYTES} bytes"
        )));
    }
    if request.action.len() > MAX_CRON_ACTION_BYTES {
        return Err(CronSchedulerError::InvalidArgument(format!(
            "cron action exceeds {MAX_CRON_ACTION_BYTES} bytes"
        )));
    }
    serde_json::from_str::<serde_json::Value>(&request.action).map_err(|error| {
        CronSchedulerError::InvalidArgument(format!("cron action must be valid JSON: {error}"))
    })?;
    if let Some(id) = request.id.as_deref() {
        validate_id(id)?;
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), CronSchedulerError> {
    if id.is_empty() {
        return Err(CronSchedulerError::InvalidArgument(String::from(
            "cron id must not be empty",
        )));
    }
    if id.len() > MAX_CRON_ID_BYTES {
        return Err(CronSchedulerError::InvalidArgument(format!(
            "cron id exceeds {MAX_CRON_ID_BYTES} bytes"
        )));
    }
    Ok(())
}

fn parse_schedule(schedule: &str) -> Result<ParsedSchedule, CronSchedulerError> {
    let normalized = schedule.trim();
    if normalized.is_empty() {
        return Err(CronSchedulerError::InvalidSchedule(schedule.to_string()));
    }
    if looks_like_one_shot(normalized) {
        return parse_one_shot(normalized)
            .map(ParsedSchedule::Date)
            .ok_or_else(|| CronSchedulerError::InvalidSchedule(schedule.to_string()));
    }

    // JavaScript croner rejects a bare numeric prefix step (`5/15`) while the
    // Rust parser accepts it. Reject it explicitly to preserve the established
    // cross-SDK grammar; wildcard and range steps remain valid.
    if normalized.split_whitespace().any(|field| {
        field.split(',').any(|part| {
            part.split_once('/')
                .is_some_and(|(range, _)| range != "*" && !range.contains('-'))
        })
    }) {
        return Err(CronSchedulerError::InvalidSchedule(schedule.to_string()));
    }

    parse_cron(normalized)
        .map(ParsedSchedule::Cron)
        .map_err(|_| CronSchedulerError::InvalidSchedule(schedule.to_string()))
}

fn parse_cron(schedule: &str) -> Result<ParsedCron, ()> {
    let mut fields = schedule.split_whitespace().collect::<Vec<_>>();
    let (day_of_month_index, day_of_week_index) = match fields.len() {
        5 => (2, 4),
        6 | 7 => (3, 5),
        _ => return Err(()),
    };
    if !fields[day_of_month_index].eq_ignore_ascii_case("LW") {
        return Cron::from_str(schedule)
            .map(ParsedCron::Standard)
            .map_err(|_| ());
    }

    let original_day_of_week = fields[day_of_week_index];
    fields[day_of_month_index] = "*";
    fields[day_of_week_index] = "*";
    let base = Cron::from_str(&fields.join(" ")).map_err(|_| ())?;
    let day_of_week = if matches!(original_day_of_week, "*" | "?") {
        None
    } else {
        fields[day_of_month_index] = "?";
        fields[day_of_week_index] = original_day_of_week;
        Some(Cron::from_str(&fields.join(" ")).map_err(|_| ())?)
    };
    Ok(ParsedCron::LastWeekday { base, day_of_week })
}

fn is_last_weekday(date: NaiveDate) -> bool {
    if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    let (next_year, next_month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    let mut last = match NaiveDate::from_ymd_opt(next_year, next_month, 1) {
        Some(first_next_month) => first_next_month - ChronoDuration::days(1),
        None => return false,
    };
    while matches!(last.weekday(), Weekday::Sat | Weekday::Sun) {
        last -= ChronoDuration::days(1);
    }
    date == last
}

fn looks_like_one_shot(schedule: &str) -> bool {
    let bytes = schedule.as_bytes();
    let mut index = 0usize;
    let take_digits = |index: &mut usize, count: usize| -> bool {
        for _ in 0..count {
            match bytes.get(*index) {
                Some(byte) if byte.is_ascii_digit() => *index += 1,
                _ => return false,
            }
        }
        true
    };
    let take = |index: &mut usize, expected: u8| -> bool {
        if bytes.get(*index) == Some(&expected) {
            *index += 1;
            true
        } else {
            false
        }
    };

    if !take_digits(&mut index, 4)
        || !take(&mut index, b'-')
        || !take_digits(&mut index, 2)
        || !take(&mut index, b'-')
        || !take_digits(&mut index, 2)
    {
        return false;
    }
    if index == bytes.len() {
        return true;
    }
    if !matches!(bytes.get(index), Some(b'T' | b' ')) {
        return false;
    }
    index += 1;
    if !take_digits(&mut index, 2) || !take(&mut index, b':') || !take_digits(&mut index, 2) {
        return false;
    }
    if take(&mut index, b':') {
        if !take_digits(&mut index, 2) {
            return false;
        }
        if take(&mut index, b'.') {
            let start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if index == start {
                return false;
            }
        }
    }
    match bytes.get(index) {
        None => return true,
        Some(b'Z') => index += 1,
        Some(b'+' | b'-') => {
            index += 1;
            if !take_digits(&mut index, 2) || !take(&mut index, b':') || !take_digits(&mut index, 2)
            {
                return false;
            }
        }
        _ => return false,
    }
    index == bytes.len()
}

fn parse_one_shot(schedule: &str) -> Option<u64> {
    let timestamp = if let Ok(date) = DateTime::parse_from_rfc3339(schedule) {
        date.with_timezone(&Utc)
    } else {
        let normalized = schedule.replacen(' ', "T", 1);
        let mut parsed = None;
        for format in [
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M",
        ] {
            if let Ok(date) = NaiveDateTime::parse_from_str(&normalized, format) {
                parsed = Utc.from_local_datetime(&date).single();
                break;
            }
        }
        parsed.or_else(|| {
            NaiveDate::parse_from_str(schedule, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|date| DateTime::<Utc>::from_naive_utc_and_offset(date, Utc))
        })?
    };
    u64::try_from(timestamp.timestamp_millis()).ok()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const ELLIPSIS: &str = "…";
    if max_bytes < ELLIPSIS.len() {
        return String::new();
    }
    let mut end = max_bytes - ELLIPSIS.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], ELLIPSIS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(
        scheduler: &mut CronScheduler,
        id: &str,
        expression: &str,
        overlap: Option<CronOverlap>,
        now_ms: u64,
    ) -> CronScheduledResponse {
        scheduler
            .schedule(
                ScheduleCronRequest {
                    id: Some(id.to_string()),
                    schedule: expression.to_string(),
                    action: String::from("{\"type\":\"callback\",\"callbackId\":\"cb\"}"),
                    overlap,
                },
                now_ms,
            )
            .unwrap()
    }

    #[test]
    fn defaults_overlap_and_allocates_id_in_sidecar() {
        let mut scheduler = CronScheduler::default();
        let response = scheduler
            .schedule(
                ScheduleCronRequest {
                    id: None,
                    schedule: String::from("* * * * *"),
                    action: String::from("{}"),
                    overlap: None,
                },
                1_700_000_000_000,
            )
            .unwrap();
        assert!(!response.id.is_empty());
        assert_eq!(scheduler.list().jobs[0].overlap, CronOverlap::Allow);
        assert!(response.alarm.next_alarm_ms.is_some());
    }

    #[test]
    fn validates_schedule_grammar_and_past_dates() {
        let mut scheduler = CronScheduler::default();
        for expression in [
            "* * * * *",
            "* * * * * *",
            "0 * * * * * 2030",
            "0 0 * JAN MON",
            "0 0 L * *",
            "0 0 LW * *",
            "0 0 15W * *",
            "0 0 * * MON#2",
        ] {
            schedule(
                &mut scheduler,
                expression,
                expression,
                None,
                1_700_000_000_000,
            );
        }

        let invalid = scheduler.schedule(
            ScheduleCronRequest {
                id: Some(String::from("invalid")),
                schedule: String::from("5/15 * * * *"),
                action: String::from("{}"),
                overlap: None,
            },
            1_700_000_000_000,
        );
        assert!(matches!(
            invalid,
            Err(CronSchedulerError::InvalidSchedule(_))
        ));

        let past = scheduler.schedule(
            ScheduleCronRequest {
                id: Some(String::from("past")),
                schedule: String::from("2020-01-01T00:00:00Z"),
                action: String::from("{}"),
                overlap: None,
            },
            1_700_000_000_000,
        );
        assert!(matches!(past, Err(CronSchedulerError::PastSchedule(_))));
    }

    #[test]
    fn last_weekday_uses_the_final_monday_to_friday_of_the_month() {
        let parsed = parse_schedule("0 0 LW * *").expect("parse LW");
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        let next = parsed.next_after(now).expect("next LW");
        assert_eq!(
            Utc.timestamp_millis_opt(next as i64).single().unwrap(),
            DateTime::parse_from_rfc3339("2026-01-30T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn stale_wake_is_noop_and_current_wake_advances_alarm() {
        let mut scheduler = CronScheduler::default();
        let response = schedule(
            &mut scheduler,
            "once",
            "2026-01-01T00:00:01Z",
            None,
            1_767_225_600_000,
        );
        let stale = scheduler
            .wake(
                WakeCronRequest {
                    generation: response.alarm.generation - 1,
                },
                1_767_225_601_000,
            )
            .unwrap();
        assert!(stale.runs.is_empty());

        let due = scheduler
            .wake(
                WakeCronRequest {
                    generation: response.alarm.generation,
                },
                1_767_225_601_000,
            )
            .unwrap();
        assert_eq!(due.runs.len(), 1);
        assert_eq!(due.events[0].kind, CronEventKind::Fire);
        assert_eq!(due.alarm.next_alarm_ms, None);
    }

    #[test]
    fn queue_overlap_starts_one_deferred_run_on_completion() {
        let mut scheduler = CronScheduler::default();
        let first_alarm = schedule(
            &mut scheduler,
            "queue",
            "* * * * * *",
            Some(CronOverlap::Queue),
            1_700_000_000_000,
        )
        .alarm;
        let first = scheduler
            .wake(
                WakeCronRequest {
                    generation: first_alarm.generation,
                },
                first_alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        assert_eq!(first.runs.len(), 1);
        let second = scheduler
            .wake(
                WakeCronRequest {
                    generation: first.alarm.generation,
                },
                first.alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        assert!(second.runs.is_empty());

        let completed = scheduler
            .complete(
                CompleteCronRunRequest {
                    run_id: first.runs[0].run_id.clone(),
                    error: None,
                },
                second.alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        assert_eq!(completed.runs.len(), 1);
        assert_eq!(scheduler.list().jobs[0].run_count, 2);
    }

    #[test]
    fn skip_overlap_drops_a_due_run_while_active() {
        let mut scheduler = CronScheduler::default();
        let alarm = schedule(
            &mut scheduler,
            "skip",
            "* * * * * *",
            Some(CronOverlap::Skip),
            1_700_000_000_000,
        )
        .alarm;
        let first = scheduler
            .wake(
                WakeCronRequest {
                    generation: alarm.generation,
                },
                alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        let second = scheduler
            .wake(
                WakeCronRequest {
                    generation: first.alarm.generation,
                },
                first.alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();

        assert_eq!(first.runs.len(), 1);
        assert!(second.runs.is_empty());
        assert_eq!(scheduler.list().jobs[0].run_count, 1);
    }

    #[test]
    fn allow_overlap_starts_concurrent_runs() {
        let mut scheduler = CronScheduler::default();
        let alarm = schedule(
            &mut scheduler,
            "allow",
            "* * * * * *",
            None,
            1_700_000_000_000,
        )
        .alarm;
        let first = scheduler
            .wake(
                WakeCronRequest {
                    generation: alarm.generation,
                },
                alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        let second = scheduler
            .wake(
                WakeCronRequest {
                    generation: first.alarm.generation,
                },
                first.alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();

        assert_eq!(first.runs.len(), 1);
        assert_eq!(second.runs.len(), 1);
        let job = &scheduler.list().jobs[0];
        assert_eq!(job.run_count, 2);
        assert!(job.running);
    }

    #[test]
    fn completion_event_records_sidecar_duration_and_error() {
        let mut scheduler = CronScheduler::default();
        let alarm = schedule(
            &mut scheduler,
            "error",
            "2026-01-01T00:00:01Z",
            None,
            1_767_225_600_000,
        )
        .alarm;
        let wake = scheduler
            .wake(
                WakeCronRequest {
                    generation: alarm.generation,
                },
                1_767_225_601_000,
            )
            .unwrap();
        let completed = scheduler
            .complete(
                CompleteCronRunRequest {
                    run_id: wake.runs[0].run_id.clone(),
                    error: Some("boom".to_string()),
                },
                1_767_225_601_125,
            )
            .unwrap();

        assert_eq!(completed.events[0].kind, CronEventKind::Error);
        assert_eq!(completed.events[0].duration_ms, Some(125));
        assert_eq!(completed.events[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn opaque_state_round_trip_restores_jobs_and_replays_active_runs() {
        let now_ms = 1_700_000_000_000;
        let mut scheduler = CronScheduler::default();
        schedule(
            &mut scheduler,
            "durable",
            "* * * * * *",
            Some(CronOverlap::Queue),
            now_ms,
        );
        let alarm = scheduler.alarm();
        let wake = scheduler
            .wake(
                WakeCronRequest {
                    generation: alarm.generation,
                },
                alarm.next_alarm_ms.unwrap(),
            )
            .unwrap();
        assert_eq!(wake.runs.len(), 1);

        let state = scheduler.export_state().unwrap();
        assert!(state.len() <= MAX_CRON_STATE_BYTES);
        let mut restored = CronScheduler::default();
        let imported = restored.import_state(&state).unwrap();
        assert_eq!(imported.runs, wake.runs);
        assert_eq!(restored.list(), scheduler.list());

        let completed = restored
            .complete(
                CompleteCronRunRequest {
                    run_id: imported.runs[0].run_id.clone(),
                    error: None,
                },
                alarm.next_alarm_ms.unwrap() + 25,
            )
            .unwrap();
        assert_eq!(completed.events[0].kind, CronEventKind::Complete);
    }

    #[test]
    fn opaque_state_rejects_unknown_versions_and_nonempty_imports() {
        let mut scheduler = CronScheduler::default();
        let error = scheduler
            .import_state(
                r#"{"version":99,"alarmGeneration":0,"nextJobRevision":0,"nextRunId":0,"jobs":[],"activeRuns":[]}"#,
            )
            .unwrap_err();
        assert!(error.to_string().contains("unsupported cron state version"));

        schedule(
            &mut scheduler,
            "existing",
            "* * * * *",
            None,
            1_700_000_000_000,
        );
        let error = scheduler.import_state("{}").unwrap_err();
        assert!(error.to_string().contains("empty sidecar scheduler"));
    }
}
