# agentOS

Agent OS is the agent-facing wrapper around secure-exec. It provides ACP sessions, agent adapters, quickstarts, and the public AgentOs client APIs while depending on secure-exec for the generic VM runtime.

## Boundaries

- Local Agent OS development dependencies on secure-exec must point to `../secure-exec`.
- Keep generic runtime, kernel, VFS, language execution, and registry software behavior in secure-exec.
- Agent OS owns ACP, sessions, agent adapters, toolkit semantics, quickstarts, and the AgentOs facade.
- Call OS instances VMs, never sandboxes.
- The protocol has no backwards compatibility. Clients and the sidecar ship in same-version lockstep, so never add protocol or config versioning, runtime negotiation, fallbacks, or converters. Configs such as `CreateVmConfig` carry no `version` field; the single same-version wire handshake is the only version check. Change the protocol freely and update both sides together.

## Security Model

Trust model (decide which side of the boundary something is on before judging whether it is a security bug). Three components:

- **Client** (trusted, *except for anything it submits for execution*). The AgentOs client / wire caller. The client and every value it configures are trusted: `CreateVmConfig`, mount descriptors and plugin configs (host_dir paths, S3 endpoints/credentials, Google Drive, sandbox-agent), the permission policy, network allowlist, resource limits, env, and DNS overrides. Configuration is **not** an attack surface. The only untrusted thing the client supplies is the code/payload it asks to run, because that runs in the executor.
- **Sidecar** (trusted; the TCB and enforcement point). The agent-os sidecar embeds and extends secure-exec; it brokers client requests and owns the kernel, VFS, mounts/plugins, socket table, and permission policy, and enforces the boundary against the executor.
- **Executor** — V8 isolates or WASM (untrusted; the adversary). Runs guest JS/Python/WASM plus any third-party/npm/agent-generated code. Assume it is actively hostile; how code reached the executor never makes it trusted.

**The security boundary is sidecar ↔ executor.** A defect that requires the client to supply a malicious config/endpoint/credential/policy is NOT a sandbox vulnerability (the client configures its own VM and already controls the host). Treat such hardening as defense-in-depth, not as an escape, and do not add validation that only guards trusted client-provided configuration. Corollaries: the permission policy/limits are trusted input but the guest is the subject they bind, so a guest *bypassing* an applied rule is in-scope; a host-backed mount's target/credentials are trusted, but confining the guest's I/O *through* it (symlink / `..` / TOCTOU escapes) is in-scope. The wire transport is single-client over stdio, so wire authn/authz-between-clients and VM-to-VM-via-forged-id concerns are out of scope until a multi-client transport exists. See secure-exec root `CLAUDE.md` → Trust Model for the canonical statement.

- Isolation is layered (defense in depth), like Cloudflare Workers. Untrusted guest code is isolated *within* the host process by V8/WASM virtualization today; host-level jailing (sandboxing the process itself) is a planned additional layer. Because the in-process layer is load-bearing: keep the embedded V8 patched to current security releases, and never let one isolate take down the shared process — a per-isolate failure (heap OOM, CPU runaway) must terminate that isolate, not abort the host process.
- Match Cloudflare Workers wherever it makes sense. Use Workers' published behavior as the reference point for isolation semantics, resource limits, and egress defaults — e.g. ~128 MiB memory per isolate, bounded CPU time, default-deny network egress. Resource limits must be bounded by default (never `None`/0 for memory, heap, stack, or CPU time); operators may raise them.

## Agent Sessions

- Every public method on `packages/core/src/agent-os.ts` must stay mirrored by RivetKit actor actions after the user confirms the Rivet repo path.
- Subscription methods are delivered through actor events; lifecycle behavior belongs in actor sleep/destroy hooks.
- Agent adapters must use real upstream agent SDKs. Do not replace SDK adapters with direct API-call stubs.
- Host-native agent wrappers are not allowed; agents run through the VM runtime supplied by secure-exec.

## Extension Authoring

- Agent OS extension payloads use the secure-exec `Ext` envelope with Agent OS-owned namespaces and generated ACP payloads.
- Keep ACP decoding and session state in Agent OS wrapper code, not in secure-exec core sidecar code.
- The agent-os sidecar wrapper embeds and extends secure-exec; secure-exec must remain free of ACP, agent, and session dependencies.

## Website And Docs

- The Agent OS website and docs live in `website/` (Astro + Starlight) and deploy to `agentos-sdk.dev` (docs at `agentos-sdk.dev/docs`). The marketing pages and docs were migrated out of `rivet.dev/agent-os` and `rivet.dev/docs/agent-os`, which now 301-redirect to this domain.
- Docs styling is owned by the shared **`@rivet-dev/docs-theme`** repo (`github.com/rivet-dev/docs-theme`), consumed via `github:rivet-dev/docs-theme#<tag>` and wired in via `...docsTheme(starlight, siteConfig)`. To change any docs styling (palette, header, sidebar, code blocks, fonts), edit that repo and follow its CLAUDE.md release workflow — never restyle docs in `website/src`. This site owns only content + `website/docs.config.mjs` (sidebar icons via each item's `attrs['data-icon']`).
- The core quickstart under `examples/quickstart/` and the RivetKit example must stay behaviorally identical.
- Every quickstart change needs a matching automated test in the same change.
- Confirm the docs repo path with the user before editing Agent OS docs.
- Keep `website/src/data/registry.ts` current when package names or registry entries change.

## Testing

- Auto-skip expensive resource-saturation tests. A test that proves the *absence* of a bound by actually saturating a resource — a JS/WASM infinite loop pinning a CPU core for the watchdog window, a heap/alloc bomb, a fork bomb, or anything that aborts the process — must be marked `#[ignore = "expensive: <resource> saturation; run with --ignored"]` (vitest: `it.skip` or an env gate). These pin cores or crash the runner and bog down normal runs.
- Still test the expensive safeguards. A configured limit/watchdog/quota actually firing — CPU-time limit set → runaway terminated; WASM fuel set → exit 124; heap cap → bounded; fd/process/socket cap → denied — is bounded and fast because the safeguard ends it. Keep these in the default suite; they are the regression guard that the protection works.
- Rule of thumb: if the test ends only when a timeout/watchdog whose *absence* you are documenting fires (slow, unbounded) → `#[ignore]`. If it ends because a *safeguard* fires (fast, bounded) → keep it running.

## Agent Working Directory

All agent working files live user-scoped in `~/.agents/`, never inside the repo. Override the location with the `AGENTS_DIR` env var. These files are not committed; `.agent/` is gitignored as a safety net.

- **Specs**: `~/.agents/specs/` — design specs and interface definitions for planned work.
- **Research**: `~/.agents/research/` — research documents on external systems, prior art, and design analysis.
- **Todo**: `~/.agents/todo/*.md` — deferred work items with context on what needs to be done and why.
- **Notes**: `~/.agents/notes/` — general notes and tracking.
- **Benchmarks**: `~/.agents/benchmarks/` — benchmark result artifacts.

When the user asks to track something in a note, store it in `~/.agents/notes/` by default. When something is identified as "do later", add it to `~/.agents/todo/`. Design documents and interface specs go in `~/.agents/specs/`.
