#!/usr/bin/env bash
# Headless smoke test for the rust-egui app: renders every sample x every tab offscreen (real wgpu adapter) and fails on
# a crash, a missing PNG, or a (near-)blank frame. Then runs the unit/integration tests.
set -euo pipefail
cd "$(dirname "$0")/../../apps/egui"
OUT=${1:-/tmp/kerf-egui-smoke}; mkdir -p "$OUT"
cargo build --profile fast -q
BIN=target/fast/kerf-egui
for doc in truss flush monopour; do
  for tab in A B 3d sheet; do
    f="$OUT/$doc-$tab.png"
    "$BIN" --screenshot "$f" --doc "$doc" --tab "$tab" 2>/dev/null
    sz=$(stat -c %s "$f")
    [ "$sz" -gt 20000 ] || { echo "FAIL $doc/$tab: PNG too small ($sz bytes)"; exit 1; }
    echo "ok   $doc/$tab ($sz bytes)"
  done
done
for fmt in svg dxf pdf; do "$BIN" --export "$OUT/truss-A.$fmt" --doc truss --view A 2>/dev/null; done
python3 ../../tools/dxf_check.py "$OUT/truss-A.dxf" >/dev/null && echo "ok   dxf_check"
python3 ../../tools/pdf_check.py "$OUT/truss-A.pdf" >/dev/null && echo "ok   pdf_check"
cargo test --profile fast -q 2>&1 | grep "test result"
