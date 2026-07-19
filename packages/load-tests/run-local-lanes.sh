#!/usr/bin/env bash
# Orchestrate the bounded local load-test lanes and collect their verdicts.
# Every lane runs inside its `just` Docker wrapper (never on the host). Prints
# one JSON verdict line per run; the caller records these in the run ledger.
#
# Usage: bash packages/load-tests/run-local-lanes.sh [reps]
set -uo pipefail
reps="${1:-3}"
here="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$here"

run() {
	local label="$1"; shift
	echo "### ${label}"
	if "$@"; then
		echo "### ${label} recipe exit=0"
	else
		echo "### ${label} recipe exit=$? (see verdict line / artifact)"
	fi
}

run "boundary" just load-test-boundary

for i in $(seq 1 "$reps"); do
	run "limits(process) rep ${i}" just load-test-limits
done

for i in $(seq 1 "$reps"); do
	run "limits-matrix rep ${i}" just load-test-limits-matrix
done

# Churn runs a few extra reps to calibrate the provisional RSS/PSS ceilings.
churn_reps="${2:-5}"
for i in $(seq 1 "$churn_reps"); do
	run "churn rep ${i}" just load-test-churn
done

echo "### all local lanes complete"
