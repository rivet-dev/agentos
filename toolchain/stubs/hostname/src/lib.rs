//! WASM-compatible stub for the hostname crate.
//! Returns a fixed hostname since WASI has no hostname syscall.

use std::ffi::OsString;
use std::io;

/// Return the system hostname.
/// On WASI, always returns "wasm-host".
pub fn get() -> io::Result<OsString> {
    Ok(OsString::from("wasm-host"))
}
