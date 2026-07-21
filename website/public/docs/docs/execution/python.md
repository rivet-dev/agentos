# Python

Execute Python source, files, modules, and package workflows in AgentOS.

AgentOS runs CPython 3.13 as a first-class VM execution engine. Python shares
the VM filesystem, process tree, networking policy, permissions, and limits
with agents, Bash, JavaScript, and installed software.

## Get started

`@rivet-dev/agentos-python` is the focused Python entry point. It uses the same
sidecar protocol as the core `AgentOs` client.

Use `python.execute()` for source and `python.evaluate()` for a JSON-compatible
value on the core client. The focused package exposes them as `execute()` and
`evaluate()` and provides default `cwd` and `env` values. Evaluation values are
also returned as bounded JSON display outputs. A value that Python's JSON
encoder cannot represent produces a structured
`evaluation_serialization_failed` result.

## Retain Python state

Reuse an `executionId` to keep Python globals, functions, imports, modules, and
guest objects in one interpreter. Set `createIfMissing: true` only on the first
operation. Structured `inputs` is replaced for every call.

Only inline execute and evaluate calls retain interpreter memory. File,
module, install, Bash, and arbitrary-command operations use fresh processes.
`executions.reset()` clears retained memory and output without reverting the
filesystem; `executions.delete()` removes an idle execution.

## Files, modules, and async Python

Use `python.executeFile()` for an existing script and
`python.executeModule()` for the standard `python -m` workflow. Inline Python
supports top-level `await`, `async for`, and `async with`; awaited work is part
of the operation deadline.

The API does not promise that unawaited `asyncio` tasks survive between
operations. Cancellation, timeout, reset, deletion, or VM disposal stops
execution-owned asynchronous work.

## Install packages

Call `python.install()` on `AgentOs`, or `install()` on the focused runtime.
Pass package specs for named installs, `requirementsFile` for a
requirements install, and `upgrade`, `indexUrl`, or `extraIndexUrls` for the
stable package workflows.

Named packages and a requirements file cannot be combined in one call.
Installs modify the VM-wide filesystem and are visible to every execution in
that VM. AgentOS admits only one npm or Python package mutation at a time in a
VM; a concurrent install fails with `execution_busy` instead of risking shared
package state. Package downloads obey the VM network policy.

## Detached work and output

Set `detached: true` on execute, file, or module operations. Manage the returned
`executionId` with `executions.wait()`, `executions.cancel()`,
`executions.signal()`, stdin, PTY resize, reset, deletion, and bounded output
replay. Core output callbacks receive `Uint8Array`; actor events carry tagged
UTF-8/base64 data. Actor actions use the same nested method paths as Core and
RivetKit transports them with dotted wire names.

## Filesystem, processes, and networking

`pathlib`, `os`, and file objects use the VM [filesystem](/docs/filesystem).
`subprocess` starts kernel-managed guest commands. Python DNS, HTTP clients, and
outbound sockets use the VM network policy. See
[Processes & Shells](/docs/processes) and
[Networking & Previews](/docs/networking).

## Custom bindings and policy

Python can call [Custom Bindings](/docs/extensions/custom-bindings) as normal
commands through `subprocess`.

All operations inherit [permissions](/docs/permissions) and
[resource limits](/docs/resource-limits). `timeoutMs` is an additional
operation wall-clock deadline covering staging, compilation, guest execution,
and result collection, while VM safety watchdogs remain independently bounded.