#!/usr/bin/env bash
# Determinism + validity gate: regenerate into a temp tree, compare byte-for-byte with the committed
# goldens, and run the DXF / PDF checkers on every export.
set -euo pipefail
cd "$(dirname "$0")/.."
K=zig-out/bin/kerf
T=../../tools
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
fail=0
for doc in ../../spec/details/*.kerf.json tests/docs/*.kerf.json; do
  name=$(basename "$doc" .kerf.json)
  g=tests/golden/$name
  views=$(python3 -c 'import json,sys;print(" ".join(v["id"] for v in json.load(open(sys.argv[1]))["views"]))' "$doc")
  for v in $views; do
    for out in "drawing-$v.json:drawing --view $v" ; do :; done
    $K drawing "$doc" --view $v -o "$TMP/drawing-$v.json"
    $K export "$doc" --view $v --format svg -o "$TMP/$v.svg"
    $K export "$doc" --view $v --format dxf -o "$TMP/$v.dxf"
    $K export "$doc" --view $v --format pdf -o "$TMP/$v.pdf"
    $K export "$doc" --view $v --format png -o "$TMP/$v.raster.png"
    $K export "$doc" --view $v --format png --sheet -o "$TMP/$v-sheet.raster.png"
    for f in drawing-$v.json $v.svg $v.dxf $v.pdf $v.raster.png $v-sheet.raster.png; do
      cmp -s "$TMP/$f" "$g/$f" || { echo "DIFF $name/$f"; fail=1; }
    done
    $T/dxf_check.py "$TMP/$v.dxf" >/dev/null || { echo "DXF AUDIT FAIL $name/$v"; fail=1; }
    $T/pdf_check.py "$TMP/$v.pdf" | grep -q "vector=True" || { echo "PDF NOT VECTOR $name/$v"; fail=1; }
  done
  $K check "$doc" 2>&1 | grep -q " 0 errors" || { echo "CHECK ERRORS $name"; fail=1; }
done
[ $fail = 0 ] && echo "golden: OK" || { echo "golden: FAILED"; exit 1; }
