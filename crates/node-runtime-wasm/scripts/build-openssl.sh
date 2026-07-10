#!/usr/bin/env bash
set -euo pipefail

SOURCE=""
BUILD_DIR=""
OUTPUT_DIR=""
SYSROOT=""
WASI_SDK=""
MANIFEST=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--source) SOURCE="$2"; shift 2 ;;
		--build-dir) BUILD_DIR="$2"; shift 2 ;;
		--output-dir) OUTPUT_DIR="$2"; shift 2 ;;
		--sysroot) SYSROOT="$2"; shift 2 ;;
		--wasi-sdk) WASI_SDK="$2"; shift 2 ;;
		--manifest) MANIFEST="$2"; shift 2 ;;
		*) echo "unknown argument: $1" >&2; exit 2 ;;
	esac
done

for required in SOURCE BUILD_DIR OUTPUT_DIR SYSROOT WASI_SDK MANIFEST; do
	if [[ -z "${!required}" ]]; then
		echo "missing required argument: $required" >&2
		exit 2
	fi
done

SOURCE="$(realpath -m "$SOURCE")"
BUILD_DIR="$(realpath -m "$BUILD_DIR")"
OUTPUT_DIR="$(realpath -m "$OUTPUT_DIR")"
SYSROOT="$(realpath -m "$SYSROOT")"
WASI_SDK="$(realpath -m "$WASI_SDK")"
MANIFEST="$(realpath -m "$MANIFEST")"

json_string() {
	python3 -c 'import json,sys; value=json.load(open(sys.argv[1]));
for key in sys.argv[2].split("."): value=value[key]
print(value)' "$MANIFEST" "$1"
}

mapfile -t CONFIGURE_FLAGS < <(
	python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["configureFlags"]))' "$MANIFEST"
)

OPENSSL_VERSION="$(json_string openssl.version)"
OPENSSL_TREE="$(json_string openssl.nodeTree)"
NODE_COMMIT="$(json_string node.commit)"
SOURCE_DATE_EPOCH="$(json_string node.sourceDateEpoch)"
CONFIGURE_TARGET="$(json_string configureTarget)"
CLANG_TARGET="$(json_string clangTarget)"
SYSROOT_SUBDIR="$(json_string sysrootSubdir)"

grep -qx 'MAJOR=3' "$SOURCE/VERSION.dat"
grep -qx 'MINOR=5' "$SOURCE/VERSION.dat"
grep -qx 'PATCH=5' "$SOURCE/VERSION.dat"

rm -rf "$BUILD_DIR" "$OUTPUT_DIR"
mkdir -p "$BUILD_DIR" "$OUTPUT_DIR/include" "$OUTPUT_DIR/lib"
cp -a "$SOURCE/." "$BUILD_DIR/"

export LC_ALL=C
export TZ=UTC
export SOURCE_DATE_EPOCH
export ZERO_AR_DATE=1
CC="$WASI_SDK/bin/clang --target=$CLANG_TARGET --sysroot=$SYSROOT"
AR="$WASI_SDK/bin/llvm-ar"
RANLIB="$WASI_SDK/bin/llvm-ranlib"
NM="$WASI_SDK/bin/llvm-nm"

pushd "$BUILD_DIR" >/dev/null
CC="$CC" \
AR="$AR" \
RANLIB="$RANLIB" \
NM="$NM" \
CFLAGS="-O2 -matomics -mbulk-memory -pthread -fwasm-exceptions -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS -DUSE_TIMEGM -DOPENSSL_NO_SECURE_MEMORY -DOPENSSL_NO_DGRAM -ffile-prefix-map=$BUILD_DIR=." \
LDFLAGS="-pthread -lwasi-emulated-mman -lwasi-emulated-process-clocks" \
./Configure "$CONFIGURE_TARGET" "${CONFIGURE_FLAGS[@]}"
make build_generated
make -j"${MAKE_JOBS:-2}" build_libs
"$RANLIB" libcrypto.a
"$RANLIB" libssl.a
cp -a include/openssl "$OUTPUT_DIR/include/"
cp libcrypto.a "$OUTPUT_DIR/lib/libcrypto.a"
cp libssl.a "$OUTPUT_DIR/lib/libssl.a"
popd >/dev/null

sha() { sha256sum "$1" | cut -d' ' -f1; }
headers_sha="$({ find "$SYSROOT/include/$SYSROOT_SUBDIR" -type f -print0 | sort -z | xargs -0 sha256sum; } | sha256sum | cut -d' ' -f1)"
crypto_sha="$(sha "$OUTPUT_DIR/lib/libcrypto.a")"
ssl_sha="$(sha "$OUTPUT_DIR/lib/libssl.a")"
clang_sha="$(sha "$WASI_SDK/bin/clang")"
ar_sha="$(sha "$WASI_SDK/bin/llvm-ar")"
ranlib_sha="$(sha "$WASI_SDK/bin/llvm-ranlib")"
flags_json="$(printf '%s\n' "${CONFIGURE_FLAGS[@]}" | python3 -c 'import json,sys; print(json.dumps([line.rstrip("\n") for line in sys.stdin]))')"

python3 - "$OUTPUT_DIR/manifest.json" <<PY
import json, sys
result = {
    "schema": 1,
    "node": {"commit": "$NODE_COMMIT", "sourceDateEpoch": $SOURCE_DATE_EPOCH},
    "openssl": {"version": "$OPENSSL_VERSION", "nodeTree": "$OPENSSL_TREE"},
    "target": "wasm32-wasip1",
    "configureTarget": "$CONFIGURE_TARGET",
    "configureFlags": $flags_json,
    "sysroot": {
        "libcSha256": "$(sha "$SYSROOT/lib/$SYSROOT_SUBDIR/libc.a")",
        "headersSha256": "$headers_sha"
    },
    "clangTarget": "$CLANG_TARGET",
    "tools": {
        "clangSha256": "$clang_sha",
        "llvmArSha256": "$ar_sha",
        "llvmRanlibSha256": "$ranlib_sha"
    },
    "archives": {"libcrypto.a": "$crypto_sha", "libssl.a": "$ssl_sha"}
}
with open(sys.argv[1], "w") as file:
    json.dump(result, file, indent=2)
    file.write("\n")
PY

echo "built Node OpenSSL $OPENSSL_VERSION with AgentOS pthread support"
echo "libcrypto.a $crypto_sha"
echo "libssl.a    $ssl_sha"
