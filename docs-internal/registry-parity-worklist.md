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
> - **One jj rev per item.** New change per fix; `jj describe` with a clear
>   conventional-commit message; do not batch unrelated fixes into one rev.
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

## P0 — Runtime / VM correctness

### 1. brush-shell `>>` append truncates instead of appending
- **Broken:** `execSync` with `>>` onto a write-only file overwrites instead of
  appends. `expected 'changed' to be 'originalchanged'`. (issue: rivet-dev/agentos#1657)
- **Objective:** `>>` opens `O_WRONLY|O_APPEND` against the kernel VFS and appends,
  identical to bash on Linux.
- **Proof:** `bridge-child-process.test.ts` "append redirection … succeeds like
  Linux" passes un-skipped, plus a direct VFS append test.
- **rev:** `fix(runtime): honor >> append mode in guest shell VFS redirection`

### 2. brush-shell `cat < file` stdin redirection fails (exit 1)
- **Broken:** `cat < stdin-input.txt` exits 1 — input redirection from a VFS path
  isn't wired to the command's stdin. (issue: #1657)
- **Objective:** `< file` feeds the VFS file to stdin; command reads it and exits 0,
  like Linux.
- **Proof:** the "stdin redirection feeds the kernel VFS file" test passes.
- **rev:** `fix(runtime): wire < stdin redirection from VFS in guest shell`

### 3. WasmVM signal/dispose — SIGKILL/SIGTERM don't terminate; dispose hangs
- **Broken:** SIGKILL/SIGTERM don't kill guest processes; `dispose` times out
  (5 tests across `signal-forwarding.test.ts`, `dispose-behavior.test.ts`).
- **Objective:** signals delivered to guest processes terminate them promptly and
  `dispose` tears down active WasmVM + Node processes, matching Linux signal
  semantics. **Not yet filed — file a separate issue.**
- **Proof:** the 5 signal/dispose integration tests pass within their timeouts.
- **rev:** `fix(runtime): deliver SIGKILL/SIGTERM to WasmVM processes and unblock dispose`

### 4. VFS missing `pwrite` — sqlite3 file-backed DBs don't persist
- **Broken:** `filesystem method pwrite is unavailable` — sqlite3 file-backed DB
  can't persist across exec calls.
- **Objective:** the VFS implements positioned writes (`pwrite`/`pwritev`) so any
  command doing positioned I/O (sqlite3, and others) behaves like Linux.
- **Proof:** sqlite3 "file-based DB persists across separate exec calls" passes;
  add a direct VFS pwrite test.
- **rev:** `fix(vfs): implement pwrite for positioned writes`

### 5. Socket-layer failures (net-server/udp/unix, signal_handler)
- **Broken:** in the audit run, `st.create is not a function` + a `LinkError` in
  net tests; signal_handler didn't catch signals. May be partial-build artifacts.
- **Objective:** TCP/UDP/Unix socket + signal test programs run to completion in
  the VM with real socket semantics. **First reconfirm on a full build** — if it
  reproduces, fix the socket-table wiring / link error.
- **Proof:** net-server/net-udp/net-unix/signal-handler suites pass on a full build.
- **rev:** `fix(runtime): repair socket-table wiring for net/signal tests` (only if confirmed)

---

## P1 — Broken shipped commands

### 6. curl — exits 1 on every operation (including `--version`)
- **Broken:** 24/30 `curl.test.ts` fail; every op returns exit 1, even
  `curl --version`. The binary is non-functional in the VM as built.
- **Objective:** real curl behavior — GET/POST, headers (`-I`/`-D`), redirects
  (`-L`), auth (`-u`), multipart (`-F`), file output (`-o`/`-O`), `-w`, `-K` — all
  work like Linux curl over the runtime's socket/HTTP layer. **Not** a fetch-shim.
- **Proof:** `software/curl/test/` (the existing 30 tests) pass un-weakened.
- **rev:** `fix(curl): make curl functional in the VM (real HTTP over runtime sockets)`

### 7. zip / unzip — hostile-archive hardening cases fail (3 each)
- **Broken:** fallback parser doesn't reject a wrapping local offset, doesn't skip
  empty-normalized-name entries, doesn't cap hostile uncompressed sizes before
  allocating.
- **Objective:** unzip rejects/handles malformed & hostile archives the way a
  hardened Linux unzip does (bounded allocation, typed errors), and zip↔unzip
  roundtrips remain correct.
- **Proof:** `software/zip/test/` + `software/unzip/test/` hardening cases pass.
- **rev:** `fix(unzip): bound allocation and reject malformed archive entries`

---

## P2 — Build / compile failures

### 8. wget — does not compile (`duplicate symbol: getpeername`)
- **Broken:** link error — wget's stub `getpeername` collides with the patched
  sysroot's socket impl.
- **Objective:** wget compiles cleanly against the patched sysroot (drop the stub;
  use the real sysroot socket symbols) **and** downloads files e2e like Linux wget.
- **Proof:** `make -C toolchain cmd/wget` succeeds; `software/wget/test/` passes
  un-skipped (real download over the runtime network).
- **rev:** `fix(wget): drop conflicting getpeername stub; build against sysroot sockets`

### 9. codex-cli — not buildable in-checkout (needs external fork)
- **Broken:** requires the external `codex-rs` fork (`CODEX_REPO` absent); tests
  `describe.skip`.
- **Objective:** decide the build story — vendor/pin the fork or document the
  required checkout — so `codex`/`codex-exec` build reproducibly here, then real
  e2e tests run (real upstream SDK per CLAUDE.md, not an API stub).
- **Proof:** codex builds in CI/dev; `software/codex-cli/test/` runs un-skipped.
- **rev:** `build(codex-cli): make the codex-rs fork build reproducible`

### 10. vix — external binary, no source or build recipe in repo
- **Broken:** `EXTERNAL_COMMANDS` — a hand-built WASM binary dropped in with **no
  source and no build pipeline** committed. Also the shipped
  `software/vix/dist/package.aospkg` is a **1100-byte manifest stub that does NOT
  embed the wasm** — the real 214 KB binary is injected at build time.
- **Provenance (recovered):** vix is a **from-scratch 309-line single-file C
  editor** (`vix.c`) — *not* a port of vis/neatvi/nvi/busybox/elvis. It avoids
  `termios.h` entirely, using three guest imports
  (`host_tty::{isatty,get_size,set_raw_mode}`, same ABI as
  `toolchain/.../c/programs/pty_probe.c`) for raw mode + window size, plus VT100;
  supports insert mode and `:w`/`:wq`/`:q`. Built by Claude thread
  `46371327-9e48-4c4c-8150-10dd21d7bf0f` (2026-06-29). Source, recipe, and binary
  are **preserved and reproducible** in
  `~/progress/agent-os/2026-06-28-just-shell-fix/` → `vix.c`, `BUILD-vix.md`,
  `vix.wasm` (214 KB, md5 `a6e650f03493ad0dff230691d67ee3bd`). Original build:
  ```
  $WASI_SDK/bin/clang --target=wasm32-wasip1 --sysroot=$WASI_SDK/share/wasi-sysroot \
    -O2 -I <c>/include -o vix.wasm vix.c      # vanilla sysroot, no termios
  ```
- **Decide first:** keep vix at all? A real **vim** (11.5 MB, `wasm32-wasip1`) now
  exists (#11), so vix's role is "tiny editor with no bundled runtime." Either (a)
  keep it and properly source it, or (b) drop the package in favor of vim.
- **Objective (if kept):** commit `vix.c` into `software/vix/native/c/`, add a real
  build recipe (host_tty imports + the sysroot), wire it into `toolchain` as a
  normal C command, **remove the `EXTERNAL_COMMANDS` drop-zone hack**, and fix the
  `.aospkg` so it actually embeds the binary. Then a real editor e2e test (raw-mode
  keystrokes through the VM PTY → `:wq` writes the VFS file).
- **Proof:** `make -C toolchain cmd/vix` builds from committed source (no dropped
  binary); `software/vix/test/` drives real modal editing and asserts the written file.
- **rev:** `build(vix): commit vix.c + build recipe; drop EXTERNAL_COMMANDS; embed binary in aospkg`

---

## P3 — Disabled / absent coverage (real tests to Linux parity)

For each: replace `describe.skip` with `describeIf(binaryPresent)` **and** write
real e2e tests that prove Linux-parity behavior — not smoke tests.

### 11. Disabled suites — git, duckdb, wget, codex
- **Broken:** tests exist but are `describe.skip`, so nothing runs even when the
  binary is present.
- **Objective (per command, Linux parity):**
  - **git** — clone/commit/log/diff/branch against a real repo & remote.
  - **duckdb** — real analytical SQL, CSV read/write, file-backed DBs (the bar
    example: real duckdb, not a WASI-stripped `SELECT 1`).
  - **wget** — real download (after #8).
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
sqlite3 file-DB tests (#11) need pwrite (#4); wget (#8) needs sockets. Fix the
runtime layer, then the commands that ride on it, then backfill coverage. One jj
rev per item throughout.
