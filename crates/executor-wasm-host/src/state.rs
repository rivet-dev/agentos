//! Per-execution state the host imports read and update.

use crate::backend::HostServiceError;
use crate::{active_cpu_limit_error, ThreadGroupHooks, WasmHostClient};
use agentos_executor_wasm_abi::{
    guest_visible_wasm_env, StartWasmExecutionRequest, WasmPermissionTier,
};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// An exec the kernel committed while the old image was running. The engine
/// unwinds the old guest, then starts `module` with this argv and environment.
#[derive(Debug)]
pub struct PendingExecReplacement<M> {
    pub permission_tier: Result<WasmPermissionTier, HostServiceError>,
    pub module: M,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// Engine-neutral execution state. `M` is the engine's compiled module type.
pub struct HostState<M> {
    pub host: WasmHostClient,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub virtual_pid: u32,
    pub thread_id: i32,
    pub thread_group: Option<Arc<dyn ThreadGroupHooks<M>>>,
    pub exit_code: Option<i32>,
    pub exec_replaced: bool,
    pub pending_exec_replacement: Option<PendingExecReplacement<M>>,
    pub max_module_file_bytes: usize,
    pub max_blocking_read_ms: u64,
    pub max_spawn_file_actions: usize,
    pub max_spawn_file_action_bytes: usize,
    pub warned_fixed_limits: HashSet<&'static str>,
    pub pending_open_mode: Option<u32>,
    pub pending_open_direct: bool,
    pub deterministic_fuel_enabled: bool,
    /// Whether this execution links the WASI threads runtime imports.
    pub threaded: bool,
    active_cpu_limit_ns: Option<u64>,
    active_cpu_started_ns: u64,
}

impl<M> std::fmt::Debug for HostState<M> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostState")
            .field("process", &self.host.process())
            .field("argv_count", &self.argv.len())
            .field("env_count", &self.env.len())
            .field("exit_code", &self.exit_code)
            .finish_non_exhaustive()
    }
}

impl<M> HostState<M> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: WasmHostClient,
        request: &StartWasmExecutionRequest,
        active_cpu_started_ns: u64,
        environment_is_guest_visible: bool,
        thread_group: Option<Arc<dyn ThreadGroupHooks<M>>>,
        thread_id: i32,
        threaded: bool,
    ) -> Result<Self, HostServiceError> {
        let argv = nul_terminated_strings(request.argv.iter().map(String::as_str), "argv")?;
        let guest_env = store_guest_environment(&request.env, environment_is_guest_visible);
        let env = nul_terminated_strings(
            guest_env
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
            "environment",
        )?;
        let virtual_pid = request
            .guest_runtime
            .virtual_pid
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_else(|| host.process().pid);
        let active_cpu_started_ns = host.active_cpu_origin(active_cpu_started_ns);
        Ok(Self {
            host,
            argv,
            env,
            virtual_pid,
            thread_id,
            thread_group,
            exit_code: None,
            exec_replaced: false,
            pending_exec_replacement: None,
            max_module_file_bytes: request
                .limits
                .max_module_file_bytes
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new("EFBIG", "module byte limit does not fit this platform")
                })?
                .unwrap_or(256 * 1024 * 1024),
            max_blocking_read_ms: request.limits.max_blocking_read_ms.unwrap_or(30_000),
            max_spawn_file_actions: request
                .limits
                .max_spawn_file_actions
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new(
                        "E2BIG",
                        "spawn file-action count limit does not fit this platform",
                    )
                })?
                .unwrap_or(4096),
            max_spawn_file_action_bytes: request
                .limits
                .max_spawn_file_action_bytes
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new(
                        "E2BIG",
                        "spawn file-action byte limit does not fit this platform",
                    )
                })?
                .unwrap_or(1024 * 1024),
            warned_fixed_limits: HashSet::new(),
            pending_open_mode: None,
            pending_open_direct: false,
            deterministic_fuel_enabled: request.limits.deterministic_fuel.is_some(),
            threaded,
            active_cpu_limit_ns: request
                .limits
                .active_cpu_time_limit_ms
                .map(|value| u64::from(value).saturating_mul(1_000_000)),
            active_cpu_started_ns,
        })
    }

    pub fn canceled(&self) -> bool {
        self.host.canceled()
            || self
                .thread_group
                .as_ref()
                .is_some_and(|group| group.is_shutting_down())
    }

    pub fn has_active_cpu_limit(&self) -> bool {
        self.active_cpu_limit_ns.is_some()
    }

    pub fn remaining_active_cpu_ms(&self) -> Result<Option<u32>, HostServiceError> {
        self.active_cpu_limit_ns
            .map(|limit| {
                let observed = self
                    .host
                    .cpu_time_ns()?
                    .saturating_sub(self.active_cpu_started_ns);
                let remaining_ms = limit.saturating_sub(observed) / 1_000_000;
                if remaining_ms == 0 {
                    return Err(active_cpu_limit_error(limit, observed));
                }
                Ok(remaining_ms.min(u32::MAX as u64) as u32)
            })
            .transpose()
    }

    pub fn check_active_cpu_budget(&self) -> Result<(), HostServiceError> {
        if self.active_cpu_limit_ns.is_none() {
            return Ok(());
        }
        self.check_active_cpu_budget_at(self.host.cpu_time_ns()?)
    }

    pub fn check_active_cpu_budget_at(&self, now_ns: u64) -> Result<(), HostServiceError> {
        if let Some(limit_ns) = self.active_cpu_limit_ns {
            let observed_ns = now_ns.saturating_sub(self.active_cpu_started_ns);
            if observed_ns >= limit_ns {
                return Err(active_cpu_limit_error(limit_ns, observed_ns));
            }
        }
        Ok(())
    }

    /// Override the active CPU limit, for tests of budget exhaustion.
    #[doc(hidden)]
    pub fn set_active_cpu_limit_ns(&mut self, limit_ns: Option<u64>) {
        self.active_cpu_limit_ns = limit_ns;
    }
}

pub fn store_guest_environment(
    environment: &BTreeMap<String, String>,
    environment_is_guest_visible: bool,
) -> BTreeMap<String, String> {
    if environment_is_guest_visible {
        environment.clone()
    } else {
        guest_visible_wasm_env(environment)
    }
}

pub fn nul_terminated_strings<I, S>(
    values: I,
    field: &'static str,
) -> Result<Vec<Vec<u8>>, HostServiceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    values
        .into_iter()
        .map(|value| {
            let value = value.as_ref();
            if value.as_bytes().contains(&0) {
                return Err(HostServiceError::new(
                    "EINVAL",
                    format!("WebAssembly {field} value contains an interior NUL"),
                ));
            }
            let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
            Ok(bytes)
        })
        .collect()
}
