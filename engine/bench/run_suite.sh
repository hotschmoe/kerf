#!/usr/bin/env bash
# Runs the benchmark suite pinned to one core: native in-process (bench.rs), wasm (bench_wasm.mjs) and
# CLI processes (bench_cli.py). Usage: bench/run_suite.sh <out dir> <cpu> name=binary ...   where a name starting with
# "n-" is a native bench binary, "w-" a kerf.wasm, "c-" a kerf CLI. Output: <out dir>/<name>.tsv
set -uo pipefail
cd "$(dirname "$0")/.."
OUT=$1; CPU=$2; shift 2
mkdir -p "$OUT"
[ -f target/bench-cases/manifest.tsv ] || node bench/gen_cases.mjs >/dev/null
for nb in "$@"; do
  name=${nb%%=*}; bin=${nb#*=}
  case $name in
    n-*) taskset -c "$CPU" "$bin" target/bench-cases --min-ms 200 > "$OUT/$name.tsv" ;;
    w-*) taskset -c "$CPU" node bench/bench_wasm.mjs "$bin" target/bench-cases --min-ms 200 > "$OUT/$name.tsv" ;;
    c-*) taskset -c "$CPU" python3 bench/bench_cli.py "$bin" 30 > "$OUT/$name.tsv" ;;
  esac
  echo "$name done $(uptime | sed 's/.*load/load/')" >&2
done
