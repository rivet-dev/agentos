#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
custom_packages=()

secure_exec_packages=(
	secure-exec-bridge
	secure-exec-kernel
	secure-exec-execution
	secure-exec-v8-runtime
	secure-exec-sidecar
	secure-exec-client
)

while [[ $# -gt 0 ]]; do
	case "$1" in
		--root)
			if [[ $# -lt 2 ]]; then
				echo "--root requires a path" >&2
				exit 2
			fi
			ROOT_DIR="$2"
			shift 2
			;;
		--package)
			if [[ $# -lt 2 ]]; then
				echo "--package requires a package name" >&2
				exit 2
			fi
			custom_packages+=("$2")
			shift 2
			;;
		*)
			echo "unknown argument: $1" >&2
			exit 2
			;;
	esac
done

if [[ ${#custom_packages[@]} -gt 0 ]]; then
	secure_exec_packages=("${custom_packages[@]}")
fi

cd "${ROOT_DIR}"

for package in "${secure_exec_packages[@]}"; do
	tree="$(cargo tree -p "${package}" -e normal)"
	if grep -E '(^|[[:space:]])(agentos-protocol|agentos-client|agentos-sidecar)[[:space:]]' <<<"${tree}" >/dev/null; then
		echo "secure-exec Rust boundary violation in ${package}:"
		grep -E '(^|[[:space:]])(agentos-protocol|agentos-client|agentos-sidecar)[[:space:]]' <<<"${tree}"
		exit 1
	fi
done

echo "secure-exec Rust boundary ok"
