#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/build-local-v8"

V8_ROOT_DEFAULT="/opt/homebrew/opt/v8"
export NAPI_V8_BUILD_METHOD="${NAPI_V8_BUILD_METHOD:-local}"
export NAPI_V8_INCLUDE_DIR="${NAPI_V8_INCLUDE_DIR:-${NAPI_V8_V8_INCLUDE_DIR:-${V8_ROOT_DEFAULT}/include}}"
export NAPI_V8_LIBRARY="${NAPI_V8_LIBRARY:-${NAPI_V8_V8_LIBRARY:-${V8_ROOT_DEFAULT}/lib/libv8.dylib}}"
if [[ -z "${NAPI_V8_EXTRA_LIBS:-}" ]]; then
  local_extra_libs=()
  [[ -f "${V8_ROOT_DEFAULT}/lib/libv8_libplatform.dylib" ]] && local_extra_libs+=("${V8_ROOT_DEFAULT}/lib/libv8_libplatform.dylib")
  [[ -f "${V8_ROOT_DEFAULT}/lib/libv8_libbase.dylib" ]] && local_extra_libs+=("${V8_ROOT_DEFAULT}/lib/libv8_libbase.dylib")
  if [[ ${#local_extra_libs[@]} -gt 0 ]]; then
    NAPI_V8_EXTRA_LIBS="$(IFS=';'; echo "${local_extra_libs[*]}")"
    export NAPI_V8_EXTRA_LIBS
  fi
fi

export NAPI_V8_DEFINES="${NAPI_V8_DEFINES:-${NAPI_V8_V8_DEFINES:-}}"

if [[ ! -d "${NAPI_V8_INCLUDE_DIR}" ]]; then
  echo "V8 include directory not found: ${NAPI_V8_INCLUDE_DIR}" >&2
  exit 1
fi

if [[ ! -f "${NAPI_V8_LIBRARY}" ]]; then
  echo "V8 library not found: ${NAPI_V8_LIBRARY}" >&2
  exit 1
fi

export DYLD_LIBRARY_PATH="$(dirname "${NAPI_V8_LIBRARY}"):${DYLD_LIBRARY_PATH:-}"

# Backward compatibility aliases during migration.
export NAPI_V8_V8_INCLUDE_DIR="${NAPI_V8_INCLUDE_DIR}"
export NAPI_V8_V8_LIBRARY="${NAPI_V8_LIBRARY}"
export NAPI_V8_V8_MONOLITH_LIB="${NAPI_V8_LIBRARY}"
export NAPI_V8_V8_EXTRA_LIBS="${NAPI_V8_EXTRA_LIBS:-}"
export NAPI_V8_V8_DEFINES="${NAPI_V8_DEFINES}"

echo "Using V8 headers: ${NAPI_V8_INCLUDE_DIR}"
echo "Using V8 library: ${NAPI_V8_LIBRARY}"
echo "Using V8 extra libs: ${NAPI_V8_EXTRA_LIBS:-<none>}"
echo "Using V8 defines: ${NAPI_V8_DEFINES}"

cmake \
  -S "${PROJECT_DIR}" \
  -B "${BUILD_DIR}" \
  -DNAPI_V8_BUILD_METHOD="${NAPI_V8_BUILD_METHOD}" \
  -DNAPI_V8_INCLUDE_DIR="${NAPI_V8_INCLUDE_DIR}" \
  -DNAPI_V8_LIBRARY="${NAPI_V8_LIBRARY}" \
  -DNAPI_V8_EXTRA_LIBS="${NAPI_V8_EXTRA_LIBS:-}" \
  -DNAPI_V8_DEFINES="${NAPI_V8_DEFINES}"

cmake --build "${BUILD_DIR}" -j4
ctest --test-dir "${BUILD_DIR}" --output-on-failure
