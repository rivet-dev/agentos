set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

preview-publish REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

# Point deps at the sibling ../secure-exec checkout for local hacking.
secure-exec-local:
	node scripts/secure-exec-dep.mjs local

# Switch deps back to pinned (published) mode without changing the pinned versions.
secure-exec-pinned:
	node scripts/secure-exec-dep.mjs pinned

# Show the current dep mode + the pinned npm/crate versions.
secure-exec-status:
	node scripts/secure-exec-dep.mjs status

# Pin secure-exec to a published version and switch to pinned mode.
#   Release <v>  -> @secure-exec/* npm AND secure-exec-* crates both pin to <v>
#                   (a release publishes npm + crates together).
#   Preview <v>  -> @secure-exec/* npm pin to the 0.0.0-<branch>.<sha> tag; the
#                   crate version is left at the last crates.io release because
#                   crates.io has no preview track. CI's `prepare-build` clones
#                   secure-exec at <sha> and builds the crates from that clone.
secure-exec-set-version VERSION:
	node scripts/secure-exec-dep.mjs pinned
	node scripts/secure-exec-dep.mjs pin-secure-exec "{{ VERSION }}"

# Pin the @agentos-software/* software packages (separate version track).
agentos-pkgs-set-version VERSION:
	node scripts/secure-exec-dep.mjs set-agentos-pkgs-version "{{ VERSION }}"

install-shell:
	#!/usr/bin/env bash
	set -euo pipefail
	pnpm --filter @rivet-dev/agentos-shell build
	global_bin_dir="$(pnpm config get global-bin-dir)"
	if [[ -z "$global_bin_dir" || "$global_bin_dir" == "undefined" ]]; then
		global_bin_dir="${PNPM_HOME:-/tmp/pnpm}"
	fi
	mkdir -p "$global_bin_dir"
	for package in @rivet-dev/agentos-shell @rivet-dev/agent-os-shell @rivet-dev/agentos-workspace; do
		PATH="$global_bin_dir:$PATH" pnpm --global remove "$package" >/dev/null 2>&1 || true
	done
	(cd packages/shell && PATH="$global_bin_dir:$PATH" pnpm link --global)

shell *args:
	NODE_OPTIONS="--no-deprecation ${NODE_OPTIONS:-}" pnpm --filter @rivet-dev/agentos-shell exec tsx src/main.ts -i -t "$@"

# Run the agentos-sdk.dev site (landing + /docs) locally with hot reload
docs:
	pnpm --filter @agentos/website dev

# Build the agentos-sdk.dev site to website/dist
docs-build:
	pnpm --filter @agentos/website build

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
