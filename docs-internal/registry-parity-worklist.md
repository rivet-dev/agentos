# Registry Linux-Parity Worklist

Status: worklist · Owner: registry · Last updated: 2026-07-07

## Goal (hand this to the driver agent)

> Drive every item in this worklist to **clean Linux parity**: each command/
> behavior must work end-to-end the way it does on real Linux, **proven by real
> e2e tests** — not by a WASI-specific port, a stub, or a shim that only satisfies
> the test. Example of the bar: `duckdb` must run real analytical SQL against real
> file-backed databases and pass real e2e tests — not a stripped "WASI duckdb"
> that only does `SELECT 1`.
>
> **Rules:**
> - **🚧 REAL TOOL, NOT A REIMPLEMENTATION (the load-bearing rule).** Every command
>   must be the **real upstream tool** (GNU coreutils, GNU grep/sed/gawk, real
>   `curl`, real `git`, real `jq`, GNU tar/gzip/diffutils, …) compiled to
>   `wasm32-wasip1` and **patched as needed** for WASI. Do **NOT** ship a
>   from-scratch Rust/C rewrite, a stub, or a hand-rolled CLI over a library.
>   Reimplementations drift from Linux behavior in a thousand small ways and are
>   exactly why several commands fail parity. Sole exception: when the upstream
>   canonical tool *is itself* the Rust project (**ripgrep**, **fd**) — then the
>   real project is correct. Prefer the genuine upstream tool (real git, real
>   grep) over a rewrite; a *popular, established* reimplementation is an
>   acceptable fallback only when the real tool genuinely won't build.
> - **"Not possible" is a valid outcome — but only after trying really hard.** The
>   sysroot is **ours**: a patched Rust std + libc with custom host-syscall imports
>   (see CLAUDE.md → Software Build (WASM Toolchain)). A missing libc/POSIX API
>   (`getrlimit`/`RLIMIT_NOFILE`, `getgroups`, …) is **NOT** a WASI wall — it is a
>   stub/patch we add one layer down, and the build should proceed as if targeting
>   native POSIX. Only if a command *still* cannot be built as the real (or an
>   established) tool do you mark it **`NOT POSSIBLE (WASI)`** in this doc, with a
>   concrete explanation of the genuine, documented wall (never "WASI lacks a
>   syscall we could implement") and what was tried. Exhaust real options first:
>   patch the sysroot, patch the tool, stub the specific missing syscall — a
>   genuine effort, not a quick bail.
> - **Commit clean revs — no stray artifacts.** Each rev must contain only the
>   intended source + test changes. Never commit build outputs, vendored toolchain
>   trees, `__pycache__`/`*.pyc`, generated binaries, or anything that belongs in
>   `.gitignore`. Before `jj describe`, run `jj diff -r @ --summary` and confirm
>   every path is intended — watch especially for `A` (added) paths under
>   `toolchain/`, `**/target/`, `**/node_modules/`, `**/build/`, `**/__pycache__/`.
>   Then **audit the entire stack up to main** (`jj diff -r 'main..@' --stat`, or
>   per rev) and strip anything that slipped in with `jj restore --from <parent>
>   <path>`, adding the pattern to `.gitignore` so it cannot recur.
> - **One jj rev per item.** Concretely: **`jj new` before starting each item**,
>   make that command's fix *and* its e2e test in that single change, `jj describe`
>   it with a clear conventional-commit message, then `jj new` again for the next
>   item. One command per rev — never batch two commands (or unrelated changes)
>   into one rev. Verify the folder + branch first (`pwd`, `jj log -r @`) since the
>   working copy is shared.
> - **Parity, not workarounds.** Fix the real cause (VFS syscall, shell semantics,
>   link conflict, missing feature). If a WASI limitation forces a deviation from
>   Linux, that is a finding to surface — not something to paper over in the test.
> - **Real tests are the deliverable.** A fix isn't done until an un-skipped e2e
>   test exercises the real behavior in a VM and passes. No `describe.skip`, no
>   assertions weakened to match broken output.
> - Work top-down by priority. Re-verify with the actual VM run, not just typecheck.

## Priority tiers

- **P0 — runtime/VM correctness**: bugs in the shared runtime that silently
  corrupt data or break process control. Highest blast radius.
- **P1 — broken shipped commands**: packages that build but don't work like Linux.
- **P2 — build/compile failures**: packages that can't be produced at all.
- **P3 — disabled/absent coverage**: real behavior exists but no real test proves it.

---

## ⚠️ Cross-cutting #0 — Command provenance: replace reimplementations with real tools

**This is the highest-leverage item and reshapes several below.** Audit revealed
that **most commands are NOT the real Linux tool** — they are custom Rust rewrites
(`secureexec-*` crates) or `uutils`, plus at least one hand-rolled C CLI (curl).
Per the load-bearing rule, each must become the **real upstream tool** compiled to
WASI and patched as needed.

**Rule (your call):** an **established project** — whether it's the real upstream
tool *or* an established third-party package that does the real work (uutils,
jaq, etc.) — is **fine**. **Custom code we wrote ourselves** is **not** and must
be replaced with a real/established implementation. Audit of every command's
actual backing:

### ✅ Established — keep (real upstream tool or established package doing the work)
| Command(s) | Backing |
|---|---|
| coreutils (`sh`+80) | **uutils** (`uucore`) — established Rust project |
| ripgrep (`rg`) | real ripgrep |
| duckdb, vim | real upstream C source, patched for WASI |
| sqlite3 **engine** | real SQLite amalgamation (⚠️ but the *CLI* is ours — see below) |
| jq | **jaq** (`jaq-core/std/json`) — established Rust jq |
| yq | jaq + `serde_yaml`/`toml`/`quick-xml` — established parsers (thin glue is ours) |
| sed | `sed` crate (published) |
| gawk (`awk`) | `awk-rs` crate (published) |
| tar | `tar` crate (established) |
| gzip | `flate2` (established) |
| diffutils (`diff`) | `similar` crate (established) |
| file | `infer` crate (established magic-byte lib; note: not real libmagic `file`) |

### ❌ CUSTOM WE BUILT — flag & replace with a real/established impl
| Command | Status | What it actually is | Replace with |
|---|---|---|---|
| **curl** | TODO | our custom driver over a libcurl fork | real `curl` CLI (upstream `src/tool_*.c`) |
| **wget** | TODO (retry) | our 174-line `wget.c` (dropped) | real GNU Wget vs our sysroot — stub `getrlimit`/`getgroups`, then build |
| **http-get** | TODO | our 95-line `http_get.c` | drop, or a real tool |
| **git** | TODO | our hand-rolled git from `sha1`+`flate2` | **real git** (upstream C), patched for WASI — **NOT gitoxide** |
| **fd** | TODO | our `secureexec-fd` on raw `regex` (not sharkdp/fd) | real **fd** (sharkdp) |
| **findutils** (`find`,`xargs`) | TODO | our hand-rolled on `regex`/shims | real GNU findutils, or `uutils/findutils` |
| **tree** | TODO | our hand-rolled, zero deps | real `tree`, or an established one |
| **grep** | TODO | our `secureexec-grep` on raw `regex` (**not** an established grep pkg) | **real GNU grep**, or a popular established grep (ripgrep's `grep` crates) |
| **zip** | TODO | our 203-line `zip.c` over zlib/minizip (not Info-ZIP) | real Info-ZIP, or an established lib's CLI |
| **unzip** | TODO | our 669-line `unzip.c` over zlib/minizip | real Info-ZIP unzip |
| **sqlite3 CLI** | TODO | our 558-line `sqlite3_cli.c` (engine is real SQLite; the shell is ours) | real SQLite `shell.c` (its official CLI) |
| **vix** | DONE | from-scratch source-less drop-zone binary | deleted; real `vim` covers the editor slot |

Note: `codex`/`codex-exec` = the rivet fork of OpenAI's codex — established fork,
external build (tracked separately in #9).

**Objective:** replace each ❌ with a real/established implementation built to
`wasm32-wasip1` and patched only where WASI forces it. The ✅ rows stay.

**Approach:** one command at a time, one jj rev each: swap our custom code for the
established source (fetched + pinned like sqlite/duckdb), wire into the toolchain,
patch for WASI, prove parity with real e2e tests.

**Interaction with other items:** subsumes several below — curl (#6) is "build the
real curl," and the `no-test` packages (#12) that are ❌ here should move to a real
impl *before* their tests are written, so the tests validate real behavior.

**Decisions (settled):** git → **real git** (not gitoxide). grep → **real GNU
grep**, or a popular established grep if the real one won't build.

## Status tracking (how the driver reports progress in this doc)

Update this doc as you go — it is the single source of truth for status. For each
❌ command, set one status and keep it current:

- **`TODO`** — not started.
- **`IN PROGRESS`** — being built; note the current blocker if any.
- **`DONE`** — the real/established tool builds and passes a real un-skipped e2e
  test; link the jj rev.
- **`NOT POSSIBLE (WASI)`** — only after a genuine effort. Write a concrete
  explanation: exactly what blocks it, what you tried (sysroot patch, tool patch,
  syscall stub), and why it can't be made to work. This is a documented dead-end,
  never a silent fallback to a custom rewrite.

Mark each row's status inline in the table (or as a short line under the command)
so a reader sees the whole board at a glance.

---

## P0 — Runtime / VM correctness

### 1. brush-shell `>>` append truncates instead of appending — DONE
- **Broken:** `execSync` with `>>` onto a write-only file overwrites instead of
  appends. `expected 'changed' to be 'originalchanged'`. (issue: rivet-dev/agentos#1657)
- **Objective:** `>>` opens `O_WRONLY|O_APPEND` against the kernel VFS and appends,
  identical to bash on Linux.
- **Proof:** `bridge-child-process.test.ts` append redirection tests pass
  un-skipped; direct kernel append and native sidecar append regressions pass.
- **rev:** `ouxrzutq` — `fix(runtime): honor >> append mode in guest shell VFS redirection`

### 2. brush-shell `cat < file` stdin redirection fails (exit 1) — DONE
- **Broken:** `cat < stdin-input.txt` exits 1 — input redirection from a VFS path
  isn't wired to the command's stdin. (issue: #1657)
- **Objective:** `< file` feeds the VFS file to stdin; command reads it and exits 0,
  like Linux.
- **Proof:** the "stdin redirection feeds the kernel VFS file" test passes
  un-skipped after the parent host-shadow pre-spawn sync fix in item 1.
- **rev:** `lonnzuqw` — `test(registry): mark stdin redirection parity proven`

### 3. WasmVM signal/dispose — SIGKILL/SIGTERM don't terminate; dispose hangs — DONE
- **Broken:** SIGKILL/SIGTERM don't kill guest processes; `dispose` times out
  (5 tests across `signal-forwarding.test.ts`, `dispose-behavior.test.ts`).
- **Objective:** signals delivered to guest processes terminate them promptly and
  `dispose` tears down active WasmVM + Node processes, matching Linux signal
  semantics. **Not yet filed — file a separate issue.**
- **Proof:** `signal-forwarding.test.ts` passes 5/5 in
  `2026-07-07T23-11-36-0700-item3-signal-forwarding-final-pass-2.txt`;
  `dispose-behavior.test.ts` passes 3/3 in
  `2026-07-07T23-11-21-0700-item3-dispose-behavior-final-pass.txt`.
- **rev:** `zkywnwup` — `fix(runtime): unblock WasmVM signal waits and dispose`

### 4. VFS missing `pwrite` — sqlite3 file-backed DBs don't persist — DONE
- **Broken:** `filesystem method pwrite is unavailable` — sqlite3 file-backed DB
  can't persist across exec calls.
- **Objective:** the VFS implements positioned writes (`pwrite`/`pwritev`) so any
  command doing positioned I/O (sqlite3, and others) behaves like Linux.
- **Proof:** sqlite3 "file-based DB persists across separate exec calls" passes
  in `2026-07-07T23-18-45-0700-item4-sqlite3-file-db-pwrite-pass.txt`; direct
  mounted JS VFS `pwrite` test passes in
  `2026-07-07T23-18-45-0700-item4-runtime-core-custom-vfs-pwrite-pass.txt`.
  Type/build checks pass in `2026-07-07T23-19-11-0700-item4-runtime-core-build.txt`
  and `2026-07-07T23-19-11-0700-item4-sqlite3-check-types.txt`.
- **rev:** `klrzzkro` — `fix(vfs): expose positioned writes in test kernel`

### 5. Socket-layer failures (net-server/udp/unix, signal_handler) — DONE
- **Broken:** in the audit run, `st.create is not a function` + a `LinkError` in
  net tests; signal_handler didn't catch signals. May be partial-build artifacts.
- **Objective:** TCP/UDP/Unix socket + signal test programs run to completion in
  the VM with real socket semantics. **First reconfirm on a full build** — if it
  reproduces, fix the socket-table wiring / link error.
- **Proof:** net-server/net-udp/net-unix/signal-handler suites pass together in
  `2026-07-08T00-23-43-0700-item5-four-suites-take-signal-bridge.txt`.
  Runtime and native sidecar builds pass in
  `2026-07-08T00-24-02-0700-item5-final-runtime-core-build.txt` and
  `2026-07-08T00-24-02-0700-item5-final-native-sidecar-build.txt`; native
  embedded signal coverage passes in
  `2026-07-08T00-24-02-0700-item5-final-native-embedded-runtime-signal-suite.txt`.
- **rev:** `zvyxkkyv` — `fix(runtime): repair Wasm socket and signal integration`

---

## P1 — Broken shipped commands

### 6. curl — reimplemented CLI, exits 1 on every operation (incl. `--version`) — DONE
- **Broken:** the `curl` command is a **hand-rolled `curl.c` driver** over a
  libcurl fork, not the real curl command-line tool — so 24/30 `curl.test.ts` fail
  and every op returns exit 1, even `curl --version`.
- **Objective (per Cross-cutting #0):** **build the real curl command-line tool**
  (upstream `src/tool_*.c`) to `wasm32-wasip1` against the patched sysroot,
  patched only where WASI forces it — replacing the custom driver. All real curl
  behavior (GET/POST, `-I`/`-D`, `-L`, `-u`, `-F`, `-o`/`-O`, `-w`, `-K`) then
  works because it *is* curl, not a shim.
- **Proof:** `software/curl/test/` passes un-weakened: 25 passed, 5 skipped in
  `2026-07-08T00-41-57-0700-item6-curl-test-after-tls-flags.txt`. Runtime runner
  build/protocol checks pass in
  `2026-07-08T00-41-51-0700-item6-runtime-core-build-tls-flags.txt`.
- **rev:** `oxoqrwvk` — `fix(curl): build the real curl CLI for WASI`

### 7. zip / unzip — hostile-archive hardening cases fail (3 each) — DONE
- **Broken:** fallback parser doesn't reject a wrapping local offset, doesn't skip
  empty-normalized-name entries, doesn't cap hostile uncompressed sizes before
  allocating.
- **Objective:** unzip rejects/handles malformed & hostile archives the way a
  hardened Linux unzip does (bounded allocation, typed errors), and zip↔unzip
  roundtrips remain correct.
- **Proof:** `software/unzip/test/` passes 6/6 in
  `2026-07-08T00-57-22-0700-item7-unzip-test-final-pass.txt`; `software/zip/test/`
  passes 2/2 in `2026-07-08T00-57-22-0700-item7-zip-test-final-pass.txt`.
  Package type checks pass in `2026-07-08T00-57-39-0700-item7-unzip-check-types.txt`
  and `2026-07-08T00-57-39-0700-item7-zip-check-types.txt`.
- **rev:** `krxkqtnx` — `fix(unzip): harden fallback archive parsing`

---

## P2 — Build / compile failures

### 8. wget — TODO (retry against our own sysroot)
- **Objective:** ship real GNU Wget as the `wget` command (real upstream tool,
  patched as needed), proven by a real e2e download test. The old package was a
  custom 174-line `wget.c` wrapper and was dropped — it must be restored as the
  real tool, not re-added as a shim.
- **Prior attempt (bailed too early):** GNU Wget 1.24.5 configured for
  `wasm32-wasip1`, HTTP-only (`--without-ssl --without-zlib --without-libpsl
  --disable-iri`, auth/thread options off) against the patched sysroot. It needed
  WASI patches for `inet_addr`, `O_BINARY`, process-group terminal checks,
  `spawn.h`/`--use-askpass`, interactive `getpass`, gnulib `NSIG`, `F_DUPFD`, and
  `flock` — all fine — then stopped at gnulib wanting `getrlimit`/`RLIMIT_NOFILE`
  and an incompatible `getgroups`.
- **Why that stop was wrong:** the sysroot is ours (see CLAUDE.md → Software Build
  (WASM Toolchain)). `getrlimit` can return a fixed `RLIMIT_NOFILE`; `getgroups`
  can return the single group. Those are a few-line stub in the patched
  libc/host-import layer, exactly like the spawn/fd/user-group imports already
  added. "WASI lacks it" is not a wall here.
- **Retry plan:** (1) add `getrlimit`/`RLIMIT_NOFILE` and a compatible `getgroups`
  to the sysroot (stub is acceptable); (2) clear the remaining gnulib cascade the
  same way, one patch at a time; (3) patch Wget's own source only if a fix
  genuinely cannot live in the libc layer; (4) restore the `wget` package/command
  and prove it with a real e2e download test. Only mark `NOT POSSIBLE (WASI)`
  again if a concrete, documented wall remains after that.
- **Note:** real upstream `curl` (#6) already covers downloads, so wget is not
  urgent — but it should be retried as the real tool, not left dropped.

### 9. codex-cli — not buildable in-checkout (needs external fork)
- **Broken:** requires the external `codex-rs` fork (`CODEX_REPO` absent); tests
  `describe.skip`.
- **Objective:** decide the build story — vendor/pin the fork or document the
  required checkout — so `codex`/`codex-exec` build reproducibly here, then real
  e2e tests run (real upstream SDK per CLAUDE.md, not an API stub).
- **Proof:** codex builds in CI/dev; `software/codex-cli/test/` runs un-skipped.
- **rev:** `build(codex-cli): make the codex-rs fork build reproducible`

### 10. vix — DONE (deleted)
- **Resolved:** `vix` was a from-scratch, source-less drop-zone binary — exactly
  the kind of hand-rolled artifact this repo should not carry. **Removed entirely**
  (package dir, shell import/dep, `EXTERNAL_COMMANDS` Makefile hack, README rows,
  website registry entry) in rev
  `chore(registry): remove vix package; document real-tool (no-reimplementation) principle`.
  Real `vim` (#11) covers the editor slot. Preserved source (`vix.c`, `BUILD-vix.md`,
  `vix.wasm`) remains in `~/progress/agent-os/2026-06-28-just-shell-fix/` if ever
  needed. No further work.

---

## P3 — Disabled / absent coverage (real tests to Linux parity)

For each: replace `describe.skip` with `describeIf(binaryPresent)` **and** write
real e2e tests that prove Linux-parity behavior — not smoke tests.

### 11. Disabled suites — git, duckdb, codex
- **Broken:** tests exist but are `describe.skip`, so nothing runs even when the
  binary is present.
- **Objective (per command, Linux parity):**
  - **git** — clone/commit/log/diff/branch against a real repo & remote.
  - **duckdb** — real analytical SQL, CSV read/write, file-backed DBs (the bar
    example: real duckdb, not a WASI-stripped `SELECT 1`).
  - **codex** — real run (after #9).
- **Proof:** each un-skipped suite passes with real behavior.
- **rev:** one per command, e.g. `test(duckdb): real analytical-SQL e2e; un-skip`

### 12. No tests at all — 12 software + 5 agents
- **Broken:** zero e2e coverage: `gawk, sed, grep, tar, gzip, jq, ripgrep, yq,
  diffutils, file, http-get, vim`; agents `claude, codex, opencode, pi, pi-cli`.
- **Objective:** write real e2e tests proving each behaves like its Linux
  counterpart (jq processes real JSON, sed edits streams, tar round-trips archives,
  grep/rg search real trees, gzip round-trips, etc.); agents exercise the real ACP
  adapter against the upstream SDK.
- **Proof:** `software/<pkg>/test/` exists and passes for each; coverage gate green.
- **rev:** one per package, e.g. `test(jq): add real JSON-processing e2e`

---

## Cross-cutting / misc

### 13. `everything` meta-package has no `agentos-package.json`
- **Broken:** parse-failed in the audit — the bundle has no manifest.
- **Objective:** valid manifest (or confirm the bundle mechanism is intentional and
  fix discovery accordingly) so `everything` resolves like the other bundles.
- **Proof:** manifest present/valid; package resolves and installs its members.
- **rev:** `fix(everything): add valid agentos-package.json`

---

## Sequencing note

P0 first — several P1/P3 items depend on it: curl (#6) needs sockets/HTTP;
sqlite3 file-DB tests (#11) need pwrite (#4). Fix the runtime layer, then the
commands that ride on it, then backfill coverage. One jj rev per item throughout.
