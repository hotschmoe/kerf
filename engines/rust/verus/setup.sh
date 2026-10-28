#!/usr/bin/env bash
# Reproduces the Verus toolchain used by engines/rust/kerf-verified on a machine that has only
# rustup, git, curl, python3, make and Zig 0.17 (Kerf's own toolchain); no cmake, no Z3, no Verus.
#
# Why from source: the official arm64-linux release zip (first published 0.2026.10.26) links
# against glibc 2.39 and its bundled z3 against glibc 2.38 / GLIBCXX_3.4.32; Debian 12 ships
# glibc 2.36, so both binaries fail to load. Z3's own arm64 release is also glibc-2.38.
#
# Every step appends "<step> <seconds> <bytes added>" to $LOG.
set -euo pipefail

VERUS_TAG=${VERUS_TAG:-release/0.2026.10.26.d17fbd0}
Z3_VER=${Z3_VER:-4.16.0}
TOOLS=${TOOLS:-$HOME/tools}
VERUS_DIR=${VERUS_DIR:-$TOOLS/verus}
Z3_DIR=${Z3_DIR:-$TOOLS/z3-$Z3_VER}
JOBS=${JOBS:-$(nproc)}
ZIG=${ZIG:-$TOOLS/zig-aarch64-linux-0.17.0/zig}
LOG=${LOG:-$TOOLS/verus-setup.log}

mkdir -p "$TOOLS"
size_of() { { du -sb "$@" 2>/dev/null || true; } | awk '{s+=$1} END {printf "%.0f", s}'; }
step() {
    local name=$1; shift
    local before after t0 t1
    before=$(size_of "$VERUS_DIR" "$Z3_DIR" "$HOME/.rustup/toolchains" "$HOME/.cargo/registry")
    t0=$(date +%s.%N)
    "$@"
    t1=$(date +%s.%N)
    after=$(size_of "$VERUS_DIR" "$Z3_DIR" "$HOME/.rustup/toolchains" "$HOME/.cargo/registry")
    printf '%-22s %8.1f s %8.1f MiB\n' "$name" "$(echo "$t1 - $t0" | bc)" "$(echo "($after - $before) / 1048576" | bc -l)" | tee -a "$LOG"
}

clone_verus() {
    [ -d "$VERUS_DIR/.git" ] && return
    git clone -q --depth 1 --branch "$VERUS_TAG" https://github.com/verus-lang/verus.git "$VERUS_DIR"
}

install_toolchain() {
    cd "$VERUS_DIR/source"
    rustup toolchain install --profile minimal --no-self-update
}

build_z3() {
    [ -x "$Z3_DIR/bin/z3" ] && return
    local src="$Z3_DIR/src"
    mkdir -p "$src"
    curl -fsSL "https://github.com/Z3Prover/z3/archive/refs/tags/z3-$Z3_VER.tar.gz" | tar -xz -C "$src" --strip-components=1
    cd "$src"
    # Z3 4.16 uses C++20 <format>: GCC 12 (Debian 12) lacks it, so compile with Zig's clang + libc++,
    # targeting this machine's glibc. The result links libc++ statically.
    local glibc
    glibc=$(ldd --version 2>&1 | awk 'NR==1 {print $NF}')
    printf '#!/bin/sh\nexec "%s" c++ -target %s-linux-gnu.%s "$@"\n' "$ZIG" "$(uname -m)" "$glibc" > "$Z3_DIR/cxx"
    printf '#!/bin/sh\nexec "%s" cc -target %s-linux-gnu.%s "$@"\n' "$ZIG" "$(uname -m)" "$glibc" > "$Z3_DIR/cc"
    chmod +x "$Z3_DIR/cxx" "$Z3_DIR/cc"
    CXX="$Z3_DIR/cxx" CC="$Z3_DIR/cc" python3 scripts/mk_make.py --prefix="$Z3_DIR" >/dev/null
    make -C build -j"$JOBS" >/dev/null
    mkdir -p "$Z3_DIR/bin"
    strip -o "$Z3_DIR/bin/z3" build/z3
    rm -rf "$src"
}

build_verus() {
    # vargo (Verus's cargo wrapper) builds rust_verify, the builtin crates and the verified vstd
    # into target-verus/release, the layout the standalone `verus` binary loads.
    cd "$VERUS_DIR/source"
    ln -sf "$Z3_DIR/bin/z3" z3
    (
        cd ../tools/vargo && cargo build --release -q
    )
    VERUS_Z3_PATH="$Z3_DIR/bin/z3" ../tools/vargo/target/release/vargo build --release
}

verify_examples() {
    cd "$VERUS_DIR/source"
    local v=./target-verus/release/verus
    for f in ../examples/vectors.rs ../examples/bitvector_basic.rs ../examples/mergesort.rs ../examples/rw2022_script.rs; do
        [ -f "$f" ] || continue
        echo "== $f"
        VERUS_Z3_PATH="$Z3_DIR/bin/z3" "$v" "$f" 2>&1 | tail -1
    done
}

echo "# $(date -Is) VERUS_TAG=$VERUS_TAG Z3=$Z3_VER" | tee -a "$LOG"
step clone-verus       clone_verus
step rust-toolchain    install_toolchain
step z3-from-source    build_z3
step verus-vargo-build build_verus
step examples          verify_examples
"$Z3_DIR/bin/z3" --version
echo "verus: $VERUS_DIR/source/target-verus/release/verus (export VERUS_Z3_PATH=$Z3_DIR/bin/z3)"
