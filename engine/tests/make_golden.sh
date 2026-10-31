#!/usr/bin/env bash
# Generates the golden set: for every reference detail and test document, the drawing IR, SVG (bare and sheet), DXF, PDF,
# PNG (bare and sheet), mesh and the check summary of each view.
#   tests/make_golden.sh [--out DIR] [--png]
# Without --out it rewrites tests/golden. --png also renders review images (sheet SVG via chromium, DXF via ezdxf, PDF via
# pdfium; needs tools/.venv). KERF overrides the binary (default target/release/kerf).
set -euo pipefail
cd "$(dirname "$0")/.."
K=${KERF:-target/release/kerf}
T=../tools
OUT=tests/golden
PNG=0
while [ $# -gt 0 ]; do
  case $1 in
    --out) OUT=$2; shift ;;
    --png) PNG=1 ;;
    *) echo "usage: $0 [--out DIR] [--png]" >&2; exit 2 ;;
  esac
  shift
done
for doc in ../spec/details/*.kerf.json tests/docs/*.kerf.json; do
  name=$(basename "$doc" .kerf.json)
  d=$OUT/$name
  mkdir -p "$d"
  $K check "$doc" > "$d/summary.txt" 2>&1 || true
  views=$(python3 -c 'import json,sys;print(" ".join(v["id"] for v in json.load(open(sys.argv[1]))["views"]))' "$doc")
  for v in $views; do
    $K drawing "$doc" --view "$v" -o "$d/drawing-$v.json"
    $K export "$doc" --view "$v" --format svg -o "$d/$v.svg"
    $K export "$doc" --view "$v" --format svg --sheet -o "$d/$v-sheet.svg"
    $K export "$doc" --view "$v" --format dxf -o "$d/$v.dxf"
    $K export "$doc" --view "$v" --format pdf -o "$d/$v.pdf"
    $K export "$doc" --view "$v" --format png -o "$d/$v.raster.png"
    $K export "$doc" --view "$v" --format png --sheet -o "$d/$v-sheet.raster.png"
    if grep -q '"acknowledge"' "$doc"; then
      $K export "$doc" --view "$v" --format svg --sheet --review -o "$d/$v-review.svg"
    fi
    if [ $PNG = 1 ]; then
      $T/svg2png.sh "$d/$v-sheet.svg" "$d/$v.png" 1400
      $T/dxf_check.py "$d/$v.dxf" --png "$d/$v-dxf.png" --png-size 1400 >/dev/null
      $T/pdf_check.py "$d/$v.pdf" --png "$d/$v-pdf.png" --dpi 110 >/dev/null
    fi
  done
  layouts=$(python3 -c 'import json,sys;print(" ".join(l["id"] for l in json.load(open(sys.argv[1])).get("layouts",[])))' "$doc")
  for l in $layouts; do
    $K drawing "$doc" --layout "$l" -o "$d/drawing-layout-$l.json"
    $K export "$doc" --layout "$l" --format svg --sheet -o "$d/layout-$l-sheet.svg"
    $K export "$doc" --layout "$l" --format dxf -o "$d/layout-$l.dxf"
    $K export "$doc" --layout "$l" --format pdf -o "$d/layout-$l.pdf"
    $K export "$doc" --layout "$l" --format png --sheet -o "$d/layout-$l-sheet.raster.png"
    if [ $PNG = 1 ]; then
      $T/svg2png.sh "$d/layout-$l-sheet.svg" "$d/layout-$l.png" 1400
      $T/dxf_check.py "$d/layout-$l.dxf" --png "$d/layout-$l-dxf.png" --png-size 1400 >/dev/null
      $T/pdf_check.py "$d/layout-$l.pdf" --png "$d/layout-$l-pdf.png" --dpi 110 >/dev/null
    fi
  done
  $K mesh "$doc" -o "$d/mesh.json"
done
