#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD/..
target=${CARGO_TARGET_DIR:-$PWD/target}
case $target in /*) ;; *) target=$PWD/$target ;; esac
binary=$target/release/kerf
if [ -x "$root/tools/.venv/bin/python" ]; then
  export PATH="$root/tools/.venv/bin:$PATH"
fi
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
cargo build --release
node ../tools/build_wasm.mjs
KERF="$binary" node tests/check_golden.mjs truss-bearing-cmu flush-beam-strap hss-column-base-plate
node tests/cli_e2e.mjs "$binary"
node tests/serve_e2e.mjs "$binary"
node tests/wasm_check.mjs dist/kerf.wasm ../spec/details/truss-bearing-cmu.kerf.json A "${TMPDIR:-/tmp}/kerf-gate-wasm.svg"
(
  cd ../apps/web
  KERF_WASM="$root/engine/dist/kerf.wasm" npm run build
  KERF_E2E_CASE='save / open .kerf.json round trip' node test/e2e.mjs
)
