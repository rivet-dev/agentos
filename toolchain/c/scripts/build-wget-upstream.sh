#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-wget-upstream.sh \
  --version <wget-version> \
  --url <release-url> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --overlay-include-dir <include-dir> \
  --cc <cc> \
  --ar <ar> \
  --ranlib <ranlib> \
  --output <output>
EOF
}

VERSION=""
URL=""
CACHE_DIR=""
BUILD_DIR=""
OVERLAY_INCLUDE_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="$2"
      shift 2
      ;;
    --url)
      URL="$2"
      shift 2
      ;;
    --cache-dir)
      CACHE_DIR="$2"
      shift 2
      ;;
    --build-dir)
      BUILD_DIR="$2"
      shift 2
      ;;
    --overlay-include-dir)
      OVERLAY_INCLUDE_DIR="$2"
      shift 2
      ;;
    --cc)
      CC_CMD="$2"
      shift 2
      ;;
    --ar)
      AR_CMD="$2"
      shift 2
      ;;
    --ranlib)
      RANLIB_CMD="$2"
      shift 2
      ;;
    --output)
      OUTPUT="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" || -z "$URL" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$OVERLAY_INCLUDE_DIR" || -z "$CC_CMD" || -z "$AR_CMD" || -z "$RANLIB_CMD" || -z "$OUTPUT" ]]; then
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

TARBALL="$CACHE_DIR/wget-${VERSION}.tar.gz"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream GNU Wget ${VERSION} release tarball..."
  fetch "$URL" "$TARBALL"
fi

echo "Extracting upstream GNU Wget ${VERSION}..."
tar -xzf "$TARBALL" -C "$BUILD_DIR"

SRC_DIR="$BUILD_DIR/wget-${VERSION}"
if [[ ! -d "$SRC_DIR" ]]; then
  echo "Expected extracted source at $SRC_DIR" >&2
  exit 1
fi

pushd "$SRC_DIR" >/dev/null

echo "Configuring upstream GNU Wget for wasm32-wasip1..."
CC="$CC_CMD" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
PKG_CONFIG=false \
NETTLE_CFLAGS="" \
NETTLE_LIBS="" \
PCRE2_CFLAGS="" \
PCRE2_LIBS="" \
PCRE_CFLAGS="" \
PCRE_LIBS="" \
CPPFLAGS="-I$OVERLAY_INCLUDE_DIR" \
gl_cv_func_posix_spawn_works=yes \
gl_cv_func_posix_spawn_secure_exec=yes \
gl_cv_func_posix_spawnp_secure_exec=yes \
gl_cv_func_posix_spawn_file_actions_addclose_works=yes \
gl_cv_func_posix_spawn_file_actions_adddup2_works=yes \
gl_cv_func_posix_spawn_file_actions_addopen_works=yes \
gl_cv_func_select_supports0=yes \
gl_cv_func_select_detects_ebadf=yes \
gl_cv_func_pselect_detects_ebadf=yes \
ac_cv_func_clock_getres=no \
ac_cv_func_clock_gettime=no \
CFLAGS="-O2 -flto -D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS -DFD_SETSIZE=8192" \
LIBS="-lwasi-emulated-signal -lwasi-emulated-mman -lwasi-emulated-process-clocks" \
./configure \
  --host=wasm32-unknown-wasi \
  --disable-shared \
  --disable-nls \
  --disable-iri \
  --disable-digest \
  --disable-ntlm \
  --disable-opie \
  --disable-pcre \
  --disable-pcre2 \
  --without-ssl \
  --without-libpsl \
  --without-zlib \
  --without-libuuid \
  --without-metalink

echo "Building upstream GNU Wget support library..."
make -C lib

echo "Building upstream GNU Wget..."
make -C src wget

BIN=""
for candidate in "src/wget" "src/.libs/wget" "src/wget.wasm"; do
  if [[ -f "$candidate" ]]; then
    BIN="$candidate"
    break
  fi
done

if [[ -z "$BIN" ]]; then
  echo "Unable to locate built wget binary in src/" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
if command -v wasm-opt >/dev/null 2>&1; then
  echo "Optimizing Wget WASM binary..."
  wasm-opt -O3 --strip-debug --all-features "$BIN" -o "$OUTPUT"
else
  cp "$BIN" "$OUTPUT"
fi

popd >/dev/null

echo "Built upstream GNU Wget at $OUTPUT"
