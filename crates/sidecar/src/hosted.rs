// Hosted composition is compiled only with the explicit `hosted` feature.

struct ActorProcessOptions {
    package_cache: agentos_client::ProcessPackageCacheOptions,
    actor_budgets: agentos_actor::HostedActorBudgetOptions,
    preload: agentos_actor::PreloadProcessOptions,
    preload_policy: agentos_actor::PreloadValidationPolicy,
    shutdown_timeout_ms: u64,
}

fn parse_actor_options(
    mut args: impl Iterator<Item = String>,
) -> Result<ActorProcessOptions, String> {
    let mut package_cache = agentos_client::ProcessPackageCacheOptions::default();
    // The default is an isolated process-lifetime cache. A shared fixed path
    // would make a second worker on the same host fail the exclusive cache
    // lock during startup. Operators opt into recovery with a distinct stable
    // directory for each concurrently running actor worker.
    package_cache.root =
        std::env::var_os("AGENTOS_PACKAGE_CACHE_DIR").map(std::path::PathBuf::from);
    let mut preload = agentos_actor::PreloadProcessOptions::default();
    let mut preload_policy = agentos_actor::PreloadValidationPolicy::default();
    let mut shutdown_timeout_ms = 30_000;
    let mut actor_budgets = agentos_actor::HostedActorBudgetOptions::default();
    while let Some(argument) = args.next() {
        let (name, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value.to_owned()))
            });
        let value = match inline_value {
            Some(value) => value,
            None => args
                .next()
                .ok_or_else(|| format!("{name} requires a value"))?,
        };
        match name {
            "--actor-budgets-json" => {
                actor_budgets = serde_json::from_str(&value)
                    .map_err(|error| format!("invalid --actor-budgets-json: {error}"))?;
                agentos_actor::HostedActorBudgets::normalize(actor_budgets.clone())
                    .map_err(|error| format!("invalid --actor-budgets-json: {error:#}"))?;
            }
            "--worker-shutdown-timeout-ms" => {
                shutdown_timeout_ms = super::parse_positive_option(name, &value)?;
                if shutdown_timeout_ms > 300_000 {
                    return Err(String::from(
                        "--worker-shutdown-timeout-ms must not exceed 300000ms",
                    ));
                }
            }
            "--package-cache-dir" => {
                if value.is_empty() {
                    return Err(String::from("--package-cache-dir must not be empty"));
                }
                package_cache.root = Some(std::path::PathBuf::from(value));
            }
            "--package-cache-min-free-bytes" => {
                package_cache.min_free_bytes = super::parse_cache_min_free_bytes(name, &value)?;
            }
            "--package-cache-max-bytes" => {
                package_cache.max_bytes = super::parse_positive_option(name, &value)?;
            }
            "--package-cache-max-entries" => {
                package_cache.max_entries = super::parse_positive_option(name, &value)?;
            }
            "--package-cache-max-concurrent-acquisitions" => {
                package_cache.max_concurrent_acquisitions =
                    super::parse_positive_option(name, &value)?;
            }
            "--package-cache-max-pending-acquisitions" => {
                package_cache.max_pending_acquisitions =
                    super::parse_positive_option(name, &value)?;
            }
            "--package-cache-acquisition-timeout-ms" => {
                package_cache.acquisition_timeout_ms = super::parse_positive_option(name, &value)?;
            }
            "--package-cache-max-source-entries" => {
                package_cache.max_source_entries = super::parse_positive_option(name, &value)?;
            }
            "--package-cache-source-ttl-ms" => {
                package_cache.source_ttl_ms = super::parse_positive_option(name, &value)?;
            }
            "--preload-policy-json" => {
                preload_policy = serde_json::from_str(&value)
                    .map_err(|error| format!("invalid --preload-policy-json: {error}"))?;
            }
            "--preload-startup-deadline-ms" => {
                preload.startup_deadline_ms = super::parse_positive_option(name, &value)?;
            }
            "--preload-plan-read-timeout-ms" => {
                preload.plan_read_timeout_ms = super::parse_positive_option(name, &value)?;
            }
            "--preload-max-plan-entries" => {
                preload.max_plan_entries = super::parse_positive_option(name, &value)?;
            }
            "--preload-max-plan-bytes" => {
                preload.max_plan_bytes = super::parse_positive_option(name, &value)?;
            }
            "--preload-warm-concurrency" => {
                preload.warm_concurrency = super::parse_positive_option(name, &value)?;
            }
            "--preload-max-observation-entries" => {
                preload.max_observation_entries = super::parse_positive_option(name, &value)?;
            }
            "--preload-flush-interval-ms" => {
                preload.flush_interval_ms = super::parse_positive_option(name, &value)?;
            }
            "--preload-action-timeout-ms" => {
                preload.action_timeout_ms = super::parse_positive_option(name, &value)?;
            }

            _ => return Err(format!("unknown agentOS actor argument: {argument}")),
        }
    }
    super::validate_cache_directory(&package_cache)?;
    package_cache
        .validate()
        .map_err(|error| format!("invalid agentOS actor package cache configuration: {error}"))?;
    preload
        .validate()
        .map_err(|error| format!("invalid agentOS actor preload configuration: {error:#}"))?;
    preload_policy
        .validate()
        .map_err(|error| format!("invalid --preload-policy-json: {error:#}"))?;
    Ok(ActorProcessOptions {
        preload_policy,
        package_cache,
        actor_budgets,
        preload,
        shutdown_timeout_ms,
    })
}

pub(super) fn run_actor(args: impl Iterator<Item = String>) -> Result<(), String> {
    let ActorProcessOptions {
        preload_policy,
        mut package_cache,
        actor_budgets,
        preload,
        shutdown_timeout_ms,
    } = parse_actor_options(args)?;
    agentos_actor::configure_preload_validation_policy(preload_policy).map_err(|error| {
        format!("configure preload validation policy before startup: {error:#}")
    })?;
    agentos_actor::configure_process_actor_budgets(actor_budgets)
        .map_err(|error| format!("configure hosted actor budgets before startup: {error:#}"))?;
    let temporary_cache = prepare_actor_cache_directory(&mut package_cache)?;
    let mut child_shutdown_confirmed = true;
    let result = (|| {
        agentos_client::configure_shared_sidecar_package_cache(package_cache)
            .map_err(|error| format!("configure agentOS sidecar package cache: {error}"))?;
        agentos_actor::configure_process_preload(preload)
            .map_err(|error| format!("configure agentOS process preload: {error:#}"))?;
        let runtime = agentos_driver_tokio::TokioDriver::process(
            &agentos_driver_tokio::DriverConfig::default(),
        )
        .map_err(|error| format!("initialize agentOS process runtime: {error}"))?;
        let run_result = runtime
            .block_on(agentos_actor::registry().start())
            .map_err(|error| format!("run agentOS actor: {error:#}"));
        let (preload_result, shared_result) = runtime.block_on(async {
            let deadline = std::time::Duration::from_millis(shutdown_timeout_ms);
            // Close shared admission even if optional preload teardown fails.
            // Both phases are bounded and retain failed owners for explicit retry.
            tokio::join!(
                async {
                    tokio::time::timeout(deadline, agentos_actor::shutdown_process_preload())
                        .await
                        .map_err(|_| format!("timeout: hosted preload shutdown exceeded {shutdown_timeout_ms}ms; raise --worker-shutdown-timeout-ms"))?
                        .map_err(|error| format!("close agentOS process preload: {error:#}"))
                },
                async {
                    agentos_client::actor_internals::shutdown_runtime_shared_sidecars(deadline)
                        .await
                        .map_err(|error| format!("close agentOS worker sidecars: {error}"))
                }
            )
        });
        child_shutdown_confirmed = shared_result.is_ok();
        let errors = [run_result, preload_result, shared_result]
            .into_iter()
            .filter_map(Result::err)
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })();
    // Child shutdown is confirmed before removing this worker's temporary cache.
    // The single process runtime remains owned by TokioDriver until process exit.
    finish_actor_cache_directory(temporary_cache, result, child_shutdown_confirmed)
}

fn prepare_actor_cache_directory(
    options: &mut agentos_client::ProcessPackageCacheOptions,
) -> Result<Option<tempfile::TempDir>, String> {
    super::validate_cache_directory(options)?;
    if options.root.is_some() {
        return Ok(None);
    }
    let directory = tempfile::Builder::new()
        .prefix("agentos-actor-package-cache-")
        .tempdir()
        .map_err(|error| format!("create actor temporary package cache: {error}"))?;
    options.root = Some(directory.path().to_path_buf());
    Ok(Some(directory))
}

fn finish_actor_cache_directory(
    directory: Option<tempfile::TempDir>,
    result: Result<(), String>,
    child_shutdown_confirmed: bool,
) -> Result<(), String> {
    if let Some(directory) = directory {
        if !child_shutdown_confirmed {
            let path = directory.keep();
            tracing::error!(cache_path = %path.display(), "retaining actor temporary package cache because sidecar shutdown is unconfirmed; remove it only after stopping the worker");
            return result.and(Err(format!(
                "sidecar shutdown unconfirmed; package cache retained at {}",
                path.display()
            )));
        }
        let path = directory.path().to_path_buf();
        if let Err(error) = directory.close() {
            tracing::error!(%error, cache_path = %path.display(), "remove actor temporary package cache");
            let cleanup = format!(
                "remove actor temporary package cache {}: {error}",
                path.display()
            );
            return Err(match result {
                Ok(()) => cleanup,
                Err(original) => format!("{original}; {cleanup}"),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_limits_are_positive_and_unknown_options_are_rejected() {
        let options = parse_actor_options(
            [
                "--preload-warm-concurrency=2".to_owned(),
                "--preload-action-timeout-ms=1000".to_owned(),
                "--package-cache-max-entries=4".to_owned(),
                "--package-cache-min-free-bytes=0".to_owned(),
            ]
            .into_iter(),
        )
        .expect("parse explicit hosted limits");
        assert_eq!(options.preload.warm_concurrency, 2);
        assert_eq!(options.preload.action_timeout_ms, 1000);
        assert_eq!(options.package_cache.max_entries, 4);
        assert_eq!(options.package_cache.min_free_bytes, 0);
        for option in [
            "--preload-warm-concurrency=0",
            "--preload-action-timeout-ms=0",
            "--worker-shutdown-timeout-ms=0",
            "--worker-shutdown-timeout-ms=300001",
            "--package-cache-max-entries=0",
            "--inspector-tabs-dir=/tmp/unused",
        ] {
            assert!(
                parse_actor_options([option.to_owned()].into_iter()).is_err(),
                "accepted {option}"
            );
        }
    }

    #[test]
    fn temporary_cache_is_removed_only_after_confirmed_child_shutdown() {
        for result in [Ok(()), Err(String::from("synthetic startup failure"))] {
            let mut options = agentos_client::ProcessPackageCacheOptions::default();
            options.root = None;
            let temporary = prepare_actor_cache_directory(&mut options).expect("private cache");
            let path = options.root.expect("cache path");
            assert!(path.is_dir());
            assert_eq!(
                finish_actor_cache_directory(temporary, result.clone(), true),
                result
            );
            assert!(!path.exists());
        }
        let mut options = agentos_client::ProcessPackageCacheOptions::default();
        options.root = None;
        let temporary = prepare_actor_cache_directory(&mut options).expect("private cache");
        let path = options.root.expect("cache path");
        assert!(finish_actor_cache_directory(temporary, Ok(()), false)
            .unwrap_err()
            .contains("shutdown unconfirmed"));
        assert!(path.is_dir());
        std::fs::remove_dir(path).expect("test owns retained cache without any child");
    }

    #[test]
    fn explicit_cache_directory_remains_owned_by_the_operator() {
        let directory = tempfile::tempdir().expect("private operator directory");
        let mut options = agentos_client::ProcessPackageCacheOptions::default();
        options.root = Some(directory.path().to_owned());
        assert!(prepare_actor_cache_directory(&mut options)
            .unwrap()
            .is_none());
        finish_actor_cache_directory(None, Ok(()), true).unwrap();
        assert!(directory.path().is_dir());
    }
}

#[cfg(test)]
#[path = "../tests/unit/hosted_actor_budgets.rs"]
mod actor_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/hosted_preload_policy.rs"]
mod preload_policy_tests;
