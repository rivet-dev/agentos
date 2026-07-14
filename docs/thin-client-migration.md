# Thin-client migration tracker

This document tracks the removal of runtime behavior and policy from the
TypeScript SDK (`packages/core`) and Rust SDK (`crates/client`). Keep it until
every item is resolved and its replacement behavior is covered at the sidecar
or kernel layer.

## Invariants

- Clients validate and serialize explicit caller input, forward requests, route
  callbacks/events, and retain only host resources the sidecar cannot access.
- Omitted fields stay omitted. AgentOS defaults live in the sidecar; generic
  kernel defaults remain appropriate for the lower-level kernel API.
- Guest behavior follows Linux/POSIX. Do not recreate a shell, process table,
  filesystem, or terminal in a client or in a parallel sidecar state machine.
- Trusted sidecar bootstrap must not depend on or consume guest filesystem
  permissions. Guest policy applies after the VM is ready.
- Behavioral tests move to the authoritative sidecar/kernel layer before the
  client implementation is deleted. Client suites retain transport/parity tests.
- TypeScript may choose its package manager's default packages and convert and
  validate Zod tool schemas. These are the intentional TypeScript exceptions.

Statuses are `pending`, `in progress`, `blocked`, or `done`.

## Mandatory JJ stack rule

Every numbered item must be implemented in a **new child `jj` revision stacked
on the preceding item**. Do not combine two numbered implementations in one
revision. A tracking-only dependency update may close an earlier checkbox in
the later item's revision, but the implementation itself remains isolated in
its own revision.

Items 1–18 were implemented before this rule was introduced and remain an
explicit historical exception: their consolidated history must not be
misrepresented as dedicated per-item revisions, and history will not be
rewritten retroactively. Every implementation from item 19 onward requires its
own stacked revision.

## Issue and recommended-fix index

This index preserves the original problem separately from the proposed fix.
Status and validation evidence remain in the work-item and checklist tables
below.

| # | Original issue | Recommended fix | Priority | Fix confidence |
|---|---|---|---|---|
| 1 | Clients populated the standard guest environment, duplicating runtime defaults. | Preserve omitted `env`; define the shared native/browser default once in the runtime. | P1 | High |
| 2 | TypeScript bootstrapped directories/files and temporarily changed guest filesystem permissions during startup. | Make trusted sidecar bootstrap independent of guest filesystem rights; clients send no bootstrap defaults or post-create filesystem setup. | P1 | High |
| 3 | Clients expanded omitted/partial permission policy and re-enforced tool permissions locally. | Let omission select sidecar-owned allow-all and forward only explicit overrides. | P1 | High |
| 4 | Clients encoded PTY state in env, selected shells, parsed commands, queued pre-start operations, and emulated interactive behavior. | Carry PTY/stdin/resize/signal/EOF/shell intent explicitly over the protocol and use kernel terminal semantics. | P1 | High |
| 5 | Clients manufactured PIDs and lifecycle state before the sidecar supplied the real process. | Await creation, return the authoritative kernel PID, and route lifecycle operations through the sidecar. | P1 | High |
| 6 | Clients filled VM, execute, and ACP defaults for env, cwd, root, permissions, mounts, runtime, capabilities, and flags. | Make presence-sensitive wire fields optional; omit defaults and return resolved read-only values when needed. | P1 | High |
| 7 | Clients duplicated ACP/session authority with pending registries, caches, synthetic state, ID checks, tombstones, and detached closes. | Put create/resume/list/state/close authority in the sidecar; retain only host callback/event/permission routes. | P1 | High |
| 8 | Clients implemented ACP filesystem and terminal operations despite lacking authoritative VM/process state. | Implement adapter-supported methods in the native ACP sidecar using kernel primitives. | P1 | High |
| 9 | Clients duplicated tool parsing, dispatch, permission checks, timeouts, prompt assembly, and metadata. | Keep Zod authoring/conversion/single validation in TypeScript; move shared command behavior to the sidecar. | P1 | High |
| 10 | Clients cached package projection and implemented parallel mount/VFS policy; Rust exposed inert local mount state. | Make projection and guest VFS routing sidecar-owned; retain only caller-owned JS bridge handles. | P1 | High |
| 11 | Cron grammar, defaults, reconciliation, run state, and persistence were duplicated, while sleeping actors still needed a host wake. | Keep scheduling truth in the sidecar; retain callbacks, one absolute-alarm hook, and opaque actor persistence. | P2 | High |
| 12 | Clients implemented filesystem algorithms/policy through recursion, probing, normalization, view merging, and local EXDEV/read-only decisions. | Forward primitives to the sidecar/kernel and preserve Linux/POSIX behavior. | P2 | High |
| 13 | Clients orchestrated VM create/readiness/config/register/rollback as a multi-step state machine. | Replace it with one atomic sidecar-owned `initialize_vm` transaction. | P2 | High |
| 14 | TypeScript read runtime `agentos-package.json` and retained an unused snapshot resolver. | Delete both; package validation, metadata, and snapshots remain sidecar-owned. | P2 | High |
| 15 | A duplicate legacy runtime and façade remained under the mistaken assumption browser support needed it. | Delete the compatibility runtime; keep browser execution in `packages/runtime-browser`. | P2 | High |
| 16 | Client paths probed Cargo, scanned mtimes, auto-built binaries, injected dev cwd, and retained bootstrap hooks. | Remove development discovery/build behavior; use explicit overrides or published binaries. | P3 | High |
| 17 | Clients retained dead software/snapshot descriptors and protocol fields, creating a second package surface. | Remove dead types/state/wire fields; keep only TypeScript package-manager selection and forward package paths. | P3 | High |
| 18 | The follow-up audit found additional duplicated defaults and legacy compatibility behavior in findings 18.1–18.72. | Resolve each at its authoritative runtime layer or delete it; track new regressions as top-level items. | P2 | High |
| 19 | TypeScript shared-sidecar callbacks/events were globally routed and could cross VM ownership boundaries. | Key registration and delivery by full ownership and dispose only the matching VM routes. | P0 | High |
| 20 | TypeScript and native tests guessed output completion using quiet timers after process exit. | Remove timing guesses and rely on the sidecar's ordered terminal-event guarantee. | P1 | High |
| 21 | Clients accumulated captured stdout/stderr without enforcing runtime capture limits. | Capture once in the sidecar with per-stream/per-VM bounds and return capture only on the terminal event. | P1 | High |
| 22 | Rust silently lost bounded wire events, used incomplete route keys, and could hang or orphan process/control consumers. | Use exact ownership/process routes, negotiated byte/count bounds, atomic start binding, typed terminal failures, and fail-closed cleanup. | P1 | High |
| 23 | TypeScript truthy checks drop explicit `streamStdin: false`, and the Rust parity path made the same false-to-omission conversion. | Preserve false, true, and omission distinctly across both client serializers. | P1 | High |
| 24 | TypeScript fires stdin write and EOF without awaiting, allowing races and dropped rejections. | Await write, close, and wait sequentially. | P1 | High |
| 25 | TypeScript parses Zod host-tool input twice. | Parse exactly once while keeping Zod tool construction client-side. | P1 | High |
| 26 | TypeScript flattens typed sidecar rejection codes into message-only errors. | Export a structured error preserving code, message, and protocol details. | P1 | High |
| 27 | TypeScript silently discards explicit software inputs it cannot serialize. | Reject structurally invalid client input; leave package existence/format/projection validation in the sidecar. | P1 | High |
| 28 | TypeScript races a client permission timer against the adapter/sidecar timeout. | Remove the local policy timer and retain callback routing only. | P1 | High |
| 29 | TypeScript retains every exited `ManagedProcess` for the VM lifetime. | Delete completed routes or define an explicit bounded host-route policy. | P1 | Medium |
| 30 | Rust opens a wire session per VM and suppresses `DisposeVm` failures. | Add/reuse explicit session-close semantics, propagate disposal failure, and keep retryability. | P1 | High |
| 31 | Clients cache projected package/agent/command state instead of reading live `/opt/agentos`. | Remove caches and query authoritative live sidecar state. | P1 | High |
| 32 | Clients remove ACP routes before session close is confirmed. | Retain routes through successful/already-gone close and preserve them on transport failure. | P1 | High |
| 33 | ACP create/resume performs a second state request before registering routes, opening an event-loss/orphan window. | Return state atomically or register and reconcile before events can be lost. | P1 | High |
| 34 | Native and browser ACP maintain divergent behavioral state machines. | Converge on one shared ACP core with explicit adapter hooks. | P1 | Medium |
| 35 | Rust drops wire fields and silently filters malformed ACP values. | Preserve the complete result and return typed decode errors. | P1 | High |
| 36 | ACP discovery and cleanup mask projected-state/resource failures. | Propagate discovery errors and aggregate cleanup failures deterministically. | P1 | High |
| 37 | Rust cron callbacks return unit, so durable failures are recorded as success. | Return a typed callback result while retaining the host alarm/wake hook. | P1 | High |
| 38 | Security docs claim omitted permissions deny while the runtime defaults to allow-all. | Correct the docs and add a claim verifier. | P1 | High |
| 39 | The README quickstart installs Pi but does not project it before creating a Pi session. | Use and execute the checked explicit-package example. | P1 | High |
| 40 | The actor cron reboot test silently skips when the sidecar binary is absent. | Make CI build/provide the sidecar and require the real teardown/reboot path. | P1 | High |
| 41 | TypeScript and Rust independently build process trees from flat lists. | Return the tree from the sidecar or remove the convenience API. | P2 | Medium |
| 42 | The TypeScript compiler creates `/tmp`, disagrees on `/root` cwd, and retains a legacy filename. | Rely on the Linux base and one real process cwd without bootstrap writes. | P2 | Medium |
| 43 | Both clients expose ignored or behaviorally divergent process options. | Remove unsupported fields or implement them once in the sidecar protocol with parity tests. | P2 | High |
| 44 | Unknown ACP methods make a pointless host round-trip. | Return `-32601` directly in the sidecar unless a real extension API exists. | P2 | High |
| 45 | Production protocol packages retain JSON and legacy test codecs despite lockstep releases. | Migrate fixtures to BARE/typed config and delete compatibility codecs. | P2 | High |
| 46 | Rust cannot distinguish omission from explicit default-valued configuration. | Use `Option`/presence-aware types and preserve presence on the wire. | P2 | High |
| 47 | TypeScript retains a synthetic sidecar lifecycle with manufactured IDs/maps. | Lease the real VM and retain only host lease/refcount state. | P2 | Medium |
| 48 | TypeScript chooses omitted overlay mode before sidecar resolution. | Preserve omission and consume the sidecar-resolved mode. | P2 | Medium |
| 49 | Core declares unused heavy dependencies and an orphaned declaration. | Remove them and regenerate locks. | P2 | High |
| 50 | A deprecated string package descriptor remains exported and used by a transpile-only test. | Remove it and typecheck the public API test. | P2 | High |
| 51 | Active guidance describes obsolete manifests, runtime architecture, permission defaults, and commands. | Align CLAUDE/docs with current architecture and verify them. | P2 | High |
| 52 | Both clients interpret legacy ACP permission methods even though support is adapter-specific. | Move compatibility into the native adapter and leave typed routing in clients. | P2 | Medium |
| 53 | TypeScript handles an ACP compatibility event shape with no producer. | Remove the dead branch. | P3 | High |
| 54 | TypeScript swallows listener errors and Rust drops session/MCP conversion errors. | Propagate failures or emit structured host-visible warnings. | P3 | High |
| 55 | The README hand-maintains a stale public API inventory. | Generate it from declarations or remove it. | P3 | High |
| 56 | Cron dispatch is an asynchronous control event; eviction can lose an alarm update or leave a host callback run unacknowledged. | Add a sidecar-owned pending-dispatch queue with cursor/ack, or a reliable sidecar-request callback, then test recovery without duplicated runs. | P0 | High |
| 57 | Rust `on_process_exit` accepts only `FnOnce(i32)`, so route failure can only be logged. | Add a result-bearing/error callback with coordinated TypeScript/Rust parity. | P2 | High |
| 58 | The generic Rust transport request method can still send `Execute` without atomic routing/cancellation cleanup. | Make Execute use a dedicated typed method and reject or hide Execute through the generic request path. | P2 | High |
| 59 | After a process starts, TypeScript and Rust finite-exec paths can return on stdin write/EOF failure without terminating the process, and host supervision may be lost. | Move finite stdin plus EOF into one sidecar-owned execute operation, or guarantee fail-closed process cleanup on every post-start stdin-control failure. | P1 | High |
| 60 | The shell CLI chains stdin writes on one promise; one rejected write permanently rejects the queue, so its later EOF never runs and the child may remain waiting. | Make queued stdin failure terminal: report it, close or kill the process, and ensure the CLI cannot silently strand the child. | P1 | High |
| 61 | TypeScript rejects user-authored Zod transforms and custom refinements during JSON Schema conversion even though the client is supposed to own full Zod behavior. | Forward a structural pre-effect JSON Schema to the sidecar while retaining the complete Zod schema for the client's single authoritative host-side parse. | P1 | High |
| 62 | Toolkit permission tests still expect omitted policy to deny and invoke captured callbacks directly to assert client-side enforcement, contradicting sidecar-owned allow-all defaults and enforcement. | Rewrite default-policy expectations and move explicit-deny coverage to sidecar integration tests that prove denied callbacks never reach the client. | P2 | High |

## Work items

| # | Status | Priority | Work and completion proof |
|---|---|---|---|
| 1 | done | P1 / high confidence | Standard guest environment moved to one shared native/browser runtime default. Normal TS/Rust clients omit `env` entirely; explicit compatibility/runtime overrides still win. Covered beside the shared default and by thin-payload serialization tests. |
| 2 | done | P1 / high confidence | Sidecar owns root/bootstrap policy, and restrictive guest fs policy does not block startup. Normal clients omit the default root descriptor; TS/Rust bootstrap directory and command defaults, temporary client permissions, and post-create filesystem materialization are removed. Sidecar coverage proves guest deny-all is restored after readiness. |
| 3 | done | P1 / high confidence | Omitted AgentOS policy defaults to allow-all through one native/browser sidecar normalization. Clients omit policy and no longer expand partial policies or re-enforce tool binding permissions. Explicit deny and omitted-policy tests live in the sidecar. |
| 4 | done | P1 / high confidence | PTY creation, dimensions, live stdin intent, resize, native ACP terminal lifecycle, default shell selection, and raw command-line classification use explicit sidecar protocol fields/methods. Clients no longer encode terminal control in environment variables, parse command lines, choose/default a shell, queue operations before terminal startup, or emulate an interactive shell/prompt. Kernel/sidecar suites cover line discipline/raw mode, resize/SIGWINCH, signals, EOF, shell behavior, command-line grammar, and exit status. |
| 5 | done | P1 / high confidence | `spawn` and `openShell` are asynchronous in both SDKs and return only after the sidecar supplies the real kernel PID. Removed synthetic PID allocators/remapping, Rust's background launch and readiness watch, and TypeScript's pre-start stdin/close/kill/resize queues. The sidecar/kernel owns lifecycle, trees, groups, signals, and wait state; clients retain only bounded host callback/event correlation keyed by the real PID and wire process ID. Real TS/Rust lifecycle suites prove the returned PID is the process-table PID. |
| 6 | done | P1 / high confidence | VM environment/root/loopback/permission/bootstrap/package-mount defaults, execute cwd/env overrides, and ACP session cwd, runtime, args, env, MCP, protocol version, client capabilities, and flags are sidecar-owned. VM creation returns the resolved guest cwd/environment for read-only client views, while TS/Rust omit default execute overrides. The VM/configure and lockstep ACP protocols carry optional fields, both clients and the actor bridge omit defaults, and native/browser sidecars use shared normalization. |
| 7 | done | P1 / high confidence | Removed client pending-request registries, synthetic prompt cancellation, hydration/config caches, synthetic config mutations, duplicate-id checks, live-session lists, closed-id tombstones, and detached close tasks. Session state/list/close now use authoritative sidecar requests; close is awaited and idempotent, create/resume collisions are sidecar-owned, and clients retain only host callback/event/permission routing. Native/core sidecar plus TS/Rust SDK integration tests cover the boundary. |
| 8 | done | P1 / high confidence | The native ACP extension owns filesystem and terminal host methods against VM state: create/write/output/wait/kill/release/resize use sidecar process, PTY, bounded-buffer, signal, and lifecycle primitives. TS/Rust filesystem and terminal dispatchers, terminal registries, counters, output buffers, and Rust ACP shell helpers are removed. Native integration coverage proves these methods never call the client; unknown adapter-specific methods retain JSON-RPC `-32601`. |
| 9 | done | P1 / high confidence | TypeScript keeps Zod authoring, JSON Schema conversion, and callback Zod validation. The sidecar now exclusively owns CLI parsing, registry/help metadata, indirect and direct command dispatch, permission enforcement, callback timeout, and ACP tool-reference prompt assembly. Rust and TypeScript command emulators, timeout races, prompt formatters, and cached prompt state are removed; native sidecar/ACP tests own the behavior. |
| 10 | done | P1 / high confidence | Package projection persistence and public mount routing are sidecar-owned. TS no longer caches/replays packages, merges mounted directories, routes public fs calls directly to host backends, or enforces local EXDEV/read-only policy; it retains only exact caller-owned `js_bridge` handles that the sidecar cannot access. Rust's inert in-process mount map/trait and unsupported plain/overlay variants are removed. The runtime-core compatibility merged view is deleted. |
| 11 | done | P2 / high confidence | Cron grammar, defaults, job/run state, overlap policy, missed-fire reconciliation, alarm generations, lifecycle events, opaque cold-wake state, and serializable action execution are shared sidecar behavior. Native/browser sidecars execute `exec`; the native ACP adapter executes `session`, while the browser adapter reports its lack of background ACP support as a typed cron error. TS/Rust clients only arm one absolute alarm, retain host callback closures, and forward state/events. The actor persists a sidecar-owned opaque snapshot plus one generation-tagged durable wake action, with a real teardown/reboot test proving restoration. |
| 12 | done | P2 / high confidence | TS/Rust filesystem calls use sidecar/kernel primitives, including direct positional `pwrite`, native recursive mkdir, and typed directory entries; removed client read/modify/write, recursive copy/remove, ancestor probing, per-entry `lstat`, batch/recursive-exclude convenience loops, mounted-directory merge, path normalization, and local read-only/cross-device policy. Relative paths and `.`/`..` resolve in the shared sidecar against the VM cwd. Non-recursive flags are omitted and default in the sidecar. Kernel `move_path` preserves Linux `EXDEV`, and unmount rebuilds the execution shadow from the revealed VFS. The compatibility filesystem implementation is deleted. |
| 13 | done | P2 / high confidence | Replaced production TS/Rust create/wait/configure/register orchestration with one session-scoped `initialize_vm` request. The sidecar owns readiness, ordering, resolved env/cwd, explicit mount/package projection, callback-metadata registration, and rollback of partial VM state. Clients omit empty mounts/packages and no longer subscribe to readiness or carry a readiness timeout. Native/browser atomic rollback tests plus real TS, Rust, and actor cold-boot tests cover the transaction. |
| 14 | done | P2 / high confidence | Removed runtime TypeScript reads/validation of `agentos-package.json` and the unused host snapshot-bundle resolver. Package metadata and snapshots remain sidecar-owned. |
| 15 | done | P2 / high confidence | The legacy runtime was not browser support. Runtime benchmarks now use the public AgentOS API; the public runtime-core `NodeRuntime`, schema, kernel proxy, legacy runtime, tests, and exports are deleted. The redundant in-repo `secure-exec` façade and orphaned private registry compatibility harness are also deleted. Browser execution remains in `packages/runtime-browser` and its internal worker drivers are not client compatibility APIs. |
| 16 | done | P3 / high confidence | Removed Cargo probing, source-tree mtime scans, automatic Cargo builds, the published runtime Cargo helper, dev target probing/cwd injection, and the unused create/configure `bootstrapCommands` hooks from both production and legacy test-runtime paths. Tests invoke Cargo explicitly; runtime binary resolution uses an explicit override or published platform package and fails actionably when absent. |
| 17 | done | P3 / high confidence | Removed Rust `software`/`SoftwareKind`/`SoftwareInput`, TS `_softwareRoots`, unused snapshot resolution, and the dead `SoftwareDescriptor` request/`appliedSoftware` response wire path. TS `software` remains only as the allowed package-manager input and is forwarded as package paths. |
| 18 | done | P2 / high confidence | The follow-up legacy/default audit is complete through finding 18.72 below. Future regressions should be added as new numbered findings before implementation. |
| 19 | done | P0 / high confidence | TypeScript shared-sidecar callback and event routing is VM-isolated through ownership-keyed request registration, ownership-filtered event delivery, and explicit disposal. Runtime-core coverage proves `js_bridge`, host-tool, ACP, cron, warning, unmatched-owner, and unregister routing; a real shared-sidecar AgentOS test proves two VMs retain distinct host tools and cron callbacks before and after sibling disposal. |
| 20 | done | P1 / high confidence | The sidecar already reorders trailing process output before the terminal event, but TypeScript still guessed completion with quiet timers, a TypeScript integration test polled after exit, and the native wire-test collector waited 200 ms after exit and therefore masked ordering regressions. Remove the client timing guesses and make native collectors return immediately on the terminal event so TypeScript and Rust rely on the same sidecar-owned ordering guarantee. |
| 21 | done | P1 / high confidence | Both clients accumulated captured stdout/stderr themselves without enforcing the configured runtime limits. Native and browser sidecars now own one shared bounded capture implementation, enforce both per-stream limits and a default `32 MiB` per-VM aggregate, return the result only on the terminal event, kill overflowed executions with `ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED`, and name the exact limit to raise. Clients only request capture, forward streaming callbacks, and deserialize the terminal result without an intermediate full-buffer copy. Raw `spawn` and `captureStdio: false` remain uncaptured streams. Browser terminal delivery is backpressured instead of queued, native stdout retains at most two waiting frames, and Rust retains each decoded terminal once in the byte-bounded transport log completed by item 22. Validated capture limits plus bounded process IDs guarantee each terminal fits the negotiated frame. |
| 22 | done | P1 / high confidence | Rust now retains process events on exact `(full ownership, process ID)` routes and control events on separate exact ownership routes, with negotiated byte bounds, a count backstop, per-subscriber cursors, and typed lag/close failures. Execute routing is installed atomically before its start response is exposed, and cancellation before enqueue, after enqueue, and after a buffered start response cannot leak a pending slot or orphan the process. Exec/spawn/shell route loss requests `SIGKILL`; cleanup rejection kills the sidecar fail-closed. Process, shell, session, permission, agent-exit, cron, and actor streams surface a terminal typed error, including late subscribers. Cron clears its durable host alarm and rejects stale post-failure dispatch, and bounded shell-route retention cannot race past its cap. Reliable replay/ack for asynchronous cron dispatch remains separately tracked as item 56; generic Execute API hardening remains item 58. |
| 23 | done | P1 / high confidence | TypeScript now forwards `streamStdin` through the core proxy and runtime wire serializer whenever it is explicitly present, including `false`; Rust forwards the equivalent `Option<bool>` without converting false to omission. Omission remains absent so the sidecar alone applies its PTY `keepStdinOpen: true` default, while explicit false and true remain explicit. The downstream TypeScript protocol conversion already preserved nullable booleans and required no behavioral change. |
| 24 | done | P1 / high confidence | TypeScript `execArgv` now matches `exec` and Rust: it awaits an optional stdin write, awaits EOF, and only then observes process completion. Write and EOF failures propagate instead of becoming unhandled promises, and public test callers await their write/close promises rather than normalizing unsafe usage. Post-start cleanup on a successfully propagated stdin-control failure is a separate cross-client concern tracked as item 59. |
| 25 | done | P1 / high confidence | TypeScript now performs one authoritative Zod `safeParse` for a host callback and passes only that parsed/stripped/transformed value directly to `tool.execute`. The redundant `executeHostTool` parser is deleted. Zod construction and validation remain client-owned as required; support for registering effect/refinement schemas is separately tracked as item 61. |
| 26 | pending | P1 / high confidence | TypeScript flattens typed sidecar rejection codes into message-only `Error` objects, preventing Linux-style `error.code` handling. Preserve code, message, and protocol details in an exported structured error. |
| 27 | pending | P1 / high confidence | TypeScript silently discards startup software entries it cannot serialize, so a VM may boot without caller-requested packages. Clients must reject structurally unserializable explicit input; the sidecar remains authoritative for package existence, format, manifest, and projection validation. |
| 28 | pending | P1 / high confidence | TypeScript races a client-owned ACP permission timeout against the sidecar-owned timeout/default. Remove the client policy timer and retain only callback routing. |
| 29 | pending | P1 / medium confidence | TypeScript retains every exited `ManagedProcess` for the VM lifetime, creating an unbounded duplicate of sidecar-owned history. Delete completed routing state or apply an explicit bounded route policy if late host correlation requires retention. |
| 30 | pending | P1 / high confidence | Rust opens a wire session per VM without a close-session operation and suppresses `DisposeVm` failures. Reuse a connection session or add explicit close semantics, propagate failure, and keep shutdown retryable. |
| 31 | pending | P1 / high confidence | Clients cache projected package, agent, and command state captured during configuration, contrary to live `/opt/agentos` authority. Remove caches and query live sidecar state. |
| 32 | pending | P1 / high confidence | TypeScript and Rust remove ACP callback/event routes before the sidecar confirms session closure. Retain routes through successful close or typed already-gone and preserve retryability after transport failure. |
| 33 | pending | P1 / high confidence | TypeScript creates/resumes an ACP session, performs a second state request, and only then registers routing, creating an event-loss and orphan window. Return sufficient state atomically or register and reconcile immediately. |
| 34 | pending | P1 / medium confidence | Native and browser ACP use separate behavioral state machines and already differ for adapter prompt/config behavior. Converge them on one shared ACP core with explicit adapter hooks. |
| 35 | pending | P1 / high confidence | Rust drops protocol fields such as `adapter_entrypoint` and silently filters malformed session values. Preserve the complete wire result and return typed decoding errors. |
| 36 | pending | P1 / high confidence | ACP discovery converts projected-state failures into empty/unknown-agent results and ACP cleanup suppresses resource failures. Propagate discovery errors and aggregate cleanup failures. |
| 37 | pending | P1 / high confidence | Rust cron host callbacks return unit and therefore cannot mark durable runs failed, unlike TypeScript. Return a typed callback result while retaining the legitimate host alarm/wake hook. |
| 38 | pending | P1 / high confidence | Public security documentation claims omitted permissions deny access while the sidecar defaults omission to allow-all. Correct every affected security, networking, and runtime page and guard the claim against regression. |
| 39 | pending | P1 / high confidence | The TypeScript README quickstart installs Pi but does not pass Pi in `software` before creating a Pi session. Use the checked explicit-package example and execute it as documentation coverage. |
| 40 | pending | P1 / high confidence | The claimed actor cron cold-boot test returns successfully when `AGENTOS_SIDECAR_BIN` is absent, and CI does not provide it. Make the real teardown/reboot path mandatory in CI. |
| 41 | pending | P2 / medium confidence | TypeScript and Rust independently build process trees from flat process lists. Move tree construction to the sidecar or remove the convenience API, then leave forwarding-only client coverage. |
| 42 | pending | P2 / medium confidence | The TypeScript compiler package creates `/tmp`, applies inconsistent `/root` cwd defaults, and retains a secure-exec-era request filename. Rely on the Linux base and one real process cwd without bootstrap writes. |
| 43 | pending | P2 / high confidence | Both clients expose process options that are never honored or behave differently across SDKs. Remove unsupported fields unless implemented once in the sidecar protocol with parity coverage. |
| 44 | pending | P2 / high confidence | Unknown ACP methods make a host round-trip even though TypeScript has no extension handler and always returns null. Return method-not-found directly in the sidecar unless a real host-extension API exists. |
| 45 | pending | P2 / high confidence | Production protocol packages retain a JSON payload codec and a large legacy test configuration parser despite lockstep releases. Migrate fixtures to BARE/typed configuration and delete compatibility paths. |
| 46 | pending | P2 / high confidence | Rust cannot distinguish omitted presence-sensitive configuration from explicitly supplied default-valued input. Represent presence with `Option` and preserve it on the wire. |
| 47 | pending | P2 / medium confidence | TypeScript retains a synthetic `AgentOsSidecarClient` lifecycle with IDs and maps unrelated to the authoritative wire lifecycle. Lease the real VM directly and retain only host lease/refcount state. |
| 48 | pending | P2 / medium confidence | TypeScript chooses the omitted overlay mode as `ephemeral`, duplicating the sidecar default. Keep the JS bridge host-owned but obtain the resolved mode from the sidecar. |
| 49 | pending | P2 / high confidence | Core declares unused heavy production dependencies and an orphaned `long-timeout` declaration. Remove them and regenerate locks. |
| 50 | pending | P2 / high confidence | The deprecated string package descriptor remains exported and a transpile-only test calls `defineSoftware(string)` despite the supported `{ packagePath }` type. Remove the legacy surface and typecheck the public API test. |
| 51 | pending | P2 / high confidence | Active CLAUDE/docs files describe obsolete JSON package manifests, an in-process runtime, contradictory permission defaults, and a deleted registry command. Align all guidance with the current architecture. |
| 52 | pending | P2 / medium confidence | Legacy ACP permission-method shims remain in both clients even though support varies by native adapter. Move compatibility into the adapter/sidecar and leave typed routing in clients. |
| 53 | pending | P3 / high confidence | TypeScript handles a structured `acp.session_event` compatibility shape with no current producer. Remove the dead branch. |
| 54 | pending | P3 / high confidence | TypeScript swallows event-listener exceptions and Rust silently drops some session/MCP conversion errors. Propagate failures or emit structured host-visible warnings. |
| 55 | pending | P3 / high confidence | The core README hand-maintains an API inventory containing removed options, nonexistent types, and obsolete fields. Generate it from declarations or remove it. |
| 56 | pending | P0 / high confidence | Asynchronous cron dispatch still crosses a bounded control-event route. If it is evicted, an alarm update or callback run can be lost even though item 22 now fails the client route, clears the host alarm, rejects subsequent cron operations, and surfaces a typed actor error. Add a sidecar-owned pending-dispatch queue with cursor/ack or a reliable sidecar-request callback so recovery cannot duplicate or strand runs. |
| 57 | pending | P2 / high confidence | Rust `on_process_exit` accepts only `FnOnce(i32)`, so a route failure can be logged but cannot reach that callback without inventing an exit code. Add a result-bearing/error callback and mirror it in TypeScript. |
| 58 | pending | P2 / high confidence | All production Rust Execute paths use atomic process routing, but the generic transport request method can still encode Execute without the specialized cancellation tombstone. Make Execute a dedicated typed transport operation and reject or hide it on the generic path. |
| 59 | pending | P1 / high confidence | Both TypeScript `exec`/`execArgv` and Rust `exec_request` can abandon an already-started process when a subsequent stdin write or EOF request fails. Prefer one finite-input sidecar operation so the sidecar owns write/EOF ordering and cleanup; until then, all clients must issue fail-closed cleanup and retain the original typed error. |
| 60 | pending | P1 / high confidence | The shell CLI serializes writes by replacing one promise with `stdinQueue.then(...)`. A rejected write leaves that queue permanently rejected, so the queued EOF operation does not run; logging the rejection does not prevent a child from waiting forever. Make the failure terminal and explicitly close or kill the process. |
| 61 | pending | P1 / high confidence | `host-tools-zod.ts` rejects Zod pipe/pipeline transforms and custom refinements during VM registration because their semantics cannot be represented faithfully in JSON Schema. That prevents callers from using full Zod behavior even though TypeScript owns the authoritative callback parse. Derive and forward only the structural pre-effect input schema for sidecar CLI/help parsing, retain the complete Zod schema client-side, and run it exactly once before `execute`. |
| 62 | pending | P2 / high confidence | Three `toolkit-permissions.test.ts` cases still encode the removed client-enforcement model: omitted permissions are expected to deny, and tests invoke the captured callback directly while expecting binding policy to run there. Omission is sidecar-owned allow-all and explicit policy is enforced before callback dispatch. Rewrite these as sidecar integration coverage and keep direct callback tests limited to host-side Zod/callback behavior. |

## Open-item validation checklists

Each completed implementation must live in its own stacked `jj` revision. The
before test is run against the item's parent behavior (or first demonstrated as
a failing regression test in the item revision); the after test must pass with
the implementation. An item is not `done` until all three boxes are checked.

| # | Before-change behavior test | After-change validation | Item complete |
|---|---|---|---|
| 1 | - [ ] Historical parent test for client-populated base env must be reconstructed; this predates the checklist rule. | - [x] Consolidated migration coverage verifies omitted client env and the shared sidecar/runtime base environment. | - [x] Implemented before the per-item revision rule; explicit historical exception, no retroactive stack rewrite. |
| 2 | - [ ] Historical parent test for client filesystem bootstrap/temporary permissions must be reconstructed. | - [x] Consolidated startup coverage verifies restrictive guest filesystem policy does not block trusted bootstrap. | - [x] Implemented before the per-item revision rule; historical exception. |
| 3 | - [ ] Historical parent test for client-expanded omitted policy must be reconstructed. | - [x] Consolidated native/browser policy coverage verifies omitted allow-all and explicit deny behavior. | - [x] Implemented before the per-item revision rule; historical exception. |
| 4 | - [ ] Historical parent test for client terminal emulation/env control must be reconstructed. | - [x] Kernel/sidecar PTY suites cover line discipline, resize, signals, EOF, shell grammar, and exit status. | - [x] Implemented before the per-item revision rule; historical exception. |
| 5 | - [ ] Historical parent test for synthetic PID/lifecycle state must be reconstructed. | - [x] Real TypeScript/Rust process lifecycle suites verify the returned PID is authoritative. | - [x] Implemented before the per-item revision rule; historical exception. |
| 6 | - [ ] Historical parent serialization tests for client-filled VM/execute/ACP defaults must be reconstructed. | - [x] Lockstep wire/config tests verify omission and sidecar-resolved env/cwd values. | - [x] Implemented before the per-item revision rule; historical exception. |
| 7 | - [ ] Historical parent tests for duplicate ACP registries/caches/tombstones must be reconstructed. | - [x] Native/core sidecar plus TS/Rust lifecycle tests cover authoritative list/state/close behavior. | - [x] Implemented before the per-item revision rule; historical exception. |
| 8 | - [ ] Historical parent test for client ACP filesystem/terminal dispatch must be reconstructed. | - [x] Native ACP integration coverage verifies filesystem/terminal methods stay inside the adapter/sidecar. | - [x] Implemented before the per-item revision rule; historical exception. |
| 9 | - [ ] Historical parent tests for duplicated tool dispatch/prompt/timeout behavior must be reconstructed. | - [x] Native tool/ACP tests cover sidecar dispatch while TypeScript Zod conversion/validation tests remain client-owned. | - [x] Implemented before the per-item revision rule; historical exception. |
| 10 | - [ ] Historical parent tests for client projection/mount routing must be reconstructed. | - [x] Sidecar package/VFS coverage and TS/Rust forwarding tests verify authoritative projection. | - [x] Implemented before the per-item revision rule; historical exception. |
| 11 | - [ ] Historical parent tests for duplicated cron grammar/state/reconciliation must be reconstructed. | - [x] Shared scheduler and actor teardown/reboot coverage verify opaque state plus the generation-tagged alarm hook. | - [x] Implemented before the per-item revision rule; historical exception; reliable async dispatch remains item 56. |
| 12 | - [ ] Historical parent tests for client filesystem algorithms/policy must be reconstructed. | - [x] Kernel/sidecar filesystem suites cover positional writes, recursive mkdir, relative paths, unmount, and Linux `EXDEV`. | - [x] Implemented before the per-item revision rule; historical exception. |
| 13 | - [ ] Historical parent tests for multi-step client VM initialization must be reconstructed. | - [x] Native/browser rollback plus TS/Rust/actor cold-boot tests cover atomic `initialize_vm`. | - [x] Implemented before the per-item revision rule; historical exception. |
| 14 | - [ ] Historical parent inventory for runtime manifest/snapshot client reads must be reconstructed. | - [x] Package build/runtime tests pass without shipping or reading `agentos-package.json`. | - [x] Implemented before the per-item revision rule; historical exception. |
| 15 | - [ ] Historical parent usage tests for the legacy runtime/façade must be reconstructed. | - [x] Runtime benchmarks/public API and browser-runtime suites pass after compatibility deletion. | - [x] Implemented before the per-item revision rule; historical exception. |
| 16 | - [ ] Historical parent tests for Cargo probing/auto-build/dev cwd behavior must be reconstructed. | - [x] Explicit-binary resolution and test-runtime suites pass without production Cargo probing. | - [x] Implemented before the per-item revision rule; historical exception. |
| 17 | - [ ] Historical parent type/wire tests for dead software/snapshot fields must be reconstructed. | - [x] TS/Rust public surface and generated protocol checks pass with only forwarded package paths. | - [x] Implemented before the per-item revision rule; historical exception. |
| 18 | - [ ] Findings 18.1–18.72 retain their individual evidence in the detailed audit below; a consolidated parent-test index was not created before the rule. | - [x] Each detailed finding records its post-change behavior/validation and confidence. | - [x] Consolidated legacy audit complete before the per-item revision rule; future findings are top-level items. |
| 19 | - [x] `packages/runtime-core/tests/shared-sidecar-ownership.test.ts` failed against the parent because only one mutable handler API existed; the review also demonstrated global unfiltered delivery. | - [x] Runtime-core coverage proves isolated bridge, tool, ACP, cron, warning, unmatched-owner, and unregister routing; `packages/core/tests/shared-sidecar-ownership.test.ts` passes against two real VMs sharing one sidecar, including sibling disposal. | - [x] Dedicated stacked `jj` revision `pmsonxok`; work-item row marked `done`. |
| 20 | - [x] `packages/core/tests/process-event-ordering.test.ts` failed against the parent because `wait()` remained pending until a client timer advanced; `python-cli.test.ts` and the native wire collector also explicitly waited after exit. | - [x] The focused TypeScript ordering/leak tests, native queue test, immediate-exit wire collector integration, real Python stdin test, and Rust `process_e2e` all pass without post-exit polling. | - [x] Dedicated stacked `jj` revision `uosvolyk`; work-item row marked `done`. |
| 21 | - [x] Against the parent, `packages/core/tests/execute.test.ts` and `crates/client/tests/process_e2e.rs` configured an 8-byte limit but still returned all 9 captured bytes, proving both clients ignored the production limit. | - [x] Shared-core per-stream/aggregate/bound tests, native frame-budget, stdout-backpressure, aggregate-budget, and real JavaScript/Python/WASM terminal-overflow tests, all browser wire tests including aggregate reuse and suppressed-event draining, Rust/TypeScript terminal-source/ordering tests, and real TS/Rust 8-byte-limit E2Es pass. The full TypeScript execute suite also proves ordinary output no longer floods the structured limit-warning buffer. Raw `spawn` and `captureStdio: false` stream all 9 bytes without capture. | - [x] Dedicated stacked `jj` revision `yoktzlwv`; final Rust-retention dependency closed by item 22. |
| 22 | - [x] Review plus the new transport/stream regressions demonstrate that the parent retained up to 4,096 large global events, accepted same-process events from the wrong owner, and silently skipped forced lag in wire, byte, session, permission, agent-exit, and actor consumers. | - [x] `agentos-sidecar-client` exact-owner/process isolation, fast/slow subscriber, negotiated byte/count retention, drop/close, atomic response binding, cancellation-tombstone, buffered-response cleanup, and process/control isolation tests pass (29 total). `agentos-client` typed byte/session/process/shell/cron failure, late-subscriber, permission-slot-only bridge, `SIGKILL`, and fail-closed cleanup tests pass (52 total). Actor `streamError` tests cover process, shell, session, permission, agent-exit, and cron pumps; all 9 actor units and 12 action-contract tests pass. `cargo check --workspace`, `cargo fmt --all --check`, scoped `@rivet-dev/agentos` typecheck/build, and real serial Rust process (2), shell (1), and ACP session (1) E2Es pass. The root `pnpm build` remains blocked by the separately logged pre-existing OpenCode/Bun postinstall environment issue. | - [x] Dedicated stacked `jj` revision `snorouxn`; work-item row marked `done`. |
| 23 | - [x] Before the fix, `packages/core/tests/allowed-node-builtins.test.ts` received `keepStdinOpen: undefined` for explicit `streamStdin: false`, and `packages/runtime-core/tests/sidecar-process.test.ts` encoded no `keep_stdin_open`; both focused suites failed while true and omission passed. The parity audit found the same `false`-to-omission conversion in Rust. | - [x] Core proxy (4 tests), runtime wire/generated-payload (14 tests), Rust client (53 tests, including the three-state request builder), and sidecar execution-default (3 tests) suites pass; both affected TypeScript package typechecks and Rust formatting pass. These prove false, true, and omission remain distinct and only omitted PTY input receives the sidecar default. | - [x] Dedicated stacked `jj` revision `xrouuwrl`; work-item row marked `done`. |
| 24 | - [x] Before the fix, `packages/core/tests/process-event-ordering.test.ts` observed `closeStdin` while the write was still blocked, then observed `execArgv` resolve successfully despite a rejected write; Vitest also reported the dropped promise as an unhandled rejection. | - [x] Five focused tests prove blocked write → blocked EOF → completion ordering and separate write/EOF rejection propagation. The full real `execute.test.ts` suite passes 10/10, including byte-exact 1 MiB stdin, and core TypeScript compilation passes. | - [x] Dedicated stacked `jj` revision `tkwqskvw`; work-item row marked `done`. |
| 25 | - [x] Before the fix, `packages/core/tests/toolkit-permissions.test.ts` invoked the captured production callback with `{ value: 1 }`; the non-idempotent Zod transform ran twice and `execute` received `{ value: 3 }` instead of `{ value: 2 }`. | - [x] Focused production-callback tests prove one transform, parsed hostile-key stripping/no prototype pollution, invalid-input rejection without execute, and forged legacy-shape rejection; `host-tools-zod` tests and core TypeScript compilation pass. The unrelated stale permission expectations are item 62. | - [x] Dedicated stacked `jj` revision `xuuxpqsy`; work-item row marked `done`. |
| 26 | - [ ] `packages/runtime-core/tests/protocol-client.test.ts` demonstrates that a rejection code exists only in `message`. | - [ ] Filesystem, permission, process, and cron errors expose stable structured `.code` values. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 27 | - [ ] `packages/core/tests/options-schema.test.ts` proves malformed software input is silently discarded. | - [ ] TS rejects structurally unserializable input; native package tests retain semantic format/projection validation. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 28 | - [ ] `packages/core/tests/session-config-routing.test.ts` detects a client-owned permission timeout racing the adapter. | - [ ] Native ACP timeout/default tests pass and the client test proves no local policy timer is created. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 29 | - [ ] `packages/core/tests/process-management.test.ts` demonstrates retained exited routes growing beyond sidecar history. | - [ ] Sequential-process coverage proves bounded client routing and sidecar-authoritative late lookup/wait behavior. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 30 | - [ ] `crates/client/tests/session_lifecycle_e2e.rs` demonstrates session growth and suppressed `DisposeVm` failure. | - [ ] Repeated create/shutdown returns server session count to baseline and injected disposal failure remains retryable. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 31 | - [ ] `packages/core/tests/software-projection.test.ts` and `crates/client/tests/link_software_e2e.rs` demonstrate stale post-create enumeration. | - [ ] Both clients observe live package/agent/command projection changes without configuration-time caches. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 32 | - [ ] TS `session-cleanup.test.ts` and Rust `session_e2e.rs` inject a failed close and demonstrate lost routing. | - [ ] Routes survive failed close, a retry succeeds, and confirmed close removes them in both clients. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 33 | - [ ] `packages/core/tests/session-event-ordering.test.ts` injects an event/state failure between create response and route registration. | - [ ] No event is lost and no live session is orphaned on create/resume failure. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 34 | - [ ] Native/browser ACP conformance fixtures demonstrate prompt/config divergence. | - [ ] `crates/agentos-sidecar-core/tests/acp_conformance.rs` passes identical create/resume/prompt/config cases through both adapters. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 35 | - [ ] `crates/client/tests/session_e2e.rs` demonstrates dropped `adapter_entrypoint` and silently shortened malformed values. | - [ ] Complete field parity and typed malformed-value failures pass. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 36 | - [ ] `crates/agentos-sidecar/tests/acp_extension.rs` injects projected-state and cleanup failures and observes masking. | - [ ] Original discovery failures and deterministic aggregated cleanup failures are returned or logged. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 37 | - [ ] `crates/client/tests/cron_e2e.rs` demonstrates a failed Rust callback recorded as success. | - [ ] Rust and TypeScript record the same durable failed run and preserve alarm/wake behavior. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 38 | - [ ] `scripts/verify-thin-client-docs.mjs` detects deny-by-default claims that contradict implementation. | - [ ] The verifier and `pnpm --dir website build` pass with explicit allow-all omission guidance. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 39 | - [ ] `packages/core/tests/readme-quickstart.test.ts` executes the current README quickstart and demonstrates missing Pi software. | - [ ] The checked explicit-package quickstart runs/typechecks successfully. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 40 | - [ ] The actor persistence test is invoked without `AGENTOS_SIDECAR_BIN` and demonstrates a false-success skip. | - [ ] CI builds the sidecar and `cargo test -p agentos-actor-plugin persistence_e2e` executes real teardown/reboot restoration. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 41 | - [ ] Existing TS/Rust process-tree tests demonstrate duplicated orphan/self-parent/order behavior. | - [ ] Sidecar tree tests own those cases; client tests assert forwarding/parity only. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 42 | - [ ] `packages/typescript/tests/typescript-tools.integration.test.ts` fails when unnecessary `/tmp` creation is denied and cwd is omitted. | - [ ] Compile/run works with no bootstrap mkdir and consistent relative-path/cwd behavior. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 43 | - [ ] TS public type tests and Rust API tests identify accepted options with no observable effect or parity. | - [ ] `pnpm check-types`, Rust API tests, and retained-option E2E tests prove only implemented options remain. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 44 | - [ ] `crates/agentos-sidecar/tests/acp_extension.rs` demonstrates unknown methods emitting a host callback/wait. | - [ ] Unknown methods return `-32601` promptly without a client callback. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 45 | - [ ] Protocol fixture inventory proves production JSON/legacy helpers are used only by compatibility tests. | - [ ] BARE roundtrip/generated protocol tests pass after all fixtures migrate and the helpers are deleted. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 46 | - [ ] Rust serialization tests demonstrate omission and explicit default-valued input producing the same wire payload. | - [ ] Rust/TypeScript fixtures distinguish omission, explicit empty, and explicit default where the protocol requires presence. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 47 | - [ ] `packages/core/tests/sidecar-client.test.ts` documents manufactured lifecycle IDs/maps used by the production lease path. | - [ ] Lease lifecycle tests pass against direct sidecar VM administration with only host lease/refcount state. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 48 | - [ ] `packages/core/tests/overlay-backend.test.ts` demonstrates omitted mode being selected before sidecar resolution. | - [ ] Omitted mode follows the sidecar-resolved value while explicit modes and caller-owned bridge state remain correct. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 49 | - [ ] Dependency/import audit proves the listed production dependencies and `long-timeout` declaration are unused. | - [ ] Core build, typecheck, package smoke test, and lockfile checks pass after removal. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 50 | - [ ] Typechecking `public-api-exports.test.ts` exposes the unsupported `defineSoftware(string)` call. | - [ ] Public API/typecheck tests accept only `{ packagePath }` and prove legacy exports are absent. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 51 | - [ ] `scripts/verify-thin-client-docs.mjs` detects stale package, architecture, permission, and command claims. | - [ ] The verifier plus website build pass against the corrected CLAUDE/docs sources. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 52 | - [ ] TS/Rust routing tests demonstrate clients interpreting legacy permission method names. | - [ ] Native adapter conformance covers supported methods and clients route only the typed protocol callback. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 53 | - [ ] Event fixture/source inventory proves no producer emits structured `acp.session_event`. | - [ ] Typed ACP event coverage passes after the dead branch is removed. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 54 | - [ ] Protocol-client and Rust session tests demonstrate listener/serialization failures being swallowed. | - [ ] Failures propagate or produce structured host-visible warnings with no lossy collection. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 55 | - [ ] README API assertions identify `commandDirs`, `AgentConfig`, and obsolete `AgentRegistryEntry` fields. | - [ ] Generated/declaration-backed documentation checks pass with no hand-maintained stale inventory. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 56 | - [ ] A sidecar cron test forces async dispatch loss and demonstrates a stranded/unacknowledged run or stale alarm. | - [ ] Sidecar queue/cursor or reliable-callback tests prove replay/ack without duplicate execution; Rust/TypeScript/actor E2Es pass. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 57 | - [ ] Rust callback tests demonstrate `on_process_exit` logging route failure without notifying the callback. | - [ ] Rust/TypeScript parity tests deliver exit success or typed route failure without a fabricated code. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 58 | - [ ] A transport test sends Execute through the generic request path and cancels after enqueue, demonstrating missing atomic route cleanup. | - [ ] Compile-time/API tests make generic Execute impossible and specialized cancellation tests remain green. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 59 | - [ ] TS/Rust tests inject write and EOF failures after a successful Execute response and demonstrate the started process remains live/untracked. | - [ ] Sidecar atomic-input or client fail-closed cleanup tests prove no post-start stdin failure can orphan a process and the original typed error is preserved. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 60 | - [ ] A shell CLI unit test rejects one queued stdin write and demonstrates the later EOF callback never reaches the process. | - [ ] Queue-failure tests prove the error is host-visible and the process is closed or killed without a hang. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 61 | - [ ] Host-tool registration tests demonstrate Zod transform/custom-refinement schemas being rejected before a callback can use them. | - [ ] Registration, structural sidecar dispatch, and single host-parse tests cover transforms/refinements without pretending JSON Schema implements their semantics. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 62 | - [ ] The full toolkit permission suite demonstrates three expectations tied to omitted-deny/client-enforcement behavior. | - [ ] Omitted allow-all and explicit sidecar deny tests pass, and direct callback tests no longer claim to enforce binding policy. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |

## Decisions and explanations

### 2 — Filesystem bootstrap

The old TypeScript bootstrap reconciled the kernel VFS with a sidecar shadow
root used by WASM/host-backed execution. That is not a reason for client-side
filesystem authority. The VFS already supplies its minimal root, and the
sidecar creates its shadow-root directories through trusted internal operations
before restoring the guest policy. A VM with explicit deny-all fs rights now
boots successfully while guest writes after readiness still fail with `EACCES`.

### 4–5 — Terminal and process protocol

The kernel PTY is the terminal; no layer should emulate one. The protocol should
start a process with argv/env/cwd and optional PTY dimensions/modes, return a
stable process ID and PTY handle, stream ordered events, accept stdin/EOF/resize
and POSIX signals, and expose the authoritative kernel exit status. `openShell`
is only `sh` attached to a PTY. The kernel already owns PID parentage, process
groups, sessions, signals, and reaping, so clients should only correlate IDs and
events returned by the protocol.

The clients previously manufactured a numeric PID because `Kernel.spawn()` and
`openShell()` returned synchronously while process creation over the sidecar was
asynchronous. Both APIs now await creation and return the sidecar's kernel PID;
writes, resize, signals, EOF, and wait address the returned process over the
protocol. Launch failure rejects creation instead of creating a fake process that
later exits with code 1.

`ExecuteRequest.pty` is now that standard terminal interface: presence requests
a kernel PTY, optional `cols`/`rows` set its initial window, and the existing
process handle receives stdin, EOF, resize, signal, output, and exit operations.
`keepStdinOpen` separately expresses streaming input without leaking the
executor's private control environment onto the client API. Browser execution
returns a typed unsupported response for PTY requests until its adapter provides
a real terminal implementation.

### 8 — ACP ownership

Filesystem and terminal ACP methods operate on VM state, so the active native
ACP extension implements them directly. `clientCapabilities` describes the
sidecar host surface; an upstream adapter may use any subset it understands.
Non-native/browser adapters that do not install that host surface must omit the
capability and return the ACP-standard unsupported/method-not-found response;
client callback code is never used to manufacture support.

### 9 — Zod tools

TypeScript remains the tool authoring surface: callers define tools with Zod,
the client converts the supported subset to JSON Schema, and the host callback
uses Zod to validate structured input. The sidecar uses the forwarded schema to
build guest CLI/help metadata, parse CLI or JSON input, enforce binding policy,
bound the callback, and send one structured callback payload to the client.

Toolkit command names are sidecar policy. The registration protocol therefore
contains no command-alias fields: both clients send only the toolkit name,
description, and tool definitions, and the sidecar derives `agentos` plus
`agentos-<toolkit>`. Empty toolkit collections are omitted.

### 10 — Caller-owned filesystem state

TypeScript still has in-memory layer and exact `js_bridge` mount helpers because
those objects contain caller-owned JavaScript callbacks and memory that cannot
cross the process protocol. They are host resources, not a second guest VFS:
the sidecar chooses every mount/filesystem operation and invokes the exact
registered callback when needed. They do not create startup directories,
materialize a root, grant permissions, or perform VM bootstrap. Root defaults,
bootstrap, Linux path behavior, mount policy, and guest-visible errors remain
sidecar/kernel-owned.

### ACP permission callbacks

Permission handlers are host callbacks and must remain routable through the
clients. The clients now return an optional answer only when a host handler
actually responds. Missing sessions, missing handlers, dropped responders, and
timeouts produce no client-authored reply; the native ACP sidecar converts that
absence to its standard reject outcome. This keeps host-only callback state in
the host while putting the permission default in one adapter-owned place.

### 11 — Rivet actor wake integration

The sidecar owns schedule truth, but a host actor may suspend the VM. Emit an
idempotent `next_alarm_changed` integration event with the earliest durable wake
timestamp or `null`. Rivet's plugin ABI does expose `set_alarm`, but that method
only arms the actor's existing durable schedule queue; it does not deliver a
custom alarm event to this plugin. The AgentOS actor adapter must therefore
enqueue one generation-tagged internal `schedule_at` wake action. Rivet derives
and reports its alarm from that durable action. On wake, the adapter forwards the
generation to the sidecar, which makes obsolete/duplicate wakes no-ops,
reconciles missed fires, and publishes the next alarm. This hook wakes the
scheduler; it is not a second client/actor scheduler.

Actor sleep disposes the VM and its in-memory sidecar scheduler. Before teardown
the actor now requests a bounded, versioned, sidecar-owned snapshot, stores the
opaque JSON value in actor SQLite, and returns it to a fresh sidecar scheduler on
cold boot. The sidecar validates and reconstructs grammar, job/run state,
counters, overlap state, and alarms; interrupted serializable runs are returned
for at-least-once delivery. The actor never parses jobs, applies defaults, or
becomes another scheduler. A real-sidecar regression test schedules a job,
tears the VM down, boots a new VM, and observes the restored registry.

### 13 — Atomic VM initialization

`initialize_vm` is one sidecar-owned transaction: create the VM, reach ready,
apply explicit mounts/packages, register host callback metadata, and either
return the resolved VM view or dispose every partially created resource. The
clients do not wait for lifecycle events or decide initialization order.

One host-only route must exist before the request in the Rust actor path: a
`js_bridge` mount can receive filesystem calls while the sidecar is applying
explicit mounts. The Rust client therefore installs the opaque callback route
before forwarding `initialize_vm` and removes it if initialization fails. This
does not bootstrap the filesystem or grant guest filesystem rights; the sidecar
still chooses and performs every filesystem operation. The callback is the only
component capable of reaching the caller-owned filesystem object. TypeScript
keeps the equivalent exact mount-id-to-object callback for the same reason.

## Additional legacy findings

- **18.1 — done / high confidence:** Deleted the former compatibility runtime,
  which contained a second bootstrap, process/terminal implementation, native
  sidecar lifecycle, kernel proxy, and options schema. Benchmarks and active
  tests now use the public protocol-owned runtime path; generated Secure Exec
  compatibility consumes the AgentOS public packages instead of this duplicate.
- **18.2 — done / high confidence:** Audited the broad TS/Rust filesystem, cron,
  session, process, and mount surfaces under items 4–13. Their authoritative
  state/defaults now live in the sidecar; remaining client maps contain only
  host callbacks, event subscriptions, and caller-owned resources that cannot
  cross the protocol.
- **18.3 — done / high confidence:** Removed the dead configure-time
  `bootstrapCommands`/`toolShimCommands` wire fields and TS alias replay.
  Sidecar toolkit registration already installs the authoritative command
  driver and aliases. Updated the stale `secure-exec-sidecar` protocol golden to
  the canonical AgentOS schema name.
- **18.4 — done / high confidence:** Removed TS package/policy replay and
  `registerLinkedPackage` state from mount reconfiguration. An omitted package
  list now preserves the sidecar's live package mounts, commands, agent launch
  records, and snapshot bundle; create-time permissions and loopback policy also
  stay sidecar-owned. Sidecar and thin-payload coverage own this behavior.
- **18.5 — done / high confidence:** Removed the dead configure-time
  `instructions` and `projectedModules` protocol/state fields. Removed
  `moduleAccessCwd` and the implicit `module_access` plugin in favor of the
  existing explicit `host_dir` mount. Cross-language protocol fixtures and
  sidecar host-mount/module-resolution coverage now prove the smaller surface.
- **18.6 — done / high confidence:** Made `packagesMountAt` optional on the
  lockstep wire protocol. Rust, TypeScript, and the actor bridge now omit it by
  default instead of sending `""` or `/opt/agentos`; the package projection
  selects `/opt/agentos` inside the sidecar and preserves explicit overrides.
- **18.7 — done / high confidence:** Configure-time packages, command
  permissions, and loopback ports are optional patches rather than required
  empty containers. Mount-only client requests now omit all three; the sidecar
  preserves their authoritative state, while an explicit empty value remains a
  real override. Native package/loopback and browser command-permission tests
  cover preservation across a later omitted patch.
- **18.8 — done / high confidence:** Configure-time mounts are also an optional
  patch. The sidecar separately tracks explicit operator mounts and its derived
  package mounts, so package-only initialization omits `mounts`, omission
  preserves operator mounts, and an explicit empty mount list clears only the
  operator layer. Sidecar and TS reconfiguration tests cover all three cases.
- **18.9 — done / high confidence:** Removed both clients' `agentos`/toolkit CLI
  parsers, help/list metadata builders, indirect dispatch paths, callback timeout
  races, tool-reference markdown builders, and cached prompt reference. The
  sidecar now derives all of this from its registered toolkit state. TypeScript
  retains Zod conversion and callback `safeParse`; authoritative toolkit and ACP
  prompt tests moved to the native sidecar.
- **18.10 — done / high confidence:** Moved ACP terminal creation, stdin,
  bounded output capture, wait/exit, signals, resize, release, and session cleanup
  into the native ACP extension. Removed the TS/Rust terminal host dispatchers
  and the Rust-only duplicate shell fan-out implementation. The same sidecar
  integration suite now owns filesystem and terminal host-method behavior and
  asserts no client callback occurs.
- **18.11 — done / high confidence:** Removed the final inert `moduleAccess` and
  `moduleAccessPaths` options from the runtime-core test compatibility types.
  Explicit `host_dir` mounts remain the only host-path module access mechanism.
- **18.12 — done / high confidence:** Removed both clients' generic ACP host
  request parsers and method emulators. The client callback now returns no
  implementation for unknown adapter methods; the native ACP extension owns
  supported host methods and emits JSON-RPC method-not-found itself. Dedicated
  permission and Zod tool callbacks remain the only client-routed ACP hooks.
- **18.13 — done / high confidence:** Removed the Rust `ExecOptions` cwd default
  and the TypeScript proxy's implicit execute cwd/environment payloads. VM create
  responses now return the sidecar-resolved guest cwd and environment so clients
  can expose accurate read-only views without duplicating or retransmitting
  runtime defaults. Native/browser and TS/Rust serialization tests cover omitted
  overrides and the `/workspace` sidecar default.
- **18.14 — done / high confidence:** Replaced client-authored
  `AGENTOS_EXEC_TTY`, `COLUMNS`, `LINES`, and `AGENTOS_KEEP_STDIN_OPEN` execution
  flags with explicit optional `pty { cols, rows }` and `keepStdinOpen` wire
  fields. The native sidecar now creates/resizes the kernel PTY and translates
  streaming-stdin intent into executor-private state. TS payload coverage, the
  ACP terminal integration, and the Rust package-backed PTY E2E own the behavior.
- **18.15 — done / high confidence:** Removed the TypeScript and Rust raw-command
  parsers, guest-command-map checks, direct-vs-shell selection, shell wrapping,
  and the TypeScript synthetic interactive shell/prompt implementation. Clients
  forward the untouched command line through `ExecuteRequest.shellCommand`; one
  shared native/browser sidecar classifier preserves direct argv for plain
  commands, verbatim `sh -c` input for shell behavior, and typed rejection of
  blank input. Shared classifier tests, TypeScript wire tests, and the real Rust
  client/sidecar command-line E2E own the prior behavior.
- **18.16 — done / high confidence:** Removed the production TypeScript client's
  merged local/sidecar filesystem view and local mount policy. Public filesystem
  methods always traverse the sidecar; only an exact host filesystem callback
  registry remains for sidecar-originated `js_bridge` calls. Surfacing that
  boundary exposed and fixed two sidecar bugs: cross-mount `move` now returns
  Linux `EXDEV`, and unmount no longer leaks mirrored mounted files through the
  execution shadow root. Sidecar-owned mount tests cover both, while the TS
  integration retains transport and semantic round-trip coverage.
- **18.17 — done / high confidence:** Removed explicit `allowAll` policy
  construction from both direct and actor-backed `agentos-shell` startup. The
  shell now omits permissions like every normal client and receives the shared
  allow-all default from the sidecar; explicit caller overrides forwarded by
  actor options remain intact.
- **18.18 — done / high confidence:** Removed Rust's `mount_fs`/`unmount_fs`
  methods, 25-method host filesystem trait, mount options, and local mount map.
  That API never crossed the protocol and no filesystem request consulted the
  map, so it falsely reported successful mounts that were invisible to the VM.
  `MountConfig` now represents only sidecar-native plugin mounts; the formerly
  rejected plain and overlay variants are gone.
- **18.19 — done / high confidence:** Removed TS/Rust absolute/normalized path
  guards and protected-directory string policy. The shared native/browser
  sidecar now resolves relative and non-normalized request paths against the VM
  cwd before kernel dispatch; empty paths return `ENOENT`, and kernel/VFS policy
  supplies Linux errno behavior. Rust directory typing now consumes the one
  typed `readDir` response instead of issuing a client-side `lstat` per child,
  and recursive mkdir is one native operation in both clients.
- **18.20 — done / high confidence:** Removed client-only `writeFiles` and
  `readFiles` partial-result loops plus basename-exclusion filtering from both
  SDKs and the actor contract. These were convenience semantics, not Linux or
  protocol operations, and moving them into the sidecar would have added an
  unnecessary second batch API. Examples now compose ordinary `mkdir`,
  `writeFile`, and `readFile` calls explicitly.
- **18.21 — done / high confidence:** Removed the still-published
  `runtime-core/cargo` helper and the test runtime's duplicate source-tree mtime
  scan, automatic `cargo build`, Rust toolchain discovery, environment mutation,
  and repository-cwd launch. Even compatibility/test runtime startup now uses
  only the explicit sidecar override or published platform binary.
- **18.22 — done / high confidence:** Removed the TypeScript and Rust terminal
  defaults for `sh` and streaming stdin (including TypeScript's extra `-i`). A
  PTY execute request with no explicit executable now selects the standard
  `sh` terminal and live stdin in one shared sidecar normalization; explicit
  executable and stdin choices remain untouched. Shared default tests and the
  TypeScript omitted-payload test own this boundary.
- **18.23 — done / high confidence:** Removed TypeScript/Rust synthetic PID
  allocators, PID remapping, background spawn launch, shell readiness watches,
  and pre-start operation queues. Process and shell creation now await the
  sidecar, expose the returned kernel PID, and reject launch failures directly.
  Clients retain only the host callback/event correlation that the sidecar
  cannot perform. Real SDK lifecycle tests verify PID identity and behavior.
- **18.24 — done / high confidence:** Process snapshots previously discarded the
  kernel's argv/cwd/exit timestamp and both clients manufacture start/exit
  timestamps from when they happened to observe an event or snapshot. The
  kernel/sidecar snapshot now carries guest argv/cwd and start/exit timestamps
  end to end; TS/Rust observation caches, local fallback rows, and presentation
  overrides are deleted. TS process listing/tree now await a fresh sidecar
  snapshot and both clients preserve the kernel's stopped state.
- **18.25 — done / high confidence:** Removed the TS/Rust pending-session-request
  registries, method markers, local prompt cancellation, and background
  fire-and-forget cancel. A normal `session/cancel` request now uses the
  sidecar transport's existing blocking-request interrupt; sidecar tests own the
  synthetic cancelled-prompt and `via: "prompt-interrupt"` cancel responses.
- **18.26 — done / high confidence:** Moved duplicate session-id
  rejection for create and resume into both sidecar implementations and removed
  the TypeScript client collision checks. Removed TS/Rust modes, config,
  capabilities, agent-info, and synthetic-config caches; state getters now read
  the authoritative sidecar snapshot and surface transport failures. The
  live-session listing and idempotent awaited close are now sidecar protocol
  operations, so both clients also dropped closed-id tombstones and detached
  close registries. The remaining client session map contains only host
  callback/event/permission routes the sidecar cannot access. Native/core
  sidecar tests prove collision, ownership-filtered listing, and idempotent close;
  TS/Rust SDK integration tests prove the forwarding surface.
- **18.27 — done / high confidence:** Replaced both complete client cron
  schedulers with one shared sidecar implementation. The sidecar now owns cron
  and one-shot parsing, UUID/default-overlap selection, bounded job/run
  registries, allow/skip/queue policy, missed-fire coalescing, generation-tagged
  alarms, and lifecycle events. TS/Rust retain one absolute timer and host
  callback correlations only; client schedule/list/cancel are asynchronous
  protocol forwards. The actor plugin installs a narrow alarm hook that persists
  a generation-tagged internal `schedule_at` action without interpreting cron
  state. Shared sidecar tests cover grammar/defaults/overlap/errors, and native
  browser plus TS/Rust tests cover wire and public behavior. The actor now stores
  and restores a bounded opaque sidecar snapshot across full VM teardown, with a
  real cold-boot regression and at-least-once replay for interrupted runs.
  Serializable command actions execute inside both sidecars, native session
  actions execute through the native ACP adapter, and unsupported browser
  session actions complete with a typed cron error. Callback closures remain
  host-only by design.
- **18.28 — done / high confidence:** TypeScript and Rust still answered
  spawned-process list/get calls from client-cached command, argv, timestamps, and
  exit state, while process signals returned before the sidecar transport completed
  (Rust also discarded every signal error). Both list/get surfaces now await the
  sidecar's process snapshot and the local registries retain only PID-to-host-route
  state. Stop/kill now await and propagate the sidecar response; exited-process
  idempotence and the bounded 1,024-entry exited snapshot history are sidecar-owned.
  Native signal tests prove exited-vs-unknown behavior, and real TS/Rust SDK
  process suites prove fresh list/get state and awaited control.
- **18.29 — done / high confidence:** Both process event paths still
  invent successful exit codes when authoritative events disappear: TypeScript
  polls until a process vanishes and returns `0`, while Rust maps a closed event
  channel to `0`. TypeScript also turns an event-pump transport failure into exit
  code `1`. Those compatibility fallbacks are removed: waits resolve only from
  a sidecar `process_exited` event and reject on event-pump/channel failure.
  Focused TS/Rust regression tests prove transport loss cannot become exit `0`
  or `1`.
- **18.30 — done / high confidence:** Rust process stdin/EOF and shell
  write/resize/close operations now await and validate exact sidecar responses;
  TypeScript shell resize/close and both clients' process signals do the same.
  `ExecuteRequest.timeoutMs` is an explicit optional wire field enforced by the
  native sidecar process pump, which emits the real SIGKILL exit status; browser
  execution returns typed unsupported instead of providing partial client-timer
  behavior. TypeScript/Rust timeout races and detached control tasks are removed.
  VM disposal now completes every cleanup step and propagates aggregate failures,
  while startup cleanup logs secondary failures before preserving the primary
  error. Sidecar timeout/signal tests, TS/Rust process E2Es, payload tests, and
  focused teardown regressions own the behavior.
- **18.31 — done / high confidence:** `ExecuteRequest.processId` is optional and
  production TypeScript/Rust process and shell requests omit it. Native and
  browser sidecars allocate one bounded-monotonic correlation ID and return it
  with the authoritative kernel PID; clients retain only that returned ID for
  host output/control routes. Explicit IDs remain available to lower-level
  sidecar adapters and tests, with empty/current/retained collisions rejected by
  the native owner. Native/browser allocation tests, thin-payload coverage, and
  real TS/Rust process and PTY suites prove response-before-event correlation.
- **18.32 — done / high confidence:** Public shell handles now use the
  sidecar-returned process correlation ID; both clients removed synthetic
  `shell-N` allocators and bounded closed-shell tombstone/exit-code stores. Live
  maps contain only host output subscribers, explicit handle-closed state, and
  in-flight exit tasks/promises. Once an exit route drains, late wait and
  idempotent close read the sidecar's retained process snapshot instead of a
  client lifecycle cache. Real TS/Rust PTY suites prove sidecar identity,
  immediate closed-handle rejection, and repeated late waits.
- **18.33 — done / high confidence:** Removed production TypeScript's
  synchronous socket lookup, signal-handler, and zombie-timer caches/background
  refreshes, including the per-output signal query. Callers that need these
  diagnostics already use the awaited `SidecarProcess` queries and propagate
  transport errors. Deleted the duplicate cache-only tests; the lower-level
  sidecar query tests remain authoritative. The former static compatibility
  stubs were deleted with item 15.
- **18.34 — done / high confidence:** Replaced both production clients'
  create/readiness/configure/tool-registration state machine with the atomic
  `initialize_vm` transaction. Omitted mounts/packages remain omitted, the
  sidecar returns resolved cwd/env and projected software, and native/browser
  implementations dispose partial VMs on configuration or callback-registration
  failure. Rust's public client readiness timeout and event subscription were
  removed. A pre-initialization `js_bridge` callback route remains only where a
  caller-owned filesystem may be invoked during mount application; actor
  cold-boot coverage proves why that host-only route is required.
- **18.35 — done / high confidence:** Deleted core's 2,280-line duplicate test
  runtime and made its explicit `test/runtime` compatibility surface re-export
  runtime-core's existing test-only implementation. A regression exposed one
  behavioral divergence—unmounting before lazy kernel initialization did not
  remove the queued mount—so the surviving implementation now covers and
  preserves that behavior. Core, TypeScript-tools, and secure-exec type/public
  surfaces compile against the single copy. Item 15 remains open until those
  consumers migrate and the surviving compatibility runtime can be deleted.
- **18.36 — done / high confidence:** Removed the duplicated 120-second ACP
  permission timeout and Rust public timeout constant from both clients. The
  native ACP adapter now owns the bound and includes `timeoutMs` in its callback;
  TypeScript/Rust use only that forwarded value to bound host reply correlation
  and clean pending routes. Missing replies are now also sidecar-defaulted as
  recorded in 18.63. Sidecar callback coverage asserts the authoritative value,
  and client permission regressions retain host routing/warning behavior.
- **18.37 — done / high confidence:** Removed client-authored JavaScript
  `platform: "node"` and `moduleResolution: "node"` values. The VM config wire
  model now preserves omission separately from an explicit default override,
  TypeScript and Rust send only caller-supplied builtin/timer overrides, and the
  native sidecar resolves omitted platform and module resolution. Rust now also
  exposes the explicit high-resolution timer override for SDK parity. VM-config,
  Rust serialization, and TypeScript wire tests prove defaults remain omitted.
- **18.38 — done / high confidence:** Removed Rust's malformed ACP permission
  callback fallback from invalid JSON to `{}`; malformed trusted-sidecar input
  now fails the callback like TypeScript instead of changing its meaning.
  TypeScript's best-effort ACP/event subscriber boundary still isolates host
  callback exceptions from the transport loop, but now reports every such
  failure and malformed sidecar event through a host-visible warning instead of
  swallowing it. Focused Rust and TypeScript regressions cover both paths.
- **18.39 — done / high confidence:** Removed production TypeScript's dead
  native-mount credential/path parsers, duplicate host-path maps, and VM-to-host
  path resolver left behind after ACP filesystem handling moved into the
  sidecar. Also removed the unreachable local-VFS root snapshot fallback;
  `snapshotRootFilesystem` now always forwards to the authoritative sidecar.
  Mount, base-image, snapshot, and full native migration-parity suites pass.
- **18.40 — done / high confidence:** Removed production TypeScript's hidden
  `spawn(..., { shell: true|string })` compatibility branch. It was not in the
  public `KernelSpawnOptions`, had no Rust equivalent, and manufactured a lossy
  command line with `argv.join(" ")`. Public `exec` still forwards its caller's
  raw command line unchanged through `shellCommand`; focused coverage now tests
  that supported path. The duplicate branch in runtime-core is deleted with
  the compatibility runtime under item 15.
- **18.41 — done / high confidence:** Removed TypeScript/Rust root-snapshot
  fallbacks that invented mode, uid, gid, empty file content, and UTF-8 encoding
  when a sidecar response omitted them. The shared sidecar snapshot producer
  already serializes complete Linux metadata; clients now preserve it verbatim
  and reject malformed responses. Focused malformed/complete response tests and
  real TypeScript snapshot coverage pass.
- **18.42 — done / high confidence:** Removed ordinary filesystem response
  fallbacks that turned missing sidecar fields into `exists = false`, empty
  directory listings, or implicit UTF-8 file reads. The sidecar already emits
  explicit values for these operations; both clients now reject malformed
  responses rather than fabricate valid Linux results. TypeScript and Rust
  malformed-response regressions cover the boundary.
- **18.43 — done / high confidence:** Removed TypeScript/Rust `vm.fetch`
  response defaults for status text, headers, and body. Native and browser
  sidecars already normalize every HTTP response with those explicit fields;
  clients now only validate/decode them and reject malformed responses. Focused
  validation plus real guest-listener fetch coverage passes in TypeScript, with
  matching Rust deserialization coverage.
- **18.44 — done / high confidence:** Removed client-authored root-filesystem
  mode, base-layer flag, empty lower/bootstrap lists, and native-root read-only
  defaults. The canonical VM config preserves omitted and explicitly supplied
  default-valued fields separately; native/browser sidecar root conversion owns
  the effective ephemeral, base-layer-enabled, empty-layer, and writable-native
  behavior. TypeScript/Rust wire regressions, VM-config round trips, shared
  sidecar conversion tests, and real overlay/native-root E2Es cover the boundary.
- **18.45 — done / high confidence:** Removed client-authored mount
  `readOnly: false`, empty plugin config, and the host-dir/node-modules helper's
  implicit read-only policy. Optional mount fields now cross the lockstep
  protocol; the sidecar alone resolves an omitted mount to writable with `{}`
  plugin config, matching a normal Linux bind mount. Package projection remains
  explicitly read-only because it is built inside the sidecar. TypeScript/Rust
  omission tests, protocol default tests, host-dir integration, mount/native-root
  E2Es, and the anchored symlink-escape regression cover the behavior.
- **18.46 — done / high confidence:** Corrected the remaining item-3 leak:
  TypeScript and Rust no longer expand omitted permission-rule operations,
  paths, or patterns to `"*"`/`"**"`. VM config and the lockstep configure
  protocol preserve omission separately from explicitly empty lists; the shared
  sidecar permission evaluator applies wildcard semantics to omission and still
  rejects explicit empty fields. Client serialization, generated-protocol,
  shared evaluator, and native permission-flag tests cover the boundary.
- **18.47 — done / high confidence:** Removed TypeScript/Rust cron-event
  fallbacks that converted malformed sidecar completion/error records into a
  zero duration or generic `"cron action failed"` error. Both clients now
  require the sidecar-owned result fields and reject malformed records; focused
  cron manager regressions cover both missing-field cases.
- **18.48 — done / high confidence:** Removed TypeScript's limit-warning
  fallbacks that converted missing names and malformed measurements from the
  trusted sidecar into empty strings and numeric zeroes. Complete warnings are
  forwarded unchanged apart from number decoding; malformed warnings are
  rejected with a host-visible diagnostic, with focused dispatch coverage.
- **18.49 — done / high confidence:** Removed TypeScript ACP stderr/exit
  identity supplementation. The protocol already requires `sessionId`,
  `agentType`, and `processId`, and the native adapter always emits them; the
  client now forwards those values verbatim and uses its session map only for
  the host-only numeric pid. Rust already forwarded the sidecar identity.
  Focused stderr and exit regressions prove stale client session metadata can no
  longer replace adapter-owned event fields.
- **18.50 — done / high confidence:** Moved adapter-specific session config
  category resolution out of TypeScript and Rust. `setSessionModel` and
  `setSessionThoughtLevel` now forward only `sessionId`, category, and value in
  one ACP request; the shared sidecar resolver chooses the adapter-reported
  config id, applies read-only support, and produces the OpenCode-specific
  unsupported response for native and browser adapters. Shared resolver/core
  sidecar tests cover writable, missing, and read-only categories, while the
  TypeScript client test proves no metadata lookup or interpretation remains.
- **18.51 — done / high confidence:** Removed client session-lifecycle gates
  from ordinary ACP sends, cancellation, state/config reads, and legacy
  permission replies. The sidecar now decides whether the supplied session id is
  live and returns the authoritative error. `destroySession`/`destroy_session`
  issue only the sidecar close request instead of running a duplicate
  cancel-then-close sequence, and TypeScript no longer synthesizes an unsupported
  cancel result because the native ACP adapter already owns and tests its
  notification fallback. Local session entries now gate only genuine host event,
  permission, exit, and prompt-text routes.
- **18.52 — done / high confidence:** Moved `PromptResult.text` assembly and
  Rust's former byte/chunk bounds into one shared native/browser ACP sidecar
  accumulator. The sidecar still streams every live `session/update`, returns
  the bounded accumulated text on prompt responses, warns once at 80%, and
  fails with an actionable limit error. TypeScript/Rust prompts now send one
  request and consume the returned text without installing a client event
  subscription or requiring local session state. Shared limit, synchronous and
  resumable browser, real native ACP, and thin TypeScript tests cover the path.
- **18.53 — done / high confidence:** Removed the final client cron action
  executor. The sidecar protocol now emits a typed asynchronous cron dispatch;
  native/browser sidecars launch serializable commands themselves, suppress the
  command output the clients previously discarded, complete runs from the real
  exit status, and recursively launch queued follow-ups. Native session actions
  perform create/prompt/close through the native ACP extension. TS/Rust execute
  only opaque callback ids whose closures cannot cross the protocol. Shared
  decoding, native ACP, browser command, runtime event, and client leak-boundary
  tests cover the split.
- **18.54 — done / high confidence:** Removed VM-wide ACP instruction assembly
  from TypeScript. TS and Rust now forward an explicitly supplied VM instruction
  string once in `CreateVmConfig` and forward only per-session instructions on
  `createSession`; native/browser ACP extensions read the VM value from sidecar
  state and combine it with the session override. Shared combination tests,
  native ACP integration, browser wire-state coverage, and the TypeScript
  OS-instruction integration suite prove the sidecar-owned behavior.
- **18.55 — done / high confidence:** Removed the TypeScript companion package's
  hidden runtime-driver/kernel fallback, filesystem/mount discovery, permission
  synthesis, environment construction, and memory/CPU option translation.
  Callers now provide one already-configured `AgentOs` VM; the package writes a
  bounded request/runner, executes `node` once through the public protocol,
  parses the compiler response, and cleans up without disposing caller state.
  Its formerly skipped real-VM suite is enabled by default and all six tests
  pass, including project emit and caller-owned-state preservation.
- **18.56 — done / high confidence:** Deleted the redundant private in-repo
  `secure-exec` façade and its tests. Secure Exec compatibility packages are
  generated in the compatibility mirror directly from AgentOS public packages,
  so retaining a second hand-authored legacy export list only kept the removed
  runtime API alive and could drift from the generated shim.
- **18.57 — done / high confidence:** Migrated runtime benchmark setup to the
  public `AgentOs` API and deleted runtime-core's public `NodeRuntime`, legacy
  runtime, options schema, kernel proxy, compatibility tests, and exports.
  Browser worker drivers remain internal to `packages/runtime-browser`; they are
  not an SDK-side runtime implementation.
- **18.58 — done / high confidence:** Deleted the orphaned `registry/tests`
  compatibility harness, including its roughly 4,000-line private kernel proxy
  and test runtime. The root registry package was excluded from the workspace,
  its `check-types` script was a no-op, and its documented test recipe did not
  exist, so it provided no active coverage. Authoritative client, sidecar,
  kernel, and protocol suites remain in their owning packages.
- **18.59 — done / high confidence:** Removed TypeScript's remaining toolkit
  name/description policy validation and moved it to the sidecar. Then removed
  client-authored toolkit/registry command aliases from both SDKs and the wire
  protocol; the sidecar derives `agentos` and `agentos-<toolkit>`. TypeScript
  still owns Zod authoring, schema conversion, and callback `safeParse` as the
  explicit item-9 exception.
- **18.60 — done / high confidence:** Removed `connectTerminal` from both SDKs,
  including TypeScript host raw-mode/stdin/stdout/resize wiring and Rust's
  synthetic ACP-terminal reservation logic. `openShell` over the process/PTY
  protocol is the single terminal interface; applications explicitly connect
  its data, input, resize, signal, EOF, and wait operations to their terminal UI.
- **18.61 — done / high confidence:** Removed Rust-only fixed exec-output and
  VM-fetch response caps. Output and HTTP response bounds are already enforced
  by the sidecar; the Rust client now behaves like TypeScript and only consumes
  the bounded protocol result instead of killing or rejecting at a second
  client-authored threshold.
- **18.62 — done / high confidence:** Removed the final synthetic TypeScript
  shell-id fallback and unused process driver/cwd guesses. Shell handles require
  the sidecar process ID. Empty host-callback registrations and non-recursive
  filesystem flags are optional on the wire and omitted by both SDKs; native
  root plugin config is also omitted instead of being expanded to `{}`. Focused
  protocol and sidecar tests cover the sidecar defaults.
- **18.63 — done / high confidence:** Moved the ACP missing-permission-answer
  policy out of both clients. The callback protocol now carries an optional
  reply; clients forward an actual host answer or `None`, while the native ACP
  adapter maps absence to its standard reject result. Sidecar unit coverage
  proves both the missing and explicit-reply paths.
- **18.64 — done / high confidence:** Deleted the unexported duplicate sandbox
  provider/mount/toolkit implementation from core. It had its own mount-path,
  lifecycle, filesystem, and process-tool defaults but no production consumer;
  the supported `@rivet-dev/agentos-sandbox` package remains the single explicit
  integration surface, so no duplicate behavior was moved into the sidecar.
- **18.65 — done / high confidence:** Removed the unused session metadata map
  from both clients, the protocol, and native sidecar state; the browser sidecar
  had already ignored it and native never read it after insertion. Made overlay
  mode optional on the wire and defaulted omitted mode to ephemeral in shared
  sidecar code, removed the dead runtime-core root-descriptor converter that
  manufactured nested filesystem defaults, and stopped storing empty env/false
  stdin values in TypeScript process tracking. Required BARE collections such as
  execute `args` remain explicit serialization: an empty list means no additional
  argv entries and does not select runtime policy or a guest default.
- **18.66 — done / high confidence:** Deleted runtime-browser's test-only legacy
  VFS migration shim, which recursively walked a caller-owned TypeScript
  filesystem and manufactured root bootstrap entries. Browser converged tests
  now boot with the browser sidecar's root defaults, and the unused client-side
  snapshot-to-OPFS persistence adapter and its wrapper methods are removed.
  Explicit public AgentOS snapshot requests remain protocol operations; no SDK
  scans or reconstructs a filesystem during VM startup.
- **18.67 — done / high confidence:** Removed the second orphaned runtime-core
  root-descriptor serializer and its unused descriptor/lower aliases, plus stale
  core process, mount, snapshot, and error-classification imports/helpers left by
  the deleted compatibility runtime. Scoped lint and package typechecks now
  enforce that these legacy branches have no remaining consumers.
- **18.68 — done / high confidence:** Restored the Rust SDK's typed
  `SessionNotFound` result without reintroducing a client session registry gate.
  The ACP sidecar now emits the authoritative `session_not_found` error code for
  missing or cross-connection sessions; Rust maps that typed protocol result to
  its public error and TypeScript preserves the same code on the thrown error.
  The real Rust create/prompt/close lifecycle test covers the behavior.
- **18.69 — done / high confidence:** Removed unused agent packages from the
  TypeScript client's production dependency closure. Agent packages are package
  manager inputs, not hidden client runtime behavior: callers explicitly pass a
  package path, then `createSession(name)` forwards only the name and the sidecar
  resolves its manifest and ACP entrypoint. Updated stale integration fixtures
  that claimed agents were auto-projected to pass each tested agent package
  explicitly; the real Claude filesystem/session path now guards this boundary.
- **18.70 — done / high confidence:** Removed the last stale core re-export of
  runtime-core's deleted sidecar root-lower compatibility alias, plus unused
  process, JSON-RPC, cron, schema, and cached environment members exposed by a
  clean declaration rebuild and scoped lint. Explicit caller-supplied root
  lowers still serialize through the VM-config schema; the client does not
  manufacture or expose a second sidecar descriptor API.
- **18.71 — done / high confidence:** Removed the remaining TypeScript
  sidecar-handle session/VM metadata options, lifecycle fields, bootstrap
  cloning, and tests. This host-only metadata never reached a transport or
  runtime consumer. The handle retains only real host concerns—placement,
  cancellation, pooled-child ownership, disposal, and lifecycle visibility.
- **18.72 — done / high confidence:** Removed the final Secure Exec-branded
  handshake identities from the native and browser TypeScript transports and
  matched Rust's `agentos-client` identity. The non-empty fields are structural
  authentication data for the local stdio connection, not guest/runtime
  defaults; normal spawned sidecars do not configure a competing auth policy.
