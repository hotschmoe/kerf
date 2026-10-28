#!/usr/bin/env bash
# Build dist/kerf.wasm: cargo wasm32 release, export section pruned to the SPEC 13.1 ABI, then binaryen `wasm-opt -Oz`
# when it is on PATH (the Zig release does the same). Usage: tests/build_wasm.sh   -> engines/rust/dist/kerf.wasm
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --profile small -p kerf-wasm --target wasm32-unknown-unknown
mkdir -p dist
python3 tests/wasm_prune.py target/wasm32-unknown-unknown/small/kerf_wasm.wasm dist/kerf.wasm
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --strip-debug --strip-producers dist/kerf.wasm -o dist/kerf.wasm
fi
ls -l dist/kerf.wasm
