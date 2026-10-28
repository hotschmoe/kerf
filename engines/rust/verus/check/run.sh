#!/usr/bin/env bash
# End-to-end behavioural checks of kerf-verified against the Zig engine (no unit tests):
#   pdf     13 golden PDFs rebuilt byte for byte by the verified writer
#   png     26 golden raster PNGs: filters, zlib frame and container reproduced byte for byte
#   sort    2448 generated cases vs the engine's sort.stable
#   json    differential fuzz vs json.zig (mutated spec/details documents + targeted cases)
#   raster  every canvas call of the engine's PNG export replayed, pixels compared
# Usage: run.sh [json-mutants]   (default 100000). Needs Zig 0.17 ($ZIG) and stable Rust.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
ZIG=${ZIG:-$HOME/tools/zig-aarch64-linux-0.17.0/zig}
N=${1:-100000}
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
SRC=$REPO/engines/zig/src

zb() { "$ZIG" build-exe -O ReleaseFast --cache-dir "$W/zc" --global-cache-dir "$W/zg" "$@"; }
zb --dep json -Mroot="$HERE/json_dump.zig" -Mjson="$SRC/json.zig" -femit-bin="$W/json_dump"
zb --dep sort -Mroot="$HERE/sort_dump.zig" -Msort="$SRC/sort.zig" -femit-bin="$W/sort_dump"
zb "$HERE/png_inflate.zig" -femit-bin="$W/png_inflate"

mkdir -p "$W/t/engines/zig" "$W/png" "$W/trace"
cp -r "$REPO/engines/zig/"{build.zig,build.zig.zon,src} "$W/t/engines/zig/"
ln -s "$REPO/spec" "$W/t/spec"
python3 "$HERE/trace_raster.py" "$W/t/engines/zig/src/raster.zig"
(cd "$W/t/engines/zig" && "$ZIG" build -Doptimize=ReleaseFast --cache-dir "$W/zc2")
K=$W/t/engines/zig/zig-out/bin/kerf
cd "$REPO"
for doc in spec/details/*.kerf.json engines/zig/tests/docs/*.kerf.json; do
    name=$(basename "$doc" .kerf.json)
    views=$(python3 -c 'import json,sys;print(" ".join(v["id"] for v in json.load(open(sys.argv[1]))["views"]))' "$doc")
    for v in $views; do
        "$K" export "$doc" --view "$v" --format png -o "$W/trace/$name-$v.trace"
        "$K" export "$doc" --view "$v" --format png --sheet -o "$W/trace/$name-$v-sheet.trace"
        for px in 200 3000; do "$K" export "$doc" --view "$v" --format png --px $px -o "$W/trace/$name-$v-px$px.trace"; done
    done
done
for d in engines/zig/tests/golden/*/; do
    for f in "$d"*.raster.png; do cp "$f" "$W/png/$(basename "$d")-$(basename "$f")"; done
done
"$W/png_inflate" "$W"/png/*.png

cd "$HERE"
cargo +stable build --release -q
B=$HERE/target/release/kvcheck
$B pdf "$REPO"/engines/zig/tests/golden/*/*.pdf | tail -n +1 | awk '{print $2}' | sort | uniq -c | sed 's/^/pdf: /'
$B png "$W"/png/*.png | awk '{print $7, $10, $12}' | sort | uniq -c | sed 's/^/png filters|zlib|container: /'
$B sort "$W/sort_dump" "$W/sort"
$B json "$W/json_dump" "$W/json" "$N" "$REPO"/spec/details/*.kerf.json
$B raster "$W"/trace/*.trace | awk '{print $8}' | sort | uniq -c | sed 's/^/raster: /'
