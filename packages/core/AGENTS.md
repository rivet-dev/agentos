# agentOS Core package

`@rivet-dev/agentos-core` is the embedded TypeScript API for VM lifecycle,
filesystem, processes, terminals, language execution, networking, software,
cron commands, permissions, limits, and trusted host functions. Agents, durable
agent sessions, and ACP are not part of Core.

## Security boundary

All untrusted guest code executes inside the kernel. Guest filesystem,
networking, and process operations must go through the sidecar/runtime; never
route them to host APIs. Trusted embedded callers may configure host functions
and host-backed mounts. Hosted actor policy is stricter and is implemented
outside this package.

## API and ownership

- Keep public inputs and results JSON-serializable except for explicitly
  embedded-only callbacks and filesystem objects.
- Keep TypeScript and Rust client behavior aligned.
- Clients serialize explicit caller input and route events. Defaults, package
  projection, permissions, process policy, and filesystem semantics belong in
  the sidecar/runtime.
- Filesystem and process methods should remain thin mirrors of the Rust Core
  contract.
- Cron expression parsing and command scheduling remain in TypeScript Core.
  The hosted Rust actor uses RivetKit scheduling instead.
- Host functions convert Zod to JSON Schema, validate calls and results, and run
  trusted local callbacks. Schema conversion is fail-closed.
- Host function invocations require explicit host-function permissions in addition to any
  filesystem or child-process permissions used by the guest command.
- The native framed transport retains connection and ownership-session scopes.
  Those are transport lifetimes, not the removed public agent-session API.
- The native transport uses BARE payloads by default. Keep any JSON codec behind
  explicit diagnostic or migration-only options.
- Do not restore Actor Runtime Socket support.

## Runtime details

- Shell-sensitive commands go through guest `sh -c`; shell-free commands may
  use direct spawn.
- Preserve shell quoting, escapes, cwd, exit status, and `&&` semantics.
- VM teardown must drain process and terminal completion before removing event
  listeners or disposing the sidecar lease.
- Package projection is sidecar-owned under `/opt/agentos`. The client does
  not parse runtime manifests.
- Module resolution reads mounted guest-visible files through the kernel VFS.
- Permissions are declarative input forwarded to Rust, which owns enforcement.
- Native sidecar output goes to stderr because stdout is a protocol lane.

## Testing

- Prefer scoped Vitest files while iterating.
- Rebuild `packages/core` before testing workspaces that import its `dist`
  output.
- VM-backed tests must dispose VMs and shared sidecars.
- Permission-focused tests must pass an explicit policy.
- Long-lived event waits must be abortable and should not use multi-hour timeout
  sentinels.
- Keep host-function coverage after any transport, permission, or lifecycle
  change.
- Registry command binaries are generated artifacts; use the repository
  toolchain recipes when a test needs them.
