# Resource Limits

Cap per-VM processes, file descriptors, sockets, and filesystem bytes so guest code can never exhaust the host.

Every agentOS VM runs with **per-VM resource caps**. Runaway or malicious guest code can exhaust its own VM, but it can never starve the host or any sibling VM.

- **Bounded by default**: each VM ships with conservative caps. Unset fields fall back to built-in defaults that match the runtime's historical constants.
- **Per-VM**: every VM gets its own budget. Limits are not shared across VMs.
- **Enforced by the kernel**: a guest that exceeds a cap fails inside the VM (out-of-memory, `EMFILE`, `EAGAIN`, etc.). The host is never affected.
- **Operator-raisable**: the operator (the trusted process that creates the VM) may raise any cap for trusted workloads. Guest code can never raise its own caps.

## Setting limits

Set caps on the `limits` object in the `agentOS` config. Limits are grouped by subsystem (`resources` and more). Omitted limits keep their secure default.

## Available caps

| Limit | Controls | Notes |
|---|---|---|
| `resources.maxProcesses` | Concurrent processes in the VM process table | Caps fork bombs and runaway spawning. New spawns fail with `EAGAIN`. |
| `resources.maxOpenFds` | Open file descriptors | Exhausting the table fails with `EMFILE` / `ENFILE`. |
| `resources.maxSockets` | Open sockets in the socket table | Bounds concurrent connections; excess `connect`/`accept` fail. |
| `resources.maxFilesystemBytes` | Total bytes stored in the virtual filesystem | Bounds VFS storage; writes past the budget fail with a no-space error. |
| `resources.maxWasmStackBytes` | Maximum WASM call-stack size, in bytes | Deep recursion fails with a stack overflow instead of crashing the VM. |

## Behavior at the limit

- **WASM stack**: deep recursion throws a stack-overflow error in the guest, never a host crash.
- **Filesystem bytes**: writing past the VFS budget fails with a no-space error to the guest.
- **Counts (fds / processes / sockets)**: hitting a table cap returns the standard POSIX errno (`EMFILE`, `EAGAIN`, etc.), exactly as a real Linux kernel would under `ulimit`.

## Warnings & observability

Limits are observable, not just enforced. Every bound — resource caps and the
internal bounded queues alike — is tracked in a central limit registry that:

- **Warns before the limit is hit.** As usage crosses ~80% of a cap, the runtime
  emits a structured warning (once per crossing, re-armed only after it recovers),
  so a slow consumer or a runaway guest is visible *before* it fails.
- **Applies backpressure instead of failing catastrophically.** The internal
  queues between the guest, the runtime, and the host block their producer when
  full rather than dropping data or tearing down the session — so a transient
  burst degrades to "slower", not "broken".
- **Surfaces through logs.** secure-exec logs to stderr (stdout is the wire
  protocol); set `SECURE_EXEC_LOG=warn` (the default) to see near-limit warnings
  or `SECURE_EXEC_LOG=debug` for live per-limit usage snapshots.

See [Limits & Observability](/docs/architecture/limits-and-observability) for the
full architecture.