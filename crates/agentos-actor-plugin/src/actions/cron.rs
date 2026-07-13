//! Cron actions. The client's `CronJobOptions` / `CronAction` /
//! `CronJobInfo` are not serde types (they carry closures), so we define
//! serde DTOs here and map to/from the client types.

use crate::host_ctx::HostCtx;
use agentos_client::{AgentOs, CronAction, CronEvent, CronJobOptions, CronOverlap};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::Vars;

pub(crate) const INTERNAL_CRON_WAKE_ACTION: &str = "__agentos_cron_wake";

/// `{ type: "exec", command, args }` | `{ type: "session", agentType, prompt }`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CronActionDto {
    Exec {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Session {
        agent_type: String,
        prompt: String,
    },
}

/// Options object for `scheduleCron(...)`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobOptionsDto {
    #[serde(default)]
    pub id: Option<String>,
    pub schedule: String,
    pub action: CronActionDto,
    #[serde(default)]
    pub overlap: Option<CronOverlap>,
}

/// `{ id }` returned by `scheduleCron`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledCronDto {
    pub id: String,
}

/// One entry returned by `listCronJobs`. `last_run` / `next_run` are
/// epoch-millis timestamps serialized as `f64` so they cross the napi
/// boundary as JS `number`s (not `BigInt`s), matching the core API.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobInfoDto {
    pub id: String,
    pub schedule: String,
    pub overlap: CronOverlap,
    pub last_run: Option<f64>,
    pub next_run: Option<f64>,
}

fn to_action(dto: CronActionDto) -> CronAction {
    match dto {
        CronActionDto::Exec { command, args } => CronAction::Exec { command, args },
        CronActionDto::Session { agent_type, prompt } => CronAction::Session {
            agent_type,
            prompt,
            options: None,
        },
    }
}

pub(crate) fn cron_event_payload(event: &CronEvent) -> JsonValue {
    match event {
        CronEvent::Fire { job_id, time } => json!({
            "event": {
                "type": "cron:fire",
                "jobId": job_id,
                "time": time.timestamp_millis(),
            },
        }),
        CronEvent::Complete {
            job_id,
            time,
            duration_ms,
        } => json!({
            "event": {
                "type": "cron:complete",
                "jobId": job_id,
                "time": time.timestamp_millis(),
                "durationMs": duration_ms,
            },
        }),
        CronEvent::Error {
            job_id,
            time,
            error,
        } => json!({
            "event": {
                "type": "cron:error",
                "jobId": job_id,
                "time": time.timestamp_millis(),
                "error": error,
            },
        }),
    }
}

pub(crate) fn encode_cron_event(event: &CronEvent) -> Result<Vec<u8>> {
    super::encode_event_arg(&cron_event_payload(event))
}

pub(crate) fn ensure_cron_event_pump(host: &HostCtx, vm: &AgentOs, vars: &mut Vars) {
    if vars.cron_task.is_some() {
        return;
    }
    let host = host.clone();
    let cron_vm = vm.clone();
    let mut rx = vm.cron_events();
    vars.cron_task = Some(tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => match encode_cron_event(&event) {
                    Ok(bytes) => {
                        let _ = host.broadcast(b"cronEvent".to_vec(), bytes);
                        if let Err(error) = crate::vm::persist_cron_state(&host, &cron_vm).await {
                            host.log_warn(&format!(
                                "failed to persist cron state after lifecycle event: {error}"
                            ));
                        }
                    }
                    Err(error) => {
                        tracing::warn!(?error, "failed to encode cron event broadcast");
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));
}

pub async fn schedule_cron(
    host: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    dto: CronJobOptionsDto,
) -> Result<ScheduledCronDto> {
    ensure_cron_event_pump(host, vm, vars);
    let options = CronJobOptions {
        id: dto.id,
        schedule: dto.schedule,
        action: to_action(dto.action),
        overlap: dto.overlap,
    };
    let handle = vm.schedule_cron(options).await.map_err(|e| anyhow!(e))?;
    crate::vm::persist_cron_state(host, vm)
        .await
        .map_err(|error| anyhow!(error))?;
    Ok(ScheduledCronDto { id: handle.id })
}

pub async fn list_cron_jobs(vm: &AgentOs) -> Result<Vec<CronJobInfoDto>> {
    Ok(vm
        .list_cron_jobs()
        .await
        .map_err(|error| anyhow!(error))?
        .into_iter()
        .map(|info| CronJobInfoDto {
            id: info.id,
            schedule: info.schedule,
            overlap: info.overlap,
            last_run: info.last_run.map(|t| t.timestamp_millis() as f64),
            next_run: info.next_run.map(|t| t.timestamp_millis() as f64),
        })
        .collect())
}

pub async fn cancel_cron_job(host: &HostCtx, vm: &AgentOs, id: &str) -> Result<()> {
    vm.cancel_cron_job(id)
        .await
        .map_err(|error| anyhow!(error))?;
    crate::vm::persist_cron_state(host, vm)
        .await
        .map_err(|error| anyhow!(error))
}
