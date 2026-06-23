#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

if [[ -d /workspace/.cargo && -d /workspace/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin ]]; then
	export CARGO_HOME=/workspace/.cargo
	export RUSTUP_HOME=/workspace/.rustup
	export PATH="/workspace/.cargo/bin:${PATH}"
	export RUSTC=/workspace/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc
	export RUSTDOC=/workspace/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustdoc
fi

run_step() {
	echo
	echo "==> $*"
	"$@"
}

if [[ "${CI_FORK_PULL_REQUEST:-0}" == "1" ]]; then
	NETWORK_ENV=()
else
	NETWORK_ENV=("AGENTOS_E2E_NETWORK=1" "SECURE_EXEC_E2E_NETWORK=1")
fi

run_step pnpm install --frozen-lockfile
run_step pnpm build
run_step pnpm --dir scripts/publish run check-types
run_step pnpm --dir scripts/publish test
run_step bash scripts/check-secure-exec-rust-boundary.test.sh
run_step bash scripts/check-secure-exec-rust-boundary.sh
run_step node --test scripts/check-rust-package-metadata.test.mjs
run_step node scripts/check-rust-package-metadata.mjs
run_step node --test scripts/check-stale-split-names.test.mjs
run_step node scripts/check-stale-split-names.mjs
run_step node --test scripts/check-agentos-client-protocol-compat.test.mjs
run_step node scripts/check-agentos-client-protocol-compat.mjs
run_step node --test scripts/check-registry-test-runtime-boundary.test.mjs
run_step node scripts/check-registry-test-runtime-boundary.mjs
run_step node --test scripts/check-registry-software-split.test.mjs
run_step node scripts/check-registry-software-split.mjs
run_step node --test scripts/check-secure-exec-package-boundary.test.mjs
run_step node scripts/check-secure-exec-package-boundary.mjs
run_step cargo fmt --check
run_step cargo clippy --workspace --all-targets -- -D warnings
# Agent OS owns only the wrapper crates; the generic runtime crates are
# consumed from secure-exec releases and tested by secure-exec's own CI.
run_step cargo test -p agentos-protocol -- --test-threads=1
run_step cargo test -p agentos-sidecar -- --test-threads=1
run_step cargo test -p agentos-sidecar-browser -- --test-threads=1
run_step cargo test -p agentos-client -- --test-threads=1
run_step pnpm check-types
run_step pnpm lint

echo
if [[ ${#NETWORK_ENV[@]} -gt 0 ]]; then
	echo "==> AGENTOS_E2E_NETWORK=1 SECURE_EXEC_E2E_NETWORK=1 pnpm test"
	env "${NETWORK_ENV[@]}" pnpm test
else
	echo "==> pnpm test"
	pnpm test
fi
