//! Engine-neutral agentOS WebAssembly host imports.
//!
//! The V8 and Wasmtime executors bind the same handlers, so guest-visible
//! behavior cannot diverge between engines. An engine implements
//! [`WasmGuest`] to give the handlers guest memory, the per-execution state,
//! and the few operations that need the engine itself.

#![deny(unsafe_code)]

pub mod abi {
    pub use agentos_executor_wasm_abi::abi::*;
}

pub mod backend {
    pub use agentos_executor_contract::backend::*;
}

pub mod host {
    pub use agentos_executor_contract::host::*;
}

mod client;
mod diagnostics;
mod error;
mod event;
mod guest;
pub mod image;
pub mod imports;
pub mod memory;
mod state;
mod value;

pub use client::{
    active_cpu_limit_error, cpu_time_ns, max_host_reply_bytes, WasmHostClient,
    DEFAULT_MAX_HOST_REPLY_BYTES,
};
pub use diagnostics::{ExecutionDiagnostics, PHASE_METRICS_PREFIX};
pub use error::{Error, Result};
pub use event::QueuedWasmEvent;
pub use guest::{BoxFuture, ExecCpuSample, ThreadGroupHooks, WasmGuest, WorkerTransport};
pub use state::{nul_terminated_strings, store_guest_environment, HostState, PendingExecReplacement};
pub use value::Val;
