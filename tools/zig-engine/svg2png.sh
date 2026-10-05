#!/usr/bin/env bash
# Usage: svg2png.sh in.svg out.png [width_px]   (headless chromium via tools/shot.mjs)
# Wraps the SVG in an <img> page scaled to the viewport so thin lines are inspectable.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
IN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
OUT="$2"; W="${3:-1400}"
DIR="$(dirname "$IN")"
BASE="$(basename "$IN")"
WRAP="$DIR/.wrap-$BASE.html"
printf '<!doctype html><body style="margin:0;background:#fff"><img src="%s" style="width:100vw;display:block"></body>' "$BASE" > "$WRAP"
PORT=$((20000 + RANDOM % 10000))
node "$HERE/serve.mjs" "$DIR" "$PORT" >/dev/null 2>&1 &
SP=$!
trap 'kill $SP 2>/dev/null || true; rm -f "$WRAP"' EXIT
sleep 0.6
# aspect from the SVG viewBox
read -r VW VH < <(grep -o 'viewBox="[^"]*"' "$IN" | head -1 | sed 's/viewBox="0 0 \([0-9.]*\) \([0-9.]*\)"/\1 \2/')
H=$(python3 -c "print(int($W*$VH/$VW)+2)")
node "$HERE/shot.mjs" "http://localhost:$PORT/.wrap-$BASE.html" "$OUT" --width "$W" --height "$H" --wait-ms 300 --no-gpu >/dev/null
