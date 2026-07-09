# Handoff — Registry networking (TLS) + ssh + /proc + remaining work

Status: execution handoff (state reconciled) · Last updated: 2026-07-09 (PST) · Read this first, then
`docs-internal/networking-parity-spec.md` and `docs-internal/registry-parity-worklist.md`.

This picks up an in-flight effort: give curl/wget/git **real HTTPS with native
Linux semantics**, make the **codex build reproducible**, and then continue into
**ssh**, **/proc**, and the Node stdlib migration. Current proof state:
- curl/wget/git HTTPS is **done and green** on in-guest mbedTLS.
- ssh is built and its direct suite is **7/7 green**, but the git-over-SSH
  clone/push test is opt-in and blocked by a runtime synthetic-pipe + host-net
  poll/flush bug (§4); the ssh rev is not yet forklifted.
- the kernel already has a **partial synthetic procfs**, but it has not met this
  program's acceptance bar: no authoritative-format citations at the emitters,
  no real-Linux golden fixture, no real procps-ng/psmisc packages, and no real
  `ps`/`pgrep`/`pkill` e2e (§5).
- the Node replacement spec is ready in the separate `node-stdlib` workspace;
  its final backend decision is real OpenSSL 3.5.5 wasm, not an invented
  mbedTLS-backed OpenSSL façade (§5).

---

## 0. The two original acceptance criteria (BOTH MET)

1. **Forklift cleanly pushed** — the whole stack is on GitHub as PRs **#1661–#1717**
   (52 revs: all real-tool replacements, codex reproducible build, the networking
   spec, mbedTLS+CA, curl/wget/git TLS, and the VFS-deletion fix).
2. **curl + wget + git have real HTTPS with native Linux semantics** — verified:
   - **curl 32/32** — `libcurl/8.11.1 mbedTLS/3.6.2 zlib/1.3.1 brotli/1.1.0 zstd/1.5.6`; cert-fail → **exit 60**, `--cacert`, `--compressed` (gzip/br/zstd).
   - **wget 12/12** — `+ssl/mbedtls`; cert-fail → **exit 5**, `--ca-certificate`, gzip.
   - **git 25/25 (smart-HTTP 8/8)** — HTTPS clone/fetch/**push (small & >1 MiB)**/cert-verify/`sslVerify`/`sslCAInfo`/clone-after-push all pass.

Everything after this (ssh, /proc, etc.) is **additional** work the user directed.

---

## 1. Architecture you must understand

**Sockets/DNS/HTTP were already real** (BSD sockets over the `host_net` WASM-import
bridge — `toolchain/std-patches/wasi-libc/0008-sockets.patch`,
`0023-host-net-*`; runner `crates/execution/assets/runners/wasm-runner.mjs`
forwards to the sidecar socket table). The only gaps were **TLS + decompression**.

**The implemented TLS model for the original registry milestone (do NOT revert
to host-brokered):** in-guest **mbedTLS 3.6**
+ a Debian-shaped **CA bundle at `/etc/ssl/certs/ca-certificates.crt`**. Real
verification happens *in the tool's own code* → correct exit codes, `--cacert`,
`-v` cert chain. The old host-brokered `wasi_tls.c` shim (host cert store, exit 35,
ignored `--cacert`) is **deleted** and documented as refuted-for-parity in
`networking-parity-spec.md`.

**Final convergence target (decided after the Node review):** preserve the
in-guest model and VM CA bundle, but migrate TLS/crypto consumers forward to
one real OpenSSL 3.5.5 wasm build owned by `toolchain/c`. Node 24.15.0 requires
OpenSSL 3 EVP/provider/error behavior; mbedTLS 3.6 does not ship the proposed
OpenSSL 3 compatibility surface. The existing mbedTLS revisions remain valid
historical proof and are not rewritten. The convergence revs rebuild
curl/libcurl, wget, git, and OpenSSH against the shared OpenSSL archives, remove
`wasi_ssl.c` and the host `rustls-native-certs` path, and prove the shared CA
behavior cross-runtime (§5 and the Node spec §7.4).

**ssh is different from TLS:** TLS is a *library linked in-process*; **ssh is a
standalone command other tools spawn as a subprocess** (git execs `/opt/agentos/
bin/ssh`). There is no "link ssh into git."

**Foundation artifacts (built, in the stack):**
- `make -C toolchain/c mbedtls` → `toolchain/c/build/mbedtls/{libmbedtls,libmbedx509,libmbedcrypto}.a` (TLS 1.3, `getentropy` entropy, `mbedtls_x509_crt_parse_file`).
- `make -C toolchain/c ca-certificates` → Mozilla bundle; VM bootstrap
  (`crates/native-sidecar/src/vm.rs`) seeds it at `/etc/ssl/certs/ca-certificates.crt` (+ `/etc/ssl/cert.pem` symlink).
- zlib already built (`toolchain/c/build/zlib/libz.a`); brotli + zstd added for curl.

---

## 2. The bug cascade we fixed (so you don't re-discover them)

Real e2e (not "build verified") is what caught these. All fixed + tested:
- **git `fork()` in fetch-pack sideband demux** (no fork on WASI) → synchronous
  spool-to-tempfile + index-pack (`toolchain/c/patches/git/0002-*.patch`).
- **kernel fd-CLOEXEC inheritance** — a grandchild inherited its own stdin-pipe
  writer → no EOF → hung (`crates/kernel/src/fd_table.rs`, `execution.rs`).
- **spawn cwd** resolved PWD-first → stale cwd (`toolchain/std-patches/wasi-libc/0012-posix-spawn-cwd.patch`).
- **runner dropped `SOCK_NONBLOCK`** → curl `recv()` blocked → deadlock on uploads
  >16 KiB (one TLS record); + socket fds above `FD_SETSIZE`. Fixed in
  `wasm-runner.mjs` (parse `SOCK_NONBLOCK`, `fd_fdstat_get/set_flags` for sockets,
  fd base 4096→600). **This same fix is why ssh's select loop works.**
- **stack overflow in `recv_sideband`** (64 KiB buffer on git's default 64 KiB wasm
  stack) → link git with `-Wl,-z,stack-size=8388608 -Wl,--stack-first`.
- **sidecar truncated >64 KiB child stdin** (ignored `fd_write` partial writes) →
  non-blocking stdin + a 64 MiB backlog queue (`execution.rs`, `state.rs`).
- **VFS deletions never propagated** (systemic): `OverlayFs::remove_file` followed
  symlinks instead of lstat (git's dangling symlink probe → `.git` never emptied);
  `rmdir` mapped `ENOTEMPTY`→`EIO`; shadow→kernel sync was additive-only. Fixed with
  lstat removal + `ENOTEMPTY` mapping + an inventory-diff deletion-reconcile
  (`crates/vfs/src/posix/overlay_fs.rs`, `wasi-module.js`, `execution.rs`).
- **curl `-o/-O` "failures" were a red herring** — stub `cat`/`wc` test-programs
  shadowing real coreutils, now renamed `c-*`.

---

## 3. State of the jj stack (CRITICAL — read carefully)

The whole thing is one linear jj stack managed with **forklift** (`forklift submit -y`
rebases onto trunk + pushes each rev as a stacked PR). `reg-tests` is the workspace.

**Pushed (PRs #1661–#1717):** all tool replacements, codex reproducible build
(#1708), networking spec (#1709), mbedTLS+CA (#1710), curl (#1711), git (#1712),
wget (#1713), git-fork-fix (#1714), SOCK_NONBLOCK (#1715), index-pack/stack/stdin
(#1716), VFS-deletion (#1717).

**Committed locally but NOT yet forklifted (do this first):**
- `yvxuokln` — `docs(CLAUDE): require citing authoritative specs at the implementation site`
- `opqnkltx` — `feat(ssh): real OpenSSH client …` (**built + 7/7 e2e; see §4**)
- `lnqvznlo` — this handoff doc.

**To push them:** `cd` to the workspace, ensure `@` is at the top rev, then
`forklift submit -y`. If forklift reports many conflicts on rebase, **they're
almost always just 2 workflow files** (`.github/workflows/ci.yml` + `publish.yaml`)
conflicting from the flatten vs main's ci changes — resolve at rev `wktwwvso`
(keep the flatten's `toolchain` paths + main's `cache-workspace-crates: true`) and
all descendant conflicts collapse. (We hit "47 conflicts" that were really these 2.)

**jj discipline (shared workspace, ~100 workspaces + concurrent agents):** one
fix per rev; `jj describe` each; never `jj edit`/`rebase`/`abandon` to *inspect*;
verify `jj diff -r @ --summary` has **no build-artifact leakage**
(`toolchain/c/build/**`, `**/target`, `.cache` extracted trees, `node_modules`,
`.a`) before describing — those are gitignored, only source/patch/test/vendored-cmd
files belong in a rev.

---

## 4. ssh — BUILT + VERIFIED, needs forklift + one gap closed

Rev `opqnkltx` (`feat(ssh)…`). **Real OpenSSH portable 10.4p1, `--without-openssl`**,
built for wasm32-wasip1 (845 KB, at `software/ssh/bin/ssh`, `packages/runtime-core/
commands/ssh`, `toolchain/c/build/ssh`). Files: `toolchain/c/scripts/build-ssh-
upstream.sh`, `toolchain/c/Makefile` target, `toolchain/std-patches/wasi-libc/
0029-openssh-compat-header-surface.patch`, `toolchain/std-patches/wasi-libc-
overrides/openssh_compat.c` (the no-op `closefrom` + ENOSYS `socketpair`),
`software/ssh/*`, `wasm-runner.mjs` setsockopt polish.

**Verified — ssh e2e 7/7** (`~/progress/agent-os/2026-07-09-openssh/…-ssh-suite-
final.log`): ed25519 publickey auth + known_hosts; exit-status propagation;
unauthorized-key failure; host-key-verification failure; BatchMode fail-closed on
unknown host; `StrictHostKeyChecking=accept-new`. **git 25/25 unregressed.**

**Gap to close (state reconciled):** the git-over-SSH clone/push test now exists
at `software/ssh/test/ssh.test.ts`, but is opt-in behind
`AGENTOS_SSH_GIT_E2E=1`. It reaches the real ssh binary, completes KEX/auth, and
opens the session; it then hangs because child-spawn synthetic pipes and a
host-net socket are polled together but the queued exec channel request is not
flushed. Fix that runtime poll/write interaction in the owning runtime layer,
remove the opt-in gate, and require clone + push green by default. Do not weaken
the test or patch git/OpenSSH around the runtime bug.

**Build/test commands:**
```
make -C toolchain/c build/ssh          # deps: mbedtls not needed (no openssl); zlib + patched sysroot
pnpm --dir software/ssh test           # with AGENTOS_SIDECAR_BIN pinned (see §6)
```

---

## 5. Remaining queue (normative order)

### 5.1 Finish the reg-tests stack

1. **Close git-over-SSH:** fix the runtime synthetic-pipe + host-net poll/flush
   bug described in §4, remove `AGENTOS_SSH_GIT_E2E`, and make real clone + push
   pass by default with a pinned `AGENTOS_SIDECAR_BIN`.
2. **Finish `/proc` from the existing partial implementation:** retain the
   process-table-backed design in `crates/kernel/src/kernel.rs`, then add the
   missing consumer surface (`/proc/<pid>/comm`, `/proc/stat`, and any further
   fields/files demonstrated necessary by procps-ng), correct `stat` field order
   and `comm` escaping, and cite `proc(5)`, Linux `fs/proc/array.c`, and procps-ng
   `readproc.c` at the emitters.
3. **Prove and ship the consumers:** add a captured real-Linux golden fixture and
   regeneration script; build real upstream procps-ng + psmisc with the patched
   sysroot; ship `ps`, `pgrep`, `pkill` (plus directly unlocked commands); run
   those real commands against live guest processes. Unit tests alone do not
   satisfy this item.
4. Keep the handoff/worklist status current after every rev, audit for build
   artifact leakage, and `forklift submit -y` each clean change before leaving
   this workspace.

### 5.2 Drive the Node-stdlib/wasm migration in its own workspace

Spec: `~/.herdr/workspaces/agent-os/node-stdlib/docs-internal/
node-stdlib-replacement-spec.md`. Do not implement it in this reg-tests working
copy. The Node program begins after §5.1 is green and forklifted.

**Final crypto/TLS decision:** build the exact OpenSSL 3.5.5 source bundled by
Node v24.15.0 once in `toolchain/c`, producing content-addressed wasm32
`libcrypto.a`/`libssl.a` archives. Node adapters and registry consumers link
those same archive/source hashes. Migrate curl/libcurl, wget, git, and OpenSSH
forward from the proven mbedTLS/no-OpenSSL state; do not rewrite the published
history and do not retain a second TLS backend or `wasi_ssl.c`-style adapter.

**Required unification proof:**
1. Captured build/link manifests show every consumer linking the same
   OpenSSL archive/source hashes (each final static wasm may embed its copy).
2. One cross-runtime e2e drives curl and Node `https.get`/`tls.connect` against
   the same trusted and self-signed servers: trusted succeeds, self-signed
   fails in the same class, and `--cacert`/`NODE_EXTRA_CA_CERTS` work against
   the same `/etc/ssl/certs/ca-certificates.crt`.
3. The host `rustls-native-certs`/TLS-upgrade path, mbedTLS TLS backend,
   per-tool adapters, and RustCrypto bridge crypto are retired when the shared
   OpenSSL path becomes the only one.
4. OpenSSH is rebuilt with the shared OpenSSL for its normal full-crypto
   configuration. OpenSSH-owned protocol primitives are not a second TLS
   backend.

**Explicitly out of scope:** wasm threads and the disabled browser runtime.
If real OpenSSL hits a concrete wall, exhaust sysroot and upstream OpenSSL
patches first, save the failing e2e/build evidence, and update both specs with
the refuted thesis before requesting a different architecture.

**Doc-citation convention (now in CLAUDE.md, enforce it):** every format emitter /
protocol handler cites its authoritative reference **in a code comment at the site**
— man page, kernel source path, RFC, and/or the consumer's parser. For /proc:
`proc(5)`, `fs/proc/array.c`, procps `readproc.c`. For ssh patches: RFC 4251–4254.

---

## 6. Operational knowledge you WILL need

**The shared workspace is churned in real time** by other sessions —
`node_modules`, `target/`, and the cargo `cc-1.2.66` crate (`src/target/`) get wiped
mid-run, breaking vitest (`@vitest/utils`/`tinypool` not found) and racing the
sidecar `cargo build`. **This is not your code.** Workarounds that worked:
- Before any e2e: `pnpm install --frozen-lockfile`; if `cc` is broken, re-extract its `.crate`.
- **Pin a prebuilt sidecar** so tests don't trigger a racing `cargo build`:
  `AGENTOS_SIDECAR_BIN=<stable path>`. Recent good binaries in the scratchpad:
  `/tmp/claude-1000/-home-nathan--herdr-workspaces-agent-os-reg-tests/91e4450c-fb78-4e8b-a128-eff26631dc40/scratchpad/{sidecar-vfs-deletion-fix,agentos-native-sidecar}`.
  If you change Rust, rebuild the sidecar once (`cargo build -p agentos-native-sidecar`)
  to a stable copy and pin THAT.
- The git HTTPS/ssh suites stand up a loopback server (git: `git http-backend`;
  ssh: the `ssh2` npm package) and use `createGitKernelWithNet` — mirror that pattern.

**Build gotcha:** wasi-sdk 25's clang auto-runs `wasm-opt` from PATH after link,
stripping the wasm name section (kills symbolized traps). Build with
`PATH=/usr/bin:/bin` when you need to debug a wasm crash address.

**codex reproducible build:** `make -C toolchain codex` (CODEX_REPO unset → clones
`rivet-dev/codex@<toolchain/codex-ref>` + injects committed patches). Verified the
hard crates compile; **one open blocker for the full 29 MB artifact**: `rmcp` 0.15
oauth transport wants a `Send` future the reqwest-shim doesn't provide (scoped fix:
make the shim future `Send` or cfg-gate rmcp oauth off wasi).

---

## 7. Key file map

- TLS/CA foundation today: `toolchain/c/Makefile`
  (mbedtls/brotli/zstd/ca-certificates targets),
  `crates/native-sidecar/src/vm.rs` (CA seeding), `build.rs`. Final shared
  OpenSSL ownership/layout is normative in the Node replacement spec §9.4.
- curl: `toolchain/c/scripts/build-curl-upstream.sh`, `software/curl/{test,native}`.
- wget: `toolchain/c/scripts/build-wget-upstream.sh`, `software/wget/native/c/overlay/src/wasi_ssl.c` (mbedTLS backend), `software/wget/test`.
- git: `toolchain/c/scripts/build-git-upstream.sh`, `toolchain/c/patches/git/000{1,2}-*.patch`, `software/git/{test,agentos-package.json}`.
- ssh: `toolchain/c/scripts/build-ssh-upstream.sh`, `toolchain/std-patches/wasi-libc-overrides/openssh_compat.c`, `toolchain/std-patches/wasi-libc/0029-*.patch`, `software/ssh/*`.
- Runtime fixes: `crates/execution/assets/runners/wasm-runner.mjs` (sockets/stdio/spawn), `crates/native-sidecar/src/execution.rs` (stdin backlog, TLS), `crates/kernel/src/{fd_table,process_table,kernel}.rs`, `crates/vfs/src/posix/overlay_fs.rs`.
- Procfs baseline: `crates/kernel/src/kernel.rs`,
  `crates/kernel/src/process_table.rs`, `crates/kernel/tests/{identity,api_surface}.rs`.
- Specs/state: `docs-internal/networking-parity-spec.md`, `docs-internal/registry-parity-worklist.md`, this file.
- Friction log (root causes + repros): `~/.agents/friction/agentos.md`.
- Proof logs: `~/progress/agent-os/2026-07-0{8,9}-*/`.

---

## 8. Immediate next steps for the picking-up agent

1. Forklift the pending policy/ssh/handoff revisions, preserving the shared
   workspace and resolving only the known workflow conflicts if they recur.
2. Fix the git-over-SSH runtime blocker and make clone + push unskipped and green.
3. Complete and prove `/proc`, then ship real procps-ng/psmisc commands.
4. Forklift the completed reg-tests revisions and update this status section.
5. Switch to the existing `node-stdlib` workspace without moving `reg-tests`'s
   `@`; execute the Node spec M0→M5 with the shared real-OpenSSL decision.
6. Optional after the ordered work: git credential-cache note and the codex
   `rmcp` OAuth `Send` blocker.

Everything real-tool must stay **real upstream, patch the sysroot not the app**
(CLAUDE.md → Software Build). Prove every claim with a **real e2e**, not a build
check — that discipline is what caught every bug above.
