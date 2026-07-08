//! Shim implementations for commands that require subprocess support.
//!
//! Commands like `env` and `timeout` need to spawn child processes
//! via wasi-ext syscalls (through std::process::Command patches)
//! rather than using uutils versions.

pub mod env;
pub mod nice;
pub mod nohup;
pub mod stdbuf;
pub mod timeout;
pub mod which;
pub mod xargs;
