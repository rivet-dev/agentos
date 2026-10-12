#!/usr/bin/env bash
# The curl executable and Git's libcurl must use this single artifact generation.
set -euo pipefail
for var in CC AR RANLIB OPENSSL_PREFIX PROTOCOL_PREFIX COMPRESSION_PREFIX PREFIX CACHE_DIR CA_BUNDLE; do
  [[ -n ${!var:-} ]] || { echo "$var is required" >&2; exit 2; }
done
: "${JOBS:=4}"
[[ "$JOBS" =~ ^[1-9][0-9]*$ && "$JOBS" -le 16 ]] || exit 2
[[ "$PREFIX" = /* && "$CACHE_DIR" = /* ]] || exit 2
source "$(dirname "${BASH_SOURCE[0]}")/resolve-generation-prefix.sh"
OPENSSL_PREFIX=$(resolve_generation_prefix "$OPENSSL_PREFIX")
PROTOCOL_PREFIX=$(resolve_generation_prefix "$PROTOCOL_PREFIX")
COMPRESSION_PREFIX=$(resolve_generation_prefix "$COMPRESSION_PREFIX")
mkdir -p "$CACHE_DIR" "$(dirname "$PREFIX")"
exec 9>"$PREFIX.lock"; flock 9
source "$(dirname "${BASH_SOURCE[0]}")/check-generation-limit.sh"
work=$(mktemp -d "${PREFIX}.build.XXXXXXXX")
generation=
installed=0
cleanup() { rm -rf "$work"; if [[ $installed == 0 && -n $generation && ! "$PREFIX" -ef "$generation" ]]; then rm -rf "$generation"; fi; }
trap cleanup EXIT
archive="$CACHE_DIR/curl-8.23.0.tar.xz"
digest=8dadd7d2536a314f982ad05a651b713f426bf7d02cc2c76cffb9796f2218e6c5
if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 3 --connect-timeout 20 --max-time 180 https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz -o "$work/download"
  printf '%s  %s\n' "$digest" "$work/download" | sha256sum --check --status
  mv "$work/download" "$archive"
fi
printf '%s  %s\n' "$digest" "$archive" | sha256sum --check --status
tar -xf "$archive" -C "$work"
export CC AR RANLIB
export PKG_CONFIG_PATH=
export PKG_CONFIG_LIBDIR="$PROTOCOL_PREFIX/lib/pkgconfig:$OPENSSL_PREFIX/lib/pkgconfig:$COMPRESSION_PREFIX/lib/pkgconfig"
export CFLAGS="${CFLAGS:--O2 -flto}"
export CPPFLAGS="-I$PROTOCOL_PREFIX/include -I$OPENSSL_PREFIX/include -I$COMPRESSION_PREFIX/include"
# The QUIC receive path alone needs more than wasm-ld's default 64 KiB stack.
# Put the stack before static data so overflow traps instead of corrupting globals.
export LDFLAGS="-L$PROTOCOL_PREFIX/lib -L$OPENSSL_PREFIX/lib -L$COMPRESSION_PREFIX/lib -Wl,-z,stack-size=1048576 -Wl,--stack-first"
export LIBS='-lbrotlicommon'
patch_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/../patches/curl" && pwd)
cd "$work/curl-8.23.0"
patch --batch --forward --fuzz=0 -p1 < "$patch_dir/0001-clear-failed-pthread-probe.patch"
./configure --host=wasm32-unknown-wasi --prefix="$work/install" --libdir="$work/install/lib" \
  --disable-shared --enable-static --disable-threaded-resolver --disable-ldap --without-libpsl \
  --with-openssl="$OPENSSL_PREFIX" --with-nghttp2="$PROTOCOL_PREFIX" \
  --with-nghttp3="$PROTOCOL_PREFIX" --with-ngtcp2="$PROTOCOL_PREFIX" \
  --with-libssh2="$PROTOCOL_PREFIX" --with-zlib="$COMPRESSION_PREFIX" \
  --with-brotli="$COMPRESSION_PREFIX" --with-zstd="$COMPRESSION_PREFIX" --with-ca-bundle="$CA_BUNDLE"
for feature in USE_OPENSSL USE_NGHTTP2 USE_NGHTTP3 USE_NGTCP2 USE_LIBSSH2; do
  grep -Eq "^#define $feature 1$" lib/curl_config.h || { echo "Required curl backend missing: $feature" >&2; exit 1; }
done
make -j"$JOBS" -C lib libcurl.la
make -j"$JOBS" -C src curl
make -C lib install
make -C include install
mkdir -p "$work/install/bin" "$work/install/share/agentos"
if [[ -f src/.libs/curl ]]; then binary=src/.libs/curl; else binary=src/curl; fi
install -m755 "$binary" "$work/install/bin/curl.wasm"
cp lib/curl_config.h "$work/install/share/agentos/curl_config.h"
# One canonical static link description for Git and other consumers.
./curl-config --static-libs > "$work/install/share/agentos/curl-static-libs"
test -s "$work/install/lib/libcurl.a"
test -s "$work/install/include/curl/curl.h"
[[ ! -e "$PREFIX" || -L "$PREFIX" ]] || { echo "Convert legacy prefix $PREFIX before building" >&2; exit 2; }
generation=$(mktemp -d "${PREFIX}.generation.XXXXXXXX")
python3 - "$work/install" "$generation" <<'PY'
import pathlib,sys
root,final=map(pathlib.Path,sys.argv[1:])
for path in [*root.rglob('*.pc'),*root.rglob('*.la'),root/'share/agentos/curl-static-libs']:
 path.write_text(path.read_text().replace(str(root),str(final)))
PY
cp -a "$work/install/." "$generation/"
printf 'agentos-immutable-prefix-v1\n' > "$generation/.agentos-generation"
ln -s "$(basename "$generation")" "$work/current"
mv -Tf "$work/current" "$PREFIX"
installed=1
