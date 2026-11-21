#!/usr/bin/env bash
# Keep native compilation and bindgen on the same release libc headers.
set -euo pipefail
dest="${1:?usage: prepare-linux-x64.sh <new-directory> <environment-file>}"
env_file="${2:?environment-file is required}"
script_dir="$(cd "$(dirname "$0")" && pwd)"
test ! -e "$dest"
mkdir -p "$dest"
dest="$(cd "$dest" && pwd)"
url=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["url"])' "$script_dir/linux-x64-sysroot.json")
curl --fail --location --silent --show-error --proto '=https' "$url" -o "$dest/sysroot.tar.xz"
python3 - "$script_dir/linux-x64-sysroot.json" "$dest/sysroot.tar.xz" <<'PY'
import hashlib, json, sys
from pathlib import Path
expected = json.loads(Path(sys.argv[1]).read_text())["sha256"]
actual = hashlib.sha256(Path(sys.argv[2]).read_bytes()).hexdigest()
if actual != expected:
    raise SystemExit("release sysroot SHA-256 mismatch")
PY
# Device nodes from the distribution image are unrelated to compiler inputs
# and cannot be materialized by an unprivileged build runner.
tar --exclude=sysroot/dev -xJf "$dest/sysroot.tar.xz" -C "$dest"
test -f "$dest/sysroot/usr/include/stdlib.h"
python3 - "$dest/sysroot" "$env_file" <<'PY'
import shlex, sys
from pathlib import Path
root, output = sys.argv[1:]
# GN's target_sysroot is not used for GNU x64 in this pinned upstream tree.
# V8-as-a-library does not use GLib; do not mix runner GLib headers into it.
values = {
    "AGENTOS_V8_RELEASE_SYSROOT": root,
    "GN_ARGS": f'v8_generate_external_defines_header=true use_glib=false sysroot="{root}"',
    "BINDGEN_EXTRA_CLANG_ARGS": f"--sysroot={root} -isystem{root}/usr/include/x86_64-linux-gnu",
}
Path(output).write_text("".join(f"export {key}={shlex.quote(value)}\n" for key, value in values.items()))
PY
