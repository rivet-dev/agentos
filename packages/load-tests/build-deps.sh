#!/usr/bin/env bash
# Build exactly the TypeScript packages the load-test image needs, in
# dependency order, WITHOUT the WASM toolchain.
#
# Why this exists (see docs-internal/load-testing.md handoff notes): the load
# workloads only ever run `node` inside `defaultSoftware:false` VMs, so they
# never read a `.aospkg` binary. That means we can build the whole chain with
# `tsc`/`tsup` and skip the heavy toolchain steps:
#   - `@agentos-software/*` leaves only export a `packagePath` string; importing
#     them never touches the (unbuilt) WASM binary, so `tsc` alone suffices.
#   - runtime-core's `copy-commands` (WASM binaries) and `build:protocol`
#     (regenerates already-present generated sources) are skipped.
#   - core's `build:protocols` is skipped for the same reason.
#
# Run from the repo root. Requires deps installed (`pnpm install`) and, for
# runtime-core's vm-config generation, a working cargo toolchain.
set -euo pipefail

echo "==> building @agentos-software chain (tsc, no WASM binaries)"
# `@agentos-software/common` (the sole default-software import in core) plus its
# eight leaf packages and the manifest types package.
for pkg in manifest coreutils sed grep gawk findutils diffutils tar gzip common; do
	pnpm --filter "@agentos-software/${pkg}" exec tsc
done

echo "==> building @rivet-dev/agentos-runtime-core (tsc, no copy-commands)"
# build:vm-config (cargo test) and build:protocol are skipped: their generated
# sources (src/generated/*.ts) are already present in the tree, so this stage
# needs no cargo — keeping the TS build layer independent of the Rust toolchain.
pnpm --filter @rivet-dev/agentos-runtime-core exec tsc

echo "==> building @rivet-dev/agentos-core (tsc)"
pnpm --filter @rivet-dev/agentos-core exec tsc

echo "==> building @rivet-dev/agentos actor bundle (tsup)"
pnpm --filter @rivet-dev/agentos build:actor

echo "==> building @rivet-dev/agentos-load-tests"
pnpm --dir packages/load-tests build

echo "==> load-test dependency build complete"
