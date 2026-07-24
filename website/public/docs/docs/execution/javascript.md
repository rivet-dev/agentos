# JavaScript

Execute JavaScript and TypeScript, install npm dependencies, and manage execution lifecycles in AgentOS.

AgentOS runs JavaScript in native V8 behind the VM kernel boundary. Use the
first-class language methods for source, files, values, TypeScript, npm
projects, and package binaries. Use `process.execFile()` only when you need an
unusual command that does not have a language-level method.

## Get started

`@rivet-dev/agentos-javascript` is the focused JavaScript/TypeScript entry
point. It creates the same AgentOS VM and delegates every operation to the
sidecar-owned execution protocol.

Every attached call returns one result discriminated by `outcome`. Successful
evaluations contain a JSON-compatible `value`; failed, cancelled, and timed-out
results contain a structured `error`. Stdout and stderr are always captured
with explicit truncation flags. Evaluation results also expose the value as a
bounded `{ type: "json", data }` display output. Returning `undefined`, a
function, a symbol, a circular object, or another non-JSON value produces an
`evaluation_serialization_failed` result instead of losing the value silently.

## Use the core client

The same methods are available directly on `AgentOs` under
`javascript.execute()`, `javascript.evaluate()`, `javascript.executeFile()`,
`javascript.typescript.*`, and `javascript.npm.*`.

The focused runtime adds default `cwd` and `env` values and exposes the core VM
as `runtime.vm`; it does not implement a second execution engine.

## Retain JavaScript state

Give an operation an `executionId` and set `createIfMissing: true` on the first
call. Later inline JavaScript and TypeScript operations with the same ID share
one retained JavaScript realm. `inputs` is replaced on every operation and is
serialized separately from source.

An execution accepts one active operation at a time. Reusing a busy ID fails
immediately. Bash, files, npm operations, and type checks can use the same
lifecycle ID, but run in fresh processes and do not alter retained language
memory. `executions.reset()` clears memory, results, and output; filesystem and
installed-package changes remain. `executions.delete()` removes an idle
execution.

## TypeScript

TypeScript execution is a convenience API in the JavaScript layer. **Executing
TypeScript transpiles it without semantic type checking.** Call
`javascript.typescript.check()` or
`javascript.typescript.checkProject()` when diagnostics are part of the
workflow.

Inline `filePath` identifies source for diagnostics and module resolution; it
does not read that file. Use `javascript.typescript.executeFile()` on the core
client to execute an existing file. JavaScript and TypeScript inline calls
share retained state when they use the same execution ID.

## npm projects and packages

Use the options-only `javascript.npm.install()` overload for the project in
`cwd`. `frozen: true` performs a lockfile-exact clean install. Pass package
names for a named install, and use `javascript.npm.runScript()` or
`javascript.npm.runPackage()` instead of assembling npm commands. The focused
runtime exposes the same workflows as `installNpmPackages()`,
`executeNpmScript()`, and `executeNpmPackage()`.

Package names, script names, paths, and arguments are transferred as distinct
argv values. Installs modify the VM-wide filesystem, so all executions in that
VM see the same dependencies. AgentOS admits only one npm or Python package
mutation at a time in a VM; a concurrent install fails with `execution_busy`
instead of allowing package managers to corrupt shared state.

## Detached work and output

Set `detached: true` on an execution method for a long-running program. The
returned descriptor contains its `executionId`; use `executions.wait()`,
`executions.cancel()`, `executions.signal()`, `executions.writeStdin()`,
`executions.closeStdin()`, `executions.resizePty()`, and
`executions.readOutput()` to manage it. Core callbacks receive exact
`Uint8Array` chunks.

Actor actions use the same nested method paths as Core; RivetKit transports
them with dotted wire names such as `javascript.execute` and
`executions.readOutput`. Actor stdin and output use tagged
`{ encoding: "utf8" | "base64", data: string }` values, and live output is
broadcast through `executionOutput` and `executionCompleted` events.

## Filesystem, processes, and networking

`node:fs` uses the VM [filesystem](/docs/filesystem), `node:child_process`
starts kernel-managed guest processes, and Node networking APIs use the VM
socket table. Files and installed packages are immediately visible to Bash,
Python, agents, and other executions. See [Processes & Shells](/docs/processes),
[Networking & Previews](/docs/networking), and the detailed
[Node.js compatibility matrix](/docs/execution/javascript-compatibility).

## Custom bindings and policy

Guest JavaScript invokes [Custom Bindings](/docs/extensions/custom-bindings)
as ordinary typed commands, keeping host credentials outside the VM.

All operations inherit the VM's [permissions](/docs/permissions) and
[resource limits](/docs/resource-limits). `timeoutMs` adds an operation-level
wall-clock deadline covering sidecar staging, TypeScript transformation,
guest execution, and result collection; it does not replace the independently
bounded VM safety limits.