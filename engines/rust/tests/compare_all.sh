#!/usr/bin/env bash
# Gate 2: Drawing IR of the Rust CLI vs the Zig CLI on every view of every reference document.
# Usage: compare_all.sh [rust-kerf] [zig-kerf]. Prints one line per view; exit 1 on any difference.
set -euo pipefail
cd "$(dirname "$0")/../../.."
R=${1:-engines/rust/target/release/kerf}; Z=${2:-engines/zig/zig-out/bin/kerf}
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
n=0; bad=0; ident=0
for d in spec/details/*.kerf.json engines/zig/tests/docs/*.kerf.json; do
  for v in $(python3 -c 'import json,sys;print(" ".join(x["id"] for x in json.load(open(sys.argv[1]))["views"]))' "$d"); do
    "$Z" drawing "$d" --view "$v" -o "$TMP/z.json"; "$R" drawing "$d" --view "$v" -o "$TMP/r.json"
    n=$((n+1)); cmp -s "$TMP/z.json" "$TMP/r.json" && ident=$((ident+1))
    python3 engines/rust/tests/compare_drawings.py "$TMP/z.json" "$TMP/r.json" || { bad=$((bad+1)); echo "DIFF $d $v"; }
  done
done
echo "views=$n byte-identical=$ident ir-diffs=$bad"
[ $bad = 0 ]
