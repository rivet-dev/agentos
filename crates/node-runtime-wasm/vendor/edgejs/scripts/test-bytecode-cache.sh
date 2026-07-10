#!/usr/bin/env bash
# End-to-end tests for bytecode sidecar caches (.v8b / .qjsb).
# Usage: EDGE_BIN=./build-edge/edge ./scripts/test-bytecode-cache.sh
set -u

EDGE_BIN="${EDGE_BIN:?usage: EDGE_BIN=<path-to-edge> $0}"
EDGE_BIN="$(cd "$(dirname "$EDGE_BIN")" && pwd)/$(basename "$EDGE_BIN")"

if [ ! -x "$EDGE_BIN" ]; then
  echo "error: EDGE_BIN is not executable: $EDGE_BIN" >&2
  exit 1
fi

# Per-file (user) sidecars are OFF by default; this suite exercises their
# mechanics, so opt in globally. Kill-switch scenarios (--no-bytecode-cache,
# EDGE_BYTECODE_CACHE=0, --check) override per-run, and the default-policy
# scenario clears it with `env -u` to assert the shipped default.
export EDGE_BYTECODE_CACHE=1

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/edge-bytecode-cache-test.XXXXXX")"
# Resolve symlinked temp roots (macOS /var -> /private/var) so trace-output
# greps match the canonical paths the runtime reports.
WORKDIR="$(cd "$WORKDIR" && pwd -P)"
trap 'chmod -R u+w "$WORKDIR" 2>/dev/null; rm -rf "$WORKDIR"' EXIT

FAILURES=0
CURRENT=""
CURRENT_FAILURES=0

begin() {
  CURRENT="$1"
  CURRENT_FAILURES=$FAILURES
}

fail() {
  echo "FAIL: $CURRENT: $1" >&2
  FAILURES=$((FAILURES + 1))
}

pass() {
  if [ "$FAILURES" -eq "$CURRENT_FAILURES" ]; then
    echo "ok: $CURRENT"
  fi
}

# User-file caches live in a per-directory subdir, PEP 3147 __pycache__ style:
#   <dir>/app.js -> <dir>/__edgecache__/app.js.<engine-tag>.jsc
# The full source filename (extension kept) keys the entry; the engine/version
# tag is not known statically, so locate caches by glob and detect the engine
# from the tag the precompile probe writes.
sidecar_glob_dir() { dirname "$1"; }
sidecar_name() { basename "$1"; }

sidecar_path() {  # $1 = source file -> first matching cache file, or empty
  local dir name matches
  dir="$(sidecar_glob_dir "$1")"; name="$(sidecar_name "$1")"
  matches=("$dir"/__edgecache__/"$name".*.jsc)
  [ -e "${matches[0]}" ] && printf '%s\n' "${matches[0]}"
}
sidecar_exists() { [ -n "$(sidecar_path "$1")" ]; }
sidecar_event() {  # $1 = event (hit/write/remove)  $2 = source file  $3 = trace file
  grep -F "__edgecache__/$(sidecar_name "$2")." "$3" | grep -q "] $1 "
}

detect_engine() {
  local dir="$WORKDIR/engine-probe" f
  mkdir -p "$dir"
  echo "module.exports = 1;" > "$dir/probe.js"
  "$EDGE_BIN" --precompile "$dir" >/dev/null 2>&1
  f="$(basename "$(sidecar_path "$dir/probe.js")" 2>/dev/null)"
  case "$f" in
    probe.js.v8-*) echo "v8" ;;
    probe.js.qjs-*) echo "qjs" ;;
    *) echo "" ;;
  esac
}

ENGINE="$(detect_engine)"
if [ -z "$ENGINE" ]; then
  echo "error: could not determine engine (precompile probe wrote no __edgecache__/*.jsc)" >&2
  exit 1
fi
# The consolidated builtins cache keeps its engine suffix (one file by the binary).
if [ "$ENGINE" = "v8" ]; then SUFFIX=".v8b"; else SUFFIX=".qjsb"; fi
echo "testing $EDGE_BIN (engine: $ENGINE, builtins suffix: $SUFFIX)"

make_project() {
  local dir="$1"
  mkdir -p "$dir"
  cat > "$dir/lib.js" <<'EOF'
module.exports = { value: 21 };
EOF
  cat > "$dir/main.js" <<'EOF'
const lib = require('./lib.js');
console.log(lib.value * 2);
EOF
}

# --- precompile writes sidecars and the cached run output is identical -------
begin "precompile then run"
DIR="$WORKDIR/precompile"
make_project "$DIR"
baseline="$("$EDGE_BIN" --no-bytecode-cache "$DIR/main.js" 2>/dev/null)"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1 || fail "precompile exited non-zero"
sidecar_exists "$DIR/main.js" || fail "missing main.js cache"
sidecar_exists "$DIR/lib.js" || fail "missing lib.js cache"
cached="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/main.js" 2>"$WORKDIR/trace.txt")"
[ "$cached" = "$baseline" ] || fail "output mismatch: '$cached' vs '$baseline'"
sidecar_event hit "$DIR/main.js" "$WORKDIR/trace.txt" || fail "expected a cache hit for main.js"
pass

# --- write-on-first-run -------------------------------------------------------
begin "write-on-first-run then consume"
DIR="$WORKDIR/first-run"
make_project "$DIR"
out1="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/main.js" 2>"$WORKDIR/trace1.txt")"
[ "$out1" = "42" ] || fail "first run output: $out1"
sidecar_event write "$DIR/main.js" "$WORKDIR/trace1.txt" || fail "first run did not write sidecar"
out2="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/main.js" 2>"$WORKDIR/trace2.txt")"
[ "$out2" = "42" ] || fail "second run output: $out2"
sidecar_event hit "$DIR/main.js" "$WORKDIR/trace2.txt" || fail "second run did not consume sidecar"
pass

# --- corrupted sidecar falls back and is rewritten ----------------------------
begin "corrupted sidecar falls back"
DIR="$WORKDIR/corrupt"
make_project "$DIR"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1
printf 'garbage' > "$(sidecar_path "$DIR/main.js")"
out="$("$EDGE_BIN" "$DIR/main.js" 2>/dev/null)"
[ "$out" = "42" ] || fail "corrupted sidecar broke execution: $out"
size="$(wc -c < "$(sidecar_path "$DIR/main.js")")"
[ "$size" -gt 64 ] || fail "corrupted sidecar was not rewritten (size=$size)"
pass

# --- stale sidecar (source edited) falls back ---------------------------------
begin "stale sidecar falls back"
DIR="$WORKDIR/stale"
make_project "$DIR"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1
echo "console.log('changed');" > "$DIR/main.js"
out="$("$EDGE_BIN" "$DIR/main.js" 2>/dev/null)"
[ "$out" = "changed" ] || fail "stale sidecar served old code: $out"
pass

# --- relocated tree ---------------------------------------------------------------
# QuickJS bytecode embeds the compile-time path/URL (import.meta.url, stack
# traces), so its self-validating payload header rejects relocated sidecars
# and the runtime recompiles + rewrites. V8 caches are relocatable and keep
# hitting. Either way: correct output, no stale paths.
begin "relocated tree recompiles (quickjs) or keeps hitting (v8)"
DIR="$WORKDIR/relocate-src"
mkdir -p "$DIR"
cat > "$DIR/main.mjs" <<'RELOC_EOF'
console.log(import.meta.url.endsWith('relocate-dst/main.mjs') ? 'url-ok' : 'url-stale');
RELOC_EOF
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1
sidecar_exists "$DIR/main.mjs" || fail "missing sidecar before relocation"
mv "$DIR" "$WORKDIR/relocate-dst"
out="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$WORKDIR/relocate-dst/main.mjs" 2>"$WORKDIR/reloc.txt")"
[ "$out" = "url-ok" ] || fail "relocated import.meta.url is stale: $out"
if [ "$ENGINE" = "qjs" ]; then
  sidecar_event remove "$WORKDIR/relocate-dst/main.mjs" "$WORKDIR/reloc.txt" || fail "quickjs did not drop the relocated sidecar"
  sidecar_event write "$WORKDIR/relocate-dst/main.mjs" "$WORKDIR/reloc.txt" || fail "quickjs did not rewrite the relocated sidecar"
else
  sidecar_event hit "$WORKDIR/relocate-dst/main.mjs" "$WORKDIR/reloc.txt" || fail "v8 should consume relocated sidecars"
fi
pass

# --- opt-outs ------------------------------------------------------------------
begin "--no-bytecode-cache writes and reads nothing"
DIR="$WORKDIR/optout-flag"
make_project "$DIR"
"$EDGE_BIN" --no-bytecode-cache "$DIR/main.js" >/dev/null 2>&1
! sidecar_exists "$DIR/main.js" || fail "sidecar written despite --no-bytecode-cache"
pass

begin "EDGE_BYTECODE_CACHE=0 writes and reads nothing"
DIR="$WORKDIR/optout-env"
make_project "$DIR"
EDGE_BYTECODE_CACHE=0 "$EDGE_BIN" "$DIR/main.js" >/dev/null 2>&1
! sidecar_exists "$DIR/main.js" || fail "sidecar written despite EDGE_BYTECODE_CACHE=0"
pass

# --- read-only tree ------------------------------------------------------------
begin "read-only directory still runs"
DIR="$WORKDIR/readonly"
make_project "$DIR"
chmod -R a-w "$DIR"
out="$("$EDGE_BIN" "$DIR/main.js" 2>/dev/null)"
chmod -R u+w "$DIR"
[ "$out" = "42" ] || fail "read-only tree broke execution: $out"
! sidecar_exists "$DIR/main.js" || fail "sidecar appeared in read-only tree"
pass

# --- shebang and BOM -----------------------------------------------------------
begin "shebang and BOM sources round-trip"
DIR="$WORKDIR/encodings"
mkdir -p "$DIR"
printf '#!/usr/bin/env node\nconsole.log("shebang");\n' > "$DIR/shebang.js"
printf '\xef\xbb\xbfconsole.log("bom");\n' > "$DIR/bom.js"
out_a1="$("$EDGE_BIN" "$DIR/shebang.js" 2>/dev/null)"
out_a2="$("$EDGE_BIN" "$DIR/shebang.js" 2>/dev/null)"
[ "$out_a1" = "shebang" ] && [ "$out_a2" = "shebang" ] || fail "shebang: '$out_a1' / '$out_a2'"
out_b1="$("$EDGE_BIN" "$DIR/bom.js" 2>/dev/null)"
out_b2="$("$EDGE_BIN" "$DIR/bom.js" 2>/dev/null)"
[ "$out_b1" = "bom" ] && [ "$out_b2" = "bom" ] || fail "bom: '$out_b1' / '$out_b2'"
pass

# --- package type rules ---------------------------------------------------------
begin "precompile respects package.json type=module"
DIR="$WORKDIR/typemodule"
mkdir -p "$DIR"
echo '{ "type": "module" }' > "$DIR/package.json"
echo 'export const x = 1;' > "$DIR/esm.js"
echo 'module.exports = 1;' > "$DIR/legacy.cjs"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1 || fail "precompile exited non-zero"
sidecar_exists "$DIR/esm.js" || fail ".js in type=module scope must get an ESM-shape sidecar"
sidecar_exists "$DIR/legacy.cjs" || fail ".cjs must be precompiled regardless of scope"
pass

# --- ESM sidecars -----------------------------------------------------------------
begin "esm write-on-first-run then consume"
DIR="$WORKDIR/esm-first-run"
mkdir -p "$DIR"
cat > "$DIR/lib.mjs" <<'EOF'
export const value = 21;
EOF
cat > "$DIR/main.mjs" <<'EOF'
import { value } from './lib.mjs';
console.log(value * 2);
EOF
out1="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/main.mjs" 2>"$WORKDIR/etrace1.txt")"
[ "$out1" = "42" ] || fail "first run output: $out1"
sidecar_event write "$DIR/main.mjs" "$WORKDIR/etrace1.txt" || fail "first run did not write esm sidecar"
out2="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/main.mjs" 2>"$WORKDIR/etrace2.txt")"
[ "$out2" = "42" ] || fail "second run output: $out2"
sidecar_event hit "$DIR/main.mjs" "$WORKDIR/etrace2.txt" || fail "second run did not consume esm sidecar"
sidecar_event hit "$DIR/lib.mjs" "$WORKDIR/etrace2.txt" || fail "imported module did not consume esm sidecar"
pass

begin "esm import chain fully cached"
DIR="$WORKDIR/esm-chain"
mkdir -p "$DIR"
echo 'export const c = 7;' > "$DIR/c.mjs"
printf "import { c } from './c.mjs';\nexport const b = c * 2;\n" > "$DIR/b.mjs"
printf "import { b } from './b.mjs';\nconsole.log(b * 3);\n" > "$DIR/a.mjs"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1 || fail "precompile exited non-zero"
out="$(EDGE_BYTECODE_CACHE_TRACE=1 "$EDGE_BIN" "$DIR/a.mjs" 2>"$WORKDIR/chain.txt")"
[ "$out" = "42" ] || fail "chain output: $out"
hits="$(grep -c "] hit " "$WORKDIR/chain.txt")"  # sidecar hits only, not builtin-hit
[ "$hits" -eq 3 ] || fail "expected 3 cache hits, got $hits"
pass

begin "dynamic import and import.meta from cached modules"
DIR="$WORKDIR/esm-dynamic"
mkdir -p "$DIR"
echo 'export const value = 5;' > "$DIR/lib.mjs"
cat > "$DIR/main.mjs" <<'EOF'
const mod = await import('./lib.mjs');
console.log(mod.value, import.meta.url.endsWith('main.mjs'));
EOF
"$EDGE_BIN" "$DIR/main.mjs" >/dev/null 2>&1
out="$("$EDGE_BIN" "$DIR/main.mjs" 2>/dev/null)"
[ "$out" = "5 true" ] || fail "cached dynamic import / import.meta: $out"
pass

# Regression: import.meta inside an IMPORTED cached module. The importer's
# bytecode read used to register stub modules under the dependency's real URL,
# shadowing the real module in quickjs's name-based import.meta lookup.
begin "import.meta in imported cached module"
DIR="$WORKDIR/esm-meta-dep"
mkdir -p "$DIR"
cat > "$DIR/dep.mjs" <<'EOF'
export const fromDep = import.meta.url;
EOF
cat > "$DIR/main.mjs" <<'EOF'
import { fromDep } from './dep.mjs';
console.log(typeof fromDep, String(fromDep).endsWith('dep.mjs'));
EOF
"$EDGE_BIN" "$DIR/main.mjs" >/dev/null 2>&1
out="$("$EDGE_BIN" "$DIR/main.mjs" 2>/dev/null)"
[ "$out" = "string true" ] || fail "import.meta in imported cached module: $out"
pass

begin "top-level await module cached"
DIR="$WORKDIR/esm-tla"
mkdir -p "$DIR"
cat > "$DIR/main.mjs" <<'EOF'
const v = await Promise.resolve('tla-ok');
console.log(v);
EOF
"$EDGE_BIN" "$DIR/main.mjs" >/dev/null 2>&1
out="$("$EDGE_BIN" "$DIR/main.mjs" 2>/dev/null)"
[ "$out" = "tla-ok" ] || fail "cached TLA module: $out"
pass

begin "corrupted esm sidecar falls back"
DIR="$WORKDIR/esm-corrupt"
mkdir -p "$DIR"
echo 'console.log("esm-fine");' > "$DIR/main.mjs"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1
printf 'garbage' > "$(sidecar_path "$DIR/main.mjs")"
out="$("$EDGE_BIN" "$DIR/main.mjs" 2>/dev/null)"
[ "$out" = "esm-fine" ] || fail "corrupted esm sidecar broke execution: $out"
pass

begin "stale esm sidecar falls back"
DIR="$WORKDIR/esm-stale"
mkdir -p "$DIR"
echo 'console.log("esm-old");' > "$DIR/main.mjs"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1
echo 'console.log("esm-new");' > "$DIR/main.mjs"
out="$("$EDGE_BIN" "$DIR/main.mjs" 2>/dev/null)"
[ "$out" = "esm-new" ] || fail "stale esm sidecar served old code: $out"
pass

begin "detect-module .js precompiles as ESM and runs cached"
DIR="$WORKDIR/esm-detect"
mkdir -p "$DIR"
echo 'export default 1; console.log("detected");' > "$DIR/ambiguous.js"
"$EDGE_BIN" --precompile "$DIR" >/dev/null 2>&1 || fail "precompile exited non-zero"
sidecar_exists "$DIR/ambiguous.js" || fail "ESM-syntax .js should get an ESM-shape sidecar"
out="$("$EDGE_BIN" "$DIR/ambiguous.js" 2>/dev/null)"
[ "$out" = "detected" ] || fail "detect-module run with sidecar: $out"
pass

# --- vm cachedData round-trips ----------------------------------------------------
begin "vm.Script and vm.compileFunction cachedData round-trip"
DIR="$WORKDIR/vm-cache"
mkdir -p "$DIR"
cat > "$DIR/vm-test.cjs" <<'EOF'
const vm = require('node:vm');
const script = new vm.Script('1 + 41', { produceCachedData: true });
if (!script.cachedData || script.cachedData.length === 0) throw new Error('no script cachedData');
const script2 = new vm.Script('1 + 41', { cachedData: script.cachedData });
if (script2.cachedDataRejected !== false) throw new Error('script cachedData rejected: ' + script2.cachedDataRejected);
if (script2.runInThisContext() !== 42) throw new Error('script2 result');
const fn = vm.compileFunction('return a + b;', ['a', 'b'], { produceCachedData: true, filename: 'fn-test.js' });
if (!fn.cachedData || fn.cachedData.length === 0) throw new Error('no fn cachedData');
const fn2 = vm.compileFunction('return a + b;', ['a', 'b'], { cachedData: fn.cachedData, filename: 'fn-test.js' });
if (fn2(40, 2) !== 42) throw new Error('fn2 result');
console.log('vm-ok');
EOF
out="$("$EDGE_BIN" --no-bytecode-cache "$DIR/vm-test.cjs" 2>&1)"
[ "$out" = "vm-ok" ] || fail "vm cachedData round-trip: $out"
pass

begin "vm.SourceTextModule cachedData round-trip"
DIR="$WORKDIR/vm-module"
mkdir -p "$DIR"
cat > "$DIR/vm-module-test.cjs" <<'EOF'
const vm = require('node:vm');
async function main() {
  const m1 = new vm.SourceTextModule('export const x = 42;');
  const cached = m1.createCachedData();
  if (!cached || cached.length === 0) throw new Error('no module cachedData');
  const m2 = new vm.SourceTextModule('export const x = 42;', { cachedData: cached });
  await m2.link(() => { throw new Error('no links expected'); });
  await m2.evaluate();
  if (m2.namespace.x !== 42) throw new Error('module namespace');
  console.log('vm-module-ok');
}
main().catch((err) => { console.error(err.message); process.exit(1); });
EOF
out="$("$EDGE_BIN" --no-bytecode-cache --no-warnings --experimental-vm-modules "$DIR/vm-module-test.cjs" 2>/dev/null)"
[ "$out" = "vm-module-ok" ] || fail "vm.SourceTextModule cachedData round-trip: $out"
pass

# --- vm cachedData cross-shape is never executed during compile -------------------
# A whole-script cache fed to vm.compileFunction must not run the script body
# while compiling (the QJSB shape tag / V8 compile-kind guard reject it). Both
# engines.
begin "vm cachedData cross-shape does not execute body"
DIR="$WORKDIR/vm-xshape"
mkdir -p "$DIR"
cat > "$DIR/xshape.cjs" <<'EOF'
const vm = require('node:vm');
globalThis.__hit = 0;
const s = new vm.Script("globalThis.__hit++; 7", { produceCachedData: true });
if (globalThis.__hit !== 0) throw new Error('Script ctor ran body');
// Feed the script cache to compileFunction (wrong shape). Must not execute.
const f = vm.compileFunction("globalThis.__hit++; 7", [], { cachedData: s.cachedData });
if (globalThis.__hit !== 0) throw new Error('cross-shape executed body during compile');
console.log('xshape-ok');
EOF
out="$("$EDGE_BIN" --no-bytecode-cache "$DIR/xshape.cjs" 2>&1)"
[ "$out" = "xshape-ok" ] || fail "vm cross-shape body execution: $out"
pass

# --- QuickJS self-validating payload rejects wrong source/params ------------------
# QuickJS bytecode carries no source identity natively, so the provider's QJSB
# header enforces source + params. (V8's CompileFunction code cache is not
# source-validated — matching Node — so this strict check is QuickJS-only.)
if [ "$SUFFIX" = ".qjsb" ]; then
  begin "quickjs cachedData rejects wrong source and params"
  DIR="$WORKDIR/vm-strict"
  mkdir -p "$DIR"
  cat > "$DIR/strict.cjs" <<'EOF'
const vm = require('node:vm');
// Different source.
const a = vm.compileFunction("return 1", [], { produceCachedData: true });
const b = vm.compileFunction("return 2", [], { cachedData: a.cachedData });
if (b.cachedDataRejected !== true) throw new Error('wrong-source not rejected');
if (b() !== 2) throw new Error('recompiled body wrong: ' + b());
// Different params.
const c = vm.compileFunction("return p", ["p", "q"], { produceCachedData: true });
const d = vm.compileFunction("return p", ["p"], { cachedData: c.cachedData });
if (d.cachedDataRejected !== true) throw new Error('wrong-params not rejected');
if (d(5) !== 5) throw new Error('recompiled params wrong: ' + d(5));
console.log('strict-ok');
EOF
  out="$("$EDGE_BIN" --no-bytecode-cache "$DIR/strict.cjs" 2>&1)"
  [ "$out" = "strict-ok" ] || fail "quickjs strict cachedData rejection: $out"
  pass
fi

# --- --check stays clean ---------------------------------------------------------
begin "--check writes no sidecars"
DIR="$WORKDIR/checkmode"
make_project "$DIR"
"$EDGE_BIN" --check "$DIR/main.js" >/dev/null 2>&1
! sidecar_exists "$DIR/main.js" || fail "--check wrote a sidecar"
pass

# --- conflicts -------------------------------------------------------------------
begin "--precompile conflict validation"
"$EDGE_BIN" --precompile >/dev/null 2>&1 && fail "--precompile with no paths should fail"
"$EDGE_BIN" --precompile --check x.js >/dev/null 2>&1 && fail "--precompile --check should fail"
"$EDGE_BIN" --precompile --no-bytecode-cache x >/dev/null 2>&1 && fail "--precompile --no-bytecode-cache should fail"
pass

# === Builtins consolidated bytecode cache (<binary>.builtins<suffix>) ==============
# The cache lands next to the binary, so each scenario gets its own COPY of
# the binary (copy, not symlink: the exec-path lookup resolves symlinks).
BUILTINS_FILE_NAME="edge.builtins$SUFFIX"

make_builtins_bin() {
  local dir="$1"
  mkdir -p "$dir"
  cp "$EDGE_BIN" "$dir/edge"
  echo 'console.log(1 + 1);' > "$dir/app.js"
}

# --- shipped default: builtins on, user sidecars off ------------------------------
# Clear EDGE_BYTECODE_CACHE to see the real default. The builtins cache should
# be written (on by default), but no per-file user sidecar — that needs opt-in.
begin "default policy: builtins cached, user sidecars off until opt-in"
DIR="$WORKDIR/default-policy"
make_builtins_bin "$DIR"
out="$(env -u EDGE_BYTECODE_CACHE "$DIR/edge" "$DIR/app.js" 2>/dev/null)"
[ "$out" = "2" ] || fail "default run output: $out"
[ -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "builtins cache not written by default"
! sidecar_exists "$DIR/app.js" || fail "user sidecar written without opt-in"
# Opt in: --bytecode-cache writes the per-file sidecar.
env -u EDGE_BYTECODE_CACHE "$DIR/edge" --bytecode-cache "$DIR/app.js" >/dev/null 2>&1
sidecar_exists "$DIR/app.js" || fail "--bytecode-cache did not write a user sidecar"
# EDGE_BYTECODE_CACHE=1 also opts in.
rm -rf "$DIR/__edgecache__"
EDGE_BYTECODE_CACHE=1 "$DIR/edge" "$DIR/app.js" >/dev/null 2>&1
sidecar_exists "$DIR/app.js" || fail "EDGE_BYTECODE_CACHE=1 did not write a user sidecar"
pass

# --- first run writes, second run hits --------------------------------------------
begin "builtins cache write then hit"
DIR="$WORKDIR/builtins-basic"
make_builtins_bin "$DIR"
out1="$(EDGE_BYTECODE_CACHE_TRACE=1 "$DIR/edge" "$DIR/app.js" 2>"$DIR/trace1.txt")"
[ "$out1" = "2" ] || fail "first run output: $out1"
[ -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "missing $BUILTINS_FILE_NAME after first run"
grep -q "builtins-write" "$DIR/trace1.txt" || fail "first run did not trace builtins-write"
out2="$(EDGE_BYTECODE_CACHE_TRACE=1 "$DIR/edge" "$DIR/app.js" 2>"$DIR/trace2.txt")"
[ "$out2" = "2" ] || fail "second run output: $out2"
grep -q "builtin-hit" "$DIR/trace2.txt" || fail "second run had no builtin-hit"
grep -q "builtins-write" "$DIR/trace2.txt" && fail "second run rewrote the builtins cache"
grep -q "builtin-miss" "$DIR/trace2.txt" && fail "second run had builtin-miss lines"
pass

# --- corrupted builtins file: output parity + rewrite ------------------------------
begin "corrupted builtins cache falls back and rewrites"
DIR="$WORKDIR/builtins-corrupt"
make_builtins_bin "$DIR"
"$DIR/edge" "$DIR/app.js" >/dev/null 2>&1
[ -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "missing builtins cache to corrupt"
# Truncate mid-file: structural validation fails, all entries recompile.
head -c 100 "$DIR/$BUILTINS_FILE_NAME" > "$DIR/$BUILTINS_FILE_NAME.tmp"
mv "$DIR/$BUILTINS_FILE_NAME.tmp" "$DIR/$BUILTINS_FILE_NAME"
out="$(EDGE_BYTECODE_CACHE_TRACE=1 "$DIR/edge" "$DIR/app.js" 2>"$DIR/trace.txt")"
[ "$out" = "2" ] || fail "corrupted-cache run output: $out"
grep -q "builtins-load-failed" "$DIR/trace.txt" || fail "no builtins-load-failed trace"
grep -q "builtins-write" "$DIR/trace.txt" || fail "corrupt cache was not rewritten"
out2="$(EDGE_BYTECODE_CACHE_TRACE=1 "$DIR/edge" "$DIR/app.js" 2>"$DIR/trace2.txt")"
[ "$out2" = "2" ] || fail "post-rewrite run output: $out2"
grep -q "builtin-hit" "$DIR/trace2.txt" || fail "rewritten cache produced no hits"
pass

# --- opt-outs write no builtins file -----------------------------------------------
begin "builtins cache opt-outs write nothing"
DIR="$WORKDIR/builtins-optout"
make_builtins_bin "$DIR"
"$DIR/edge" --no-bytecode-cache "$DIR/app.js" >/dev/null 2>&1
[ ! -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "--no-bytecode-cache wrote builtins cache"
EDGE_BYTECODE_CACHE=0 "$DIR/edge" "$DIR/app.js" >/dev/null 2>&1
[ ! -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "EDGE_BYTECODE_CACHE=0 wrote builtins cache"
"$DIR/edge" --check "$DIR/app.js" >/dev/null 2>&1
[ ! -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "--check wrote builtins cache"
pass

# --- read-only binary dir: silent no-op --------------------------------------------
begin "read-only binary dir runs fine without builtins cache"
DIR="$WORKDIR/builtins-readonly"
make_builtins_bin "$DIR"
chmod a-w "$DIR"
out="$("$DIR/edge" "$DIR/app.js" 2>/dev/null)"
chmod u+w "$DIR"
[ "$out" = "2" ] || fail "read-only dir run output: $out"
[ ! -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "builtins cache appeared in read-only dir"
pass

# --- cached vs disabled output parity ----------------------------------------------
begin "builtins cached output matches disabled output"
DIR="$WORKDIR/builtins-parity"
make_builtins_bin "$DIR"
cat > "$DIR/app.js" <<'EOF'
const os = require('node:os');
const path = require('node:path');
const { Buffer } = require('node:buffer');
console.log(typeof os.platform(), path.join('a', 'b'), Buffer.from('hi').toString('hex'));
process.nextTick(() => console.log('tick'));
EOF
expected="$("$DIR/edge" --no-bytecode-cache "$DIR/app.js" 2>/dev/null)"
"$DIR/edge" "$DIR/app.js" >/dev/null 2>&1  # seed
warm="$("$DIR/edge" "$DIR/app.js" 2>/dev/null)"
[ "$warm" = "$expected" ] || fail "cached output differs: '$warm' vs '$expected'"
pass

# --- worker_threads smoke -----------------------------------------------------------
# Skipped on QuickJS: worker_threads hangs there independently of the bytecode
# cache (the parent never receives worker messages; pre-existing engine issue,
# reproducible on a clean tree with --no-bytecode-cache).
if [ "$SUFFIX" = ".qjsb" ]; then
  echo "skip: builtins cache worker_threads smoke (worker_threads hangs on quickjs)"
else
  begin "builtins cache worker_threads smoke"
  DIR="$WORKDIR/builtins-worker"
  make_builtins_bin "$DIR"
  cat > "$DIR/app.js" <<'EOF'
const { Worker } = require('node:worker_threads');
const worker = new Worker("require('node:worker_threads').parentPort.postMessage(40 + 2);", { eval: true });
worker.on('message', (value) => { console.log(value); });
setTimeout(() => { console.error('worker timed out'); process.exit(1); }, 20000);
EOF
  out1="$("$DIR/edge" "$DIR/app.js" 2>/dev/null)"
  [ "$out1" = "42" ] || fail "worker first run output: $out1"
  [ -f "$DIR/$BUILTINS_FILE_NAME" ] || fail "worker run wrote no builtins cache"
  out2="$(EDGE_BYTECODE_CACHE_TRACE=1 "$DIR/edge" "$DIR/app.js" 2>"$DIR/trace2.txt")"
  [ "$out2" = "42" ] || fail "worker second run output: $out2"
  grep -q "builtin-hit" "$DIR/trace2.txt" || fail "worker second run had no builtin-hit"
  pass
fi

echo
if [ "$FAILURES" -gt 0 ]; then
  echo "$FAILURES failure(s)" >&2
  exit 1
fi
echo "all bytecode-cache tests passed"
