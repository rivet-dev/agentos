#!/usr/bin/env bash
set -euo pipefail

MANIFEST=""
CACHE_DIR=""
BUILD_DIR=""
PATCH_DIR=""
SYSROOT=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--manifest) MANIFEST="$2"; shift 2 ;;
		--cache-dir) CACHE_DIR="$2"; shift 2 ;;
		--build-dir) BUILD_DIR="$2"; shift 2 ;;
		--patch-dir) PATCH_DIR="$2"; shift 2 ;;
		--sysroot) SYSROOT="$2"; shift 2 ;;
		--cc) CC_CMD="$2"; shift 2 ;;
		--ar) AR_CMD="$2"; shift 2 ;;
		--ranlib) RANLIB_CMD="$2"; shift 2 ;;
		--output-dir) OUTPUT_DIR="$2"; shift 2 ;;
		*) echo "Unknown argument: $1" >&2; exit 1 ;;
	esac
done

for required in MANIFEST CACHE_DIR BUILD_DIR PATCH_DIR SYSROOT CC_CMD AR_CMD RANLIB_CMD OUTPUT_DIR; do
	if [[ -z "${!required}" ]]; then
		echo "Missing required argument: $required" >&2
		exit 1
	fi
done

json_string() {
	python3 -c 'import json,sys; value=json.load(open(sys.argv[1]));
for key in sys.argv[2].split("."): value=value[key]
print(value)' "$MANIFEST" "$1"
}

mapfile -t CONFIGURE_FLAGS < <(python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["configure_flags"]))' "$MANIFEST")
NODE_TAG="$(json_string node.tag)"
NODE_URL="$(json_string node.archive_url)"
NODE_SHA256="$(json_string node.archive_sha256)"
NODE_COMMIT="$(json_string node.commit)"
OPENSSL_VERSION="$(json_string openssl.version)"
OPENSSL_TREE="$(json_string openssl.node_tree)"
CONFIGURE_TARGET="$(json_string configure_target)"

fetch() {
	local url="$1" output="$2"
	if command -v curl >/dev/null 2>&1; then
		curl --retry 3 --retry-all-errors -fSL "$url" -o "$output"
	else
		wget -q "$url" -O "$output"
	fi
}

mkdir -p "$CACHE_DIR"
archive="$CACHE_DIR/node-${NODE_TAG}.tar.xz"
[[ -f "$archive" ]] || fetch "$NODE_URL" "$archive"
printf '%s  %s\n' "$NODE_SHA256" "$archive" | sha256sum -c -

rm -rf "$BUILD_DIR" "$OUTPUT_DIR"
mkdir -p "$BUILD_DIR/source" "$OUTPUT_DIR/include/openssl" "$OUTPUT_DIR/lib"
tar -xJf "$archive" -C "$BUILD_DIR/source" --strip-components=4 \
	"node-${NODE_TAG}/deps/openssl/openssl"
source_dir="$BUILD_DIR/source"

version_file="$source_dir/VERSION.dat"
grep -qx 'MAJOR=3' "$version_file"
grep -qx 'MINOR=5' "$version_file"
grep -qx 'PATCH=5' "$version_file"

for patch_file in "$PATCH_DIR"/*.patch; do
	[[ -e "$patch_file" ]] || continue
	patch -d "$source_dir" -p1 < "$patch_file"
done

patch_set_sha256="$({ find "$PATCH_DIR" -maxdepth 1 -type f -name '*.patch' -print0 | sort -z | xargs -0 -r sha256sum; } | sha256sum | cut -d' ' -f1)"
sysroot_sha256="$(sha256sum "$SYSROOT/lib/wasm32-wasi/libc.a" | cut -d' ' -f1)"
compiler_identity="$($CC_CMD --version | head -1)"

# OpenSSL's provider model remains intact, but dynamic loading is disabled:
# default/base/legacy providers are compiled into libcrypto and activated by
# the Node-facing adapter without dlopen(3). OpenSSL INSTALL.md documents the
# no-module/no-dso configuration used for static targets.
# https://github.com/openssl/openssl/blob/openssl-3.5.5/INSTALL.md
pushd "$source_dir" >/dev/null
make distclean >/dev/null 2>&1 || true
CC="$CC_CMD --target=wasm32-wasip1 --sysroot=$SYSROOT" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
NM="${AR_CMD%/*}/llvm-nm" \
CFLAGS="-O2 -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS -DUSE_TIMEGM -DOPENSSL_NO_SECURE_MEMORY -DOPENSSL_NO_DGRAM" \
LDFLAGS="-lwasi-emulated-mman -lwasi-emulated-process-clocks" \
./Configure "$CONFIGURE_TARGET" "${CONFIGURE_FLAGS[@]}"
make build_generated
make -j"${MAKE_JOBS:-2}" build_libs
"$RANLIB_CMD" libcrypto.a
"$RANLIB_CMD" libssl.a
cp -R include/openssl/. "$OUTPUT_DIR/include/openssl/"
cp libcrypto.a "$OUTPUT_DIR/lib/libcrypto.a"
cp libssl.a "$OUTPUT_DIR/lib/libssl.a"
popd >/dev/null

crypto_sha256="$(sha256sum "$OUTPUT_DIR/lib/libcrypto.a" | cut -d' ' -f1)"
ssl_sha256="$(sha256sum "$OUTPUT_DIR/lib/libssl.a" | cut -d' ' -f1)"
configure_flags_json="$(printf '%s\n' "${CONFIGURE_FLAGS[@]}" | python3 -c 'import json,sys; print(json.dumps([line.rstrip("\n") for line in sys.stdin]))')"
python3 - "$OUTPUT_DIR/manifest.json" <<PY
import json, sys
output = {
    "schema": 1,
    "node": {"tag": "$NODE_TAG", "commit": "$NODE_COMMIT", "archive_sha256": "$NODE_SHA256"},
    "openssl": {"version": "$OPENSSL_VERSION", "node_tree": "$OPENSSL_TREE"},
    "target": "wasm32-wasip1",
    "configure_target": "$CONFIGURE_TARGET",
    "configure_flags": $configure_flags_json,
    "link_libraries": ["wasi-emulated-mman", "wasi-emulated-process-clocks"],
    "patch_set_sha256": "$patch_set_sha256",
    "sysroot_libc_sha256": "$sysroot_sha256",
    "compiler": "$compiler_identity",
    "archives": {"libcrypto.a": "$crypto_sha256", "libssl.a": "$ssl_sha256"},
}
with open(sys.argv[1], "w") as file:
    json.dump(output, file, indent=2)
    file.write("\n")
PY

echo "Built Node-bundled OpenSSL $OPENSSL_VERSION for wasm32-wasip1"
echo "  libcrypto.a $crypto_sha256"
echo "  libssl.a    $ssl_sha256"
