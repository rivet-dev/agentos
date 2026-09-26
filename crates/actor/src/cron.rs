use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use rivetkit::{CronSetOptions, Ctx, Handles};
use serde::{Deserialize, Serialize};

use crate::actions::BoxFuture;
use crate::events::CronFiredEvent;
use crate::process::{validate_arguments, validate_command, ActorSpawnOptions};
use crate::{AgentOsActor, ProcessSpawn};

const PRIVATE_CRON_ACTION: &str = "__agentos.cron.invoke";
const MAX_CRON_JOBS: usize = 1_024;
const MAX_CRON_NAME_BYTES: usize = 256;
const MAX_CRON_EXPRESSION_BYTES: usize = 256;
const MAX_TIMEZONE_BYTES: usize = 128;
const MAX_CRON_ARGUMENT_BYTES: usize = 64 * 1024;
const DEFAULT_CRON_HISTORY: i64 = 32;
const MAX_CRON_HISTORY: i64 = 256;
const MAX_CRON_ERROR_BYTES: usize = 16 * 1024;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CronSchedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub command: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorSpawnOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_history: Option<i64>,
}

crate::register_action!(CronSchedule => ActorCronJob, "cron.schedule");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronList {}

crate::register_action!(CronList => Vec<ActorCronJob>, "cron.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronCancel {
    pub name: String,
}

crate::register_action!(CronCancel => bool, "cron.cancel");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCronJob {
    pub name: String,
    pub expression: String,
    pub timezone: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub options: ActorSpawnOptions,
    pub config_revision: u64,
    pub next_run_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub max_history: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronLaunchError {
    /// Uses the same stable error codes as public actor actions.
    pub code: String,
    pub message: String,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CronInvoke {
    pub schedule_name: String,
    /// Kept only in RivetKit's durable schedule payload, never in public job
    /// metadata. A raw actor action call cannot forge a scheduled invocation.
    pub invoke_token: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub options: ActorSpawnOptions,
    pub config_revision: u64,
}

crate::register_action!(CronInvoke => (), "__agentos.cron.invoke");

impl Handles<CronSchedule> for AgentOsActor {
    type Future = BoxFuture<ActorCronJob>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: CronSchedule) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            // Keep the capacity check and captured configuration revision
            // stable until the scheduler has durably registered the command.
            let _mutation = self.config_mutation.lock().await;
            let current = ctx.cron().list().await?;
            if current.len() >= MAX_CRON_JOBS {
                let replacing = action
                    .name
                    .as_ref()
                    .is_some_and(|name| current.iter().any(|job| &job.name == name));
                if !replacing {
                    bail!(
                        "limit_exceeded: actor has {} cron jobs; maximum is {MAX_CRON_JOBS}; cancel a job before scheduling another",
                        current.len()
                    );
                }
            }
            let name = action
                .name
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
            validate_nonempty_bytes("cron name", &name, MAX_CRON_NAME_BYTES)?;
            validate_nonempty_bytes(
                "cron expression",
                &action.expression,
                MAX_CRON_EXPRESSION_BYTES,
            )?;
            if let Some(timezone) = &action.timezone {
                validate_nonempty_bytes("cron timezone", timezone, MAX_TIMEZONE_BYTES)?;
            }
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            action.options.validate()?;
            let max_history = action.max_history.unwrap_or(DEFAULT_CRON_HISTORY);
            if !(0..=MAX_CRON_HISTORY).contains(&max_history) {
                bail!("limit_exceeded: cron maxHistory must be between 0 and {MAX_CRON_HISTORY}");
            }
            let config_revision = self.snapshot().await.revision;
            let invocation = CronInvoke {
                schedule_name: name.clone(),
                invoke_token: uuid::Uuid::new_v4().simple().to_string(),
                command: action.command,
                args: action.args,
                options: action.options,
                config_revision,
            };
            let args = rivetkit::action::encode_positional(&invocation)
                .context("encode private cron invocation")?;
            if args.len() > MAX_CRON_ARGUMENT_BYTES {
                bail!(
                    "limit_exceeded: encoded cron action is {} bytes; maximum is {MAX_CRON_ARGUMENT_BYTES}",
                    args.len()
                );
            }
            ctx.cron()
                .set(CronSetOptions {
                    name: &name,
                    expression: &action.expression,
                    timezone: action.timezone.as_deref(),
                    action: PRIVATE_CRON_ACTION,
                    args: &args,
                    max_history: Some(max_history),
                })
                .await
                .context("schedule RivetKit cron job")?;
            let info = ctx
                .cron()
                .get(&name)
                .await?
                .ok_or_else(|| anyhow!("scheduled cron job {name:?} was not found"))?;
            actor_cron_job(info)
        })
    }
}

impl Handles<CronList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorCronJob>>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, _action: CronList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let jobs = ctx.cron().list().await?;
            if jobs.len() > MAX_CRON_JOBS {
                bail!(
                    "limit_exceeded: RivetKit returned {} cron jobs; maximum is {MAX_CRON_JOBS}",
                    jobs.len()
                );
            }
            jobs.into_iter()
                .filter(|job| job.action == PRIVATE_CRON_ACTION)
                .map(actor_cron_job)
                .collect()
        })
    }
}

impl Handles<CronCancel> for AgentOsActor {
    type Future = BoxFuture<bool>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: CronCancel) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_nonempty_bytes("cron name", &action.name, MAX_CRON_NAME_BYTES)?;
            let _mutation = self.config_mutation.lock().await;
            ctx.cron().delete(&action.name).await
        })
    }
}

impl Handles<CronInvoke> for AgentOsActor {
    type Future = BoxFuture<()>;

    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: CronInvoke) -> Self::Future {
        Box::pin(async move {
            validate_nonempty_bytes(
                "cron schedule name",
                &action.schedule_name,
                MAX_CRON_NAME_BYTES,
            )?;
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            action.options.validate()?;
            let _mutation = self.config_mutation.lock().await;
            let scheduled = ctx.cron().get(&action.schedule_name).await?;
            if let Err(error) = validate_durable_invocation(&action, scheduled.as_ref()) {
                return Err(emit_cron_failure(&ctx, &action.schedule_name, error)?);
            }
            let current_revision = self.snapshot().await.revision;
            if action.config_revision != current_revision {
                let error = anyhow!(
                    "revision_conflict: cron job was validated at revision {} but current revision is {current_revision}",
                    action.config_revision
                );
                return Err(emit_cron_failure(&ctx, &action.schedule_name, error)?);
            }

            let schedule_name = action.schedule_name;
            let process = ProcessSpawn {
                command: action.command,
                args: action.args,
                options: action.options,
            };
            match <AgentOsActor as Handles<ProcessSpawn>>::handle(
                self.clone(),
                ctx.clone(),
                process,
            )
            .await
            {
                Ok(process) => {
                    ctx.emit(CronFiredEvent {
                        schedule_name,
                        process: Some(process),
                        error: None,
                        fired_at_ms: crate::runtime::now_ms()?,
                    })?;
                    Ok(())
                }
                Err(error) => Err(emit_cron_failure(&ctx, &schedule_name, error)?),
            }
        })
    }
}

fn validate_durable_invocation(
    invocation: &CronInvoke,
    scheduled: Option<&rivetkit::context::CronJobInfo>,
) -> Result<()> {
    let scheduled = scheduled.ok_or_else(|| {
        anyhow!(
            "not_found: cron job {:?} was cancelled before invocation",
            invocation.schedule_name
        )
    })?;
    if scheduled.name != invocation.schedule_name
        || scheduled.kind != rivetkit::ScheduleKind::Cron
        || scheduled.action != PRIVATE_CRON_ACTION
    {
        bail!(
            "revision_conflict: cron job {:?} no longer targets the agentOS command action",
            invocation.schedule_name
        );
    }
    let current: CronInvoke = rivetkit::action::decode_positional(&scheduled.args)
        .with_context(|| format!("decode cron job {:?} invocation", scheduled.name))?;
    if &current != invocation {
        bail!(
            "revision_conflict: cron job {:?} was replaced before invocation",
            invocation.schedule_name
        );
    }
    Ok(())
}

fn actor_cron_job(info: rivetkit::context::CronJobInfo) -> Result<ActorCronJob> {
    if info.action != PRIVATE_CRON_ACTION {
        bail!(
            "invalid_input: cron job {:?} is not an agentOS command job",
            info.name
        );
    }
    let invocation: CronInvoke = rivetkit::action::decode_positional(&info.args)
        .with_context(|| format!("decode cron job {:?} invocation", info.name))?;
    let expression = info
        .expression
        .ok_or_else(|| anyhow!("cron job {:?} has no expression", info.name))?;
    Ok(ActorCronJob {
        name: info.name,
        expression,
        timezone: info.timezone,
        command: invocation.command,
        args: invocation.args,
        options: invocation.options,
        config_revision: invocation.config_revision,
        next_run_at_ms: info.next_run_at,
        last_run_at_ms: info.last_run_at,
        max_history: info.max_history,
    })
}

fn emit_cron_failure(
    ctx: &Ctx<AgentOsActor>,
    schedule_name: &str,
    error: anyhow::Error,
) -> Result<anyhow::Error> {
    let error = crate::action_set::classify_public_error(error);
    ctx.emit(CronFiredEvent {
        schedule_name: schedule_name.to_owned(),
        process: None,
        error: Some(cron_launch_error(&error)),
        fired_at_ms: crate::runtime::now_ms()?,
    })?;
    Ok(error)
}

fn cron_launch_error(error: &anyhow::Error) -> CronLaunchError {
    let code = error
        .downcast_ref::<rivet_error::RivetError>()
        .and_then(|error| match &error.kind {
            rivet_error::RivetErrorKind::Dynamic { code, .. } => Some(code.as_str()),
            _ => None,
        })
        .unwrap_or("internal");
    CronLaunchError {
        code: bounded_error(code),
        message: bounded_error(&error.to_string()),
    }
}

fn bounded_error(error: &str) -> String {
    if error.len() <= MAX_CRON_ERROR_BYTES {
        return error.to_owned();
    }
    let mut end = MAX_CRON_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &error[..end])
}

fn validate_nonempty_bytes(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} cannot be empty");
    }
    if value.len() > max {
        bail!(
            "limit_exceeded: {label} is {} bytes; maximum is {max} bytes",
            value.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation() -> CronInvoke {
        CronInvoke {
            schedule_name: String::from("nightly"),
            invoke_token: String::from("0123456789abcdef0123456789abcdef"),
            command: String::from("echo"),
            args: vec![String::from("hello")],
            options: ActorSpawnOptions::default(),
            config_revision: 7,
        }
    }

    fn scheduled_job(invocation: &CronInvoke) -> rivetkit::context::CronJobInfo {
        rivetkit::context::CronJobInfo {
            name: invocation.schedule_name.clone(),
            kind: rivetkit::ScheduleKind::Cron,
            action: String::from(PRIVATE_CRON_ACTION),
            args: rivetkit::action::encode_positional(invocation).expect("encode cron payload"),
            next_run_at: 100,
            last_run_at: None,
            expression: Some(String::from("* * * * *")),
            timezone: None,
            interval: None,
            max_history: DEFAULT_CRON_HISTORY,
        }
    }

    #[test]
    fn private_cron_payload_round_trips_through_positional_cbor() {
        let action = invocation();
        let encoded = rivetkit::action::encode_positional(&action).expect("encode cron action");
        let decoded: CronInvoke =
            rivetkit::action::decode_positional(&encoded).expect("decode cron action");
        assert_eq!(decoded, action);
    }

    #[test]
    fn private_cron_invocation_requires_current_durable_job() {
        let action = invocation();
        let scheduled = scheduled_job(&action);
        validate_durable_invocation(&action, Some(&scheduled)).expect("current durable job");

        let missing = validate_durable_invocation(&action, None).unwrap_err();
        assert!(missing.to_string().starts_with("not_found:"));

        let mut forged = action.clone();
        forged.invoke_token = String::from("ffffffffffffffffffffffffffffffff");
        let mismatch = validate_durable_invocation(&forged, Some(&scheduled)).unwrap_err();
        assert!(mismatch.to_string().starts_with("revision_conflict:"));

        let mut replaced = scheduled_job(&action);
        replaced.args = rivetkit::action::encode_positional(&CronInvoke {
            command: String::from("sh"),
            ..action.clone()
        })
        .unwrap();
        let mismatch = validate_durable_invocation(&action, Some(&replaced)).unwrap_err();
        assert!(mismatch.to_string().starts_with("revision_conflict:"));

        let mut wrong_kind = scheduled_job(&action);
        wrong_kind.kind = rivetkit::ScheduleKind::Every;
        let mismatch = validate_durable_invocation(&action, Some(&wrong_kind)).unwrap_err();
        assert!(mismatch.to_string().starts_with("revision_conflict:"));
    }

    #[test]
    fn cron_errors_are_utf8_bounded() {
        let error = "🦀".repeat(MAX_CRON_ERROR_BYTES);
        let bounded = bounded_error(&error);
        assert!(bounded.len() <= MAX_CRON_ERROR_BYTES + '…'.len_utf8());
        assert!(bounded.ends_with('…'));
    }

    #[test]
    fn cron_launch_errors_preserve_public_error_codes() {
        let error = crate::action_set::classify_public_error(anyhow!(
            "revision_conflict: scheduled configuration is no longer current"
        ));
        let event = CronFiredEvent {
            schedule_name: "nightly".into(),
            process: None,
            error: Some(cron_launch_error(&error)),
            fired_at_ms: 123,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["error"]["code"], "revision_conflict");
        assert_eq!(value["firedAtMs"], 123);
        assert!(value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("scheduled configuration"));
    }
}
