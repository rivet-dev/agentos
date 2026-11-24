#!/usr/bin/env bash
# Native validation tools only; never used as guest protocol implementations.
set -euo pipefail
: "${PREFIX:?set absolute native fixture prefix}"
: "${CACHE_DIR:?set absolute source cache}"
: "${JOBS:=4}"
[[ "$PREFIX" = /* && "$CACHE_DIR" = /* ]] || exit 2
[[ "$JOBS" =~ ^[1-9][0-9]*$ && "$JOBS" -le 16 ]] || exit 2
mkdir -p "$CACHE_DIR" "$(dirname "$PREFIX")"
exec 9>"$PREFIX.lock"; flock 9
source "$(dirname "${BASH_SOURCE[0]}")/check-generation-limit.sh"
[[ ! -e "$PREFIX" || -L "$PREFIX" ]] || { echo "Expected symlink prefix: $PREFIX" >&2; exit 2; }
work=$(mktemp -d "${PREFIX}.build.XXXXXXXX")
generation=$(mktemp -d "${PREFIX}.generation.XXXXXXXX")
installed=0
cleanup() { rm -rf "$work"; if [[ $installed == 0 && ! "$PREFIX" -ef "$generation" ]]; then rm -rf "$generation"; fi; }
trap cleanup EXIT
# An explicitly native compiler is required so callers cannot inherit a WASM CC.
export CC="${NATIVE_CC:-cc}" CXX="${NATIVE_CXX:-c++}" AR="${NATIVE_AR:-ar}" RANLIB="${NATIVE_RANLIB:-ranlib}"
export CFLAGS=-O2 CXXFLAGS=-O2
export CPPFLAGS="-I$generation/include" LDFLAGS="-L$generation/lib"
export PKG_CONFIG_PATH= PKG_CONFIG_LIBDIR="$generation/lib/pkgconfig"
unset PKG_CONFIG_SYSROOT_DIR LIBS
# nghttp2's native applications require C++23, including the standard print header.
# Fail before building the dependency chain if the selected host compiler lacks it.
printf '#include <print>\nint main() { std::print(""); }\n' > "$work/cxx23.cpp"
if ! $CXX -std=c++23 "$work/cxx23.cpp" -o "$work/cxx23"; then
  echo 'A C++23 host compiler/library is required; set NATIVE_CXX to a supported compiler.' >&2
  exit 2
fi
fetch() {
  local name=$1 url=$2 digest=$3 archive="$CACHE_DIR/$1"
  if [[ ! -f "$archive" ]]; then
    curl --fail --location --retry 3 --connect-timeout 20 --max-time 180 "$url" -o "$work/download"
    printf '%s  %s\n' "$digest" "$work/download" | sha256sum --check --status
    mv "$work/download" "$archive"
  fi
  printf '%s  %s\n' "$digest" "$archive" | sha256sum --check --status
  tar -xf "$archive" -C "$work"
}
build() {
  local source=$1; shift
  (cd "$work/$source"; ./configure --prefix="$generation" --libdir="$generation/lib" --disable-shared --enable-static "$@"; make -j"$JOBS"; make install)
}
fetch openssl-3.5.7.tar.gz https://github.com/openssl/openssl/releases/download/openssl-3.5.7/openssl-3.5.7.tar.gz a8c0d28a529ca480f9f36cf5792e2cd21984552a3c8e4aa11a24aa31aeac98e8
(cd "$work/openssl-3.5.7"; ./config --prefix="$generation" --libdir=lib no-shared no-tests; make -j"$JOBS" build_libs; make install_dev)
fetch zlib-1.4.tar.xz https://github.com/madler/zlib/releases/download/v1.4/zlib-1.4.tar.xz 4f11c512053d7bab8d9e9b89bde3575add3e4c50a383b42ef6cb837035fc5a7c
(cd "$work/zlib-1.4"; ./configure --prefix="$generation" --static; make -j"$JOBS"; make install)
fetch libev-4.33.tar.gz https://dist.schmorp.de/libev/Attic/libev-4.33.tar.gz 507eb7b8d1015fbec5b935f34ebed15bf346bed04a11ab82b8eee848c4205aea
build libev-4.33
fetch c-ares-1.34.9.tar.gz https://github.com/c-ares/c-ares/releases/download/v1.34.9/c-ares-1.34.9.tar.gz 9364b70a12eb59a06830fb488d9357f90100dff0d98b23ea3593a183d43e6f2f
build c-ares-1.34.9 --disable-tests
fetch nghttp3-1.19.0.tar.xz https://github.com/ngtcp2/nghttp3/releases/download/v1.19.0/nghttp3-1.19.0.tar.xz 228e17f5cc3d4de7bfced9912bd09de30e7f45741cc47503cf88d3fa9209e155
build nghttp3-1.19.0 --enable-lib-only
fetch ngtcp2-1.26.0.tar.xz https://github.com/ngtcp2/ngtcp2/releases/download/v1.26.0/ngtcp2-1.26.0.tar.xz 7804aa0fa1c39942ba322a90805824c37231f7675b08d0c3cc4332858a7b085e
build ngtcp2-1.26.0 --enable-lib-only --disable-cryptotest --with-openssl
fetch nghttp2-1.71.0.tar.xz https://github.com/nghttp2/nghttp2/releases/download/v1.71.0/nghttp2-1.71.0.tar.xz c0f55121d88650e4b9227aec28cebaf664d2417ca60fe70be0f8d38829f52116
export LIBEV_CFLAGS="-I$generation/include" LIBEV_LIBS="$generation/lib/libev.a -lm"
build nghttp2-1.71.0 --enable-app --enable-http3 --disable-examples --disable-hpack-tools --with-openssl --with-libev --with-libcares --without-libxml2 --without-jansson --without-libevent-openssl --without-jemalloc --without-libbpf
fetch curl-8.23.0.tar.xz https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz 8dadd7d2536a314f982ad05a651b713f426bf7d02cc2c76cffb9796f2218e6c5
build curl-8.23.0 --with-openssl="$generation" --with-ngtcp2="$generation" --with-nghttp3="$generation" --with-nghttp2="$generation" --with-zlib="$generation" --without-libpsl --without-brotli --without-zstd --without-libssh2 --without-libssh --disable-ldap
# Build success alone cannot certify the selected backend.
grep -Eq '^#define ENABLE_HTTP3 1$' "$work/nghttp2-1.71.0/config.h"
"$generation/bin/curl" --version | grep -q HTTP3
"$generation/bin/nghttpx" --version
ln -s "$(basename "$generation")" "$work/current"
mv -Tf "$work/current" "$PREFIX"
installed=1
printf 'AGENTOS_NGHTTPX_HTTP3_SERVER=%s/bin/nghttpx\nAGENTOS_NATIVE_HTTP3_CURL=%s/bin/curl\n' "$generation" "$generation"
