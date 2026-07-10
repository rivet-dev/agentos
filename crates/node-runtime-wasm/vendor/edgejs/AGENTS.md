# EdgeJS N-API QuickJS Notes

This repo contains an N-API implementation backed by QuickJS. Most of the active
work has been in the `napi` submodule/worktree, especially:

- `napi/quickjs/src/js_native_api_quickjs.cc`
- `napi/quickjs/src/unofficial_napi.cc`
- `napi/quickjs/src/internal/`
- `napi/quickjs/tests/`
- `napi/tests/js-native-api/`

## QuickJS WASIX Resume Context

When resuming this work, start with:

```text
plans/quickjs-wasm/development/README.md
```

That file indexes the development phases:

- `001_merge_analysis.md`: comparison with the other QuickJS branch and integration plan.
- `002_native_bootstrap_contextify.md`: native Edge QuickJS bootstrap and `ContextifyScript` fix.
- `003_repl_tty_readline.md`: REPL TTY/readline troubleshooting.
- `004_promise_hooks_microtasks.md`: QuickJS promise hooks and microtask/job draining.
- `005_wasix_wasmer_http.md`: WASIX/Wasmer bootstrap, Atomics, and HTTP stream listener fix.
- `006_framework_app_adapters.md`: Astro, Vite, and Next.js app adapter notes.
- `007_framework_standalone_builds.md`: framework standalone build notes and remaining runtime
  blockers.
- `008_runtime_change_containment_rollback.md`: shared runtime rollback containment, native
  compatibility relocation, and QuickJS WASIX build/linkage notes.

Current useful state:

- Native QuickJS-backed Edge CLI can bootstrap and run the HTTP echo server.
- REPL input works with persistent history after the promise hook/microtask fix.
- WASIX Edge QuickJS can run under Wasmer and handle HTTP requests with `--net`.
- `quickjs-wasm/build.sh` currently builds `build-quickjs-wasix/edge.wasm` and
  `edgejs.wasm`, and its final no-N-API-imports check passes.
- The root `wasmer.toml` publishes/uses `sadhbh-c0d3/edgejs-quickjs` at
  `0.0.1`, module `edge`, source `build-quickjs-wasix/edgejs.wasm`.
- Framework app notes use anonymized paths: `~/src/astro-app`,
  `~/src/vite-app`, and `~/src/next-app`.

## Plans Documentation Workflow

Before starting a new task, always list the plan tree recursively and look for
existing information:

```sh
find /Users/sadhbh/src/dev/edgejs/plans -type f -print
rg -n "<relevant terms>" /Users/sadhbh/src/dev/edgejs/plans
```

Read the relevant existing plan, development note, or troubleshooting note
before changing code. While working, keep existing information current: if the
task discovers new facts about an existing topic, update the existing note
instead of creating a duplicate.

If the context window reaches 90% while work is in progress, create a new
development task note under:

```text
plans/quickjs-wasm/development/NNN_<meaningful_name>.md
```

Include all information needed to continue the current task: user requests,
review comments being addressed, files changed, verification already run,
known failures, and the next concrete steps.

### Generate PDF Documentation

When asked to "Generate PDF documentation", build a polished white-paper/book
PDF from `plans/quickjs-wasm` and all of its subdirectories.

Use this process:

1. List the plan tree recursively and search the plans for relevant context
   before generating the document.
2. Generate temporary Markdown and LaTeX under `/private/tmp`, leaving the
   source plan notes untouched.
3. Organize the book by knowledge structure, not raw file order: program
   definition, chronological development narrative, cleanup/containment
   subtasks, troubleshooting registry, Astro SSR, Vite app, Next.js, and Wasmer
   deploy/WASIX packaging.
4. Preserve all source-note information as chapters or chapter sections, and
   include source paths for traceability.
5. Use the title `EdgeJS QuickJS WASIX`, author
   `Sonia Sadhbh Kolasinska in collaboration with Christoph Herzog, Wasmer`,
   the current date, and an abstract.
6. Render through temporary LaTeX with Pandoc and XeLaTeX, rerunning XeLaTeX as
   needed for the table of contents.
7. Preserve literal tilde characters in paths and code examples; do not rewrite
   `~` as math such as `$\sim$`.
8. Write the final PDF into `plans/quickjs-wasm/`.

## Experimental Rules

### Experimental 001: Parallel Development Subtasks

For larger development work, split the task into a development task directory:

```text
plans/quickjs-wasm/development/dev_<number>_<meaningful-name>/<subtask-number>_<meaningful-name>.md
```

Each subtask note should record scope, dependencies, write ownership, status,
verification expectations, and enough context for an independent worker to
continue safely. Spawn workers intelligently based on dependency order: only run
parallel workers for subtasks with disjoint write sets or read-only checks, and
make each worker aware that others may be active in the same codebase.

Use this heuristic when deciding where documentation belongs:

- Development task: broad implementation progress, integration work, runtime
  design, refactors, or milestone notes. Write or update a numbered development
  note under:

```text
plans/quickjs-wasm/development/NNN_<meaningful_name>.md
```

- Troubleshooting issue: an observed failure, crash, regression, compatibility
  gap, or focused diagnostic trail. Write or update a numbered issue note under
  the app-specific troubleshooting directory:

```text
plans/quickjs-wasm/development/troubleshooting/astro-ssr/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/vite-app/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/next-app/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/node-test/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/node-compat/napi/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/node-compat/edgejs/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/node-compat/deploy/NNN_<issue_name>.md
plans/quickjs-wasm/development/troubleshooting/wasmer-deploy/NNN_<issue_name>.md
```

Choose `astro-ssr`, `vite-app`, `next-app`, or `wasmer-deploy` based on the app
or deployment path where the issue is reproduced. Use `node-compat/napi`,
`node-compat/edgejs`, or `node-compat/deploy` when the issue is primarily a
shared Node compatibility adaptation owned by the QuickJS N-API layer, EdgeJS
runtime source, or deployment/package layout. If a failure affects shared
QuickJS runtime behavior, still file the troubleshooting note under the app or
deploy path that exposed it, then cross-reference any shared development note it
updates.

For each new troubleshooting issue, write the action plan before changing code.
Each issue note must include a Jira-style metadata table immediately after the
title:

```md
| | | Remarks |
| --- | --- | --- |
| **Status** | ▶️ | Planned investigation or active work. |
| **Severity** | High | Blocks the app unless fixed or worked around. |
```

Use these status icons consistently:

- `▶️`: open or in progress.
- `🟢`: done.
- `🟠`: done with known issues, caveats, or incomplete compatibility.
- `🔴`: unresolved blocker.

Set severity from the impact observed during investigation:

- `High`: must be fixed or worked around, otherwise the target app does not
  work.
- `Medium`: must be fixed in some way to enable the app, but the current fix may
  be incomplete or inaccurate.
- `Low`: known issue or documentation/runtime caveat that does not make the app
  break significantly.

After creating or renaming a note, update the troubleshooting registry and the
most recent note pointer in this `AGENTS.md` section. The troubleshooting
registry heading must include the same status icon before the linked filename,
for example:

```md
### 🟢 [002_depd_callsite_methods.md](astro-ssr/002_depd_callsite_methods.md): depd CallSite method compatibility
```

```text
plans/quickjs-wasm/development/troubleshooting/README.md
```

Most recent Astro SSR troubleshooting plan:

```text
plans/quickjs-wasm/development/troubleshooting/astro-ssr/014_pnpm_deploy_externalized_runtime_links.md
```

Most recent Vite app troubleshooting note:

```text
plans/quickjs-wasm/development/troubleshooting/vite-app/001_standalone_build.md
```

Most recent Next app troubleshooting note:

```text
plans/quickjs-wasm/development/troubleshooting/next-app/005_entry_css_work_store_async_context.md
```

Most recent Node test troubleshooting note:

```text
plans/quickjs-wasm/development/troubleshooting/node-test/018_tls_securecontext_sni_lifetime.md
```

Most recent Node compatibility troubleshooting note:

```text
plans/quickjs-wasm/development/troubleshooting/node-compat/edgejs/012_v8_ctest_environment_attach.md
```

Most recent Wasmer deploy troubleshooting note:

```text
plans/quickjs-wasm/development/troubleshooting/wasmer-deploy/004_wasix_safe_mode_https_exit.md
```

Important commands:

```sh
make build-edge-quickjs-cli JOBS=4
cmake --build build-edge-quickjs-cli --target edge -j4
cd /Users/sadhbh/src/dev/edgejs/quickjs-wasm/ && ./build.sh
wasmer package build --check .
wasmer run --net .
```

When working on WASIX-impacting changes under `src/`, `lib/`, or
`napi/quickjs/`, use the `cd /Users/sadhbh/src/dev/edgejs/quickjs-wasm/ &&
./build.sh` form for the rebuild.

For Linux-only WASIX failures, use Docker from macOS to reproduce the Linux
environment. Before starting Docker-based troubleshooting, remind Sadhbh to
launch the Docker daemon. Check the host's native architecture first; on
Sadhbh's Apple Silicon machine that means native Linux aarch64. Prefer
`ubuntu:latest` containers for the native architecture before forcing
`--platform linux/amd64`; amd64 emulation is much slower. The May 7, 2026
`build-wasix-linux` safe-mode HTTPS failure was investigated by building inside
native arm64 `ubuntu:latest` with the aarch64 `wasixcc` v0.4.2 release and
sysroot tag `v2026-02-16.1`, then running the final CI-matching safe-mode smoke
suite under Linux amd64 Docker with Wasmer 7.1.0.

For embedded QuickJS WASIX builds, targets that include N-API headers before
linking `napi_quickjs` must compile with `NAPI_EXTERN=`. Without that, wasm
objects can disagree on the import module for unresolved `napi_*` calls
(`napi` versus `env`) and fail at the final `wasm-ld` step.

The N-API test suites are owned by the N-API repository. EdgeJS CI and the root
`Makefile` should not run duplicate native N-API or standalone N-API Cargo test
jobs; keep EdgeJS verification focused on Edge runtime, QuickJS CLI, WASIX, and
package-level smoke tests.

For QuickJS WASIX smoke testing:

```sh
wasmer run . -- --version
wasmer run . -- -e "console.log('hello from quickjs')"
wasmer run --net --volume ./quickjs-wasm:/app . -- /app/echo-server.js
```

Useful diagnostics:

- `EDGE_TRACE_NET=1` traces TCP, stream, HTTP parser, and JS HTTP server paths.
- `EDGE_TRACE_TTY=1` traces native/JS TTY, stream, readline, and REPL history.
- `EDGE_TRACE_BOOTSTRAP=1` traces top-level CLI runner exit status.

Known caveats to remember:

- The vendored QuickJS source has local compatibility patches for promise hooks
  and WASIX atomics. Preserve them unless replacing QuickJS with an upstream
  version that provides equivalent behavior.
- QuickJS N-API class instances should not look like `napi_external`; keep
  `napi_get_value_external(...)` limited to values created by
  `napi_create_external(...)`.
- For Next.js adapters, avoid `export const runtime = 'edge'` on routes that
  `server/generate-next-dynamic-shells.cjs` must import from
  `.next/server/app/.../page.js`.

## Build And Test Workflow

From the repo root `~/src/edgejs`, use Edge runtime targets:

```sh
make build-edge-quickjs-cli JOBS=4
make test-quickjs-only TEST_JOBS=4
make build JOBS=4
make test-only TEST_JOBS=4
```

N-API implementation tests should be added to and run by the N-API repo, not by
EdgeJS root Makefile targets or EdgeJS CI workflows.

## Methodology

When a Node compatibility test fails, first reproduce the exact failing test,
then inspect the crash/failure in LLDB before changing code. Fix one behavior at
a time and rerun the targeted EdgeJS test first, then the relevant Edge runtime
suite. Avoid fixes that make one test pass by changing broad semantics; previous
work often found that narrow QuickJS/V8 semantic differences caused regressions
elsewhere.

Before editing, compare with the V8 backend for intent:

```text
napi/v8/src/
```

Use the V8 implementation as behavioral guidance, but do not copy V8-specific
assumptions into QuickJS. QuickJS often has different ownership, context,
microtask, module, stack-limit, and GC semantics.

## Current QuickJS Design Direction

The QuickJS backend has been refactored toward small internal C++ classes under
`napi/quickjs/src/internal/`. If a new `napi_*__` struct/class is needed, put it
in its own header/source pair there, keep fields encapsulated, and use RAII for
QuickJS handles.

Important local conventions:

- Prefer lower_case_naming_convention for new internal helpers.
- Keep `js_native_api_quickjs.cc` focused on the public `extern "C"` N-API
  functions; move helper logic into `internal/` files.
- `napi_value__` wraps a `JSValue` and owns/free-dups according to how it was
  created. Use scope wrapping helpers instead of raw global wrap/unwrap helpers.
- `napi_ref__`, scopes, callbacks, env cleanup hooks, externals, function
  trampolines, and utility code already have internal files; extend those rather
  than reintroducing large local structs in the public implementation file.
- Do not revert unrelated user changes or broad refactors already present in the
  working tree.

## Unofficial N-API QuickJS Surface

`napi/include/unofficial_napi.h` is broad and V8-shaped. The QuickJS
implementation in `napi/quickjs/src/unofficial_napi.cc` should provide real
QuickJS-backed behavior where the engine supports it, and explicit stable
fallbacks where the API is V8-only.

Implemented/expected QuickJS-backed areas include:

- env creation/release and testing teardown
- env cleanup/destroy callbacks
- low-memory/GC request via `JS_RunGC`
- microtask/job draining via `JS_ExecutePendingJob`
- source-map/error arrow-message helpers used by tests
- contextify make/run/dispose/compile/cache-data helpers
- memory/heap/hash metadata approximations from QuickJS APIs
- structured clone and serialize/deserialize using `JS_WriteObject` /
  `JS_ReadObject` where possible

V8-only areas such as full `module_wrap`, CPU/heap profiling, and precise V8
promise internals should not pretend to be complete. Prefer returning
`napi_generic_failure` or sane empty/default outputs after validating arguments,
so embedders get stable behavior and linkable symbols.

## Known Good Baseline

The EdgeJS root suite should validate the runtime through `make test-only` for
V8 and `make test-quickjs-only` for QuickJS. Native N-API test baselines live in
the N-API repository.
