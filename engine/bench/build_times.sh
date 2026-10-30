#!/usr/bin/env bash
# Build times: clean (fresh target dir; the toolchain's std warm) and incremental (one constant in geom changed, then changed
# back: the mean of the two rebuilds). Usage: bench/build_times.sh <scratch dir>   -> TSV: build, clean s, incremental s
set -uo pipefail
cd "$(dirname "$0")/.."
S=$1
export PATH=$HOME/.cache/trunk/wasm-opt-version_123/bin:$PATH
t() { local s=$(date +%s.%N); "$@" >/dev/null 2>&1 || echo "FAILED: $*" >&2; python3 -c "print('%.1f' % ($(date +%s.%N)-$s))"; }
flip() { sed -i "s/$2/$3/" "$1"; }
G=kerf-core/src/geom.rs; A='MAX_ARC_STEPS: usize = 4096'; B='MAX_ARC_STEPS: usize = 4097'
for cfg in "dev|cargo build" "check|cargo check" "release|cargo build --release" "fastbuild|cargo build --profile fastbuild" \
           "small|cargo build --profile small" "wasm|cargo build --profile small -p kerf-wasm --target wasm32-unknown-unknown"; do
  name=${cfg%%|*}; cmd=${cfg#*|}; T=$S/bt-$name; rm -rf "$T"
  c=$(t env CARGO_TARGET_DIR=$T $cmd)
  flip $G "$A" "$B"; i1=$(t env CARGO_TARGET_DIR=$T $cmd); flip $G "$B" "$A"; i2=$(t env CARGO_TARGET_DIR=$T $cmd)
  echo -e "$name\t$c\t$(python3 -c "print('%.1f' % (($i1+$i2)/2))")"
done
w=$(t env CARGO_TARGET_DIR=$S/bt-wasm node ../tools/build_wasm.mjs --out "$S/bt.wasm")
echo -e "wasm post (prune + wasm-opt)\t$w\t$w"
