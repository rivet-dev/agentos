//! Process entry point shared by the `agentos-sidecar` binary and embedding binaries.
//!
//! The executable adopts its inherited control descriptor; this module owns the
//! sidecar arguments and package-cache setup so every binary that runs the
//! sidecar process loop parses and starts it the same way.

use std::os::fd::OwnedFd;

/// Runs the agentOS sidecar process loop on `control_fd` with sidecar CLI `args`.
///
/// The process must follow the sidecar transport contract: fd 0 carries host
/// requests, stdout carries only event frames (log to stderr), and `control_fd`
/// is the inherited full-duplex response/control socket (fd 3 by convention),
/// adopted by the caller. Blocks until the host shuts the sidecar down; returns
/// an error for invalid arguments or a failed run.
pub fn run(control_fd: OwnedFd, args: impl Iterator<Item = String>) -> Result<(), String> {
    let (runtime_config, package_cache) = parse_sidecar_options(args)?;
    if let Some(package_cache) = package_cache {
        agentos_client::configure_process_package_cache(package_cache)
            .map_err(|error| format!("configure sidecar package cache: {error}"))?;
    }
    crate::transport::run_with_configured_executors(
        crate::extensions(),
        Some(control_fd),
        Some(runtime_config),
        crate::executor_registry(),
    )
    .map_err(|error| format!("run agentOS sidecar: {error:#}"))
}

fn parse_runtime_config(
    mut args: impl Iterator<Item = String>,
) -> Result<agentos_driver_tokio::DriverConfig, String> {
    let mut config = agentos_driver_tokio::DriverConfig::default();
    let mut explicit_control_frames = false;
    while let Some(argument) = args.next() {
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        if !matches!(
            name,
            "--max-blocking-bytes"
                | "--max-active-vms"
                | "--max-in-flight-requests"
                | "--max-terminal-frames"
                | "--max-terminal-bytes"
                | "--max-control-frames"
                | "--max-control-bytes"
        ) {
            return Err(format!("unknown agentOS sidecar argument: {argument}"));
        }
        let value = match inline {
            Some(value) => value.to_owned(),
            None => args
                .next()
                .ok_or_else(|| format!("{name} requires a positive integer"))?,
        };
        let maximum = parse_positive_option::<usize>(name, &value)?;
        match name {
            "--max-blocking-bytes" => config.max_blocking_job_bytes = maximum,
            "--max-active-vms" => config.max_active_vm_executors = maximum,
            "--max-in-flight-requests" => config.protocol.max_in_flight_requests = maximum,
            "--max-terminal-frames" => config.protocol.max_terminal_frames = maximum,
            "--max-terminal-bytes" => config.protocol.max_terminal_bytes = maximum,
            "--max-control-frames" => {
                config.protocol.max_control_frames = maximum;
                explicit_control_frames = true;
            }
            "--max-control-bytes" => config.protocol.max_control_bytes = maximum,
            _ => unreachable!("runtime option was validated above"),
        }
    }
    // The CLI and SDK expose ordinary admission without exposing ingress or
    // queue sizing. Accommodate the selected ordinary lane plus the default
    // queue while keeping the byte cap independent.
    let protocol = &mut config.protocol;
    let required_ingress = protocol
        .max_in_flight_requests
        .checked_add(protocol.request_queue_limit)
        .ok_or("runtime.protocol.maxInFlightRequests + runtime.protocol.requestQueueLimit overflows usize")?;
    protocol.max_ingress_frames = protocol.max_ingress_frames.max(required_ingress);

    // Expanded CLI profiles used to have spare control room for heartbeat
    // frames. If the operator left control capacity at its default, account
    // for the new priority lane and retain headroom equal to its default count.
    if !explicit_control_frames
        && protocol.max_in_flight_requests
            > agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_IN_FLIGHT_REQUESTS
    {
        let required_control = protocol
            .max_terminal_frames
            .checked_add(protocol.max_in_flight_priority_requests)
            .and_then(|value| value.checked_add(protocol.max_progress_frames))
            .and_then(|value| value.checked_add(protocol.max_rejection_frames))
            .and_then(|value| value.checked_add(protocol.max_in_flight_priority_requests))
            .ok_or("runtime.protocol logical control frame capacities overflow usize")?;
        protocol.max_control_frames = protocol.max_control_frames.max(required_control);
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
        if matches!(
            name,
            "--max-blocking-bytes"
                | "--max-active-vms"
                | "--max-in-flight-requests"
                | "--max-terminal-frames"
                | "--max-terminal-bytes"
                | "--max-control-frames"
                | "--max-control-bytes"
        ) {
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

#[doc(hidden)]
pub fn parse_cache_min_free_bytes(name: &str, value: &str) -> Result<u64, String> {
    // Zero intentionally disables the disk reserve; unlike queue/size caps it
    // does not make acquisition unbounded (the max-byte cap still applies).
    value
        .parse()
        .map_err(|_| format!("{name} requires a non-negative integer, received {value:?}"))
}

#[doc(hidden)]
pub fn parse_positive_option<T>(name: &str, value: &str) -> Result<T, String>
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

#[doc(hidden)]
pub fn validate_cache_directory(
    options: &agentos_client::ProcessPackageCacheOptions,
) -> Result<(), String> {
    if options
        .root
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(String::from(
            "AGENTOS_PACKAGE_CACHE_DIR or --package-cache-dir must not be empty; unset it to use an isolated temporary cache",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_runtime_config, parse_sidecar_options};

    #[test]
    fn protocol_capacity_preserves_terminal_response_reservations() {
        let default = parse_runtime_config(std::iter::empty()).expect("default config");
        assert_eq!(default.protocol.max_in_flight_requests, 128);
        assert_eq!(default.protocol.max_ingress_frames, 512);
        let error = parse_runtime_config(["--max-in-flight-requests=512".to_owned()].into_iter())
            .expect_err("response credits remain enforced");
        assert!(error.contains("maxTerminalFrames"));
        let (configured, cache) = parse_sidecar_options(
            [
                "--max-in-flight-requests=512",
                "--max-terminal-frames=512",
                "--max-terminal-bytes=8388608",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .expect("explicit complete budget");
        assert_eq!(configured.protocol.max_in_flight_requests, 512);
        assert_eq!(configured.protocol.max_terminal_frames, 512);
        assert_eq!(configured.protocol.max_ingress_frames, 768);
        assert_eq!(configured.protocol.max_control_frames, 1152);
        let logical_control_frames = configured.protocol.max_terminal_frames
            + configured.protocol.max_in_flight_priority_requests
            + configured.protocol.max_progress_frames
            + configured.protocol.max_rejection_frames;
        assert!(configured.protocol.max_control_frames > logical_control_frames);
        assert!(cache.is_none());
        let expanded = parse_runtime_config(
            [
                "--max-in-flight-requests=1024",
                "--max-terminal-frames=1024",
                "--max-terminal-bytes=16777216",
                "--max-control-frames=2048",
                "--max-control-bytes=67108864",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .expect("explicit physical control budget");
        assert_eq!(expanded.protocol.max_control_frames, 2048);
        assert_eq!(expanded.protocol.max_in_flight_requests, 1024);
        assert_eq!(expanded.protocol.max_ingress_frames, 1280);
        let error = parse_runtime_config(
            [
                "--max-in-flight-requests=512",
                "--max-terminal-frames=512",
                "--max-terminal-bytes=8388608",
                "--max-control-frames=1023",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .expect_err("an explicit physical control budget must not expand");
        assert!(error.contains("maxControlFrames"));
        let overflow = parse_runtime_config(std::iter::once(format!(
            "--max-in-flight-requests={}",
            usize::MAX,
        )))
        .expect_err("derived ingress must not wrap");
        assert!(overflow.contains("maxInFlightRequests") && overflow.contains("overflows"));
        assert!(
            parse_runtime_config(["--max-in-flight-requests=0".to_owned()].into_iter()).is_err()
        );
    }

    #[test]
    fn blocking_byte_budget_is_explicit_and_enforced() {
        let default = parse_runtime_config(std::iter::empty()).unwrap();
        assert_eq!(default.max_blocking_job_bytes, 64 * 1024 * 1024);
        let (configured, cache) = parse_sidecar_options(
            [
                "--max-blocking-bytes=268435456",
                "--package-cache-max-entries=4",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .unwrap();
        assert_eq!(configured.max_blocking_job_bytes, 256 * 1024 * 1024);
        assert_eq!(cache.unwrap().max_entries, 4);
        assert!(parse_runtime_config(["--max-blocking-bytes=0".into()].into_iter()).is_err());
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
