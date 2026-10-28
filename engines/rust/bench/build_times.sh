#!/usr/bin/env bash
# Build times, Rust and Zig: clean (fresh target/cache dir; toolchain std and Zig's global cache warm) and incremental
# (one constant in geom changed, then changed back: the mean of the two rebuilds). Usage: bench/build_times.sh <scratch dir> [rust|zig|all]
# Zig incremental edits a copy of engines/zig under <scratch dir>, never the tree.
set -uo pipefail
cd "$(dirname "$0")/.."
S=$1; what=${2:-all}
RUST=$PWD; ZIG=${ZIG:-$HOME/tools/zig-aarch64-linux-0.17.0/zig}
export PATH=$HOME/.cache/trunk/wasm-opt-version_123/bin:$PATH
t() { local s=$(date +%s.%N); "$@" >/dev/null 2>&1 || echo "FAILED: $*" >&2; python3 -c "print('%.1f' % ($(date +%s.%N)-$s))"; }
flip() { sed -i "s/$2/$3/" "$1"; }
row() { echo -e "$1\t$2\t$3"; }
if [ "$what" != zig ]; then
  G=kerf-core/src/geom.rs; A='MAX_ARC_STEPS: usize = 4096'; B='MAX_ARC_STEPS: usize = 4097'
  for cfg in "dev|cargo build" "check|cargo check" "release|cargo build --release" "small|cargo build --profile small" \
             "wasm|cargo build --profile small -p kerf-wasm --target wasm32-unknown-unknown"; do
    name=${cfg%%|*}; cmd=${cfg#*|}; T=$S/bt-rust-$name; rm -rf "$T"
    c=$(t env CARGO_TARGET_DIR=$T $cmd)
    flip $G "$A" "$B"; i1=$(t env CARGO_TARGET_DIR=$T $cmd); flip $G "$B" "$A"; i2=$(t env CARGO_TARGET_DIR=$T $cmd)
    row "rust $name" "$c" "$(python3 -c "print('%.1f' % (($i1+$i2)/2))")"
  done
  # the shipped wasm step after cargo: export pruning + wasm-opt -Oz
  T=$S/bt-rust-wasm; w=$(t sh -c "python3 tests/wasm_prune.py $T/wasm32-unknown-unknown/small/kerf_wasm.wasm $S/bt.wasm && wasm-opt -Oz --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --strip-debug --strip-producers $S/bt.wasm -o $S/bt.wasm")
  row "rust wasm post (prune + wasm-opt)" "$w" "$w"
fi
if [ "$what" != rust ]; then
  Z=$S/zigcopy; rm -rf "$Z"; mkdir -p "$Z/engines"; cp -r ../zig "$Z/engines/zig"; rm -rf "$Z/engines/zig/zig-out" "$Z/engines/zig/.zig-cache" "$Z/engines/zig/dist"
  ln -s "$RUST/../../spec" "$Z/spec"
  G=$Z/engines/zig/src/geom.zig; A='max_arc_steps: usize = 4096'; B='max_arc_steps: usize = 4097'
  for cfg in "debug|build" "fast|build -Doptimize=fast" "small|build -Doptimize=small" "wasm|build wasm -Dwasm-opt"; do
    name=${cfg%%|*}; args=${cfg#*|}; C=$S/bt-zig-$name; rm -rf "$C"
    c=$(cd $Z/engines/zig && t $ZIG $args --cache-dir $C --prefix $S/bt-zig-out-$name)
    flip $G "$A" "$B"; i1=$(cd $Z/engines/zig && t $ZIG $args --cache-dir $C --prefix $S/bt-zig-out-$name)
    flip $G "$B" "$A"; i2=$(cd $Z/engines/zig && t $ZIG $args --cache-dir $C --prefix $S/bt-zig-out-$name)
    row "zig $name" "$c" "$(python3 -c "print('%.1f' % (($i1+$i2)/2))")"
  done
fi
