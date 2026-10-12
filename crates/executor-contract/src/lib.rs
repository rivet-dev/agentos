#![deny(unsafe_code)]

//! Runtime- and engine-neutral contracts between agentOS executors and the
//! sidecar-owned kernel services.

pub mod backend;
mod guest;
pub mod host;
mod signal;

pub use guest::{GuestBridgeLimits, GuestRuntimeConfig, HostRpcRequest};
pub use signal::{ExecutionSignalDispositionAction, ExecutionSignalHandlerRegistration};
