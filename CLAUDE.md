# agentOS

A high-level wrapper around the Agent OS runtime that provides a clean API for running coding agents inside isolated VMs via the Agent Communication Protocol (ACP).

## Agent OS Runtime

Agent OS is a **fully virtualized operating system**. The kernel, written as a Rust sidecar, provides a complete POSIX-like environment -- virtual filesystem, process table, socket table, pipe/PTY management, and permission system. Guest code sees a self-contained OS and must never interact with the host directly. Every system call (file I/O, networking, process spawning, DNS resolution) must be mediated by the kernel. No guest operation may fall through to a real host syscall.

**⚠️ CRITICAL: ALL guest code MUST execute inside the kernel with ZERO host escapes.** The three execution environments (WASM, Node.js/V8 isolates, Python/Pyodide) must all run within the kernel's isolation boundary. No runtime may spawn unsandboxed host processes, touch real host filesystems, open real network sockets, or call real Node.js builtins. **NEVER use `Command::new("node")` for guest execution — not even temporarily, not behind a flag.** Guest JS runs in V8 isolates (`crates/v8-runtime/`). If tests fail because they assume the old host-process model, fix or delete the tests. See `crates/execution/CLAUDE.md` for details.

- **Virtualization invariants, key subsystems, and Rust architecture rules** -- see `crates/CLAUDE.md`
- **Node.js isolation model, polyfill rules, Python execution** -- see `crates/execution/CLAUDE.md`
- **Linux compatibility, VFS design, filesystem conventions** -- see `crates/kernel/CLAUDE.md`
- **Agent sessions (ACP), testing, debugging policy** -- see `packages/core/CLAUDE.md`
- **Registry packages (software, agents, file-systems, tools)** -- see `registry/CLAUDE.md`

## Project Structure

- **Monorepo**: pnpm workspaces + Turborepo + TypeScript + Biome
- **Core package**: `@rivet-dev/agent-os-core` in `packages/core/` -- contains everything (VM ops, ACP client, session management)
- **Use the renamed core package everywhere**: workspace dependencies and TypeScript subpath imports must reference `@rivet-dev/agent-os-core` (including `@rivet-dev/agent-os-core/internal/runtime-compat` and `@rivet-dev/agent-os-core/test/*`). The legacy `@rivet-dev/agent-os` name is stale and breaks pnpm workspace resolution.
- **Registry types**: `@rivet-dev/agent-os-registry-types` in `packages/registry-types/` -- shared type definitions for WASM command package descriptors. The registry software packages link to this package. When changing descriptor types, update here and rebuild the registry.
- **`crates/bridge/` is the browser/native portability seam.** Shared contracts like `ExecutionBridge` and `HostBridge` belong there. Do not treat native-only V8 embedding interfaces as cross-environment portability abstractions.
- **`crates/execution/` is the native execution implementation, not the portability layer.** Keep browser/native sharing at the `agent-os-bridge` boundary; `crates/sidecar-browser/` should not depend on `crates/execution/`.
- **npm scope**: `@rivet-dev/agent-os-*`
- **Actor integration** lives in the Rivet repo at `rivetkit-typescript/packages/rivetkit/src/agent-os/`, not as a separate package
- **The actor layer must maintain 1:1 feature parity with AgentOs.** Every public method on the `AgentOs` class (`packages/core/src/agent-os.ts`) must have a corresponding actor action in the Rivet repo's `rivetkit-typescript/packages/rivetkit/src/agent-os/`. Subscription methods are wired through actor events. Lifecycle methods are handled by the actor's onSleep/onDestroy hooks. This includes changes to method signatures, option types, return types, and configuration interfaces. **Always ask the user which Rivet repo/path to update** (e.g., `~/r-aos`, `~/r16`, etc.) before making changes there.
- **The RivetKit driver test suite must have full feature coverage of all agent-os actor actions.** Tests live in the Rivet repo's `rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/`. When adding a new actor action, add a corresponding driver test in the same change.
- **The core quickstart (`examples/quickstart/`) and the RivetKit example (in the Rivet repo at `examples/agent-os/`) must stay in sync.** Both cover the same set of features with identical behavior, just different APIs.
- **Every quickstart must have a matching automated test landed in parallel.** When adding or changing a quickstart under `examples/quickstart/`, add or update the corresponding test in the same change so the documented path stays truthful.

## Terminology

- Call instances of the OS **"VMs"**, never "sandboxes"

## Architecture

- **The VM base filesystem artifact is derived from Alpine Linux, but runtime source should stay generic.** `packages/core/src/` must not hardcode Alpine-specific defaults. The runtime consumes `packages/core/fixtures/base-filesystem.json` as the default root layer.
- **Base filesystem rebuild flow:** `pnpm --dir packages/core snapshot:alpine-defaults` writes `alpine-defaults.json`, then `pnpm --dir packages/core build:base-filesystem` rewrites AgentOs-specific values and emits `base-filesystem.json`.
- **The default VM filesystem model should be Docker-like.** Layered overlay view with one writable upper layer on top of one or more immutable lower snapshot layers.
- **Everything runs inside the VM.** Agent processes, servers, network requests -- all spawned inside the Agent OS kernel, never on the host. This is a hard rule with no exceptions.

## Native Binary Distribution

- **Ship the `agent-os-sidecar` binary to npm the same way `rivet-dev/rivet` ships `rivet-engine`** via `@rivetkit/engine-cli` (reference: the rivet repo's `rivetkit-typescript/packages/engine-cli/index.js` resolver and `.github/workflows/publish.yaml`). The pattern: a meta resolver package (`@rivet-dev/agent-os-sidecar`) with platform packages (`@rivet-dev/agent-os-sidecar-<platform>`) declared as `optionalDependencies`, the binary bundled in each platform tarball, selected at install by npm `os`/`cpu`/`libc`.
- **Platform binary packages MUST be published with `npm publish`, never `pnpm publish`.** `pnpm publish` normalizes file modes to `0644` and strips the binary's executable bit; `npm publish` preserves `0755`. The release workflow publishes platform packages via npm and workspace packages via pnpm (for `workspace:*` rewriting). Because `npm publish` preserves `0755`, the resolver does NOT `chmod` at runtime, matching `@rivetkit/engine-cli`'s `getEnginePath()`.
- **Spawn the native binary the same way `rivet-dev/rivet` spawns `rivet-engine`: thread the resolved absolute path as a typed value down to `Command::new(path)`, not through a process-global env var.** Reference `rivetkit-core/src/engine_process.rs`, where `engineBinaryPath` flows TS (`getEnginePath()`) → typed serve config → `Command::new(binary_path)`. The agent-os client must mirror this: the npm-resolved `getSidecarPath()` value is passed through `AgentOsConfig` to `SidecarTransport::spawn(...)` and into `Command::new(...)`. The `AGENT_OS_SIDECAR_BIN` env var stays only as a debug/override fallback; callers must not rely on `std::env::set_var` to hand the path to the spawn.

## Secure-Exec Reference Implementation

The Rust sidecar kernel was migrated from a working JavaScript kernel (`@secure-exec/core` + `@secure-exec/nodejs` + `@secure-exec/v8`). The original source is at `/home/nathan/secure-exec-1/` (tagged `v0.2.1`), and recovered polyfill/bridge code lives at `~/.agents/recovery/secure-exec/`. **When something doesn't work in the Rust V8 isolate runtime, check how secure-exec handled it first** — the answer is almost always already there. Key reference files:
- `nodejs/src/bridge-handlers.ts` (6,405 lines) -- host-side handlers for all kernel syscalls
- `nodejs/src/bridge/fs.ts` (3,974 lines) -- full kernel-backed `fs` polyfill
- `nodejs/src/bridge/network.ts` (11,149 lines) -- full `net`/`dgram`/`dns` polyfill
- `nodejs/src/bridge/process.ts` (2,251 lines) -- virtualized `process` global
- `nodejs/src/execution-driver.ts` (1,693 lines) -- V8 isolate session lifecycle

## V8 Polyfill and Module System Rules

- **Use `node-stdlib-browser` for pure-JS builtins, NOT hand-written stubs.** The package is already in `packages/core/package.json`. Bundle it into `v8-bridge.js` for modules like `path`, `assert`, `util`, `events`, `stream`, `buffer`, `url`, `querystring`, `string_decoder`, `punycode`, `constants`, `zlib`. Only write custom bridge-backed polyfills for kernel-backed modules (`fs`, `net`, `child_process`, `dns`, `http`, `os`, `crypto`). This is how secure-exec did it. Hand-written stubs are incomplete and break real packages.
- **Use undici for fetch(), not a high-level bridge call.** Guest `fetch()` must use undici running inside the V8 isolate, making TCP connections through the kernel socket table (`net.connect` bridge). Do NOT use `_networkFetchRaw` which bypasses the kernel network stack, permissions, and DNS. The fetch path must be: `undici → net.connect → kernel socket table → host network adapter`. This matches how real Node.js works.
- **Every Node.js builtin module must be a COMPLETE implementation, not a stub.** If `require('path')` is supported, it must have ALL standard methods (normalize, resolve, relative, join, dirname, basename, extname, isAbsolute, sep, delimiter, parse, format). A module that only implements `join` and `resolve` is a stub — stubs cause silent failures in real packages. If you can't implement a method fully, throw `ERR_NOT_IMPLEMENTED` — never return undefined or silently skip.
- **CJS export extraction must handle dynamic patterns.** The ESM wrapper for CJS modules extracts named exports via `extract_cjs_export_names()`. This MUST handle: `exports.X = ...`, `Object.defineProperty(exports, ...)`, `Object.assign(module.exports, ...)`, and spread syntax. If static extraction fails, fall back to runtime extraction (evaluate module, enumerate `Object.keys(module.exports)`). Incomplete extraction causes missing named imports that silently break downstream packages.
- **CJS/ESM interop must never hang.** If `require()` is called on an ESM-only package, throw `ERR_REQUIRE_ESM` immediately — never recurse infinitely or hang. If `import()` is called on a CJS package, wrap it in an ESM shim. Test both directions.
- **Circular dependencies must terminate.** The module cache must prevent re-evaluation. Test with A→B→A and A→B→C→A chains.
- **Every polyfill addition needs a conformance test.** When adding a new builtin method or module, add a test that verifies the return value matches real Node.js behavior. Tests go in `crates/execution/tests/` or `crates/sidecar/tests/`.

## npm Package Compatibility

- **npm packages must work UNMODIFIED inside the VM.** The V8 module resolver must load published npm packages from `node_modules/` as-is — no esbuild, no bundling, no transpilation, no preprocessing. If `require('some-package')` or `import 'some-package'` doesn't work, fix the module resolver or polyfills, don't add a build step to transform the package. The goal is: `npm install` a package on the host, mount `node_modules/` into the VM, and it just works.
  - **Exception: the mounted `node_modules` must use a flat (hoisted) layout, not a symlinked one.** Mounting host `node_modules` into the VM is a bind-mount, so it inherits the same symlink-portability limit as Docker bind-mounts: package managers that lay out `node_modules` as symlinks into a content store (pnpm's default `node-linker=isolated`, yarn-berry) do not resolve inside the VM, because Node canonicalizes a required module to its store realpath (e.g. `node_modules/.pnpm/...`) which the guest `fs` cannot see. npm, yarn-classic, and bun produce flat layouts and work as-is. For pnpm use `node-linker=hoisted` (in `.npmrc`); for yarn-berry use `nodeLinker: node-modules` (in `.yarnrc.yml`, not PnP). Do NOT "fix" this by mounting the `.pnpm` store wholesale — that over-exposes the host tree, pins to churning hashed paths, and still breaks on cross-package peer-dep symlinks. The proper long-term fix is a module-projection layer (a single virtualized module tree both the loader and guest `fs` read through); until that exists, surface a typed, actionable error at `module_access` mount setup that names the hoisted-linker fix rather than letting agents crash with a deep `ENOENT` on a `.pnpm` path.
- **Agent SDKs must run unmodified.** Pi SDK (`@mariozechner/pi-coding-agent`), Anthropic SDK (`@anthropic-ai/sdk`), and any other agent SDK must load and execute inside V8 without modification. Our custom ACP adapters (`registry/agent/*/`) are thin wrappers that import the SDK — the SDK itself is never patched or bundled.

## Agent Adapters

- **Agent adapters MUST use the real agent SDK.** Each agent adapter (`registry/agent/*/src/adapter.ts`) must call the agent's SDK directly (e.g., `createAgentSession()` from `@mariozechner/pi-coding-agent`). **NEVER replace an SDK adapter with a minimal/stub adapter that makes direct API calls** (e.g., direct `fetch` to `/v1/messages`). If the SDK doesn't work in V8, fix the V8 compatibility — don't bypass the SDK.
- **No host agent exceptions.** Host-native wrappers and host binary launch paths are not allowed.
- **Claude patched SDK/CLI artifacts are discovered via dist manifests.** `registry/agent/claude/scripts/build-patched-cli.mjs` writes `dist/claude-cli-patched.json` and `dist/claude-sdk-patched.json`; the adapter resolves those manifests first and only falls back to the upstream SDK files when they are missing. Update the build script/manifests rather than hardcoding hashed artifact paths in the adapter.

## VM System Tools

- **The VM has a full POSIX toolchain.** WASM-compiled coreutils, `sh`, `grep`, `sed`, `awk`, `find`, `tar`, `git`, and 100+ other commands are available via registry software packages (`registry/software/`, compiled from `registry/native/crates/commands/`). Agent code running inside the VM can spawn these tools via `child_process`. **Do not assume system tools are missing** — if a command isn't resolving, debug the command resolution path in the sidecar, don't work around it.

## Dependencies

- **Rivet repo** -- A modifiable copy lives at `~/r-aos`. Use this when you need to make changes to the Rivet codebase.
- Mount host `node_modules` read-only for agent packages (pi-acp, etc.)

## Documentation

- **Keep docs in `~/r-aos/docs/docs/agent-os/` up to date** when public API methods or types are added, removed, or changed on AgentOs or Session classes.
- **Keep the standalone `secure-exec` docs repo up to date** when exported API methods, types, or package-level behavior change for public `secure-exec` compatibility packages. The source of truth is the repo that contains `docs/docs.json`.
- **The active public `secure-exec` package scope is currently `secure-exec` and `@secure-exec/typescript`.** Do not assume other legacy `@secure-exec/*` packages are still part of the maintained public surface unless the user explicitly says so.
- **If a user asks for a `secure-exec` change without naming the package, prompt them to choose the target public package when it is ambiguous.**
- **Keep `website/src/data/registry.ts` up to date.** When adding, removing, or renaming a package, update this file so the website reflects the current set of available apps.
- **No implementation details in user-facing docs.** Never mention WebAssembly, WASM, V8 isolates, Pyodide, or SQLite VFS in documentation outside of `architecture.mdx`. Use user-facing language instead.
- If you need to look at the documentation for a package, visit `https://docs.rs/{package-name}`. For example, serde docs live at `https://docs.rs/serde/`.

## Agent Working Directory

All agent working files live user-scoped in `~/.agents/`, never inside the repo. Override the location with the `AGENTS_DIR` env var. These files are not committed; `.agent/` is gitignored as a safety net.

- **Specs**: `~/.agents/specs/` -- design specs and interface definitions for planned work.
- **Research**: `~/.agents/research/` -- research documents on external systems, prior art, and design analysis.
- **Todo**: `~/.agents/todo/*.md` -- deferred work items with context on what needs to be done and why.
- **Notes**: `~/.agents/notes/` -- general notes and tracking.

When the user asks to track something in a note, store it in `~/.agents/notes/` by default. When something is identified as "do later", add it to `~/.agents/todo/`. Design documents and interface specs go in `~/.agents/specs/`.

## CLAUDE.md Convention

- Every directory that has a `CLAUDE.md` must also have an `AGENTS.md` symlink pointing to it (`ln -s CLAUDE.md AGENTS.md`). This ensures other AI agents that look for `AGENTS.md` find the same instructions.
- When adding entries to any `CLAUDE.md`, keep them concise -- ideally a single bullet point. Do not write paragraphs.
- Only add design constraints, invariants, and non-obvious rules that shape how new code should be written. Do not add general trivia, current implementation wiring, module organization, API signatures, or ephemeral migration state. Anything a reader can learn from the code belongs in module doc-comments or reference docs.

## Naming + Data Conventions

- Data structures often include:
  - `id` (uuid)
  - `name` (machine-readable name, must be valid DNS subdomain, convention is using kebab case)
  - `description` (human-readable, if applicable)
- Use UUID (v4) for generating unique identifiers.
- Store dates as i64 epoch timestamps in milliseconds for precise time tracking.
- Timestamps use `*_at` naming with past-tense verbs. For example, `created_at`, `destroyed_at`.

## Code Style

- Follow existing patterns in neighboring files.
- Always check existing imports and dependencies before adding new ones.
- **Always add imports at the top of the file instead of inline within a function.**
- Never use a `_ =>` fall-through arm when matching on a Rust enum or a TypeScript discriminated union. Enumerate every variant so adding a new one later is a compile error, not a silent behavior change. `_` is fine for `Result`, `Option`, integers, strings, and other open value spaces. `_ => unreachable!()` / `_ => panic!()` are explicit asserts and acceptable.

### Comments

- Write comments as normal, complete sentences. Avoid fragmented structures with parentheticals and dashes (hyphens are OK).
- Do not use em dashes. Use periods to separate sentences instead.
- Documenting deltas is not useful. A developer who never saw the previous code gains nothing from a comment saying something was removed or changed. The only reason to note something missing is if its absence is unintuitive.

## Logging

- Use tracing in Rust. Never use `eprintln!` or `println!` for logging. Always use `tracing::info!`, `tracing::warn!`, `tracing::error!`, etc.
- Do not format parameters into the main message. Use structured fields: `tracing::info!(?x, "foo")` instead of `tracing::info!("foo {x}")`.
- Log messages should be lowercase unless mentioning specific code symbols.

## Error Handling

- Always return anyhow errors from failable Rust functions. Do not glob-import from anyhow. Prefer `.context()` over the `anyhow!` macro.
- A failing fallback path must rethrow the original error with the fallback's failure attached as context. Never let the fallback's error replace the original.

## Runtime Limits

- **Every new limit-shaped constant must be classified.** Any `MAX_*` / `*_LIMIT` / `*_CAPACITY` / retention / sizing constant added under the scanned roots must get an entry in `crates/sidecar/tests/fixtures/limits-inventory.json`: either `policy` (wired through `VmLimits` with a `wired` field naming its config field) or `invariant`/`policy-deferred` with a one-line rationale. The `cargo test -p agent-os-sidecar --test limits_audit` audit fails when a qualifying constant is unclassified.

## Fail-By-Default Runtime

- Avoid silent no-ops for required runtime behavior. If a capability is required, validate it and throw an explicit error with actionable context instead of returning early.
- Do not use optional chaining for required lifecycle and bridge operations. Optional chaining is acceptable only for best-effort diagnostics and cleanup paths (logging hooks, dispose/release cleanup).
- Never land a public callback, stream, or event API without a wired delivery source. If the source is not wired yet, the doc comment must say so explicitly so callers do not wait on a stream that never yields.

## Async Rust Locks

- Async Rust code defaults to `tokio::sync::Mutex` / `tokio::sync::RwLock`. Do not use `std::sync::Mutex` / `std::sync::RwLock`.
- Use `parking_lot::Mutex` / `parking_lot::RwLock` only when sync is mandated by the call context: `Drop`, sync traits, FFI callbacks, or sync `&self` accessors.
- If an external dependency's struct requires `std::sync::Mutex`, keep it at the construction boundary with an explicit forced-std-sync comment.
- Prefer async locks because sync guards can be silently held across `.await`, and poisoning creates `.expect("lock poisoned")` boilerplate.

## Performance

- **VM/agent coldstart latency is a first-class constraint. Keep the start path cheap.** Do not do expensive or O(closure)/O(filesystem) work on every VM bring-up or session create (bulk file materialization, dependency-graph walks, hashing large trees, network fetches, recompiling/rebundling). Such work must be deterministic-keyed and done once, then reused: precompute it at build/publish time into an immutable lower layer, or cache it by a stable key (e.g. lockfile hash) and surface it as a shared read-only layer. Coldstart should attach/mount a prebuilt artifact, not rebuild it. Example: node_modules closure materialization (see the hoisted-`node_modules` note under npm Package Compatibility) is baked into the agent software layer at publish time and lockfile-cached for user deps, never rebuilt per start.
- Never use `Mutex<HashMap<...>>` or `RwLock<HashMap<...>>`. Use `scc::HashMap` (preferred), `moka::Cache` (for TTL/bounded), or `DashMap` for concurrent maps. Use `scc::HashSet` instead of `Mutex<HashSet<...>>`.
- Hold lock guards as briefly as possible. Clone/copy needed data and `drop(...)` before async work.
- Never poll a shared-state counter with `loop { if ready; sleep(Nms).await; }`. Pair the counter with a `tokio::sync::Notify` (or `watch::channel`) that every decrement-to-zero site pings, and wait on that instead.
- Reserve `tokio::time::sleep` for per-call timeouts, retry/reconnect backoff, deliberate debounce windows, or `sleep_until(deadline)` arms in an event-select loop. A `loop { check; sleep }` body is polling and should be event-driven instead.
- `scc` async methods do not hold locks across `.await` points. Use `entry_async` for atomic read-then-write.
- Never add unexplained wall-clock defers like `sleep(1ms)` to decouple a spawn from its caller. Use `tokio::task::yield_now().await` or rely on the spawn itself.
- Polling is forbidden in every language and layer, not just Rust. Never wait for a state change by re-checking in a loop in TypeScript, tests, or shell. Wait on an event: a Notify/watch channel, promise, callback, process exit, or stream EOF. If an external system genuinely offers no event signal, bound the poll with a deadline and justify it in a comment.
- Never block while holding a lock. No bounded-channel sends, thread joins, or IO under any lock guard. Remove or copy the needed state under the lock, release it, then do the blocking work.
- Code that registers a waiter or pending entry in a shared queue must remove it on every exit path: success, early drain, timeout, and error.

## Memory Leaks

- Do not introduce intentional leaks (`Box::leak`, `std::mem::forget`, `*_into_raw` without matching cleanup) unless an upstream API makes ownership impossible to express safely.
- Never call `Box::leak` inside a per-request, per-error, or per-call code path. If a `'static` reference is required, use a compile-time `static`/`const` or intern it through a process-global map keyed by identity.
- Interned leaks must be bounded by unique schema/config identity and must not include unbounded user input such as raw error messages, request paths, or headers.
- `std::mem::forget` is only acceptable when an FFI handle cannot be dropped in the current context; document the constraint inline, prove the leak is bounded, and prefer routing cleanup through an Env-bearing owner.
- Spawned futures that capture JS callbacks or other heavy resources must have a guaranteed completion path (e.g. a `CancellationToken` whose clones are guaranteed to drop). A `spawn_local(async move { token.cancelled().await; ... })` only drains if every clone of the token is dropped or cancelled.

## Untrusted Input

- Write parser bounds checks in subtraction form after an explicit minimum-length guard (`len >= off && len - off >= n`), never `off + n > len`, which wraps on 32-bit targets.
- Cap any allocation whose size derives from untrusted input before allocating.

## Testing

- **Never use `vi.mock`, `jest.mock`, or module-level mocking.** Write tests against real infrastructure (real kernel, real filesystems, real processes). For LLM calls, use `@copilotkit/llmock` to run a mock LLM server. For protocol-level test doubles (e.g., ACP adapters), write hand-written scripts that run as real processes. `vi.fn()` for simple callback tracking is acceptable.
- **Never paper over flakes with retry loops or bumped waits.** When a test flakes, root-cause the race, write a deterministic repro, fix the underlying ordering, and delete any flake-workaround note.
- **Rust tests live under `tests/`, not inline `#[cfg(test)] mod tests` in `src/`.** Exceptions must be justified (e.g., testing a private internal that can't be reached from an integration test).

## Version control (jj)

- This repo uses jj (Jujutsu) on top of git. **jj's workflow is inverted from git:** the working copy is itself a revision that auto-tracks edits, so you create a new revision *before* making changes (with `jj new`) rather than committing *after* (`git commit`). The description is set separately via `jj describe`. There is no staging step.
- Before making changes, check whether jj is initialized by running `jj status`. If it fails (e.g. "There is no jj repo in '.'"), run `jj git init --colocate` from the repo root so jj lives alongside the existing `.git` directory. Do NOT run `jj git init` without `--colocate` — that creates a standalone jj repo and breaks the git workflow.
- **One revision = one self-contained change. MUST run `jj new` before starting each change**, before reading, planning, or editing. The unit is the *change*, not the *task*, *request*, or *session*. A single user request routinely contains several unrelated changes (a fix here, a refactor there, a test update); each one is its own revision, so run `jj new` again the moment you move on to the next change. Do not let edits pile up in one revision just because they came from one prompt or one work session.
- **Heuristic for "is this one revision or several?"** If a single `jj describe` line cannot honestly describe the whole diff without the word "and", or the diff spans unrelated subsystems/concerns (e.g. a test fix plus a build change plus an adapter tweak), it is more than one revision. Err toward more, smaller revisions. A revision touching a dozen files across many subsystems under a vague message like "triage failed tests" is the anti-pattern, not the goal.
- Run it before reading, before planning, before editing. The only exception is when you are directly fixing or finishing the change at `@` that you just made in this same session. In that case use `jj squash --into <rev>` or `jj edit <rev>`. If you already started editing and find the working copy now mixes unrelated changes, stop immediately and split them apart with `JJ_EDITOR=true jj split <paths>` before continuing. Never mix unrelated work into one revision.
- Set the revision description with `jj describe -m "[SLOP({full-model-id}-{reasoning})] {conventional commit message}"`. Use conventional commits (`feat`, `fix`, `chore`, `docs`, `refactor`, etc.) with a single-line message. `{full-model-id}` is the canonical model ID (e.g. `claude-opus-4-7`, `claude-sonnet-4-6`, `claude-haiku-4-5`). `{reasoning}` is the reasoning effort (`high`, `medium`, `low`, `off`) — include it only if the runtime exposes it; otherwise omit the `-{reasoning}` suffix entirely.
- Examples: `[SLOP(claude-opus-4-7-high)] feat(metrics): record depot sqlite phase timings` or, when reasoning is not known, `[SLOP(claude-opus-4-7)] fix(pegboard): handle empty ack batch`.
- **Never add a co-author trailer** (no `Co-Authored-By: ...` line). Descriptions are single-line only.
- **A revision description must describe its actual diff.** Check the message against `jj diff -r <rev> --stat` before running `jj describe`.
- Abandon stray empty undescribed revisions before ending a session. Do not leave `jj new` artifacts in the branch.
- Never commit fetched or vendored source trees. Add the ignore entry before fetching.
- **Never push to `main` unless explicitly specified by the user.**
- **Safety:** Never run destructive jj or git commands (`jj git push`, `jj abandon`, `jj squash` into a non-current revision, `jj op restore`, `jj op undo` past your own work, `jj rebase -d main`, `git push --force`, `git reset --hard`) unless the user explicitly requests it.

## Build & Dev

```bash
pnpm install
pnpm build        # turbo run build
pnpm test         # turbo run test
pnpm check-types  # turbo run check-types
pnpm lint         # biome check
```

- **Always run tests and agent-os-heavy commands through `just test-bounded '<command>'`** so the whole process tree is constrained by systemd. This keeps test runners, sidecars, agent sessions, builds, and their subprocesses from overwhelming the host by capping them to 60% of logical CPUs and 60% of currently available memory with lower CPU/IO priority. Use `just test-bounded` for the default `pnpm test`, or pass an explicit shell string such as `just test-bounded 'pnpm --dir packages/core exec vitest run tests/pi-sdk-adapter.test.ts'`.
- CI and release automation must install the pnpm workspace with `--frozen-lockfile` before Cargo builds that generate V8 bridge assets into `OUT_DIR`. Fork pull requests should run the same `pnpm test` command without `AGENTOS_E2E_NETWORK=1`.
- When changing V8 bridge registration or snapshot bootstrap code under `crates/v8-runtime/`, rebuild `agent-os-v8-runtime` before rerunning sidecar V8 integration tests. `cargo test -p agent-os-sidecar` can otherwise reuse stale embedded-runtime objects from `target/`.
- The `crates/v8-runtime` snapshot test (`snapshot::tests::snapshot_consolidated_tests`) currently has to run in isolation: use `cargo test -p agent-os-v8-runtime -- --test-threads=1` for the main suite and `cargo test -p agent-os-v8-runtime snapshot::tests::snapshot_consolidated_tests -- --exact --ignored` separately until the shared test binary teardown SIGSEGV is fixed.
- Biome honors `.gitignore` (`vcs.useIgnoreFile`), and the core-dump patterns (`**/core`) match `packages/core`, so `pnpm lint` silently skips that entire package. Do not treat a green lint as proof those files were checked. Fixing the pattern requires first cleaning up the package's accumulated lint debt (tracked in `~/.agents/todo/`).
