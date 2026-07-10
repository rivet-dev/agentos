#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <test-name>" >&2
  echo "test-name maps to tests/programs/<test-name>.c|.cc|.cpp" >&2
  exit 1
fi

TEST_NAME="$1"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
CARGO_TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
OUT_DIR="${NAPI_WASIX_TEST_OUT_DIR:-$CARGO_TARGET_ROOT/wasm32-wasix/release}"
OUT_FILE="$OUT_DIR/${TEST_NAME}.wasm"
NAPI_INCLUDE_DIR="$ROOT_DIR/include"
TEST_INCLUDE_DIR="$ROOT_DIR/tests/programs"

if ! command -v wasixcc >/dev/null 2>&1 && [[ -x "${HOME}/.wasixcc/bin/wasixcc" ]]; then
  export PATH="${HOME}/.wasixcc/bin:${PATH}"
fi

TEST_SRC=""
for ext in c cc cpp; do
  candidate="$ROOT_DIR/tests/programs/${TEST_NAME}.${ext}"
  if [[ -f "$candidate" ]]; then
    TEST_SRC="$candidate"
    break
  fi
done

if [[ -z "$TEST_SRC" ]]; then
  echo "test not found: $ROOT_DIR/tests/programs/${TEST_NAME}.c|.cc|.cpp" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

HEADER_DEPS=()
while IFS= read -r dep; do
  HEADER_DEPS+=("$dep")
done < <(find "$NAPI_INCLUDE_DIR" "$TEST_INCLUDE_DIR" -type f \( -name '*.h' -o -name '*.hpp' \) | sort)

DEPS=("$TEST_SRC")
DEPS+=("${HEADER_DEPS[@]}")

if [[ -f "$OUT_FILE" ]]; then
  NEED_REBUILD=0
  for dep in "${DEPS[@]}"; do
    if [[ "$OUT_FILE" -ot "$dep" ]]; then
      NEED_REBUILD=1
      break
    fi
  done
  if [[ $NEED_REBUILD -eq 0 ]]; then
    echo "Up-to-date: $OUT_FILE"
    exit 0
  fi
fi

# Compile to WASIX. Core N-API functions import from "napi" and Wasmer-specific
# unofficial APIs import from "napi_extension_wasmer_v0" based on the headers.
WASIX_DRIVER="wasixcc"
case "$TEST_SRC" in
  *.cc|*.cpp)
    WASIX_DRIVER="wasixcc++"
    ;;
esac

"$WASIX_DRIVER" \
  --target=wasm32-wasix \
  -O2 \
  -DBUILDING_NODE_EXTENSION \
  -DNAPI_VERSION=8 \
  -I"$NAPI_INCLUDE_DIR" \
  -I"$TEST_INCLUDE_DIR" \
  -Wl,--allow-undefined \
  -Wl,--export-memory \
  -Wl,--export=main \
  -Wl,--export=malloc \
  -Wl,--export=free \
  "$TEST_SRC" \
  -o "$OUT_FILE"

echo "Built: $OUT_FILE"
