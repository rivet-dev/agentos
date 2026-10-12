#!/usr/bin/env bash
# Build real protocol libraries against the caller's owned POSIX sysroot.
set -euo pipefail
: "${CC:?set CC to the owned sysroot compiler}"
: "${AR:?set AR}"
: "${RANLIB:?set RANLIB}"
: "${OPENSSL_PREFIX:?set OpenSSL 3.5.7 prefix}"
: "${PREFIX:?set absolute protocol dependency prefix}"
: "${CACHE_DIR:?set absolute source cache}"
: "${JOBS:=4}"
[[ "$PREFIX" = /* && "$CACHE_DIR" = /* && "$OPENSSL_PREFIX" = /* ]] || { echo 'Prefixes must be absolute' >&2; exit 2; }
[[ "$JOBS" =~ ^[1-9][0-9]*$ && "$JOBS" -le 16 ]] || { echo 'JOBS must be 1..16' >&2; exit 2; }
source "$(dirname "${BASH_SOURCE[0]}")/resolve-generation-prefix.sh"
OPENSSL_PREFIX=$(resolve_generation_prefix "$OPENSSL_PREFIX")
for archive in libssl.a libcrypto.a; do test -s "$OPENSSL_PREFIX/lib/$archive"; done
mkdir -p "$CACHE_DIR" "$(dirname "$PREFIX")"
exec 9>"$PREFIX.lock"
flock 9
source "$(dirname "${BASH_SOURCE[0]}")/check-generation-limit.sh"
work=$(mktemp -d "${PREFIX}.build.XXXXXXXX")
generation=
installed=0
cleanup() { rm -rf "$work"; if [[ $installed == 0 && -n $generation && ! "$PREFIX" -ef "$generation" ]]; then rm -rf "$generation"; fi; }
trap cleanup EXIT
stage="$work/prefix"
mkdir -p "$stage"
export CC AR RANLIB
export CFLAGS="${CFLAGS:--O2 -flto}"
export CPPFLAGS="-I$OPENSSL_PREFIX/include"
export LDFLAGS="-L$OPENSSL_PREFIX/lib"
export PKG_CONFIG_PATH=
export PKG_CONFIG_LIBDIR="$stage/lib/pkgconfig:$OPENSSL_PREFIX/lib/pkgconfig"
fetch() {
  local name=$1 version=$2 url=$3 digest=$4 archive="$CACHE_DIR/$1-$2.tar.xz"
  if [[ ! -f "$archive" ]]; then
    curl --fail --location --retry 3 --connect-timeout 20 --max-time 180 "$url" -o "$work/download"
    printf '%s  %s\n' "$digest" "$work/download" | sha256sum --check --status
    mv "$work/download" "$archive"
  fi
  printf '%s  %s\n' "$digest" "$archive" | sha256sum --check --status
  tar -xf "$archive" -C "$work"
}
build() {
  local name=$1 version=$2; shift 2
  (
    cd "$work/$name-$version"
    ./configure --host=wasm32-unknown-wasi --prefix="$stage" --libdir="$stage/lib" --disable-shared --enable-static "$@"
    make -j"$JOBS"
    make install
  )
}
fetch nghttp2 1.71.0 https://github.com/nghttp2/nghttp2/releases/download/v1.71.0/nghttp2-1.71.0.tar.xz c0f55121d88650e4b9227aec28cebaf664d2417ca60fe70be0f8d38829f52116
build nghttp2 1.71.0 --enable-lib-only
fetch nghttp3 1.19.0 https://github.com/ngtcp2/nghttp3/releases/download/v1.19.0/nghttp3-1.19.0.tar.xz 228e17f5cc3d4de7bfced9912bd09de30e7f45741cc47503cf88d3fa9209e155
build nghttp3 1.19.0 --enable-lib-only
fetch ngtcp2 1.26.0 https://github.com/ngtcp2/ngtcp2/releases/download/v1.26.0/ngtcp2-1.26.0.tar.xz 7804aa0fa1c39942ba322a90805824c37231f7675b08d0c3cc4332858a7b085e
build ngtcp2 1.26.0 --enable-lib-only --disable-cryptotest --with-openssl
fetch libssh2 1.11.1 https://libssh2.org/download/libssh2-1.11.1.tar.xz 9954cb54c4f548198a7cbebad248bdc87dd64bd26185708a294b2b50771e3769
build libssh2 1.11.1 --with-crypto=openssl --without-libz --disable-examples-build
for archive in libnghttp2.a libnghttp3.a libngtcp2.a libngtcp2_crypto_ossl.a libssh2.a; do test -s "$stage/lib/$archive"; done
# Install into immutable generations, then replace one symlink atomically.
# Reject a legacy directory so no reader observes a temporarily missing prefix.
[[ ! -e "$PREFIX" || -L "$PREFIX" ]] || { echo "Convert legacy prefix $PREFIX before building" >&2; exit 2; }
generation=$(mktemp -d "${PREFIX}.generation.XXXXXXXX")
# pkg-config paths must refer to the permanent generation, never the work dir.
python3 - "$stage" "$generation" <<'PY'
import pathlib,sys
root,final=map(pathlib.Path,sys.argv[1:])
for pattern in ('*.pc','*.la'):
 for path in root.rglob(pattern):
  path.write_text(path.read_text().replace(str(root),str(final)))
PY
cp -a "$stage/." "$generation/"
printf 'agentos-immutable-prefix-v1\n' > "$generation/.agentos-generation"
ln -s "$(basename "$generation")" "$work/current"
mv -Tf "$work/current" "$PREFIX"
installed=1
