# Executor refactor rebase validation

Status: local correctness/security validation passes; measured WASM performance retains a small regression. A stdin-close lifecycle race found by CI is fixed with a failing-first regression. [PR checks](https://github.com/rivet-dev/agentos/pull/2040/checks) carry the latest remote results.
The final all-targets check, six launch-source tests, 15 filesystem tests, 39
builtin conformance tests, and 15 Core smoke tests completed successfully.
The complete V8 unit suite passes 246 tests, including the warm-refill test in a
fresh process for its custom runtime configuration. The paired release comparison
is complete; it does not establish zero performance regression (see below).

The refactor is rebased onto main `c7462927ec45` (34 commits after its previous base
`5d299eeb`). V8 remains the default engine; Wasmtime and Binaryen remain opt-in.
The table below tracks the mainline behavior that must survive the crate and
TypeScript package restructuring. A ported source path is not a test result;
the last column distinguishes the two.

## Verified so far

- Full Rust workspace/all-targets checks and frozen pnpm installation pass.
- Workspace/all-targets Clippy passes with warnings denied.
- Package build: 43 tasks passed; typechecks: 97 tasks passed.
- Publish/layout/protocol/resolver helper tests: 26 passed.
- Minimal embedded VM dependency boundary passes: 42 forbidden packages absent.
- Embedded VM binary runs and measures 775,824 bytes (1 MiB ceiling).
- Executor feature dependency matrix, repository layout, and fixed versions pass.
- V8 runtime unit tests: 246 passed, including bounded ArrayBuffer allocation tests.
- Resource accounting: 16 passed, including failed admission without false wakeups.
- Kernel PTY: 29 passed; driver capability: 15 passed.
- Focused Core transport tests: 42 passed; Rust client process unit tests: 18 passed.
- Final Core unit run: 205 unique tests passed, with three saturation tests
  skipped by default; secure-exec: 17 passed. Registry resolution: 21 passed.
- Focused Python exit and partial-output cases, both context-handle cases,
  and Core fetch/host-global cases passed on the final runtime.
- Complete generated software rebuild succeeds with `AGENTOS_ENABLE_BINARYEN=0`.
- Security-hardening suite passes host environment/command escape and configured resource checks.
- Thirteen process/event-pump tests pass, including full-queue child exit handoff, cancelled write cleanup, and retained payload accounting.
- Rust client process E2E: 14 passed, one ignored; shell/terminal E2E: eight passed.
- Explicit memory-security run: eight passed, including the three opt-in probes.
- API-seeded file reconciliation passed for JavaScript, Python, and shell writes;
  the strict JavaScript disposal regression now passes without a teardown warning.

- Full source package creation produced five publishable tarballs with all 169
  commands staged (163 manifest-required names); committed versions were restored
  and verified at 0.0.1 afterward. Pipboy typechecks and both footprint tests pass.
- Native CI repairs use the portable interface-name lookup on macOS/Linux and
  copy the renamed sidecar artifact. The focused interface/error test, formatting,
  VM runtime library check, and full workspace Clippy pass locally.
- All four native CI builds now pass (Linux and macOS, x64 and arm64).
- Architecture guards pass 79/79. A failed PTY window-size read now takes the
  startup rollback path before starting the executor. Trusted client registry
  resolution has explicit host-access allowances; test-only thread helpers
  remain guarded as test-only.
- Removed an unused sidecar test dependency that pulled in the unmaintained
  `serde_cbor` crate. The required `wast` fixture dependency has an exact-version
  exception for its upstream Apache/LLVM license. Remote hygiene now passes.
- Formatting uses Rust 1.98.1 in both the Checks and Rust jobs; previously the
  floating formatter upgraded independently and disagreed with the build job.
- Remote Rust validation passes Clippy, all architecture guards, sidecar tests,
  and the selected Rust client contracts. Core transport, executor isolation,
  WASM commands, and the benchmark gate pass. Core runtime passed 204 tests but
  exposed a fast-process exit racing with stdin close. The new route-level
  regression fails without the correction and passes with it; the existing
  write-after-close regression also passes. Unknown IDs, unconfirmed exits, and
  expired records still fail. This reuses the existing two-second exit-record
  lifetime and adds no retained state; it does not promise indefinite handle
  validity. The rebuilt release sidecar passes all 18 affected host-function
  cases and the complete Core unit suite (205 passed, three default skips).
  Follow the PR checks for the corrected remote run.
- A later remote run passed the stdin-close case but exposed a transport-ordering
  race: an `EPIPE` write callback could precede the process-exit observer and
  escape as an untyped error. Request, callback-response, and shutdown-control
  writes now share explicit fatal-error classification, retaining the original
  cause and invalidating the dead shared transport. Nonfatal request failures
  remain nonterminal. Failing-first ordering regressions, the live exit tests,
  and the focused transport suites pass (34 tests); Core typechecking passes.

## Rebase regressions found by tests

- Preserve typed ENOENT/ENOTDIR errors through the executor contract for `process.chdir`.
- Preserve PTY slave partial writes while retaining the master input allocation bound.
- Reject failed hierarchical resource reservations atomically, without waking waiters.
- Restore bounded ArrayBuffer allocation and external WASM memory enforcement.
- Restore child exit event admission before cleanup when the public queue is full.
- Charge process-global V8 warm-pool refills to the process blocking ledger so
  VM disposal does not quarantine while another warm isolate is still building.
- Symlink/parent-component and trailing-slash filesystem parity: full VFS suite and all 39 guest builtin conformance tests pass.
- Restore readdir type codes for files, directories, and symlinks across the Rust/JavaScript boundary.

## Security review evidence

Three review passes covered mainline security fixes, the new executor request/reply
ownership, and cancellation/resource cleanup. Confirmed findings were fixed with
regression tests: per-isolate backing-store limits, atomic resource admission,
full-queue child exit delivery, parked socket write cancellation, and retained
payload accounting. The real 6 MiB socket-write case failed before its fix and
passes afterward. Core permission tests (16), the event broker suite (8), security
audit tests (6), permission-flag tests (6), and session isolation tests (3) passed.

The subsequent snapshot optimization retains immutable backing bytes until the
last isolate drops them. Its ownership assertion failed before the change; tests
now cover cache-owner release, final allocation release, and separate mutable
guest state. Independent review checked the pinned rusty_v8 ownership contract.
These checks provide evidence for the exercised boundaries, not a claim that the
runtime has no security defects. Warm-refill review found process-global work charged to the VM ledger. The fix retains process-level admission, rejects refills after pool shutdown, and passes a bounded ownership/shutdown regression. Staging review also reproduced a stale parent symlink redirecting a later projection write. Both JavaScript and WASM staging now validate the private root and replace stale ancestors without following them. The failing-first containment regression and the other focused launch tests pass.

## Per-commit checklist

The status reflects checks run on this branch; source inspection alone does not establish runtime parity.

| Commit | Active port or reconciliation | Verification/status |
|---|---|---|
| `c7462927` closed loopback ECONNREFUSED | `vm` TCP emits `ECONNREFUSED` for closed VM loopback | Cross-runtime loopback nightly and kernel socket permission tests passed. |
| `63e0da64` drain large socket writes | `vm` managed write uses bounded recheck on socket-budget release; kernel and driver reservations retained | 6 MiB guest regression and 13 focused managed-write cancellation/lease tests passed. |
| `fe26d665` cwd/symlink/rm Linux semantics | `vm-kernel`, `vm` filesystem, bridge FS/process changed | Builtin conformance 39/39 passed, including cwd/symlink/rm/readdir; full VFS suite passed. |
| `b815ecdf` atomic reservations | `driver-tokio` accounting/capability moved with all-or-nothing admission | Resource accounting 16 tests passed. |
| `3c60c9aa` WASM memory heap stop | V8 external-memory guard and bridge tracker ported | V8 limits and all eight opt-in memory-security cases passed; saturation probes remain skipped by default. |
| `a959ad46` ArrayBuffer cap | `executor-v8-runtime` per-isolate budget installed on fresh and snapshot paths | V8 allocator tests passed. |
| `80f81c6c` self-host image tag | `docs/content/docs/self-hosting.mdx` uses `rivetdev/engine:next` | Source inspected; rendered docs build unavailable in this checkout. |
| `ae5e5cd4` fs.rm symlink | Bridge FS and VM filesystem ported | Symlink removal passed in builtin conformance 39/39 and the full VFS suite. |
| `89e4a301` bare-ts 0.4.1 | Core uses `@bare-ts/lib`; toolchain uses forked `@rivetkit/bare-ts` | Active fork passed 256 KiB BARE frame regression; no version-number equivalence assumed. |
| `893d7fe8` API-seeded file writes | VM filesystem preserves writes over seeded shadow content | JavaScript and Python/shell reconciliation regressions passed; strict JavaScript disposal passed after fixing process-wide warm-refill ownership. |
| `86a0d369` oversized frames | Core frame writer checks negotiated max and throws `SidecarFrameTooLarge` | Focused transport 42 tests passed. |
| `cfb28d91` Python partial output | Active Python runner keeps output without final newline | Focused Python no-trailing-newline regression passed. |
| `02c0aaf1` released context handles | V8/bridge active-handle release path ported | Released-context-socket regression passed. |
| `3758f301` unread fetch body liveness | Active undici stream shim ported | Core network VM fetch suite 2/2 passed. |
| `fac5e9a3` heap trim after disposal | `executor-v8-runtime/src/native_heap.rs` owns `malloc_trim` | Paired RSS results record pre/live/disposed usage; retained-context disposed RSS is lower than main. |
| `9413c84e` Python SystemExit codes | Active Python runner maps `SystemExit` to Linux exit status | Focused Python exit-status regression passed. |
| `e2f2e948` HTTP server unref | Bridge HTTP server forwards `unref()` to underlying net server | HTTP server unref case passed within builtin conformance 39/39. |
| `b9ce935d` settled context calls | V8/bridge context completion and handle release ported | Context-settlement and released-socket regressions passed. |
| `1536575f` disposed event buffer | Core protocol/event buffer clears disposed VM entries | Focused transport tests and Core sidecar-exit 5/5 passed. |
| `547c2952` drop unused deps | Flattened Core manifest reconciled | `pnpm build` and 97/97 typechecks passed. |
| `fe26223a` drop googleapis | Flattened Core manifest and lock reconciled | `pnpm install --frozen-lockfile` passed. |
| `8cc8f8bb` Pipboy CI insight | Pipboy workflow and scripts retained | Source inspected; PR workflow execution pending. |
| `6ca130a2` software by name | TS/Rust registry resolvers and VM software source forwarding ported | Core registry 21/21 and software-install 4/4 passed; Rust client all-target check passed. |
| `bd3f6f44` software docs/examples | Embedded software examples and software docs ported | Checked examples, snippet files/regions, and frontmatter validated; rendered site unavailable. |
| `648b92f8` local network under deny | VM permission profile preserves local IPC/loopback under scalar deny | Core network-deny-vm-local 2/2 passed. Branch default also denies external model-provider DNS by design. |
| `3c5eff73` host globals in child Node | VM child process host-function globals ported | Final Core unit passed on refreshed sidecar; initial pre-fix failure is resolved. |
| `16099732` release/nightly CI runners | Release and nightly workflows use GitHub-hosted runners | Source inspected; workflow execution pending. |
| `d7d23651` interactive terminals | `vm-kernel` PTY and VM terminal paths ported | Kernel PTY 29 tests and shell/terminal E2E eight tests passed. |
| `c8c5b169` trap signal dispositions | Brush patch 0005 reconciled after main 0004; toolchain staged | Full Binaryen-disabled tools rebuild and both shell trap E2E cases passed. |
| `a00bbaf9` Linux stdin semantics | Rust client and Core process API ported; VM stdin paths retained | Rust client stdin E2E 4/4 and process E2E 14 passed (one ignored); Core unit suite passed. |
| `8fa18f9f` V8 tracing diagnostics | V8 session uses tracing for warm-pool and snapshot diagnostics | Tracing calls preserved and exercised by runtime tests; no dedicated log-format assertion. |
| `0ec5f928` shared sidecar recovery | Rust and Core sidecar clients ported | Rust sidecar-crash E2E passed; Core sidecar-exit 5/5 passed. |
| `470af5cb` stdin/poll/pipe stalls | VM child process and JS poll/pipe paths ported | Rust stdin E2E and delayed pipeline-reader case passed; the 30-second saturation case remains ignored by default. |
| `4d486283` README/quickstarts | README and docs examples reconciled with flattened package graph | Checked examples, snippet files/regions, and frontmatter validated; rendered site unavailable. |

## Performance results

Six alternating release trials (three per revision), with 12 measured iterations
and three warmups per trial, use identical packed WASM bytes. Lower is better.
Both measured native binaries were built with Rust 1.97.0. The later EOF-close
fix changes a post-exit race path and was not included in these timed binaries.

| Workload | Main median (ms) | Candidate median (ms) | Change | Main / candidate p95 (ms) |
|---|---:|---:|---:|---:|
| Cold JS (VM creation + first execution) | 289.23 | 281.76 | -2.6% | 297.88 / 290.38 |
| Warm JS | 12.05 | 11.21 | -7.0% | 13.50 / 12.89 |
| Serial JS per operation | 12.67 | 11.15 | -12.0% | 13.47 / 12.90 |
| Concurrent JS per operation | 8.30 | 8.20 | -1.2% | 8.97 / 8.82 |
| Host callback | 5.00 | 4.98 | -0.3% | 5.11 / 5.16 |
| First WASM command | 84.03 | 84.92 | +1.1% | 88.31 / 88.24 |
| Warm WASM command | 37.30 | 38.21 | +2.4% | 39.75 / 42.00 |
| WASM filesystem command | 115.78 | 121.04 | +4.5% | 120.64 / 125.74 |

Retained JavaScript-context RSS slope is 24.90 MiB/VM on main versus 19.13 MiB/VM
on the candidate (23% lower), fitted across 3–12 VMs. Whole process-tree RSS with
12 retained contexts is 681.43 versus 578.61 MiB; after disposal it is 424.54 versus
392.15 MiB. Idle-VM slopes are 0.21 versus 0.22 MiB/VM. These figures include the
Node benchmark client and sidecar descendants.

The original warm WASM regression (37.47 to 61.62 ms) was caused by rewriting the
same staged module on every launch, invalidating compilation warmup. Preserving
unchanged modules removes most of that regression. A subsequent review removed
a redundant full-image read from the admission probe; staging still performs the
bounded authoritative read and containment checks before executor startup.
The six staging and five kernel image tests pass after that change.

**The final run retains a 2.4% warm WASM median increase and 5.7% p95 increase;
WASM filesystem median is 4.5% higher.** The preceding six-trial run measured
2.7% and 2.8% median increases respectively, so the extra probe optimization
has not established parity. These results must not be presented as proof of
zero regression. The runtime keeps fresh image validation rather than trusting
an unchanged pathname as evidence of unchanged guest contents.

The host runs unrelated background services, but this task ran no concurrent
builds or tests during measurement. These are focused runtime workloads with
default software disabled and a shared coreutils artifact for the WASM cases;
they do not establish performance for every registry application. P95 uses only
36 samples per lane and should be interpreted with that limited sample size.

[Raw final results](../../packages/benchmarks/results/v8-default-vs-main-20261030.json)
include sample arrays, binary/package hashes, and revision identifiers.
[The preceding run](../../packages/benchmarks/results/v8-default-vs-main-20261029.json)
is retained for comparison.
[Before-fix results](../../packages/benchmarks/results/v8-default-vs-main-before-fixes-20261029.json)
preserve the initial regression. The runner and reproduction command are in the
[benchmark README](../../packages/benchmarks/README.md).

## Remaining gates

- Full footprint measurement still fails its unchanged sandbox install-size
  budget, but mixes the current source tarball with the historical published
  sidecar: 594.6 MiB for the registry Linux binary versus about 151 MiB for the
  local unstripped candidate binary. Reported closures are 844.0 MB for Core and
  881.7 MB for the sandbox wrapper. This is not a complete candidate release-size
  measurement. The report now labels native provenance; no budget was raised and
  no required runtime assets were removed. See the [Pipboy notes](../../scripts/pipboy/README.md).
- Full source footprint builds are explicit on PRs through `ci: full-footprint`
  and still run on main pushes. This keeps the complete source build in the
  expensive validation phase while preserving strict package completeness.
- Required CI must be timed on the candidate. Recent successful mainline PR runs
  took 14m00s and 15m18s end to end; the WASM Commands job alone took 12m25s
  and 13m38s on `ubuntu-latest`. The candidate builds the default package set
  once on the existing eight-core Depot runner. Its first remote run completed
  WASM Commands in 3m18s, including setup, packaging, transfer, and cleanup
  (command compilation: 2m36s). Checks exposed a stale TypeScript re-export,
  corrected to the local module. Pipboy also required a compatible transitive
  Effect dependency pin; its typecheck and self-tests pass. Follow the PR checks
  for the complete corrected run and its end-to-end duration.
- The separate benchmark job exceeded the 10-minute target while building its
  own command corpus. Recent successful runs took 22m15s for a PR and 15m03s /
  15m35s on scheduled main. PR benchmark runs now use the existing eight-core
  runner; scheduled runs retain their existing configuration. The next PR run
  passed in 5m23s. WASM Commands passed in 3m08s and executor isolation in 5m23s.
- The native platform matrix passed in 9m39s–14m57s; the Pipboy workflow took
  15m58s including setup/fan-in, exceeding the limit. Linux x64 was the critical
  path: 13m30s in Cargo versus 8m53s on a recent main run, with zero sccache hits
  in both. Final sidecar compilation/linking took about 6m40s in both; the extra
  time accrued earlier in compilation. Recent main workflows took
  13m54s–18m01s. PR Linux x64 builds now use the existing eight-core runner with
  identical build settings and passed in 3m54s. Both macOS cross-builds still
  took 13m27s; they now use that runner for PRs as well. Subsequent run timings
  are available in the PR checks.
  Linux arm64 passed in 9m51s. Main-push runner selection is unchanged.
- With the runner changes, the next Pipboy workflow passed in 10m47s. Native
  Linux x64 took 3m53s, macOS x64 3m46s, and macOS arm64 4m11s. Linux arm64 was
  the critical path at 9m48s. The workflow is below the 15-minute ceiling but
  still above the 10-minute target, including setup and the final report.
- The full diff was reviewed for unintended files and sensitive data before publication; package builds and typechecks passed.

The repository contains a docs bundle, not the `website/` application described
in the global instructions. Local validation covers referenced checked examples,
MDX frontmatter, and snippet files/regions; a rendered website build is unavailable in this checkout.
