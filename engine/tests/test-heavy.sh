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
KERF="$binary" node tests/check_golden.mjs
node tests/wasm_golden.mjs
node tests/cli_ergonomics.mjs "$binary"
node tests/serve_smoke.mjs "$binary"
node tests/platform_serve.mjs "$binary"
cargo build
KERF="$target/debug/kerf" python3 tests/fuzz.py 300 1
(
  cd ../apps/web
  KERF_WASM="$root/engine/dist/kerf.wasm" npm run build
  node test/e2e.mjs
  KERF_WASM="$root/engine/dist/kerf.wasm" npm run build:serve
  node test/e2e-serve.mjs
)

CARGO_TARGET_DIR="$target/heavy-ui" KERF_UI="$root/apps/web/dist-serve" cargo build --release
stub=$(mktemp -d)
printf '#!/bin/sh\necho "1.0.0 (Claude Code)"\n' > "$stub/claude"
chmod +x "$stub/claude"
(
  cd ../apps/web
  PATH="$stub:$PATH" KERF_SERVE_BIN="$target/heavy-ui/release/kerf" node test/e2e-serve.mjs --real '--only=REAL agent bridge'
)
