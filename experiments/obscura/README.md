# Obscura executor embedding experiment

**This is an unfinished integration, not an installed `obscura` VM command.**
It runs the real upstream browser against agentOS's executor host-call
contract. It is deliberately outside the shipping workspace and executor
registry. Do not pass untrusted pages to this experiment.

The upstream source is pinned in [UPSTREAM.json](UPSTREAM.json). The browser,
DOM, network, and JavaScript implementation come from Obscura, under its
Apache-2.0 license. They are not reimplemented here. Deno is MIT-licensed.
`prepare.py` verifies both archive checksums, applies the checked-in patches,
and keeps upstream licenses in the ignored `vendor/` directory.

## Run the fixture

From the repository root, with Python 3.12+, GNU patch, the repository Rust
toolchain, and pnpm installed:

```sh
pnpm install --frozen-lockfile
python3 experiments/obscura/prepare.py
cargo test --locked --manifest-path experiments/obscura/Cargo.toml
```

Preparation downloads pinned source archives; Cargo may download locked
dependencies and V8's build artifact. Tests use deterministic local fixtures
and make no internet requests. `--cache /path/to/cache` selects a reusable
archive cache. Generated source and binaries must not be committed.

[tests/embedding.rs](tests/embedding.rs) is the runnable embedding example:

1. Initialize the existing agentOS V8 platform and process Tokio driver.
2. Run Obscura's non-`Send` futures in a `LocalSet` on the calling thread.
3. Give the browser an `ExecutorBrowserHost` bound to a generation and PID.
4. Dispatch typed host requests through the bounded executor event channel.
5. Navigate virtual HTML, execute its script, and perform a JavaScript fetch.

The fixture dispatcher returns HTML from a virtual path and HTTP responses
for `obscura.gameinc.io`. It proves transport behavior, **not** production VM
permission enforcement: it is not the kernel's production host dispatcher.
That dispatcher must own policy when a real executor is added.

The tests cover virtual file reads, denied files and requests, policy checks
on redirect destinations, JavaScript fetch, payload rejection before admission,
queue saturation, pending-call teardown, and canceled response consumers.
They also compile WASM and run ordinary V8 JavaScript after dropping Obscura.

Validation performed for this revision: the four experiment tests, the existing
`embedded_runtime_session` suite with `obscura-embedding` enabled, the default
`agentos-executor-v8-runtime` check, the embedded-VM dependency boundary check,
and fixed-version verification all pass. The prepared dependency graph contains
one native V8 version, 130.0.7. The full workspace and browser-conformance suites
have not been run.

## Implemented

- Source preparation is pinned and checksum-verified; Cargo locks dependencies.
- `ExecutorBrowserHost` submits `FilesystemOperation::ReadFileAt` and
  `NetworkOperation::HttpRequest`, with bounded payloads and direct replies.
- Browser navigation and JavaScript fetch use the injected authority. Those
  paths fail without it; they do not fall back to reqwest or host file reads.
- File paths remain guest paths. The host adapter does not translate them to
  real host paths. HTTP requests are passed to the sidecar as typed requests.
- An optional `obscura-embedding` feature initializes Deno through the existing
  process-lifetime V8 platform owner and its fixed four-worker platform. Default
  V8 builds and shipping executor selection stay unchanged.

## Compatibility patches

Upstream uses Deno 0.412 and V8 150; agentOS uses V8 130.0.7. Linking both
native V8 libraries produced duplicate C++ symbols. This experiment ports
Obscura to Deno 0.336 and the same V8 130.0.7 used by agentOS. The patch adapts
handle scopes, op annotations, module loading, and per-realm metadata. The
fixture is not comprehensive browser-conformance coverage of that downgrade.

Deno 0.336's async-op scheduler assumes a single-thread Tokio runtime and
marks non-`Send` futures as `Send` under that assumption. JavaScript fetch
aborted under agentOS's shared multithread runtime. The narrowly scoped
`deno-local-ops.patch` uses `tokio::task::spawn_local` instead. The embedding
must provide a `LocalSet`; it must not create a second Tokio runtime or run
guest V8 code on trusted I/O workers. This Cargo override is local to the
experiment.

## Work required before enabling the CLI

- Add a concrete executor adapter and registry dispatch for top-level commands,
  shell/child-process launch, exec replacement, stdin/stdout/stderr, signals,
  pause/resume, generation changes, and VM teardown. No such adapter is included.
- Port the real CLI's command dispatch and I/O. The upstream CLI has native
  process spawning/listeners, storage and file output, and a hard timeout that
  calls `std::process::exit(124)`. Those cannot execute in the sidecar unchanged.
- Replace the upstream watchdog/fallback runtime with the shared driver and
  bounded guest executor. Cancellation must interrupt JS and abandon pending
  host work; a Tokio timeout alone cannot stop synchronous native DOM work.
- Account for native DOM, retained resources, cookies, frames, module maps,
  pending operations, and browser output. A V8 heap limit does not cover the
  browser's Rust allocations. Current request caps do not solve this.
- Audit every enabled upstream native I/O and environment access. The current
  patches cover navigation, files, and fetch; they are not an exhaustive audit.
  CDP/MCP servers, rendering, storage persistence, and stealth transport are not
  included in the prepared browser subset.
- Add actual kernel-policy and process-lifecycle integration tests and upstream
  browser-conformance tests for the V8 compatibility port. Decide whether to
  maintain that port or upgrade the existing V8 engine before shipping.

These are remaining engineering tasks, not a claim that Obscura is impossible
to integrate. Registering a guest command before they are implemented would
give an incomplete and misleading security boundary.
