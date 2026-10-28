#!/usr/bin/env bash
# Verifies the Verus-proven modules of kerf-core (sort, json, png, pdf/writer, raster/kernels) through the
# verification root verified.rs, which includes those exact files, then builds kerf-core as plain stable Rust
# for native and wasm32 (no dependencies, warnings are errors). Toolchain: setup.sh.
#   verify.sh [extra verus args, e.g. --verify-only-module json --time-expanded]
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
VERUS=${VERUS:-$HOME/tools/verus/source/target-verus/release/verus}
export VERUS_Z3_PATH=${VERUS_Z3_PATH:-$HOME/tools/z3-4.16.0/bin/z3}
cd "$HERE"
"$VERUS" --crate-type=lib verified.rs --time "$@"
cd ..
RUSTFLAGS="-D warnings" cargo +stable build -q --release -p kerf-core
RUSTFLAGS="-D warnings" cargo +stable build -q --release -p kerf-core --target wasm32-unknown-unknown
echo "verify.sh: verified, and kerf-core builds on stable for native and wasm32"
