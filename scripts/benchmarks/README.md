# agentOS JS-layer benchmarks

Benchmarks that run from the TypeScript layer (the surface real consumers use:
`AgentOs.create()` / `createSession()`), under a local **llmock** so the
infrastructure metrics are deterministic and gate-able.

## Benchmarks

| file | what it measures |
|---|---|
| `coldstart.bench.ts` | total cold-start for `echo` / `pi-session` / `pi-prompt-turn` / `claude-session` |
| `memory.bench.ts` | per-session memory (RSS/heap) |
| `session.bench.ts` | **session-creation "VM tax"**: agentOS VM path vs the bare-node pi-SDK equivalent |

### `session.bench.ts` — the VM-tax benchmark

Two lanes, same llmock, same timer:

- **`vm`** — `AgentOs.create()` (`vmCreate`) + `createSession("pi")` (`sessionCreate`).
- **`bare-node`** — the *same* pi-SDK session construction on host node, **no VM**
  (`sessionCreate` = load pi SDK + `createAgentSession`). This is the "Node.js
  equivalent" baseline; it mirrors `../secure-exec/registry/agent/pi/src/adapter.ts` `newSession`.

Derived metrics:

- `derived.vmTaxMs` = `vm.sessionCreate.p50 − bareNode.sessionCreate.p50`
- `derived.vmTaxRatio` = `vm.sessionCreate.p50 / bareNode.sessionCreate.p50` (hardware-independent)

Prompt latency is **not** measured here — it's LLM-bound and belongs in a separate
informational real-API suite, never in the deterministic gate.

```bash
tsx scripts/benchmarks/session.bench.ts                   # run, print delta vs baseline
tsx scripts/benchmarks/session.bench.ts --lanes=vm        # one lane only
tsx scripts/benchmarks/session.bench.ts --gate            # exit non-zero on regression (CI)
tsx scripts/benchmarks/session.bench.ts --update-baseline # refresh baseline.json (review the diff!)
```

The bare-node lane is skipped with a clear message if `@mariozechner/pi-coding-agent`
isn't resolvable on the host (it's a devDependency for exactly this reason).

## Baselines & regression gating

Strategy: a committed **`baseline.json`** (golden numbers + full metadata) +
**relative-threshold** comparison.

- The gate runs only on **deterministic, llmock-backed** metrics (`vmCreate`,
  `sessionCreate`, `vmTaxRatio`) — never on LLM-bound latency.
- Thresholds are **relative** (e.g. +12% on `sessionCreate.p50`) so the gate
  tolerates within-class hardware drift, plus a **noise floor** (absolute ms) so
  tiny/fast metrics don't flake on sub-ms jitter.
- `vmTaxRatio` is gated as a **hardware-independent** signal — it survives
  cross-machine variance far better than absolute latencies.

Gate rules live in `session.bench.ts` (`GATE_RULES`); the comparison/IO logic
is in `baseline.ts` (reusable across benches).

### Updating the baseline

Regenerate on a **clean checkout in the canonical environment** (CI runner or a
clean `main` install — *not* a dev workspace in `secure-exec-local` mode, whose
numbers and dep versions are unrepresentative):

```bash
pnpm install            # clean, pinned deps
pnpm build
tsx scripts/benchmarks/session.bench.ts --update-baseline
# review baseline.json — check gitSha, deps versions, gitDirty:false, hardware
```

Each baseline records `gitSha`, `deps` versions (these move the numbers — a
published binary vs a source build differ a lot), hardware, node version, llmock
flag, and full percentiles. Treat a baseline as only comparable on matching
hardware + dep versions.

## CI wiring

`run-benchmarks.sh` runs the suite and writes `results/*.json`. Toggle gating with
env vars:

```bash
BENCH_GATE=1 bash scripts/benchmarks/run-benchmarks.sh             # fail on regression
BENCH_UPDATE_BASELINE=1 bash scripts/benchmarks/run-benchmarks.sh  # refresh baseline
```

For trend history / PR-comment deltas, layer `github-action-benchmark` or CodSpeed
on top later — both consume the result JSON this harness already emits.

## Notes

- `baseline.json` in this repo may be a dev-workspace seed — regenerate it per
  above before trusting the gate in CI.
- The phase sub-breakdown (`loadPiSdkRuntime`, `resourceLoader.reload`,
  `createAgentSession`) that explains *why* `sessionCreate` is ~1.5s is available
  from the sidecar's `AGENTOS_LOG_FILE` (`kind=create_session elapsed_ms=…`) and
  the `perf`-target phase tracing; wire it into the result JSON if you want the
  breakdown gated too.
