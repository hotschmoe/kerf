#!/usr/bin/env bash
# Every engine gate in one run: release build, golden set, wasm (goldens + ABI), CLI ergonomics, mutation fuzz and, with
# --serve, the `kerf serve` smoke test. Usage: tests/gates.sh [fuzz cases=300] [--serve]
# Needs tools/.venv on PATH (the DXF/PDF checkers) and, for a size-faithful wasm, wasm-opt.
set -euo pipefail
cd "$(dirname "$0")/.."
N=${1:-300}
cargo build --release -q
node ../tools/build_wasm.mjs >/dev/null
tests/check_golden.sh | tail -1
node tests/wasm_golden.mjs | tail -1
node tests/wasm_check.mjs dist/kerf.wasm ../spec/details/truss-bearing-cmu.kerf.json A "$(mktemp -d)/a.svg" | grep -E "ABI|imports" | head -1
node tests/cli_ergonomics.mjs "$PWD/target/release/kerf" | tail -1
KERF=$PWD/target/release/kerf python3 tests/fuzz.py "$N" 1 | tail -1
if [ "${2:-}" = "--serve" ]; then node tests/serve_smoke.mjs "$PWD/target/release/kerf" | tail -1; fi
