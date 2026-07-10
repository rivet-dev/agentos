#!/usr/bin/env sh

export WASIXCC_WASM_EXCEPTIONS=exnref

WASIXCC_DRIVER="${1:-wasixcc}"
export WASIXCC_SYSROOT="${WASIXCC_SYSROOT:-$(wasixccenv print-sysroot)}"
