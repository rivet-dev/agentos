#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-openssl-upstream.sh \
  --version <openssl-version> \
  --url <release-url> \
  --sha256 <release-sha256> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --prefix <install-prefix> \
  --overlay-include-dir <dir> \
  --cc <cc> \
  --ar <ar> \
  --ranlib <ranlib>
EOF
}

VERSION=""
URL=""
SHA256=""
CACHE_DIR=""
BUILD_DIR=""
PREFIX=""
OVERLAY_INCLUDE_DIR=""
CC_CMD=""
AR_CMD=""
RANLIB_CMD=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --url) URL="$2"; shift 2 ;;
    --sha256) SHA256="$2"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    --overlay-include-dir) OVERLAY_INCLUDE_DIR="$2"; shift 2 ;;
    --cc) CC_CMD="$2"; shift 2 ;;
    --ar) AR_CMD="$2"; shift 2 ;;
    --ranlib) RANLIB_CMD="$2"; shift 2 ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" || -z "$URL" || -z "$SHA256" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$PREFIX" || -z "$OVERLAY_INCLUDE_DIR" || -z "$CC_CMD" || -z "$AR_CMD" || -z "$RANLIB_CMD" ]]; then
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

verify_tarball() {
  local tarball="$1"
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tarball" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tarball" | awk '{print $1}')"
  else
    echo "Neither sha256sum nor shasum is available to verify $tarball" >&2
    return 1
  fi
  if [[ "$actual" != "$SHA256" ]]; then
    echo "OpenSSL ${VERSION} checksum mismatch: expected $SHA256, got $actual" >&2
    return 1
  fi
}

mkdir -p "$CACHE_DIR" "$(dirname "$BUILD_DIR")" "$(dirname "$PREFIX")"
[[ "$PREFIX" = /* ]] || { echo 'OpenSSL prefix must be absolute' >&2; exit 2; }
exec 9>"$PREFIX.lock"
flock 9
source "$(dirname "${BASH_SOURCE[0]}")/check-generation-limit.sh"
[[ ! -e "$PREFIX" || -L "$PREFIX" ]] || {
  echo "ImmutablePrefixRequired: use a new OpenSSL prefix instead of replacing legacy directory $PREFIX" >&2
  exit 2
}

TARBALL="$CACHE_DIR/openssl-${VERSION}.tar.gz"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream OpenSSL ${VERSION} release tarball..."
  TARBALL_TMP="${TARBALL}.tmp.$$"
  trap 'rm -f "${TARBALL_TMP:-}"' EXIT
  fetch "$URL" "$TARBALL_TMP"
  verify_tarball "$TARBALL_TMP"
  mv "$TARBALL_TMP" "$TARBALL"
fi
verify_tarball "$TARBALL"

# A complete install is published as one symlink to an immutable generation.
# Keep old generations available to compilers that already resolved their inputs.
BUILD_TMP="${BUILD_DIR}.tmp.$$"
STAGE_TMP="${PREFIX}.stage.$$"
BUILD_OLD="${BUILD_DIR}.old.$$"
BUILD_HAD_OLD=0
BUILD_INSTALLED=0
PROMOTION_STARTED=0
generation=
cleanup() {
  local published=0
  if [[ -n "$generation" && "$PREFIX" -ef "$generation" ]]; then published=1; fi
  rm -rf "$BUILD_TMP" "$STAGE_TMP"
  rm -f "${TARBALL_TMP:-}"
  if [[ "$PROMOTION_STARTED" = 1 && "$published" = 0 ]]; then
    if [[ "$BUILD_INSTALLED" = 1 ]]; then rm -rf "$BUILD_DIR"; fi
    if [[ "$BUILD_HAD_OLD" = 1 && -e "$BUILD_OLD" ]]; then
      rm -rf "$BUILD_DIR"
      mv "$BUILD_OLD" "$BUILD_DIR"
    fi
  fi
  if [[ "$published" = 1 ]]; then
    rm -rf "$BUILD_OLD"
  elif [[ -n "$generation" ]]; then
    rm -rf "$generation"
  fi
}
trap cleanup EXIT
rm -rf "$BUILD_TMP" "$STAGE_TMP" "$BUILD_OLD"
mkdir -p "$BUILD_TMP" "$STAGE_TMP"

echo "Extracting upstream OpenSSL ${VERSION}..."
tar -xzf "$TARBALL" -C "$BUILD_TMP" --strip-components=1

pushd "$BUILD_TMP" >/dev/null

# OpenSSH uses libcrypto; curl and Git share this generation's libssl/libcrypto.
# Omit programs, modules, dynamic loading, threads, and platform assembly.
# BSD-generic32 matches wasm32's data model; getrandom comes from the owned libc.
echo "Configuring upstream OpenSSL ${VERSION} for wasm32-wasip1..."
CC="$CC_CMD" \
AR="$AR_CMD" \
RANLIB="$RANLIB_CMD" \
./Configure BSD-generic32 \
  --prefix=/usr \
  --openssldir=/etc/ssl \
  --libdir=lib \
  --with-rand-seed=getrandom \
  -D_WASI_EMULATED_PROCESS_CLOCKS \
  -I"$OVERLAY_INCLUDE_DIR" \
  -Wl,-lwasi-emulated-process-clocks \
  no-asm \
  no-threads \
  no-shared \
  no-dso \
  no-module \
  no-tests \
  no-apps \
  no-ui-console \
  no-secure-memory \
  no-pinshared \
  no-afalgeng \
  no-engine \
  no-devcryptoeng \
  no-padlockeng

echo "Building upstream OpenSSL libcrypto..."
make -j"${MAKE_JOBS:-2}" build_libs
make DESTDIR="$STAGE_TMP" install_dev

popd >/dev/null

if [[ ! -f "$STAGE_TMP/usr/lib/libssl.a" || ! -f "$STAGE_TMP/usr/lib/libcrypto.a" || ! -f "$STAGE_TMP/usr/include/openssl/opensslv.h" ]]; then
  echo "Expected OpenSSL libssl, libcrypto and development headers were not installed" >&2
  exit 1
fi

generation=$(mktemp -d "${PREFIX}.generation.XXXXXXXX")
cp -a "$STAGE_TMP/usr/." "$generation/"
# OpenSSL keeps /usr as its guest build prefix. Only host consumption metadata
# is relocated, so pkg-config cannot introduce host /usr headers or libraries.
python3 - "$generation" "$STAGE_TMP/usr" <<'PYMETA'
import pathlib,re,sys
root,staged=map(pathlib.Path,sys.argv[1:])
pattern=re.compile(re.escape(str(staged))+r"(?=/|$)|/usr(?=/|[\s'\"$]|$)")
for extension in ('*.pc','*.la'):
 for path in root.rglob(extension):
  path.write_text(pattern.sub(lambda _:str(root),path.read_text()))
PYMETA
printf 'agentos-immutable-prefix-v1\n' > "$generation/.agentos-generation"

PROMOTION_STARTED=1
if [[ -e "$BUILD_DIR" ]]; then
  mv "$BUILD_DIR" "$BUILD_OLD"
  BUILD_HAD_OLD=1
fi
mv "$BUILD_TMP" "$BUILD_DIR"
BUILD_INSTALLED=1
ln -s "$(basename "$generation")" "$STAGE_TMP/current"
mv -Tf "$STAGE_TMP/current" "$PREFIX"
# Cleanup detects publication by identity, including interruption before here.
echo "Built upstream OpenSSL libssl/libcrypto at $generation/lib"
