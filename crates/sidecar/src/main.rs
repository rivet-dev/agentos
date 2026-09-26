use std::os::fd::{FromRawFd, OwnedFd};

use nix::fcntl::{fcntl, FcntlArg};
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const CONTROL_FD: i32 = 3;

fn parse_runtime_config(
    mut args: impl Iterator<Item = String>,
) -> Result<agentos_driver_tokio::DriverConfig, String> {
    let mut config = agentos_driver_tokio::DriverConfig::default();
    while let Some(argument) = args.next() {
        let value = if argument == "--max-active-vms" {
            args.next()
                .ok_or_else(|| String::from("--max-active-vms requires a positive integer"))?
        } else if let Some(value) = argument.strip_prefix("--max-active-vms=") {
            value.to_owned()
        } else {
            return Err(format!("unknown agentOS sidecar argument: {argument}"));
        };
        let maximum = value.parse::<usize>().map_err(|_| {
            format!("--max-active-vms must be a positive integer, received {value:?}")
        })?;
        if maximum == 0 {
            return Err(String::from(
                "--max-active-vms must be greater than zero when configured",
            ));
        }
        config.max_active_vm_executors = maximum;
    }
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

fn parse_sidecar_options(
    args: impl Iterator<Item = String>,
) -> Result<
    (
        agentos_driver_tokio::DriverConfig,
        Option<agentos_client::ProcessPackageCacheOptions>,
    ),
    String,
> {
    let mut runtime_args = Vec::new();
    let mut cache = agentos_client::ProcessPackageCacheOptions::default();
    let mut cache_configured = false;
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        let (name, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        let value = match inline_value {
            Some(value) => value.to_owned(),
            None => args
                .next()
                .ok_or_else(|| format!("{name} requires a value"))?,
        };
        if name == "--max-active-vms" {
            runtime_args.extend([name.to_owned(), value]);
            continue;
        }
        cache_configured = true;
        match name {
            "--package-cache-dir" => {
                if value.is_empty() {
                    return Err("--package-cache-dir must not be empty".into());
                }
                cache.root = Some(std::path::PathBuf::from(value));
            }
            "--package-cache-min-free-bytes" => {
                cache.min_free_bytes = parse_cache_min_free_bytes(name, &value)?;
            }
            "--package-cache-max-bytes" => {
                cache.max_bytes = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-entries" => {
                cache.max_entries = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-concurrent-acquisitions" => {
                cache.max_concurrent_acquisitions = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-pending-acquisitions" => {
                cache.max_pending_acquisitions = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-acquisition-timeout-ms" => {
                cache.acquisition_timeout_ms = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-source-entries" => {
                cache.max_source_entries = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-source-ttl-ms" => {
                cache.source_ttl_ms = parse_positive_actor_option(name, &value)?;
            }
            _ => return Err(format!("unknown agentOS sidecar argument: {argument}")),
        }
    }
    let runtime_config = parse_runtime_config(runtime_args.into_iter())?;
    if cache_configured {
        validate_actor_cache_directory(&cache)?;
        cache.validate().map_err(|error| error.to_string())?;
    }
    Ok((runtime_config, cache_configured.then_some(cache)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entrypoint {
    Actor,
    Sidecar,
}

fn parse_entrypoint(args: &mut impl Iterator<Item = String>) -> Result<Entrypoint, String> {
    match args.next().as_deref() {
        None | Some("sidecar") => Ok(Entrypoint::Sidecar),
        Some("actor") => Ok(Entrypoint::Actor),
        Some(argument) => Err(format!(
            "unknown agentOS entry point {argument:?}; expected 'actor' or 'sidecar'"
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActorProcessOptions {
    package_cache: agentos_client::ProcessPackageCacheOptions,
    preload: agentos_actor::PreloadProcessOptions,
    inspector_tabs_dir: Option<std::path::PathBuf>,
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
    let mut inspector_tabs_dir =
        std::env::var_os("AGENTOS_INSPECTOR_TABS_DIR").map(std::path::PathBuf::from);
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
            "--package-cache-dir" => {
                if value.is_empty() {
                    return Err(String::from("--package-cache-dir must not be empty"));
                }
                package_cache.root = Some(std::path::PathBuf::from(value));
            }
            "--package-cache-min-free-bytes" => {
                package_cache.min_free_bytes = parse_cache_min_free_bytes(name, &value)?;
            }
            "--package-cache-max-bytes" => {
                package_cache.max_bytes = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-entries" => {
                package_cache.max_entries = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-concurrent-acquisitions" => {
                package_cache.max_concurrent_acquisitions =
                    parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-pending-acquisitions" => {
                package_cache.max_pending_acquisitions = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-acquisition-timeout-ms" => {
                package_cache.acquisition_timeout_ms = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-max-source-entries" => {
                package_cache.max_source_entries = parse_positive_actor_option(name, &value)?;
            }
            "--package-cache-source-ttl-ms" => {
                package_cache.source_ttl_ms = parse_positive_actor_option(name, &value)?;
            }
            "--preload-startup-deadline-ms" => {
                preload.startup_deadline_ms = parse_positive_actor_option(name, &value)?;
            }
            "--preload-plan-read-timeout-ms" => {
                preload.plan_read_timeout_ms = parse_positive_actor_option(name, &value)?;
            }
            "--preload-max-plan-entries" => {
                preload.max_plan_entries = parse_positive_actor_option(name, &value)?;
            }
            "--preload-max-plan-bytes" => {
                preload.max_plan_bytes = parse_positive_actor_option(name, &value)?;
            }
            "--preload-warm-concurrency" => {
                preload.warm_concurrency = parse_positive_actor_option(name, &value)?;
            }
            "--preload-max-observation-entries" => {
                preload.max_observation_entries = parse_positive_actor_option(name, &value)?;
            }
            "--preload-flush-interval-ms" => {
                preload.flush_interval_ms = parse_positive_actor_option(name, &value)?;
            }
            "--preload-action-timeout-ms" => {
                preload.action_timeout_ms = parse_positive_actor_option(name, &value)?;
            }
            "--inspector-tabs-dir" => {
                if value.is_empty() {
                    return Err(String::from("--inspector-tabs-dir must not be empty"));
                }
                inspector_tabs_dir = Some(std::path::PathBuf::from(value));
            }
            _ => return Err(format!("unknown agentOS actor argument: {argument}")),
        }
    }
    validate_actor_cache_directory(&package_cache)?;
    package_cache
        .validate()
        .map_err(|error| format!("invalid agentOS actor package cache configuration: {error}"))?;
    preload
        .validate()
        .map_err(|error| format!("invalid agentOS actor preload configuration: {error:#}"))?;
    if let Some(path) = &inspector_tabs_dir {
        if !path.is_dir() || !path.join("index.html").is_file() {
            return Err(format!(
                "agentOS inspector tabs directory must contain index.html: {}",
                path.display()
            ));
        }
    }
    Ok(ActorProcessOptions {
        package_cache,
        preload,
        inspector_tabs_dir,
    })
}

fn parse_cache_min_free_bytes(name: &str, value: &str) -> Result<u64, String> {
    // Zero intentionally disables the disk reserve; unlike queue/size caps it
    // does not make acquisition unbounded (the max-byte cap still applies).
    value
        .parse()
        .map_err(|_| format!("{name} requires a non-negative integer, received {value:?}"))
}

fn parse_positive_actor_option<T>(name: &str, value: &str) -> Result<T, String>
where
    T: std::str::FromStr + PartialEq + From<u8>,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| format!("{name} requires a positive integer, received {value:?}"))?;
    if parsed == T::from(0) {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn main() {
    init_tracing();
    tracing::info!(target: "agentos_sidecar::perf", "sidecar process started");
    #[cfg(feature = "wasm-wasmtime")]
    if std::env::args().nth(1).as_deref()
        == Some(agentos_executor_wasm_wasmtime::WORKER_MODE_ARGUMENT)
    {
        if let Err(error) = agentos_executor_wasm_wasmtime::run_worker_entry() {
            tracing::error!(
                code = %error.code,
                message = %error.message,
                "Wasmtime thread worker failed"
            );
            std::process::exit(1);
        }
        return;
    }
    let mut args = std::env::args().skip(1);
    let entrypoint = match parse_entrypoint(&mut args) {
        Ok(entrypoint) => entrypoint,
        Err(error) => {
            tracing::error!(%error, "invalid agentOS executable entry point");
            std::process::exit(2);
        }
    };
    let result = match entrypoint {
        Entrypoint::Actor => run_actor(args),
        Entrypoint::Sidecar => run_sidecar(args),
    };
    if let Err(error) = result {
        tracing::error!(?error, "agentOS executable failed");
        std::process::exit(1);
    }
}

fn run_actor(args: impl Iterator<Item = String>) -> Result<(), String> {
    let ActorProcessOptions {
        mut package_cache,
        preload,
        inspector_tabs_dir,
    } = parse_actor_options(args)?;
    let temporary_cache = prepare_actor_cache_directory(&mut package_cache)?;
    let mut child_shutdown_confirmed = true;
    let result = (|| {
        agentos_client::configure_shared_sidecar_package_cache(package_cache)
            .map_err(|error| format!("configure agentOS sidecar package cache: {error}"))?;
        agentos_actor::configure_process_preload(preload)
            .map_err(|error| format!("configure agentOS process preload: {error:#}"))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("build agentOS actor runtime: {error}"))?;
        let run_result = runtime
            .block_on(agentos_actor::registry_with_inspector_tabs(inspector_tabs_dir).start())
            .map_err(|error| format!("run agentOS actor: {error:#}"));
        let shutdown_result = runtime
            .block_on(agentos_actor::shutdown_process_preload())
            .map_err(|error| format!("close agentOS package session: {error:#}"));
        child_shutdown_confirmed = shutdown_result.is_ok();
        match (run_result, shutdown_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(run_error), Err(shutdown_error)) => Err(format!("{run_error}; {shutdown_error}")),
        }
    })();
    // The runtime has drained and dropped before removing this worker's own
    // temporary directory. Static cache state otherwise never drops its TempDir.
    finish_actor_cache_directory(temporary_cache, result, child_shutdown_confirmed)
}

fn validate_actor_cache_directory(
    options: &agentos_client::ProcessPackageCacheOptions,
) -> Result<(), String> {
    if options
        .root
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(String::from("AGENTOS_PACKAGE_CACHE_DIR or --package-cache-dir must not be empty; unset it to use an isolated temporary cache"));
    }
    Ok(())
}

fn prepare_actor_cache_directory(
    options: &mut agentos_client::ProcessPackageCacheOptions,
) -> Result<Option<tempfile::TempDir>, String> {
    validate_actor_cache_directory(options)?;
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

fn run_sidecar(args: impl Iterator<Item = String>) -> Result<(), String> {
    let (runtime_config, package_cache) = parse_sidecar_options(args)?;
    if let Some(package_cache) = package_cache {
        agentos_client::configure_process_package_cache(package_cache)
            .map_err(|error| format!("configure sidecar package cache: {error}"))?;
    }
    if let Err(error) = fcntl(CONTROL_FD, FcntlArg::F_GETFD) {
        return Err(format!(
            "missing inherited sidecar response/control descriptor: {error}"
        ));
    }
    // SAFETY: the launch contract transfers sole ownership of the open fd 3.
    let control_fd = unsafe { OwnedFd::from_raw_fd(CONTROL_FD) };
    agentos_sidecar::transport::run_with_configured_executors(
        agentos_sidecar::extensions(),
        Some(control_fd),
        Some(runtime_config),
        agentos_sidecar::executor_registry(),
    )
    .map_err(|error| format!("run agentOS sidecar: {error:#}"))
}

/// `1` => true, anything else => false. Mirrors rivet's `env_flag`.
fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

/// Initialize tracing for the sidecar.
///
/// Mirrors the rivet logging setup (`rivetkit-napi::init_tracing`): an
/// `EnvFilter`-gated subscriber with a logfmt formatter, so log level and
/// verbosity are runtime-configurable instead of hardcoded.
///
/// Configuration (all optional):
/// - level: `AGENTOS_LOG_LEVEL` > `LOG_LEVEL` > `RUST_LOG` > `"info"`.
/// - format: `RUST_LOG_FORMAT=logfmt` (default) or `text`.
/// - sink: `AGENTOS_LOG_FILE` (append to that file) else stderr.
/// - field toggles: `RUST_LOG_{SPAN_NAME,SPAN_PATH,TARGET,LOCATION,MODULE_PATH,ANSI_COLOR}`.
///
/// The sink MUST be stderr or a file — NEVER stdout, which carries the
/// sidecar's binary frame protocol (see crates/CLAUDE.md: "Control channels
/// must be out-of-band").
fn init_tracing() {
    // Level priority: AGENTOS_LOG_LEVEL > LOG_LEVEL > RUST_LOG > "info".
    let directive = std::env::var("AGENTOS_LOG_LEVEL")
        .ok()
        .or_else(|| std::env::var("AGENTOS_LOG").ok())
        .or_else(|| std::env::var("LOG_LEVEL").ok())
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    let env_filter = EnvFilter::try_new(&directive).unwrap_or_else(|_| EnvFilter::new("info"));

    // Sink: a file if AGENTOS_LOG_FILE is set, else stderr. Never stdout.
    let writer: BoxMakeWriter = match std::env::var("AGENTOS_LOG_FILE") {
        Ok(path) if !path.is_empty() => {
            let path = std::path::PathBuf::from(path);
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let name = path
                .file_name()
                .map(std::ffi::OsString::from)
                .unwrap_or_else(|| std::ffi::OsString::from("agentos-sidecar.log"));
            BoxMakeWriter::new(tracing_appender::rolling::never(dir, name))
        }
        _ => BoxMakeWriter::new(std::io::stderr),
    };

    let registry = tracing_subscriber::registry().with(env_filter);

    let text_format = std::env::var("RUST_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("text"))
        .unwrap_or(false);

    if text_format {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer),
            )
            .init();
    } else {
        registry
            .with(
                tracing_logfmt::builder()
                    .with_span_name(env_flag("RUST_LOG_SPAN_NAME"))
                    .with_span_path(env_flag("RUST_LOG_SPAN_PATH"))
                    .with_target(env_flag("RUST_LOG_TARGET"))
                    .with_location(env_flag("RUST_LOG_LOCATION"))
                    .with_module_path(env_flag("RUST_LOG_MODULE_PATH"))
                    .with_ansi_color(env_flag("RUST_LOG_ANSI_COLOR"))
                    .layer()
                    .with_writer(writer),
            )
            .init();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        finish_actor_cache_directory, parse_actor_options, parse_entrypoint, parse_runtime_config,
        parse_sidecar_options, prepare_actor_cache_directory, Entrypoint,
    };

    #[test]
    fn executable_entrypoints_are_fixed() {
        assert_eq!(
            parse_entrypoint(&mut std::iter::empty()).expect("default entrypoint"),
            Entrypoint::Sidecar
        );
        assert_eq!(
            parse_entrypoint(&mut [String::from("actor")].into_iter()).expect("actor entrypoint"),
            Entrypoint::Actor
        );
        assert!(parse_entrypoint(&mut [String::from("unknown")].into_iter())
            .expect_err("unknown entrypoint must fail")
            .contains("expected 'actor' or 'sidecar'"));
    }

    #[test]
    fn runtime_executor_limit_is_bounded_by_default_and_configurable() {
        let default = parse_runtime_config(std::iter::empty()).expect("parse default config");
        assert!(default.max_active_vm_executors > 0);

        let configured =
            parse_runtime_config([String::from("--max-active-vms"), String::from("7")].into_iter())
                .expect("parse configured executor limit");
        assert_eq!(configured.max_active_vm_executors, 7);

        let error = parse_runtime_config([String::from("--max-active-vms=0")].into_iter())
            .expect_err("zero executor limit must fail");
        assert!(error.contains("greater than zero"));
    }

    #[test]
    fn cache_minimum_disk_reserve_accepts_explicit_zero() {
        let actor =
            parse_actor_options([String::from("--package-cache-min-free-bytes=0")].into_iter())
                .unwrap();
        assert_eq!(actor.package_cache.min_free_bytes, 0);
        let (_, cache) = super::parse_sidecar_options(
            [String::from("--package-cache-min-free-bytes=0")].into_iter(),
        )
        .unwrap();
        assert_eq!(cache.unwrap().min_free_bytes, 0);
    }

    #[test]
    fn sidecar_accepts_worker_package_cache_settings() {
        let (runtime, cache) = parse_sidecar_options(
            [
                String::from("--max-active-vms=7"),
                String::from("--package-cache-dir=/tmp/agentos-child-cache-test"),
                String::from("--package-cache-max-entries=4"),
            ]
            .into_iter(),
        )
        .expect("parse child sidecar options");
        assert_eq!(runtime.max_active_vm_executors, 7);
        let cache = cache.expect("package cache override");
        assert_eq!(cache.max_entries, 4);
        assert_eq!(
            cache.root,
            Some(std::path::PathBuf::from("/tmp/agentos-child-cache-test"))
        );
        assert!(
            parse_sidecar_options([String::from("--package-cache-max-entries=0")].into_iter())
                .is_err()
        );
    }

    #[test]
    fn actor_package_cache_limits_are_startup_only_and_bounded() {
        let default = parse_actor_options(std::iter::empty()).expect("parse actor defaults");
        assert_eq!(
            default.package_cache.root,
            std::env::var_os("AGENTOS_PACKAGE_CACHE_DIR").map(std::path::PathBuf::from),
            "without an operator override, each worker needs its own temporary cache"
        );
        let options = parse_actor_options(
            [
                String::from("--package-cache-dir=/tmp/agentos-test-worker-cache"),
                String::from("--package-cache-max-bytes=1024"),
                String::from("--package-cache-max-entries"),
                String::from("4"),
                String::from("--package-cache-max-concurrent-acquisitions=2"),
                String::from("--package-cache-max-pending-acquisitions=8"),
                String::from("--package-cache-acquisition-timeout-ms=9000"),
                String::from("--package-cache-max-source-entries=16"),
                String::from("--package-cache-source-ttl-ms=30000"),
                String::from("--preload-startup-deadline-ms=5000"),
                String::from("--preload-plan-read-timeout-ms=1000"),
                String::from("--preload-max-plan-entries=8"),
                String::from("--preload-max-plan-bytes=2048"),
                String::from("--preload-warm-concurrency=2"),
                String::from("--preload-max-observation-entries=6"),
                String::from("--preload-flush-interval-ms=60000"),
                String::from("--preload-action-timeout-ms=2000"),
            ]
            .into_iter(),
        )
        .expect("parse actor package cache limits");
        assert_eq!(
            options.package_cache.root,
            Some(std::path::PathBuf::from("/tmp/agentos-test-worker-cache"))
        );
        assert_eq!(options.package_cache.max_bytes, 1024);
        assert_eq!(options.package_cache.max_entries, 4);
        assert_eq!(options.package_cache.max_concurrent_acquisitions, 2);
        assert_eq!(options.package_cache.max_pending_acquisitions, 8);
        assert_eq!(options.package_cache.acquisition_timeout_ms, 9000);
        assert_eq!(options.package_cache.max_source_entries, 16);
        assert_eq!(options.package_cache.source_ttl_ms, 30000);
        assert_eq!(options.preload.startup_deadline_ms, 5000);
        assert_eq!(options.preload.plan_read_timeout_ms, 1000);
        assert_eq!(options.preload.max_plan_entries, 8);
        assert_eq!(options.preload.max_plan_bytes, 2048);
        assert_eq!(options.preload.warm_concurrency, 2);
        assert_eq!(options.preload.max_observation_entries, 6);
        assert_eq!(options.preload.flush_interval_ms, 60000);
        assert_eq!(options.preload.action_timeout_ms, 2000);
        assert!(
            parse_actor_options([String::from("--package-cache-max-entries=0")].into_iter())
                .is_err()
        );
        assert!(parse_actor_options(
            [
                String::from("--package-cache-max-concurrent-acquisitions=9"),
                String::from("--package-cache-max-pending-acquisitions=8"),
            ]
            .into_iter()
        )
        .is_err());
    }

    #[test]
    fn actor_temporary_cache_is_isolated_and_removed_after_success_or_failure() {
        for result in [Ok(()), Err(String::from("actor startup failed"))] {
            let mut options = agentos_client::ProcessPackageCacheOptions::default();
            let directory = prepare_actor_cache_directory(&mut options).unwrap();
            let path = options.root.unwrap();
            assert!(path.is_dir());
            std::fs::write(path.join("cached-package"), b"test package").unwrap();
            assert_eq!(
                finish_actor_cache_directory(directory, result.clone(), true),
                result
            );
            assert!(!path.exists(), "temporary cache survived actor shutdown");
        }
    }

    #[test]
    fn actor_persistent_cache_is_preserved_and_empty_paths_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let mut options = agentos_client::ProcessPackageCacheOptions {
            root: Some(directory.path().to_path_buf()),
            ..Default::default()
        };
        let cleanup = prepare_actor_cache_directory(&mut options).unwrap();
        assert!(cleanup.is_none());
        finish_actor_cache_directory(cleanup, Ok(()), true).unwrap();
        assert!(directory.path().is_dir());
        options.root = Some(std::path::PathBuf::new());
        assert!(prepare_actor_cache_directory(&mut options)
            .unwrap_err()
            .contains("must not be empty"));
    }

    #[test]
    fn actor_temporary_cache_survives_unconfirmed_child_shutdown() {
        let mut options = agentos_client::ProcessPackageCacheOptions::default();
        let directory = prepare_actor_cache_directory(&mut options).unwrap();
        let path = options.root.unwrap();
        let failure = Err(String::from("child termination failed"));
        assert_eq!(
            finish_actor_cache_directory(directory, failure.clone(), false),
            failure
        );
        assert!(path.is_dir());
        // This test has no child; reclaim its isolated retained directory.
        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn actor_inspector_tabs_require_built_assets() {
        let root = tempfile::tempdir().expect("temp inspector root");
        let option = format!("--inspector-tabs-dir={}", root.path().display());
        assert!(parse_actor_options([option.clone()].into_iter())
            .expect_err("missing index must fail")
            .contains("index.html"));
        std::fs::write(root.path().join("index.html"), "<html></html>")
            .expect("write tab entrypoint");
        assert!(parse_actor_options([option].into_iter()).is_ok());
    }
}
