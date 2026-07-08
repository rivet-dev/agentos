#!/bin/bash
# patch-std.sh — Apply wasmVM patches to the Rust std source tree
#
# Patches modify the WASI platform implementation in std to support
# process spawning, pipes, user/group IDs, and terminal detection
# via custom host_process/host_user WASM imports.
#
# Usage:
#   ./scripts/patch-std.sh [--check] [--reverse]
#
# Options:
#   --check    Dry-run: verify patches apply cleanly without modifying files
#   --reverse  Reverse (unapply) previously applied patches

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WASMCORE_DIR="$(dirname "$SCRIPT_DIR")"
PATCHES_DIR="$WASMCORE_DIR/std-patches"

# Get the sysroot for the toolchain specified in rust-toolchain.toml
SYSROOT="$(rustc --print sysroot)"
STD_SRC="$SYSROOT/lib/rustlib/src/rust"

if [ ! -d "$STD_SRC/library/std" ]; then
    echo "ERROR: Rust source not found at $STD_SRC"
    echo "Ensure rust-src component is installed: rustup component add rust-src"
    exit 1
fi

# Parse arguments
MODE="apply"
PATCH_FLAGS="-p1"
for arg in "$@"; do
    case "$arg" in
        --check)
            MODE="check"
            ;;
        --reverse)
            MODE="reverse"
            ;;
        *)
            echo "Unknown argument: $arg"
            echo "Usage: $0 [--check] [--reverse]"
            exit 1
            ;;
    esac
done

# `patch-std` mutates rustup's installed rust-src tree. CI runners and local
# machines can therefore inherit a partially patched std from a prior failed
# build, and `patch --forward` will skip instead of repairing mismatched hunks.
# Start apply mode from a pristine rust-src component when rustup owns this
# sysroot; non-rustup toolchains keep the historical behavior.
if [ "$MODE" = "apply" ] && command -v rustup >/dev/null 2>&1; then
    TOOLCHAIN="$(basename "$SYSROOT")"
    case "$SYSROOT" in
        */.rustup/toolchains/*)
            echo "Refreshing rust-src for toolchain $TOOLCHAIN before applying std patches..."
            rm -rf "$SYSROOT"
            rustup toolchain install "$TOOLCHAIN" \
                --profile minimal \
                --component rust-src \
                --target wasm32-wasip1 \
                --force >/dev/null
            ;;
    esac
fi

# Find std patch files in order (reversed for --reverse mode).
# Only top-level std-patches/*.patch are std-source patches; subdirectories
# (std-patches/crates/*, std-patches/wasi-libc/*) target vendored crates and wasi-libc
# and must NOT be applied to the Rust std source tree, so use -maxdepth 1.
if [ "$MODE" = "reverse" ]; then
    PATCH_FILES=$(find "$PATCHES_DIR" -maxdepth 1 -name '*.patch' -type f 2>/dev/null | sort -r)
else
    PATCH_FILES=$(find "$PATCHES_DIR" -maxdepth 1 -name '*.patch' -type f 2>/dev/null | sort)
fi

if [ -z "$PATCH_FILES" ]; then
    echo "No patch files found in $PATCHES_DIR"
    exit 0
fi

PATCH_COUNT=$(echo "$PATCH_FILES" | wc -l)
echo "Found $PATCH_COUNT patch(es) in $PATCHES_DIR"
echo "Rust std source: $STD_SRC"
echo ""

FAILED=0

for PATCH in $PATCH_FILES; do
    PATCH_NAME="$(basename "$PATCH")"

    case "$MODE" in
        check)
            echo -n "Checking $PATCH_NAME ... "
            if patch --batch --dry-run $PATCH_FLAGS -d "$STD_SRC" < "$PATCH" > /dev/null 2>&1; then
                echo "OK (applies cleanly)"
            elif patch --batch --dry-run -R $PATCH_FLAGS -d "$STD_SRC" < "$PATCH" > /dev/null 2>&1; then
                echo "OK (already applied)"
            else
                # When layered patches modify files created by earlier patches,
                # neither forward nor reverse will match exactly. Check if any
                # new files from this patch exist as a secondary heuristic.
                NEW_FILES=$(grep '^+++ b/' "$PATCH" | sed 's|^+++ b/||' | while read -r f; do
                    [ -f "$STD_SRC/$f" ] && echo "$f"
                done)
                if [ -n "$NEW_FILES" ]; then
                    echo "OK (applied, modified by later patch)"
                else
                    echo "FAIL (does not apply)"
                    FAILED=1
                fi
            fi
            ;;
        apply)
            echo -n "Applying $PATCH_NAME ... "
            # Use `--forward` (-N) for idempotency: it applies hunks that are not
            # yet present and SKIPS hunks already applied (reversed) instead of
            # applying them a second time. Without this, additive (insert-only)
            # patches stay forward-applicable after they are applied — their
            # anchor context is still present — and a naive forward apply inserts
            # a duplicate copy, producing E0119 conflicting-implementation errors
            # on a re-run. `--forward` makes a second `make wasm` a no-op.
            if patch --batch --forward --dry-run $PATCH_FLAGS -d "$STD_SRC" < "$PATCH" > /dev/null 2>&1; then
                patch --batch --forward $PATCH_FLAGS -d "$STD_SRC" < "$PATCH" > /dev/null 2>&1
                echo "applied"
            else
                echo "already applied (skipping)"
            fi
            ;;
        reverse)
            echo -n "Reversing $PATCH_NAME ... "
            if patch --batch -R $PATCH_FLAGS -d "$STD_SRC" < "$PATCH" > /dev/null 2>&1; then
                echo "reversed"
            else
                echo "not applied (skipping)"
            fi
            ;;
    esac
done

# Install companion source files that a patch declares (e.g. `pub mod process;`)
# but cannot reliably carry inline: a `diff`/`patch` cannot create a brand-new
# file in the std source from a `/dev/null` hunk reliably across patch versions
# (the hunk is silently skipped, leaving the declared module with no source file
# and a `file not found for module` E0583 build error). Convention mirrors the
# vendored-crate mechanism in patch-vendor.sh: `std-patches/copy.manifest` with lines
# "<src-relative-to-PATCHES_DIR> <dest-relative-to-STD_SRC>". Example:
# `std/os/wasi/process.rs library/std/src/os/wasi/process.rs` installs the public
# wasi child-pipe fd traits that 0007-wasi-childpipe-fd.patch's `pub mod process;`
# references. Without this the patched std fails to compile (missing module).
MANIFEST="$PATCHES_DIR/copy.manifest"
if [ -f "$MANIFEST" ]; then
    while read -r SRC DEST; do
        # Skip blank lines and comments.
        case "$SRC" in ""|\#*) continue ;; esac
        case "$MODE" in
            apply)
                if [ ! -f "$PATCHES_DIR/$SRC" ]; then
                    echo "copy.manifest source missing: $SRC"
                    FAILED=1
                    continue
                fi
                mkdir -p "$(dirname "$STD_SRC/$DEST")"
                cp "$PATCHES_DIR/$SRC" "$STD_SRC/$DEST"
                echo "Installed companion: $SRC -> $DEST"
                ;;
            reverse)
                rm -f "$STD_SRC/$DEST"
                echo "Removed companion: $DEST"
                ;;
            check)
                if [ ! -f "$PATCHES_DIR/$SRC" ]; then
                    echo "copy.manifest source missing: $SRC"
                    FAILED=1
                fi
                ;;
        esac
    done < "$MANIFEST"
fi

echo ""
if [ "$FAILED" -ne 0 ]; then
    echo "Some patches failed to apply. Check patch compatibility with current nightly."
    exit 1
else
    case "$MODE" in
        check)   echo "All patches verified." ;;
        apply)   echo "All patches applied successfully." ;;
        reverse) echo "All patches reversed." ;;
    esac
fi
