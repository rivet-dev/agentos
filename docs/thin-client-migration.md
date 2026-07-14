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
| 28 | TypeScript and Rust use the sidecar's permission decision deadline as a client timer, so a client can race the authoritative default and a late reply can fall through a legacy request path. | Make the sidecar apply its default on a typed timeout; give clients only a strictly later route-cleanup deadline and reject replies to expired routes. | P1 | High |
| 29 | TypeScript retains every exited `ManagedProcess` for the VM lifetime. | Have the sidecar advertise one resolved terminal-route bound; compact success/failure routes and evict only terminal correlation by completion order in both clients. | P1 | High |
| 30 | Rust opens a wire session per VM but never closes it, and both client lifecycles discard retry state before remote VM/session disposal is confirmed. | Expose idempotent sidecar-owned session close, bound open sessions, propagate teardown failures, and release client routes/leases only after confirmed close. | P1 | High |
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
| 41 | TypeScript, Rust, and the actor façade independently expose a client-built tree derived from the authoritative flat process snapshot, despite no production consumer. | Remove the unused recursive convenience API and retain the sidecar-owned flat process table with `ppid` as the only system-wide process view. | P2 | High |
| 42 | The TypeScript compiler creates `/tmp`, disagrees on `/root` cwd, and retains a legacy filename. | Rely on the Linux base and one real process cwd without bootstrap writes. | P2 | Medium |
| 43 | Both clients expose ignored or behaviorally divergent process options: most never reach the wire, raw-spawn PTY works only in TypeScript despite a standard terminal API, and three advertised runtime-limit overrides never reach the executor constants. | Remove unsupported fields, keep PTY behavior on the sidecar-backed `openShell` terminal interface, retain only options that reach the sidecar, and classify fixed executor bounds honestly instead of exposing fake VM controls. | P1 | High |
| 44 | Unknown ACP methods make a pointless host round-trip. | Return `-32601` directly in the sidecar unless a real extension API exists. | P2 | High |
| 45 | Production protocol packages retain JSON and legacy test codecs despite lockstep releases. | Migrate fixtures to BARE/typed config and delete compatibility codecs. | P2 | High |
| 46 | Rust cannot distinguish omission from explicit default-valued configuration. | Use `Option`/presence-aware types and preserve presence on the wire. | P2 | High |
| 47 | TypeScript retains a synthetic sidecar lifecycle with manufactured IDs/maps. | Lease the real VM and retain only host lease/refcount state. | P2 | Medium |
| 48 | TypeScript chooses omitted overlay mode before sidecar resolution. | Preserve omission and consume the sidecar-resolved mode. | P2 | Medium |
| 49 | Core declares unused heavy dependencies and an orphaned declaration. | Remove them and regenerate locks. | P2 | High |
| 50 | A deprecated string package descriptor remains exported and used by a transpile-only test. | Remove it and typecheck the public API test. | P2 | High |
| 51 | Active guidance describes obsolete manifests, runtime architecture, permission defaults, and commands. | Align CLAUDE/docs with current architecture and verify them. | P2 | High |
| 52 | TypeScript still interprets legacy ACP permission notifications even though replies now require a typed callback route and adapter support is adapter-specific. | Move method compatibility into the native adapter and leave only typed callback routing in clients. | P2 | Medium |
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
| 63 | TypeScript process-terminal and ACP errors use anonymous `Error & { code }` objects, so callers cannot reliably identify them or recover the originating protocol detail. | Use exported structured operation errors that preserve the exact code, message, and source event/envelope without interpreting sidecar policy. | P2 | High |
| 64 | TypeScript and Rust cron clients parse error codes/message markers and replace sidecar rejections with client-owned schedule error classes; native and browser sidecars do not yet emit one stable semantic code set. | Define invalid/past-schedule codes once in shared sidecar cron handling, pass them through both adapters, and delete client-side message parsing/remapping. | P1 | High |
| 65 | Several TypeScript cleanup paths stringify multiple errors into one new `Error`, discarding structured codes and protocol details. | Throw `AggregateError` with the original errors and a contextual message so every typed cause remains inspectable. | P2 | High |
| 66 | The shell client probes package files/manifests, substitutes a local build directory, and silently skips missing, unreadable, or command-less packages before VM creation. | Forward the selected registry package refs unchanged and let sidecar package loading return the real typed Linux/package error; remove all development fallback and skip logic. | P1 | High |
| 67 | A synchronous TypeScript permission-handler exception leaves its pending reply route/timer alive until delayed cleanup and prevents later handlers from running. | Remove the route immediately, clear its timer, and surface the handler failure without selecting a permission result in the client. | P1 | High |
| 68 | The callback protocol cannot tell a client that the authoritative sidecar wait expired, requiring a conservative cleanup grace and allowing an ignored late extension result. | Add explicit callback cancellation or expiry acknowledgement so the sidecar terminates its wait and the client route exactly once. | P2 | Medium |
| 69 | A TypeScript process output handler can throw through the shared sidecar event pump, fail unrelated live process routes, and stop later event delivery. | Isolate each host callback failure, report it through a structured host-visible error path, and keep sibling handlers/process routes alive. | P1 | High |
| 70 | `NativeSidecarKernelProxy` persistently duplicates the latest complete sidecar process snapshot even though production `AgentOs` reads the returned snapshot directly and no production caller reads the cache. | Delete the unused cache and the legacy `Kernel.processes` fallback/requirement; retain only active event routing. | P2 | High |
| 71 | Native sidecar process history is shared across `spawn`, `exec`, and shell activity, so unrelated churn can evict a public-spawn snapshot while a client still retains its terminal route; browser snapshots expose only current executions. | Define an explicit sidecar/protocol-owned terminal-history lookup or expiry contract for both adapters, and have clients obey that signal without inferring policy from snapshot absence. | P1 | High |
| 72 | Rust retains broadcast senders and output-task handles in every terminal `ProcessEntry` until history pressure or VM shutdown. | Replace terminal entries with compact exit/failure correlation while preserving late wait/subscription behavior and the sidecar-advertised retention bound. | P2 | High |
| 73 | The exported browser converged-sidecar factory defaults to a no-op ACP execution bridge, while its real async worker/reactor exists only in tests and does not execute the projected packed entrypoint. | Make the pending boundary asynchronous and run the actual projected VFS entrypoint in the standard production runtime Worker; delete the synchronous fake executor and path-to-fixture-worker fallback. | P1 | High |
| 74 | After the TypeScript sidecar event pump fails, `startTrackedProcess` can still start a new process even though no consumer remains to deliver its output or exit, so the new process can hang indefinitely. | Make pump failure terminal for new starts: reject before Execute when failure is already known and close the concurrent start/failure race without adding client runtime policy. | P1 | High |
| 75 | Shared ACP missing-session lookups return generic `invalid_state`, breaking the clients' stable missing-session contract. | Add one sidecar-owned `session_not_found` error across the shared core and both adapters. | P1 | High |
| 76 | Rust process-global shared-sidecar transport tasks are owned by the first caller's Tokio runtime, so dropping that runtime leaves other live VM leases with an undriven cached transport. | Give the shared transport its own runtime/thread lifetime and prove a VM on another runtime remains usable after the creator runtime exits. | P1 | High |
| 77 | Native child shutdown can lose process ownership on cancellation or watchdog races, race a concurrent VM create, and publish disposed before termination is confirmed; TypeScript also ignores an unconfirmed post-kill exit. | Add one cancellation-safe host lifecycle per pooled sidecar that serializes create/dispose, supervises and confirms child reaping, and propagates the same failure contract in TypeScript and Rust. | P1 | High |
| 78 | The rebuilt real sidecar bypasses or loses part of its declared Linux root bootstrap for active VM roots: default roots lack `/etc/agentos`, and roots without the bundled base expose `/tmp` as `0755` instead of `01777`. | Make the actual sidecar-owned root creation path provide one Linux directory contract, or remove dead bootstrap machinery and correct stale coverage if those expectations are obsolete; never restore client bootstrap. | P1 | High |
| 79 | A VM whose guest policy denies filesystem writes can compile from stdin but cannot be disposed, so trusted lifecycle cleanup incorrectly depends on guest filesystem rights. | Keep disposal and sidecar-owned cleanup on trusted operator paths that do not consult guest write policy; preserve guest policy for executor-originated operations only. | P1 | High |
| 80 | Native execution implicitly translates host cwd and entrypoint paths into guest paths and can let an untrusted JavaScript child execute a raw host file without an explicit mount. | Remove every raw-host compatibility branch; protocol and child-process paths are guest Linux paths, and `host_dir` mounts are the sole supported host-access mechanism. | P0 | High |
| 81 | The test-only native `acp_legacy` harness retains a duplicate obsolete ACP permission state machine. | Replace the legacy harness with shared/generated protocol fixtures or delete it once its remaining coverage is mapped to authoritative ACP tests. | P3 | High |
| 82 | Native/shared ACP loops silently ignore complete response-shaped JSON that lacks an `id`, turning malformed adapter output into a 10-second to 10-minute timeout. | Make shared-core classification fallible and reject malformed response-shaped frames immediately with typed `invalid_state`, preserving existing abort cleanup; do not emit a JSON-RPC request error for a malformed response. | P2 | High |

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
| 26 | done | P1 / high confidence | Every normal sidecar rejection now becomes the exported `SidecarRequestRejected`, preserving the exact `.code` and message plus request ID, ownership, and the original response frame at the one shared protocol boundary. Runtime-core root, `sidecar-client`, and core root export the class; Rust already preserves normal rejection codes in `ClientError::Kernel`. Anonymous process/ACP errors, cron policy remapping, and cleanup flattening are separately tracked as items 63–65. |
| 27 | done | P1 / high confidence | Core and actor package-manager boundaries now accept only serializable path strings, `{ packagePath: string }`, and one-level meta-package arrays; malformed explicit entries fail before startup instead of disappearing. Normalizers are total and retain only exact-path deduplication. TypeScript does not inspect paths, files, package formats, manifests, commands, or projection; all semantic validation remains sidecar-owned. Rust already forwards every typed `PackageRef` without filtering. |
| 28 | done | P1 / high confidence | The native ACP sidecar owns the 120-second permission decision deadline and maps only a typed callback timeout to its default reject outcome; other transport failures still propagate. The protocol gives TypeScript/Rust only a strictly later 125-second host-route cleanup bound. Both clients preserve omission, remove every pending route/responder at cleanup, and reject late replies instead of issuing legacy RPCs. |
| 29 | done | P1 / high confidence | Native/browser sidecars advertise one sidecar-resolved completed-route retention value; TypeScript and Rust retain only that many terminal routes and evict them by completion order without limiting active sidecar-owned processes. TypeScript compacts successful/failed `ManagedProcess` routes to exit code or typed error, while native snapshot history uses the same resolved bound. Independent sealing review found no blocker. |
| 30 | done | P1 / high confidence | Added connection-owned `CloseSession` with bounded session admission and bounded terminal close-outcome history shared by native/browser sidecars. Success and cleanup failure remain ownership-safe and replayable; expired retained outcomes return a typed error instead of manufactured success. Rust validates before opening, authoritatively closes every failed post-open create, and clears host routes only after confirmation. TypeScript and Rust serialize concurrent teardown and retain retry state through failed remote disposal. Rust keeps one session per VM only because startup JS-bridge callbacks are session-keyed before a VM id exists. Independent reseal found no blocker. |
| 31 | done | P1 / high confidence | Removed the TypeScript projected-command registry and Rust projected-command/agent snapshots, including the Rust-only synchronous snapshot API. Both clients now retain only live sidecar-backed `providedCommands`/`provided_commands` and `listAgents`/`list_agents` queries; dynamic linking forwards the package and records no projected state locally. Real TS/Rust tests prove pre-link absence, post-link command/agent enumeration, and `$PATH` execution. Independent sealing found no blocker. |
| 32 | done | P1 / high confidence | TypeScript and Rust now retain the complete ACP event, permission, agent-exit, and pending-reply route until a matching sidecar close confirmation. Transport/rejection/unexpected/wrong-id failures preserve retry state; confirmed close finalizes it. TypeScript clears residual ACP routes after, and only after, authoritative VM/wire-session disposal succeeds. Rust already had the equivalent VM-shutdown ordering. Independent parity review found no blocker. |
| 33 | done | P1 / high confidence | Create/resume success responses now carry the complete sidecar-owned host-route identity. TypeScript and Rust install only their host callback/event routes synchronously while the response frame is dispatched, before the waiter wakes or the next event is handled; TypeScript no longer issues a bootstrap state request. Rust retains the bounded response hook after cancellation so an authoritative success is still routed, but the hook owns only a weak route-map reference and cannot retain the VM/client/transport graph. Independent reseal found no P0/P1/P2 blocker. |
| 34 | done (`pqpkrqpt`) | P1 / high confidence | Native and browser ACP use one shared behavioral core with adapter hooks. Independent reseal found no remaining P0/P1/P2 blocker. |
| 35 | done (`nnmknwoo`) | P1 / high confidence | Rust forwards the sidecar-resolved `adapter_entrypoint` and returns indexed `ClientError::AcpDecode` failures instead of shortening or normalizing malformed present ACP values. Independent reseal found no P0/P1/P2 blocker. |
| 36 | done (`lqprmlyn`) | P1 / high confidence | Discovery errors propagate unchanged. Shared/native/browser cleanup now uses typed ordered aggregates, bounded non-routable retry records, checkpointed lifecycle/signal/event phases, retained extension/worker handles, success-only close history, and forced disconnect reclamation. Independent shared/native/browser reviews found no remaining P0/P1 blocker. |
| 37 | done (`wzvurwvz`) | P1 / high confidence | Rust host callbacks now return `Result<(), String>` and forward the exact failure through the existing completion request; the sidecar alone classifies the run, emits `cron:error`, and terminalizes its state. The client retains only the closure/correlation plus the required alarm hook. Rust and TypeScript real-sidecar regressions prove parity. Independent review findings were fixed; the discovered generic cross-runtime shared-transport lifetime defect is tracked separately as Item 76. |
| 38 | done (`twktuyvz`) | P1 / high confidence | README, paired website guidance, architecture pages, networking/Python docs, comparison copy, and the permissions example now state the sidecar-owned allow-all omission behavior while preserving explicit rule-set deny semantics and VM capability boundaries. A reusable source/public claim verifier and CI gate reject contradictory defaults and require positive omission guidance. No runtime or client policy changed. |
| 39 | done (`unxzlvkx`) | P1 / high confidence | The Core README now imports Pi, projects it as explicit `software`, forwards the required API key, and cleans up the session and VM. Its runnable block is byte-aligned with the checked Pi-only example and executes under deterministic success/failure coverage; the real Pi SDK sidecar flow also passes. Independent review findings were resolved. |
| 40 | done (`ltnsrmlp`) | P1 / high confidence | The actor cron cold-boot E2E now fails closed without a real wrapper binary, executes the real shutdown path, starts a distinct sidecar, restores the opaque cron registry, and exercises final disposal. Regular, nightly, and local CI build the wrapper and provide its stable path. The separate pre-existing cross-client child-reaping and create/dispose races exposed during review are tracked as Item 77. No production lifecycle behavior changed here. |
| 41 | done (`qmzytqsv`) | P2 / high confidence | Removed `processTree` / `process_tree` and `ProcessTreeNode` from TypeScript, Rust, and actor APIs rather than adding an unnecessary recursive protocol for an unused convenience view. `allProcesses` / `all_processes` remains the bounded, permission-checked, sidecar-authoritative process table, with exact `ppid` lineage preserved for caller-side presentation. Generated declarations, actor contracts, active docs, and the website cache no longer advertise the recursive API. Independent reseal found no remaining P0/P1/P2 issue. |
| 42 | done (`suwmustu`) | P2 / medium confidence | The TypeScript compiler now sends source requests over stdin, preserves omitted cwd, resolves an explicit relative cwd once through the sidecar, and performs no client filesystem bootstrap. Native/browser execution use the same sidecar-owned cwd validation, and the legacy secure-exec transport filename is gone. |
| 43 | done (`orpyyprl`) | P1 / high confidence | TypeScript and Rust process options now expose only fields that reach the sidecar. Removed ignored per-exec filename/CPU/timing controls, raw-spawn stdin/capture/stdio/fd controls, and the orphaned timing types; Rust spawn options are flat and parity-aligned. The functional TypeScript-only raw-spawn PTY divergence is removed in favor of the shared sidecar-backed `openShell` terminal interface. Three falsely configurable JavaScript executor bounds are no longer client/VM options and remain fixed, documented implementation safeguards at their actual executor codec/buffer sites. |
| 44 | pending | P2 / high confidence | Unknown ACP methods make a host round-trip even though TypeScript has no extension handler and always returns null. Return method-not-found directly in the sidecar unless a real host-extension API exists. |
| 45 | pending | P2 / high confidence | Production protocol packages retain a JSON payload codec and a large legacy test configuration parser despite lockstep releases. Migrate fixtures to BARE/typed configuration and delete compatibility paths. |
| 46 | pending | P2 / high confidence | Rust cannot distinguish omitted presence-sensitive configuration from explicitly supplied default-valued input. Represent presence with `Option` and preserve it on the wire. |
| 47 | pending | P2 / medium confidence | TypeScript retains a synthetic `AgentOsSidecarClient` lifecycle with IDs and maps unrelated to the authoritative wire lifecycle. Lease the real VM directly and retain only host lease/refcount state. |
| 48 | pending | P2 / medium confidence | TypeScript chooses the omitted overlay mode as `ephemeral`, duplicating the sidecar default. Keep the JS bridge host-owned but obtain the resolved mode from the sidecar. |
| 49 | pending | P2 / high confidence | Core declares unused heavy production dependencies and an orphaned `long-timeout` declaration. Remove them and regenerate locks. |
| 50 | pending | P2 / high confidence | The deprecated string package descriptor remains exported and a transpile-only test calls `defineSoftware(string)` despite the supported `{ packagePath }` type. Remove the legacy surface and typecheck the public API test. |
| 51 | pending | P2 / high confidence | Active CLAUDE/docs files describe obsolete JSON package manifests, an in-process runtime, contradictory permission defaults, and a deleted registry command. Align all guidance with the current architecture. |
| 52 | pending | P2 / medium confidence | TypeScript still invokes host handlers for legacy ACP permission notifications, but those notifications have no typed pending reply route and cannot be answered after item 28. Remove the dead client interpretation and keep adapter-specific method compatibility in the adapter/sidecar. |
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
| 63 | pending | P2 / high confidence | Process-terminal events and decoded ACP error responses currently create anonymous code-bearing errors. Replace them with exported structured host errors containing the unmodified sidecar/adapter code and full source event or ACP envelope; do not add client policy. |
| 64 | pending | P1 / high confidence | Cron schedule failures are interpreted independently by TypeScript and Rust through code/message substring checks, while native and browser adapters emit divergent generic codes. Establish `invalid_schedule` and `past_schedule` in shared sidecar cron behavior, then remove client normalization and legacy schedule-error policy. |
| 65 | pending | P2 / high confidence | Sidecar-session, lease, shared-sidecar, and injected-transport cleanup paths flatten multiple typed failures into joined message text. Preserve each original error in `AggregateError`, retaining stable codes and protocol context. |
| 66 | pending | P1 / high confidence | `packages/shell/src/main.ts` performs host `existsSync`/`statSync`/manifest reads, replaces missing package refs with one local native command directory, and skips packages on failure. Delete those probes/fallbacks and forward the statically selected package refs so the sidecar remains the only semantic validator. |
| 67 | pending | P1 / high confidence | A synchronous TypeScript permission-handler exception rejects the callback but leaves its pending reply entry and timer alive until delayed cleanup, and later handlers are not invoked. Remove the route immediately, clear its timer, and surface the handler failure without letting the client choose the permission result. |
| 68 | pending | P2 / medium confidence | The callback protocol has no sidecar-to-client cancellation/expiry signal, so permission routes need a conservative post-decision cleanup grace and may send an ignored late extension result. Add explicit callback cancellation or expiry acknowledgement so host routes can terminate exactly when the authoritative sidecar wait ends. |
| 69 | pending | P1 / high confidence | TypeScript invokes stdout/stderr listeners inside the shared sidecar event dispatch path. One throwing listener can reject the event handler, fail every process route attached to that transport, and stop the pump. Catch each host listener independently, emit a structured host-visible failure, and prove sibling routes continue. |
| 70 | pending | P2 / high confidence | `NativeSidecarKernelProxy.processes` retains a second copy of every entry in the latest process snapshot but has no production reader because `AgentOs.listProcesses/getProcess` use the directly returned authoritative snapshot. Delete this duplicate cache and the legacy kernel fallback surface. |
| 71 | pending | P1 / high confidence | Native snapshot retention is one shared completion history across direct spawn, finite exec, and shell activity, while browser snapshots expose only active executions. Mixed churn can remove a native public-spawn snapshot independently of the client route window, but clients cannot safely infer terminal expiry from snapshot absence. Add one explicit sidecar/protocol-owned terminal lookup or expiry contract across adapters; clients only consume that result. |
| 72 | pending | P2 / high confidence | Rust now bounds terminal entries using the sidecar-advertised count, but each retained entry still owns broadcast senders and output callback task handles. Compact terminal success/failure entries as TypeScript does while preserving typed late wait/subscription parity. |
| 73 | pending | P1 / high confidence | The public browser factory cannot launch a real ACP adapter by default: the production bridge is a no-op without an optional synchronous fake, and the browser-WASM fixture maps argv paths to prebuilt workers whose `.aospkg` entrypoints contain no executable adapter. Keep the resumable sidecar on the main thread, make its pending driver awaitable, and launch the real projected entrypoint in the existing production runtime Worker with exact owner/process correlation and bounded output. |
| 74 | pending | P1 / high confidence | A genuine TypeScript event-pump failure is retained in `pumpError`, but `startTrackedProcess` does not consult it before or while starting a process. Reject starts against a failed pump and make the Execute/route-registration race fail closed so no process can start without a live event consumer. |
| 75 | pending | P1 / high confidence | Item 34's shared ACP core classifies absent and cross-owner sessions as generic `invalid_state`, so Rust cannot preserve its existing `SessionNotFound` contract without client message parsing. Add `AcpCoreError::SessionNotFound`, emit stable `session_not_found` from all authoritative shared-core lookups, and preserve identical absent/cross-owner responses in native/browser conformance tests. |
| 76 | pending | P1 / high confidence | `SidecarTransport::spawn` places its reader, writer, and watchdog tasks on whichever Tokio runtime first creates a process-global shared sidecar. When that runtime exits while another runtime still owns a VM lease, the cached sidecar remains live but its transport is no longer driven. Move transport I/O to a lifetime-owned runtime/thread; do not duplicate sidecar policy in the client. |
| 77 | pending | P1 / high confidence | Rust `kill_child` takes and drops its `Child` after best-effort `start_kill`, so cancellation/watchdog overlap cannot retry or prove reaping; connection removal and `Disposed` publication also leave a gap where concurrent VM creation can install a new child. TypeScript suppresses kill failure and ignores an unconfirmed post-`SIGKILL` exit. Build one host-owned, cancellation-safe lifecycle gate per pooled sidecar, reserve creation under it, supervise child termination independently of caller cancellation, and publish disposed only after acknowledged reaping in both clients. |
| 78 | pending | P1 / high confidence | `kernel-bootstrap-base.test.ts` against the rebuilt real sidecar passes bundled-base `/tmp`, but overlay VMs bypass the bootstrap table and the no-base snapshot discards directory modes. Converge on one bounded sidecar-owned Linux root specification and keep all startup filesystem bootstrapping out of clients. |
| 79 | pending | P1 / high confidence | Native disposal calls guest-authorized `KernelVm::unmount_filesystem`, so global guest `fs.write` denial returns `EACCES` for a configured mount after execution succeeds. Add a narrowly scoped operator-only unmount seam for native/browser lifecycle cleanup while preserving guest unmount enforcement and every real teardown error. |
| 80 | done (`pzzlonpr`) | P0 / high confidence | Native cwd, command, JavaScript, Python-file, and Wasm paths are now guest VFS paths. Raw-host compatibility translation/materialization is gone; explicit `host_dir` mounts are the only host filesystem bridge, package links resolve through bounded guest-VFS `realpath`, and V8 `fs.realpath` delegates to the sidecar instead of duplicating symlink policy. |
| 81 | done (`sqnqyqws`) | P3 / high confidence | Deleted the 4,786-line test-only ACP client/session state machine, its two integration roots, and the unused native typed JSON-RPC codec. Three real contracts now live at authoritative production layers: permission option aliases in the native ACP extension, initialize-version mismatch cleanup in the shared core, and non-string config values in shared behavior. |
| 82 | done (`vsqvzlkn`) | P2 / high confidence | The shared ACP classifier now rejects every complete non-protocol envelope and gives response-shaped messages without `id` a focused `invalid_state`. Blocking and all four resumable paths propagate the same error without writing an uncorrelated `-32600`; native abort cleanup waits within its existing bound for adapter exit before dropping the route. No client parser or compatibility policy was added. |

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
| 26 | - [x] Against the parent behavior, `packages/runtime-core/tests/protocol-client.test.ts` failed because `EACCES` existed only inside a generic error message; the error had no `.code`, request identity, ownership, or rejected response frame. | - [x] The shared protocol boundary regression passes, all 121 runtime-core tests pass, the core root-export suite passes 6/6, the runtime-core build succeeds, and both package typechecks pass. Because every normal filesystem, permission, process, and cron request uses this boundary, each rejected response now exposes the same structured `.code` and full frame. | - [x] Dedicated stacked `jj` revision `vpwruksl`; work-item row marked `done`; independent sealing review found no blocker. |
| 27 | - [x] Against the parent behavior, six core schema cases accepted malformed entries (`undefined`, `null`, boolean, number, empty object, and non-string `packagePath`), while the actor bridge test showed `{ packagePath: 42 }` was silently omitted. | - [x] Core schema tests pass 12/12, including valid raw/object/meta/future-field inputs; the full actor bridge suite passes 15/15 with explicit local native binaries; both package typechecks pass. Native sidecar package projection passes 11/11 for missing/invalid manifests, invalid entrypoints, duplicate commands, and mount behavior, proving semantics stayed authoritative there. | - [x] Dedicated stacked `jj` revision `ysymytqk`; work-item row marked `done`; independent sealing review found no blocker. |
| 28 | - [x] Against the parent behavior, the initial TypeScript regression passed the callback's fourth argument as 10 ms and observed the callback settle locally; source audit confirmed that value directly drove the client timer and found the equivalent Rust `select!` race. | - [x] Native timeout/default and ACP integration tests prove typed timeout → reject, non-timeout propagation, and cleanup 125 s > decision 120 s. TypeScript permission routing passes 9/9; all 55 Rust client units pass, including retained-responder cleanup and reply races; core build, workspace check, and Rust formatting pass. | - [x] Dedicated stacked `jj` revision `ysnlrxzo`; work-item row marked `done`; independent sealing review found no remaining blocker. |
| 29 | - [x] `packages/core/tests/leak-agent-os-processes.test.ts`'s 1,025-completion regression fails against the parent because all 1,025 entries remain heavyweight `ManagedProcess`/listener routes; source audit found the Rust client also copied a fixed 1,024-entry policy and pruned by PID rather than completion. | - [x] Six focused TypeScript leak/correlation cases, nine real process-management cases, all 56 Rust client units, 26 runtime protocol/initialization cases, four native initialization tests, all 30 browser wire tests, and 99 shared sidecar-core tests pass. They prove default/raised protocol propagation, lightweight success/failure correlation, completion-order eviction, in-flight waiter delivery after pruning, and no client-owned active-process admission limit. | - [x] Dedicated stacked `jj` revision `lxmkzylx`; work-item row marked `done`; independent sealing review found no blocker. |
| 30 | - [x] Source audit proves each Rust VM opens a session that `shutdown` never closes, ignores `DisposeVm`, marks itself disposed, and drops its lease/routes before confirmation. The new TS retry regressions in `leak-rpc-client.test.ts` and `sidecar-client.test.ts` fail against the parent because the first rejected teardown still clears state or permanently marks the client disposed. | - [x] Shared core (101), native close (5), browser wire (33), native lib (89), limit audit (2), and protocol (25) tests prove bounded admission/history, exact parity codes, ownership, idempotent success, stable failed-close replay, and typed expiry. Rust client units (60) plus real failed-create churn/concurrent shutdown, lifecycle, and shared-pool E2Es pass. Runtime protocol (37), core retry (9), and real TS shared-sidecar (3) tests pass; both TS typechecks, workspace Cargo check, formatting, diff check, and fixed-version check pass. Website source/public docs match; the website build remains blocked by the already-logged absent vendored theme in this checkout. | - [x] Dedicated stacked `jj` revision `xwpzpllv`; work-item row marked `done`; independent reseal found no P0/P1/P2 blocker. |
| 31 | - [x] Before removal, `packages/core/tests/agentos-base-filesystem.test.ts` asserted the client-mirrored `kernel.commands` map, while the Rust source/API audit found an untested synchronous `projected_agents()` snapshot and a never-read command cache; the existing TS/Rust dynamic-link E2Es preserved real command-resolution behavior. | - [x] `agentos-package-link-vm.test.ts` passes 4/4 and `link_software_e2e.rs` passes 1/1, proving live sidecar command/agent enumeration before and after linking plus actual `$PATH` execution; focused TS proxy tests pass 12/12, all 60 Rust client units pass, and both client type/check gates pass with no projected-state cache. | - [x] Dedicated stacked `jj` revision `molyqylu`; work-item row marked `done`; independent seal found no P0/P1/P2 blocker. |
| 32 | - [x] The new `session-config-routing.test.ts` failure/retry cases fail against the parent because `_sessions` and pending replies are removed before the injected failure; the Rust source audit found the identical pre-send removal, while existing TS/Rust session E2Es covered only successful/idempotent close. | - [x] TypeScript focused routing/disposal tests pass 9/9; all 61 Rust client units pass, including transport/rejection/unexpected/wrong-id retention and matching retry finalization. Real Rust session, lifecycle, and wire-session lifecycle E2Es pass, and both client check/type/format gates pass. | - [x] Dedicated stacked `jj` revision `tlkwyuou`; work-item row marked `done`; independent parity review found no P0/P1/P2 blocker. |
| 33 | - [x] `packages/core/tests/session-route-registration.test.ts`'s create/resume immediate-event regressions fail against the parent sequence because it handles the success, sends `AcpGetSessionState`, and only then inserts `_sessions`; Rust source/transport audit found the equivalent response-await window and cancellation orphan risk. | - [x] TypeScript route tests pass 4/4 and runtime response-ordering tests pass 22/22. Rust sidecar-client tests pass 28/28 and client units pass 63/63, including immediate-event ordering, post-enqueue cancellation, and the weak-owner lifetime regression; protocol tests pass 9/9, shared sidecar core 21/21, generated TS protocol 2/2, and the real Rust ACP session E2E 1/1. Both TS typechecks, Rust checks/formatting, fixed-version, and diff gates pass. | - [x] Dedicated stacked `jj` revision `qlxnlvlz`; work-item row marked `done`; independent reseal found no P0/P1/P2 blocker. |
| 34 | - [x] Against item 33 (`066f6b51`), wrapper and resumable regressions demonstrated the native-only state machine, browser pending-response leak, prompt/config divergence, and missing browser production lifecycle. | - [x] The 8-case shared-core conformance suite, 15-case production-wrapper suite, 37 browser runtime-driver tests, 15 focused runtime-browser tests, complete Chromium suite (16 pass, 6 explicit skips), and focused terminal-buffer regressions pass with one sidecar-owned behavior implementation. | - [x] Dedicated stacked `jj` revision `pqpkrqpt`; work-item row marked `done` after independent reseal found no P0/P1/P2 blocker. |
| 35 | - [x] Against Item 34 (`ac77fa88`), `agent_registry_e2e` fails to compile because Rust's public `AgentRegistryEntry` has no `adapter_entrypoint`; the independently runnable malformed-config E2E reaches parent `filter_map` and returns one valid entry after silently dropping `configOptions[1]`. | - [x] Four focused decode unit tests pass. Strict real-sidecar `agent_registry_e2e` proves the resolved entrypoint survives discovery, BARE transport, and Rust mapping; `session_config_decode_rejects_malformed_entry_without_shortening` independently returns indexed typed failure without shortening. | - [x] Dedicated stacked `jj` revision `nnmknwoo`; work-item row marked `done` after independent reseal found no P0/P1/P2 blocker. |
| 36 | - [x] Revision `066f6b51` used `unwrap_or_default`/`.ok()?` for projected-agent discovery; `projected_agent_catalog_errors_propagate_without_becoming_unknown_agent` records the corrected distinction. Replaced parent tests recorded pending-state removal after failed abort, first-error-only cleanup, drained browser worker handles, and terminalized failed close outcomes. | - [x] Shared core: 77 unit + 8 conformance tests, including cleanup-only replacement close and event-backpressure retry. Native: 100 pass/1 ignored units, 11 ACP-wrapper units, 7 session-close integrations, focused cleanup-limit integration, and all 9 extension cases in isolated processes; the combined extension binary's pre-existing cross-test V8 SIGSEGV is logged. Browser: 85 native-browser + 15 ACP-wrapper tests. `cargo check --workspace`, formatting, and diff checks pass. Named regressions include `cleanup_error_code_and_display_are_stable_and_ordered`, `disposal_progress_checkpoints_lifecycle_events_and_signals_across_retry`, `connection_loss_forces_reclamation_after_cleanup_event_limit`, `committed_cleanup_events_backpressure_without_growth_and_deliver_once`, `release_execution_preserves_both_errors_and_retries_incomplete_phases`, and `browser_failed_close_retries_cleanup_before_recording_terminal_success`. | - [x] Dedicated stacked `jj` revision `lqprmlyn`; independent shared/native/browser reviews found no remaining P0/P1 blocker; work-item row marked `done`. |
| 37 | - [x] Against Item 36, `failed_cron_callback_is_recorded_as_error` cannot compile because `CronAction::Callback` requires `BoxFuture<Output = ()>`; source inspection confirms `run_host_action` discards the callback outcome and manufactures `Ok(())`, while an unavailable route logs and returns success. | - [x] All 69 Rust client units, the two-test real-sidecar cron E2E in three default-parallel runs, cron grammar, 10 shared sidecar cron tests, actor alarm-state persistence, 8 TypeScript manager tests, and the fixture-independent real TypeScript rejected-callback integration pass. Rust workspace check, scoped core build/typecheck, formatting, diff, and fixed-version gates pass. | - [x] Dedicated stacked `jj` revision `wzvurwvz`; independent review findings resolved; work-item row marked `done`. |
| 38 | - [x] Added the verifier first and ran it against Item 37's prose: it exited 1 with exact path/line diagnostics for both README claims and all stale permissions, security, networking, Python, architecture, filesystem/process, and comparison source/public pairs. | - [x] The 8-case verifier suite and 109-file repository audit pass; example permissions typecheck, native omitted/partial-policy unit, browser wire default, and 4 browser runtime policy tests pass. CI YAML and shell syntax parse. After materializing the local docs-theme checkout's repository-tracked generated assets, `pnpm --dir website build` passes and renders 134 pages. | - [x] Dedicated stacked `jj` revision `twktuyvz`; work-item row marked `done`. |
| 39 | - [x] Added `readme-quickstart.test.ts` before changing the prose; `projects Pi before creating the documented session` reached the fake sidecar invariant and failed with `unknown agent type: pi`. The old multi-agent example typecheck also failed on its unbuilt, unused OpenCode declaration. | - [x] All 3 executable-snippet tests pass, including exact checked-source equality and awaited cleanup after prompt failure. The pruned Pi-only example and Core package typecheck; the frozen lockfile check passes. After building the existing Pi package artifact, `pi-headless.test.ts` passes both real native-sidecar Pi SDK cases (1 intentional bash skip). | - [x] Dedicated stacked `jj` revision `unxzlvkx`; independent review findings resolved; work-item row marked `done`. |
| 40 | - [x] Against Item 39, the exact cold-boot test with `AGENTOS_SIDECAR_BIN` unset printed `skipping actor cold-wake cron test` and Cargo falsely reported 1 passed. | - [x] Unset and missing-file prerequisites now fail explicitly. After building the real wrapper, all 3 actor persistence tests pass, invoke shutdown, launch a distinct second sidecar, restore the cron registry, and exercise final disposal. Actor/client checks, workflow YAML parsing, shell syntax, formatting, and diff checks pass; scoped Clippy remains blocked by the logged pre-existing `agentos-vm-config` `derivable_impls` lint. Review-discovered child termination races are explicitly deferred to Item 77 rather than partially changing one client here. | - [x] Dedicated stacked `jj` revision `ltnsrmlp`; focused scope independently reviewed; work-item row marked `done`. |
| 41 | - [x] Temporary TypeScript and Rust characterization tests passed against Item 40, proving both client builders duplicated orphan-root, self-parent omission, nested-child, and PID-order policy before removal. | - [x] The recursive API/type/action and its client builders/tests are gone. The retained flat snapshot passes 10 TypeScript tests, 70 Rust units (including exact sidecar `ppid` lineage preservation), and both real Rust process E2Es; the 12-test actor contract regenerates a surface without `processTree`, all 15 actor package tests pass against the real wrapper, Core/actor typechecks and builds pass, and the 134-page website build succeeds. | - [x] Dedicated stacked `jj` revision `qmzytqsv`; two independent reseals found no remaining P0/P1/P2 issue; work-item row marked `done`. |
| 42 | - [x] Against Item 41 (`a9b4c012`), the no-write regression returns code 0 because the client attempts `mkdir '/tmp'` and is denied by `fs.create_dir`; the relative-project regression resolves `project` twice and searches `/project/project`; the browser wire regression records literal `project` instead of `/workspace/project`. | - [x] `pnpm --dir packages/typescript test` passes all 9 unit/integration cases, including denied `/tmp` bootstrap, omitted cwd, and single relative-cwd resolution; the native/browser cwd regressions in the dedicated revision pass with sidecar-owned Linux validation. | - [x] Dedicated stacked `jj` revision `suwmustu`; Item 80 removed the last native host-path compatibility dependency; work-item row marked `done`. |
| 43 | - [x] Before removal, temporary `process-options.public-api.ts` and Rust `public_process_options_accept_fields_that_never_reach_execute_before_item_43` compiled the ignored fields while the request assertion proved they had no wire effect. Source characterization found the exception: TypeScript raw-spawn `pty` did reach `ExecuteRequest`, but had no Rust counterpart; the existing PTY protocol suite established `openShell` as the cross-client terminal behavior to preserve. | - [x] `process-options.public-api.ts` now rejects every removed TypeScript field and type; `reduced_process_options_forward_only_implemented_fields`, Core schema/public-export tests, flat-spawn tests, serial real-sidecar Rust process E2Es, VM-limit tests, and protocol tests prove retained fields still forward. `allowed-node-builtins.test.ts` proves explicit `openShell` PTY serialization, and the enabled real C-WASM `pty-protocol.test.ts` passes in CI snapshot mode for raw/cooked input, resize, and EOF after its stale snapshot keys were repaired. | - [x] Dedicated stacked `jj` revision `orpyyprl`; independent review found no production-code blocker; work-item row marked `done`. |
| 44 | - [ ] `crates/agentos-sidecar/tests/acp_extension.rs` demonstrates unknown methods emitting a host callback/wait. | - [ ] Unknown methods return `-32601` promptly without a client callback. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 45 | - [ ] Protocol fixture inventory proves production JSON/legacy helpers are used only by compatibility tests. | - [ ] BARE roundtrip/generated protocol tests pass after all fixtures migrate and the helpers are deleted. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 46 | - [ ] Rust serialization tests demonstrate omission and explicit default-valued input producing the same wire payload. | - [ ] Rust/TypeScript fixtures distinguish omission, explicit empty, and explicit default where the protocol requires presence. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 47 | - [ ] `packages/core/tests/sidecar-client.test.ts` documents manufactured lifecycle IDs/maps used by the production lease path. | - [ ] Lease lifecycle tests pass against direct sidecar VM administration with only host lease/refcount state. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 48 | - [ ] `packages/core/tests/overlay-backend.test.ts` demonstrates omitted mode being selected before sidecar resolution. | - [ ] Omitted mode follows the sidecar-resolved value while explicit modes and caller-owned bridge state remain correct. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 49 | - [ ] Dependency/import audit proves the listed production dependencies and `long-timeout` declaration are unused. | - [ ] Core build, typecheck, package smoke test, and lockfile checks pass after removal. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 50 | - [ ] Typechecking `public-api-exports.test.ts` exposes the unsupported `defineSoftware(string)` call. | - [ ] Public API/typecheck tests accept only `{ packagePath }` and prove legacy exports are absent. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 51 | - [ ] `scripts/verify-thin-client-docs.mjs` detects stale package, architecture, permission, and command claims. | - [ ] The verifier plus website build pass against the corrected CLAUDE/docs sources. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 52 | - [ ] TypeScript routing tests demonstrate a legacy permission notification invokes a handler but cannot create an answerable typed reply route. | - [ ] Native adapter conformance covers supported methods and clients route only the typed protocol callback. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
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
| 63 | - [ ] Focused process-terminal and ACP tests demonstrate anonymous code-bearing errors without an exported type or source protocol detail. | - [ ] Both paths throw exported structured errors with exact code/message and their originating event/envelope. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 64 | - [ ] TypeScript/Rust cron tests demonstrate client message parsing/remapping and native/browser code divergence. | - [ ] Shared-sidecar conformance tests prove stable invalid/past codes and both clients pass the structured rejection through unchanged. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 65 | - [ ] Cleanup tests inject multiple structured errors and demonstrate that joined-message errors discard their identities and codes. | - [ ] Every affected cleanup path throws `AggregateError` retaining the original typed errors and contextual message. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 66 | - [ ] Shell package-selection tests demonstrate missing/unreadable refs being substituted or skipped without a sidecar request. | - [ ] Shell serialization forwards every selected package ref unchanged, performs no host package filesystem reads, and sidecar package error/projection tests pass. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 67 | - [ ] A TypeScript permission callback test throws synchronously from the first handler and observes the pending reply entry/timer remain plus the second handler being skipped. | - [ ] Handler-failure coverage proves immediate route cleanup, host-visible failure, no client-selected reply, and defined delivery behavior for remaining handlers. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 68 | - [ ] A callback transport test advances the authoritative sidecar timeout and demonstrates the client route surviving only because of the cleanup grace. | - [ ] Cancellation/expiry protocol tests prove the sidecar ends both wait and client route exactly once, with no ignored late result and no client policy timer. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 69 | - [ ] A shared-sidecar process test throws from one stdout/stderr listener and demonstrates sibling handlers/routes being failed or starved. | - [ ] Callback-isolation tests prove the failure is host-visible and later handlers plus unrelated processes continue receiving ordered events. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 70 | - [ ] A source/heap regression demonstrates the proxy retaining a full duplicate of the latest process snapshot although no production caller reads it. | - [ ] Proxy tests pass with no `processes` cache or legacy fallback and authoritative snapshot requests remain unchanged. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 71 | - [ ] Mixed direct-spawn/finite-exec/shell churn demonstrates native terminal-history eviction, while browser coverage proves snapshot absence alone cannot mean terminal expiry. | - [ ] Native/browser sidecar tests define the same explicit terminal lookup/expiry result and TS/Rust clients obey it without local snapshot inference. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 72 | - [ ] A Rust registry regression demonstrates terminal entries retaining broadcast senders and output callback task handles. | - [ ] Rust terminal entries retain only compact exit/failure correlation up to the sidecar-advertised count while late wait/subscription parity remains green. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 73 | - [ ] A public-factory browser E2E passes a real `.aospkg` whose executable adapter is present only in the projected VFS and demonstrates the default bridge spawning nothing (or the old fixture path substituting a prebuilt worker). | - [ ] The public factory asynchronously completes list/create/prompt through the standard production runtime Worker, executes the actual packed upstream ACP adapter entrypoint, exposes no internal pending response, and leaves zero routes/workers after close/dispose. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 74 | - [ ] A TypeScript transport regression fails the shared event pump, then starts a process (including the concurrent failure/start ordering) and demonstrates Execute can succeed with no remaining event consumer. | - [ ] Focused transport/process tests prove known pump failure rejects before Execute, the concurrent race terminates or cleans up the started process with the original typed failure, and no route/process is stranded. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 75 | - [x] Against Item 34 (`ac77fa88`), `session_surface_create_prompt_events_close` receives `ClientError::Kernel { code: "invalid_state", message: "unknown ACP session nope" }` instead of `ClientError::SessionNotFound`. | - [ ] Shared-core taxonomy/ownership tests, native/browser wrapper conformance, unchanged Rust lifecycle E2E, and a focused TypeScript unknown-session test all preserve `session_not_found` without client-side message parsing. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 76 | - [x] `cargo test -p agentos-client --test cron_e2e` failed in 2/3 default-parallel runs: one test runtime created the shared transport, then exited and aborted its transport tasks while the sibling VM stayed leased. | - [ ] A deterministic two-runtime shared-pool regression proves VM B can issue requests and fire a cron callback after creator runtime A exits; transport teardown and all existing shared-sidecar suites pass. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 77 | - [ ] Rust cancellation/watchdog-overlap and concurrent-create regressions demonstrate that a child handle can be lost or replaced before disposed publication; TypeScript timeout/kill-failure tests demonstrate disposal resolving without confirmed exit. | - [ ] Deterministic Rust and TypeScript lifecycle tests prove cancellation-safe retry, serialized create/dispose, identical typed termination failures, and no disposed state before the owned native child is reaped. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 78 | - [x] Against the rebuilt real `agentos-sidecar` on Item 42, `kernel-bootstrap-base.test.ts` passes bundled-base `/tmp` `01777` but fails because `/etc/agentos` is absent and a root with `disableDefaultBaseLayer` reports `/tmp` as `0755` instead of `01777`. | - [ ] Sidecar-native root tests and the real TypeScript VM gate prove `/tmp`, `/workspace`, and required Linux directories have one authoritative mode/existence contract with and without the bundled base, under restrictive guest permissions and with no client bootstrap. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 79 | - [x] On Item 42, a real VM with guest reads allowed but `write` and `create_dir` denied for `/` completes the stdin-backed TypeScript request, then `AgentOs.dispose()` fails with `failed to dispose sidecar VM; failed to dispose sidecar session`; removing `rm` from the denied operations does not change the failure. | - [ ] Native/browser lifecycle tests and a real TypeScript VM regression prove VM/session disposal succeeds under guest deny-all or write-deny policy, cleans every process/mount/runtime route, and does not weaken executor filesystem enforcement. | - [ ] Dedicated stacked `jj` revision; work-item row marked `done`. |
| 80 | - [x] Item 42's native compatibility regression creates a directory only beneath `vm.host_cwd`, sends that absolute host directory as execute `cwd`, and observes the sidecar manufacture `/host-nested` in the guest without any `host_dir` mount. | - [x] `service_execute_cwd_matches_linux_before_process_admission`, `security_hardening_suite`, `agentos_packages_launch_keeps_adapter_and_child_entrypoints_guest_native`, `posix_path_repro_suite`, and the focused Python/builtin/stdio suites prove guest-only paths, explicit mounts, Linux errno, child-process parity, and bounded package-link resolution. | - [x] Dedicated stacked `jj` revision `pzzlonpr`; workspace check, bridge tests/build, build-tools typecheck, formatting, and focused native suites pass; work-item row marked `done`. |
| 81 | - [x] The pre-deletion `acp_integration` and `acp_session` suites pass 23/23 and 33/33; the inventory maps all 43 unique legacy tests and identifies only permission aliases, initialize-version cleanup, and non-string config values as retained gaps. | - [x] The three new production-path assertions, 78 shared-core units, 8 shared conformance tests, 12 native extension units, 2 native extension integrations, and 15 native/browser wrapper cases pass; the service binary compiles/lists no legacy tests and native-sidecar check passes. | - [x] Dedicated stacked `jj` revision `sqnqyqws`; dead harness/codec search, formatting, diff check, and workspace gates pass; work-item row marked `done`. |
| 82 | - [x] Parent-behavior characterizations prove the exact missing-id response classifies as `Unknown`, leaves a resumable resume pending with no kill, and the deleted legacy harness instead emitted an inapplicable `-32600`. | - [x] `json_rpc_classifier_rejects_complete_invalid_envelopes`, `complete_response_without_id_fails_without_a_wire_reply`, `resumable_resume_missing_response_id_clears_state_and_aborts_agent`, and `acp_extension_suite` prove immediate `invalid_state`, no extra wire reply, no partial session, confirmed adapter exit, and successful owner teardown. | - [x] Dedicated stacked `jj` revision `vsqvzlkn`; 80 core units, 8 core conformance tests, 12 native units, 15 wrapper cases, workspace check, formatting, diff, and dead-`Unknown` search pass; work-item row marked `done`. |

### Item 34 convergence acceptance

Item 34 was not sealed until every row below was complete. A same-core
blocking/resumable comparison is useful reducer coverage, but it does not count
as native/browser production-wrapper conformance by itself.

| ID | Original issue | Before validation | After validation | Complete |
|---|---|---|---|---|
| 34.a | Production browser returned internal `AcpPendingResponse` values to ordinary SDK calls. | - [x] Against item 33 (`066f6b51`), the production browser request expected `AcpSessionCreatedResponse` but received `AcpPendingResponse`. | - [x] The 37-test browser runtime-driver suite plus the 5-test runtime-browser converged-executor suite pass create/prompt/resume through the production opaque-frame driver. | - [x] |
| 34.b | Any connection that guessed a sequential process handle could inject resumable adapter output. | - [x] Against item 33 (`066f6b51`), connection B delivered output for connection A's handle and advanced the pending request instead of receiving an ownership error. | - [x] `engine::tests::resumable_output_delivery_rejects_cross_owner_injection_before_mutation` passes. | - [x] |
| 34.c | Messages with both `id` and `method` were misclassified as notifications, while browser advertised host tools it could not execute. | - [x] Against item 33 (`066f6b51`), `adapter_request_with_id_and_method_gets_a_method_not_found_response` left stdin at three writes instead of four: `{id:"host-1",method:"host/read"}` leaked into notification handling and received no response. | - [x] One shared classifier now routes `id + method` as an inbound request. Native wrapper host-tool/permission fixtures retain their supported callbacks; the actual browser wrapper returns canonical `-32601`, emits no fake session event, and advertises no unroutable host tools. | - [x] |
| 34.d | Browser/core agent resolution read guest `agentos-package.json`, which real packed packages strip, and browser rejected `listAgents`. | - [x] Against item 33 (`066f6b51`), the packed-agent regression reached the core's stripped guest-manifest read and failed unknown-agent/`ENOENT`; the browser `AcpListAgentsRequest` path returned `invalid_state` because `list_agents` was explicitly unsupported. | - [x] `browser_initialize_vm_projects_real_packed_agent_then_lists_and_creates_it`, `browser_acp_host_delegates_projected_agent_resolution_and_listing`, and `projected_agent_catalog_errors_propagate_without_becoming_unknown_agent` pass with live vbare metadata and no guest manifest. | - [x] |
| 34.e | Native production retained its separate ACP request/session state machine; same-core tests did not prove adapter convergence. | - [x] Against item 33 (`066f6b51`), the wrapper-conformance probe found no production browser lifecycle path to compare and source comparison showed native still dispatching its independent create/resume/prompt/config state machine. | - [x] The complete 15-test `acp_wrapper_conformance` binary passes; its one native-thread scenario exercises real native and browser dispatchers across list/create/resume/prompt/config/state/cancel/close, custom package roots, and exact-owner behavior while `acp_conformance` passes 8 shared transition cases. | - [x] |
| 34.f | Resumable notification buffers were unbounded and could partially mutate state when notification overflow was discovered too late. | - [x] Against item 33 (`066f6b51`), create and prompt accepted 4,097 notifications and returned pending instead of enforcing a limit or cleaning up; item 33 had no resumable `begin_resume_session` API, so the resume regression failed to compile with `E0599`. | - [x] `resumable_{create,resume,prompt}_event_overflow_*` return `limit_exceeded`, commit no partial event/session state, and clean up. | - [x] |
| 34.g | Malformed output, exit, or timeout could strand pending core state, browser execution mappings, or consumed prompt context. | - [x] Against item 33 (`066f6b51`), malformed create output retained `pending_create_count == 1`; malformed prompt retained `pending_prompt_count == 1`, performed no kill, and consumed its durable preamble. The parent had no typed abort request (`E0432` compile probe), so exit/timeout cleanup was unavailable. | - [x] `browser_wrapper_initial_pending_failures_clear_every_resource_route`, `browser_wrapper_restart_failures_are_terminal_and_resource_clean`, `resumable_resume_terminal_parse_error_clears_state_and_kills_agent`, and `malformed_prompt_output_restores_consumed_preamble_and_aborts_agent` pass with zero pending/session/execution routes. | - [x] |
| 34.h | A second prompt overwrote an in-flight prompt after writing its request. | - [x] Against item 33 (`066f6b51`), a second prompt returned `Ok("proc-s1")`, proving it wrote and replaced the in-flight request. | - [x] `resumable_session_request_rejects_a_second_in_flight_request_before_writing` returns `conflict` with one write and one pending request. | - [x] |
| 34.i | Browser wire-session disposal left ACP sessions, pending interactions, and execution handles alive. | - [x] Against item 33 (`066f6b51`), three prompt/start plus wire-session-disposal cycles retained three core sessions, three pending prompts, and three browser execution routes because the extension inherited no-op disposal. | - [x] `browser_wrapper_vm_disposal_cleans_only_that_owners_pending_interaction`, `dispose_owner_removes_only_that_owners_pending_and_live_state`, and the 16-cycle `browser_wrapper_repeated_create_close_churn_returns_to_zero_resources` pass with exact sibling isolation and zero resources after dispose. | - [x] |
| 34.j | `close_session` removed authoritative state before fallible cleanup, so retry manufactured success. | - [x] Against item 33 (`066f6b51`), an injected first wait failure left `session_count == 0`, proving authoritative state had already been discarded before cleanup succeeded. | - [x] `close_session_retains_authoritative_state_until_cleanup_can_be_retried` passes. | - [x] |
| 34.k | Missing browser connection ownership collapsed to the shared empty-string owner. | - [x] Against item 33 (`066f6b51`), a valid request without connection ownership was accepted and returned an encoded ACP `invalid_state` response rather than being rejected at dispatch. | - [x] `browser_acp_extension_requires_connection_ownership` rejects the request before dispatch. | - [x] |
| 34.l | Cleanup failures were swallowed and event/package-limit failures used generic `invalid_state`. | - [x] Against item 33 (`066f6b51`), injected abort cleanup used `let _ =` and returned the original response, while event overflow was unbounded/generic and browser package entry count had no configured typed limit. | - [x] `browser_acp_host_retains_route_until_abort_cleanup_can_be_retried`, `cancel_write_failure_kills_route_and_returns_typed_error`, all three resumable overflow tests, `browser_sidecar_errors_keep_their_acp_semantic_class`, and `package_index_entry_overflow_is_a_typed_limit_without_materializing_entries` pass; the changed cleanup paths contain no ignored fallible result. | - [x] |
| 34.m | Browser fixtures fabricated a stripped guest manifest because production ConfigureVm had no trusted `.aospkg` byte projection path. | - [x] Against item 33 (`066f6b51`), ConfigureVm rejected the packed-package descriptor as unsupported and the fallback fixture materialized `agentos-package.json`; production could neither list nor create the packed agent. | - [x] `browser_initialize_vm_projects_real_packed_agent_then_lists_and_creates_it` passes a real inline `.aospkg`, proves the guest manifest is `ENOENT`, projects the vbare index read-only, and successfully lists/creates the agent. Actual execution of the packed entrypoint through the public browser factory remains separately tracked as item 73. | - [x] |
| 34.n | Native interrupt synthesized a cancelled response but did not deliver `session/cancel` to the live adapter; an in-flight permission callback could therefore keep cancellation blocked until its decision timeout. A first fix released the continuation before the old RPC response, allowing delayed old notifications to contaminate the next prompt. | - [x] Against item 33 (`066f6b51`), `extension_cancel_interrupt_gets_synthetic_response` returned locally while adapter stdin remained unchanged; independent reseal then showed immediate continuation removal had no old-response boundary. | - [x] Native now cancels the exact permission wait, writes an idless adapter notification, keeps the shared-core prompt busy while discarding old notifications/text through the matching RPC response, and restarts or evicts the adapter if that bounded drain cannot complete. `interrupted_resumable_prompt_drains_old_boundary_before_accepting_next_prompt` proves wrong-owner isolation, busy-until-boundary behavior, stale-text/event discard, durable preamble restoration, and a clean next prompt; callback race/failure tests remain green. | - [x] |
| 34.p | Shared ACP sessions were keyed only by the adapter-returned session id, so independent owners whose fresh adapter processes both returned a local id such as `session-1` collided. | - [x] Against item 33 (`066f6b51`), the two-owner regression completed owner A's `echo-session-1` create and owner B's identical create returned `session id collision: echo-session-1`. | - [x] `identical_adapter_session_ids_are_independent_per_owner` and the native/browser `native_and_browser_wrappers_scope_identical_adapter_session_ids_by_exact_owner` scenario pass create/prompt/list, close owner A, then state/prompt owner B without cross-owner mutation. | - [x] |
| 34.q | Normal browser session close released the underlying execution but retained the core-process-to-browser-execution route because only abort cleanup removed that host map entry. | - [x] Against item 33 (`066f6b51`), repeated successful create/close left one additional `BrowserAcpHost` process route per cycle because close never removed the host map entry. | - [x] `orderly_close_churn_releases_every_browser_route_without_double_abort` and `browser_wrapper_repeated_create_close_churn_returns_to_zero_resources` pass 32 host cycles plus 16 production-wrapper cycles with zero core/route state and no double abort. | - [x] |
| 34.r | The browser ACP host ignored `SpawnAgentRequest.runtime` and always created a JavaScript context, unlike native runtime dispatch. | - [x] Against item 33 (`066f6b51`), the host-runtime regression sent Python and WebAssembly requests and recorded both in `create_javascript_context`, with zero WASM-context calls. | - [x] `browser_acp_host_uses_the_ordinary_browser_context_for_each_runtime` passes: JavaScript/Python use the ordinary JS context, WebAssembly uses the WASM context, and bridge failures retain their typed ACP semantic class. | - [x] |
| 34.s | Native wrapper event encoding/live-sink delivery happened after shared-core state and event removal, so a sink failure could reject a request after the authoritative mutation was already hidden. | - [x] Against item 33 (`066f6b51`), the one-shot sink regression rejected prompt/config after the session/config mutation had committed and `take_events` had consumed the only event, so retry could not observe it. | - [x] `event_delivery_snapshot_retains_unacknowledged_suffix_in_order` and production `native_wrapper_retries_committed_event_after_one_shot_sink_failure` pass: the response succeeds, the committed event remains queued, the next request retries it once, and acknowledgement prevents duplication. | - [x] |
| 34.t | Native held one global core mutex across blocking host exchanges, so an adapter callback for owner A could stop unrelated owner B requests. | - [x] Review of the first Item 34 draft showed `run_core_transition` locking the global `AcpCore` before `NativeCoreHost::exchange`, including the 120-second permission callback path. | - [x] Native uses one instance of the shared behavior core per exact VM owner, and `(ownerId, processId)` route keys preserve isolation when owner-local counters repeat. `stalled_native_owner_does_not_lock_unrelated_owner_core` and the exact-owner wrapper lifecycle pass. | - [x] |
| 34.u | Native pending interactions zero-time-polled or consumed unrelated process/lifecycle events while waiting for one adapter. | - [x] Review found the draft repeatedly polling the ownership-wide event queue, storing unrelated frames inside one request future, and losing those frames when interruption dropped that future. | - [x] Native ACP installs one sidecar-owned exact-process output buffer immediately after `Execute`, while dispatch still has exclusive sidecar access, and then drains only that process. `exact_extension_output_buffer_preserves_sibling_events_and_cleans_captured_exit` proves sibling output/exit remain ordinary events and a captured exit still runs authoritative cleanup before VM disposal; `silent_buffered_exit_completes_handoff_without_binding_or_leaking_the_buffer` covers a terminal process with no stdout/stderr. | - [x] |
| 34.v | The browser's synchronous pending driver had no cancellation seam, so a worker blocked in reactor polling could not clean up until output or timeout. | - [x] Review found no cancellation probe or protocol reason between `createAcpPendingResponseDriver` and `KernelReactor::poll`. | - [x] `forwards host cancellation to sidecar-owned atomic cleanup`, `resumable_caller_cancellation_is_sidecar_typed_and_atomic`, and `returns immediately when the host cancellation flag is set` pass. The client reads host-owned shared state and forwards only `CALLER_CANCELLED`; the sidecar owns cleanup and the typed result. | - [x] |
| 34.w | Native and browser close paths blocked or used fake millisecond polling, and wrapper resources could be removed in a different order than core state. | - [x] Review found blocking teardown waits, browser poll-clock loops, and native route cleanup after core removal. | - [x] `resumable_close_releases_core_between_bounded_signal_phases`, `close_session_retains_authoritative_state_until_cleanup_can_be_retried`, browser retry cleanup, orderly churn, and full wrapper conformance pass with sidecar-owned resumable close phases and cleanup-before-state-removal. | - [x] |
| 34.x | Native disposal inferred owners only from live process routes, so an exited adapter could leave owner-scoped core state undiscoverable. | - [x] Review found no authoritative owner registry independent of `core_processes`. | - [x] Every native dispatch registers exact connection/wire-session ownership, disposal uses that registry even with no process route, and `session_disposal_uses_authoritative_owner_registry_without_process_routes` passes. | - [x] |
| 34.y | Browser execution contexts and diagnostic failures could leak or orphan otherwise committed execution state. | - [x] Review found context IDs discarded after start, no release on several terminal paths, and fallible diagnostic emission mixed into lifecycle commits. | - [x] Start failure, abort, normal release, and VM disposal release contexts; diagnostics are logged after state transitions. `browser_wrapper_releases_context_when_adapter_start_fails`, zero-resource churn, and `browser_sidecar_diagnostic_failures_do_not_orphan_execution_or_context` pass. | - [x] |
| 34.z | Native emitted adapter stderr as a public ACP event while browser invoked a local TypeScript callback, creating divergent ownership, bounds, and retry behavior. | - [x] Review found the two wrappers constructing/delivering stderr through different paths. | - [x] Both wrappers submit `AcpDeliverAgentStderrRequest` to the shared core; `agent_stderr_is_owner_scoped_bounded_and_retryable`, the Rust frame-helper regression, and `forwards adapter stderr to the sidecar-owned event path` pass. | - [x] |
| 34.aa | Browser create accepted malformed JSON shapes late, and native command projection still assumed `/opt/agentos` after a custom package root was configured. | - [x] Review found spawn could occur before malformed `clientCapabilities`/`mcpServers` failed and native command construction reintroduced the default root. | - [x] `browser_wrapper_rejects_malformed_create_json_before_spawning` passes with zero resources, and the full native wrapper lifecycle lists/creates/closes its adapter from `/srv/agentos`. | - [x] |
| 34.ab | Dropping an interrupted native prompt future left its shared-core pending continuation live, so that session remained permanently busy. | - [x] Independent review traced stdio interruption through `drop(dispatch)` without any removal from `AcpCore::pending_prompts`. | - [x] Explicit cancel now retains only a bounded draining continuation until the old response boundary; close/kill abandon it before their terminal operation. The shared-core drain regression proves the session remains open and accepts the next prompt only after stale output is isolated. | - [x] |
| 34.ac | Native extension dispatch and the stdio blocking classifier could invoke extension code before proving that the request named a live VM owned by the connection/session. | - [x] Independent review showed forged VM ownership could invoke an extension or its stateful `is_blocking_request` classifier and retain owner-scoped state. | - [x] `extension_dispatch_rejects_unknown_vm_before_invoking_or_retaining_extension_state` churns 100 forged VM owners with zero handler invocations, and `blocking_classifier_is_not_invoked_before_live_vm_validation` observes zero classifier invocations. | - [x] |
| 34.ad | Browser ACP smoke fixtures sent forged VM ownership directly, so they did not prove the production wrapper obeyed the ordinary authenticated lifecycle. | - [x] Enabling live-ownership validation made the old fixtures fail before ACP dispatch because they had never authenticated, opened a wire session, or initialized their VM. | - [x] The ACP codec, kernel-worker, and demo smoke tests now authenticate, open a session, initialize with omitted defaults, and then dispatch ACP. The complete Chromium suite passes 16 tests with 6 explicit capability/model skips. | - [x] |

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
closed routes produce no client-authored reply. The native ACP sidecar owns the
120-second decision deadline and converts its typed callback timeout to the
standard reject outcome. Clients receive only a later 125-second cleanup
deadline for bounded host bookkeeping; it cannot choose the permission result,
and a reply after route cleanup is rejected instead of becoming a legacy ACP
request. This keeps host-only callback state in the host while putting the
permission default in one adapter-owned place.

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
- **18.36 — done / high confidence:** Removed the duplicated public 120-second
  ACP permission constant from both clients. The native ACP adapter owns that
  decision deadline and maps a typed callback timeout to its default reject
  outcome. Its callback carries only `cleanupAfterMs: 125000`, a strictly later
  bound for TypeScript/Rust host-route bookkeeping. Client cleanup returns no
  policy answer, and late replies cannot fall through to a legacy request.
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
