#!/usr/bin/env bash
# Verifies engines/rust/kerf-verified with Verus, then builds the same sources as plain stable Rust
# (native and wasm32, no dependencies, warnings are errors). Toolchain: setup.sh.
#   verify.sh [extra verus args, e.g. --verify-only-module json --time-expanded]
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$HERE/../kerf-verified
VERUS=${VERUS:-$HOME/tools/verus/source/target-verus/release/verus}
export VERUS_Z3_PATH=${VERUS_Z3_PATH:-$HOME/tools/z3-4.16.0/bin/z3}
cd "$CRATE"
"$VERUS" --crate-type=lib src/lib.rs --time "$@"
RUSTFLAGS="-D warnings" cargo +stable build -q --release
RUSTFLAGS="-D warnings" cargo +stable build -q --release --target wasm32-unknown-unknown
echo "verify.sh: verified, and builds on stable for native and wasm32 with no dependencies"
