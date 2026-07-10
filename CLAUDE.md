# agentOS

AgentOS owns the runtime, kernel, VFS, language execution, registry packages,
ACP/session layer, AgentOS client APIs, docs, and publish machinery. The
`secure-exec` repository is now a generated compatibility mirror only.

## Boundaries

- Keep AgentOS product versions pinned at `0.0.1` in committed files. Release
  workflows apply real versions transiently with `scripts/publish`; never commit
  release-version rewrites.
- Call guest environments VMs, not sandboxes, except when referring to a package
  or public API that already uses the word.
- The protocol has no backward compatibility guarantee. Client, sidecar, and
  protocol crates ship in same-version lockstep; update both sides together.
- Generic runtime work belongs here, not in `../secure-exec`. Regenerate that
  mirror with `node scripts/generate-secure-exec-mirror.mjs` after changing a
  shimmed public surface.
- Keep root `package.json` scripts limited to Turbo orchestration; repo-specific
  commands belong in `justfile` recipes or scoped package scripts.

## Security Model

Trust model:

- **Client**: trusted, except for code/payloads it submits for execution.
- **Sidecar/runtime**: trusted enforcement point. It owns the kernel, VFS,
  mounts/plugins, socket table, permissions, and resource policy.
- **Executor**: untrusted V8 isolate or WASM guest. Assume guest JS/Python/WASM
  and third-party packages are hostile.

The security boundary is sidecar/runtime to executor. Client-provided config is
trusted input; a guest bypassing an applied policy is in scope, while a client
choosing dangerous credentials, endpoints, mounts, or allowlists is not a
runtime escape.

Every limit, timeout, queue, buffer, and per-entity collection must be bounded
by default, warn near threshold, and fail with a typed error that names the
limit and how to raise it. Host-visible warnings/errors must reach stderr/log
or structured trace paths, not stay trapped in the VM.

Never swallow errors silently. Every failure must either propagate as a hard,
typed error to the caller (preferred) or be clearly logged at the failure site;
empty `catch`/`let _ =` on fallible operations and fire-and-forget promises
that drop rejections are bugs, not defensive coding. For guest-visible
surfaces, prefer matching Linux behavior — the correct POSIX errno delivered to
the guest — over inventing a softer fallback that hides the failure.

## Runtime And Registry

- The projected `/opt/agentos` filesystem is the source of truth for software
  and agent resolution. Read it live; do not cache package lists captured at VM
  configuration time.
- Packages are packed `.aospkg` files (`crates/vfs/package-format/v1.bare`:
  header + vbare manifest + mount index + mount tar) projected under
  `/opt/agentos/pkgs/<name>/<version>`; commands are linked under
  `/opt/agentos/bin/`. The vbare chunk1 manifest is the only runtime manifest —
  `agentos-package.json` is toolchain input, stripped at pack time and never
  shipped or materialized into the guest.
- Agent resolution and enumeration are sidecar-owned. Clients send agent names
  and forward a single package `path` (the `.aospkg`, or a transition dir);
  they do not scan `node_modules` or parse adapter manifests for discovery.
- TypeScript and Rust clients must stay behaviorally identical. Any public
  method or wire behavior change in one client must be mirrored in the other.
- Agent adapters must use real upstream SDKs. Do not replace SDK adapters with
  direct API-call stubs.

## Software Build (WASM Toolchain)

Registry software is **real upstream Linux software** (GNU coreutils, grep, sed,
gawk, real curl/sqlite/duckdb/vim, …) compiled to `wasm32-wasip1` against a
**sysroot we fully own** — a patched Rust std + libc whose gaps are filled by
custom host-syscall imports. Treat that target as **native POSIX**;
`wasm32-wasip1` is an implementation detail, not a feature ceiling.

- **We do not depend on stock WASI / wasi-libc.** The sysroot is ours. A missing
  libc/POSIX API (`getrlimit`/`RLIMIT_NOFILE`, `getgroups`, spawn, fd dup, …) is
  never a blocker — implement it (real, or a sane stub) in the patched
  std/libc/host-import layer. "WASI doesn't have X" is not a reason to stop; X is
  ours to add.
- **Fix portability one layer down, in the sysroot** — a new std/libc patch or a
  new host import — not with `cfg(target_*)` branches or shims in the tool's own
  source. A WASM-specific branch in application code usually means the fix
  belongs in the libc layer.
- **Patch the real upstream tool only as a fallback**, when the fix genuinely
  cannot live in the sysroot. Patching the real tool is allowed; reimplementing
  it is not.
- **"NOT POSSIBLE" is reserved for genuine impossibility** after exhausting both
  sysroot patches and tool patches — never for a missing syscall we could
  implement. Document the specific wall if you claim it.
- **Working in `software/`, you may (and should) fix the layer underneath.** When
  a package behaves differently from real Linux, the root cause is usually not the
  package — it's the runtime. It is in-scope and expected to fix the underlying
  implementation: the Node-compat / bridge layer, the WASM execution runtime, the
  kernel/VFS syscalls, or the patched sysroot/libc. Do **not** paper over a
  Linux-deviating behavior in the package, its wrapper, or its test — chase it
  down into whichever runtime layer owns it and make that layer match Linux.
- **Cite the authoritative spec at the implementation site.** When you implement
  against an external spec or wire/file format — `/proc`, network protocols,
  syscall ABIs, on-disk layouts, TLS/crypto — put the reference **in a code
  comment right where the format is emitted or parsed**: the man-page section
  (e.g. `proc(5)`), the kernel source path (e.g. `fs/proc/array.c`), the RFC
  (e.g. RFC 4253 for SSH transport), and/or the consumer's parser we must satisfy
  (e.g. procps-ng `readproc.c`). A format emitter or protocol handler without a
  doc link is incomplete. Conformance tests should name the captured real-Linux
  fixture they validate against, so the chain spec → impl → test is explicit.

## Publishing

- `scripts/publish` is the source of truth for npm/crates discovery, version
  rewriting, npm publish, crates publish, release assets, and R2 upload.
- Publishable npm packages and Rust crates are AgentOS-owned. Compatibility
  `@secure-exec/*`, `secure-exec`, and `secure-exec-*` artifacts are emitted
  from the generated mirror.
- The release workflow must build and stage the native sidecar binaries,
  runtime-sidecar binaries, registry WASM commands, and pyodide assets before
  publish.
- `scripts/verify-fixed-versions.mjs` must pass in the committed tree.

## Docs

- The AgentOS website lives in `website/` and deploys to `agentos-sdk.dev`.
- Keep docs current in the same change as user-facing behavior: public APIs,
  runtime options, env knobs, limits, architecture, and package names.
- Runnable docs code must come from real checked example files via the docs
  theme `<CodeSnippet>` mechanism. Inline code is fine only for shell commands,
  config fragments, or non-runnable examples.
- Validate docs changes with `pnpm --dir website build` when the site changes.

## Tests

- Cheap gates for normal changes: `cargo check --workspace`, `pnpm build`,
  `pnpm check-types`, publish helper checks, changed script syntax checks, and
  workflow YAML parsing.
- Expensive runtime suites, cross-repo dispatches, real publish workflows,
  benchmarks, protocol fixture regeneration, and end-to-end sanity runs belong
  in the explicit expensive validation phase.
- Tests that prove absence of a bound by saturating CPU, heap, fd/process/socket
  limits, or watchdog timeouts must be ignored/skipped by default with a clear
  reason. Fast tests where the configured safeguard fires should stay in the
  default suite.

## Version Control

- Commit and PR titles are plain conventional commits with no coding-agent
  attribution.
- PR descriptions should be a short high-level bullet list. Avoid per-file
  narration and generated-by language.
