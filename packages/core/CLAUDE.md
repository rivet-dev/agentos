# agentOS Core Package

`@rivet-dev/agentos-core` -- contains VM ops, ACP client, session management.

**⚠️ CRITICAL INVARIANT: ALL guest code MUST execute inside the kernel with ZERO host escapes.** The VM is a fully virtualized OS — every file read, network connection, and process spawn goes through the kernel. Guest code must never touch real host APIs. The Node.js execution engine is currently broken (spawns real host `node` processes instead of V8 isolates). See `crates/execution/CLAUDE.md`.

## AgentOs Class

- Wraps the kernel and proxies its API directly.
- Keep the client as small and simple as possible: validate/serialize explicit
  input, forward it unchanged, route host callbacks/events, and retain only
  state the sidecar cannot own. Preserve omission instead of supplying VM,
  environment, permission, bootstrap, session, prompt, or execution defaults.
  The only client-owned default is the TypeScript package manager's default
  package list, which is forwarded like caller-supplied packages.
- **All public methods must accept and return JSON-serializable data.** No object references (Session, ManagedProcess, ShellHandle) in the public API. Reference resources by ID (session ID, PID, shell ID).
- Filesystem methods mirror the kernel API 1:1 (readFile, writeFile, mkdir, readdir, stat, exists, move, delete).
- Command execution mirrors the kernel API (exec, spawn).
- `fetch(port, request)` reaches services running inside the VM using the kernel network adapter pattern (`proc.network.fetch`).
- Cron grammar, defaults, job/run state, overlap policy, missed-fire coalescing,
  alarm generations, and lifecycle events are sidecar-owned. The TypeScript
  cron adapter may only arm the one absolute host alarm returned by the sidecar,
  route host callback correlations, and report completion. Do not add a client
  scheduler, parser, default overlap/id, or injectable schedule driver.
- Native sidecar execution requests should stay unresolved on the TypeScript side. Forward `command`, `args`, `cwd`, and VM config through the wire payload, and let Rust own command lookup, guest-path to host-path mapping, shadow materialization, and `AGENT_OS_*` runtime env assembly.
- Do not add client-side command classification, simple-command parsing, shell wrapping, or synthetic terminal behavior. Send the caller's raw command, argv, and explicit options to the sidecar; shell grammar, default cwd, runtime selection, and exit semantics belong there. `openShell` over the process/PTY protocol is the only SDK terminal interface.
- The sidecar owns mount routing, merged directory views, read-only enforcement, and cross-device behavior. A client may retain only the exact callback/object handle required by a caller-owned host-backed mount; that state must never bootstrap or directly serve the guest filesystem.
- Host-tool client responsibilities are limited to TypeScript Zod-to-JSON-Schema conversion, callback registration and execution, and result serialization. CLI parsing, permission enforcement, prompt documentation, timeout policy, and command dispatch belong in the sidecar.
- Host-tool `inputSchema` conversion in `src/host-tools-zod.ts` is intentionally fail-closed. Support only the Zod subset that round-trips cleanly into the sidecar-facing JSON Schema contract; if a schema would degrade semantics or emit `$ref`/`$defs` (`discriminatedUnion`, `intersection`, `tuple`, `record`, `date`, `bigint`, custom refinements, metadata `id`, etc.), throw `HostToolSchemaConversionError` with the offending field path instead of coercing it to `{ type: "string" }`.
- Host-tool name, description, example, count, and timeout limits are
  sidecar-owned. TypeScript retains only Zod schema conversion and callback
  validation; do not copy sidecar registration policy into the client. Toolkit
  CLI names (`agentos`, `agentos-<toolkit>`) are derived in the sidecar and are
  not client or wire inputs.
- `src/sidecar/rpc-client.ts` is the consolidated home for framed sidecar I/O and sidecar descriptor serializers. Keep shared/explicit sidecar pool and VM lease bookkeeping in `src/agent-os.ts`; do not add runtime emulation or policy to either layer.
- The native sidecar framed stdio path now defaults to the BARE payload codec. Keep any JSON payload support behind explicit migration-only opts such as `payloadCodec: "json"`, and remember that BARE structs need every positional field serialized explicitly across the Rust/TypeScript boundary rather than relying on JSON-style `skip_serializing_if` omissions.
- In `src/sidecar/native-process-client.ts`, treat `child.on("exit")` and `child.on("error")` as the authoritative terminal-disconnect path for framed stdio clients. `stdout` can close before Node fills in `exitCode`/`signalCode`, so reject in-flight RPCs with a typed disconnect immediately and upgrade the stored terminal error once the concrete exit metadata arrives.
- In the native-sidecar event path, long-lived background loops should call `waitForEvent()` in abortable no-timeout mode instead of parking a multi-hour timeout sentinel. The abort signal is the cancellation mechanism; the timeout itself becomes the regression surface on idle VMs.
- For native-sidecar BARE ACP session bootstrap payloads, keep `SessionCreatedResponse` aligned with `crates/sidecar/protocol/agentos_native_sidecar_v1.bare`: `sessionId` is the first positional field on the wire, before optional `pid`, `modes`, `configOptions`, `agentCapabilities`, and `agentInfo`. If the TypeScript decoder reads `session_id` last, every `createSession()` response desynchronizes.
- Public SDK type exports funnel through `src/types.ts`. Do not add a compatibility
  runtime, alternate kernel facade, or client-side runtime implementation.
- When adding a new public SDK option/result/helper type under `src/agent-os.ts`, `src/json-rpc.ts`, `src/host-dir-mount.ts`, or other root-facing modules, mirror it through `src/types.ts` and keep `tests/public-api-exports.test.ts` aligned so the package entrypoint stays truthful.

## Agent Sessions (ACP)

- Uses the **Agent Communication Protocol** (ACP) -- JSON-RPC 2.0 over stdio (newline-delimited)
- No HTTP adapter layer; communicate directly with agent ACP adapters over stdin/stdout
- Reference `~/sandbox-agent` for ACP integration patterns. Do not copy code from it.
- ACP docs: https://agentclientprotocol.com/get-started/introduction
- Session design is **agent-agnostic**: each agent type has a config specifying its ACP adapter package and main agent package name
- Currently configured agents: PI (`@agentos-software/pi`), PI CLI (`@agentos-software/pi-cli`), OpenCode (`@agentos-software/opencode`), Claude (`@agentos-software/claude-code`), and Codex (`@agentos-software/codex` + `@agentos-software/codex-cli`).
- **No host agent exceptions.** Host-native wrappers and host binary launch paths are not allowed. OpenCode support must use the real upstream OpenCode implementation rebuilt into the VM adapter package and executed inside the VM.
- `createSession("pi")` spawns the ACP adapter inside the VM, which calls the Pi SDK directly
- Agents are resolved by the sidecar, not the client. There is no `AGENT_CONFIGS` table; `AgentType` is just a package name. `createSession(name)` and `resumeSession(name)` send only explicit caller fields, and `listAgents()` is a sidecar RPC. The sidecar reads the packed vbare manifest live from `/opt/agentos`; clients must not parse toolchain `agentos-package.json` files or cache agent metadata.
- In `createSession()`, forward instruction fields without combining, trimming, or generating tool-reference text. The sidecar owns base-prompt assembly, `skipOsInstructions` semantics, and agent-specific instruction preparation.
- ACP agents that issue live `session/request_permission` calls during `session/prompt` cannot rely on queued session events alone. Route those permission round-trips through the sidecar callback channel (`SidecarRequestPayload`) so the host can answer them before the prompt request completes.
- Native-sidecar inbound ACP host methods are adapter-owned. VM filesystem and terminal methods execute directly in the native ACP extension; unknown methods return JSON-RPC `-32601`. Do not recreate a generic client ACP request dispatcher.
- Permission callbacks route through the client only because the host handler lives there. The client forwards an explicit host reply or no reply; the ACP sidecar owns the missing-handler/timeout/failure default.
- Session cancellation is a sidecar state transition. Local prompt resolution and synthetic cancellation responses are migration debt; clients should forward cancellation and relay the authoritative result.
- Native-sidecar ACP request timeouts should surface as JSON-RPC errors with `error.data.kind === "acp_timeout"` rather than string-only transport errors. Use `isAcpTimeoutErrorData()` from `src/json-rpc.ts` instead of parsing timeout messages.

### Agent Adapter Approaches

Each agent type can have two adapter approaches:
- **SDK adapter** (default) -- Embeds the agent SDK directly via library import (`createAgentSession()`). Lower memory footprint (~100MB less for Pi). Binary: `pi-sdk-acp`. Package: `@agentos-software/pi`. Agent ID: `pi`.
- **CLI adapter** -- Spawns the full agent CLI as a headless subprocess via its ACP adapter (`pi-acp` spawns `pi --mode rpc`). Higher memory overhead but provides full CLI feature set. Binary: `pi-acp`. Package: `@agentos-software/pi-cli`. Agent ID: `pi-cli`.

### Agent Configs

An agent's launch config lives in the packed vbare manifest projected under `/opt/agentos`. The sidecar resolves its entrypoint, manifest environment, and launch arguments live; the client sends the agent name and explicit overrides without holding a second config surface.

## Testing

- **Framework**: vitest
- **Prefer scoped tests while iterating.**
  - `pnpm --dir packages/core exec vitest run tests/path/to/file.test.ts` or `pnpm --dir packages/core exec vitest run -t "test name pattern"`
  - Repo-root `pnpm test` is the RC sweep and exits cleanly; it is still too broad for normal iteration.
  - `pnpm --dir packages/core test` intentionally uses Vitest's `verbose` reporter because `tests/wasm-commands.test.ts` and similar long-running VM suites otherwise sit silent for minutes and get misread as hangs during `US-088` sweeps.
  - Use low timeouts for test commands (60000ms max).
- The vitest setup file at `tests/helpers/default-vm-permissions.ts` disposes every cached shared sidecar via `__disposeAllSharedSidecarsForTesting()` in `afterAll`. It must not alter VM options or inject permissions/defaults. Workers can hang on exit if the shared sidecar's piped stdio handles stay open, so any new test entrypoints that bypass this setup file must dispose their sidecars themselves.
- `NativeSidecarProcessClient.dispose()` enforces a graceful exit window then `SIGKILL`s the child if it ignores stdin EOF; `tests/native-sidecar-process.test.ts` covers the regression so future changes cannot reintroduce an unbounded teardown wait.
- In `packages/core` tests that capture `spawn()` stdout/stderr via callbacks and then call `waitProcess(pid)`, drain one macrotask (`await new Promise((resolve) => setTimeout(resolve, 0))`) before asserting on the buffered strings. Native-sidecar `process_output` events can arrive one turn after the exit notification, and tiny outputs like `curl -s` bodies are the first thing to get lost if you snapshot immediately.
- `NativeSidecarProcessClient.waitForEvent(...)` supports indexed `SidecarEventSelector` objects; prefer selectors over ad hoc lambdas on shared sidecar clients so buffered events stay O(1) to retrieve and `ownership` can pin a wait to one VM/session.
- The native sidecar client's unmatched event buffer is intentionally bounded and fail-closed. If a test or runtime path can leave `runEventPump` idle while output events stream, expect `SidecarEventBufferOverflow` rather than unbounded buffering, and set a larger `eventBufferCapacity` explicitly only for cases that truly need it.
- Runtime client code must never probe for or invoke Cargo. Repository tests that build the sidecar may invoke `process.env.CARGO ?? "cargo"` explicitly as test setup.
- For `tests/wasm-commands.test.ts`, broad `-t "grep"` or `-t "sed"` filters can pull in unrelated `rg`, `gzip`, or cross-package pipeline coverage via substring matches. When a story only gates the `grep`/`sed` blocks, use the explicit case names or a narrower `--testNamePattern` that only matches those block entries.
- For `tests/wasm-commands.test.ts` and similar long-running VM truth suites, prefer one shared VM per `describe(...)` block over one VM per individual test unless the case truly needs pristine bootstrap state. Per-test VM boots push the file into multi-minute runtimes and make the RC sweep look hung even when it is still progressing.
- The `examples/quickstart` package also resolves `@rivet-dev/agentos-core` from `packages/core/dist`; after TypeScript changes in `packages/core/src`, rebuild `packages/core` before rerunning quickstart acceptance commands.
- `spawn()` and `openShell()` are asynchronous because the client must return the
  authoritative kernel PID from the sidecar. Do not restore synthetic PID
  allocation, pre-start operation queues, or a client terminal fallback.
- **Always verify related tests pass before considering work done.**
- **All tests run inside the VM** -- network servers, file I/O, agent processes.
- For `vm.exec()` cwd/path tests, prefer setting up files from inside the guest shell when the assertion is about command resolution or relative paths. VM filesystem API writes becoming visible to host-backed runtimes is a separate shadow-sync surface and should be tested independently.
- For active agent-session/bash-tool filesystem regressions, cover the host read path in `tests/filesystem.test.ts` with a Claude llmock prompt. Long-lived session processes keep writing into the sidecar shadow root after a tool call returns, so `vm.readFile()`/`vm.stat()` need shadow reconciliation before the session itself exits.
- Session tests that need launch argv or OS-instruction assertions should inspect `getSessionAgentInfo(sessionId)` from sidecar state instead of spying on `kernel.spawn`; `createSession()` now launches through sidecar RPCs.
- `listSessions()` and `closeSession()` are awaited protocol operations. The sidecar owns the live-session list and idempotent close semantics; do not add client session tombstones, close-promise registries, or detached close tasks.
- Pi CLI session state currently reports the shared V8 host PID when multiple ACP sessions share one JavaScript runtime child. In cleanup tests, treat only host PIDs that are unique to a session as dedicated session roots; a shared PID is runtime-wide context, not three distinct leaked processes.
- For projected npm CLIs in package tests, prefer `node /root/node_modules/<pkg>/dist/<entry>.js` over `/root/node_modules/.bin/*`. pnpm's generated `.bin` wrappers embed host filesystem paths, which are not stable or guest-visible inside the VM.
- Browserbase VM tests should read credentials from host env as `BROWSER_BASE_API_KEY` / `BROWSER_BASE_PROJECT_ID`, alias them to `BROWSERBASE_API_KEY` / `BROWSERBASE_PROJECT_ID` in the guest env, and keep VM `network` permissions narrowed to `dns://*.browserbase.com` plus `tcp://*.browserbase.com:*` so remote Browserbase sessions work while direct guest egress stays denied.
- For Browserbase e2e flows inside the VM, prefer a small guest `fetch()` helper that creates/releases the Browserbase session plus `node /root/node_modules/@browserbasehq/browse-cli/dist/index.js --ws <connectUrl> ...` over the browse daemon session socket path. The direct `--ws` mode avoids a guest-local Unix-socket control hop and keeps the test focused on Browserbase API plus CDP connectivity.
- For `tests/wasm-commands.test.ts` curl coverage, prefer a guest `net.createServer()` HTTP fixture over guest `http.createServer()` when the story is about the curl/WASM client path. The HTTP-server transport wrapper is a separate compatibility surface and can hide or conflate curl regressions.
- Layer lifecycle regressions should be covered in both `tests/layers.test.ts` for in-memory snapshot reuse/composition semantics and `crates/sidecar/tests/layer_management.rs` for VM-scoped layer RPC isolation; the package-level suite alone does not prove per-VM ownership boundaries.
- For guest-JavaScript startup diagnostics, isolate each suspect import or constructor in its own fresh VM. Once a V8-side probe wedges or times out, later `node` spawns in the same VM can degrade into generic broken-pipe noise instead of the original failure.
- Agent tests must be run sequentially in layers:
  1. PI headless mode (spawn pi directly, verify output)
  2. pi-acp manual spawn (JSON-RPC over stdio)
  3. Full `createSession()` API
- **API tokens**: All tests use `@copilotkit/llmock` with `ANTHROPIC_API_KEY='mock-key'`. No real API tokens needed. Do not load tokens from `~/misc/env.txt` or any external file.
- **Mock LLM testing**: Use `@copilotkit/llmock` to run a mock LLM server on the HOST (not inside the VM). Use `loopbackExemptPorts` in `AgentOs.create()` to exempt the mock port from SSRF checks. The kernel needs `permissions: allowAll` for network access.
- Declarative sidecar permission rules must use explicit `["*"]` wildcards for rule `operations` and `paths`/`patterns`; empty arrays are rejected by the native sidecar instead of being treated as implicit wildcards.
- **Pi SDK llmock setup**: Pi reads Anthropic endpoints from `~/.pi/agent/models.json`, not `ANTHROPIC_BASE_URL`. For `createSession("pi")` tests, write a provider override such as `{ "providers": { "anthropic": { "baseUrl": "<llmock-url>", "apiKey": "mock-key" } } }` inside the VM before creating the session.
- Pi headless llmock tests should still pass `ANTHROPIC_BASE_URL` through the session env even with the `~/.pi/agent/models.json` override, because some Pi SDK request paths still consult the env-configured base URL during ACP-driven tool turns.
- `packages/core` agent-session tests execute the local registry agent workspaces
  through their built artifacts. After changing an adapter under
  `registry/agent/*/src`, rebuild that workspace before trusting core Vitest.
- Keep Claude's default `CLAUDE_CODE_NODE_SHELL_WRAPPER` enabled (`"1"`) in
  `registry/agent/claude`. Forcing it to `"0"` breaks real Bash-tool execution
  under llmock-backed sessions.
- **Module access**: Pass `mounts: [nodeModulesMount("<host>/node_modules")]` to `AgentOs.create()` to expose a host `node_modules` tree at `/root/node_modules`. The VM module resolver reads the mounted tree through the kernel VFS (no host-direct reads, no `moduleAccessCwd`). pnpm puts devDeps in `packages/core/node_modules/`, so tests use `nodeModulesMount(join(resolve(import.meta.dirname, ".."), "node_modules"))`. Software-package agents (`software: [pi]`) mount their own `/root/node_modules/<pkg>` roots and do not need this mount.
- Quickstarts and integration tests that run full-tier registry commands (for example `@agentos-software/git`) should set an explicit `/root/node_modules` mount via `nodeModulesMount(...)` when the package needs host Node modules. Omitted AgentOS permissions default to allow-all in the sidecar; permission tests must pass an explicit policy.
- S3-backed core tests can use `tests/helpers/mock-s3.ts` as the explicit local harness instead of Docker/MinIO; when the endpoint resolves to `127.0.0.1` or `localhost`, set `AGENT_OS_ALLOW_LOCAL_S3_ENDPOINTS=1` before creating the VM so the sidecar accepts the local test endpoint.
- Sandbox toolkit quickstarts/tests that depend on external Docker should use an explicit `SKIP_DOCKER=1` gate instead of `skipIf`, and the truthful host-tool path is to read `AGENTOS_TOOLS_PORT` inside the VM and `POST` `{ toolkit, tool, input }` to `http://127.0.0.1:$AGENTOS_TOOLS_PORT/call` from a guest Node script.
- Shared Vitest helpers under `src/test/` should register optional capability coverage conditionally in code instead of with `describe.skipIf` / `test.skipIf`; `US-088` treats those markers as product-debt skips even when they only guard backend capability differences.
- Agent E2E fixtures must pass the tested agent package explicitly; never rely
  on `createSession(name)` to discover an npm dependency. Registry command
  fixtures use `tests/helpers/registry-commands.ts` `requireBuilt` and fail with
  build instructions when their `.aospkg` artifacts are missing.
- Registry package tests for C-built commands such as `duckdb` and `http_get`
  go through `tests/helpers/registry-commands.ts` and the local `registry/`
  build. Build `registry/native/c`'s sysroot first, then run a second `make` for
  the concrete `build/...` targets so `SYSROOT` uses the patched tree.
- `tests/claude-session.test.ts` is the Claude SDK truth suite. It runs the real `@anthropic-ai/claude-agent-sdk` session path through llmock and covers PATH-backed `xu`, text-only replies, nested `node` `execSync` and `spawn`, metadata, lifecycle, and mode updates. Run it with `pnpm --dir packages/core exec vitest run tests/claude-session.test.ts --reporter=verbose` when verifying Claude regressions.
- **Kernel permissions are declarative pass-through config.** `AgentOsOptions.permissions` should stay JSON-serializable and be forwarded to the native sidecar without host-side probing or callback evaluation; Rust owns glob matching and policy decisions.
- ACP session events are live-only over `onSessionEvent()`. Do not reintroduce sequence numbers, local replay buffers, or event cursor recovery.
- Forward caller-supplied ACP `protocolVersion` and `clientCapabilities` unchanged and preserve omission; the sidecar owns their defaults and initialize behavior.
- **Sidecar permission path patterns preserve `*` vs `**`.** Use single-segment globs such as `/workspace/*` only for direct children; use `/workspace/**` when the VM should reach nested paths through the native sidecar permission policy.
- **Native-sidecar socket/process inspection is explicit now.** If a `Kernel` or `NativeSidecarProcessClient` caller needs `findListener()`, `findBoundUdp()`, or `getProcessSnapshot()`, grant `network.inspect` and/or `process.inspect` in the forwarded permissions; broad `network.listen` or `childProcess` access is not enough on its own.
- **Spawned-process presentation and control are sidecar-authoritative.** `listProcesses()`, `getProcess()`, `stopProcess()`, and `killProcess()` are awaited protocol operations. The client process map may retain PID-to-callback/process-ID routes, but must not cache command, argv, timestamps, running state, or signal success.
- Process/shell stdin, EOF, resize, signal, close, and wait operations are awaited
  protocol operations. Execution timeouts are optional execute-request data and
  are enforced by the sidecar; never add a client timer, detached control task,
  or fabricated exit status.
- Production clients omit `ExecuteRequest.processId`; the sidecar allocates and
  returns the event-correlation ID with the real kernel PID. Retain the returned
  ID only for host callback/event routing.
- Public shell IDs are the returned sidecar process IDs. Clients may retain live
  host output routes and in-flight exit promises, but late wait/close state comes
  from the sidecar process snapshot; do not add shell ID allocators or closed-ID
  tombstones.
- Do not add synchronous caches for sidecar socket, signal, process, timer, or
  resource state. Diagnostics use awaited sidecar queries so transport failures
  remain visible.
- **Host tool invocation is its own permission surface.** Guest `agentos-*`/tools-RPC calls must grant `permissions.binding` with `invoke` rules that match `<toolkit>:<tool>` patterns; if the same test/example also boots guest command software, keep `fs` and `childProcess` permissions explicit because command execution still needs those guest-visible capabilities.
- `packages/core` Vitest must exercise the real sidecar default. Permission-focused tests pass their own explicit policy; the shared setup performs cleanup only.

### Test Structure

See `.agent/specs/test-structure.md` for the full restructuring plan. Target layout:

- `unit/` -- no VM, no sidecar; pure logic (host-tools Zod conversion, descriptors, cron manager, etc.)
- `filesystem/` -- VFS CRUD, overlay, mount, layers, host-dir
- Shared filesystem conformance coverage in `src/test/file-system.ts` is fail-closed: backend-specific deviations must be modeled as explicit `capabilities` flags on the test descriptor, never with permissive `try/catch` branches that treat any thrown error as success.
- `process/` -- execution, signals, process tree, flat API wrappers
- `session/` -- ACP lifecycle, events, capabilities, MCP, cancellation
- `agents/{pi,claude,opencode,codex}/` -- per-agent adapter tests
- `wasm/` -- WASM command and permission tier tests
- `network/` -- connectivity and fetch behavior inside the VM
- `tests/migration-parity.test.ts` is the dedicated Rust/native migration gate. Keep it on the default `AgentOs.create()` sidecar path and make it cover filesystem, process, layer snapshot, tool dispatch, networking, and at least one real agent prompt/session flow together; the canonical invocation is `pnpm test:migration-parity` from the repo root.
- Host tool command-path coverage belongs with VM-backed sidecar tests such as `tests/sidecar-tool-dispatch.test.ts`, not a standalone TypeScript RPC server suite.
- Shell-backed host-tool dispatch coverage in `tests/sidecar-tool-dispatch.test.ts` needs the `@agentos-software/common` software package in the test VM so `/bin/sh` exists; otherwise the suite only proves direct spawn/RPC dispatch and misses the guest-shell path.
- `sidecar/` -- sidecar client, native process
- `cron/` -- cron integration

### WASM Binaries and Quickstart Examples

- **WASM command binaries are not checked into git.** The
  `registry/software/*/wasm/` directories are build artifacts.
- **Quickstart examples that use `exec()` or shell commands require WASM binaries.** Without them, these fail with "No shell available."
- **To build WASM binaries locally:** Run `just registry-native`, then build the
  required registry package. This requires Rust nightly and wasi-sdk.
- **Examples that work without WASM binaries:** `hello-world.ts`, `filesystem.ts`, `cron.ts` (schedule/cancel only).
- **When testing quickstart examples**, don't treat WASM-dependent failures as regressions unless the WASM binaries are present.

### Known VM Limitations

- `globalThis.fetch` is hardened (non-writable) in the VM -- can't be mocked in-process
- Kernel child_process.spawn can't resolve bare commands from PATH (e.g., `pi`). Use `PI_ACP_PI_COMMAND` env var to point to the `.js` entry directly.
- `allProcesses()` / `processTree()` on the native sidecar path are derived from
  the VM's kernel process snapshot, never host `ps` output or client PID
  remapping. `spawn()` already returns that same kernel PID.
- Module resolution reads the mounted `/root/node_modules` through the kernel VFS. Host-side adapter/agent package.json reads (for bin resolution) still use `readFileSync` against the host dir behind the `/root/node_modules` mount (or the matching software root)
- Native ELF binaries cannot execute in the VM -- the kernel's command resolver only handles `.js`/`.mjs`/`.cjs` scripts and WASM commands.
- Projected native assets under `/root/node_modules` are readable through module access, but guest `child_process.spawn*()` still routes them through the VM command resolver; spawning a projected ELF currently fails during WASM warmup instead of executing host-native code.
- The native sidecar framed stdio client is bidirectional: host-originated `request`/`response` frames use positive `request_id` values, and sidecar-originated `sidecar_request`/`sidecar_response` frames use negative IDs. When adding host callbacks, register a sidecar request handler instead of assuming stdout only carries events plus responses.

### Debugging Policy

- **Never guess without concrete logs.** Every assertion about what's happening at runtime must be backed by log output. Add logs at every decision point and trace the full execution path before drawing conclusions. Never assume something is a timeout issue unless there are logs proving the system was actively busy for the entire duration.
- **Never use CJS transpilation as a workaround** for ESM module loading issues. Fix root causes in the ESM resolver, the `/root/node_modules` mount / kernel VFS, or V8 runtime.
- **Diagnosing stalls / backpressure / silent hangs:** secure-exec runs a central limit registry (`secure_exec_bridge::queue_tracker`) over the chain of bounded queues (V8→host event channel, per-session frame channel, sidecar stdout/stdin frame queues). A full queue applies backpressure (it blocks the producer), so a "hung" session is often a slow/stuck *consumer* upstream, not a deadlock. The registry emits a structured `WARN` ("bounded limit near capacity…") as any limit crosses ~80%, and resource/heap/CPU breaches surface as typed errors naming the limit. Set `SECURE_EXEC_LOG=warn` (the default) to see near-limit warnings, or `SECURE_EXEC_LOG=debug` for per-limit usage snapshots; secure-exec logs to **stderr** (stdout is the wire protocol). See the **Limits & Observability** architecture doc (`website/src/content/docs/docs/architecture/limits-and-observability.mdx`).
- **Maintain a friction log** at `.agent/notes/vm-friction.md` for anything that behaves differently from a standard POSIX/Node.js system.
