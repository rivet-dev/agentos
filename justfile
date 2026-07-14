set positional-arguments := true

release *args:
	pnpm --filter=publish release "$@"

# Cut a release-preview (debug build, npm-only, branch dist-tag) — see the
# release-preview skill for the end-to-end flow.
release-preview REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

# --- @agentos-software/* registry packages (independent, PER-PACKAGE versions) ---
registry-native:
	make -C registry/native commands

registry-native-cmd name:
	make -C registry/native cmd/{{ name }}

# Pre-flight for the publish "WASM Commands" job's fragile state: build the C
# programs against the VANILLA wasi-sdk sysroot exactly like a fresh CI runner
# (a locally-built patched sysroot is moved aside for the run). Catches
# socket/netdb programs missing from PATCHED_PROGRAMS before CI does.
registry-native-preflight:
	#!/usr/bin/env bash
	set -euo pipefail
	cd registry/native/c
	if [ -e sysroot ]; then mv sysroot sysroot.preflight-stash; fi
	restore() { if [ -e sysroot.preflight-stash ]; then rm -rf sysroot; mv sysroot.preflight-stash sysroot; fi; }
	trap restore EXIT
	make wasi-sdk
	make programs

registry-copy-commands:
	node packages/runtime-core/scripts/copy-wasm-commands.mjs

registry-build:
	pnpm --filter '@agentos-software/*' build

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
	gigacode_root="${GIGACODE_ROOT:-$PWD/../sandbox-agent}"
	if [[ ! -f "$gigacode_root/justfile" ]]; then
		echo "Gigacode repository not found at $gigacode_root (override with GIGACODE_ROOT)" >&2
		exit 1
	fi
	if [[ "$(uname -s)" == "Linux" \
		&& -f /usr/include/openssl/ssl.h \
		&& -f /usr/lib/x86_64-linux-gnu/libssl.so ]]; then
		export OPENSSL_DIR=/usr
		export OPENSSL_INCLUDE_DIR=/usr/include
		export OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu
	fi
	just --justfile "$gigacode_root/justfile" install-gigacode
	"$HOME/.cargo/bin/gigacode" --version

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
	if [[ ! -e registry/software/common/dist/index.js ]]; then
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
	cargo_packages=(-p agentos-sidecar)
	if [[ "$actor_mode" == true ]]; then
		cargo_packages+=(-p agentos-actor-plugin)
	fi
	CARGO_TARGET_DIR="$PWD/target" cargo build "${cargo_packages[@]}"
	actor_env=()
	if [[ "$actor_mode" == true ]]; then
		case "$(uname -s)" in
			Darwin) plugin_name=libagentos_actor_plugin.dylib ;;
			Linux) plugin_name=libagentos_actor_plugin.so ;;
			*) plugin_name=agentos_actor_plugin.dll ;;
		esac
		actor_env+=(AGENTOS_PLUGIN_BIN="$PWD/target/debug/$plugin_name")
	fi
	env "${actor_env[@]}" \
		AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
		NODE_OPTIONS="--no-deprecation ${NODE_OPTIONS:-}" \
		pnpm --filter @rivet-dev/agentos-shell exec tsx src/main.ts "$@"

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
