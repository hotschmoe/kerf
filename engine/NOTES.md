# engine: the Kerf engine (Rust, zero crates)

The one engine of the repo: the SPEC 13 API as a library (`kerf-core`), the `kerf` CLI with `kerf serve` (`kerf-cli`) and
the raw-ABI wasm module the web app loads (`kerf-wasm`). Std only: `Cargo.lock` lists the three workspace packages and
nothing else. Rust 1.96 stable, edition 2024. It replaced the Zig engine on 2026-10-31 (feature parity: the goldens, the
Drawing IR on every view, the CLI ergonomics and the `kerf serve` smoke tests were identical; the Zig tree is in git
history). `README.md` is the short version of this file, `PERF.md` has the numbers.

## Layout
- `kerf-core`: `#![no_std]` + `alloc`, the whole engine. Public: `api` (the SPEC 13 dispatcher, `api::call(name, input)`),
  `json`, `canon`, `model`, `ops`, `schema`, `units`; every other module is private, so the compiler finds dead code.
  Pipeline: `load` -> `compile` (components -> `scene`) -> `section`/`iso` -> `drawview` + `annot` + `route` (notes,
  dimensions, leaders) -> `drawing` (the IR) -> exporters `svg`, `dxf`, `pdf`, `raster` + `png`; `mesh` for 3D;
  `validate`, `lint`, `coverage`, `crop`, `thinland` for the checks. `builders/` has one file per component type.
  `params!`/`param_enum!` (`params.rs`) declare a builder's parameters once: the typed struct, the parser (all problems
  in one pass) and the catalog rows. `build.rs` compiles `spec/fonts/kerf-simplex.json` into static glyph tables (it runs
  the engine's own `json.rs` through `#[path]`; coordinates must stay small integers). `math.rs` carries the float
  functions `core` lacks (ported from musl, identical bits on every target); `sort.rs` is the one stable sort.
- `kerf-cli`: binary `kerf`. `main.rs` (commands, options, typed `Fail`), `workspace` (op log, atomic writes, `DocLock`),
  `hash` (wyhash for ETags), `http`, `events`, `agents` (the agent templates and process control) and `serve` +
  `serve/*` (routes, connections, SSE, UI hashing). `build.rs` embeds the web UI named by `KERF_UI` (relative to the
  workspace root; empty means a page that says the UI is missing).
- `kerf-wasm`: cdylib, the SPEC 13.1 ABI, 120 lines plus a 90-line size-class allocator (`heap.rs`); a panic traps.
- `tests/`: `docs/` + `golden/` (see Gates), the Node/Python/bash gates, `tests/gates.sh` runs them all. `bench/`:
  the benchmark and profiling scripts (`PERF.md`). `../tools/build_wasm.mjs` builds `dist/kerf.wasm`.

## Build, run, test
```sh
cd engine
cargo build --release                      # target/release/kerf
CARGO_TARGET_DIR=target/ui KERF_UI=../apps/web/dist-serve cargo build --release   # with the web UI (npm run build:serve first)
node ../tools/build_wasm.mjs               # dist/kerf.wasm (profile small, export section pruned to the ABI, wasm-opt -Oz if on PATH)
cargo build --profile fastbuild            # optimized, no LTO, incremental: ~4 s after an edit, runs ~1.02x release
PATH=<repo>/tools/.venv/bin:$PATH tests/gates.sh [fuzz cases] [--serve]
```
`rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`. `cargo clippy --all-targets -- -D warnings` is clean;
the few `allow`s left are a long-argument list or two with no better shape, and the libm constants in `math.rs`.

## Gates (all green on this tree)
| gate | what it proves |
|---|---|
| `cargo build --release`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` | builds, lints, formatting |
| `tests/check_golden.sh` | `tests/make_golden.sh` regenerates the drawing JSON, SVG (bare and sheet), DXF, PDF, PNG (bare and sheet), mesh and check summary of every view of the 5 documents (`../spec/details/*`, `tests/docs/*`) into a temp tree; every file is byte-identical to `tests/golden`, every summary has 0 errors, every DXF passes `dxf_check.py` (ezdxf audit), every PDF is vector (`pdf_check.py`) |
| `tests/wasm_golden.mjs`, `tests/wasm_check.mjs` | the wasm output equals the native goldens byte for byte (24 files); the module has 0 imports and exactly the six ABI exports |
| `tests/cli_ergonomics.mjs` | 145 invocations of the CLI (stdout, stderr, exit code, files): what an agent sees |
| `tests/fuzz.py N seed` | mutation fuzz of the reference documents (hostile numbers, styles, ops) through check/drawing/export: no crash, no hang. Run it on a debug build too: overflow checks, debug assertions |
| `tests/serve_smoke.mjs` | 190 checks of `kerf serve` on temp folders: routes, tokens, hostile HTTP, SSE, the agent bridge with `fake-agent.mjs`/`fake-grok.mjs`, op log, symlinks, limits. `tests/windows_serve.mjs` is the field-shaped session written for Windows (it runs everywhere) |
| `../apps/web`: `typecheck`, `test:unit`, `test:e2e`, `test:serve [-- --real]`, `build` | the UI against this engine |

Golden PNGs for human review (`A.png`, `A-dxf.png`, `A-pdf.png` per view) come from `tests/make_golden.sh --png`; look at
them after a visual change (`tools/svg2png.sh`, `tools/dxf_check.py --png`, `tools/pdf_check.py --png`), a drawing that
passes the byte gate can still look wrong. `tools/compare_drawings.py a.json b.json` diffs two `kerf drawing` outputs
geometrically (tolerance 1e-3).

## Rules that hold for every input (document, style, ops)
- No raw float-to-int casts on untrusted data: `num::to_int*`. Numbers are validated where they enter: `json::parse`
  rejects non-finite and > 9e15 numbers, invalid UTF-8, leading zeros and nesting over 200; `units::parse_*` is the one
  float parser for strings (no `nan`/`inf`); lengths are limited to +-`limits::MAX_COORD_IN`, scales to 0.1..10000,
  polygon bulges to 0 or 1e-6..1e3; the style is validated once in `style::load` (E_STYLE lists every bad key with its
  range and default).
- `limits.rs` holds the global caps (input bytes, components, instances, annotations per view, views, points, note
  characters), each reported as `E_LIMIT` with found / maximum / what to do.
- Layout effort is bounded: `route::Work` is shared by every pass and trial of one view build; when it is spent the notes
  keep the basic layout and one aggregate `W_LEADER_HIT` says so.
- No silent defaults: a malformed offset, `place`, `array`, `recess` or `cover` is `E_PARAM` with a fix hint.
- Errors are values: no `unwrap`/`expect`/`panic!`/panicking index on a document-reachable path. Debug and the native
  release profile have overflow checks, so an arithmetic bug is a panic, which the CLI reports as
  `kerf: internal error: ... at file:line:col` with exit 70 (a hook; `panic = "abort"` would otherwise end in SIGABRT).
- Determinism is a feature: no HashMap iteration order, timestamps or randomness in outputs; float math only through
  `crate::math`; sorting only through `sort::stable` (never panics, same result on every target); numbers are printed by
  `json::fmt_number`/`fmt_fixed` (integer arithmetic, no `{}` of an `f64`, except two message paths that need the
  shortest round-trip digits). The version is never in an export: the PDF `Producer` is `(kerf)`.
- Text: the CLI folds its text output to ASCII for Windows consoles (`ascii_fold`); the stroke font folds dashes, quotes,
  the x-sign and fractions and replaces anything else with `?` plus `I_GLYPH`.

## Decisions, with measurements
- **`no_std` core.** With std (default allocator + panic machinery) the wasm was 648,690 bytes after `wasm-opt -Oz`;
  `no_std` + the 90-line allocator + trap-on-panic was 604,251 (-6.9%). It also forces every float function through
  `math.rs`.
- **Wasm profile `small` = `opt-level = "z"`:** 604 KB vs 717 KB with `s`. `panic = "abort"`, LTO, 1 codegen unit.
  The wasm has overflow checks off (measured: +14% time, +1% size on the 126-case set); native release has them on
  (+4% time, png +42% before the filter rewrite, +0.7% size).
- **Wasm sqrt:** `math::sqrt` calls the libm symbol `sqrt`, which compiler-builtins defines on wasm32-unknown-unknown and
  LLVM lowers to `f64.sqrt` (0 imports); the software version it replaced was 40% of the wasm time.
- **rustc exports `__heap_base` and `__data_end`** from a cdylib, which the exact-ABI check rejects; `tools/build_wasm.mjs`
  rewrites the export section (40 lines of Node) instead of a linker shim.
- **PNG** is a gray8 encoder: per-row adaptive filters picked by minimum sum of absolute residuals (earliest filter on
  ties), a deflate at zlib level-6 parameters through one 64 KiB window, zlib framing, IDAT chunks of 64 KiB. The exact
  token choices are part of the golden bytes. `px` is the output width (default 1600, clamped 200-6000; a page taller
  than 8192 reduces the scale instead). Pen widths are true mm at the chosen scale, minimum 1 px.
- **Unsafe** (each with a SAFETY note): `math.rs` 4 (NEON/SSE2 sqrt, wasm libm `sqrt`), `style.rs` 4 (the default style
  tree parsed once per process behind an `AtomicPtr`), `kerf-wasm` (allocator, ABI pointers), `kerf-cli` (`signal`/`kill`
  declared through `extern "C"`, no `libc` crate).

## Behavior worth knowing (decisions that are not in the SPEC text)
- Note wrap: `wrap_chars` counts characters (greedy, words longer than a line split); text widths come from the stroke
  font. `lumber.plies` stack along the thickness direction. Canonical `fmt` key order: common fields first (`id type label
  material at rotate slope mirror z array embedded visible`), then type params in catalog order.
- Arc flattening for iso and mesh uses the finer of 0.22 rad and 0.004" sagitta. Iso uses the isometric drawing
  projection (axes at true length). Section X marks and ply lines use the `beyond` pen. Vapor retarder lines stay at least
  0.03 paper inch off the host edge. `src` of array instances is `id#k`.
- Warnings: `W_NEAR_MISS` looks at axis-aligned `lumber` only (panels are excluded: the reference details leave small gaps
  there); `W_FLOATING` / `W_UNTREATED_CONTACT` use 3D proximity (plan distance and z gap both <= 1/32"); `W_COVER` host is
  the concrete/cmu outline containing the bar.
- Lengths print as `3'-0 1/4"` (feet with zero inches keep the `0`). Keynote mode (`notes.mode: "keynote"`) puts a hexagonal
  tag with the note number at the leader and the texts in a `KEYNOTES` legend.
- Exports: `format` is `svg|dxf|pdf|png`; PDF frames the drawing on the sheet by default, SVG/DXF/PNG do not (`sheet`).
  Materials accept an optional `role`; a material `pen` must be an engine pen.

## kerf serve (spec/SERVE.md)
- The first stdout line is `kerf serve: http://127.0.0.1:<port>/?token=<32 hex>  (dir <abs>)` (scripts parse it; `--port 0`
  picks a free port). A token is the default, also on loopback; `--token-file F` / `KERF_TOKEN` keep it out of `ps`;
  `--no-token` opts out (the Host/Origin checks then still apply on loopback). Exit 1 with a hint when the port is busy.
- One thread per connection; SSE streams never block requests. Keep-alive with idle (30 s), head (10 s), body and write
  deadlines enforced by a watchdog; at most 256 connections (503 beyond), 32 SSE streams, 1000 requests per connection;
  per-route body caps (stop 4 KiB, create 64 KiB, apply 16 MiB, agent/run 64 MiB, GET routes 0).
- A 500 ms poller scans the folder: per `*.kerf.json` (mtime, size), per `<file>.log.jsonl` the byte offset reported; it
  emits `doc_added/doc_changed/doc_removed` and one `log` event per complete line. Designer writes update the scan state
  themselves so nothing is reported twice. `apply` serializes writers, re-reads the document, checks `if_match` (quoted
  or bare), calls the engine, writes atomically (temp + rename, fsync) and appends the log. Invalid ops are HTTP 200 with
  `ok:false` and the engine's diagnostics, nothing written. ETag = `"<mtime_ms>-<size>-<wyhash of the bytes>"`.
- Security: no CORS unless `--allow-origin`; any `/api` request whose `Origin` is not the server's gets 403; constant-time
  token compare (`Authorization: Bearer` or `?token=`); CSP, `X-Frame-Options`, `Referrer-Policy` on every response (the UI
  gets a CSP with the SHA-256 of each inline script of the embedded `index.html`); file names are `[A-Za-z0-9._ ()-]+.kerf.json`
  and Windows device names are refused; symlinks are never served, listed or written through; the server makes no
  outbound connection and holds no API key. Strict request parsing (bare CR/LF, duplicate or conflicting
  `Content-Length`/`Transfer-Encoding`/`Host`, obs-fold, over 100 headers, chunk sizes over 8 hex digits are 400).
- Agent bridge: templates for claude, grok, codex and pi (`agents.rs`) plus `<dir>/.kerf/agents.json`, which is executed
  only when trusted (`--trust-agents` or a yes at the prompt) and otherwise listed `untrusted`. Placeholders `{message}
  {session_id} {dir} {file}` and a `{resume}` element. cwd = `--dir`, stdin ignored, stdout/stderr piped, the server
  binary's directory prepended to PATH, `KERF_ACTOR=agent`, `KERF_TOKEN` removed. stdout lines become SSE `agent` events (JSON lines
  verbatim, others `{type:"text"}`); `session_id`/`sessionId`/`thread_id` is captured for resume; at the end the folder is scanned once
  so `doc_changed`/`log` precede `exit`. One run at a time (409 `E_BUSY`); stop/timeout/shutdown signal the whole process
  group, then kill after 3 s; `--agent-timeout` defaults to 1800 s. Claude gets only `Bash(kerf)`, Read, Write and Edit
  pre-approved plus `acceptEdits`; Grok runs `--always-approve`; Codex runs the `workspace-write` sandbox.
- Op log: `<file>.log.jsonl`, one object per line `{ts, who, tool, why, ops, changed, summary_head}`; the CLI's `apply -w`
  and `new` write it (who = `$KERF_ACTOR` or `agent`), the server's `who` is the request's `actor`. Timestamps exist only
  in the log. `-w` takes the same `DocLock` as the server, writes atomically, and `--if-match` refuses a stale document.

## Windows

All `unsafe` lives in `kerf-cli/src/sys/` (`unix.rs`, `windows.rs`: safe wrappers, `#![deny(unsafe_op_in_unsafe_fn)]`, a `// SAFETY:` per block); zero crates. Built and run on a Windows 11 x86_64 machine (2026-10-31)
(release and debug): `serve_smoke.mjs` 189/189 (190 on Linux: one POSIX-only check), `windows_serve.mjs` 32/32, `cli_ergonomics.mjs` 145/145, `tests/platform_serve.mjs` 16/16,
`tests/ctrl_break.ps1`, `tests/check_golden.mjs` (101 outputs byte-identical to the Linux goldens, DXF/PDF checkers clean), clippy `-D warnings` clean. aarch64-pc-windows-msvc only type-checks here (CI: `windows-11-arm`).
- **Agent runs**: the agent is created suspended (`CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP`), assigned to its own job object with `KILL_ON_JOB_CLOSE`, then resumed (`NtResumeProcess`). Stop, timeout, sweep and
  server exit are `TerminateJobObject` (exit code 1, no SIGTERM stage); a server killed hard closes the job and takes the tree with it. If the job cannot be made the leader alone is terminated (`TerminateProcess`).
- **Agent CLIs**: std only finds `.exe`, so `sys::resolve_program` walks `PATH` (the agent's PATH has kerf's directory first) with the runnable `PATHEXT` extensions (`.exe .com .cmd .bat`): npm `.cmd` shims work, a
  `.ps1` or extensionless script is "not found". A `.cmd` goes through cmd.exe, which refuses CR/LF in an argument (`InvalidInput`): the spawn is retried once with line breaks flattened to spaces. A command line
  over 32 767 characters (`ERROR_FILENAME_EXCED_RANGE`) is the 400 `E_INPUT` as E2BIG is on Linux.
- **Token**: `ProcessPrng` (bcryptprimitives, `raw-dylib`). **Shutdown**: `SetConsoleCtrlHandler`: Ctrl-C/Ctrl-Break = 2, console close = 1, logoff/shutdown = 15, stored in the same atomic the poller reads (exit `128 + n`,
  so 130 for Ctrl-C); the close/logoff handlers block 10 s so the poller can stop the agent before the OS ends the process. The agent has its own process group, so the console's Ctrl-C reaches only the server.
- **Op log**: `LockFileEx` on one byte at offset 2^62 (Windows range locks are mandatory; `File::lock` would cover byte 0 on and block readers), as the Zig engine; a file system without locks is tolerated.
  Directory fsync does not exist (the open fails and is ignored), file data is `FlushFileBuffers`.
- **Paths**: `canonicalize` returns `\\?\C:\...`; `sys::plain_path` strips it (banner, `/api/info` `dir`, the agent's `cwd` and `{dir}`). Document names are validated by `workspace::valid_doc_file` (device names, stems
  ending in dot/space, `:` streams, separators). Links are never followed (`symlink_metadata` reports symlinks and junctions): verified with a file symlink and a junction at `.kerf/attachments`.
- **Not covered**: console-close events (the handler is the same code as Ctrl-Break), a `windows-11-arm` run, a network share for the op log.

Why (the Zig engine's field report): `kerf serve` on Windows 11 died right after the banner in the first Zig releases
because it had only been cross-compiled, never run. Requirements kept from that: open documents without following
symlinks, a byte-range op-log lock far past EOF, `.cmd` shims through `cmd.exe` with the CR/LF retry, `.ps1`-only installs
unavailable, tree kill by job object, `.gitattributes` forcing LF (CRLF in embedded fixtures broke tests), the 400 for
over-long command lines. CI runs the Windows gates against the shipped build on real Windows runners.

## Known gaps
- `solid` / `polygon` profiles with holes are not supported (profiles are single loops; the mesh ignores holes).
- Hatch under dimension/label text is knocked out in SVG/PDF only (the DXF HATCH uses the pattern, not lines).
- PDF content streams are uncompressed (about 150 KB per sheet).
- Allocation churn: glibc malloc, not a per-call arena (the measured bound is 0.77x in-process; a safe arena for the
  threaded server is real allocator work). Process start-up is dynamic glibc + libgcc_s (0.9 ms). See `PERF.md`.
- The Zig test suite's unit tests (hostile-input table, OOM sweep, layout and view tests) were not ported: tests here are
  end to end (goldens, `cli_ergonomics`, fuzz, `serve_smoke`), by decision.

## SPEC ISSUES
- SPEC 13 says `kerf export` takes `format: "svg"|"dxf"|"pdf"`; the engine also accepts `png`, and PDF export frames the
  drawing on a sheet by default while SVG/DXF/PNG do not. Worth one sentence in 13/13.2.
- SPEC 12 `3'-0 1/4"` versus `3'-1/4"` for feet with zero whole inches and a fraction: the engine prints the former.
