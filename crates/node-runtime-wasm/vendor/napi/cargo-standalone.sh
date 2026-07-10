#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$repo_root"

vendored_manifest="$repo_root/Cargo.toml"
standalone_manifest="$repo_root/Cargo.standalone.toml"

need_standalone=0
for dep in api cache compiler-llvm types wasix virtual-fs; do
  if [[ ! -f "$repo_root/../$dep/Cargo.toml" ]]; then
    need_standalone=1
    break
  fi
done

use_standalone=0
if [[ "${NAPI_WASMER_FORCE_STANDALONE:-0}" == "1" || $need_standalone -eq 1 ]]; then
  use_standalone=1
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 [+toolchain] <cargo-subcommand> [args...]" >&2
  exit 1
fi

cargo_cmd=(cargo)
if [[ "${1:-}" == +* ]]; then
  cargo_cmd+=("$1")
  shift
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 [+toolchain] <cargo-subcommand> [args...]" >&2
  exit 1
fi

subcommand="$1"
shift

if [[ "$subcommand" == -* ]]; then
  echo "usage: $0 [+toolchain] <cargo-subcommand> [args...]" >&2
  exit 1
fi

if [[ $use_standalone -eq 0 ]]; then
  exec "${cargo_cmd[@]}" "$subcommand" --manifest-path "$vendored_manifest" "$@"
fi

standalone_parent="$(dirname "$repo_root")"
if [[ -w "$standalone_parent" ]]; then
  standalone_work_dir="$(mktemp -d "$standalone_parent/.napi-cargo-standalone.XXXXXX")"
else
  standalone_work_dir="$(mktemp -d "${TMPDIR:-/tmp}/napi-cargo-standalone.XXXXXX")"
fi

cleanup_standalone_work_dir() {
  rm -rf "$standalone_work_dir"
}

trap cleanup_standalone_work_dir EXIT

cp "$standalone_manifest" "$standalone_work_dir/Cargo.toml"
if [[ -f "$repo_root/Cargo.lock" ]]; then
  cp "$repo_root/Cargo.lock" "$standalone_work_dir/Cargo.lock"
fi

for entry in build.rs include lib src tests v8; do
  ln -s "$repo_root/$entry" "$standalone_work_dir/$entry"
done

"${cargo_cmd[@]}" "$subcommand" --manifest-path "$standalone_work_dir/Cargo.toml" "$@"
