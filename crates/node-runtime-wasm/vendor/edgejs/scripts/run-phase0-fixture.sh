#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/build-local-v8"

"${SCRIPT_DIR}/test-local-v8.sh" >/dev/null

ctest \
  --test-dir "${BUILD_DIR}" \
  --output-on-failure \
  -R "ValidFixtureScriptReturnsZero"
