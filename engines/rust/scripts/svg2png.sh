#!/usr/bin/env bash
# Usage: svg2png.sh <in.svg> <out.png> [width_px=1600]
# Renders an SVG to PNG with the repo's headless chromium tooling (tools/shot.mjs + tools/serve.mjs).
set -euo pipefail
IN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
OUT="$2"
W="${3:-1600}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TMP="$(mktemp -d)"
trap 'kill $SRV 2>/dev/null || true; rm -rf "$TMP"' EXIT
cp "$IN" "$TMP/in.svg"
cat > "$TMP/index.html" <<HTML
<!doctype html><html><body style="margin:0;background:#fff"><img id="i" src="in.svg" style="width:100vw;display:block" onload="window.__ready=true"></body></html>
HTML
PORT=$((20000 + RANDOM % 20000))
node "$ROOT/tools/serve.mjs" "$TMP" "$PORT" >/dev/null 2>&1 &
SRV=$!
sleep 0.5
# height from the svg aspect ratio
H=$(python3 - "$IN" "$W" <<'PY'
import re,sys
s=open(sys.argv[1]).read(4000)
m=re.search(r'viewBox="0 0 ([\d.]+) ([\d.]+)"',s)
w,h=float(m.group(1)),float(m.group(2))
print(int(float(sys.argv[2])*h/w)+1)
PY
)
node "$ROOT/tools/shot.mjs" "http://localhost:$PORT/" "$OUT" --width "$W" --height "$H" --wait-for "window.__ready===true" >/dev/null
