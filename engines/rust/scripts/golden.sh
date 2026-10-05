#!/usr/bin/env bash
# Regenerate golden artifacts + PNGs for the reference details, then audit DXF/PDF with the shared tools.
# Usage: engines/rust/scripts/golden.sh   (review the PNGs with your image viewer before committing)
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
cd "$HERE"
KERF_UPDATE_GOLDEN=1 cargo test -q -p kerf-core --test golden golden_outputs 2>&1 | tail -2
for d in tests/golden/*/; do
  name="$(basename "$d")"
  for v in A B; do
    [ -f "$d/$v.svg" ] || continue
    "$HERE/scripts/svg2png.sh" "$d/$v-sheet.svg" "$d/$v-sheet.png" 1650
    "$HERE/scripts/svg2png.sh" "$d/$v.svg" "$d/$v.png" 1600
    "$ROOT/tools/dxf_check.py" "$d/$v.dxf" --png "$d/$v-dxf.png" > "$d/$v-dxf-check.txt"
    "$ROOT/tools/pdf_check.py" "$d/$v.pdf" --png "$d/$v-pdf.png" > "$d/$v-pdf-check.txt"
    head -1 "$d/$v-dxf-check.txt"; sed -n 2p "$d/$v-pdf-check.txt"
  done
done
