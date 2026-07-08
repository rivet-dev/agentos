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
| **curl** | DONE | our custom driver over a libcurl fork | real `curl` CLI (upstream `src/tool_*.c`) |
| **wget** | DONE | our 174-line `wget.c` (dropped) | real GNU Wget vs our sysroot — stub `getrlimit`/`getgroups`, then build |
| **http-get** | TODO | our 95-line `http_get.c` | drop, or a real tool |
| **git** | TODO | our hand-rolled git from `sha1`+`flate2` | **real git** (upstream C), patched for WASI — **NOT gitoxide** |
| **fd** | TODO | our `secureexec-fd` on raw `regex` (not sharkdp/fd) | real **fd** (sharkdp) |
| **findutils** (`find`,`xargs`) | TODO | our hand-rolled on `regex`/shims | real GNU findutils, or `uutils/findutils` |
| **tree** | DONE | our hand-rolled, zero deps | real `tree`, or an established one |
| **grep** | TODO | our `secureexec-grep` on raw `regex` (**not** an established grep pkg) | **real GNU grep**, or a popular established grep (ripgrep's `grep` crates) |
| **zip** | DONE | our 203-line `zip.c` over zlib/minizip (not Info-ZIP) | real Info-ZIP, or an established lib's CLI |
| **unzip** | DONE | our 669-line `unzip.c` over zlib/minizip | real Info-ZIP unzip |
| **sqlite3 CLI** | DONE | our 558-line `sqlite3_cli.c` (engine is real SQLite; the shell is ours) | real SQLite `shell.c` (its official CLI) |
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

**git — where the issues are (assessment):**
- **LICENSE — RESOLVED, not a blocker.** Each command ships as its **own published
  npm package**, so a GPL-2.0 git binary in `@agentos-software/git` is **mere
  aggregation** — it does not affect the Apache-2.0 licensing of agentOS or any
  other package. Ship the git package under GPL-2.0 (offer its source) and we're
  compliant. This **supersedes** the clean-room-reimpl rationale in
  `toolchain/std-patches/git/README.md` ("cannot be vendored due to license
  restrictions"): that premise no longer holds — go with **real git** (upstream C)
  and update/remove that README. (gitoxide stays ruled out.)
- **Technical WASI issues if we do build real git** (from that README + git's own
  build knobs), easiest → hardest:
  - `mmap` (packfiles/index) → build `NO_MMAP=1` (malloc+read). Fine.
  - signals (SIGPIPE/SIGCHLD) → build without; WASI has none. Fine.
  - threads (index-pack/pack-objects) → `NO_PTHREADS`; single-threaded, slower. Fine.
  - `fork()`+`exec()` in `run_command.c` (hooks, filters, remote helpers) → route to
    `posix_spawn` via the `wasi-spawn` broker (spawn IS supported — same fix as wget).
  - **Network transport (clone/fetch/push) — the hard part.** Smart-HTTP needs the
    `git-remote-https` **helper subprocess** + libcurl; `git://` needs raw sockets;
    ssh needs an `ssh` subprocess. Each helper must itself exist as a module.
  - symlink checkout → `core.symlinks=false` fallback (WASI symlink support is
    partial); local time → UTC like elsewhere.
- **Bottom line:** license is a non-issue (separate published package = mere
  aggregation). **local** git (init/add/commit/log/diff/branch/merge/status/
  checkout) is very achievable; **remote** git (clone/fetch/push over
  smart-HTTP/ssh) is the real effort. Proceed with real git.

**Replacement findings — what each remaining ❌ tool will take (investigated):**

The recurring wall is **never a syscall** — it's one of two known, already-solved
patterns: **(a) no threads** on `wasm32-wasip1` → serial patch like
`toolchain/std-patches/crates/uu_sort/0001-wasi-serial-sort.patch` (hits real fd &
ripgrep crates); **(b) gnulib `getrlimit`/`getgroups`** → sysroot stubs, already
documented for wget (item #8) (hits GNU grep/findutils). Subprocess spawn already
works (`wasi-spawn` broker), so `xargs` is not a blocker.

- **sqlite3 CLI — DONE.** Engine was already the real amalgamation
  (`libs/sqlite3/sqlite3.c`); the command now builds the official upstream
  `shell.c` from the same fetched zip as `sqlite3`. The local 558-line
  `sqlite3_cli.c` reimplementation is deleted, `toolchain/c/build/sqlite3` is the
  primary C output, `sqlite3_cli` remains only as a compatibility alias, and the
  tracked runtime-core fallback command is refreshed to the same official shell.
  Proof: official shell build passes in
  `2026-07-08T05-04-47-0700-sqlite3-official-shell-build-command-name.log`;
  package-focused e2e passes 16/16, including real `.tables`, `.schema`, and
  `.dump` CLI arguments, in
  `2026-07-08T05-07-06-0700-sqlite3-official-shell-tests-final-focused.log`;
  package build/check-types pass in
  `2026-07-08T05-07-52-0700-sqlite3-package-build-official-shell-final.log` and
  `2026-07-08T05-07-52-0700-sqlite3-check-types-official-shell-final.log`;
  runtime-core fallback command path passes `.tables` in
  `2026-07-08T05-09-12-0700-sqlite3-runtime-core-command-fallback-test.log`;
  aggregate C `programs` builds 57 commands in
  `2026-07-08T05-09-53-0700-sqlite3-make-programs-final.log`.
  Rev: `typytnkk` — `fix(sqlite3): build official SQLite shell`.
- **http-get — drop, don't port.** `http_get.c` is a 95-line raw-socket **loopback
  test client**, not an HTTP fetcher; real curl covers GET. But it's imported by
  `packages/shell` and is the client in cross-runtime network tests — migrate those
  dependents first, then drop the command.
- **tree — DONE.** Replaced the custom Rust `secureexec-tree`/`cmd-tree` crates
  with upstream Steve Baker `tree` 2.3.2 from `OldManProgrammer/unix-tree`.
  It builds as a C toolchain command from pinned source, stages into
  `@agentos-software/tree`, and refreshes the tracked runtime-core fallback
  command. Sysroot fixes live one layer down: install `<grp.h>` and provide
  deterministic missing-group lookup stubs so upstream `-g` support links
  without a tree-source WASI branch. Proof: upstream source inspection in
  `2026-07-08T05-13-50-0700-tree-fetch-upstream-2.3.2-inspect.log`; sysroot
  patch check passes in
  `2026-07-08T05-18-16-0700-tree-wasi-libc-patch-check-group-lookup-fixed.log`;
  Makefile build passes in
  `2026-07-08T05-20-02-0700-tree-upstream-make-build.log`; package build and
  check-types pass in
  `2026-07-08T05-21-08-0700-tree-package-build-upstream-after-install.log` and
  `2026-07-08T05-21-08-0700-tree-check-types-upstream-after-install.log`; e2e
  tree tests pass 6/6 in
  `2026-07-08T05-29-45-0700-tree-vitest-upstream-final.log`; aggregate C
  `programs` builds 58 commands in
  `2026-07-08T05-30-44-0700-tree-make-programs-final.log`; Cargo metadata no
  longer includes the deleted Rust tree crates in
  `2026-07-08T05-33-17-0700-tree-cargo-metadata-after-removing-empty-dirs.log`.
  Rev: `kpmrwxln` — `fix(tree): build upstream tree`.
- **fd — moderate.** Swap `cmd-fd` to depend on real `fd-find` (like
  coreutils→`uu_*`; `fd-lock` is already patched). Only real issue: the parallel
  `ignore`/crossbeam walker needs the **serial-thread patch** (uu_sort pattern).
- **grep — moderate.** Decision is real GNU grep (gnulib cascade → sysroot stubs)
  or ripgrep's `grep-*` crates (threads → serial patch). NB the current
  `secureexec-grep` also backs `rg`, so the shipped "ripgrep" is custom too.
- **zip / unzip — moderate.** Real **Info-ZIP** source (fetch+pin like zlib/sqlite);
  zlib is already vendored. Filesystem + `isatty`/`utime`/`chmod`/perms stubs; no
  sockets/threads/spawn. Friction is Info-ZIP's crufty build, not syscalls.
- **findutils — moderate→hard.** `find` is fs+regex (easy); `xargs` spawn already
  works. **uutils/findutils** (Rust) avoids gnulib; **GNU findutils** (C) hits the
  same gnulib cascade as wget. Prefer uutils unless GNU parity is required.

Ranked easiest→hardest: **sqlite3-CLI · http-get (drop) · tree · fd · grep · zip ·
unzip · findutils.**

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
- **Note (how well it works):** it *is* the real curl CLI (`src/tool_main.c`) plus a
  custom `vtls/wasi_tls.c` backend (`USE_WASI_TLS`) — HTTPS runs through the host
  TLS bridge, not OpenSSL. Real HTTP(S) `GET/POST/-I/-D/-L/-u/-F/-o/-O/-w/-K` all
  work because it's genuine curl. Known gaps from the trimmed `./configure`:
  `--compressed`/gzip response decode (`--without-zlib`), brotli/zstd, `libpsl`
  cookie-suffix checks, LDAP, and no CA bundle (cert trust is whatever `wasi_tls`
  enforces). Those are the 5 skipped tests. Verdict: solid for real HTTP(S).

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

### 8. wget — DONE
- **Resolved:** the `wget` command is real upstream GNU Wget 1.24.5 built for
  `wasm32-wasip1`, HTTP-only for now (`--without-ssl --without-zlib
  --without-libpsl --disable-iri`) against the patched AgentOS C sysroot. The old
  custom 174-line wrapper stays removed.
- **Sysroot/runtime fixes:** Wget builds without a Wget-source WASI fork by adding
  the missing POSIX surface one layer down: process/terminal headers including
  `spawn.h`, signal/process/timezone compatibility, overrideable `FD_SETSIZE`,
  Wget-only `_POSIX_TIMERS` overlay, POSIX socket `read`/`write` routing through
  `host_net`, low host-net fds, and `MSG_PEEK` queue preservation in the WASM
  runner. Configure is seeded so gnulib trusts the sysroot `select` instead of
  replacing it with a host-net-incompatible fallback.
- **Proof:** focused basename download passes in
  `2026-07-08T04-33-31-0700-item8-wget-vitest-focused-clean-msg-peek.log`; full
  Wget e2e suite passes 5/5 in
  `2026-07-08T04-33-41-0700-item8-wget-vitest-full-clean-msg-peek.log`. Final
  runner syntax and wasi-libc patch checks pass in
  `2026-07-08T04-34-02-0700-item8-node-check-wasm-runner-final.log` and
  `2026-07-08T04-34-02-0700-item8-wasi-libc-patch-check-final.log`.
- **rev:** `zuosnzmq` — `fix(wget): build real GNU Wget for WASI`

### 9. codex-cli — DONE
- **Resolved:** the `codex`/`codex-exec` package now has an AgentOS-owned wrapper
  for the external `codex-rs` fork build. `make -C toolchain codex-required`
  requires `CODEX_REPO=/path/to/codex-rs/codex-rs`, uses this checkout's
  `toolchain/c/vendor/wasi-sdk`, and installs the fork-built optimized wasm into
  generated `software/codex/wasm/{codex-exec,codex}` for the package build. The
  generated toolchain and wasm command directories are ignored and not committed.
- **Test fix:** the real `codex-exec --session-turn` e2e now uses a streaming
  Responses mock (SSE) and disables Codex shell snapshots inside the VM config,
  avoiding the optional pre-turn shell-snapshot subprocess deadlock while still
  driving the real codex-core agent and shell tool path.
- **Proof:** `CODEX_REPO=/home/nathan/agent-e2e/codex-rs/codex-rs make -C
  toolchain codex-required` builds and installs 29,924,651-byte command artifacts
  in `2026-07-08T01-37-05-0700-item9-codex-build-rerun.txt`; `pnpm --dir
  software/codex-cli build` stages 2 commands and assembles `package.aospkg` in
  `2026-07-08T01-44-50-0700-item9-codex-cli-build.txt`;
  `AGENTOS_E2E_FULL=1 pnpm --dir packages/core exec vitest run
  tests/codex-fullturn.test.ts --reporter=verbose` passes 2 real VM tests in
  `2026-07-08T01-53-55-0700-item9-core-codex-fullturn-pass.txt`.
- **rev:** `svksnzon` — `build(codex-cli): make the codex-rs fork build reproducible`

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

---

## Candidate software (future additions)

Constraint: **C or Rust only** (no Go/Haskell/Python). Real upstream tool or an
established project, same rules as above. **Focus: tools agents invoke headless
and programmatically** — no TUI/visual tools, no dev/build toolchains, no
raw-socket tools. Feasibility: 🟢 easy (fs/compute), 🟡 needs a known pattern
(spawn→`wasi-spawn`, PTY, host TCP/DNS bridge).

**Atuin-validated priority (from 348K real shell commands):** by actual usage the
clear wins are **jj** (18,929 — 3rd overall after sed/rg/grep) and **tmux**
(1,268), then **perl** (563). Modest but real: ssh/rsync (~23 each), psql (20),
dig (10), redis-cli (8), less (4), openssl (3). **Zero usage in this history:**
zstd, xz, gpg, ffmpeg, age, mlr, socat, nc, screen — generically useful but not
agent-triggered here, so lower priority. Big unserved *demand*:
**ps (1,364) + pkill (866) + pgrep (755) ≈ 3K** — see process management below.

**Cross-source validation (Homebrew + Debian popcon + agent-sandbox base images +
SWE-agent/OpenHands/Terminal-Bench trajectories):** independent sources converge
tightly on the list below. **Every agent-sandbox image (OpenHands, devcontainers,
GH Actions) pre-installs:** curl, wget, git, jq, tmux, gnupg, xz, zip/unzip, rsync,
ssh, less, vim, tree, procps, psmisc, socat, netcat, ripgrep, bzip2, lz4, sqlite3,
patch, file — near-exact overlap with what we ship/plan. Real **agent
trajectories** are dominated by coreutils (ls/rm/find/mkdir/cat/mv/chmod) + python
+ curl/wget + grep/sed + openssl + ps — all shipped or listed. New adds surfaced:
**imagemagick** ⭐ (C, image ops — high popularity), **openssl** (confirmed
high-use), **aria2 / brotli / parallel / sshpass** (base-image staples). Method
signals worth knowing: (1) agents **edit files ~2:1 over running shell commands**,
and the dominant shell idiom is "write a python repro script, run it, `rm` it" —
so a solid coreutils + python + curl/grep/sed/openssl core matters more than tool
breadth; (2) `git` is **rare inside agent turns** (harnesses extract the diff
out-of-band) but stays essential; (3) a **long tail of project-specific CLIs**
(`dvc`, `sqlglot`, `sanic`, …) comes from pip/npm install, not the registry.

**Requested (add):** ssh 🟡, rsync 🟡, tmux/screen 🟡 (PTY — session persistence),
gpg 🟡, ffmpeg 🟡 (media transcode — heavy but headless), jj 🟢, dig 🟡,
nslookup 🟡, less ⭐🟡 (pager), openssl ⭐🟡 (TLS/certs/keys/hashing).
tail/head/cat are already in coreutils — confirm present.

**Text / stream:** less ⭐🟡, **perl** ⭐🟡 (ubiquitous `-pe`/`-ne` text munging —
big C runtime but real; 563 uses in history), miller `mlr` 🟢 (CSV/JSON),
xmlstarlet 🟢, pcre2grep 🟢. (jq/yq/sed/awk/grep/head/tail already covered.)

**Networking (host TCP/DNS bridge only):** openssl ⭐🟡, ssh 🟡, nc/netcat 🟡
(TCP/UDP), socat 🟡, whois 🟢, dig/nslookup 🟡, redis-cli / psql client 🟡,
aria2 🟡 (C++ downloader), sshpass 🟢 (ssh password helper).

**VCS:** git (item above), jj 🟢.

**Crypto:** gpg 🟡, openssl 🟡, age 🟢 (Rust), minisign 🟢.

**Compression:** xz, zstd, bzip2, lz4, brotli, p7zip (7z) — all ⭐🟢, common + easy.

**Media / image:** ffmpeg 🟡 (transcode), imagemagick ⭐🟡 (C, image ops — high
popularity; agents do image work).

**Files / sync:** rsync 🟡, diff/patch 🟢, rename 🟢, fdupes 🟢 (find/fd tracked
above).

**Session:** tmux/screen 🟡 (PTY).

**Process management (add — real procps-ng + psmisc, C; ps/pkill/pgrep ≈ 3K uses):**
- **Need the `/proc` prerequisite (below):** ps, pgrep, pkill, pidof, pstree,
  killall, uptime, free, vmstat, w, pwdx, pmap 🟡.
- **Signal-only — already work via the kernel (no /proc):** kill, killall-by-PID.
  (kill, sleep, timeout, env, nohup, nproc, nice/renice are coreutils — confirm
  they're shipped via uutils rather than re-adding.)

**⚙️ Runtime prerequisite — implement `/proc` (process-table-backed):** procps
reads `/proc/<pid>/{stat,cmdline,status,comm}` and enumerates `/proc/<pid>/`. The
**kernel already owns the process table** (`crates/kernel/src/process_table.rs`),
so expose a read-only procfs view of it to the guest (per-PID stat/cmdline/status
+ directory enumeration). Scope it minimal — just the fields procps parses, backed
by the existing process table, not a full Linux procfs. Unlocks the whole
ps/pkill/pgrep family (and top/htop later if ever wanted). **This is a runtime/VFS
item, do it before the procps packages.**

**Excluded — not worth it / not possible here:**
- **TUI / visual-only:** gitui, lazygit, eza, dust, ncdu, bat, delta, broot, k9s,
  skim/fzf — a terminal UI has no agent value.
- **top / htop — excluded (TUIs).** (ps/pkill/pgrep and the rest of procps-ng +
  psmisc are ADD items above, gated on the `/proc` runtime prerequisite.)
- **Raw sockets:** ping, traceroute, mtr, nmap (need raw/ICMP, not just TCP).
- **ptrace:** strace, ltrace, gdb, lldb, valgrind — genuinely impossible on WASI.
- **Dev / build toolchains:** make, cmake, clang/gcc, binutils, pkg-config,
  ctags — out of scope.
- **Go-only:** rclone, gh, kubectl — no C/Rust equivalent.
