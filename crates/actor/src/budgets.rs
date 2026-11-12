//! Immutable hosted-worker policy. VM limits remain independently authoritative.
use agentos_actor_contract::{warn_near_budget, ActionBudgetViolation, ActionCodecLimits};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedActorBudgetOptions {
    #[serde(default)]
    pub admission: HostedAdmissionBudgetOptions,
    #[serde(default)]
    pub transport: HostedTransportBudgetOptions,
    #[serde(default)]
    pub codec: HostedCodecBudgetOptions,
    #[serde(default)]
    pub filesystem: HostedFilesystemBudgetOptions,
    #[serde(default)]
    pub config: HostedConfigBudgetOptions,
    #[serde(default)]
    pub paths: HostedPathBudgetOptions,
    #[serde(default)]
    pub retention: HostedRetentionBudgetOptions,
    #[serde(default)]
    pub cron: HostedCronBudgetOptions,
    #[serde(default)]
    pub preview: HostedPreviewBudgetOptions,
    #[serde(default)]
    pub network: HostedNetworkBudgetOptions,
    #[serde(default)]
    pub process: HostedProcessBudgetOptions,
    #[serde(default)]
    pub language: HostedLanguageBudgetOptions,
    #[serde(default)]
    pub execution: HostedExecutionBudgetOptions,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedAdmissionBudgetOptions {
    pub max_concurrent_actions: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedTransportBudgetOptions {
    pub max_message_bytes: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedCodecBudgetOptions {
    pub max_input_depth: Option<u64>,
    pub max_input_values: Option<u64>,
    pub max_binary_bytes: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedFilesystemBudgetOptions {
    pub max_batch_paths: Option<u64>,
    pub max_directory_entries: Option<u64>,
    pub max_directory_result_bytes: Option<u64>,
    pub max_recursive_depth: Option<u64>,

    pub max_transfer_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedConfigBudgetOptions {
    pub max_environment_entries: Option<u64>,
    pub max_environment_key_bytes: Option<u64>,
    pub max_environment_value_bytes: Option<u64>,
    pub max_environment_bytes: Option<u64>,
    pub max_permission_rules_per_domain: Option<u64>,
    pub max_permission_values_per_rule: Option<u64>,
    pub max_permission_value_bytes: Option<u64>,
    pub max_permission_config_bytes: Option<u64>,
    pub max_hosted_mounts: Option<u64>,
    pub max_filesystem_namespace_bytes: Option<u64>,
    pub max_remote_software: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedPathBudgetOptions {
    pub max_path_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigValidationBudgets {
    pub(crate) max_environment_entries: usize,
    pub(crate) max_environment_key_bytes: usize,
    pub(crate) max_environment_value_bytes: usize,
    pub(crate) max_environment_bytes: usize,
    pub(crate) max_permission_rules_per_domain: usize,
    pub(crate) max_permission_values_per_rule: usize,
    pub(crate) max_permission_value_bytes: usize,
    pub(crate) max_permission_config_bytes: usize,
    pub(crate) max_hosted_mounts: usize,
    pub(crate) max_filesystem_namespace_bytes: usize,
    pub(crate) max_remote_software: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedActorBudgets {
    pub(crate) max_concurrent_actions: usize,
    pub(crate) max_message_bytes: u32,
    pub(crate) codec: ActionCodecLimits,
    pub(crate) config: ConfigValidationBudgets,
    pub(crate) retention: RetentionValidationBudgets,
    pub(crate) cron: CronValidationBudgets,
    pub(crate) preview: PreviewValidationBudgets,
    pub(crate) network: NetworkValidationBudgets,
    pub(crate) filesystem: FilesystemValidationBudgets,
    pub(crate) process: ProcessValidationBudgets,
    pub(crate) language: LanguageValidationBudgets,
    pub(crate) execution: ExecutionValidationBudgets,
    pub(crate) max_path_bytes: usize,
    pub(crate) max_transfer_bytes: usize,
}
fn positive(setting: &'static str, value: Option<u64>, default: usize) -> Result<usize> {
    let value = match value {
        None => default,
        Some(value) => usize::try_from(value).map_err(|_| {
            anyhow::anyhow!("invalid_input: --actor-budgets-json {setting} exceeds platform usize")
        })?,
    };
    if value == 0 {
        bail!("invalid_input: --actor-budgets-json {setting} must be positive");
    }
    Ok(value)
}
impl HostedActorBudgets {
    pub fn normalize(options: HostedActorBudgetOptions) -> Result<Self> {
        let max_concurrent_actions = positive(
            "admission.maxConcurrentActions",
            options.admission.max_concurrent_actions,
            crate::ACTION_CONCURRENCY_LIMIT,
        )?;
        if max_concurrent_actions > tokio::sync::Semaphore::MAX_PERMITS {
            bail!("invalid_input: --actor-budgets-json admission.maxConcurrentActions exceeds Tokio semaphore MAX_PERMITS ({})", tokio::sync::Semaphore::MAX_PERMITS);
        }
        let message = positive(
            "transport.maxMessageBytes",
            options.transport.max_message_bytes,
            crate::ACTOR_MESSAGE_SIZE_LIMIT as usize,
        )?;
        let max_message_bytes = u32::try_from(message).map_err(|_| anyhow::anyhow!(
            "invalid_input: --actor-budgets-json transport.maxMessageBytes exceeds RivetKit u32 message representation"))?;
        let codec = ActionCodecLimits::for_hosted_actor(
            message,
            positive("codec.maxInputDepth", options.codec.max_input_depth, 64)?,
            positive(
                "codec.maxInputValues",
                options.codec.max_input_values,
                128 * 1024,
            )?,
            positive(
                "codec.maxBinaryBytes",
                options.codec.max_binary_bytes,
                768 * 1024,
            )?,
        )?;
        let budgets = Self {
            execution: ExecutionValidationBudgets {
                max_command_bytes: ValidationBound {
                    setting: "actor.execution.maxCommandBytes",
                    maximum: positive(
                        "execution.maxCommandBytes",
                        options.execution.max_command_bytes,
                        16384,
                    )?,
                    units: "bytes",
                },
                max_arguments: ValidationBound {
                    setting: "actor.execution.maxArguments",
                    maximum: positive(
                        "execution.maxArguments",
                        options.execution.max_arguments,
                        1024,
                    )?,
                    units: "arguments",
                },
                max_argument_bytes: ValidationBound {
                    setting: "actor.execution.maxArgumentBytes",
                    maximum: positive(
                        "execution.maxArgumentBytes",
                        options.execution.max_argument_bytes,
                        16384,
                    )?,
                    units: "bytes",
                },
                max_environment_entries: ValidationBound {
                    setting: "actor.execution.maxEnvironmentEntries",
                    maximum: positive(
                        "execution.maxEnvironmentEntries",
                        options.execution.max_environment_entries,
                        1024,
                    )?,
                    units: "entries",
                },
                max_environment_bytes: ValidationBound {
                    setting: "actor.execution.maxEnvironmentBytes",
                    maximum: positive(
                        "execution.maxEnvironmentBytes",
                        options.execution.max_environment_bytes,
                        262144,
                    )?,
                    units: "bytes",
                },
                max_stdin_bytes: ValidationBound {
                    setting: "actor.execution.maxStdinBytes",
                    maximum: positive(
                        "execution.maxStdinBytes",
                        options.execution.max_stdin_bytes,
                        262144,
                    )?,
                    units: "bytes",
                },
                max_terminal_dimension: ValidationBound {
                    setting: "actor.execution.maxTerminalDimension",
                    maximum: positive(
                        "execution.maxTerminalDimension",
                        options.execution.max_terminal_dimension,
                        4096,
                    )?,
                    units: "cells",
                },
            },
            language: LanguageValidationBudgets {
                max_context_id_bytes: ValidationBound {
                    setting: "actor.language.maxContextIdBytes",
                    maximum: positive(
                        "language.maxContextIdBytes",
                        options.language.max_context_id_bytes,
                        256,
                    )?,
                    units: "bytes",
                },
                max_contexts: ValidationBound {
                    setting: "actor.language.maxContexts",
                    maximum: positive("language.maxContexts", options.language.max_contexts, 1024)?,
                    units: "contexts",
                },
                max_source_bytes: ValidationBound {
                    setting: "actor.language.maxSourceBytes",
                    maximum: positive(
                        "language.maxSourceBytes",
                        options.language.max_source_bytes,
                        524288,
                    )?,
                    units: "bytes",
                },
                max_json_bytes: ValidationBound {
                    setting: "actor.language.maxJsonBytes",
                    maximum: positive(
                        "language.maxJsonBytes",
                        options.language.max_json_bytes,
                        262144,
                    )?,
                    units: "bytes",
                },
                max_packages: ValidationBound {
                    setting: "actor.language.maxPackages",
                    maximum: positive("language.maxPackages", options.language.max_packages, 256)?,
                    units: "entries",
                },
                max_index_urls: ValidationBound {
                    setting: "actor.language.maxIndexUrls",
                    maximum: positive(
                        "language.maxIndexUrls",
                        options.language.max_index_urls,
                        16,
                    )?,
                    units: "URLs",
                },
                max_timeout_ms: ValidationBound {
                    setting: "actor.language.maxTimeoutMs",
                    maximum: positive(
                        "language.maxTimeoutMs",
                        options.language.max_timeout_ms,
                        300000,
                    )?,
                    units: "milliseconds",
                },
                max_result_bytes: ValidationBound {
                    setting: "actor.language.maxResultBytes",
                    maximum: positive(
                        "language.maxResultBytes",
                        options.language.max_result_bytes,
                        786432,
                    )?,
                    units: "bytes",
                },
                max_diagnostics: ValidationBound {
                    setting: "actor.language.maxDiagnostics",
                    maximum: positive(
                        "language.maxDiagnostics",
                        options.language.max_diagnostics,
                        1024,
                    )?,
                    units: "diagnostics",
                },
                max_diagnostic_bytes: ValidationBound {
                    setting: "actor.language.maxDiagnosticBytes",
                    maximum: positive(
                        "language.maxDiagnosticBytes",
                        options.language.max_diagnostic_bytes,
                        524288,
                    )?,
                    units: "bytes",
                },
            },
            process: ProcessValidationBudgets {
                max_exec_output_bytes: ValidationBound {
                    setting: "actor.process.maxExecOutputBytes",
                    maximum: positive(
                        "process.maxExecOutputBytes",
                        options.process.max_exec_output_bytes,
                        786432,
                    )?,
                    units: "bytes",
                },
                max_process_list_entries: ValidationBound {
                    setting: "actor.process.maxProcessListEntries",
                    maximum: positive(
                        "process.maxProcessListEntries",
                        options.process.max_process_list_entries,
                        4096,
                    )?,
                    units: "entries",
                },
                max_wait_ms: ValidationBound {
                    setting: "actor.process.maxWaitMs",
                    maximum: positive("process.maxWaitMs", options.process.max_wait_ms, 300000)?,
                    units: "milliseconds",
                },
            },
            filesystem: FilesystemValidationBudgets {
                max_batch_paths: ValidationBound {
                    setting: "actor.filesystem.maxBatchPaths",
                    maximum: positive(
                        "filesystem.maxBatchPaths",
                        options.filesystem.max_batch_paths,
                        128,
                    )?,
                    units: "paths",
                },
                max_directory_entries: ValidationBound {
                    setting: "actor.filesystem.maxDirectoryEntries",
                    maximum: positive(
                        "filesystem.maxDirectoryEntries",
                        options.filesystem.max_directory_entries,
                        4096,
                    )?,
                    units: "entries",
                },
                max_directory_result_bytes: ValidationBound {
                    setting: "actor.filesystem.maxDirectoryResultBytes",
                    maximum: positive(
                        "filesystem.maxDirectoryResultBytes",
                        options.filesystem.max_directory_result_bytes,
                        524288,
                    )?,
                    units: "bytes",
                },
                max_recursive_depth: ValidationBound {
                    setting: "actor.filesystem.maxRecursiveDepth",
                    maximum: usize::try_from(options.filesystem.max_recursive_depth.unwrap_or(64)).map_err(|_| anyhow::anyhow!("invalid_input: --actor-budgets-json filesystem.maxRecursiveDepth exceeds usize"))?,
                    units: "levels",
                },
            },
            network: NetworkValidationBudgets {
                max_http_path_bytes: ValidationBound {
                    setting: "actor.network.maxHttpPathBytes",
                    maximum: positive(
                        "network.maxHttpPathBytes",
                        options.network.max_http_path_bytes,
                        16384,
                    )?,
                    units: "bytes",
                },
                max_http_method_bytes: ValidationBound {
                    setting: "actor.network.maxHttpMethodBytes",
                    maximum: positive(
                        "network.maxHttpMethodBytes",
                        options.network.max_http_method_bytes,
                        32,
                    )?,
                    units: "bytes",
                },
                max_http_headers: ValidationBound {
                    setting: "actor.network.maxHttpHeaders",
                    maximum: positive(
                        "network.maxHttpHeaders",
                        options.network.max_http_headers,
                        128,
                    )?,
                    units: "headers",
                },
                max_http_header_bytes: ValidationBound {
                    setting: "actor.network.maxHttpHeaderBytes",
                    maximum: positive(
                        "network.maxHttpHeaderBytes",
                        options.network.max_http_header_bytes,
                        65536,
                    )?,
                    units: "bytes",
                },
                max_http_body_bytes: ValidationBound {
                    setting: "actor.network.maxHttpBodyBytes",
                    maximum: positive(
                        "network.maxHttpBodyBytes",
                        options.network.max_http_body_bytes,
                        524288,
                    )?,
                    units: "bytes",
                },
                max_http_response_bytes: ValidationBound {
                    setting: "actor.network.maxHttpResponseBytes",
                    maximum: positive(
                        "network.maxHttpResponseBytes",
                        options.network.max_http_response_bytes,
                        786432,
                    )?,
                    units: "bytes",
                },
                max_stream_chunk_bytes: ValidationBound {
                    setting: "actor.network.maxStreamChunkBytes",
                    maximum: positive(
                        "network.maxStreamChunkBytes",
                        options.network.max_stream_chunk_bytes,
                        131072,
                    )?,
                    units: "bytes",
                },
                max_stream_id_bytes: ValidationBound {
                    setting: "actor.network.maxStreamIdBytes",
                    maximum: positive(
                        "network.maxStreamIdBytes",
                        options.network.max_stream_id_bytes,
                        256,
                    )?,
                    units: "bytes",
                },
                max_stream_lifetime_ms: ValidationBound {
                    setting: "actor.network.maxStreamLifetimeMs",
                    maximum: positive(
                        "network.maxStreamLifetimeMs",
                        options.network.max_stream_lifetime_ms,
                        3600000,
                    )?,
                    units: "milliseconds",
                },
                max_preview_token_bytes: ValidationBound {
                    setting: "actor.network.maxPreviewTokenBytes",
                    maximum: positive(
                        "network.maxPreviewTokenBytes",
                        options.network.max_preview_token_bytes,
                        128,
                    )?,
                    units: "bytes",
                },
            },
            preview: PreviewValidationBudgets {
                max_ttl_ms: ValidationBound {
                    setting: "actor.preview.maxTtlMs",
                    maximum: positive("preview.maxTtlMs", options.preview.max_ttl_ms, 86400000)?,
                    units: "milliseconds",
                },
                max_active: ValidationBound {
                    setting: "actor.preview.maxActive",
                    maximum: positive("preview.maxActive", options.preview.max_active, 128)?,
                    units: "previews",
                },
            },
            cron: CronValidationBudgets {
                max_jobs: ValidationBound {
                    setting: "actor.cron.maxJobs",
                    maximum: positive("cron.maxJobs", options.cron.max_jobs, 1024)?,
                    units: "jobs",
                },
                max_name_bytes: ValidationBound {
                    setting: "actor.cron.maxNameBytes",
                    maximum: positive("cron.maxNameBytes", options.cron.max_name_bytes, 256)?,
                    units: "bytes",
                },
                max_expression_bytes: ValidationBound {
                    setting: "actor.cron.maxExpressionBytes",
                    maximum: positive(
                        "cron.maxExpressionBytes",
                        options.cron.max_expression_bytes,
                        256,
                    )?,
                    units: "bytes",
                },
                max_timezone_bytes: ValidationBound {
                    setting: "actor.cron.maxTimezoneBytes",
                    maximum: positive(
                        "cron.maxTimezoneBytes",
                        options.cron.max_timezone_bytes,
                        128,
                    )?,
                    units: "bytes",
                },
                max_argument_bytes: ValidationBound {
                    setting: "actor.cron.maxArgumentBytes",
                    maximum: positive(
                        "cron.maxArgumentBytes",
                        options.cron.max_argument_bytes,
                        65536,
                    )?,
                    units: "bytes",
                },
                max_history: ValidationBound {
                    setting: "actor.cron.maxHistory",
                    maximum: usize::try_from(options.cron.max_history.unwrap_or(256)).map_err(
                        |_| {
                            anyhow::anyhow!(
                                "invalid_input: --actor-budgets-json cron.maxHistory exceeds usize"
                            )
                        },
                    )?,
                    units: "runs",
                },
                max_error_bytes: ValidationBound {
                    setting: "actor.cron.maxErrorBytes",
                    maximum: positive("cron.maxErrorBytes", options.cron.max_error_bytes, 16384)?,
                    units: "bytes",
                },
            },
            retention: RetentionValidationBudgets {
                max_runtime_issues: ValidationBound {
                    setting: "actor.retention.maxRuntimeIssues",
                    maximum: positive(
                        "retention.maxRuntimeIssues",
                        options.retention.max_runtime_issues,
                        32,
                    )?,
                    units: "issues",
                },
            },
            max_concurrent_actions,
            max_message_bytes,
            codec,
            config: ConfigValidationBudgets {
                max_environment_entries: positive(
                    "config.maxEnvironmentEntries",
                    options.config.max_environment_entries,
                    256,
                )?,
                max_environment_key_bytes: positive(
                    "config.maxEnvironmentKeyBytes",
                    options.config.max_environment_key_bytes,
                    256,
                )?,
                max_environment_value_bytes: positive(
                    "config.maxEnvironmentValueBytes",
                    options.config.max_environment_value_bytes,
                    8192,
                )?,
                max_environment_bytes: positive(
                    "config.maxEnvironmentBytes",
                    options.config.max_environment_bytes,
                    65536,
                )?,
                max_permission_rules_per_domain: positive(
                    "config.maxPermissionRulesPerDomain",
                    options.config.max_permission_rules_per_domain,
                    256,
                )?,
                max_permission_values_per_rule: positive(
                    "config.maxPermissionValuesPerRule",
                    options.config.max_permission_values_per_rule,
                    128,
                )?,
                max_permission_value_bytes: positive(
                    "config.maxPermissionValueBytes",
                    options.config.max_permission_value_bytes,
                    4096,
                )?,
                max_permission_config_bytes: positive(
                    "config.maxPermissionConfigBytes",
                    options.config.max_permission_config_bytes,
                    262144,
                )?,
                max_hosted_mounts: positive(
                    "config.maxHostedMounts",
                    options.config.max_hosted_mounts,
                    32,
                )?,
                max_filesystem_namespace_bytes: positive(
                    "config.maxFilesystemNamespaceBytes",
                    options.config.max_filesystem_namespace_bytes,
                    128,
                )?,
                max_remote_software: positive(
                    "config.maxRemoteSoftware",
                    options.config.max_remote_software,
                    128,
                )?,
            },
            max_path_bytes: positive("paths.maxPathBytes", options.paths.max_path_bytes, 4096)?,
            max_transfer_bytes: positive(
                "filesystem.maxTransferBytes",
                options.filesystem.max_transfer_bytes,
                768 * 1024,
            )?,
        };
        if budgets.execution.max_terminal_dimension.maximum > u16::MAX as usize {
            bail!("invalid_input: --actor-budgets-json execution.maxTerminalDimension exceeds u16 representation");
        }
        for bound in [budgets.language.max_timeout_ms, budgets.process.max_wait_ms] {
            if bound.maximum as u128 > 9_007_199_254_740_991u128 {
                bail!("invalid_input: --actor-budgets-json {} exceeds exact f64 millisecond representation", bound.setting);
            }
            if std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(bound.maximum as u64))
                .is_none()
            {
                bail!(
                    "invalid_input: --actor-budgets-json {} exceeds host deadline representation",
                    bound.setting
                );
            }
        }
        for bound in [
            budgets.filesystem.max_recursive_depth,
            budgets.network.max_stream_chunk_bytes,
            budgets.preview.max_active,
        ] {
            if bound.maximum > u32::MAX as usize {
                bail!(
                    "invalid_input: --actor-budgets-json {} exceeds u32 representation",
                    bound.setting
                );
            }
        }
        for bound in [
            budgets.network.max_stream_lifetime_ms,
            budgets.preview.max_ttl_ms,
            budgets.cron.max_history,
        ] {
            if bound.maximum as u128 > i64::MAX as u128 {
                bail!(
                    "invalid_input: --actor-budgets-json {} exceeds i64 representation",
                    bound.setting
                );
            }
        }
        if budgets.preview.max_ttl_ms.maximum < crate::config::MIN_PREVIEW_TTL_MS as usize {
            bail!("invalid_input: --actor-budgets-json preview.maxTtlMs is below the minimum preview TTL");
        }
        Ok(budgets)
    }
    pub(crate) fn transfer_limit(&self, requested: Option<usize>) -> Result<usize> {
        let limit = requested.unwrap_or(self.max_transfer_bytes);
        if limit == 0 || limit > self.max_transfer_bytes {
            return Err(ActionBudgetViolation {
                setting: if limit == 0 {
                    "maxBytes"
                } else {
                    "actor.filesystem.maxTransferBytes"
                },
                observed: limit,
                limit: self.max_transfer_bytes,
                units: "bytes",
                reason: "maxBytes must be positive and must not exceed the worker transfer ceiling"
                    .into(),
            }
            .into());
        }
        Ok(limit)
    }
    pub(crate) fn check_transfer(
        &self,
        label: &'static str,
        observed: usize,
        limit: usize,
    ) -> Result<()> {
        let limit = limit.min(self.max_transfer_bytes);
        let setting = if limit < self.max_transfer_bytes {
            "maxBytes"
        } else {
            "actor.filesystem.maxTransferBytes"
        };
        if observed > limit {
            return Err(ActionBudgetViolation {
                setting, observed, limit, units: "bytes",
                reason: format!("{label} exceeds the selected maxBytes/worker transfer ceiling; encoded replies must also fit transport.maxMessageBytes"),
            }.into());
        }
        warn_near_budget(setting, observed, limit, "bytes");
        Ok(())
    }
}
static PROCESS_BUDGETS: OnceLock<Arc<HostedActorBudgets>> = OnceLock::new();

pub fn configure_process_actor_budgets(options: HostedActorBudgetOptions) -> Result<()> {
    let budgets = Arc::new(HostedActorBudgets::normalize(options)?);
    match PROCESS_BUDGETS.set(Arc::clone(&budgets)) {
        Ok(()) => Ok(()),
        Err(_) if PROCESS_BUDGETS.get().is_some_and(|existing| **existing == *budgets) => Ok(()),
        Err(_) => bail!("invalid_input: hosted actor budgets are already frozen; configure --actor-budgets-json before registry()/Actor::create, then restart this worker to change policy"),
    }
}
pub(crate) fn process_actor_budgets() -> Arc<HostedActorBudgets> {
    Arc::clone(PROCESS_BUDGETS.get_or_init(|| {
        Arc::new(
            HostedActorBudgets::normalize(HostedActorBudgetOptions::default())
                .expect("built-in hosted actor defaults are valid"),
        )
    }))
}

pub(crate) fn check_budget(
    setting: &'static str,
    observed: usize,
    limit: usize,
    units: &'static str,
    label: &str,
) -> Result<()> {
    if observed > limit {
        return Err(ActionBudgetViolation {
            setting,
            observed,
            limit,
            units,
            reason: format!("{label} exceeds the worker validation budget"),
        }
        .into());
    }
    warn_near_budget(setting, observed, limit, units);
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedExecutionBudgetOptions {
    pub max_command_bytes: Option<u64>,
    pub max_arguments: Option<u64>,
    pub max_argument_bytes: Option<u64>,
    pub max_environment_entries: Option<u64>,
    pub max_environment_bytes: Option<u64>,
    pub max_stdin_bytes: Option<u64>,
    pub max_terminal_dimension: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionValidationBudgets {
    pub(crate) max_command_bytes: ValidationBound,
    pub(crate) max_arguments: ValidationBound,
    pub(crate) max_argument_bytes: ValidationBound,
    pub(crate) max_environment_entries: ValidationBound,
    pub(crate) max_environment_bytes: ValidationBound,
    pub(crate) max_stdin_bytes: ValidationBound,
    pub(crate) max_terminal_dimension: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedLanguageBudgetOptions {
    pub max_context_id_bytes: Option<u64>,
    pub max_contexts: Option<u64>,
    pub max_source_bytes: Option<u64>,
    pub max_json_bytes: Option<u64>,
    pub max_packages: Option<u64>,
    pub max_index_urls: Option<u64>,
    pub max_timeout_ms: Option<u64>,
    pub max_result_bytes: Option<u64>,
    pub max_diagnostics: Option<u64>,
    pub max_diagnostic_bytes: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageValidationBudgets {
    pub(crate) max_context_id_bytes: ValidationBound,
    pub(crate) max_contexts: ValidationBound,
    pub(crate) max_source_bytes: ValidationBound,
    pub(crate) max_json_bytes: ValidationBound,
    pub(crate) max_packages: ValidationBound,
    pub(crate) max_index_urls: ValidationBound,
    pub(crate) max_timeout_ms: ValidationBound,
    pub(crate) max_result_bytes: ValidationBound,
    pub(crate) max_diagnostics: ValidationBound,
    pub(crate) max_diagnostic_bytes: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedProcessBudgetOptions {
    pub max_exec_output_bytes: Option<u64>,
    pub max_process_list_entries: Option<u64>,
    pub max_wait_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessValidationBudgets {
    pub(crate) max_exec_output_bytes: ValidationBound,
    pub(crate) max_process_list_entries: ValidationBound,
    pub(crate) max_wait_ms: ValidationBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidationBound {
    pub(crate) setting: &'static str,
    pub(crate) maximum: usize,
    pub(crate) units: &'static str,
}
impl ValidationBound {
    pub(crate) fn check(self, label: &str, observed: usize) -> Result<()> {
        check_budget(self.setting, observed, self.maximum, self.units, label)
    }
}
impl HostedActorBudgets {
    pub(crate) fn path_bound(&self) -> ValidationBound {
        ValidationBound {
            setting: "actor.paths.maxPathBytes",
            maximum: self.max_path_bytes,
            units: "bytes",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemValidationBudgets {
    pub(crate) max_batch_paths: ValidationBound,
    pub(crate) max_directory_entries: ValidationBound,
    pub(crate) max_directory_result_bytes: ValidationBound,
    pub(crate) max_recursive_depth: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedNetworkBudgetOptions {
    pub max_http_path_bytes: Option<u64>,
    pub max_http_method_bytes: Option<u64>,
    pub max_http_headers: Option<u64>,
    pub max_http_header_bytes: Option<u64>,
    pub max_http_body_bytes: Option<u64>,
    pub max_http_response_bytes: Option<u64>,
    pub max_stream_chunk_bytes: Option<u64>,
    pub max_stream_id_bytes: Option<u64>,
    pub max_stream_lifetime_ms: Option<u64>,
    pub max_preview_token_bytes: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetworkValidationBudgets {
    pub(crate) max_http_path_bytes: ValidationBound,
    pub(crate) max_http_method_bytes: ValidationBound,
    pub(crate) max_http_headers: ValidationBound,
    pub(crate) max_http_header_bytes: ValidationBound,
    pub(crate) max_http_body_bytes: ValidationBound,
    pub(crate) max_http_response_bytes: ValidationBound,
    pub(crate) max_stream_chunk_bytes: ValidationBound,
    pub(crate) max_stream_id_bytes: ValidationBound,
    pub(crate) max_stream_lifetime_ms: ValidationBound,
    pub(crate) max_preview_token_bytes: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedPreviewBudgetOptions {
    pub max_ttl_ms: Option<u64>,
    pub max_active: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewValidationBudgets {
    pub(crate) max_ttl_ms: ValidationBound,
    pub(crate) max_active: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedCronBudgetOptions {
    pub max_jobs: Option<u64>,
    pub max_name_bytes: Option<u64>,
    pub max_expression_bytes: Option<u64>,
    pub max_timezone_bytes: Option<u64>,
    pub max_argument_bytes: Option<u64>,
    pub max_history: Option<u64>,
    pub max_error_bytes: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CronValidationBudgets {
    pub(crate) max_jobs: ValidationBound,
    pub(crate) max_name_bytes: ValidationBound,
    pub(crate) max_expression_bytes: ValidationBound,
    pub(crate) max_timezone_bytes: ValidationBound,
    pub(crate) max_argument_bytes: ValidationBound,
    pub(crate) max_history: ValidationBound,
    pub(crate) max_error_bytes: ValidationBound,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedRetentionBudgetOptions {
    pub max_runtime_issues: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetentionValidationBudgets {
    pub(crate) max_runtime_issues: ValidationBound,
}
