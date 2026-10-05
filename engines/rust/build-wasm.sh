#!/usr/bin/env bash
# Build the Kerf engine as a raw-ABI wasm module: engines/rust/dist/kerf.wasm
# Usage: engines/rust/build-wasm.sh [--size-report]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
cargo build --release --target wasm32-unknown-unknown -p kerf-wasm
mkdir -p dist
cp target/wasm32-unknown-unknown/release/kerf_wasm.wasm dist/kerf.wasm
echo "built $HERE/dist/kerf.wasm ($(wc -c < dist/kerf.wasm) bytes)"
if [ "${1:-}" = "--size-report" ]; then
  "$HERE/../../tools/size_report.sh" dist/kerf.wasm
fi
