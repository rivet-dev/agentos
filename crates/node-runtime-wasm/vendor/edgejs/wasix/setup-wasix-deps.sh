#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEPS_DIR="${PROJECT_ROOT}/deps"
OPENSSL_WASIX_DIR="${DEPS_DIR}/openssl-wasix"

if [[ ! -e "${OPENSSL_WASIX_DIR}/.git" ]]; then
  echo "error: ${OPENSSL_WASIX_DIR} is not initialized" >&2
  echo "Run: git submodule update --init deps/openssl-wasix" >&2
  exit 1
fi

if [[ ! -x "${OPENSSL_WASIX_DIR}/Configure" ]]; then
  echo "error: ${OPENSSL_WASIX_DIR}/Configure is missing or not executable" >&2
  exit 1
fi

echo "WASIX deps are ready under ${DEPS_DIR}"
