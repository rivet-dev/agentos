# Git WASM Compatibility

Git (GPL-2.0) cannot be vendored due to license restrictions.
This project uses a clean-room, Apache-2.0 licensed reimplementation
of git plumbing commands.

## WASM-Incompatible Patterns in Git Source

The following patterns in upstream git are incompatible with WASM/WASI
and were avoided in our clean-room implementation:

### 1. fork+exec in run_command.c
Git's `run_command()` uses `fork()`+`exec()` extensively for spawning
child processes (hooks, filters, remotes). WASI has no `fork()`.
**Our approach:** Use `posix_spawn()` via `host_process` imports when
process spawning is needed in future commands.

### 2. mmap in wrapper.c
Git uses `mmap()` for memory-mapped file I/O (packfiles, index).
WASI has limited `mmap()` support.
**Our approach:** Use standard `malloc()`+`read()` for all file I/O.
Git upstream supports this via `NO_MMAP=1`.

### 3. Signal handlers (sigaction for SIGPIPE, SIGCHLD)
Git registers signal handlers for `SIGPIPE` (ignore broken pipe) and
`SIGCHLD` (reap child processes). WASI has no signal support.
**Our approach:** No signal handlers needed — WASI processes don't
receive signals, and our process model handles cleanup via the kernel.
