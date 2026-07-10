//! WASI-specific extensions to primitives in the [`std::process`] module.
//!
//! Mirrors `os/unix/process.rs`' child-pipe fd traits for wasm32-wasip1 so that
//! `tokio::process` (and other fd-extracting code) can reach the parent-side
//! pipe ends of a spawned child. (secure-exec pipeline-only codex port.)
//!
//! [`std::process`]: crate::process

#![stable(feature = "rust1", since = "1.0.0")]

use crate::os::wasi::io::{AsFd, AsRawFd, BorrowedFd, IntoRawFd, OwnedFd, RawFd};
use crate::process;
use crate::sys::{AsInner, FromInner, IntoInner};

macro_rules! impl_child_pipe_fd {
    ($t:ty) => {
        #[stable(feature = "process_extensions", since = "1.2.0")]
        impl AsRawFd for $t {
            #[inline]
            fn as_raw_fd(&self) -> RawFd {
                self.as_inner().as_fd().as_raw_fd()
            }
        }

        #[stable(feature = "into_raw_os", since = "1.4.0")]
        impl IntoRawFd for $t {
            #[inline]
            fn into_raw_fd(self) -> RawFd {
                self.into_inner().into_inner().into_raw_fd()
            }
        }

        #[stable(feature = "io_safety", since = "1.63.0")]
        impl AsFd for $t {
            #[inline]
            fn as_fd(&self) -> BorrowedFd<'_> {
                self.as_inner().as_fd()
            }
        }

        #[stable(feature = "io_safety", since = "1.63.0")]
        impl From<$t> for OwnedFd {
            #[inline]
            fn from(child: $t) -> OwnedFd {
                child.into_inner().into_inner()
            }
        }
    };
}

impl_child_pipe_fd!(process::ChildStdin);
impl_child_pipe_fd!(process::ChildStdout);
impl_child_pipe_fd!(process::ChildStderr);

/// WASI-specific extension to construct an [`ExitStatus`] from a raw code,
/// mirroring `std::os::unix::process::ExitStatusExt::from_raw`. (secure-exec
/// pipeline-only codex port — codex's synthetic exit statuses need this.)
#[stable(feature = "rust1", since = "1.0.0")]
pub trait ExitStatusExt {
    /// Construct an `ExitStatus` from the given raw code.
    #[stable(feature = "exit_status_from", since = "1.12.0")]
    fn from_raw(raw: i32) -> Self;

    /// If the process was terminated by a signal, returns that signal.
    ///
    /// AgentOS process statuses are reported as exit codes by the wasi-spawn
    /// broker, so there is no POSIX signal payload to expose here.
    #[stable(feature = "process_extensions", since = "1.2.0")]
    fn signal(&self) -> Option<i32>;
}

#[stable(feature = "exit_status_from", since = "1.12.0")]
impl ExitStatusExt for process::ExitStatus {
    fn from_raw(raw: i32) -> Self {
        process::ExitStatus::from_inner(crate::sys::process::ExitStatus::from(raw))
    }

    fn signal(&self) -> Option<i32> {
        None
    }
}
