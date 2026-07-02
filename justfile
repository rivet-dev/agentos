set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

preview-publish REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

# THE COMMITTED STATE IS FILE-BASED (link:/path at ../secure-exec) — see
# CLAUDE.md "Boundaries". The pinned recipes below are publish-time machinery;
# never commit their output (the verify-file-deps CI gate rejects it).

# Pin secure-exec.ref to a new secure-exec sha (defaults to the sibling
# checkout's current commit). This is how agent-os advances its secure-exec dep.
secure-exec-bump REF="":
	node scripts/secure-exec-dep.mjs bump-ref "{{ REF }}"

# Restore the runtime track (@secure-exec/* npm + crates) to the committed
# file-dep state at ../secure-exec.
secure-exec-local:
	node scripts/secure-exec-dep.mjs local

# Publish-time: switch the runtime deps to their pinned catalog versions.
secure-exec-pinned:
	node scripts/secure-exec-dep.mjs pinned

# Show both dep tracks' modes + the catalog versions.
secure-exec-status:
	node scripts/secure-exec-dep.mjs status

# Publish-time: pin secure-exec to a published version (what release-swap
# uses). Release <v> pins npm + crates; preview <v> pins npm only (crates come
# from the ../secure-exec clone at secure-exec.ref). Never commit the result.
secure-exec-set-version VERSION:
	node scripts/secure-exec-dep.mjs pinned
	node scripts/secure-exec-dep.mjs pin-secure-exec "{{ VERSION }}"

# --- @agentos-software/* registry packages (independent, PER-PACKAGE versions) ---

# Restore the @agentos-software/* packages to the committed file-dep state
# (link: into ../secure-exec/registry/{software,agent}/*). Build them there
# first: `just registry-native` + `just registry-build` in ../secure-exec.
agentos-pkgs-local:
	node scripts/secure-exec-dep.mjs agentos-pkgs-local

# Publish-time: switch the @agentos-software/* packages to their pinned
# per-package catalog versions. Never commit the result.
agentos-pkgs-pinned:
	node scripts/secure-exec-dep.mjs agentos-pkgs-pinned

# Show both dep tracks' modes + the pinned versions (alias of secure-exec-status).
agentos-pkgs-status:
	node scripts/secure-exec-dep.mjs status

# Pin ONE @agentos-software package (PKG may omit the scope), e.g.
#   just agentos-pkgs-set-version coreutils 0.3.1
agentos-pkgs-set-version PKG VERSION:
	node scripts/secure-exec-dep.mjs set-agentos-pkg-version "{{ PKG }}" "{{ VERSION }}"

# Re-pin every @agentos-software package to its published version under a
# dist-tag (default: latest), e.g. `just agentos-pkgs-update dev`.
agentos-pkgs-update TAG="latest":
	node scripts/secure-exec-dep.mjs agentos-pkgs-update "{{ TAG }}"

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
