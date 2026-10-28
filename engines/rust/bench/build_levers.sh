#!/usr/bin/env bash
# Build-time levers for the dev loop and release: clean + incremental (geom constant edit, mean of edit and revert).
# Usage: bench/build_levers.sh <scratch dir>
set -uo pipefail
cd "$(dirname "$0")/.."
S=$1
SYS=$(rustc --print sysroot); LLD="-C link-arg=-B$SYS/lib/rustlib/aarch64-unknown-linux-gnu/bin/gcc-ld -C link-arg=-fuse-ld=lld"
G=kerf-core/src/geom.rs; A='MAX_ARC_STEPS: usize = 4096'; B='MAX_ARC_STEPS: usize = 4097'
t() { local s=$(date +%s.%N); "$@" >/dev/null 2>&1 || echo "FAILED: $*" >&2; python3 -c "print('%.2f' % ($(date +%s.%N)-$s))"; }
measure() { # name, env..., -- cargo args
  local name=$1; shift; local T=$S/bl-$name; rm -rf "$T"
  local c=$(t env CARGO_TARGET_DIR=$T "$@")
  sed -i "s/$A/$B/" $G; local i1=$(t env CARGO_TARGET_DIR=$T "$@"); sed -i "s/$B/$A/" $G; local i2=$(t env CARGO_TARGET_DIR=$T "$@")
  echo -e "$name\t$c\t$(python3 -c "print('%.2f' % (($i1+$i2)/2))")"
}
if [ -z "${NIGHTLY_ONLY:-}" ]; then
measure dev cargo build
measure dev-lld RUSTFLAGS="$LLD" cargo build
measure dev-lines CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo build
measure dev-nodebug CARGO_PROFILE_DEV_DEBUG=0 cargo build
measure dev-lines-lld CARGO_PROFILE_DEV_DEBUG=line-tables-only RUSTFLAGS="$LLD" cargo build
measure release-thin CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release
measure release-nolto CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_INCREMENTAL=true cargo build --release
measure release-lld RUSTFLAGS="$LLD" cargo build --release
fi
if [ -n "${NIGHTLY:-}" ]; then
  measure dev-nightly cargo +nightly build
  measure dev-threads8 RUSTFLAGS="-Zthreads=8" cargo +nightly build
  measure dev-cranelift CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift cargo +nightly build -Zcodegen-backend
fi
