#!/usr/bin/env bash
# Rust twin of engines/zig/tests/check_golden.sh: same goldens (engines/zig/tests/golden), byte-for-byte.
# Also compares the sheet SVGs, mesh.json and the check summary, and runs the DXF / PDF checkers on every export.
set -euo pipefail
cd "$(dirname "$0")/.."
K=${KERF:-target/release/kerf}
T=../../tools
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
fail=0
for doc in ../../spec/details/*.kerf.json ../zig/tests/docs/*.kerf.json; do
  name=$(basename "$doc" .kerf.json)
  g=../zig/tests/golden/$name
  views=$(python3 -c 'import json,sys;print(" ".join(v["id"] for v in json.load(open(sys.argv[1]))["views"]))' "$doc")
  for v in $views; do
    $K drawing "$doc" --view $v -o "$TMP/drawing-$v.json"
    $K export "$doc" --view $v --format svg -o "$TMP/$v.svg"
    $K export "$doc" --view $v --format svg --sheet -o "$TMP/$v-sheet.svg"
    $K export "$doc" --view $v --format dxf -o "$TMP/$v.dxf"
    $K export "$doc" --view $v --format pdf -o "$TMP/$v.pdf"
    $K export "$doc" --view $v --format png -o "$TMP/$v.raster.png"
    $K export "$doc" --view $v --format png --sheet -o "$TMP/$v-sheet.raster.png"
    for f in drawing-$v.json $v.svg $v-sheet.svg $v.dxf $v.pdf $v.raster.png $v-sheet.raster.png; do
      cmp -s "$TMP/$f" "$g/$f" || { echo "DIFF $name/$f"; fail=1; }
    done
    $T/dxf_check.py "$TMP/$v.dxf" >/dev/null || { echo "DXF AUDIT FAIL $name/$v"; fail=1; }
    $T/pdf_check.py "$TMP/$v.pdf" | grep -q "vector=True" || { echo "PDF NOT VECTOR $name/$v"; fail=1; }
  done
  $K mesh "$doc" -o "$TMP/mesh.json"
  cmp -s "$TMP/mesh.json" "$g/mesh.json" || { echo "DIFF $name/mesh.json"; fail=1; }
  $K check "$doc" > "$TMP/summary.txt" 2>&1 || true
  cmp -s "$TMP/summary.txt" "$g/summary.txt" || { echo "DIFF $name/summary.txt"; fail=1; }
  grep -q " 0 errors" "$TMP/summary.txt" || { echo "CHECK ERRORS $name"; fail=1; }
done
[ $fail = 0 ] && echo "golden: OK" || { echo "golden: FAILED"; exit 1; }
