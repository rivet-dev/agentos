//! WASI PTY-based process management via host_process FFI.
//!
//! Provides [`WasiPtyChild`] — an interactive process handle using a pseudo-terminal
//! instead of pipes. This is the WASI equivalent of `ExecCommandSession` from
//! `codex-utils-pty` (which wraps `portable-pty` on native platforms).
//!
//! Key difference from [`wasi_spawn::WasiChild`]:
//! - Uses a PTY master/slave pair instead of separate stdout/stderr pipes
//! - All child output (stdout + stderr) is multiplexed through the PTY master
//! - Supports interactive programs that require a terminal (e.g., editors, shells)
//! - The PTY provides terminal emulation (line discipline, echo, signals)

use std::io::{self, Read, Write};
use std::mem::ManuallyDrop;

const MAX_ARG_COUNT: usize = 4096;
const MAX_ENV_COUNT: usize = 4096;
const MAX_SERIALIZED_BYTES: usize = 1024 * 1024;
const MAX_CWD_BYTES: usize = 4096;
const MAX_CAPTURED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Handle to a spawned process connected via a pseudo-terminal.
///
/// Created by [`spawn_pty`]. The child process has the PTY slave as its
/// stdin/stdout/stderr. The parent reads and writes via the PTY master FD.
///
/// This is the WASI equivalent of `SpawnedPty` from `codex-utils-pty`.
pub struct WasiSpawnedPty {
    master_fd: RawFd,
}

/// Interactive process session using a PTY.
///
/// Created by [`spawn_session`]. Wraps a [`WasiSpawnedPty`] and the child
/// process handle, providing a unified API for interactive process management.
///
/// This is the WASI equivalent of `ExecCommandSession` from `codex-utils-pty`.
pub struct WasiPtyChild {
    pid: u32,
    master_fd: RawFd,
    exited: bool,
}

type RawFd = u32;

fn errno_to_io_error(errno: wasi_ext::Errno) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("wasi errno {}", errno))
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Read from a raw WASI file descriptor into a buffer.
fn fd_read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    use std::os::fd::FromRawFd;
    // The caller owns this fd. This temporary File only routes through WASI fd_read.
    let file = unsafe { ManuallyDrop::new(std::fs::File::from_raw_fd(fd as i32)) };
    (&*file).read(buf)
}

/// Write to a raw WASI file descriptor from a buffer.
fn fd_write(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    use std::os::fd::FromRawFd;
    // The caller owns this fd. This temporary File only routes through WASI fd_write.
    let file = unsafe { ManuallyDrop::new(std::fs::File::from_raw_fd(fd as i32)) };
    (&*file).write(buf)
}

/// Close a raw WASI file descriptor.
fn fd_close(fd: RawFd) {
    use std::os::fd::FromRawFd;
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

fn append_captured_output_with_limit(
    stdout: &mut Vec<u8>,
    chunk: &[u8],
    limit: usize,
) -> io::Result<()> {
    let next_len = stdout
        .len()
        .checked_add(chunk.len())
        .ok_or_else(|| invalid_data("captured PTY output length overflowed"))?;
    if next_len > limit {
        return Err(invalid_data(format!(
            "captured PTY output exceeds limit of {limit} bytes"
        )));
    }
    stdout.extend_from_slice(chunk);
    Ok(())
}

fn read_captured_output<R, C>(read_output: R, cleanup: C) -> io::Result<Vec<u8>>
where
    R: FnMut(&mut [u8]) -> io::Result<usize>,
    C: FnMut(),
{
    read_captured_output_with_limit(read_output, cleanup, MAX_CAPTURED_OUTPUT_BYTES)
}

fn read_captured_output_with_limit<R, C>(
    mut read_output: R,
    mut cleanup: C,
    limit: usize,
) -> io::Result<Vec<u8>>
where
    R: FnMut(&mut [u8]) -> io::Result<usize>,
    C: FnMut(),
{
    let mut stdout = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match read_output(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = append_captured_output_with_limit(&mut stdout, &buf[..n], limit) {
                    cleanup();
                    return Err(e);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
            Err(e) => {
                cleanup();
                return Err(e);
            }
        }
    }
    Ok(stdout)
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

/// Spawn a child process connected via a PTY.
///
/// Allocates a PTY master/slave pair, spawns the child with the slave FD as
/// stdin/stdout/stderr, and returns a [`WasiPtyChild`] handle. The slave FD
/// is closed in the parent after spawn (POSIX close-after-fork).
///
/// # Arguments
/// * `argv` - Command and arguments (argv[0] is the program name)
/// * `env` - Environment variable pairs (empty inherits parent env via host)
/// * `cwd` - Working directory for the child
pub fn spawn_session(argv: &[&str], env: &[(&str, &str)], cwd: &str) -> io::Result<WasiPtyChild> {
    if argv.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty argv"));
    }

    validate_cwd(cwd)?;
    let argv_buf = serialize_null_separated(argv)?;
    let envp_buf = serialize_env(env)?;

    // Allocate PTY master/slave pair via kernel
    let (master_fd, slave_fd) = wasi_ext::openpty().map_err(errno_to_io_error)?;

    // Spawn child with PTY slave as all stdio
    let result = wasi_ext::spawn(
        &argv_buf,
        &envp_buf,
        slave_fd,
        slave_fd,
        slave_fd,
        cwd.as_bytes(),
    );

    // Close slave FD in parent (POSIX close-after-fork) — child has its own ref
    fd_close(slave_fd);

    match result {
        Ok(pid) => Ok(WasiPtyChild {
            pid,
            master_fd,
            exited: false,
        }),
        Err(errno) => {
            fd_close(master_fd);
            Err(errno_to_io_error(errno))
        }
    }
}

/// Allocate a PTY pair without spawning a process.
///
/// Returns a [`WasiSpawnedPty`] for the master end. The slave FD is returned
/// separately so the caller can pass it to [`wasi_spawn::spawn_child`] or
/// use it directly.
pub fn open_pty() -> io::Result<(WasiSpawnedPty, RawFd)> {
    let (master_fd, slave_fd) = wasi_ext::openpty().map_err(errno_to_io_error)?;

    Ok((WasiSpawnedPty { master_fd }, slave_fd))
}

impl WasiSpawnedPty {
    /// Get the master FD for direct I/O.
    pub fn master_fd(&self) -> RawFd {
        self.master_fd
    }

    /// Read output from the PTY master (data written by the child).
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        fd_read(self.master_fd, buf)
    }

    /// Write input to the PTY master (delivered to the child's stdin).
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        fd_write(self.master_fd, buf)
    }
}

impl Drop for WasiSpawnedPty {
    fn drop(&mut self) {
        fd_close(self.master_fd);
    }
}

impl WasiPtyChild {
    /// Get the child's virtual PID.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Get the PTY master FD for direct I/O if needed.
    pub fn master_fd(&self) -> RawFd {
        self.master_fd
    }

    /// Read output from the child via the PTY master.
    ///
    /// All child output (stdout and stderr) is multiplexed through the PTY.
    /// Returns 0 bytes when the PTY slave is closed (child exited).
    pub fn read_output(&self, buf: &mut [u8]) -> io::Result<usize> {
        fd_read(self.master_fd, buf)
    }

    /// Write input to the child via the PTY master.
    ///
    /// Data is delivered to the child's stdin through the PTY line discipline.
    pub fn write_stdin(&self, buf: &[u8]) -> io::Result<usize> {
        fd_write(self.master_fd, buf)
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
        Ok(status as i32)
    }

    /// Send a signal to the child process.
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

    /// Read all output from the PTY, then wait for exit.
    ///
    /// Reads output until the PTY master gets EOF (child closed slave),
    /// then waits for the child to exit.
    pub fn consume_output(&mut self) -> io::Result<wasi_spawn::WasiOutput> {
        let master_fd = self.master_fd;
        let stdout = read_captured_output(|buf| fd_read(master_fd, buf), || self.kill_and_reap())?;

        let wait_result = self.wait();
        let exit_code = wait_or_cleanup(wait_result, || self.kill_and_reap())?;

        // PTY multiplexes stdout+stderr, so stderr is empty
        Ok(wasi_spawn::WasiOutput {
            stdout,
            stderr: Vec::new(),
            exit_code,
        })
    }
}

impl Drop for WasiPtyChild {
    fn drop(&mut self) {
        self.kill_and_reap();
        fd_close(self.master_fd);
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
    fn appends_captured_output_until_limit() {
        let mut output = vec![b'x'; MAX_CAPTURED_OUTPUT_BYTES - 1];

        append_captured_output_with_limit(&mut output, b"y", MAX_CAPTURED_OUTPUT_BYTES).unwrap();

        assert_eq!(output.len(), MAX_CAPTURED_OUTPUT_BYTES);
    }

    #[test]
    fn rejects_oversized_captured_output() {
        let mut output = vec![b'x'; MAX_CAPTURED_OUTPUT_BYTES];
        let err = append_captured_output_with_limit(&mut output, b"y", MAX_CAPTURED_OUTPUT_BYTES)
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(output.len(), MAX_CAPTURED_OUTPUT_BYTES);
    }

    #[test]
    fn consume_helper_cleans_up_on_output_limit() {
        let mut reads = 0;
        let mut cleanup_calls = 0;
        let err = read_captured_output_with_limit(
            |buf| {
                reads += 1;
                buf[..4].copy_from_slice(b"xxxx");
                Ok(4)
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
    fn consume_helper_cleans_up_on_read_error() {
        let mut cleanup_calls = 0;
        let err = read_captured_output_with_limit(
            |_buf| Err(io::Error::new(io::ErrorKind::PermissionDenied, "boom")),
            || cleanup_calls += 1,
            8,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(cleanup_calls, 1);
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
