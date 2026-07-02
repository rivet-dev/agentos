# agentOS

Agent OS is the agent-facing wrapper around secure-exec. It provides ACP sessions, agent adapters, quickstarts, and the public AgentOs client APIs while depending on secure-exec for the generic VM runtime.

## Boundaries

- secure-exec dependency workflow. Manage the secure-exec dependency ONLY through `scripts/secure-exec-dep.mjs` (the `just secure-exec-*` / `just agentos-pkgs-*` recipes); never hand-edit the `path` / `version` / `catalog:` pins.
  - **GATE — the COMMITTED state is always FILE-BASED.** Every `@secure-exec/*` npm dep is `link:../secure-exec/...`, every `secure-exec-*` crate a `path = "../secure-exec/..."` dep, and every `@agentos-software/*` package a `link:` into `../secure-exec/registry/{software,agent}/*`, with `.github/refs/secure-exec` pinning the full sha CI materializes the sibling at. `scripts/verify-file-deps.mjs` enforces this in CI — a branch with published-version pins fails the gate. Published-version pins exist ONLY transiently inside publish workflow checkouts (`release-swap`), never on a branch.
  - Local development therefore needs no mode flip: clone/keep a sibling `../secure-exec` checkout, `pnpm install` there once (or cargo panics in `v8-runtime/build.rs` with "missing Node dependencies at .../packages/build-tools/node_modules"), and build its registry packages (`just registry-native` + `just registry-build` there) for the shell/software links. `just secure-exec-status` inspects; `node scripts/secure-exec-dep.mjs release-revert` restores the file-dep state if a swap leaked.
  - **The ref (`.github/refs/secure-exec`)** is the ONLY committed secure-exec version state: one full 40-char sha that CI materializes `../secure-exec` at, publishes derive the preview identity from, and the sibling build cache keys on. Bump it with `just secure-exec-bump [sha]` (defaults to the sibling checkout's current commit) — never hand-edit. Bump WHEN: (a) agent-os work needs a secure-exec change — merge that change to secure-exec main first, then bump to the merge sha; (b) before cutting a release-preview/release that must carry newer secure-exec. The sha must stay reachable in rivet-dev/secure-exec (main-merged shas always are; squash-merges orphan branch heads). Each new sha's first CI run rebuilds the sibling from scratch (slow, then cached). See "Publishing against secure-exec" and the `bump-secure-exec` skill.
- Keep generic runtime, kernel, VFS, language execution, generic registry software, and packaged agent definitions/adapters in secure-exec.
- Agent OS owns ACP, sessions, toolkit semantics, quickstarts, docs, and the AgentOs facade.
- Call OS instances VMs, never sandboxes.
- Keep root `package.json` scripts limited to Turbo orchestration; put repo-specific commands in `justfile` recipes.
- The protocol has no backwards compatibility. Clients and the sidecar ship in same-version lockstep, so never add protocol or config versioning, runtime negotiation, fallbacks, or converters. Configs such as `CreateVmConfig` carry no `version` field; the single same-version wire handshake is the only version check. Change the protocol freely and update both sides together.

## Development

### secure-exec dependency versions (`just`)

Three release tracks:
- **secure-exec runtime** — `@secure-exec/*` npm packages and `secure-exec-*` crates. See "Depending on unreleased secure-exec changes" for preview-vs-release behavior.
- **`@agentos-software/*` registry packages** — generic VM software from secure-exec `registry/software/*` plus agent adapters from secure-exec `registry/agent/*`; versioned independently of secure-exec runtime packages AND independently of each other (per-package semver, pinned per-package in the catalog). Built/published in secure-exec via `@rivet-dev/agentos-toolchain` + the `just registry-*` recipes there (dist-tag `dev` by default; `latest` only deliberately).
- **agent-os product/API** — `@rivet-dev/agentos*`, AgentOs APIs, sidecar wrapper, docs, quickstarts, and examples; pins compatible secure-exec and registry package versions.

Manage them ONLY via these recipes (never hand-edit `path`/`version`/`catalog:` pins):
The COMMITTED state is file-based on both tracks (the Boundaries gate); the pinned/published state exists only transiently at publish time. Recipes:
- `just secure-exec-bump [sha]` — pin `.github/refs/secure-exec` to a new secure-exec sha (defaults to the sibling checkout's commit). This is THE way agent-os advances its secure-exec dependency.
- `just secure-exec-status` / `just agentos-pkgs-status` — show both tracks' modes + the catalog versions (the catalog holds the release-swap defaults; it is not what the committed tree resolves).
- `just secure-exec-local` / `just agentos-pkgs-local` — restore the file-dep state for the runtime / registry track (equivalently `node scripts/secure-exec-dep.mjs release-revert` for both at once). The committed tree must always be in this state.
- `just secure-exec-pinned`, `just secure-exec-set-version <v>`, `just agentos-pkgs-pinned`, `just agentos-pkgs-set-version <pkg> <v>`, `just agentos-pkgs-update [tag]` — publish-time pinning machinery (what `release-swap` composes). Never commit their output; the `verify-file-deps` CI gate rejects it.

### Workflow skills

Follow these skills rather than improvising the flows:
- `.claude/skills/bump-secure-exec` — advance the committed `.github/refs/secure-exec` sha (the only way agent-os takes newer secure-exec).
- `.claude/skills/release-preview` — end-to-end release-preview: bump ref → `just release-preview <branch>` (auto-cuts the secure-exec preview).
- `.claude/skills/release` — versioned releases (`just release --secure-exec-version <v> ...`).
- secure-exec side: its `.claude/skills/{release-secure-exec,publish-registry}` cover cutting secure-exec releases and publishing `@agentos-software/*` packages.

### Publishing against secure-exec

The committed deps are file-based (see the Boundaries gate), so PUBLISHES are what pin real versions — transiently, inside the workflow checkout, via `secure-exec-dep.mjs release-swap <secure-exec-version> <registry-dist-tag>` (the release script/workflow owns the swap AND the revert; a branch never carries pins):

- **agent-os release-preview** (`just release-preview <branch>`): the `secure-exec-version` job reads `.github/refs/secure-exec`, and cuts (or reuses) a **secure-exec preview at exactly that sha** — branch `agentos-dep-<sha7>`, npm version `0.0.0-agentos-dep-<sha7>.<sha7>`, registry packages under the same dist-tag. Every build/publish job then release-swaps npm to those versions; cargo stays path-based and `prepare-build` materializes `../secure-exec` at the ref (crates.io has no preview track). Cross-repo dispatch needs the `SECURE_EXEC_DISPATCH_TOKEN` secret (workflow rights on secure-exec).
- **agent-os release** (`just release --secure-exec-version <v>`): requires a **real secure-exec release** `<v>` (verified on npm AND crates.io) so the published npm packages and the crates.io crates reference the same secure-exec. The swap pins npm + crates to `<v>`; no clone is needed.

**Invariant — `prepare-build` must precede EVERY cargo invocation in EVERY pipeline.** The committed cargo deps point at `../secure-exec`, which does not exist in a fresh checkout — `prepare-build` materializes it at the committed `.github/refs/secure-exec` sha (clone + `pnpm install`; `--build` also builds the TS packages + native wasm for the npm links, `--clone-only` is just enough for `pnpm install` to resolve `link:` deps). After a release `release-swap` it is a no-op (everything resolves from registries), so running it is always safe. The pipelines are: `.github/workflows/ci.yml` (PR + main gate; also runs the `verify-file-deps` gate and caches the built sibling on the ref sha), `.github/workflows/publish.yaml` (preview/release), and `docker/build/darwin.Dockerfile`. If you add a new pipeline, job, or cargo step, run `prepare-build` before it.

### Release-previewing agent-os

`just release-preview <branch>` dispatches `.github/workflows/publish.yaml` to cut a **preview** (debug build, npm-only, dist-tag = sanitized branch name) — for handing a build to an external project. **Release-preview is for previews ONLY; never cut a release with it.** Releases go through `just release` (the `scripts/publish` flow); see the `release` and `release-preview` skills.

### Testing a local build from an external project (same machine)

To consume an unpublished agent-os build in another project on this machine:
- **npm:** `pnpm -r build`, then either `pnpm pack` the package(s) and `npm install ./rivet-dev-agentos-*.tgz` in the external project, or add a `link:`/`file:` override (e.g. `"@rivet-dev/agentos": "link:/abs/path/agent-os/packages/agentos"`). The sidecar binary ships as `@rivet-dev/agentos-sidecar`.
- **cargo:** point the external Cargo project at the local crate via a path dep or `[patch.crates-io]` override (e.g. `[patch.crates-io] agentos-sidecar = { path = "/abs/path/agent-os/crates/agentos-sidecar" }`).

## Security Model

Trust model (decide which side of the boundary something is on before judging whether it is a security bug). Three components:

- **Client** (trusted, *except for anything it submits for execution*). The AgentOs client / wire caller. The client and every value it configures are trusted: `CreateVmConfig`, mount descriptors and plugin configs (host_dir paths, S3 endpoints/credentials, Google Drive, sandbox-agent), the permission policy, network allowlist, resource limits, env, and DNS overrides. Configuration is **not** an attack surface. The only untrusted thing the client supplies is the code/payload it asks to run, because that runs in the executor.
- **Sidecar** (trusted; the TCB and enforcement point). The agent-os sidecar embeds and extends secure-exec; it brokers client requests and owns the kernel, VFS, mounts/plugins, socket table, and permission policy, and enforces the boundary against the executor.
- **Executor** — V8 isolates or WASM (untrusted; the adversary). Runs guest JS/Python/WASM plus any third-party/npm/agent-generated code. Assume it is actively hostile; how code reached the executor never makes it trusted.

**The security boundary is sidecar ↔ executor.** A defect that requires the client to supply a malicious config/endpoint/credential/policy is NOT a sandbox vulnerability (the client configures its own VM and already controls the host). Treat such hardening as defense-in-depth, not as an escape, and do not add validation that only guards trusted client-provided configuration. Corollaries: the permission policy/limits are trusted input but the guest is the subject they bind, so a guest *bypassing* an applied rule is in-scope; a host-backed mount's target/credentials are trusted, but confining the guest's I/O *through* it (symlink / `..` / TOCTOU escapes) is in-scope. The wire transport is single-client over stdio, so wire authn/authz-between-clients and VM-to-VM-via-forged-id concerns are out of scope until a multi-client transport exists. See secure-exec root `CLAUDE.md` → Trust Model for the canonical statement.

- Isolation is layered (defense in depth), like Cloudflare Workers. Untrusted guest code is isolated *within* the host process by V8/WASM virtualization today; host-level jailing (sandboxing the process itself) is a planned additional layer. Because the in-process layer is load-bearing: keep the embedded V8 patched to current security releases, and never let one isolate take down the shared process — a per-isolate failure (heap OOM, CPU runaway) must terminate that isolate, not abort the host process.
- Match Cloudflare Workers wherever it makes sense. Use Workers' published behavior as the reference point for isolation semantics, resource limits, and egress defaults — e.g. ~128 MiB memory per isolate, bounded CPU time, default-deny network egress. Resource limits must be bounded by default (never `None`/0 for memory, heap, stack, or CPU time); operators may raise them.

## Limits, Bounds & Observability

Every limit, timeout, bounded queue/buffer, and per-entity collection MUST be bounded by default, warn as it approaches, and fail clearly. This applies to both the secure-exec limits agent-os forwards AND agent-os-owned bounds (ACP/session/frame timeouts, the sidecar event buffer, session/shell-id retention, host-tool registration caps, in-VM adapter log buffers). The canonical statement lives in secure-exec root `CLAUDE.md` → "Limits, Bounds & Observability"; agent-os adds:

- **Forward, don't reimplement.** Resource limits (`limits.resources/jsRuntime/python/wasm/acp/tools/...`) are secure-exec's; agent-os forwards them on the wire and surfaces them in `AgentOsOptions.limits` — never duplicate the enforcement or the defaults TS-side. Agent-os owns only the ACP/session/client-layer bounds.
- **Warn-on-approach + typed error for agent-os-owned bounds.** ACP method timeouts (`initialize`/`session/new`/`session/prompt`/default — `acp_extension.rs`), the native-sidecar frame timeout, and the event buffer must emit a structured near-threshold signal (default ≥80%) and fail with a typed error that names the limit and how to raise it. ACP timeout errors already carry `data.kind === "acp_timeout"` (keep using `isAcpTimeoutErrorData`); extend the rest to match. The default 120s ACP method timeout is the adapter-stall failure mode — make it observable, not a silent 120s hang.
- **No unbounded per-entity collections.** Anything that grows per session / process / shell / toolkit (`_sessions`, tracked-process maps, socket-lookup caches, toolkit registries) must be a bounded collection that warns on eviction, or carry an explicit, documented cap. Enforce registration caps (`maxRegisteredToolkits`, etc.) at registration time, not silently at use time. Silent LRU eviction without a log is a footgun.
- **Host-visible.** A limit warning/error that fires inside the VM or sidecar is useless if it never reaches the host — route it through the agent-process log channel (`onAgentStderr` / the ACP-extension forward) and a structured trace channel, the same path that makes adapter stderr observable.

## Agent Sessions

- Every public method on `packages/core/src/agent-os.ts` must stay mirrored by RivetKit actor actions after the user confirms the Rivet repo path.
- Subscription methods are delivered through actor events; lifecycle behavior belongs in actor sleep/destroy hooks.
- Agent adapters must use real upstream agent SDKs. Do not replace SDK adapters with direct API-call stubs.
- Host-native agent wrappers are not allowed; agents run through the VM runtime supplied by secure-exec.

## Extension Authoring

- Agent OS extension payloads use the secure-exec `Ext` envelope with Agent OS-owned namespaces and generated ACP payloads.
- Keep ACP decoding and session state in Agent OS wrapper code, not in secure-exec core sidecar code.
- The agent-os sidecar wrapper embeds and extends secure-exec; secure-exec must remain free of ACP, agent, and session dependencies.
- Prefer the agent-os sidecar wrapper for heavy lifting. Multi-step ACP/session orchestration, state machines, and anything that would otherwise cost several client→sidecar round-trips belong in the sidecar (`crates/agentos-sidecar`), exposed as a single wire request; the TypeScript (`packages/core`) and Rust (`crates/client`) clients stay thin forwarders and must BOTH expose it. Rationale: (a) keep clients simple and in parity, (b) cut client↔sidecar latency. Keep logic client-side only when it needs state the sidecar cannot reach — e.g. RivetKit actor durable storage (`ctx.db_*`/SQLite), which the sidecar has no access to. Even then, the sidecar must not pull ACP/session deps into secure-exec core.

## Benchmarks

- Agent OS keeps **product-surface benches only** in `scripts/benchmarks/`: `session.bench.ts` (VM-tax vs bare-node pi SDK — the CI-gated one, with `baseline.json`), `coldstart.bench.ts`, and `memory.bench.ts`. Run via `bash scripts/benchmarks/run-benchmarks.sh` (`BENCH_ONLY=<lane>`).
- The runtime differential matrix (native/node/vm-js/vm-wasm lanes), focused runtime lanes (bridge floor, sockets, fs, readdir, process spawn), and ecosystem command benches (`ls`, `grep`, `git`, `sh`) live in **secure-exec `packages/benchmarks`** — see secure-exec root `CLAUDE.md` → Benchmarks. Runtime perf work belongs there, not here.
- **Keep them current:** changes to session/ACP/client behavior that affect latency or memory must re-run the affected agent-os lanes; new product surface (session kinds, adapters, client-side orchestration) needs a lane or an explicit follow-up. Never delete or silently skip a bench — skips carry a reason.

## Website And Docs

- External/consumer usage (installing `@rivet-dev/agentos` and using it in your own project) is documented in the website quickstart + Agents/Custom Software pages under `website/`, not in this file. This `CLAUDE.md` is contributor/maintainer-only.
- The Agent OS website and docs live in `website/` (Astro + Starlight) and deploy to `agentos-sdk.dev` (docs at `agentos-sdk.dev/docs`). The marketing pages and docs were migrated out of `rivet.dev/agent-os` and `rivet.dev/docs/agent-os`, which now 301-redirect to this domain.
- Docs styling is owned by the shared **`@rivet-dev/docs-theme`** repo (`github.com/rivet-dev/docs-theme`), consumed via `github:rivet-dev/docs-theme#<tag>` and wired in via `...docsTheme(starlight, siteConfig)`. To change any docs styling (palette, header, sidebar, code blocks, fonts), edit that repo and follow its CLAUDE.md release workflow — never restyle docs in `website/src`. This site owns only content + `website/docs.config.mjs` (sidebar icons via each item's `attrs['data-icon']`).
- **Keep the docs up to date in the same change.** When user-facing behavior changes or is added — public API options/hooks, session semantics, limits, CLI/env knobs, architecture — update the matching pages under `website/src/content/docs/` as part of that change, without waiting to be asked. Architecture reference docs live in `website/src/content/docs/docs/architecture/` (surfaced in `website/docs.config.mjs` under Reference → Advanced → Architecture) and are the canonical human-facing architecture reference.
- The core quickstart under `examples/quickstart/` and the RivetKit example must stay behaviorally identical.
- Every quickstart change needs a matching automated test in the same change.
- **Docs code blocks MUST embed real example files via `<CodeSnippet>` — never hand-write checked code inline.** All runnable code shown in docs lives as a file under `examples/*` (a real, separately type-checked project) and is embedded at build time, so the rendered code is always the exact code we type-check + ship, and each block auto-links to its source on GitHub. There is NO in-Astro type-checking of code blocks (the old `typecheckCodeBlocks` integration is removed); correctness comes from the example projects' own `tsc`, run by `turbo check-types` (and gated by `scripts/verify-check-types.mjs`, which fails if any package lacks a `check-types` script). Authoring:
  - Embed a whole file: `<CodeSnippet file="examples/quickstart/hello-world/index.ts" />` (repo-relative path). `remarkCodeSnippet` (in `@rivet-dev/docs-theme`) inlines the content; the language is inferred from the extension (override with `lang=`), the tab label is the basename (override with `title=`).
  - Embed only part of a file with `region="name"`, delimiting it in the source with `// docs:start name` … `// docs:end name` (markers are stripped, the region is dedented).
  - `<CodeSnippet>` is the ONLY embed API. A bare ```` ```ts file="server.ts" ```` fence (no slash) is just a CodeGroup tab label, not an embed.
  - Paths resolve from the repo root (override via `DOCS_EMBED_ROOT`). If the referenced code doesn't exist yet, add it as a proper example under `examples/*` (its own `package.json` with a `check-types` script + `tsconfig`, and a workspace-matched path) rather than inlining unchecked code.
  - Non-runnable snippets (shell commands, config fragments, illustrative pseudo-code) may stay inline — the rule is about code that should compile.
  - This convention is owned by the shared `@rivet-dev/docs-theme` and applies to every site built on it (agent-os AND secure-exec).
- The docs live in this repo (`website/`); no confirmation is needed to edit them. Validate docs changes with `pnpm --dir website build` before pushing.
- Keep `website/src/data/registry.ts` current when package names or registry entries change.

## Testing

- Auto-skip expensive resource-saturation tests. A test that proves the *absence* of a bound by actually saturating a resource — a JS/WASM infinite loop pinning a CPU core for the watchdog window, a heap/alloc bomb, a fork bomb, or anything that aborts the process — must be marked `#[ignore = "expensive: <resource> saturation; run with --ignored"]` (vitest: `it.skip` or an env gate). These pin cores or crash the runner and bog down normal runs.
- Still test the expensive safeguards. A configured limit/watchdog/quota actually firing — CPU-time limit set → runaway terminated; WASM fuel set → exit 124; heap cap → bounded; fd/process/socket cap → denied — is bounded and fast because the safeguard ends it. Keep these in the default suite; they are the regression guard that the protection works.
- Rule of thumb: if the test ends only when a timeout/watchdog whose *absence* you are documenting fires (slow, unbounded) → `#[ignore]`. If it ends because a *safeguard* fires (fast, bounded) → keep it running.

## Version Control

- **Commit titles and PR titles are pure conventional commits** (`feat`, `fix`, `chore`, `docs`, `refactor`, etc.) with an optional scope, e.g. `fix(sidecar): handle empty ack batch`. Never indicate that a change was written by a coding agent: no model name, no agent name, no `[SLOP(...)]` prefix, and no `Co-Authored-By:` or `Generated with` trailer. The title must read exactly as a human-authored conventional commit.
- **PR descriptions are a simple, high-level bullet list of what changed.** One bullet per meaningful change in plain language. No per-file or line-by-line detail, no implementation narration, and no mention of an agent.

## Agent Working Directory

All agent working files live user-scoped in `~/.agents/`, never inside the repo. Override the location with the `AGENTS_DIR` env var. These files are not committed; `.agent/` is gitignored as a safety net.

- **Specs**: `~/.agents/specs/` — design specs and interface definitions for planned work.
- **Research**: `~/.agents/research/` — research documents on external systems, prior art, and design analysis.
- **Todo**: `~/.agents/todo/*.md` — deferred work items with context on what needs to be done and why.
- **Notes**: `~/.agents/notes/` — general notes and tracking.
- **Benchmarks**: `~/.agents/benchmarks/` — benchmark result artifacts.

When the user asks to track something in a note, store it in `~/.agents/notes/` by default. When something is identified as "do later", add it to `~/.agents/todo/`. Design documents and interface specs go in `~/.agents/specs/`.
