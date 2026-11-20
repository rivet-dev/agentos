#!/usr/bin/env bash
set -euo pipefail

VERSION=""
URL=""
SHA256=""
CACHE_DIR=""
BUILD_DIR=""
SDK_DIR=""
SYSROOT=""
OUTPUT=""
PATCH=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --url) URL="$2"; shift 2 ;;
    --sha256) SHA256="$2"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --sdk-dir) SDK_DIR="$2"; shift 2 ;;
    --sysroot) SYSROOT="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --patch) PATCH="$2"; shift 2 ;;
    *) echo "Unknown GNU awk build argument: $1" >&2; exit 1 ;;
  esac
done
if [[ -z "$VERSION" || -z "$URL" || -z "$SHA256" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$SDK_DIR" || -z "$SYSROOT" || -z "$OUTPUT" || -z "$PATCH" ]]; then
  echo "Required: --version --url --sha256 --cache-dir --build-dir --sdk-dir --sysroot --output --patch" >&2
  exit 1
fi

mkdir -p "$CACHE_DIR" "$BUILD_DIR"
TARBALL="$CACHE_DIR/gawk-$VERSION.tar.xz"
if [[ ! -f "$TARBALL" ]]; then
  temporary=$(mktemp "$TARBALL.download.XXXXXX")
  trap 'rm -f "$temporary"' EXIT
  curl --retry 3 --retry-all-errors -fSL "$URL" -o "$temporary"
  printf '%s  %s\n' "$SHA256" "$temporary" | sha256sum --check
  mv "$temporary" "$TARBALL"
  trap - EXIT
fi
printf '%s  %s\n' "$SHA256" "$TARBALL" | sha256sum --check
SRC_DIR="$BUILD_DIR/gawk-$VERSION"
rm -rf "$SRC_DIR"
tar -xJf "$TARBALL" -C "$BUILD_DIR"
cd "$SRC_DIR"
patch -p1 < "$PATCH"

# GNU awk uses real setjmp/longjmp. LLVM20 lowers these directly to finalized
# WASM exception handling; SDK25 emits the legacy encoding our runtime rejects.
# Restrict only the compiler's PATH so an ambient wasm-opt is never invoked.
# SjLj lowering also needs the linker backend option when LTO is enabled.
CC="env PATH=$SDK_DIR/bin $SDK_DIR/bin/clang --target=wasm32-wasip1 --sysroot=$SYSROOT" \
AR="$SDK_DIR/bin/llvm-ar" \
RANLIB="$SDK_DIR/bin/llvm-ranlib" \
CFLAGS="-O2 -flto -mexception-handling -mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS" \
LDFLAGS="-Wl,-mllvm,-wasm-enable-sjlj,-mllvm,-wasm-use-legacy-eh=false" \
LIBS="-lsetjmp -lwasi-emulated-mman -lwasi-emulated-process-clocks" \
./configure --host=wasm32-unknown-wasi \
  --disable-nls --disable-mpfr --disable-pma --disable-extensions

make -j4 -C support
make -j4 gawk
mkdir -p "$(dirname "$OUTPUT")"
cp gawk "$OUTPUT"
cp COPYING "$OUTPUT.COPYING"
echo "Built upstream GNU awk $VERSION at $OUTPUT"
