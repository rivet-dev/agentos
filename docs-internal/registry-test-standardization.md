# Spec: Registry Test Standardization

Status: proposal · Owner: registry · Last updated: 2026-07-07

## Problem

Every registry integration test lives under `registry/tests/`, organized by
**where the WASM binaries are built (`registry/native/`), not by what is being
tested.** The harness (`registry/tests/helpers.ts`) resolves command binaries at
`registry/tests/../native/...`, so anything needing a real WASM command was
parked next to the native build — including tests that have nothing to do with
registry packages (npm e2e, VFS, sockets, libc conformance).

That layout coupling is already obsolete:

- `packages/runtime-core/scripts/copy-wasm-commands.mjs` vendors the binaries
  into `packages/runtime-core/commands/`, and CI stages them there.
- The harness already supports an env seam:
  `AGENTOS_WASM_COMMANDS_DIR` / `AGENTOS_C_WASM_COMMANDS_DIR`.
- `packages/runtime-core/` already has its own `tests/` + `vitest.config.ts`.

Consequences today: the `agentos-registry` test package is **not a pnpm
workspace member**, so root `pnpm test` (turbo, member-filtered) never runs it;
`@xterm/headless` doesn't resolve without a manual symlink; and there is no way
to answer "which packages have no test?" mechanically.

## Goals

1. Organize tests by **subject under test**, not by binary location.
2. Make "does package X have a test?" answerable by `ls` + a coverage gate.
3. Re-attach the suites to CI (turbo) so they actually run.
4. **Report test status; never silently fix or hide failures.** Migration must
   not touch package behavior. A package whose test fails moves as-is and is
   recorded as `failing` — see [Non-goals](#non-goals).

## Non-goals

- **Do not fix failing packages.** Moving a test must not change the command's
  behavior to make a red test green. If a test fails after moving, its status is
  `failing` and it is tracked in the manifest, not patched.
- No new test coverage is *required* by the migration itself. Writing the
  missing tests (the `no-test` rows) is follow-up work, tracked but separate.

## Target layout

| Home | Contains | Rationale |
|---|---|---|
| `registry/software/<pkg>/test/` | Tests exercising **one package's command** | The package owns its behavior |
| `packages/runtime-core/tests/integration/` | Tests of the **VM/kernel** using commands as generic fixtures | runtime-core already vendors the binaries and has a test setup |
| `registry/native/tests/` | **libc/sysroot conformance** — validates the C toolchain | Lives next to the artifact it validates |
| `packages/vm-test-harness/` (new, workspace member) | `helpers.ts` + `terminal-harness.ts` shared harness | One import surface; fixes `@xterm/headless` resolution and turbo wiring |

Binaries are injected via `AGENTOS_WASM_COMMANDS_DIR` pointing at
`packages/runtime-core/commands` — no relative-path coupling to `registry/native`.

## The rule (enforced)

> Every `*.test.ts` is either **owned by exactly one package**
> (`software/<pkg>/test/`) **or** lives in a runtime/toolchain home with a
> one-line reason header. No loose files. No test organized by binary location.

CI coverage gate (`registry/scripts/check-test-coverage.mjs`):
- Fail if any `software/*` has neither a `test/` dir nor an entry in the
  **known-gap allowlist**.
- Allowlist (no direct test expected): meta bundles `common`,
  `build-essential`, `everything`; external wrappers `browserbase`, `vix`.

## Status reporter (`registry/scripts/test-status.mjs`)

A report-only tool. It **does not fix anything** and its default exit code does
not fail on known-tracked failures.

Behavior:
1. Enumerate `software/*` + `agent/*`.
2. For each, resolve its test dir (post-migration) and run it via vitest with
   binaries from `runtime-core/commands`.
3. Classify each package into one status (below) and print a table + write
   `registry/test-status.json`.
4. Reconcile against the **expected-status manifest** in this spec:
   - A package whose result matches its recorded status → OK.
   - A `failing`/`not-compiling` package that is still failing → **reported, not
     an error** (exit 0). This is the "don't fix broken packages" contract.
   - A package that **regressed** (recorded `working`, now failing) → exit
     non-zero. Only *new* breakage fails CI.

Status values:

| Status | Meaning |
|---|---|
| `working` | Live test, all (or all-but-hardening) pass |
| `failing` | Live test, real functional failures — tracked, not fixed |
| `disabled` | Test file exists but is `describe.skip` |
| `no-test` | Compiles, no test exists |
| `not-compiling` | Build fails |
| `not-buildable` | Needs an external artifact absent from this checkout |
| `meta` | Aggregate bundle — no direct test |

## Migration & coverage manifest

Statuses below are from the 2026-07-07 audit (full run:
8 failed / 6 passed / 9 skipped files; 34 failed / 84 passed / 59 skipped tests).
The reporter regenerates them; they are the baseline the reporter reconciles
against.

### Software packages (29)

| Package | Command(s) | Current test | Action | Target | Status |
|---|---|---|---|---|---|
| coreutils | `sh` +80 | `shell-terminal`, `shell-redirect` | move | `software/coreutils/test/` | `working` |
| fd | `fd` | `fd-find` (shared) | **split** + move | `software/fd/test/` | `working` |
| findutils | `find`,`xargs` | `fd-find` (shared) | **split** + move | `software/findutils/test/` | `working` |
| tree | `tree` | `kernel/tree-test` (**misfiled**) | move | `software/tree/test/` | `working` |
| sqlite3 | `sqlite3` | `sqlite3` | move | `software/sqlite3/test/` | `failing` (1/16: VFS `pwrite`) |
| zip | `zip` | `zip-unzip` (shared) | **split** + move | `software/zip/test/` | `failing` (3/8 hardening) |
| unzip | `unzip` | `zip-unzip` (shared) | **split** + move | `software/unzip/test/` | `failing` (3/8 hardening) |
| curl | `curl` | `curl` | move | `software/curl/test/` | `failing` (24/30; exits 1 incl. `--version`) |
| git | `git` | `git` (`describe.skip`) | move + unskip via `describeIf` | `software/git/test/` | `disabled` (compiles) |
| duckdb | `duckdb` | `duckdb` (`describe.skip`) | move + `describeIf` | `software/duckdb/test/` | `disabled` (compiles) |
| wget | `wget` | `wget` (`describe.skip`) | move + `describeIf` | `software/wget/test/` | `not-compiling` (dup `getpeername`) |
| codex-cli | `codex`,`codex-exec` | `codex-exec`,`codex-tui` (`describe.skip`) | move + `describeIf` | `software/codex-cli/test/` | `not-buildable` (needs codex-rs fork) |
| gawk | `awk` | — | **write new** | `software/gawk/test/` | `no-test` |
| sed | `sed` | — | **write new** | `software/sed/test/` | `no-test` |
| grep | `grep` | — (only a vehicle in `dynamic-module`) | **write new** | `software/grep/test/` | `no-test` |
| tar | `tar` | — | **write new** | `software/tar/test/` | `no-test` |
| gzip | `gzip` | — | **write new** | `software/gzip/test/` | `no-test` |
| jq | `jq` | — | **write new** | `software/jq/test/` | `no-test` |
| ripgrep | `rg` | — | **write new** | `software/ripgrep/test/` | `no-test` |
| yq | `yq` | — | **write new** | `software/yq/test/` | `no-test` |
| diffutils | `diff` | — | **write new** | `software/diffutils/test/` | `no-test` |
| file | `file` | — | **write new** | `software/file/test/` | `no-test` |
| http-get | `http_get` | — | **write new** | `software/http-get/test/` | `no-test` |
| vim | `vim` | — | **write new** | `software/vim/test/` | `no-test` (compile unverified; heavy `make cmd/vim`) |
| vix | `vix` | — | allowlist | — | `not-buildable` (external, no source) |
| browserbase | (`browse`) | — | allowlist | — | `no-test` (external CLI wrapper) |
| common | (bundle) | — | allowlist | — | `meta` |
| build-essential | (bundle) | — | allowlist | — | `meta` |
| everything | (bundle) | — | allowlist | — | `meta` |

### Agent packages (5)

All are JS ACP adapters with **no integration test**. Decision needed: adapter
smoke tests, or explicitly out of scope (allowlist).

| Package | Action | Status |
|---|---|---|
| claude | write new / allowlist | `no-test` |
| codex | write new / allowlist | `no-test` |
| opencode | write new / allowlist | `no-test` |
| pi | write new / allowlist | `no-test` |
| pi-cli | write new / allowlist | `no-test` |

### Not-a-package tests → relocate out of `registry/tests/`

**→ `packages/runtime-core/tests/integration/`** (VM/kernel):
- from `wasmvm/`: `net-server`, `net-udp`, `net-unix`, `signal-handler`,
  `wasi-http`, `wasi-spawn`, `dynamic-module-integration`
- **all of `kernel/`** except `tree-test`: `e2e-npm-*`, `e2e-nextjs-build`,
  `e2e-concurrently`, `e2e-npx-and-pipes`, `e2e-project-matrix`,
  `cross-runtime-*`, `bridge-child-process`, `ctrl-c-shell-behavior`,
  `dispose-behavior`, `error-propagation`, `exec-integration`,
  `fd-inheritance`, `module-resolution`, `node-binary-behavior`,
  `shim-streaming`, `signal-forwarding`, `vfs-consistency`
- `registry/tests/projects/` (~45 npm fixtures) — move with the kernel e2e

**→ `registry/native/tests/`** (toolchain/libc conformance):
- `os-test-conformance`, `libc-test-conformance`, `c-parity`,
  `ci-artifact-availability`, `kernel/ci-wasm-artifact-availability`

**Decision:** `envsubst` — command has **no owning package**. Either give it one
(coreutils?) or move `envsubst.test.ts` to runtime. Currently `working` (6/6).

## Migration phases

1. **Foundation** — extract `packages/vm-test-harness`; make it a workspace
   member; point it at `runtime-core/commands` via env. Wire turbo `test`.
   (Fixes xterm + CI-orphan bugs; moves nothing yet.)
2. **Relocate runtime + toolchain tests** out of `registry/tests/` per the lists
   above. Pure `git mv`, no behavior change.
3. **Co-locate package tests** into `software/<pkg>/test/`; split the two shared
   files (`fd-find`, `zip-unzip`); move misfiled `tree-test`; swap
   `describe.skip` → `describeIf(binaryPresent)`.
4. **Gate** — add `check-test-coverage.mjs` + `test-status.mjs`; commit the
   baseline `test-status.json`; fail CI only on regressions.
5. **Backfill** — write the `no-test` suites (12 software + 5 agents). Separate
   PRs, tracked by the manifest.

## Open decisions

- Runtime tests home: **`runtime-core/tests/`** (recommended, binaries already
  there) vs. a new `packages/vm-integration-tests`.
- `envsubst` owner.
- Agent adapter tests: in scope or allowlist.
- `describe.skip` → `describeIf`: gate on binary presence, or also on
  network/env (git remote clone, codex `OPENAI_API_KEY`).
