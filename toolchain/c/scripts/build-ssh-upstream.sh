#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-ssh-upstream.sh \
  --version <openssh-version> \
  --url <release-url> \
  --sha256 <release-sha256> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --patch-dir <patch-dir> \
  --zlib-include <dir> \
  --zlib-libdir <dir> \
  --openssl-prefix <dir> \
  --overlay-include-dir <dir> \
  --cc <cc> \
  --ar <ar> \
  --ranlib <ranlib> \
  --output <ssh-output> \
  --keysign-output <ssh-keysign-output> \
  --sk-helper-output <ssh-sk-helper-output>
EOF
}

VERSION=""
URL=""
SHA256=""
CACHE_DIR=""
BUILD_DIR=""
PATCH_DIR=""
ZLIB_INCLUDE=""
ZLIB_LIBDIR=""
OPENSSL_PREFIX=""
OVERLAY_INCLUDE_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
OUTPUT=""
KEYSIGN_OUTPUT=""
SK_HELPER_OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --url) URL="$2"; shift 2 ;;
    --sha256) SHA256="$2"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --patch-dir) PATCH_DIR="$2"; shift 2 ;;
    --zlib-include) ZLIB_INCLUDE="$2"; shift 2 ;;
    --zlib-libdir) ZLIB_LIBDIR="$2"; shift 2 ;;
    --openssl-prefix) OPENSSL_PREFIX="$2"; shift 2 ;;
    --overlay-include-dir) OVERLAY_INCLUDE_DIR="$2"; shift 2 ;;
    --cc) CC_CMD="$2"; shift 2 ;;
    --ar) AR_CMD="$2"; shift 2 ;;
    --ranlib) RANLIB_CMD="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --keysign-output) KEYSIGN_OUTPUT="$2"; shift 2 ;;
    --sk-helper-output) SK_HELPER_OUTPUT="$2"; shift 2 ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" || -z "$URL" || -z "$SHA256" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$PATCH_DIR" || -z "$ZLIB_INCLUDE" || -z "$ZLIB_LIBDIR" || -z "$OPENSSL_PREFIX" || -z "$OVERLAY_INCLUDE_DIR" || -z "$CC_CMD" || -z "$AR_CMD" || -z "$RANLIB_CMD" || -z "$OUTPUT" || -z "$KEYSIGN_OUTPUT" || -z "$SK_HELPER_OUTPUT" ]]; then
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

verify_sha256() {
  local file="$1"
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    echo "Neither sha256sum nor shasum is available to verify $file" >&2
    return 1
  fi
  if [[ "$actual" != "$SHA256" ]]; then
    echo "OpenSSH ${VERSION} archive SHA-256 mismatch: expected $SHA256, got $actual" >&2
    return 1
  fi
}

TARBALL="$CACHE_DIR/openssh-${VERSION}.tar.gz"
TARBALL_TMP="${TARBALL}.tmp.$$"
BUILD_TMP="${BUILD_DIR}.tmp.$$"
BUILD_OLD="${BUILD_DIR}.old.$$"
OUTPUT_TMP="${OUTPUT}.tmp.$$"
KEYSIGN_OUTPUT_TMP="${KEYSIGN_OUTPUT}.tmp.$$"
SK_HELPER_OUTPUT_TMP="${SK_HELPER_OUTPUT}.tmp.$$"
OUTPUT_OLD="${OUTPUT}.old.$$"
KEYSIGN_OUTPUT_OLD="${KEYSIGN_OUTPUT}.old.$$"
SK_HELPER_OUTPUT_OLD="${SK_HELPER_OUTPUT}.old.$$"
BUILD_HAD_OLD=0
OUTPUT_HAD_OLD=0
KEYSIGN_OUTPUT_HAD_OLD=0
SK_HELPER_OUTPUT_HAD_OLD=0
BUILD_INSTALLED=0
OUTPUT_INSTALLED=0
KEYSIGN_OUTPUT_INSTALLED=0
SK_HELPER_OUTPUT_INSTALLED=0
PROMOTION_STARTED=0
PROMOTION_COMPLETE=0
cleanup() {
  rm -f "$TARBALL_TMP" "$OUTPUT_TMP" "$KEYSIGN_OUTPUT_TMP" "$SK_HELPER_OUTPUT_TMP"
  rm -rf "$BUILD_TMP"
  if [[ "$PROMOTION_STARTED" = 1 && "$PROMOTION_COMPLETE" = 0 ]]; then
    if [[ "$BUILD_INSTALLED" = 1 ]]; then rm -rf "$BUILD_DIR"; fi
    if [[ "$OUTPUT_INSTALLED" = 1 ]]; then rm -rf "$OUTPUT"; fi
    if [[ "$KEYSIGN_OUTPUT_INSTALLED" = 1 ]]; then rm -rf "$KEYSIGN_OUTPUT"; fi
    if [[ "$SK_HELPER_OUTPUT_INSTALLED" = 1 ]]; then rm -rf "$SK_HELPER_OUTPUT"; fi
    if [[ "$BUILD_HAD_OLD" = 1 && -e "$BUILD_OLD" ]]; then
      rm -rf "$BUILD_DIR"
      mv "$BUILD_OLD" "$BUILD_DIR"
    fi
    if [[ "$OUTPUT_HAD_OLD" = 1 && -e "$OUTPUT_OLD" ]]; then
      rm -rf "$OUTPUT"
      mv "$OUTPUT_OLD" "$OUTPUT"
    fi
    if [[ "$KEYSIGN_OUTPUT_HAD_OLD" = 1 && -e "$KEYSIGN_OUTPUT_OLD" ]]; then
      rm -rf "$KEYSIGN_OUTPUT"
      mv "$KEYSIGN_OUTPUT_OLD" "$KEYSIGN_OUTPUT"
    fi
    if [[ "$SK_HELPER_OUTPUT_HAD_OLD" = 1 && -e "$SK_HELPER_OUTPUT_OLD" ]]; then
      rm -rf "$SK_HELPER_OUTPUT"
      mv "$SK_HELPER_OUTPUT_OLD" "$SK_HELPER_OUTPUT"
    fi
  fi
}
trap cleanup EXIT

rm -rf "$BUILD_OLD" "$OUTPUT_OLD" "$KEYSIGN_OUTPUT_OLD" \
  "$SK_HELPER_OUTPUT_OLD"

mkdir -p "$CACHE_DIR"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream OpenSSH ${VERSION} release tarball..."
  fetch "$URL" "$TARBALL_TMP"
  verify_sha256 "$TARBALL_TMP"
  mv "$TARBALL_TMP" "$TARBALL"
else
  verify_sha256 "$TARBALL"
fi

echo "Extracting upstream OpenSSH ${VERSION}..."
rm -rf "$BUILD_TMP"
mkdir -p "$BUILD_TMP"
tar -xzf "$TARBALL" -C "$BUILD_TMP"

SRC_DIR="$BUILD_TMP/openssh-${VERSION}"
if [[ ! -d "$SRC_DIR" ]]; then
  echo "Expected extracted source at $SRC_DIR" >&2
  exit 1
fi

# OpenSSH's --with-zlib=PATH expects a normal install prefix (PATH/include,
# PATH/lib). Our zlib artifacts live in the source tree + a separate build
# dir, so stage a prefix out of symlinks.
ZLIB_PREFIX="$BUILD_TMP/zlib-prefix"
mkdir -p "$ZLIB_PREFIX/include" "$ZLIB_PREFIX/lib"
ln -sf "$ZLIB_INCLUDE/zlib.h" "$ZLIB_PREFIX/include/zlib.h"
ln -sf "$ZLIB_INCLUDE/zconf.h" "$ZLIB_PREFIX/include/zconf.h"
ln -sf "$ZLIB_LIBDIR/libz.a" "$ZLIB_PREFIX/lib/libz.a"

pushd "$SRC_DIR" >/dev/null

for patch_file in "$PATCH_DIR"/*.patch; do
  [[ -e "$patch_file" ]] || continue
  echo "Applying OpenSSH patch $(basename "$patch_file")..."
  patch -p1 < "$patch_file"
done

# Batch/key-based OpenSSH client (RFC 4251 architecture, RFC 4253 transport,
# RFC 4252 auth, RFC 4254 connection) linked with the hermetic OpenSSL
# libcrypto build. This restores the RSA, ECDSA, DH, NIST ECDH, AES-GCM,
# AES-CBC, and 3DES families exposed by a normal Linux OpenSSH client.
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
echo "Configuring upstream OpenSSH for wasm32-wasip1 with OpenSSL libcrypto..."
CC="$CC_CMD" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
CFLAGS="-O2 -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_MMAN -DAGENTOS_NO_PROCESS_SNAPSHOT=1 -I$OVERLAY_INCLUDE_DIR" \
CPPFLAGS="-I$OPENSSL_PREFIX/include" \
LDFLAGS="-L$OPENSSL_PREFIX/lib $SSH_WASM_STACK_FLAGS" \
LIBS="-lwasi-emulated-process-clocks -lwasi-emulated-mman" \
ac_cv_func_setrlimit=no \
ac_cv_func_sendmsg=yes \
ac_cv_func_recvmsg=yes \
ac_cv_func_dlopen=yes \
ac_cv_have_decl_RTLD_NOW=yes \
ac_cv_have_broken_snprintf=no \
ac_cv_have_broken_vsnprintf=no \
./configure \
  --host=wasm32-unknown-wasi \
  --with-ssl-dir="$OPENSSL_PREFIX" \
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
  --with-security-key-builtin=no \
  --libexecdir=/opt/agentos/bin \
  --sysconfdir=/etc/ssh

echo "Building upstream OpenSSH client and client-side helpers..."
make -j"${MAKE_JOBS:-2}" ssh ssh-keysign ssh-sk-helper

for binary in ssh ssh-keysign ssh-sk-helper; do
  if [[ ! -f "$binary" ]]; then
    echo "Expected $binary binary was not produced" >&2
    exit 1
  fi
done

mkdir -p "$(dirname "$OUTPUT")" "$(dirname "$KEYSIGN_OUTPUT")" \
  "$(dirname "$SK_HELPER_OUTPUT")"
if command -v wasm-opt >/dev/null 2>&1; then
  echo "Optimizing OpenSSH WASM binaries..."
  wasm-opt -O3 --strip-debug --all-features ssh -o "$OUTPUT_TMP"
  wasm-opt -O3 --strip-debug --all-features ssh-keysign -o "$KEYSIGN_OUTPUT_TMP"
  wasm-opt -O3 --strip-debug --all-features ssh-sk-helper -o "$SK_HELPER_OUTPUT_TMP"
else
  cp ssh "$OUTPUT_TMP"
  cp ssh-keysign "$KEYSIGN_OUTPUT_TMP"
  cp ssh-sk-helper "$SK_HELPER_OUTPUT_TMP"
fi

popd >/dev/null

# No last-known-good build tree or output is touched until extraction,
# patching, configure, link, and optimization have all succeeded.
PROMOTION_STARTED=1
if [[ -e "$BUILD_DIR" ]]; then
  mv "$BUILD_DIR" "$BUILD_OLD"
  BUILD_HAD_OLD=1
fi
if [[ -e "$OUTPUT" ]]; then
  mv "$OUTPUT" "$OUTPUT_OLD"
  OUTPUT_HAD_OLD=1
fi
if [[ -e "$KEYSIGN_OUTPUT" ]]; then
  mv "$KEYSIGN_OUTPUT" "$KEYSIGN_OUTPUT_OLD"
  KEYSIGN_OUTPUT_HAD_OLD=1
fi
if [[ -e "$SK_HELPER_OUTPUT" ]]; then
  mv "$SK_HELPER_OUTPUT" "$SK_HELPER_OUTPUT_OLD"
  SK_HELPER_OUTPUT_HAD_OLD=1
fi
mv "$BUILD_TMP" "$BUILD_DIR"
BUILD_INSTALLED=1
mv "$OUTPUT_TMP" "$OUTPUT"
OUTPUT_INSTALLED=1
mv "$KEYSIGN_OUTPUT_TMP" "$KEYSIGN_OUTPUT"
KEYSIGN_OUTPUT_INSTALLED=1
mv "$SK_HELPER_OUTPUT_TMP" "$SK_HELPER_OUTPUT"
SK_HELPER_OUTPUT_INSTALLED=1
PROMOTION_COMPLETE=1
rm -rf "$BUILD_OLD" "$OUTPUT_OLD" "$KEYSIGN_OUTPUT_OLD" \
  "$SK_HELPER_OUTPUT_OLD"
trap - EXIT

echo "Built upstream OpenSSH ssh and helpers at $(dirname "$OUTPUT")"
