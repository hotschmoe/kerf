#!/usr/bin/env bash
# Regenerate engines/zig/tests/golden/<detail>/ from the three reference details.
# Usage: tests/make_golden.sh [--png]    (needs zig-out/bin/kerf; --png also renders PNGs for review)
set -euo pipefail
cd "$(dirname "$0")/.."
K=zig-out/bin/kerf
T=../../tools
for doc in ../../spec/details/*.kerf.json; do
  name=$(basename "$doc" .kerf.json)
  d=tests/golden/$name
  mkdir -p "$d"
  $K check "$doc" > "$d/summary.txt" 2>&1 || true
  for v in A B; do
    $K drawing "$doc" --view $v -o "$d/drawing-$v.json"
    $K export "$doc" --view $v --format svg -o "$d/$v.svg"
    $K export "$doc" --view $v --format svg --sheet -o "$d/$v-sheet.svg"
    $K export "$doc" --view $v --format dxf -o "$d/$v.dxf"
    $K export "$doc" --view $v --format pdf -o "$d/$v.pdf"
    $K export "$doc" --view $v --format png -o "$d/$v.raster.png"
    $K export "$doc" --view $v --format png --sheet -o "$d/$v-sheet.raster.png"
    if [ "${1:-}" = "--png" ]; then
      $T/zig-engine/svg2png.sh "$d/$v-sheet.svg" "$d/$v.png" 1400
      $T/dxf_check.py "$d/$v.dxf" --png "$d/$v-dxf.png" --png-size 1400 >/dev/null
      $T/pdf_check.py "$d/$v.pdf" --png "$d/$v-pdf.png" --dpi 110 >/dev/null
    fi
  done
  $K mesh "$doc" -o "$d/mesh.json"
done
