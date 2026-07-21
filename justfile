set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

# Cut a release-preview (debug build, npm-only, branch dist-tag) — see the
# release-preview skill for the end-to-end flow.
release-preview REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

# --- @agentos-software/* software packages (independent, PER-PACKAGE versions) ---
toolchain-build:
	make -C toolchain commands

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
	node packages/runtime-core/scripts/copy-wasm-commands.mjs

software-build:
	pnpm --filter '@agentos-software/*' build

# Rebuild and stage the complete default WASM tool set from source. All outputs
# land in ignored build/bin/commands directories and must not be committed.
tools-rebuild:
	just toolchain-build
	just toolchain-copy-commands
	just software-build

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

install-gigacode:
	#!/usr/bin/env bash
	set -euo pipefail
	repo_root='{{justfile_directory()}}'
	pnpm --dir "$repo_root" install
	make -C "$repo_root/toolchain" wasm
	if [[ -n "${CODEX_REPO:-}" ]]; then
		make -C "$repo_root/toolchain" codex-required CODEX_REPO="$CODEX_REPO"
	else
		make -C "$repo_root/toolchain" codex-required
	fi
	if [[ -n "${AGENTOS_SIDECAR_BIN:-}" ]]; then
		export AGENTOS_SKIP_NATIVE_META_BUILD=1
	fi
	pnpm --dir "$repo_root" --filter '@rivet-dev/agentos-experiment-gigacode...' build
	pnpm --dir "$repo_root/experiments/gigacode" check-types
	pnpm --dir "$repo_root/experiments/gigacode" install-global
	"$HOME/.local/bin/gigacode" --version

shell *args:
	#!/usr/bin/env bash
	set -euo pipefail
	actor_mode=false
	for arg in "$@"; do
		if [[ "$arg" == "--actor" ]]; then
			actor_mode=true
		fi
	done
	if [[ ! -x packages/shell/node_modules/.bin/tsx \
		|| ! -e packages/shell/node_modules/@agentos-software/codex-cli \
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
	if [[ ! -e packages/runtime-core/dist/index.js \
		|| ! -e packages/core/dist/index.js \
		|| ! -e packages/agentos/dist/index.js ]]; then
		pnpm --filter @rivet-dev/agentos-runtime-core build
		pnpm --filter @rivet-dev/agentos-core build
		pnpm --filter @rivet-dev/agentos build
	fi
	if [[ "$actor_mode" == true ]]; then
		r6_root="${AGENTOS_R6_ROOT:-$PWD/../r6}"
		rivetkit_loader="$r6_root/rivetkit-typescript/packages/rivetkit/node_modules/tsx/dist/loader.mjs"
		if [[ ! -e "$r6_root/pnpm-lock.yaml" ]]; then
			echo "just shell --actor requires the Rivet repo at $r6_root (override with AGENTOS_R6_ROOT)" >&2
			exit 1
		fi
		if [[ ! -e "$rivetkit_loader" ]]; then
			pnpm --dir "$r6_root" install --frozen-lockfile --filter 'rivetkit...'
		fi
		if [[ ! -e "$r6_root/shared/typescript/virtual-websocket/dist/mod.js" \
			|| ! -e "$r6_root/rivetkit-typescript/packages/traces/dist/tsup/index.js" \
			|| ! -e "$r6_root/rivetkit-typescript/packages/workflow-engine/dist/tsup/index.js" \
			|| ! -e "$r6_root/engine/sdks/typescript/envoy-protocol/dist/index.js" \
			|| ! -e "$r6_root/rivetkit-typescript/packages/rivetkit-wasm/pkg/rivetkit_wasm.js" ]]; then
			pnpm --dir "$r6_root" --filter 'rivetkit...' build
		fi
	fi
	CARGO_TARGET_DIR="$PWD/target" cargo build -p agentos-sidecar
	env \
		AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
		NODE_OPTIONS="--no-deprecation ${NODE_OPTIONS:-}" \
		pnpm --filter @rivet-dev/agentos-shell exec tsx src/main.ts "$@"

# Run the agentos-sdk.dev site (landing + /docs) locally with hot reload
docs:
	pnpm --filter @rivet-dev/agentos-website dev

# Build the agentos-sdk.dev site to website/dist
docs-build:
	pnpm --filter @rivet-dev/agentos-website build

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

# Build the exact-workspace image used by every load-test lane. Dockerfile-local
# ignore rules keep generated artifacts and unrelated website sources out of the
# build context.
load-test-image:
	docker build --file packages/load-tests/Dockerfile --tag agentos-load-tests:local .

# Container-boundary self-test: prove the cgroup envelope, fd ulimit, tmpfs, and
# network isolation match the flags before trusting any survival verdict. Uses
# the exact bounded profile of the limit lane.
load-test-boundary:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-boundary-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 2m docker run --rm \
		--name "$container" \
		--memory=3g --memory-swap=3g --cpus=2 --pids-limit=256 \
		--ulimit nofile=1024:1024 --network=none \
		--user "$(id -u):$(id -g)" \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=512m,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		agentos-load-tests:local boundary

# Guest process-limit attack beside a sentinel VM. The workload never runs on
# the host; the container has hard memory/CPU/PID/fd/swap/time ceilings.
load-test-limits:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-limits-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 8m docker run --rm \
		--name "$container" \
		--memory=3g --memory-swap=3g --cpus=2 --pids-limit=256 \
		--ulimit nofile=1024:1024 --network=none \
		--user "$(id -u):$(id -g)" \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=512m,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		--env LOAD_TEST_PROCESS_LIMIT --env LOAD_TEST_PROCESS_ATTEMPTS \
		agentos-load-tests:local limits

# High-scale adversarial battery: bounded-but-LARGER cgroup (8 CPU / 8 GiB) so
# the V8 executor pool (= CPU count) is big enough to actually run hundreds of
# concurrent VMs. Still a hard-capped container. Runs the `scale` command.
# Args after the recipe name are passed as the command (default `scale`).
load-test-scale cmd='scale':
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-scale-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 25m docker run --rm \
		--name "$container" \
		--user "$(id -u):$(id -g)" \
		--memory=8g --memory-swap=8g --cpus=8 --pids-limit=2048 \
		--ulimit nofile=8192:8192 --network=none \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=2g,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		--env LOAD_TEST_VM_COUNT --env LOAD_TEST_CONCURRENCY --env LOAD_TEST_CYCLES \
		--env LOAD_TEST_EXEC_CONCURRENCY --env LOAD_TEST_MATRIX_ONLY \
		agentos-load-tests:local "{{ cmd }}"

# Full deterministic adversarial limit matrix (processes, fds, sockets,
# filesystem bytes) beside a sentinel, same bounded cgroup as the limit lane.
load-test-limits-matrix:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-matrix-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 10m docker run --rm \
		--name "$container" \
		--user "$(id -u):$(id -g)" \
		--memory=3g --memory-swap=3g --cpus=2 --pids-limit=256 \
		--ulimit nofile=1024:1024 --network=none \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=512m,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		agentos-load-tests:local limits-matrix

# Sequential, burst, and steady-replacement VM churn with leak gates. This is
# intentionally more generous than the limit lane but remains a bounded cgroup.
load-test-churn:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-churn-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 20m docker run --rm \
		--name "$container" \
		--memory=4g --memory-swap=4g --cpus=3 --pids-limit=384 \
		--ulimit nofile=2048:2048 --network=none \
		--user "$(id -u):$(id -g)" \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=1g,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		--env LOAD_TEST_CYCLES --env LOAD_TEST_BATCH --env LOAD_TEST_SETTLE_MS \
		--env LOAD_TEST_RSS_SLOPE_BYTES --env LOAD_TEST_RSS_TOTAL_BYTES \
		--env LOAD_TEST_PSS_TOTAL_BYTES \
		agentos-load-tests:local churn

# The external Compute load generator is also containerized. Unlike the local
# lanes it needs egress to the Rivet APIs, but retains hard resource ceilings.
load-test-compute:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .artifacts/load-tests
	container="agentos-load-compute-$$"
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT INT TERM
	timeout --signal=TERM --kill-after=30s 20m docker run --rm \
		--name "$container" \
		--memory=1g --memory-swap=1g --cpus=1 --pids-limit=128 \
		--ulimit nofile=1024:1024 \
		--user "$(id -u):$(id -g)" \
		--security-opt no-new-privileges --cap-drop=ALL \
		--tmpfs /tmp:rw,nosuid,nodev,size=128m,mode=1777 \
		--volume "$PWD/.artifacts/load-tests:/artifacts" \
		--env RIVET_ENDPOINT --env RIVET_PUBLIC_ENDPOINT --env RIVET_RUN_URL \
		--env COMPUTE_STEPS --env COMPUTE_HOLD_MS --env COMPUTE_SCALE_DOWN_MS \
		--env COMPUTE_ACTOR_NAME --env COMPUTE_CREATE_CONCURRENCY \
		--env COMPUTE_READY_TIMEOUT_MS --env COMPUTE_CLEANUP_DEADLINE_MS \
		agentos-load-tests:local compute-load
