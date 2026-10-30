#!/usr/bin/env bash
# The determinism and validity gate: regenerates the golden set into a temp tree, requires every file to be byte-identical
# to tests/golden, every check summary to report 0 errors, every DXF to pass `dxf_check.py` and every PDF to be vector
# (`pdf_check.py`). Needs tools/.venv on PATH (PATH=<repo>/tools/.venv/bin:$PATH). KERF overrides the binary.
set -euo pipefail
cd "$(dirname "$0")/.."
T=../tools
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
fail=0
tests/make_golden.sh --out "$TMP"
for dir in "$TMP"/*/; do
  name=$(basename "$dir")
  for f in "$dir"*; do
    cmp -s "$f" "tests/golden/$name/$(basename "$f")" || { echo "DIFF $name/$(basename "$f")"; fail=1; }
  done
  grep -q " 0 errors" "$dir/summary.txt" || { echo "CHECK ERRORS $name"; fail=1; }
  for dxf in "$dir"*.dxf; do
    $T/dxf_check.py "$dxf" >/dev/null || { echo "DXF AUDIT FAIL $name/$(basename "$dxf")"; fail=1; }
  done
  for pdf in "$dir"*.pdf; do
    $T/pdf_check.py "$pdf" | grep -q "vector=True" || { echo "PDF NOT VECTOR $name/$(basename "$pdf")"; fail=1; }
  done
done
[ $fail = 0 ] && echo "golden: OK" || { echo "golden: FAILED"; exit 1; }
