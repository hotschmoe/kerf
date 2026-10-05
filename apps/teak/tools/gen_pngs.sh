#!/bin/bash
# Build the native PNG dev tool and render the fixtures (or any Drawing JSON).
#   tools/gen_pngs.sh                       # renders fixtures/*.drawing.json -> $OUT (default /tmp/kerf-pngs)
#   tools/gen_pngs.sh in.json out.png [render_cli flags]
set -euo pipefail
export PATH="$HOME/tools/zig-aarch64-linux-0.16.0:$PATH"
here="$(cd "$(dirname "$0")/.." && pwd)"
spec="$here/../../spec"
bin="${KERF_RENDER_BIN:-/tmp/kerf-render-cli}"
cd "$here"
zig build-exe -OReleaseFast --dep spec_font_json -Mroot=src/draw/render_cli.zig -Mspec_font_json="$spec/fonts/kerf-simplex.json" \
  --cache-dir "${ZIG_LOCAL_CACHE_DIR:-/tmp/kerf-zig-cache}" --global-cache-dir "$HOME/.cache/zig" -femit-bin="$bin" 2>&1 | head -50
if [ $# -ge 2 ]; then exec "$bin" "$@"; fi
out="${OUT:-/tmp/kerf-pngs}"; mkdir -p "$out"
for f in fixtures/*.drawing.json; do
  n="$(basename "$f" .drawing.json)"
  "$bin" "$f" "$out/$n.png"
  "$bin" "$f" "$out/$n.live.png" --live
done
