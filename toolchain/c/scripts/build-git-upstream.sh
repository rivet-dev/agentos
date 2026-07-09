#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-git-upstream.sh \
  --version <git-version> \
  --url <release-url> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --patch-dir <patch-dir> \
  --zlib-dir <zlib-source-dir> \
  --zlib-build-dir <zlib-build-dir> \
  --curl-prefix <libcurl-install-prefix> \
  --mbedtls-libdir <dir> \
  --brotli-libdir <dir> \
  --zstd-libdir <dir> \
  --cc <cc> \
  --ar <ar> \
  --ranlib <ranlib> \
  --sysroot <sysroot> \
  --output <output> \
  --remote-http-output <output>
EOF
}

VERSION=""
URL=""
CACHE_DIR=""
BUILD_DIR=""
ZLIB_DIR=""
ZLIB_BUILD_DIR=""
CURL_PREFIX=""
MBEDTLS_LIBDIR=""
BROTLI_LIBDIR=""
ZSTD_LIBDIR=""
PATCH_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
SYSROOT=""
OUTPUT=""
REMOTE_HTTP_OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --url) URL="$2"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --patch-dir) PATCH_DIR="$2"; shift 2 ;;
    --zlib-dir) ZLIB_DIR="$2"; shift 2 ;;
    --zlib-build-dir) ZLIB_BUILD_DIR="$2"; shift 2 ;;
    --curl-prefix) CURL_PREFIX="$2"; shift 2 ;;
    --mbedtls-libdir) MBEDTLS_LIBDIR="$2"; shift 2 ;;
    --brotli-libdir) BROTLI_LIBDIR="$2"; shift 2 ;;
    --zstd-libdir) ZSTD_LIBDIR="$2"; shift 2 ;;
    --cc) CC_CMD="$2"; shift 2 ;;
    --ar) AR_CMD="$2"; shift 2 ;;
    --ranlib) RANLIB_CMD="$2"; shift 2 ;;
    --sysroot) SYSROOT="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --remote-http-output) REMOTE_HTTP_OUTPUT="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

if [[ -z "$VERSION" || -z "$URL" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$PATCH_DIR" || -z "$ZLIB_DIR" || -z "$ZLIB_BUILD_DIR" || -z "$CURL_PREFIX" || -z "$MBEDTLS_LIBDIR" || -z "$BROTLI_LIBDIR" || -z "$ZSTD_LIBDIR" || -z "$CC_CMD" || -z "$AR_CMD" || -z "$RANLIB_CMD" || -z "$SYSROOT" || -z "$OUTPUT" || -z "$REMOTE_HTTP_OUTPUT" ]]; then
  usage >&2
  exit 1
fi

fetch() {
  local url="$1"
  local out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl --retry 3 --retry-all-errors -fSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$out"
  else
    echo "Neither curl nor wget is available to fetch $url" >&2
    exit 1
  fi
}

mkdir -p "$CACHE_DIR"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

TARBALL="$CACHE_DIR/git-${VERSION}.tar.xz"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream Git ${VERSION} release tarball..."
  fetch "$URL" "$TARBALL"
fi

echo "Extracting upstream Git ${VERSION}..."
tar -xf "$TARBALL" -C "$BUILD_DIR"

SRC_DIR="$BUILD_DIR/git-${VERSION}"
if [[ ! -d "$SRC_DIR" ]]; then
  echo "Expected extracted source at $SRC_DIR" >&2
  exit 1
fi

pushd "$SRC_DIR" >/dev/null

if [[ -d "$PATCH_DIR" ]]; then
  for patch_file in "$PATCH_DIR"/*.patch; do
    [[ -e "$patch_file" ]] || continue
    echo "Applying $(basename "$patch_file")..."
    patch -p1 < "$patch_file"
  done
fi

# libcurl + its TLS/compression backends, linked in-process by git-remote-http.
# Defining CURL_CFLAGS/CURL_LDFLAGS makes Git's Makefile skip the curl-config
# probe entirely and use these flags verbatim. Static link order matters:
# libcurl first, then mbedTLS (tls -> x509 -> crypto), then zlib and the brotli /
# zstd decoders curl's --compressed path pulls in.
CURL_CFLAGS="-I$CURL_PREFIX/include"
CURL_LDFLAGS="-L$CURL_PREFIX/lib -lcurl -L$MBEDTLS_LIBDIR -lmbedtls -lmbedx509 -lmbedcrypto -L$ZLIB_BUILD_DIR -lz -L$BROTLI_LIBDIR -lbrotlidec -lbrotlicommon -L$ZSTD_LIBDIR -lzstd"

echo "Building upstream Git ${VERSION} for wasm32-wasip1 (with in-process libcurl)..."
# Stack layout: wasm-ld defaults to a 64 KiB stack placed *above* the data
# segment. Git assumes native-sized stacks (recv_sideband alone puts a
# LARGE_PACKET_MAX (~64 KiB) buffer on the stack, reached several frames deep
# on every sideband pack transfer), so with the default layout the buffer
# silently overwrites static data and clones/fetches die with wild wasm traps
# once packs carry payloads bigger than a few KiB. Give git an 8 MiB stack
# (glibc-default sized) and --stack-first so any future overflow traps at the
# faulting frame instead of corrupting the data segment.
GIT_WASM_STACK_FLAGS="-Wl,-z,stack-size=8388608 -Wl,--stack-first"
make -j"${MAKE_JOBS:-2}" \
  uname_S=WASI \
  CC="$CC_CMD" \
  HOSTCC="${HOSTCC:-cc}" \
  AR="$AR_CMD" \
  RANLIB="$RANLIB_CMD" \
  CFLAGS="--target=wasm32-wasip1 --sysroot=$SYSROOT -I$ZLIB_DIR -O2 -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_MMAN" \
  LDFLAGS="--target=wasm32-wasip1 --sysroot=$SYSROOT -L$ZLIB_BUILD_DIR -lwasi-emulated-process-clocks -lwasi-emulated-mman $GIT_WASM_STACK_FLAGS" \
  CURL_CFLAGS="$CURL_CFLAGS" \
  CURL_LDFLAGS="$CURL_LDFLAGS" \
  CSPRNG_METHOD=getentropy \
  HAVE_PATHS_H=YesPlease \
  HAVE_DEV_TTY=YesPlease \
  HAVE_CLOCK_GETTIME=YesPlease \
  HAVE_CLOCK_MONOTONIC=YesPlease \
  HAVE_GETDELIM=YesPlease \
  NO_RUST=1 \
  NO_OPENSSL=1 \
  NO_EXPAT=1 \
  NO_GETTEXT=1 \
  NO_TCLTK=1 \
  NO_PERL=1 \
  NO_PYTHON=1 \
  NO_REGEX=NeedsStartEnd \
  NO_ICONV=1 \
  NO_PTHREADS=1 \
  NO_MMAP=1 \
  NO_SYS_POLL_H=1 \
  NO_NSEC=1 \
  git git-remote-http

if [[ ! -f git-remote-http ]]; then
  echo "Expected git-remote-http binary was not produced" >&2
  exit 1
fi

emit() {
  local src="$1"
  local out="$2"
  mkdir -p "$(dirname "$out")"
  if command -v wasm-opt >/dev/null 2>&1; then
    echo "Optimizing $src WASM binary -> $out..."
    wasm-opt -O3 --strip-debug --all-features "$src" -o "$out"
  else
    cp "$src" "$out"
  fi
}

emit git "$OUTPUT"
emit git-remote-http "$REMOTE_HTTP_OUTPUT"

popd >/dev/null

echo "Built upstream Git at $OUTPUT"
echo "Built git-remote-http at $REMOTE_HTTP_OUTPUT"
