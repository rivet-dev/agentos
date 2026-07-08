//! WASM-compatible stub for the ctrlc crate.
//! Signal handling is not available in WASM.

use std::fmt;
use std::io;

/// Ctrl-C error.
#[derive(Debug)]
pub enum Error {
    /// Ctrl-C signal handler already registered.
    MultipleHandlers,
    /// Unexpected system error.
    System(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::MultipleHandlers => write!(f, "Ctrl-C signal handler already registered"),
            Error::System(e) => write!(f, "Ctrl-C system error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

/// Platform-specific signal type (stub for WASM).
pub type Signal = i32;

/// Signal type enum.
#[derive(Debug)]
pub enum SignalType {
    /// Ctrl-C
    Ctrlc,
    /// Program termination
    Termination,
    /// Other signal
    Other(Signal),
}

fn unsupported_signal_registration() -> Error {
    Error::System(io::Error::new(
        io::ErrorKind::Unsupported,
        "signal handlers are not supported on WASI",
    ))
}

/// Register signal handler for Ctrl-C.
pub fn set_handler<F>(_user_handler: F) -> Result<(), Error>
where
    F: FnMut() + 'static + Send,
{
    Err(unsupported_signal_registration())
}

/// Register signal handler, erroring if one already exists.
pub fn try_set_handler<F>(_user_handler: F) -> Result<(), Error>
where
    F: FnMut() + 'static + Send,
{
    Err(unsupported_signal_registration())
}
