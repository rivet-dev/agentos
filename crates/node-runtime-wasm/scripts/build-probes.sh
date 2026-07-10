#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
crate_dir=$(cd -- "${script_dir}/.." && pwd)
source_file="${crate_dir}/probes/nested-bootstrap.c"
artifact_file="${crate_dir}/artifacts/nested-bootstrap.wasm"
manifest_file="${crate_dir}/probes/manifest.json"
mode=${1:-build}

if [[ "${mode}" != "build" && "${mode}" != "--check" ]]; then
  echo "usage: $0 [build|--check]" >&2
  exit 2
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf "${tmp_dir}"' EXIT
tmp_artifact="${tmp_dir}/nested-bootstrap.wasm"

export LC_ALL=C
export TZ=UTC
export SOURCE_DATE_EPOCH=0
rust_sysroot=$(rustc --print sysroot)
export PATH="${rust_sysroot}/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/gcc-ld:${PATH}"

clang \
  --target=wasm32-unknown-unknown \
  -Oz \
  -fno-ident \
  -fvisibility=hidden \
  -nostdlib \
  -Wl,--no-entry \
  -Wl,--strip-all \
  -Wl,--initial-memory=131072 \
  -Wl,--max-memory=262144 \
  -o "${tmp_artifact}" \
  "${source_file}"

if [[ "${mode}" == "--check" ]]; then
  if ! cmp --silent "${tmp_artifact}" "${artifact_file}"; then
    echo "nested-bootstrap.wasm is stale; run $0 build" >&2
    exit 1
  fi

  expected_source_sha=$(node -e 'const m=require(process.argv[1]); process.stdout.write(m.source.sha256)' "${manifest_file}")
  expected_artifact_sha=$(node -e 'const m=require(process.argv[1]); process.stdout.write(m.artifact.sha256)' "${manifest_file}")
  actual_source_sha=$(sha256sum "${source_file}" | cut -d' ' -f1)
  actual_artifact_sha=$(sha256sum "${tmp_artifact}" | cut -d' ' -f1)
  if [[ "${actual_source_sha}" != "${expected_source_sha}" ]]; then
    echo "nested-bootstrap.c hash does not match probes/manifest.json" >&2
    exit 1
  fi
  if [[ "${actual_artifact_sha}" != "${expected_artifact_sha}" ]]; then
    echo "nested-bootstrap.wasm hash does not match probes/manifest.json" >&2
    exit 1
  fi
else
  mkdir -p "$(dirname -- "${artifact_file}")"
  cp "${tmp_artifact}" "${artifact_file}"
fi

sha256sum "${tmp_artifact}"
