#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_ROOT="$(mktemp -d)"
FAILURE_OUTPUT="${FIXTURE_ROOT}/failure.out"
trap 'rm -rf "${FIXTURE_ROOT}"' EXIT

write_crate() {
	local name="$1"
	local dependency_line="${2:-}"
	local crate_dir="${FIXTURE_ROOT}/${name}"
	mkdir -p "${crate_dir}/src"
	cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "${name}"
version = "0.0.0"
edition = "2021"

[dependencies]
${dependency_line}
EOF
	echo "pub fn marker() {}" >"${crate_dir}/src/lib.rs"
}

cat >"${FIXTURE_ROOT}/Cargo.toml" <<'EOF'
[workspace]
members = [
	"secure-exec-client",
	"secure-exec-sidecar",
	"agentos-client",
]
resolver = "2"
EOF

write_crate "secure-exec-client"
write_crate "secure-exec-sidecar"
write_crate "agentos-client"

bash "${ROOT_DIR}/scripts/check-secure-exec-rust-boundary.sh" \
	--root "${FIXTURE_ROOT}" \
	--package secure-exec-client \
	--package secure-exec-sidecar

cat >>"${FIXTURE_ROOT}/secure-exec-client/Cargo.toml" <<'EOF'
agentos-client = { path = "../agentos-client" }
EOF

if bash "${ROOT_DIR}/scripts/check-secure-exec-rust-boundary.sh" \
	--root "${FIXTURE_ROOT}" \
	--package secure-exec-client >"${FAILURE_OUTPUT}" 2>&1; then
	echo "expected Rust boundary checker to fail on agentos-client dependency" >&2
	exit 1
fi

if ! grep -q "secure-exec Rust boundary violation in secure-exec-client" "${FAILURE_OUTPUT}"; then
	cat "${FAILURE_OUTPUT}" >&2
	exit 1
fi
