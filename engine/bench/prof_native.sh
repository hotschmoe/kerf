#!/usr/bin/env bash
# perf profile of the native bench loop: bench/prof_native.sh <out.folded> [bench args, e.g. --loop 3 --exclude png]
set -euo pipefail
cd "$(dirname "$0")/.."
out=$1; shift
PERF=${PERF:-/usr/share/cix/bin/perf}
RUSTFLAGS="-C force-frame-pointers=yes" cargo build -q --profile profiling --example bench
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
"$PERF" record -q -F 4000 -g -o "$tmp/perf.data" target/profiling/examples/bench target/bench-cases "$@" 2>/dev/null
"$PERF" script -i "$tmp/perf.data" 2>/dev/null > "$tmp/perf.txt"
python3 bench/flame.py collapse "$tmp/perf.txt" "$PWD/target/profiling/examples/bench" > "$out"
