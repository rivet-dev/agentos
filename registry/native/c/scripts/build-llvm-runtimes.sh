#!/bin/bash
set -euo pipefail

# Build the libc++/libc++abi/libunwind runtime set ourselves so C++ exception
# handling matches the same patched WASI/POSIX sysroot DuckDB uses.
#
# Reference:
# - https://github.com/llvm/llvm-project/pull/79667

: "${LLVM_PROJECT_SRC_DIR:?LLVM_PROJECT_SRC_DIR is required}"
: "${LLVM_RUNTIME_BUILD_DIR:?LLVM_RUNTIME_BUILD_DIR is required}"
: "${LLVM_RUNTIME_INSTALL_DIR:?LLVM_RUNTIME_INSTALL_DIR is required}"
: "${WASI_SDK_DIR:?WASI_SDK_DIR is required}"
: "${SYSROOT_DIR:?SYSROOT_DIR is required}"
WASI_THREAD_MODEL="${WASI_THREAD_MODEL:-single}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PATCH_DIR="$SCRIPT_DIR/../patches/llvm-project"
WASI_NM="$WASI_SDK_DIR/bin/llvm-nm"

if [ -d "$PATCH_DIR" ]; then
  while IFS= read -r patch_file; do
    if patch --dry-run -p1 -d "$LLVM_PROJECT_SRC_DIR" < "$patch_file" >/dev/null 2>&1; then
      patch --no-backup-if-mismatch -p1 -d "$LLVM_PROJECT_SRC_DIR" < "$patch_file" >/dev/null
    elif patch --dry-run -R -p1 -d "$LLVM_PROJECT_SRC_DIR" < "$patch_file" >/dev/null 2>&1; then
      :
    else
      echo "failed to apply llvm-project patch: $patch_file" >&2
      exit 1
    fi
  done < <(find "$PATCH_DIR" -name '*.patch' -type f | sort)
fi

rm -rf "$LLVM_RUNTIME_BUILD_DIR" "$LLVM_RUNTIME_INSTALL_DIR"

if [ "$WASI_THREAD_MODEL" = "posix" ]; then
  WASI_TARGET="wasm32-wasi-threads"
  RUNTIME_COMPILE_FLAGS="--target=wasm32-wasi-threads -fwasm-exceptions -matomics -mbulk-memory -pthread"
  RUNTIME_LINK_FLAGS="--target=wasm32-wasi-threads -pthread"
  RUNTIME_REQUIRED_FLAGS="--target=wasm32-wasi-threads -matomics -mbulk-memory -pthread"
  RUNTIME_REQUIRED_LIBRARIES=""
else
  WASI_TARGET="wasm32-wasi"
  RUNTIME_COMPILE_FLAGS="-fwasm-exceptions -D_WASI_EMULATED_PTHREAD"
  RUNTIME_LINK_FLAGS="-lwasi-emulated-pthread"
  RUNTIME_REQUIRED_FLAGS="-D_WASI_EMULATED_PTHREAD"
  RUNTIME_REQUIRED_LIBRARIES="wasi-emulated-pthread"
fi

cmake \
  -S "$LLVM_PROJECT_SRC_DIR/runtimes" \
  -B "$LLVM_RUNTIME_BUILD_DIR" \
  -G "Unix Makefiles" \
  -DUNIX=1 \
  -DCMAKE_TOOLCHAIN_FILE="$WASI_SDK_DIR/share/cmake/wasi-sdk.cmake" \
  -DCMAKE_MODULE_PATH="$SCRIPT_DIR/../cmake" \
  -DWASI_SDK_PREFIX="$WASI_SDK_DIR" \
  -DCMAKE_SYSROOT="$SYSROOT_DIR" \
  -DCMAKE_C_COMPILER_TARGET="$WASI_TARGET" \
  -DCMAKE_CXX_COMPILER_TARGET="$WASI_TARGET" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_FLAGS="$RUNTIME_COMPILE_FLAGS" \
  -DCMAKE_CXX_FLAGS="$RUNTIME_COMPILE_FLAGS" \
  -DCMAKE_EXE_LINKER_FLAGS="$RUNTIME_LINK_FLAGS" \
  -DCMAKE_SHARED_LINKER_FLAGS="$RUNTIME_LINK_FLAGS" \
  -DCMAKE_REQUIRED_FLAGS="$RUNTIME_REQUIRED_FLAGS" \
  -DCMAKE_REQUIRED_LIBRARIES="$RUNTIME_REQUIRED_LIBRARIES" \
  -DLLVM_ENABLE_RUNTIMES="libunwind;libcxxabi;libcxx" \
  -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_DOCS=OFF \
  -DLIBUNWIND_INCLUDE_TESTS=OFF \
  -DLIBCXXABI_INCLUDE_TESTS=OFF \
  -DLIBCXX_INCLUDE_TESTS=OFF \
  -DLIBCXX_INCLUDE_BENCHMARKS=OFF \
  -DLIBUNWIND_ENABLE_SHARED=OFF \
  -DLIBCXXABI_ENABLE_SHARED=OFF \
  -DLIBCXX_ENABLE_SHARED=OFF \
  -DLIBUNWIND_ENABLE_THREADS=OFF \
  -DLIBCXXABI_ENABLE_THREADS=ON \
  -DLIBCXX_ENABLE_THREADS=ON \
  -DLIBCXX_USE_COMPILER_RT=ON \
  -DLIBCXXABI_USE_COMPILER_RT=ON \
  -DLIBCXXABI_USE_LLVM_UNWINDER=ON \
  -DLIBCXXABI_ENABLE_STATIC_UNWINDER=ON \
  -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=ON \
  -DLIBCXX_CXX_ABI=libcxxabi \
  -DLIBCXX_ENABLE_STATIC=ON \
  -DLIBCXXABI_ENABLE_STATIC=ON \
  -DLIBUNWIND_ENABLE_STATIC=ON \
  -DCMAKE_INSTALL_PREFIX="$LLVM_RUNTIME_INSTALL_DIR"

cmake --build "$LLVM_RUNTIME_BUILD_DIR" --target install -j"$(nproc 2>/dev/null || echo 4)"

# llvm-nm returns a non-zero status for archive members with no symbols, which
# is expected here for several libunwind objects. Capture output explicitly so
# pipefail does not turn a valid archive into a false negative.
libcxxabi_symbols="$("$WASI_NM" "$LLVM_RUNTIME_INSTALL_DIR/lib/libc++abi.a" 2>/dev/null || true)"

if ! grep -q ' T __cxa_throw$' <<<"$libcxxabi_symbols"; then
  echo "rebuilt libc++abi.a does not export __cxa_throw" >&2
  exit 1
fi
if ! grep -q ' T __cxa_allocate_exception$' <<<"$libcxxabi_symbols"; then
  echo "rebuilt libc++abi.a does not export __cxa_allocate_exception" >&2
  exit 1
fi

SYSROOT_LIB="$SYSROOT_DIR/lib/$WASI_TARGET"
SYSROOT_INCLUDE="$SYSROOT_DIR/include"
SYSROOT_CXX_INCLUDE="$SYSROOT_DIR/include/$WASI_TARGET/c++/v1"
SYSROOT_CXX_INCLUDE_ROOT="$SYSROOT_DIR/include/c++/v1"
LLVM_CXX_INCLUDE="$LLVM_RUNTIME_INSTALL_DIR/include/c++/v1"

mkdir -p "$SYSROOT_LIB" "$SYSROOT_INCLUDE" "$SYSROOT_CXX_INCLUDE" "$SYSROOT_CXX_INCLUDE_ROOT"

for header in __libunwind_config.h libunwind.h libunwind.modulemap unwind.h unwind_itanium.h unwind_arm_ehabi.h; do
  if [ -f "$LLVM_RUNTIME_INSTALL_DIR/include/$header" ]; then
    cp "$LLVM_RUNTIME_INSTALL_DIR/include/$header" "$SYSROOT_INCLUDE/$header"
  fi
done

if [ -d "$LLVM_CXX_INCLUDE" ]; then
  rm -rf "$SYSROOT_CXX_INCLUDE" "$SYSROOT_CXX_INCLUDE_ROOT"
  mkdir -p "$SYSROOT_CXX_INCLUDE" "$SYSROOT_CXX_INCLUDE_ROOT"
  cp -R "$LLVM_CXX_INCLUDE/." "$SYSROOT_CXX_INCLUDE/"
fi

for runtime in libunwind.a libc++abi.a libc++.a libc++experimental.a; do
  if [ -f "$LLVM_RUNTIME_INSTALL_DIR/lib/$runtime" ]; then
    cp "$LLVM_RUNTIME_INSTALL_DIR/lib/$runtime" "$SYSROOT_LIB/$runtime"
  fi
done
