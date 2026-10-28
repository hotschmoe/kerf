#!/usr/bin/env bash
# Unstripped (name section) small wasm for profiling: bench/wasm_names.sh <out.wasm>
set -euo pipefail
cd "$(dirname "$0")/.."
T=${CARGO_TARGET_DIR_PROF:-target/prof}
CARGO_PROFILE_SMALL_STRIP=false cargo build -q --profile small -p kerf-wasm --target wasm32-unknown-unknown --target-dir "$T"
python3 tests/wasm_prune.py "$T/wasm32-unknown-unknown/small/kerf_wasm.wasm" "$1"
