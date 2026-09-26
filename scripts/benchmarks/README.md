# agentOS benchmarks

This directory contains product-level VM benchmarks for cold starts and
incremental memory use:

- `coldstart.bench.ts` measures VM creation through the first workload.
- `memory.bench.ts` measures marginal memory overhead per VM.
- `bench-utils.ts` contains their shared helpers.

Run both lanes with:

```bash
bash scripts/benchmarks/run-benchmarks.sh
```

Set `BENCH_ONLY=coldstart-sleep` or `BENCH_ONLY=memory-sleep` to run one lane.
Results are written under `scripts/benchmarks/results/`.
