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
                cache.max_bytes = parse_positive_option(name, &value)?;
            }
            "--package-cache-max-entries" => {
                cache.max_entries = parse_positive_option(name, &value)?;
            }
            "--package-cache-max-concurrent-acquisitions" => {
                cache.max_concurrent_acquisitions = parse_positive_option(name, &value)?;
            }
            "--package-cache-max-pending-acquisitions" => {
                cache.max_pending_acquisitions = parse_positive_option(name, &value)?;
            }
            "--package-cache-acquisition-timeout-ms" => {
                cache.acquisition_timeout_ms = parse_positive_option(name, &value)?;
            }
            "--package-cache-max-source-entries" => {
                cache.max_source_entries = parse_positive_option(name, &value)?;
            }
            "--package-cache-source-ttl-ms" => {
                cache.source_ttl_ms = parse_positive_option(name, &value)?;
            }
            _ => return Err(format!("unknown agentOS sidecar argument: {argument}")),
        }
    }
    let runtime_config = parse_runtime_config(runtime_args.into_iter())?;
    if cache_configured {
        validate_cache_directory(&cache)?;
        cache.validate().map_err(|error| error.to_string())?;
    }
    Ok((runtime_config, cache_configured.then_some(cache)))
}

fn parse_cache_min_free_bytes(name: &str, value: &str) -> Result<u64, String> {
    // Zero intentionally disables the disk reserve; unlike queue/size caps it
    // does not make acquisition unbounded (the max-byte cap still applies).
    value
        .parse()
        .map_err(|_| format!("{name} requires a non-negative integer, received {value:?}"))
}

fn parse_positive_option<T>(name: &str, value: &str) -> Result<T, String>
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
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some_and(|arg| arg == "sidecar") {
        args.next();
    }
    let result = run_sidecar(args);
    if let Err(error) = result {
        tracing::error!(?error, "agentOS executable failed");
        std::process::exit(1);
    }
}

fn validate_cache_directory(
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
    use super::{parse_runtime_config, parse_sidecar_options};

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
}
