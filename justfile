set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

# Emergency release path: compile larger, slower debug sidecars.
release-fast *args:
	pnpm --filter=publish release --profile debug "$@"

# Cut a release-preview (debug build, npm-only, branch dist-tag) — see the
# release-preview skill for the end-to-end flow.
release-preview REF:
	gh workflow run publish.yaml --repo rivet-dev/agentos --ref "{{ REF }}"

check-layout:
	node scripts/check-layout.mjs

check-types:
	node scripts/verify-check-types.mjs
	pnpm check-types

lint:
	pnpm exec biome check .

fmt:
	pnpm exec biome check --write --diagnostic-level=error .

bench:
	bash scripts/benchmarks/run-benchmarks.sh

test-post-python-parity:
	pnpm --dir packages/core build
	pnpm --dir packages/core exec vitest run tests/agentos-base-filesystem.nightly.test.ts

# --- private @agentos-software/* build workspaces ---
toolchain-build:
	make -C toolchain commands
	make -C toolchain cmd/duckdb cmd/vim

toolchain-codex:
	make -C toolchain codex-required

toolchain-cmd name:
	make -C toolchain cmd/{{ name }}

# Pre-flight for the publish "WASM Commands" job's fragile state: build the C
# programs against the VANILLA wasi-sdk sysroot exactly like a fresh CI runner
# (a locally-built patched sysroot is moved aside for the run). Catches
# socket/netdb programs missing from PATCHED_PROGRAMS before CI does.
toolchain-preflight:
	#!/usr/bin/env bash
	set -euo pipefail
	cd toolchain/c
	if [ -e sysroot ]; then mv sysroot sysroot.preflight-stash; fi
	restore() { if [ -e sysroot.preflight-stash ]; then rm -rf sysroot; mv sysroot.preflight-stash sysroot; fi; }
	trap restore EXIT
	make wasi-sdk
	make programs

toolchain-copy-commands:
	node packages/core/scripts/copy-wasm-commands.mjs --require

toolchain-check-abi:
	node scripts/generate-wasm-abi-manifest.mjs

toolchain-audit-imports:
	just toolchain-check-abi
	node scripts/audit-wasm-imports.mjs

software-build:
	pnpm --filter @rivet-dev/agentos-toolchain build
	pnpm --filter '@agentos-software/*' build

# Build the catalog and stage exactly what a release would upload. This is the
# local, credential-free dry run for object-store software publication.
software-artifacts-dry-run output="target/software-artifacts":
	just tools-rebuild
	pnpm --filter=publish exec tsx src/ci/bin.ts stage-software --output "{{ output }}"

# Rebuild and stage the complete default WASM tool set from source. All outputs
# land in ignored build/bin/commands directories and must not be committed.
tools-rebuild:
	just toolchain-build
	just toolchain-codex
	just toolchain-copy-commands
	just software-build
	just toolchain-audit-imports

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
	#!/usr/bin/env bash
	set -euo pipefail
	if [[ ! -x packages/shell/node_modules/.bin/tsx \
		|| ! -d packages/build-tools/node_modules ]]; then
		pnpm install --force
	fi
	missing_registry_packages=()
	for package_json in packages/shell/node_modules/@agentos-software/*/package.json; do
		IFS=$'\t' read -r package_name package_main < <(node -e '
			const manifest = require(require("node:path").resolve(process.argv[1]));
			console.log(`${manifest.name}\t${manifest.main ?? ""}`);
		' "$package_json")
		package_dir="${package_json%/package.json}"
		if [[ -n "$package_main" && ( ! -e "$package_dir/${package_main#./}" \
			|| ! -e "$package_dir/dist/package.aospkg" ) ]]; then
			missing_registry_packages+=("$package_name")
		fi
	done
	if (( ${#missing_registry_packages[@]} > 0 )); then
		pnpm --filter @agentos-software/manifest build
		pnpm --filter @rivet-dev/agentos-toolchain build
		registry_filters=()
		for package_name in "${missing_registry_packages[@]}"; do
			registry_filters+=(--filter "$package_name")
		done
		pnpm "${registry_filters[@]}" build
	fi
	if [[ ! -e software/common/dist/index.js ]]; then
		pnpm --filter @agentos-software/common build
	fi
	if [[ ! -e packages/core/dist/index.js \
		|| ! -e packages/agentos/dist/index.js ]]; then
		pnpm --filter @rivet-dev/agentos-core build
		pnpm --filter @rivet-dev/agentos build
	fi
	CARGO_TARGET_DIR="$PWD/target" cargo build -p agentos-sidecar
	env \
		AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
		NODE_OPTIONS="--no-deprecation ${NODE_OPTIONS:-}" \
		pnpm --filter @rivet-dev/agentos-shell exec tsx src/main.ts "$@"

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

# --- Dev container (docker/dev) ---
#
# agentOS runs Linux-only, so on macOS the engine, sidecar, and VMs live in a
# container while the working tree stays on the host. The repo is bind-mounted
# at /build; node_modules, cargo target, and the toolchain build trees are
# container-owned volumes, so host edits are visible instantly without dragging
# darwin-native binaries into Linux.
#
# First run: `just dev-up && just dev-bootstrap` (the bootstrap is slow — it
# builds the WASM command set and the sidecar from source). After that,
# `just dev-terminal-example` and edit TypeScript with hot reload.

dev-compose := "docker compose -f docker/dev/compose.yaml"

# Build the image and start the container.
dev-up:
	{{dev-compose}} up -d --build

dev-down:
	{{dev-compose}} down

# Drop into a shell inside the dev container.
dev-shell:
	{{dev-compose}} exec dev bash

# Run any command inside the dev container: `just dev-exec 'cargo check --workspace'`
dev-exec cmd:
	{{dev-compose}} exec dev bash -lc "{{cmd}}"

# One-time (slow) build of everything the engine needs from source.
dev-bootstrap:
	{{dev-compose}} exec dev bash -lc '\
		set -euo pipefail; \
		echo "==> pnpm install"; \
		pnpm install --frozen-lockfile; \
		echo "==> WASM tools and software (long)"; \
		just tools-rebuild; \
		echo "==> agentos-sidecar (debug)"; \
		cargo build -p agentos-sidecar; \
		echo "==> workspace TypeScript"; \
		pnpm --filter @rivet-dev/agentos-core build; \
		pnpm --filter @rivet-dev/agentos build; \
		echo "==> done"'
