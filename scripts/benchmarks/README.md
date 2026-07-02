# Agent OS Benchmarks

Agent OS keeps only product-surface benchmarks here:

- `session.bench.ts` - deterministic llmock-backed session VM tax (`vm` vs `bare-node` PI SDK session creation).
- `coldstart.bench.ts` - Agent OS VM cold-start product workloads.
- `memory.bench.ts` - shared-sidecar per-VM memory overhead.
- `bench-utils.ts` - shared helpers for cold-start and memory workloads.
- `baseline.json` - committed baseline for the session VM-tax regression gate.

The differential matrix, focused runtime lanes, fuzz/perf harness, leak and
footprint probes, native comparisons, and ecosystem command benches now live in
secure-exec:

`/home/nathan/.herdr/workspaces/agent-os/secure-exec-perf-rules/packages/benchmarks`

Use that package for runtime-focused investigations; also follow its
`CLAUDE.md` Benchmarks section. `overlay-readdir` is deleted here too; its
secure-exec port is pending the API it needs.

## Standard Suite

Run the remaining product lanes through:

```bash
bash scripts/benchmarks/run-benchmarks.sh
```

Run a single lane with `BENCH_ONLY=<lane>`:

- `coldstart-sleep`
- `memory-sleep`
- `memory-pi-session`
- `session`

Results are written under `scripts/benchmarks/results/` as `<lane>.json` and
`<lane>.log`.

## Session Baseline Gate

`session.bench.ts` compares Agent OS session creation against a bare Node PI SDK
baseline and reports:

- `vm.vmCreate.p50`
- `vm.sessionCreate.p50`
- `derived.vmTaxRatio`

Useful commands:

```bash
pnpm exec tsx scripts/benchmarks/session.bench.ts
pnpm exec tsx scripts/benchmarks/session.bench.ts --lanes=vm
pnpm exec tsx scripts/benchmarks/session.bench.ts --gate
pnpm exec tsx scripts/benchmarks/session.bench.ts --update-baseline
BENCH_GATE=1 bash scripts/benchmarks/run-benchmarks.sh
BENCH_UPDATE_BASELINE=1 bash scripts/benchmarks/run-benchmarks.sh
```

Only refresh `baseline.json` intentionally and review the resulting diff.
