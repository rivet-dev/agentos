set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

preview-publish REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

# Point the workspace at PUBLISHED secure-exec versions (CI/release default).
secure-exec-pinned:
	node scripts/secure-exec-dep.mjs pinned

# Point the workspace at the sibling ../secure-exec checkout for local hacking.
secure-exec-local:
	node scripts/secure-exec-dep.mjs local

# Bump the pinned @secure-exec/* npm version (core/s3/google-drive/sandbox).
secure-exec-set-version VERSION:
	node scripts/secure-exec-dep.mjs set-secure-exec-version "{{ VERSION }}"

# Bump the pinned @agent-os-pkgs/* software-package npm version.
agent-os-pkgs-set-version VERSION:
	node scripts/secure-exec-dep.mjs set-agent-os-pkgs-version "{{ VERSION }}"

# Bump BOTH scopes at once (only when secure-exec + software publish in lockstep).
secure-exec-set-all-versions VERSION:
	node scripts/secure-exec-dep.mjs set-version "{{ VERSION }}"

# Bump the @secure-exec/* crate version requirement (must match the sibling crate version).
secure-exec-set-crate-version VERSION:
	node scripts/secure-exec-dep.mjs set-crate-version "{{ VERSION }}"

# Show the current secure-exec dependency mode + pinned versions.
secure-exec-status:
	node scripts/secure-exec-dep.mjs status

dev-shell *args:
	pnpm --filter @rivet-dev/agent-os-dev-shell dev-shell -- "$@"

# Run the agentos-sdk.dev site (landing + /docs) locally with hot reload
docs:
	pnpm --filter @agent-os/website dev

# Build the agentos-sdk.dev site to website/dist
docs-build:
	pnpm --filter @agent-os/website build

test-bounded cmd='pnpm test':
	#!/usr/bin/env bash
	set -euo pipefail

	repo_root='{{justfile_directory()}}'
	cmd="${1:-pnpm test}"
	avail_kb="$(awk '/MemAvailable/ {print $2}' /proc/meminfo)"
	cpus="$(nproc --all)"

	if [[ -z "$avail_kb" || -z "$cpus" ]]; then
		echo "Could not determine CPU or memory budget." >&2
		exit 1
	fi

	mem_max_kb=$((avail_kb * 60 / 100))
	mem_high_kb=$((mem_max_kb * 85 / 100))
	cpu_quota="$((cpus * 60))%"

	printf 'Running with CPUQuota=%s MemoryHigh=%sK MemoryMax=%sK\n' \
		"$cpu_quota" "$mem_high_kb" "$mem_max_kb"

	# Resource limits are scoped to the whole transient unit, so test runners and
	# every child process they spawn share the same CPU, memory, IO, and task caps.
	#
	# MemoryHigh starts reclaim/throttling before the hard MemoryMax. MemoryMax is
	# based on currently available memory, not total memory, to avoid host pressure.
	# CPUQuota limits aggregate CPU to 60% of logical cores; CPUWeight and Nice make
	# other work win contention. IOWeight and idle IO scheduling keep large test
	# output/builds from making the host sticky. OOMScoreAdjust makes this bounded
	# run a preferred kill target under pressure, and TasksMax prevents runaway
	# process fan-out.
	exec systemd-run --user --wait --collect --pipe \
		-p MemoryAccounting=yes \
		-p MemoryHigh="${mem_high_kb}K" \
		-p MemoryMax="${mem_max_kb}K" \
		-p MemorySwapMax=0 \
		-p CPUAccounting=yes \
		-p CPUQuota="$cpu_quota" \
		-p CPUWeight=20 \
		-p Nice=10 \
		-p IOWeight=20 \
		-p IOSchedulingClass=idle \
		-p OOMScoreAdjust=500 \
		-p TasksMax=512 \
		bash -lc 'cd "$1" && exec bash -lc "$2"' bounded-test "$repo_root" "$cmd"

test-risky-probe *tests:
	./.agent/scripts/run-risky-test-probe.sh "$@"
