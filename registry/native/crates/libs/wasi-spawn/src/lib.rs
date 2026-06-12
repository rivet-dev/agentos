//! WASI process spawning via host_process FFI.
//!
//! Provides `WasiChild` — a synchronous child process handle with pipe-based
//! stdout/stderr capture, wait, and kill. Uses wasi-ext FFI directly instead
//! of std::process::Command for explicit control over pipe lifecycle.
//!
//! Designed for codex-rs WASI integration: replaces tokio::process::Command
//! on wasm32-wasip1 where tokio process/signal features are unavailable.

use std::io::{self, Read};
use std::mem::ManuallyDrop;

const MAX_ARG_COUNT: usize = 4096;
const MAX_ENV_COUNT: usize = 4096;
const MAX_SERIALIZED_BYTES: usize = 1024 * 1024;
const MAX_CWD_BYTES: usize = 4096;
const MAX_CAPTURED_STREAM_BYTES: usize = 16 * 1024 * 1024;
#[cfg(target_os = "wasi")]
const READY_STDOUT: u64 = 0;
#[cfg(target_os = "wasi")]
const READY_STDERR: u64 = 1;

/// Captured output from a child process.
pub struct WasiOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Handle to a spawned child process with pipe-based I/O capture.
///
/// Created by [`spawn_child`]. Owns the read ends of stdout/stderr pipes.
/// The write ends are closed in the parent after spawn (POSIX close-after-fork).
pub struct WasiChild {
    pid: u32,
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
    exited: bool,
}

/// Raw file descriptor type matching WASI u32 FDs.
type RawFd = u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CapturedStream {
    Stdout,
    Stderr,
}

fn errno_to_io_error(errno: wasi_ext::Errno) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("wasi errno {}", errno))
}

#[cfg(target_os = "wasi")]
fn wasi_errno_to_io_error(errno: wasi::Errno) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("wasi errno {}", errno.raw()))
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Read from a raw WASI file descriptor into a buffer.
///
/// Uses std::fs::File::from_raw_fd for WASI fd_read dispatch.
fn fd_read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    use std::os::fd::FromRawFd;
    // The caller owns this fd. This temporary File only routes through WASI fd_read.
    let file = unsafe { ManuallyDrop::new(std::fs::File::from_raw_fd(fd as i32)) };
    (&*file).read(buf)
}

/// Close a raw WASI file descriptor.
fn fd_close(fd: RawFd) {
    use std::os::fd::FromRawFd;
    // Safety: fd is a valid local FD. Drop closes it via WASI fd_close.
    drop(unsafe { std::fs::File::from_raw_fd(fd as i32) });
}

/// Serialize strings as null-separated byte buffer for proc_spawn.
fn serialize_null_separated(items: &[&str]) -> io::Result<Vec<u8>> {
    if items.len() > MAX_ARG_COUNT {
        return Err(invalid_input(format!(
            "argument count exceeds limit of {MAX_ARG_COUNT}"
        )));
    }

    let mut buf = Vec::new();
    for (i, item) in items.iter().enumerate() {
        validate_no_nul("argument", item)?;
        if i > 0 {
            push_serialized_byte(&mut buf, 0)?;
        }
        append_serialized(&mut buf, item.as_bytes())?;
    }
    Ok(buf)
}

/// Serialize environment as KEY=VALUE null-separated pairs for proc_spawn.
fn serialize_env(env: &[(&str, &str)]) -> io::Result<Vec<u8>> {
    if env.len() > MAX_ENV_COUNT {
        return Err(invalid_input(format!(
            "environment count exceeds limit of {MAX_ENV_COUNT}"
        )));
    }

    let mut buf = Vec::new();
    for (i, (key, value)) in env.iter().enumerate() {
        validate_env_key(key)?;
        validate_no_nul("environment value", value)?;
        if i > 0 {
            push_serialized_byte(&mut buf, 0)?;
        }
        append_serialized(&mut buf, key.as_bytes())?;
        push_serialized_byte(&mut buf, b'=')?;
        append_serialized(&mut buf, value.as_bytes())?;
    }
    Ok(buf)
}

fn validate_env_key(key: &str) -> io::Result<()> {
    if key.is_empty() {
        return Err(invalid_input("environment key must not be empty"));
    }
    validate_no_nul("environment key", key)?;
    if key.as_bytes().contains(&b'=') {
        return Err(invalid_input("environment key must not contain '='"));
    }
    Ok(())
}

fn validate_no_nul(label: &str, value: &str) -> io::Result<()> {
    if value.as_bytes().contains(&0) {
        return Err(invalid_input(format!("{label} must not contain NUL")));
    }
    Ok(())
}

fn validate_cwd(cwd: &str) -> io::Result<()> {
    validate_no_nul("cwd", cwd)?;
    if cwd.len() > MAX_CWD_BYTES {
        return Err(invalid_input(format!(
            "cwd exceeds limit of {MAX_CWD_BYTES} bytes"
        )));
    }
    Ok(())
}

fn push_serialized_byte(buf: &mut Vec<u8>, byte: u8) -> io::Result<()> {
    reserve_serialized(buf.len(), 1)?;
    buf.push(byte);
    Ok(())
}

fn append_serialized(buf: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    reserve_serialized(buf.len(), bytes.len())?;
    buf.extend_from_slice(bytes);
    Ok(())
}

fn reserve_serialized(current_len: usize, additional_len: usize) -> io::Result<()> {
    let next_len = current_len
        .checked_add(additional_len)
        .ok_or_else(|| invalid_input("serialized spawn data length overflowed"))?;
    if next_len > MAX_SERIALIZED_BYTES {
        return Err(invalid_input(format!(
            "serialized spawn data exceeds limit of {MAX_SERIALIZED_BYTES} bytes"
        )));
    }
    Ok(())
}

fn append_captured_stream_with_limit(
    output: &mut Vec<u8>,
    chunk: &[u8],
    limit: usize,
) -> io::Result<()> {
    let next_len = output
        .len()
        .checked_add(chunk.len())
        .ok_or_else(|| invalid_data("captured stream length overflowed"))?;
    if next_len > limit {
        return Err(invalid_data(format!(
            "captured stream exceeds limit of {limit} bytes"
        )));
    }
    output.extend_from_slice(chunk);
    Ok(())
}

fn read_captured_streams<R, W, C>(
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
    read_fd: R,
    wait_readable: W,
    cleanup: C,
) -> io::Result<(Vec<u8>, Vec<u8>)>
where
    R: FnMut(RawFd, &mut [u8]) -> io::Result<usize>,
    W: FnMut(Option<RawFd>, Option<RawFd>) -> io::Result<[Option<CapturedStream>; 2]>,
    C: FnMut(),
{
    read_captured_streams_with_limit(
        stdout_fd,
        stderr_fd,
        read_fd,
        wait_readable,
        cleanup,
        MAX_CAPTURED_STREAM_BYTES,
    )
}

fn read_captured_streams_with_limit<R, W, C>(
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
    mut read_fd: R,
    mut wait_readable: W,
    mut cleanup: C,
    limit: usize,
) -> io::Result<(Vec<u8>, Vec<u8>)>
where
    R: FnMut(RawFd, &mut [u8]) -> io::Result<usize>,
    W: FnMut(Option<RawFd>, Option<RawFd>) -> io::Result<[Option<CapturedStream>; 2]>,
    C: FnMut(),
{
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_done = stdout_fd.is_none();
    let mut stderr_done = stderr_fd.is_none();
    let mut buf = [0u8; 4096];

    while !stdout_done || !stderr_done {
        let active_stdout = if stdout_done { None } else { stdout_fd };
        let active_stderr = if stderr_done { None } else { stderr_fd };
        let ready = match wait_readable(active_stdout, active_stderr) {
            Ok(ready) => ready,
            Err(e) => {
                cleanup();
                return Err(e);
            }
        };

        let mut progressed = false;
        for stream in ready.into_iter().flatten() {
            let (fd, output, done) = match stream {
                CapturedStream::Stdout if !stdout_done => (
                    stdout_fd.expect("stdout fd is present while active"),
                    &mut stdout,
                    &mut stdout_done,
                ),
                CapturedStream::Stderr if !stderr_done => (
                    stderr_fd.expect("stderr fd is present while active"),
                    &mut stderr,
                    &mut stderr_done,
                ),
                CapturedStream::Stdout => continue,
                CapturedStream::Stderr => continue,
            };

            progressed = true;
            match read_fd(fd, &mut buf) {
                Ok(0) => {
                    *done = true;
                }
                Ok(n) => {
                    if let Err(e) = append_captured_stream_with_limit(output, &buf[..n], limit) {
                        cleanup();
                        return Err(e);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                    *done = true;
                }
                Err(e) => {
                    cleanup();
                    return Err(e);
                }
            }
        }

        if !progressed {
            cleanup();
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "no captured stream became readable",
            ));
        }
    }
    Ok((stdout, stderr))
}

#[cfg(target_os = "wasi")]
fn wait_readable_streams(
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
) -> io::Result<[Option<CapturedStream>; 2]> {
    let mut subscriptions = Vec::with_capacity(2);
    if let Some(fd) = stdout_fd {
        subscriptions.push(wasi::Subscription {
            userdata: READY_STDOUT,
            u: wasi::SubscriptionU {
                tag: wasi::EVENTTYPE_FD_READ.raw(),
                u: wasi::SubscriptionUU {
                    fd_read: wasi::SubscriptionFdReadwrite {
                        file_descriptor: fd,
                    },
                },
            },
        });
    }
    if let Some(fd) = stderr_fd {
        subscriptions.push(wasi::Subscription {
            userdata: READY_STDERR,
            u: wasi::SubscriptionU {
                tag: wasi::EVENTTYPE_FD_READ.raw(),
                u: wasi::SubscriptionUU {
                    fd_read: wasi::SubscriptionFdReadwrite {
                        file_descriptor: fd,
                    },
                },
            },
        });
    }

    if subscriptions.is_empty() {
        return Ok([None, None]);
    }

    let mut events = vec![unsafe { std::mem::zeroed::<wasi::Event>() }; subscriptions.len()];
    let ready_count = unsafe {
        wasi::poll_oneoff(
            subscriptions.as_ptr(),
            events.as_mut_ptr(),
            subscriptions.len(),
        )
    }
    .map_err(wasi_errno_to_io_error)?;
    if ready_count > events.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "poll returned too many events",
        ));
    }

    let mut ready = [None, None];
    for (i, event) in events.into_iter().take(ready_count).enumerate() {
        if event.error != wasi::ERRNO_SUCCESS {
            return Err(wasi_errno_to_io_error(event.error));
        }
        ready[i] = match event.userdata {
            READY_STDOUT => Some(CapturedStream::Stdout),
            READY_STDERR => Some(CapturedStream::Stderr),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "poll returned unknown stream",
                ));
            }
        };
    }
    Ok(ready)
}

#[cfg(not(target_os = "wasi"))]
fn wait_readable_streams(
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
) -> io::Result<[Option<CapturedStream>; 2]> {
    Ok([
        stdout_fd.map(|_| CapturedStream::Stdout),
        stderr_fd.map(|_| CapturedStream::Stderr),
    ])
}

fn wait_or_cleanup<C>(result: io::Result<i32>, cleanup: C) -> io::Result<i32>
where
    C: FnOnce(),
{
    match result {
        Ok(exit_code) => Ok(exit_code),
        Err(e) => {
            cleanup();
            Err(e)
        }
    }
}

fn spawn_child_with_stdin_fd(
    argv: &[&str],
    env: &[(&str, &str)],
    cwd: &str,
    stdin_fd: u32,
) -> io::Result<WasiChild> {
    if argv.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty argv"));
    }

    validate_cwd(cwd)?;
    let argv_buf = serialize_null_separated(argv)?;
    let envp_buf = serialize_env(env)?;

    // Create stdout pipe
    let (stdout_read, stdout_write) = wasi_ext::pipe().map_err(errno_to_io_error)?;

    // Create stderr pipe
    let (stderr_read, stderr_write) = wasi_ext::pipe().map_err(|e| {
        fd_close(stdout_read);
        fd_close(stdout_write);
        errno_to_io_error(e)
    })?;

    // Spawn child with pipe-captured stdout/stderr and caller-selected stdin.
    let result = wasi_ext::spawn(
        &argv_buf,
        &envp_buf,
        stdin_fd,
        stdout_write,
        stderr_write,
        cwd.as_bytes(),
    );

    // Close write ends in parent (POSIX close-after-fork)
    fd_close(stdout_write);
    fd_close(stderr_write);

    match result {
        Ok(pid) => Ok(WasiChild {
            pid,
            stdout_fd: Some(stdout_read),
            stderr_fd: Some(stderr_read),
            exited: false,
        }),
        Err(errno) => {
            fd_close(stdout_read);
            fd_close(stderr_read);
            Err(errno_to_io_error(errno))
        }
    }
}

/// Spawn a child process with pipe-captured stdout and stderr.
///
/// Creates pipes for stdout/stderr, spawns the child via host_process FFI,
/// and returns a `WasiChild` handle. The parent's stdin is inherited.
///
/// # Arguments
/// * `argv` - Command and arguments (argv[0] is the program name)
/// * `env` - Environment variable pairs (empty inherits parent env via host)
/// * `cwd` - Working directory for the child
pub fn spawn_child(argv: &[&str], env: &[(&str, &str)], cwd: &str) -> io::Result<WasiChild> {
    spawn_child_with_stdin_fd(argv, env, cwd, 0)
}

/// Spawn a child process with stdin ignored and stdout/stderr captured.
///
/// This is appropriate for agent tool calls or other subprocesses that must not
/// inherit an interactive control stream from the parent process.
pub fn spawn_child_ignore_stdin(
    argv: &[&str],
    env: &[(&str, &str)],
    cwd: &str,
) -> io::Result<WasiChild> {
    spawn_child_with_stdin_fd(argv, env, cwd, u32::MAX)
}

/// Spawn a child process inheriting all stdio (no pipe capture).
///
/// Useful for interactive commands where output should go directly to
/// the parent's terminal.
pub fn spawn_child_inherit(
    argv: &[&str],
    env: &[(&str, &str)],
    cwd: &str,
) -> io::Result<WasiChild> {
    if argv.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty argv"));
    }

    validate_cwd(cwd)?;
    let argv_buf = serialize_null_separated(argv)?;
    let envp_buf = serialize_env(env)?;

    let pid = wasi_ext::spawn(
        &argv_buf,
        &envp_buf,
        0,
        1,
        2, // inherit all stdio
        cwd.as_bytes(),
    )
    .map_err(errno_to_io_error)?;

    Ok(WasiChild {
        pid,
        stdout_fd: None,
        stderr_fd: None,
        exited: false,
    })
}

impl WasiChild {
    /// Get the child's virtual PID.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Read from the child's stdout pipe.
    ///
    /// Returns 0 bytes when the pipe is closed (child exited or closed stdout).
    pub fn read_stdout(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.stdout_fd {
            Some(fd) => fd_read(fd, buf),
            None => Ok(0),
        }
    }

    /// Read from the child's stderr pipe.
    ///
    /// Returns 0 bytes when the pipe is closed (child exited or closed stderr).
    pub fn read_stderr(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.stderr_fd {
            Some(fd) => fd_read(fd, buf),
            None => Ok(0),
        }
    }

    /// Wait for the child to exit. Returns the exit code.
    ///
    /// Blocks via host_process_waitpid (Atomics.wait on host side).
    pub fn wait(&mut self) -> io::Result<i32> {
        if self.exited {
            return Err(io::Error::new(io::ErrorKind::Other, "already waited"));
        }

        let (status, _actual_pid) = wasi_ext::waitpid(self.pid, 0).map_err(errno_to_io_error)?;

        self.exited = true;

        // Decode exit status using bash 128+signal convention
        // Normal exit: status is the exit code directly
        // Signal kill: status is 128 + signal number
        Ok(status as i32)
    }

    /// Send a signal to the child process.
    ///
    /// Common signals: SIGTERM (15), SIGKILL (9).
    pub fn kill(&mut self, signal: u32) -> io::Result<()> {
        wasi_ext::kill(self.pid, signal).map_err(errno_to_io_error)
    }

    /// Send SIGTERM to the child process.
    pub fn terminate(&mut self) -> io::Result<()> {
        self.kill(15)
    }

    fn kill_and_reap(&mut self) {
        if self.exited {
            return;
        }

        let _ = wasi_ext::kill(self.pid, 9);
        if wasi_ext::waitpid(self.pid, 0).is_ok() {
            self.exited = true;
        }
    }

    fn close_output_fds(&mut self) {
        if let Some(fd) = self.stdout_fd.take() {
            fd_close(fd);
        }
        if let Some(fd) = self.stderr_fd.take() {
            fd_close(fd);
        }
    }

    /// Read all stdout and stderr, then wait for exit.
    ///
    /// Drains readable stdout and stderr events until both streams close, then waits.
    pub fn consume_output(&mut self) -> io::Result<WasiOutput> {
        let stdout_fd = self.stdout_fd;
        let stderr_fd = self.stderr_fd;
        let (stdout, stderr) =
            read_captured_streams(stdout_fd, stderr_fd, fd_read, wait_readable_streams, || {
                self.kill_and_reap()
            })?;

        let wait_result = self.wait();
        let exit_code = wait_or_cleanup(wait_result, || self.kill_and_reap())?;

        Ok(WasiOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

impl Drop for WasiChild {
    fn drop(&mut self) {
        self.kill_and_reap();
        self.close_output_fds();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_interior_nul_in_arguments() {
        let err = serialize_null_separated(&["echo", "a\0b"]).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_invalid_environment_keys() {
        let err = serialize_env(&[("A=B", "value")]).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_oversized_serialized_data() {
        let oversized = "x".repeat(MAX_SERIALIZED_BYTES + 1);
        let err = serialize_null_separated(&[&oversized]).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_oversized_captured_stream() {
        let mut output = vec![b'x'; 8];
        let err = append_captured_stream_with_limit(&mut output, b"y", 8).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(output.len(), 8);
    }

    #[test]
    fn capture_helper_cleans_up_on_output_limit() {
        let mut reads = 0;
        let mut cleanup_calls = 0;
        let err = read_captured_streams_with_limit(
            Some(7),
            None,
            |_fd, buf| {
                reads += 1;
                buf[..4].copy_from_slice(b"xxxx");
                Ok(4)
            },
            |stdout_fd, stderr_fd| {
                assert_eq!(stdout_fd, Some(7));
                assert_eq!(stderr_fd, None);
                Ok([Some(CapturedStream::Stdout), None])
            },
            || cleanup_calls += 1,
            8,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(reads, 3);
        assert_eq!(cleanup_calls, 1);
    }

    #[test]
    fn capture_helper_cleans_up_on_read_error() {
        let mut cleanup_calls = 0;
        let err = read_captured_streams_with_limit(
            Some(7),
            None,
            |_fd, _buf| Err(io::Error::new(io::ErrorKind::PermissionDenied, "boom")),
            |_stdout_fd, _stderr_fd| Ok([Some(CapturedStream::Stdout), None]),
            || cleanup_calls += 1,
            8,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(cleanup_calls, 1);
    }

    #[test]
    fn capture_helper_interleaves_stdout_and_stderr() {
        let mut ready_calls = 0;
        let mut stdout_reads = 0;
        let mut stderr_reads = 0;
        let (stdout, stderr) = read_captured_streams_with_limit(
            Some(1),
            Some(2),
            |fd, buf| match fd {
                1 => {
                    stdout_reads += 1;
                    if stdout_reads > 1 {
                        return Ok(0);
                    }
                    buf[..3].copy_from_slice(b"out");
                    Ok(3)
                }
                2 => {
                    stderr_reads += 1;
                    if stderr_reads > 1 {
                        return Ok(0);
                    }
                    buf[..3].copy_from_slice(b"err");
                    Ok(3)
                }
                _ => unreachable!(),
            },
            |stdout_fd, stderr_fd| {
                ready_calls += 1;
                match ready_calls {
                    1 => {
                        assert_eq!(stdout_fd, Some(1));
                        assert_eq!(stderr_fd, Some(2));
                        Ok([Some(CapturedStream::Stderr), None])
                    }
                    2 => {
                        assert_eq!(stdout_fd, Some(1));
                        assert_eq!(stderr_fd, Some(2));
                        Ok([Some(CapturedStream::Stdout), None])
                    }
                    3 => Ok([Some(CapturedStream::Stderr), None]),
                    4 => Ok([Some(CapturedStream::Stdout), None]),
                    _ => unreachable!(),
                }
            },
            || unreachable!(),
            16,
        )
        .unwrap();

        assert_eq!(stdout, b"out");
        assert_eq!(stderr, b"err");
        assert_eq!(ready_calls, 4);
    }

    #[test]
    fn wait_error_runs_cleanup() {
        let mut cleanup_calls = 0;
        let err = wait_or_cleanup(
            Err(io::Error::new(io::ErrorKind::NotFound, "missing")),
            || cleanup_calls += 1,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(cleanup_calls, 1);
    }
}
