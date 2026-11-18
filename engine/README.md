# engine

The Kerf engine in Rust, std only (no third-party crates). It is the library behind the `kerf` CLI, `kerf serve` and the
browser's `kerf.wasm`. Rust (stable pinned in `rust-toolchain.toml`, MSRV 1.96), edition 2024. Contract: `../spec/SPEC.md`.

| crate | what |
|---|---|
| `kerf-core` | `no_std` + `alloc`: parse, compile, draw, annotate, export (SVG, DXF, PDF, PNG), mesh, validate. `api::call(name, json)` is the whole surface |
| `kerf-cli` | the `kerf` binary: commands, op log, `kerf serve` (HTTP, SSE, agent bridge), embedded web UI |
| `kerf-wasm` | the raw wasm ABI of SPEC 13.1 (`kerf_alloc`, `kerf_call`, ..., no imports) |

```
engine/
  Cargo.toml, Cargo.lock, rustfmt.toml
  kerf-core/  kerf-cli/  kerf-wasm/
  tests/   golden set + docs, the gates (golden, wasm, CLI, fuzz, serve), gates.sh
  bench/   benchmark and profiling scripts
  NOTES.md (state, rules, decisions)   PERF.md (measurements)
```

## Build
```sh
cargo build --release                  # target/release/kerf
node ../tools/build_wasm.mjs           # dist/kerf.wasm (wasm-opt is used when it is on PATH)
CARGO_TARGET_DIR=target/ui KERF_UI=../apps/web/dist-serve cargo build --release   # CLI with the web UI embedded
```

Rustup installs the toolchain pinned in `rust-toolchain.toml` on first use. Release binaries are built by `release.yml`
(`tools/release.sh <version>` starts one); `CARGO_TARGET_DIR`, `KERF_UI` and `RUSTFLAGS="-C target-feature=+crt-static"`
(Windows) reproduce what it builds.

## Test

The [engineering conventions](https://github.com/hotschmoe/hotschmoe-setup/blob/master/ENGINEERING.md) define the testing policy. Install the Python tooling with `../tools/setup.sh` and web dependencies with `cd ../apps/web && npm ci` before running the gates.

```sh
cd engine
tests/gates.sh       # blocking: build, representative exports, CLI, serve, wasm, browser
tests/test-heavy.sh # background: full corpus, fuzz and broader scenarios
cargo fmt --check
```

The blocking export comparison covers three reference details. For a visual change, render PNGs with `tools/dxf_check.py --png`, `tools/pdf_check.py --png` or `tools/shot.mjs` and inspect them as a review practice.
