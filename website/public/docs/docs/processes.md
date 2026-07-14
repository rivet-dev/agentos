# Processes & Shell

Execute commands, spawn long-running processes, and open interactive shells in agentOS VMs.

Run commands with one-shot `exec`, spawn long-running processes with streaming stdout/stderr and stdin, manage their lifecycle (stop, kill, wait, inspect), open interactive PTY-backed shells, and inspect the flat sidecar-owned process table with `allProcesses()`.

## One-shot execution

Use `exec` to run a command and wait for completion. Returns stdout, stderr, and exit code.

## Spawn a long-running process

Use `spawn` for processes that run in the background. Output is streamed via `processOutput` and `processExit` events.

## Write to stdin

Send input to a running process.

## Process lifecycle

## Interactive shells

Open an interactive shell with PTY support. Shell data is streamed via `shellData` events.