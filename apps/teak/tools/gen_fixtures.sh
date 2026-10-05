#!/bin/bash
# Regenerate fixtures/*.drawing.json and fixtures/*.mesh.json from a Kerf engine CLI (real engine output).
#   KERF_BIN=/path/to/kerf tools/gen_fixtures.sh
# KERF_BIN defaults to engines/rust/target/release/kerf. The Zig engine CLI works too (same interface, SPEC 13.2).
# Fixtures committed so far came from the Rust engine (see fixtures/PROVENANCE.txt). Also regenerates tiny.* (hand-written).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$here/../.."
bin="${KERF_BIN:-$root/engines/rust/target/release/kerf}"
for d in truss-bearing-cmu monopour-slab-door-recess flush-beam-strap; do
  "$bin" drawing "$root/spec/details/$d.kerf.json" --view A -o "$here/fixtures/$d.drawing.json"
  "$bin" mesh "$root/spec/details/$d.kerf.json" -o "$here/fixtures/$d.mesh.json"
done
python3 "$here/tools/gen_tiny_fixtures.py"
