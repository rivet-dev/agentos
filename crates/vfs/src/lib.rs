#![deny(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
pub mod adapter;
pub mod engine;
pub mod posix;
