//! Cron scheduling + the `CronManager`.
//!
//! Ported from `packages/core/src/cron/`. The `schedule` is a 5-field cron expression or an ISO-8601
//! one-shot timestamp. `CronAction::Callback` is in-process only (non-serializable). `on_cron_event`
//! returns NO unsubscribe in TS; the Rust equivalent is a [`tokio::sync::broadcast::Receiver`] whose
//! drop is the unsubscribe.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use scc::HashMap as SccHashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::agent_os::AgentOs;
use crate::config::ScheduleDriver;
use crate::error::ClientError;
use crate::session::CreateSessionOptions;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Overlap policy for a cron job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronOverlap {
    #[default]
    Allow,
    Skip,
    Queue,
}

/// A cron action. `Callback` holds an in-process closure and cannot cross the wire.
#[derive(Clone)]
pub enum CronAction {
    /// Create a session, prompt it, then close it.
    Session {
        agent_type: String,
        prompt: String,
        options: Option<CreateSessionOptions>,
    },
    /// Run a command via `exec`.
    Exec { command: String, args: Vec<String> },
    /// Invoke a host-side callback.
    Callback {
        #[allow(clippy::type_complexity)]
        callback: Arc<dyn Fn() -> futures::future::BoxFuture<'static, ()> + Send + Sync>,
    },
}

impl std::fmt::Debug for CronAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CronAction::Session {
                agent_type, prompt, ..
            } => f
                .debug_struct("Session")
                .field("agent_type", agent_type)
                .field("prompt", prompt)
                .finish_non_exhaustive(),
            CronAction::Exec { command, args } => f
                .debug_struct("Exec")
                .field("command", command)
                .field("args", args)
                .finish(),
            CronAction::Callback { .. } => f.debug_struct("Callback").finish_non_exhaustive(),
        }
    }
}

/// Options for `schedule_cron`.
#[derive(Clone)]
pub struct CronJobOptions {
    /// Default: a fresh UUID.
    pub id: Option<String>,
    /// 5-field cron expression OR an ISO-8601 one-shot timestamp.
    pub schedule: String,
    pub action: CronAction,
    /// Default: [`CronOverlap::Allow`].
    pub overlap: Option<CronOverlap>,
}

/// Snapshot info for a cron job.
#[derive(Debug, Clone)]
pub struct CronJobInfo {
    pub id: String,
    pub schedule: String,
    pub action: CronAction,
    pub overlap: CronOverlap,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub running: bool,
}

/// A cron event emitted on each run.
#[derive(Debug, Clone)]
pub enum CronEvent {
    Fire {
        job_id: String,
        time: DateTime<Utc>,
    },
    Complete {
        job_id: String,
        time: DateTime<Utc>,
        duration_ms: f64,
    },
    Error {
        job_id: String,
        time: DateTime<Utc>,
        error: String,
    },
}

/// Handle to a scheduled cron job. Dropping or calling [`CronJobHandle::cancel`] cancels it.
#[derive(Clone)]
pub struct CronJobHandle {
    pub id: String,
    pub(crate) manager: Arc<CronManager>,
}

impl CronJobHandle {
    /// Cancel the job (no-op if already cancelled/unknown).
    pub fn cancel(&self) {
        self.manager.cancel_job(&self.id);
    }
}

// ---------------------------------------------------------------------------
// CronManager + CronJobState
// ---------------------------------------------------------------------------

/// Internal per-job state.
pub(crate) struct CronJobState {
    pub schedule: String,
    pub action: CronAction,
    pub overlap: CronOverlap,
    pub last_run: parking_lot::Mutex<Option<DateTime<Utc>>>,
    pub next_run: parking_lot::Mutex<Option<DateTime<Utc>>>,
    pub run_count: std::sync::atomic::AtomicU64,
    pub running: AtomicBool,
    /// Set when a `Queue`-policy fire arrives while the job is already running; drained to exactly
    /// one deferred run when the active run completes. Mirrors TS `CronJobState.queued`.
    pub queued: AtomicBool,
    pub cancel: tokio_util::sync::CancellationToken,
}

/// Owns scheduled jobs, the schedule driver, and the cron event broadcast.
pub struct CronManager {
    pub(crate) jobs: SccHashMap<String, CronJobState>,
    pub(crate) driver: Arc<dyn ScheduleDriver>,
    pub(crate) event_tx: broadcast::Sender<CronEvent>,
}

impl CronManager {
    /// Create a cron manager with the given schedule driver.
    pub(crate) fn new(driver: Arc<dyn ScheduleDriver>) -> Self {
        let (event_tx, _rx) = broadcast::channel(256);
        Self {
            jobs: SccHashMap::new(),
            driver,
            event_tx,
        }
    }

    /// Cancel a job by id (no-op if unknown).
    ///
    /// Mirrors TS `CronManager.cancel`: cancel the driver-armed timer (here the per-job
    /// [`tokio_util::sync::CancellationToken`]) and remove the job from the registry.
    pub(crate) fn cancel_job(&self, id: &str) {
        if let Some((_, state)) = self.jobs.remove(id) {
            state.cancel.cancel();
        }
    }

    /// Dispose all jobs (called during shutdown).
    ///
    /// Mirrors TS `CronManager.dispose`: cancel every armed timer, then clear the registry. The
    /// schedule driver itself is owned by the config and torn down separately.
    pub(crate) fn dispose(&self) {
        self.jobs.scan(|_, state| {
            state.cancel.cancel();
        });
        self.jobs.clear();
    }

}

/// Execute a single job run, honoring the overlap policy. Emits `Fire`, then `Complete` or `Error`.
/// Re-runs once at the end if a `Queue`-policy run was deferred while busy.
///
/// Mirrors TS `CronManager.executeJob`. Handler/action errors never crash the manager; on error a
/// `cron:error` event is emitted instead of a `cron:complete`. Returns an explicitly boxed `Send`
/// future (rather than an `async fn`) so the recursive queued re-run does not form a
/// self-referential async auto-trait inference cycle that would defeat the `Send` bound required by
/// [`tokio::spawn`].
fn execute_job(
    manager: Arc<CronManager>,
    vm: AgentOs,
    id: String,
) -> futures::future::BoxFuture<'static, ()> {
    Box::pin(execute_job_inner(manager, vm, id))
}

async fn execute_job_inner(manager: Arc<CronManager>, vm: AgentOs, id: String) {
    let manager = &manager;
    let vm = &vm;
    let id = id.as_str();
    // Overlap policy: a running job either allows a concurrent run, skips this fire, or queues
    // exactly one deferred run.
    {
        let mut should_return = false;
        let mut should_queue = false;
        manager.jobs.read(id, |_, state| {
            if state.running.load(Ordering::SeqCst) {
                match state.overlap {
                    CronOverlap::Allow => {}
                    CronOverlap::Skip => should_return = true,
                    CronOverlap::Queue => should_queue = true,
                }
            }
        });
        if should_return {
            return;
        }
        if should_queue {
            manager.jobs.read(id, |_, state| {
                state.queued.store(true, Ordering::SeqCst);
            });
            return;
        }
    }

    // Mark running, record this run, and snapshot the action to dispatch.
    let action = match manager.jobs.read(id, |_, state| {
        state.running.store(true, Ordering::SeqCst);
        *state.last_run.lock() = Some(manager.driver.now());
        state.run_count.fetch_add(1, Ordering::SeqCst);
        state.action.clone()
    }) {
        Some(action) => action,
        None => return,
    };

    let fire_time = manager.driver.now();
    let _ = manager.event_tx.send(CronEvent::Fire {
        job_id: id.to_string(),
        time: fire_time,
    });

    let start = std::time::Instant::now();
    let result = run_action(vm, &action).await;
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(()) => {
            let _ = manager.event_tx.send(CronEvent::Complete {
                job_id: id.to_string(),
                time: manager.driver.now(),
                duration_ms,
            });
        }
        Err(error) => {
            let _ = manager.event_tx.send(CronEvent::Error {
                job_id: id.to_string(),
                time: manager.driver.now(),
                error: error.to_string(),
            });
        }
    }

    // Clear running, recompute the next run, and drain a queued run if one was deferred.
    let mut run_queued = false;
    manager.jobs.read(id, |_, state| {
        state.running.store(false, Ordering::SeqCst);
        *state.next_run.lock() = compute_next_time(&state.schedule, manager.driver.now());
        if state.queued.swap(false, Ordering::SeqCst) {
            run_queued = true;
        }
    });

    if run_queued {
        let manager = Arc::clone(manager);
        let vm = vm.clone();
        let id = id.to_string();
        tokio::spawn(execute_job(manager, vm, id));
    }
}

/// Dispatch a [`CronAction`]. Mirrors TS `CronManager.runAction`.
///
/// `Session` creates a session, prompts it, and always closes it (even if the prompt errors, the
/// close still runs, matching the TS `finally`). `Exec` joins the command and args into a single
/// shell command string and runs it via [`AgentOs::exec`]. `Callback` awaits the in-process future.
async fn run_action(vm: &AgentOs, action: &CronAction) -> Result<(), ClientError> {
    match action {
        CronAction::Session {
            agent_type,
            prompt,
            options,
        } => {
            let session = vm
                .create_session(agent_type, options.clone().unwrap_or_default())
                .await
                .map_err(|err| ClientError::Sidecar(err.to_string()))?;
            let prompt_result = vm.prompt(&session.session_id, prompt).await;
            // Always close the session, mirroring the TS `finally` block.
            let _ = vm.close_session(&session.session_id);
            prompt_result.map_err(|err| ClientError::Sidecar(err.to_string()))?;
            Ok(())
        }
        CronAction::Exec { command, args } => {
            let cmd = if args.is_empty() {
                command.clone()
            } else {
                format!("{} {}", command, args.join(" "))
            };
            vm.exec(&cmd, crate::process::ExecOptions::default())
                .await
                .map_err(|err| ClientError::Sidecar(err.to_string()))?;
            Ok(())
        }
        CronAction::Callback { callback } => {
            callback().await;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Schedule validation
// ---------------------------------------------------------------------------

/// A parsed schedule: either a 5-field recurring cron expression or a one-shot ISO-8601 timestamp.
///
/// Mirrors TS `ParsedSchedule` (`parse-schedule.ts`).
enum ParsedSchedule {
    /// A one-shot absolute timestamp.
    Date(DateTime<Utc>),
    /// A recurring 5-field cron expression.
    Cron(CronExpr),
}

/// Decide whether a schedule string looks like a one-shot ISO-8601-ish timestamp rather than a cron
/// expression. Mirrors TS `looksLikeOneShotSchedule` /
/// `^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2})?)?$`.
fn looks_like_one_shot(schedule: &str) -> bool {
    let bytes = schedule.as_bytes();
    let mut i = 0usize;

    // Helper closures over the byte slice.
    let is_digit = |b: u8| b.is_ascii_digit();

    // YYYY-MM-DD
    let take_digits = |bytes: &[u8], i: &mut usize, n: usize| -> bool {
        for _ in 0..n {
            match bytes.get(*i) {
                Some(&b) if is_digit(b) => *i += 1,
                _ => return false,
            }
        }
        true
    };
    let take_lit = |bytes: &[u8], i: &mut usize, lit: u8| -> bool {
        match bytes.get(*i) {
            Some(&b) if b == lit => {
                *i += 1;
                true
            }
            _ => false,
        }
    };

    if !take_digits(bytes, &mut i, 4) {
        return false;
    }
    if !take_lit(bytes, &mut i, b'-') {
        return false;
    }
    if !take_digits(bytes, &mut i, 2) {
        return false;
    }
    if !take_lit(bytes, &mut i, b'-') {
        return false;
    }
    if !take_digits(bytes, &mut i, 2) {
        return false;
    }

    // Optional time portion: [T ]HH:MM(:SS(.fff)?)?(Z|[+-]HH:MM)?
    if i == bytes.len() {
        return true;
    }
    match bytes.get(i) {
        Some(b'T') | Some(b' ') => i += 1,
        _ => return false,
    }
    if !take_digits(bytes, &mut i, 2) {
        return false;
    }
    if !take_lit(bytes, &mut i, b':') {
        return false;
    }
    if !take_digits(bytes, &mut i, 2) {
        return false;
    }

    // Optional :SS
    if take_lit(bytes, &mut i, b':') {
        if !take_digits(bytes, &mut i, 2) {
            return false;
        }
        // Optional .fff (1-3 digits)
        if take_lit(bytes, &mut i, b'.') {
            let mut frac = 0;
            while frac < 3 && matches!(bytes.get(i), Some(&b) if is_digit(b)) {
                i += 1;
                frac += 1;
            }
            if frac == 0 {
                return false;
            }
        }
    }

    // Optional timezone: Z | [+-]HH:MM
    match bytes.get(i) {
        None => return true,
        Some(b'Z') => {
            i += 1;
        }
        Some(b'+') | Some(b'-') => {
            i += 1;
            if !take_digits(bytes, &mut i, 2) {
                return false;
            }
            if !take_lit(bytes, &mut i, b':') {
                return false;
            }
            if !take_digits(bytes, &mut i, 2) {
                return false;
            }
        }
        _ => return false,
    }

    i == bytes.len()
}

/// Parse a one-shot timestamp string into a UTC instant. Accepts a date-only form (interpreted as
/// midnight UTC), a `T`/space-separated local-ish datetime (interpreted as UTC when no offset is
/// present), and RFC-3339 forms with `Z`/offset. Mirrors `Date.parse(...)` semantics closely enough
/// for the one-shot pattern accepted by [`looks_like_one_shot`].
fn parse_one_shot(schedule: &str) -> Option<DateTime<Utc>> {
    // Try a full RFC-3339 timestamp first (handles Z and numeric offsets).
    if let Ok(dt) = DateTime::parse_from_rfc3339(schedule) {
        return Some(dt.with_timezone(&Utc));
    }

    // Normalize a space separator to `T` for the naive parsers below.
    let normalized = schedule.replacen(' ', "T", 1);

    // Date + time without a timezone: treat as UTC.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&normalized, fmt) {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
        }
    }

    // Date only: midnight UTC.
    if let Ok(date) = chrono::NaiveDate::parse_from_str(schedule, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }

    None
}

/// Parse a schedule string into a [`ParsedSchedule`]. Mirrors TS `parseSchedule`.
fn parse_schedule(schedule: &str) -> std::result::Result<ParsedSchedule, ClientError> {
    let normalized = schedule.trim();
    if looks_like_one_shot(normalized) {
        return match parse_one_shot(normalized) {
            Some(date) => Ok(ParsedSchedule::Date(date)),
            None => Err(ClientError::InvalidSchedule(schedule.to_string())),
        };
    }

    match CronExpr::parse(normalized) {
        Ok(cron) => Ok(ParsedSchedule::Cron(cron)),
        Err(_) => Err(ClientError::InvalidSchedule(schedule.to_string())),
    }
}

/// Compute the next fire time for a schedule string strictly after `now`. Returns `None` for a
/// one-shot timestamp in the past or a cron expression with no upcoming match. Mirrors TS
/// `computeNextTime` / `resolveSchedule(...).nextRun`.
pub(crate) fn compute_next_time(schedule: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match parse_schedule(schedule).ok()? {
        ParsedSchedule::Cron(cron) => cron.next_after(now),
        ParsedSchedule::Date(date) => {
            if date.timestamp_millis() > now.timestamp_millis() {
                Some(date)
            } else {
                None
            }
        }
    }
}

/// Validate a schedule string. Returns the parsed next run for one-shot ISO-8601 schedules.
///
/// Errors `InvalidSchedule` for malformed input and `PastSchedule` for one-shot timestamps already
/// in the past. Mirrors TS `validateScheduleForRegistration`: a one-shot timestamp that resolves to
/// no next run is rejected as `PastSchedule`; cron expressions are accepted even when their next run
/// is currently unknown.
pub(crate) fn validate_schedule(
    schedule: &str,
    now: DateTime<Utc>,
) -> std::result::Result<Option<DateTime<Utc>>, ClientError> {
    let parsed = parse_schedule(schedule)?;
    match parsed {
        ParsedSchedule::Cron(cron) => Ok(cron.next_after(now)),
        ParsedSchedule::Date(date) => {
            if date.timestamp_millis() > now.timestamp_millis() {
                Ok(Some(date))
            } else {
                Err(ClientError::PastSchedule(schedule.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5-field cron expression parser + next-run search
// ---------------------------------------------------------------------------

/// A parsed 5-field cron expression (`minute hour day-of-month month day-of-week`).
///
/// Implemented in-crate because the workspace has no cron-parsing dependency. Supports the standard
/// field grammar: `*`, ranges (`a-b`), steps (`*/n`, `a-b/n`, `a/n`), and comma lists, with the
/// usual ranges (minute 0-59, hour 0-23, day-of-month 1-31, month 1-12, day-of-week 0-6 with `7`
/// folded onto Sunday). Day-of-month and day-of-week combine with OR semantics when both are
/// restricted, matching Vixie cron.
struct CronExpr {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days_of_month: Vec<u32>,
    months: Vec<u32>,
    days_of_week: Vec<u32>,
    dom_restricted: bool,
    dow_restricted: bool,
}

impl CronExpr {
    fn parse(expr: &str) -> std::result::Result<Self, ()> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(());
        }

        let minutes = parse_field(fields[0], 0, 59)?;
        let hours = parse_field(fields[1], 0, 23)?;
        let days_of_month = parse_field(fields[2], 1, 31)?;
        let months = parse_field(fields[3], 1, 12)?;
        let mut days_of_week = parse_field(fields[4], 0, 7)?;
        // Fold `7` (Sunday) onto `0` and dedupe.
        for v in days_of_week.iter_mut() {
            if *v == 7 {
                *v = 0;
            }
        }
        days_of_week.sort_unstable();
        days_of_week.dedup();

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    /// Find the next instant strictly after `after` (truncated to whole minutes) that matches. Scans
    /// minute-by-minute up to a bounded horizon (~4 years) so an impossible expression terminates.
    fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Start from the next whole minute after `after`.
        let mut candidate = after
            .with_second(0)?
            .with_nanosecond(0)?
            .checked_add_signed(ChronoDuration::minutes(1))?;

        // Bound the search: 4 years of minutes.
        let max_iterations = 366 * 4 * 24 * 60;
        for _ in 0..max_iterations {
            if self.matches(&candidate) {
                return Some(candidate);
            }
            candidate = candidate.checked_add_signed(ChronoDuration::minutes(1))?;
        }
        None
    }

    fn matches(&self, dt: &DateTime<Utc>) -> bool {
        if !self.minutes.contains(&dt.minute()) {
            return false;
        }
        if !self.hours.contains(&dt.hour()) {
            return false;
        }
        if !self.months.contains(&dt.month()) {
            return false;
        }

        let dom = dt.day();
        // chrono weekday: Mon=0..Sun=6 via num_days_from_monday; cron uses Sun=0..Sat=6.
        let dow = dt.weekday().num_days_from_sunday();

        let dom_match = self.days_of_month.contains(&dom);
        let dow_match = self.days_of_week.contains(&dow);

        // Vixie-cron OR semantics: if both DOM and DOW are restricted, a match in either suffices;
        // if only one is restricted, only that one is consulted; if neither, both pass.
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom_match || dow_match,
            (true, false) => dom_match,
            (false, true) => dow_match,
            (false, false) => true,
        }
    }
}

/// Parse a single cron field (`*`, lists, ranges, steps) into the sorted set of matching values
/// within `[min, max]`.
fn parse_field(field: &str, min: u32, max: u32) -> std::result::Result<Vec<u32>, ()> {
    let mut values: Vec<u32> = Vec::new();
    for part in field.split(',') {
        if part.is_empty() {
            return Err(());
        }
        parse_field_part(part, min, max, &mut values)?;
    }
    if values.is_empty() {
        return Err(());
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

fn parse_field_part(
    part: &str,
    min: u32,
    max: u32,
    out: &mut Vec<u32>,
) -> std::result::Result<(), ()> {
    // Split off an optional step (`.../n`).
    let (range_spec, step) = match part.split_once('/') {
        Some((range_spec, step_str)) => {
            let step: u32 = step_str.parse().map_err(|_| ())?;
            if step == 0 {
                return Err(());
            }
            (range_spec, Some(step))
        }
        None => (part, None),
    };

    // Determine the [start, end] bounds for this part.
    let (start, end) = if range_spec == "*" {
        (min, max)
    } else if let Some((lo, hi)) = range_spec.split_once('-') {
        let lo: u32 = lo.parse().map_err(|_| ())?;
        let hi: u32 = hi.parse().map_err(|_| ())?;
        (lo, hi)
    } else {
        let v: u32 = range_spec.parse().map_err(|_| ())?;
        match step {
            // A bare value with a step (`a/n`) ranges from the value to the field max.
            Some(_) => (v, max),
            None => (v, v),
        }
    };

    if start < min || end > max || start > end {
        return Err(());
    }

    let step = step.unwrap_or(1);
    let mut v = start;
    while v <= end {
        out.push(v);
        v += step;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Schedule a cron job. SYNC. Validates the schedule (errors `InvalidSchedule` / `PastSchedule`).
    /// `id` defaults to a UUID; `overlap` defaults to allow.
    ///
    /// Mirrors TS `AgentOs.scheduleCron` / `CronManager.schedule`: validation happens up front, the
    /// job is registered, and a self-driven timer loop is armed against the job's cancel token. The
    /// returned [`CronJobHandle`] cancels the job on [`CronJobHandle::cancel`].
    pub fn schedule_cron(
        &self,
        options: CronJobOptions,
    ) -> std::result::Result<CronJobHandle, ClientError> {
        let cron = self.cron();
        let now = cron.driver.now();

        // Validate before any state mutation, matching TS `validateScheduleForRegistration`.
        let next_run = validate_schedule(&options.schedule, now)?;

        let id = options.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let overlap = options.overlap.unwrap_or_default();
        let cancel = tokio_util::sync::CancellationToken::new();

        let state = CronJobState {
            schedule: options.schedule.clone(),
            action: options.action,
            overlap,
            last_run: parking_lot::Mutex::new(None),
            next_run: parking_lot::Mutex::new(next_run),
            run_count: std::sync::atomic::AtomicU64::new(0),
            running: AtomicBool::new(false),
            queued: AtomicBool::new(false),
            cancel: cancel.clone(),
        };

        // Insert; if the id already exists, replace it (cancelling the old timer first), mirroring
        // the TS `Map.set` overwrite behavior.
        if let Some((_, old)) = cron.jobs.remove(&id) {
            old.cancel.cancel();
        }
        let _ = cron.jobs.insert(id.clone(), state);

        // Arm the self-driven timer loop. It recomputes the next run before each sleep and fires
        // `execute_job` on each match, exiting when the cancel token is tripped or the schedule has
        // no further runs (a one-shot in the past or an exhausted expression).
        let manager = Arc::clone(cron);
        let vm = self.clone();
        let schedule = options.schedule;
        let loop_id = id.clone();
        let loop_cancel = cancel;
        tokio::spawn(async move {
            run_schedule_loop(manager, vm, loop_id, schedule, loop_cancel).await;
        });

        Ok(CronJobHandle {
            id,
            manager: Arc::clone(cron),
        })
    }

    /// Snapshot all cron jobs. Mirrors TS `CronManager.list`.
    pub fn list_cron_jobs(&self) -> Vec<CronJobInfo> {
        let mut result = Vec::new();
        self.cron().jobs.scan(|id, state| {
            result.push(CronJobInfo {
                id: id.clone(),
                schedule: state.schedule.clone(),
                action: state.action.clone(),
                overlap: state.overlap,
                last_run: *state.last_run.lock(),
                next_run: *state.next_run.lock(),
                run_count: state.run_count.load(Ordering::SeqCst),
                running: state.running.load(Ordering::SeqCst),
            });
        });
        result
    }

    /// Cancel a cron job. No-op if unknown; never errors. Mirrors TS `CronManager.cancel`.
    pub fn cancel_cron_job(&self, id: &str) {
        self.cron().cancel_job(id);
    }

    /// Subscribe to cron events. The TS API returns no unsubscribe; dropping the receiver is the
    /// equivalent. Each run emits `Fire` then `Complete`|`Error`. Mirrors TS `AgentOs.onCronEvent`.
    pub fn cron_events(&self) -> broadcast::Receiver<CronEvent> {
        self.cron().event_tx.subscribe()
    }
}

/// The per-job timer loop. Recomputes the next run, sleeps until then (or until cancelled), fires
/// the job, and repeats. Exits when the schedule has no further runs or the job is cancelled.
///
/// This is the Rust equivalent of arming a [`ScheduleDriver`] timer per fire: rather than storing a
/// driver [`crate::config::ScheduleHandle`] on the job (the scaffold reserves only a cancel token),
/// the loop owns its own cadence and observes the cancel token directly.
async fn run_schedule_loop(
    manager: Arc<CronManager>,
    vm: AgentOs,
    id: String,
    schedule: String,
    cancel: tokio_util::sync::CancellationToken,
) {
    loop {
        // Stop if the job was removed/cancelled.
        if cancel.is_cancelled() || !manager.jobs.contains(&id) {
            return;
        }

        let now = manager.driver.now();
        let next = match compute_next_time(&schedule, now) {
            Some(next) => next,
            None => return,
        };

        // Keep the snapshot fresh for `list_cron_jobs`.
        manager.jobs.read(&id, |_, state| {
            *state.next_run.lock() = Some(next);
        });

        let delay = (next - now).to_std().unwrap_or(std::time::Duration::ZERO);

        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }

        if cancel.is_cancelled() || !manager.jobs.contains(&id) {
            return;
        }

        execute_job(manager.clone(), vm.clone(), id.clone()).await;
    }
}
