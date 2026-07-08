#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build-infozip-upstream.sh \
  --tool <zip|unzip> \
  --version <version> \
  --url <release-url> \
  --cache-dir <cache-dir> \
  --build-dir <build-dir> \
  --cc <cc> \
  --output <output>
EOF
}

TOOL=""
VERSION=""
URL=""
CACHE_DIR=""
BUILD_DIR=""
CC_CMD=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tool)
      TOOL="$2"
      shift 2
      ;;
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
    --cc)
      CC_CMD="$2"
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

if [[ -z "$TOOL" || -z "$VERSION" || -z "$URL" || -z "$CACHE_DIR" || -z "$BUILD_DIR" || -z "$CC_CMD" || -z "$OUTPUT" ]]; then
  usage >&2
  exit 1
fi

case "$TOOL" in
  zip|unzip) ;;
  *)
    echo "Unsupported Info-ZIP tool: $TOOL" >&2
    usage >&2
    exit 1
    ;;
esac

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

TARBALL="$CACHE_DIR/infozip-${TOOL}-${VERSION}.tar.gz"
if [[ ! -f "$TARBALL" ]]; then
  echo "Fetching upstream Info-ZIP ${TOOL} ${VERSION} release tarball..."
  fetch "$URL" "$TARBALL"
fi

echo "Extracting upstream Info-ZIP ${TOOL} ${VERSION}..."
tar -xzf "$TARBALL" -C "$BUILD_DIR"

SRC_DIR="$BUILD_DIR/${TOOL}${VERSION}"
if [[ ! -d "$SRC_DIR" ]]; then
  echo "Expected extracted source at $SRC_DIR" >&2
  exit 1
fi

pushd "$SRC_DIR" >/dev/null

case "$TOOL" in
  zip)
    echo "Building upstream Info-ZIP Zip ${VERSION} for wasm32-wasip1..."
    make -f unix/Makefile clean
    make -f unix/Makefile zips \
      CC="$CC_CMD" \
      BIND="$CC_CMD" \
      CFLAGS="-O2 -I. -DUNIX -DNO_ASM -DNO_BZIP2 -DNO_LCHMOD -DNO_LCHOWN -DLARGE_FILE_SUPPORT" \
      LFLAGS2=""
    BIN="zip"
    ;;
  unzip)
    echo "Building upstream Info-ZIP UnZip ${VERSION} for wasm32-wasip1..."
    make -f unix/Makefile clean
    make -f unix/Makefile unzips \
      CC="$CC_CMD" \
      LD="$CC_CMD" \
      CF="-O2 -I. -DUNIX -DBSD4_4 -DNO_PARAM_H -DNO_LCHMOD -DNO_LCHOWN -DNO_SYMLINK -DLARGE_FILE_SUPPORT" \
      LF2="" \
      SL2="" \
      FL2=""
    BIN="unzip"
    ;;
esac

if [[ ! -f "$BIN" ]]; then
  echo "Unable to locate built Info-ZIP $TOOL binary" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
if command -v wasm-opt >/dev/null 2>&1; then
  echo "Optimizing Info-ZIP $TOOL WASM binary..."
  wasm-opt -O3 --strip-debug --all-features "$BIN" -o "$OUTPUT"
else
  cp "$BIN" "$OUTPUT"
fi

popd >/dev/null

echo "Built upstream Info-ZIP $TOOL at $OUTPUT"
