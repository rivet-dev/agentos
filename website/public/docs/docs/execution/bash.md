# Bash

Run shell commands and arbitrary argv operations through the AgentOS execution lifecycle.

Shell commands are the simplest AgentOS execution surface. Use `process.exec()`
for shell syntax such as pipes and redirects, and `process.execFile()` for
injection-safe command arguments. Both use the same execution lifecycle as
JavaScript, TypeScript, Python, and package workflows.

## Run commands

`process.exec()` runs a command through the VM's configured shell and returns
its outcome, exit code, stdout, stderr, and `executionId`. The command runs
inside the VM, never in the host shell.

Prefer `process.execFile()` when command names or arguments come from data. Each
argument is transferred separately and AgentOS does not interpolate it into a
shell command. Keep `process.exec()` for workflows that intentionally need
shell syntax.

## Detached and interactive work

Set `detached: true` on `process.exec()` or `process.execFile()`. Manage the
returned `executionId` with `executions.wait()`, `executions.cancel()`,
`executions.signal()`, stdin, PTY resize, bounded output replay, reset, and
deletion. `process.spawn()` is the separate managed-child-process API: it
returns a PID and is controlled through the remaining `process.*` methods.

An execution accepts only one active operation. Reusing a running ID fails
immediately. Process-only operations do not pin a retained language, so the
same idle execution can later run JavaScript or Python.

## Files and software

Shell commands see the persistent [filesystem](/docs/filesystem) shared by
agents, JavaScript, and Python. Common POSIX commands are available by default,
and additional software is projected through the [software registry](/docs/software).

## Custom bindings

[Custom Bindings](/docs/extensions/custom-bindings) appear as commands such as
`agentos-weather forecast`, so shell pipelines can use trusted host
capabilities without putting credentials inside the VM.

## Permissions, limits, and timeouts

Every command inherits the VM [permission policy](/docs/permissions) and
[resource limits](/docs/resource-limits). `timeoutMs` sets an operation-level
wall-clock deadline. A denied guest operation receives its normal POSIX error,
and a timed-out execution retains a structured `timed_out` result.