#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-ssh-upstream.sh \
  --version <openssh-version> \
  --url <release-url> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --zlib-include <dir> \
  --zlib-libdir <dir> \
  --overlay-include-dir <dir> \
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
ZLIB_INCLUDE=""
ZLIB_LIBDIR=""
OVERLAY_INCLUDE_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --url) URL="$2"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --zlib-include) ZLIB_INCLUDE="$2"; shift 2 ;;
    --zlib-libdir) ZLIB_LIBDIR="$2"; shift 2 ;;
    --overlay-include-dir) OVERLAY_INCLUDE_DIR="$2"; shift 2 ;;
    --cc) CC_CMD="$2"; shift 2 ;;
    --ar) AR_CMD="$2"; shift 2 ;;
    --ranlib) RANLIB_CMD="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" || -z "$URL" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$ZLIB_INCLUDE" || -z "$ZLIB_LIBDIR" || -z "$OVERLAY_INCLUDE_DIR" || -z "$CC_CMD" || -z "$AR_CMD" || -z "$RANLIB_CMD" || -z "$OUTPUT" ]]; then
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

TARBALL="$CACHE_DIR/openssh-${VERSION}.tar.gz"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream OpenSSH ${VERSION} release tarball..."
  fetch "$URL" "$TARBALL"
fi

echo "Extracting upstream OpenSSH ${VERSION}..."
tar -xzf "$TARBALL" -C "$BUILD_DIR"

SRC_DIR="$BUILD_DIR/openssh-${VERSION}"
if [[ ! -d "$SRC_DIR" ]]; then
  echo "Expected extracted source at $SRC_DIR" >&2
  exit 1
fi

# OpenSSH's --with-zlib=PATH expects a normal install prefix (PATH/include,
# PATH/lib). Our zlib artifacts live in the source tree + a separate build
# dir, so stage a prefix out of symlinks.
ZLIB_PREFIX="$BUILD_DIR/zlib-prefix"
mkdir -p "$ZLIB_PREFIX/include" "$ZLIB_PREFIX/lib"
ln -sf "$ZLIB_INCLUDE/zlib.h" "$ZLIB_PREFIX/include/zlib.h"
ln -sf "$ZLIB_INCLUDE/zconf.h" "$ZLIB_PREFIX/include/zconf.h"
ln -sf "$ZLIB_LIBDIR/libz.a" "$ZLIB_PREFIX/lib/libz.a"

pushd "$SRC_DIR" >/dev/null

# Batch/key-based OpenSSH client (RFC 4251 architecture, RFC 4253 transport,
# RFC 4252 auth, RFC 4254 connection) built --without-openssl: the internal
# crypto set is ed25519 signatures, curve25519-sha256 KEX and
# chacha20-poly1305@openssh.com (see upstream README + `ssh -Q` on such a
# build). No RSA/ECDSA, no PKCS#11, no FIDO/security keys.
#
# Stack layout mirrors the git build: wasm-ld's default 64 KiB stack above the
# data segment is far too small for OpenSSH (packet buffers and kex state live
# on the stack several frames deep), so give it an 8 MiB stack and
# --stack-first so overflows trap instead of corrupting static data.
SSH_WASM_STACK_FLAGS="-Wl,-z,stack-size=8388608 -Wl,--stack-first"

# Autoconf cache seeds for cross-compilation gaps (mirrors the wget build's
# gl_cv_*/ac_cv_* seeding):
# - ac_cv_func_setrlimit=no: the patched sysroot exports a getrlimit-only
#   surface (std-patches/wasi-libc/0017-resource-limits-and-groups.patch);
#   OpenSSH only uses setrlimit to zero core dumps / fd limits, which the
#   kernel already enforces per-VM.
# - ac_cv_have_broken_snprintf/vsnprintf=no: run tests can't execute under
#   cross builds; the patched musl snprintf is C99-conformant, so declare it
#   instead of letting configure assume the worst and pull in replacements.
#
# CFLAGS carries -I$OVERLAY_INCLUDE_DIR (toolchain/c/include): the shared
# include_next overlays used by every C command build. OpenSSH needs
# <sys/ioctl.h>'s struct winsize + TIOCGWINSZ (tty(4) window-size ioctl) from
# that overlay for channels.c's window-change tracking; at runtime the
# patched sysroot ioctl() reports ENOTTY on non-PTY fds and the client just
# skips the update.
echo "Configuring upstream OpenSSH for wasm32-wasip1 (--without-openssl)..."
CC="$CC_CMD" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
CFLAGS="-O2 -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_MMAN -I$OVERLAY_INCLUDE_DIR" \
LDFLAGS="$SSH_WASM_STACK_FLAGS" \
LIBS="-lwasi-emulated-process-clocks -lwasi-emulated-mman" \
ac_cv_func_setrlimit=no \
ac_cv_have_broken_snprintf=no \
ac_cv_have_broken_vsnprintf=no \
./configure \
  --host=wasm32-unknown-wasi \
  --without-openssl \
  --with-zlib="$ZLIB_PREFIX" \
  --without-pam \
  --without-selinux \
  --without-libedit \
  --without-audit \
  --disable-utmp \
  --disable-wtmp \
  --disable-lastlog \
  --disable-btmp \
  --disable-pkcs11 \
  --disable-security-key \
  --without-stackprotect \
  --without-hardening \
  --sysconfdir=/etc/ssh

echo "Building upstream OpenSSH ssh client..."
make -j"${MAKE_JOBS:-2}" ssh

if [[ ! -f ssh ]]; then
  echo "Expected ssh binary was not produced" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
if command -v wasm-opt >/dev/null 2>&1; then
  echo "Optimizing OpenSSH ssh WASM binary..."
  wasm-opt -O3 --strip-debug --all-features ssh -o "$OUTPUT"
else
  cp ssh "$OUTPUT"
fi

popd >/dev/null

echo "Built upstream OpenSSH ssh at $OUTPUT"
