#!/usr/bin/env bash
# Usage: size_report.sh <file...>   -> raw / gzip -9 / brotli (q11) sizes in bytes + ratios.
set -euo pipefail
[ $# -ge 1 ] || { echo "usage: size_report.sh <file...>" >&2; exit 2; }
HERE="$(cd "$(dirname "$0")" && pwd)"
PY="$HERE/.venv/bin/python"; [ -x "$PY" ] || PY=python3
brotli_size() {
  if command -v brotli >/dev/null 2>&1; then brotli -q 11 -c "$1" | wc -c
  else "$PY" -c 'import sys,brotli;print(len(brotli.compress(open(sys.argv[1],"rb").read(),quality=11)))' "$1"; fi
}
printf '%-48s %12s %12s %12s %7s %7s\n' FILE RAW GZIP-9 BROTLI-11 GZ% BR%
tr=0; tg=0; tb=0
for f in "$@"; do
  [ -f "$f" ] || { echo "skip (not a file): $f" >&2; continue; }
  raw=$(wc -c <"$f"); gz=$(gzip -9 -c "$f" | wc -c); br=$(brotli_size "$f")
  pct() { awk -v a="$1" -v b="$2" 'BEGIN{ if (b==0) print "-"; else printf "%.1f", 100*a/b }'; }
  printf '%-48s %12d %12d %12d %6s%% %6s%%\n' "$f" "$raw" "$gz" "$br" "$(pct "$gz" "$raw")" "$(pct "$br" "$raw")"
  tr=$((tr+raw)); tg=$((tg+gz)); tb=$((tb+br))
done
[ $# -gt 1 ] && printf '%-48s %12d %12d %12d\n' TOTAL "$tr" "$tg" "$tb"
exit 0
