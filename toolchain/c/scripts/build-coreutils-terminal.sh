#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
version=9.11
archive="$root/.cache/libs/coreutils-$version.tar.xz"
source_dir="$root/vendor/coreutils-$version"
build_dir="$root/build/coreutils-terminal"
output_dir="$root/build"
expected_sha256=394024eda0a5955217ceda9cd1201e65dc8fa3aa29c2951135a49521d57c3cc3

mkdir -p "$(dirname "$archive")" "$root/vendor" "$output_dir"
if [[ ! -f "$archive" ]]; then
  curl --retry 3 --retry-all-errors -fSL \
    "https://ftp.gnu.org/gnu/coreutils/coreutils-$version.tar.xz" -o "$archive"
fi
printf '%s  %s\n' "$expected_sha256" "$archive" | sha256sum -c -

rm -rf "$source_dir" "$build_dir"
tar -xJf "$archive" -C "$root/vendor"
# Do not let an enclosing Git checkout interpret these upstream-relative paths.
patch --directory "$source_dir" --strip=1 --batch --fuzz=0 \
  --input "$root/patches/coreutils/0001-wasi-gnulib-support.patch"
mkdir -p "$build_dir"

(
  cd "$build_dir"
  # Gnulib's cross probe otherwise compiles an unused pthread fallback even
  # though these two single-threaded commands link against our libc symbol.
  CC="$root/scripts/wasi-clang.sh" \
  AR="$root/vendor/wasi-sdk/bin/llvm-ar" \
  RANLIB="$root/vendor/wasi-sdk/bin/llvm-ranlib" \
  gl_cv_func_pthread_create=yes \
  CFLAGS="--target=wasm32-wasip1 --sysroot=$root/sysroot -O2 -flto -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS" \
  LIBS="-lwasi-emulated-mman -lwasi-emulated-process-clocks" \
  "$source_dir/configure" --host=wasm32-unknown-wasi \
    --disable-nls --disable-dependency-tracking --disable-threads
  printf 'agentos_headers: $(BUILT_SOURCES)\n' > agentos-headers.mk
  make -j3 -f Makefile -f agentos-headers.mk agentos_headers
  make -j3 src/stty src/tty
  cp src/stty "$output_dir/stty"
  cp src/tty "$output_dir/tty"
)
