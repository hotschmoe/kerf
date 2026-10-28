#!/usr/bin/env bash
# All output gates the performance work must keep green (run after every change, see PERF.md).
# Usage: bench/gates.sh [fuzz N=300] [--serve]
set -euo pipefail
cd "$(dirname "$0")/.."
N=${1:-300}
export PATH=$HOME/github/kerf/tools/.venv/bin:$HOME/.cache/trunk/wasm-opt-version_123/bin:$PATH
cargo build --release -q
tests/build_wasm.sh >/dev/null
tests/check_golden.sh | tail -1
(cd ../zig && node ../../tools/zig-engine/wasm_golden.mjs ../rust/dist/kerf.wasm | tail -1)
node ../../tools/zig-engine/wasm_check.mjs dist/kerf.wasm ../../spec/details/truss-bearing-cmu.kerf.json A "$(mktemp -d)/a.svg" | grep -E "ABI|imports" | head -1
node ../zig/tests/cli_ergonomics.mjs "$PWD/target/release/kerf" | tail -1
KERF=$PWD/target/release/kerf KERF_ZIG=$PWD/../zig/zig-out/bin/kerf python3 tests/diff_fuzz.py "$N" 1 | tail -1
if [ "${2:-}" = "--serve" ]; then (cd ../zig && node tests/serve_smoke.mjs ../rust/target/release/kerf | tail -1); fi
