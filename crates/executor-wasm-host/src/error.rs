use crate::backend::HostServiceError;
use std::any::Any;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Why a host import stopped the guest or failed.
///
/// `Exit` and `ExecReplaced` are not failures: they unwind the guest after
/// `proc_exit` or a committed exec. Engines map them back to their own
/// unwinding mechanism; their `Display` text is the sentinel the Wasmtime
/// executor has always used.
pub enum Error {
    Exit(i32),
    ExecReplaced,
    Host(HostServiceError),
    Message(String),
    /// An engine error raised while the handler called back into the guest,
    /// carried unchanged so the engine can recover its original form.
    Engine(Box<dyn Any + Send + Sync>),
}

impl Error {
    pub fn message(message: impl Into<String>) -> Self {
        Error::Message(message.into())
    }

    /// The host error this wraps, if any.
    pub fn host_error(&self) -> Option<&HostServiceError> {
        match self {
            Error::Host(error) => Some(error),
            _ => None,
        }
    }
}

impl From<HostServiceError> for Error {
    fn from(error: HostServiceError) -> Self {
        Error::Host(error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Exit(code) => write!(formatter, "agentos:wasi-exit:{code}"),
            Error::ExecReplaced => formatter.write_str("agentos:exec-replaced"),
            Error::Host(error) => write!(formatter, "{error}"),
            Error::Message(message) => formatter.write_str(message),
            Error::Engine(_) => formatter.write_str("guest call failed"),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Exit(code) => write!(formatter, "Exit({code})"),
            Error::ExecReplaced => formatter.write_str("ExecReplaced"),
            Error::Host(error) => write!(formatter, "Host({error:?})"),
            Error::Message(message) => write!(formatter, "Message({message:?})"),
            Error::Engine(_) => formatter.write_str("Engine(..)"),
        }
    }
}

/// Build an [`Error::Message`] with `format!` syntax.
#[macro_export]
macro_rules! format_err {
    ($($argument:tt)*) => {
        $crate::Error::Message(format!($($argument)*))
    };
}
