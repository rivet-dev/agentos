#!/usr/bin/env bash
set -euo pipefail

PREBUILT_V8_VERSION="11.9.2"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <test-name>" >&2
  echo "test-name maps to tests/programs/<test-name>.c|.cc|.cpp" >&2
  exit 1
fi

TEST_NAME="$1"

# ROOT_DIR = napi (the crate root)
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../ && pwd)"
# PROJECT_ROOT = top-level repo root
PROJECT_ROOT="$ROOT_DIR/.."
CARGO_TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
OUT_DIR="${NAPI_NATIVE_TEST_OUT_DIR:-$CARGO_TARGET_ROOT/native}"
OUT_FILE="$OUT_DIR/${TEST_NAME}"
NAPI_INCLUDE_DIR="$ROOT_DIR/include"
TEST_INCLUDE_DIR="$ROOT_DIR/tests/programs"
NATIVE_INIT_SRC="$ROOT_DIR/tests/napi_native_init.cc"

# napi/v8 paths
NAPI_V8_DIR="$ROOT_DIR/v8"
NAPI_V8_INCLUDE="$ROOT_DIR/include"
NAPI_V8_SRC="$NAPI_V8_DIR/src"
EDGE_SRC="$PROJECT_ROOT/src"

resolve_primary_library() {
  local dir="$1"
  local candidate
  for candidate in \
    libv8.a \
    libv8.so \
    libv8.dylib \
    libv8_monolith.a \
    libv8_monolith.so \
    libv8_monolith.dylib
  do
    if [[ -f "$dir/$candidate" ]]; then
      printf '%s\n' "$dir/$candidate"
      return 0
    fi
  done
  return 1
}

add_local_v8_extra_links() {
  local library_path="$1"
  local lib_dir
  lib_dir="$(dirname "$library_path")"
  local candidate

  for candidate in libv8_libplatform.a libv8_libplatform.so libv8_libplatform.dylib; do
    if [[ -f "$lib_dir/$candidate" ]]; then
      V8_LINK_ARGS+=("$lib_dir/$candidate")
      break
    fi
  done

  for candidate in libv8_libbase.a libv8_libbase.so libv8_libbase.dylib; do
    if [[ -f "$lib_dir/$candidate" ]]; then
      V8_LINK_ARGS+=("$lib_dir/$candidate")
      break
    fi
  done
}

resolve_cached_prebuilt_v8() {
  local target_os target_arch platform_name include_file include_dir root_dir build_cache_dir
  target_os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  target_arch="$(uname -m)"

  case "$target_os/$target_arch" in
    darwin/arm64)
      platform_name="darwin-arm64"
      ;;
    darwin/x86_64)
      platform_name="darwin-amd64"
      ;;
    linux/x86_64)
      platform_name="linux-amd64"
      ;;
    *)
      return 1
      ;;
  esac

  build_cache_dir="$CARGO_TARGET_ROOT/debug/build"
  if [[ ! -d "$build_cache_dir" ]]; then
    return 1
  fi

  include_file="$(
    find "$build_cache_dir" \
      -path "*/v8-prebuilt/${PREBUILT_V8_VERSION}/${platform_name}/include/v8.h" \
      | sort \
      | tail -n 1
  )"

  if [[ -z "$include_file" ]]; then
    return 1
  fi

  include_dir="$(dirname "$include_file")"
  root_dir="$(dirname "$include_dir")"
  V8_LIBRARY_PATH="$(resolve_primary_library "$root_dir/lib")" || return 1
  V8_INCLUDE_DIR="$include_dir"
  V8_LINK_ARGS=("$V8_LIBRARY_PATH")
  if [[ "$target_os" == "darwin" ]]; then
    V8_LINK_ARGS+=("-framework" "CoreFoundation")
  fi
}

resolve_local_v8() {
  local root include_dir library_path
  for root in \
    /opt/homebrew/opt/v8 \
    /opt/homebrew/Cellar/v8/14.5.201.9 \
    /usr/local/opt/v8
  do
    include_dir="$root/include"
    if [[ ! -f "$include_dir/v8.h" ]]; then
      continue
    fi
    library_path="$(resolve_primary_library "$root/lib")" || continue
    V8_INCLUDE_DIR="$include_dir"
    V8_LIBRARY_PATH="$library_path"
    V8_LINK_ARGS=("$V8_LIBRARY_PATH")
    add_local_v8_extra_links "$V8_LIBRARY_PATH"
    return 0
  done
  return 1
}

# V8 paths/defines
V8_DEFINES="${V8_DEFINES:-${NAPI_V8_DEFINES:-V8_COMPRESS_POINTERS}}"
V8_INCLUDE_DIR="${V8_INCLUDE_DIR:-${NAPI_V8_INCLUDE_DIR:-}}"
V8_LIB_DIR="${V8_LIB_DIR:-}"
V8_LIBRARY_PATH="${NAPI_V8_LIBRARY:-${NAPI_V8_V8_LIBRARY:-${NAPI_V8_V8_MONOLITH_LIB:-}}}"
V8_LINK_ARGS=()
PLATFORM_LINK_ARGS=()
C_COMPILER=clang
CXX_COMPILER=clang++

if [[ "$(uname -s)" == "Darwin" ]]; then
  C_COMPILER=/usr/bin/clang
  CXX_COMPILER=/usr/bin/clang++
  PLATFORM_LINK_ARGS=(-lc++)
else
  PLATFORM_LINK_ARGS=(-lstdc++ -ldl -lm -lpthread -lrt)
fi

if [[ -n "$V8_INCLUDE_DIR" && -n "$V8_LIBRARY_PATH" ]]; then
  V8_LINK_ARGS=("$V8_LIBRARY_PATH")
  add_local_v8_extra_links "$V8_LIBRARY_PATH"
elif [[ -n "$V8_INCLUDE_DIR" && -n "$V8_LIB_DIR" ]]; then
  V8_LIBRARY_PATH="$(resolve_primary_library "$V8_LIB_DIR")"
  V8_LINK_ARGS=("$V8_LIBRARY_PATH")
  add_local_v8_extra_links "$V8_LIBRARY_PATH"
elif ! resolve_cached_prebuilt_v8; then
  resolve_local_v8
fi

if [[ -z "${V8_INCLUDE_DIR:-}" || -z "${V8_LIBRARY_PATH:-}" ]]; then
  echo "failed to resolve compatible V8 headers/library for native tests" >&2
  exit 1
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

NAPI_V8_SOURCES=(
  "$NAPI_V8_SRC/js_native_api_v8.cc"
  "$NAPI_V8_SRC/unofficial_napi.cc"
  "$NAPI_V8_SRC/unofficial_napi_error_utils.cc"
  "$NAPI_V8_SRC/unofficial_napi_contextify.cc"
  "$NAPI_V8_SRC/edge_v8_platform.cc"
)

DEPS=("$TEST_SRC" "$NATIVE_INIT_SRC")
DEPS+=("${NAPI_V8_SOURCES[@]}")
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

# Step 1: Compile the test translation unit.
case "$TEST_SRC" in
  *.cc|*.cpp)
    "$CXX_COMPILER" -c -std=c++20 -O2 \
      -fno-rtti \
      -w \
      -DNAPI_EXTERN= \
      -DNAPI_VERSION=8 \
      -I"$NAPI_INCLUDE_DIR" \
      -I"$TEST_INCLUDE_DIR" \
      "$TEST_SRC" \
      -o "$OUT_DIR/${TEST_NAME}.o"
    ;;
  *)
    "$C_COMPILER" -c -std=c11 -O2 \
      -DNAPI_EXTERN= \
      -DNAPI_VERSION=8 \
      -I"$NAPI_INCLUDE_DIR" \
      -I"$TEST_INCLUDE_DIR" \
      "$TEST_SRC" \
      -o "$OUT_DIR/${TEST_NAME}.o"
    ;;
esac

# Step 2: Compile native init + napi/v8 and link everything
"$CXX_COMPILER" -std=c++20 -O2 \
  -fno-rtti \
  -w \
  -DNAPI_EXTERN= \
  -DNAPI_VERSION=8 \
  $(echo "$V8_DEFINES" | tr ';,' '\n' | sed '/^[[:space:]]*$/d; s/^[[:space:]]*/-D/; s/[[:space:]]*$//' | tr '\n' ' ') \
  -I"$NAPI_INCLUDE_DIR" \
  -I"$NAPI_V8_INCLUDE" \
  -I"$NAPI_V8_SRC" \
  -I"$EDGE_SRC" \
  -I"$V8_INCLUDE_DIR" \
  "$OUT_DIR/${TEST_NAME}.o" \
  "$NATIVE_INIT_SRC" \
  "${NAPI_V8_SOURCES[@]}" \
  "${V8_LINK_ARGS[@]}" \
  "${PLATFORM_LINK_ARGS[@]}" \
  -o "$OUT_FILE"

# Clean up intermediate object file
rm -f "$OUT_DIR/${TEST_NAME}.o"

echo "Built: $OUT_FILE"
