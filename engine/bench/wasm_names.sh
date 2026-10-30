#!/usr/bin/env bash
# Unstripped (name section) small wasm for profiling: bench/wasm_names.sh <out.wasm>
set -euo pipefail
cd "$(dirname "$0")/.."
CARGO_PROFILE_SMALL_STRIP=false CARGO_TARGET_DIR=${CARGO_TARGET_DIR_PROF:-target/prof} node ../tools/build_wasm.mjs --no-wasm-opt --out "$(realpath -m "$1")"
