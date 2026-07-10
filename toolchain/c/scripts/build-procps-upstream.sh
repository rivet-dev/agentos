#!/usr/bin/env bash
set -euo pipefail

VERSION=""
URL=""
SHA256=""
CACHE_DIR=""
BUILD_DIR=""
OVERLAY_INCLUDE_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
SYSROOT=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--version) VERSION="$2"; shift 2 ;;
		--url) URL="$2"; shift 2 ;;
		--sha256) SHA256="$2"; shift 2 ;;
		--cache-dir) CACHE_DIR="$2"; shift 2 ;;
		--build-dir) BUILD_DIR="$2"; shift 2 ;;
		--overlay-include-dir) OVERLAY_INCLUDE_DIR="$2"; shift 2 ;;
		--cc) CC_CMD="$2"; shift 2 ;;
		--ar) AR_CMD="$2"; shift 2 ;;
		--ranlib) RANLIB_CMD="$2"; shift 2 ;;
		--sysroot) SYSROOT="$2"; shift 2 ;;
		--output-dir) OUTPUT_DIR="$2"; shift 2 ;;
		*) echo "Unknown argument: $1" >&2; exit 1 ;;
	esac
done

for required in VERSION URL SHA256 CACHE_DIR BUILD_DIR OVERLAY_INCLUDE_DIR CC_CMD AR_CMD RANLIB_CMD SYSROOT OUTPUT_DIR; do
	if [[ -z "${!required}" ]]; then
		echo "Missing required argument: $required" >&2
		exit 1
	fi
done

fetch() {
	local url="$1" out="$2"
	if command -v curl >/dev/null 2>&1; then
		curl --retry 3 --retry-all-errors -fSL "$url" -o "$out"
	else
		wget -q "$url" -O "$out"
	fi
}

mkdir -p "$CACHE_DIR"
tarball="$CACHE_DIR/procps-ng-$VERSION.tar.xz"
[[ -f "$tarball" ]] || fetch "$URL" "$tarball"
printf '%s  %s\n' "$SHA256" "$tarball" | sha256sum -c -

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/source" "$BUILD_DIR/out"
tar -xJf "$tarball" -C "$BUILD_DIR/source"
src_dir="$BUILD_DIR/source/procps-ng-$VERSION"
[[ -d "$src_dir" ]] || { echo "Expected source at $src_dir" >&2; exit 1; }

common_flags="--target=wasm32-wasip1 --sysroot=$SYSROOT -I$OVERLAY_INCLUDE_DIR -O2 -D_GNU_SOURCE -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_MMAN"
link_flags="--target=wasm32-wasip1 --sysroot=$SYSROOT -Wl,-z,stack-size=8388608 -Wl,--stack-first"

pushd "$BUILD_DIR/out" >/dev/null
CC="$CC_CMD" \
CPP="$CC_CMD $common_flags -E" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
CFLAGS="$common_flags" \
LDFLAGS="$link_flags" \
LIBS="-lwasi-emulated-process-clocks -lwasi-emulated-mman" \
ac_cv_func_fork=no \
ac_cv_func_vfork=no \
ac_cv_func_dup2=yes \
ac_cv_func_gethostname=no \
ac_cv_func_malloc_0_nonnull=yes \
ac_cv_func_realloc_0_nonnull=yes \
"$src_dir/configure" \
	--host=wasm32-unknown-wasi \
	--disable-shared \
	--enable-static \
	--disable-nls \
	--without-ncurses \
	--disable-pidof \
	--disable-pidwait \
	--disable-kill \
	--disable-w

make -j"${MAKE_JOBS:-2}" src/ps/pscommand src/pgrep

emit() {
	local source="$1" target="$2"
	mkdir -p "$OUTPUT_DIR"
	if command -v wasm-opt >/dev/null 2>&1; then
		wasm-opt -O3 --strip-debug --all-features "$source" -o "$OUTPUT_DIR/$target"
	else
		cp "$source" "$OUTPUT_DIR/$target"
	fi
}

emit src/ps/pscommand ps
emit src/pgrep pgrep
emit src/pgrep pkill
popd >/dev/null

echo "Built upstream procps-ng $VERSION: ps pgrep pkill"
