//! wasm32-wasip1 process `imp` for tokio (DRAFT — pipeline-only codex port).
//!
//! Status: starting artifact for `patches/crates/tokio/`. Needs compile-test
//! iteration against tokio 1.52.x internals (trait/type exactness) before being
//! captured as the actual .patch. See ~/tmp/agent-e2e-checklist.md.
//!
//! Approach: route to the PATCHED `std::process` (host_process bridge). The VM is
//! single-threaded, so `Child::poll` must NOT block on the synchronous `wait()`:
//! that pins the only executor thread for the child's whole lifetime, starving
//! every other task (the agent submission loop, the concurrent stdout/stderr
//! drains `output()`/`wait_with_output()` rely on) and deadlocking the runtime.
//! Instead `Child::poll` polls the child non-blockingly via std `try_wait()`
//! (host_process `proc_waitpid` with WNOHANG) and yields until it exits, exactly
//! like a normal Linux async runtime — the child runs for real in the host, we
//! just cooperatively await its exit. `ChildStdio` reads/writes the blocking OS
//! fd and resolves on first poll (a guest pipe read returns available bytes or
//! EOF; the cooperative `wait` is what keeps the executor live).
//! No SIGCHLD / orphan reaping / mio / pidfd on wasi.
//!
//! Wiring: in `src/process/mod.rs`, add alongside the unix/windows imp selection:
//!   #[path = "wasi.rs"] #[cfg(target_os = "wasi")] mod imp;
//! and add `#[cfg(target_os = "wasi")] use imp::*;` to the imp re-export block.
//! Also remove `#[cfg(not(target_os = "wasi"))]` from `cfg_process!` (macros/cfg.rs)
//! and build the wasm target with `RUSTFLAGS="--cfg tokio_unstable"`.

use crate::io::AsyncRead;
use crate::io::AsyncWrite;
use crate::io::ReadBuf;
use crate::process::kill::Kill;
use crate::process::SpawnedChild;

use std::fmt;
use std::future::Future;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::IntoRawFd;
use std::os::fd::OwnedFd;
use std::os::fd::RawFd;
use std::pin::Pin;
use std::process::Child as StdChild;
use std::process::ExitStatus;
use std::process::Stdio;
use std::task::Context;
use std::task::Poll;

/// No-op orphan queue: wasm32-wasip1 has no SIGCHLD, and the host reaps the
/// child when `wait()` returns. Kept to satisfy the imp surface.
#[derive(Debug)]
pub(crate) struct GlobalOrphanQueue;

impl GlobalOrphanQueue {
    pub(crate) fn reap_orphans() {}
}

pub(crate) struct Child {
    inner: StdChild,
}

impl fmt::Debug for Child {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Child").field("pid", &self.id()).finish()
    }
}

pub(crate) fn build_child(mut child: StdChild) -> io::Result<SpawnedChild> {
    let stdin = child.stdin.take().map(stdio).transpose()?;
    let stdout = child.stdout.take().map(stdio).transpose()?;
    let stderr = child.stderr.take().map(stdio).transpose()?;
    Ok(SpawnedChild {
        child: Child { inner: child },
        stdin,
        stdout,
        stderr,
    })
}

impl Child {
    pub(crate) fn id(&self) -> u32 {
        self.inner.id()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }
}

impl Kill for Child {
    fn kill(&mut self) -> io::Result<()> {
        self.inner.kill()
    }
}

impl Future for Child {
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Single-threaded VM: the child runs for real in the host. Do NOT block
        // the only executor thread on the synchronous `wait()` — that starves the
        // rest of the runtime (the agent submission loop, the concurrent
        // stdout/stderr drains that `output()` polls alongside this future) and
        // deadlocks the agent turn. Poll non-blockingly (host_process WNOHANG via
        // std `try_wait`) and yield until the child exits, like a normal async
        // runtime reaping a child.
        match self.get_mut().inner.try_wait() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Ok(None) => {
                // Still running: re-poll on the next runtime tick so other tasks
                // (and this child's output drains) get to run in between.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// Async-shaped child pipe backed by a blocking OS fd. `poll_*` resolve on the
/// first poll (single-threaded VM).
#[derive(Debug)]
pub(crate) struct ChildStdio {
    fd: OwnedFd,
}

impl AsRawFd for ChildStdio {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl AsyncRead for ChildStdio {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        let mut file = unsafe { std::fs::File::from_raw_fd(me.fd.as_raw_fd()) };
        let res = file.read(buf.initialize_unfilled());
        let _ = file.into_raw_fd(); // don't close the borrowed fd
        match res {
            Ok(n) => {
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for ChildStdio {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let me = self.get_mut();
        let mut file = unsafe { std::fs::File::from_raw_fd(me.fd.as_raw_fd()) };
        let res = file.write(buf);
        let _ = file.into_raw_fd();
        Poll::Ready(res)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub(crate) fn stdio<T: Into<OwnedFd>>(io: T) -> io::Result<ChildStdio> {
    let fd = io.into();
    let file = std::fs::File::from(fd);
    {
        use std::os::wasi::fs::FileExt;
        const FDFLAGS_NONBLOCK: u16 = 1 << 2;
        file.fdstat_set_flags(FDFLAGS_NONBLOCK)?;
    }
    Ok(ChildStdio { fd: file.into() })
}

pub(crate) fn convert_to_stdio(io: ChildStdio) -> io::Result<Stdio> {
    // wasi `Stdio` is `From<File>` (not `From<OwnedFd>`); `File` is `From<OwnedFd>`.
    Ok(Stdio::from(std::fs::File::from(io.fd)))
}
