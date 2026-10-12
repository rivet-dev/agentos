#!/usr/bin/env bash
set -euo pipefail
for var in PREFIX ZLIB_INCLUDE ZLIB_LIBDIR BROTLI_INCLUDE BROTLI_LIBDIR ZSTD_INCLUDE ZSTD_LIBDIR; do
  [[ -n ${!var:-} ]] || { echo "$var is required" >&2; exit 2; }
done
mkdir -p "$(dirname "$PREFIX")"
exec 9>"$PREFIX.lock"; flock 9
source "$(dirname "${BASH_SOURCE[0]}")/check-generation-limit.sh"
[[ ! -e "$PREFIX" || -L "$PREFIX" ]] || { echo "Expected symlink prefix: $PREFIX" >&2; exit 2; }
generation=$(mktemp -d "${PREFIX}.generation.XXXXXXXX")
work=$(mktemp -d "${PREFIX}.link.XXXXXXXX")
installed=0
cleanup() { rm -rf "$work"; if [[ $installed == 0 && ! "$PREFIX" -ef "$generation" ]]; then rm -rf "$generation"; fi; }
trap cleanup EXIT
mkdir -p "$generation/include" "$generation/lib"
cp "$ZLIB_INCLUDE/zlib.h" "$ZLIB_INCLUDE/zconf.h" "$generation/include/"
cp -a "$BROTLI_INCLUDE/brotli" "$generation/include/"
cp "$ZSTD_INCLUDE/zstd.h" "$generation/include/"
cp "$ZLIB_LIBDIR/libz.a" "$BROTLI_LIBDIR/libbrotlidec.a" "$BROTLI_LIBDIR/libbrotlicommon.a" "$ZSTD_LIBDIR/libzstd.a" "$generation/lib/"
printf 'agentos-immutable-prefix-v1\n' > "$generation/.agentos-generation"
ln -s "$(basename "$generation")" "$work/current"
mv -Tf "$work/current" "$PREFIX"
installed=1
