# engine

The Kerf engine in Rust, std only (no third-party crates). It is the library behind the `kerf` CLI, `kerf serve` and the
browser's `kerf.wasm`. Rust 1.96 stable, edition 2024. Contract: `../spec/SPEC.md`.

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

## Test
```sh
PATH=../tools/.venv/bin:$PATH tests/gates.sh 1000 --serve    # build, golden, wasm, CLI, fuzz (1000 cases), serve
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```
The gates: `tests/check_golden.sh` (every export of every view byte-identical to `tests/golden`, DXF and PDF audits),
`tests/wasm_golden.mjs` + `tests/wasm_check.mjs`, `tests/cli_ergonomics.mjs`, `tests/fuzz.py`, `tests/serve_smoke.mjs`.
After an intended output change: `tests/make_golden.sh`, review the diff (and the PNGs with `--png`), commit it with the
change. `NOTES.md` explains each gate and the rules the engine keeps; `PERF.md` how to benchmark.
