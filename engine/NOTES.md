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
| `tests/check_golden.sh` | `tests/make_golden.sh` regenerates the drawing JSON, SVG (bare and sheet), DXF, PDF, PNG (bare and sheet), mesh and check summary of every view of the 14 documents (`../spec/details/*`, `tests/docs/*`) into a temp tree; every file is byte-identical to `tests/golden`, every summary has 0 errors, every DXF passes `dxf_check.py` (ezdxf audit), every PDF is vector (`pdf_check.py`) |
| `tests/wasm_golden.mjs`, `tests/wasm_check.mjs` | the wasm output equals the native goldens byte for byte (46 files); the module has 0 imports and exactly the six ABI exports |
| `tests/cli_ergonomics.mjs` | 345 invocations of the CLI (stdout, stderr, exit code, files): what an agent sees |
| `tests/fuzz.py N seed` | mutation fuzz of the reference documents (hostile numbers, styles, ops) through check/drawing/export: no crash, no hang. Run it on a debug build too: overflow checks, debug assertions |
| `tests/serve_smoke.mjs` | 224 checks of `kerf serve` on temp folders: routes, tokens, hostile HTTP, SSE, the agent bridge with `fake-agent.mjs`/`fake-grok.mjs`, op log, symlinks, limits, reference files. `tests/windows_serve.mjs` is the field-shaped session written for Windows (it runs everywhere) |
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
- Per-view geometry checks (issue #5, SPEC 9): `validate::run` groups the views by the components they draw (compiled,
  `visible`, not omitted) and runs overlap, floating, untreated, cover, near-miss and short-slope once per group;
  `each_set` merges identical diagnostics and appends ` (in view(s) ...)` only when a finding misses some views, so
  documents without `omit` print exactly what they printed before (goldens unchanged). On the 36 MASS details that use
  `omit`, with their W_OVERLAP/W_NEAR_MISS acknowledgements stripped, warnings went 628 -> 267 (W_OVERLAP 573 -> 200,
  W_NEAR_MISS 39 -> 50, W_FLOATING 0 -> 1); what is left is within one scenario (100 are S-50.07's 0.01-0.02 sq in grid
  overlaps), plus near-misses that another scenario's lumber used to hide and two real misses the global check could
  not see (S-60.03 view D: the parallel joist floats; S-40.02 view B: bottom chord 3" short of the ledger). Since #19 any
  drawn prism in the gap blocks a near miss (subfloor, hanger seat, another array instance).
- Scenario membership (issue #8, SPEC 25.4): `view::Members` (component `views`, view `only` / `omit`, `*` globs via
  `view::glob_match`) is the one test of "does view V draw component C"; `compile::view_prisms`, iso `gather`, note
  landings, `validate::drawn_sets`, `crop` (W_CROP_STALE) and `inspect at` all use it (`inspect at` used every visible
  prism before). The checks of the lists themselves are document lints (`lint::members`): unknown view in `views` is
  E_REF_UNKNOWN, an `omit`/`only` entry matching nothing is W_REF_UNKNOWN (was a per-build E_REF_UNKNOWN in drawview).
  S-80.01 rewritten at one origin (scenario members `views`, no `omit`): same check output, view A PNG byte-identical,
  the others differ only in hatch phase (patterns are anchored to the model origin).
- Acknowledgements (issue #8, SPEC 25.4): `lint::apply_acknowledge(doc, diags, AckMode {strict, complete})` returns
  the suppressed warnings (`Acked`) to `Loaded.acked`; the summary prints the count on the DOC line and an ACKNOWLEDGED
  block instead of per-warning I_ACK lines (I_ACK stays in the JSON diagnostics); W_ACK_UNUSED needs `complete`
  (`check`/`apply` with views built and no errors). The drawing IR gets `acknowledged` from `load::view_acked`
  (compile + validate + lint + crop again, plus the view's own diagnostics) only when the document has `acknowledge`:
  +3..7 ms per `drawing` call on the MASS details that have acks (s-70-03 18 -> 21 ms). `review.rs` draws the export
  overlay; on a sheet it scans the frame for a free spot against every drawn item's box (0.1" steps).
  Wasm (wasm-opt -Oz): 687,182 -> 708,082 raw (+20.9 KB), 304,169 -> 313,054 gzip.
  On the 47 MASS details: 1,278 acknowledge entries, 291 warnings suppressed (by 250 entries), 1,028 entries stale
  (W_ACK_UNUSED, on 490 components: W_FLOATING 398, W_NEAR_MISS 253, W_OVERLAP 211, W_SHORT_SLOPE 74,
  W_UNTREATED_CONTACT 60, W_CROP_STALE 32); `--strict` turns the 18 visible warnings into 309 (+291: W_OVERLAP 208,
  W_NEAR_MISS 63, W_FLOATING 13, W_UNTREATED_CONTACT 4, W_SHORT_SLOPE 2, W_CROP_STALE 1).
- Papercuts (#19, SPEC 25.5): on the 47 MASS details with every `acknowledge` stripped, W_NEAR_MISS 64 -> 15 (41 gaps
  filled by a subfloor, hanger, cap plate, tie or the other ply of a double plate; 1 side-by-side 1 3/4" framing space;
  the rest were union boxes of arrays); W_FLOATING 14 -> 14; new W_BOLT_EDGE 2 (S-80.01 D rods 1 1/2" from the ICF core
  face, the case claude7 asked for), W_BOLT_SPACING 1 (S-80.03's two faked weld studs 3/16" apart) and W_PARAM 1
  (S-80.03 E: a weld with one member); every other code unchanged. Wrap changes moved text in 58 of 198 views (values
  kept together) and title blocks of long titles now take two
  lines; goldens regenerated for both. `kerf version` gets `build` from `kerf-cli/build.rs` (`git describe`), so the core
  and the wasm stay byte-identical.
- Lengths print as `3'-0 1/4"` (feet with zero inches keep the `0`). Keyed notes (SPEC 24, view `notes_style` or style
  `notes.mode`) route the number tags through the same column layout as leader text, then place the DETAIL NOTES list
  outside everything else (`list_items`), so leader output is unchanged by the feature.
- Exports: `format` is `svg|dxf|pdf|png`; PDF frames the drawing on the sheet by default, SVG/DXF/PNG do not (`sheet`).
  Materials accept an optional `role`; a material `pen` must be an engine pen.
- Steel (SPEC 5.13, 5.14, weld in 6.2): `builders/steel_shapes.rs` is the AISC table and the designation parser,
  `builders/steel.rs` lays the (u, v) cross-section out per orient, `builders/bolt.rs` the bolt groups, `annot/weld.rs`
  the weld symbol, `steelcheck.rs` W_BOLT_EDGE / W_BOLT_SPACING / W_WELD_SIZE (called from `validate::run`). Two prism
  fields carry it through the pipeline: `zoff` (a slab's z center relative to the component's, applied in
  `compile::world_prisms`) and `lengthwise` (section: a member that reaches behind the cut is beyond, never cut; its slab
  edges on a nearer slab of the same member are dropped in `section/beyond.rs`; `section/breaks.rs` gives it break
  lines at the crop). Ghost prisms with `line_pts` draw open hidden lines (only with `hidden_lines: true`). Edge distance: side bolts sample the grip axis through the union of the ply's prisms that contain the
  bolt's z and cast rays perpendicular to the axis; z distance to the ply's z extent counts too. Wasm: +70 KB raw,
  +22 KB brotli (588,023 -> 658,168 raw; 224,965 -> 246,938 brotli); about 21 KB is strings and tables, the rest the
  builders, the parser and the checks (no float formatting was pulled in).
- Cut fill and rebar placement (issues #15, #16, SPEC 25.1). `clip::boolean` splits an edge pair at every endpoint
  within `SNAP` of the other edge (`split_pair`), so near-coincident edges (a bolt rotated 180: 5e-17 instead of 0) share
  vertices and classify as boundary instead of crossing at a stray point; before, a W under a rotated bolt lost all or
  part of its fill (a stress doc of 168 W12x26 + bolts at 15-degree steps on 7 anchors: 61 fills lost area before; after,
  all 168 keep 7.05-7.55 sq in of the 7.65 and none warns;
  goldens unchanged). `section/cut.rs` checks every hole subtraction against the hole's area and records a `FillFault`;
  `drawview::fill_fault_diags` turns them into `W_CUT_FILL`. `rebar::place_rebar` builds the zone from the host outline
  (`zone_region`), picks the face run (`face_run`) or the face under a station (`face_at`) and errors
  `E_PLACE_OUTSIDE` for a centre outside the zone; a rectangle reproduces the old box placement bit for bit.
  `validate::cover` collects unhosted bars for `W_BAR_OUTSIDE` (`bar_outside`, `path_outside`). Anchors:
  `builders/common.rs::vertex_anchors` (polygons and point lists), `rebar.rs::path_anchors`, cmu cell anchors and `cell`
  zone. On the 47 MASS details: warnings 152 -> 152, errors 0 -> 0, no fill area changed (S-80-03's weld studs had been
  faked as anchor bolts; with `bolt rotate: 180` its view A now draws the filled W).
- House text rules (issue #18, SPEC 25.3): `house.rs` holds the abbreviation table, the case-significant tokens (penny
  sizes, ACI symbols, style `text.keep_case`), the dashed-fraction test and the coverage word normalization; `lint.rs`
  (W_NOTE_STYLE), `annot/text.rs::upper_if` + `sheet.rs` (the case transform) and `coverage.rs` all read it. Coverage is
  computed once in `load_after_edit` (with the scene, so a dim without `text` offers its measured length) and kept on
  `Loaded.coverage`: the summary and W_REQUESTED_MISSING share it (resolving dim points inside `summary` would re-borrow
  the diagnostics list). Measured on the 47 MASS details (copies; the shipped files had already been reworded around
  the old rules, so they are 0/0 before and after; their warnings are unchanged, 18 -> 18): replaying each detail's op
  log, the first builds go W_NOTE_STYLE 43 -> 17 and W_REQUESTED_MISSING 58 -> 31, all 253 intermediate states 48 -> 17
  and 100 -> 55. Every W_NOTE_STYLE that went away is a 16d/8d, an ACI symbol or a `PER ARCH.`; the 17 left are real
  (spelled-out CONTINUOUS/EACH/BOTTOM/CONCRETE). The 27 coverage items that now match were each checked by hand (EA. =
  EACH, CONT. = continuous, dim text `4" TYP.`, view titles, the weld, `15 ft` = `15'-0"`, `ftg` = footing); the 31 left
  are true misses (not built yet, `(3) #4` vs `(2) #4`, `hooked` vs `HOOK`, `bearing` vs `BEAR`: no stemming).
  S-50.02 with its label stuffing removed (`48in min lap`, `(8) 16d each side` in plate labels) covers 12/12 (was 9/12). Drawing
  text changes on the 47: 12 items (s-00-02 labels `f'c` now print as written, s-80-02 `1926.755(a)(1)`). Wasm without
  wasm-opt: 768,557 -> 795,385 raw, 312,380 -> 324,074 gzip (the tables and the tokenizer).
- Annotation layout (issue #17, SPEC 25.2): `annot.rs` `at_owner`/`at_drawn` + `Iso::project_on` give an iso `at` the z
  of its component's near face (`Iso::landing` now returns section-style candidates); `notes::check_landings` is
  W_NOTE_LANDING. `text::items_box` is bulge-aware, so the title bubble (and circle tags) are in a view's extents;
  `drawview::fit_area` is the frame above the title block less `FRAME_CLEAR_IN` (1/8") per side for the auto scale, the
  NTS fit (which now estimates the factor it needs) and W_VIEW_FIT. `repair.rs`: `Clearance` (leaders + note blocks),
  `repair_dim` slides text before pushing and never pushes blind, `repair_label` caps at 6 text heights,
  `report_moves` (I_ANNOT_MOVED, the stacking's `blockers` from `dims::stack_dims` plus the pushing note) and
  `report_overlaps` (W_LABEL_OVERLAP). `pin` is read in `annot::read_pin`. `route.rs` `untangle` + the two-start
  search in `route()`. On 120 random dense columns (8-11 four-line notes on one side), untangling took leader
  crossings from 138 to 40 and all leader hits from 341 to 257 (against the same tree without `untangle`). Time on the
  47 MASS details: unchanged (198 `kerf drawing` calls in 0.53 s vs 0.51 s).
  47 MASS details (copies, `kerf drawing` diagnostics per view), master -> this tree: W_LEADER_HIT 16 -> 8,
  W_VIEW_FIT 0 -> 8 (explicit scales whose list or bubble touched the frame or title block: s-10-09 A, s-10-10 A,
  s-80-01 A/B/C, s-70-02 A, s-00-02 C, s-10-02 E), W_NOTE_LANDING 0 -> 2 (s-10-03 B, s-30-01 F: arrows outside the crop),
  W_LABEL_OVERLAP 0 -> 9 (s-30-01 S: 27 table labels overprint), I_ANNOT_MOVED 0 -> 50 (10 labels, 40 dimensions;
  s-10-01/02/03, where every note has a hand-set `place`, account for 38). With every `place` removed (s-00-03, s-10-01/02/03): W_LEADER_HIT
  1 -> 1, s-10-01 A has none.
- House abbreviations (SPEC 25.6): `house::Table` is the style's list (built-in `ABBREVIATIONS` 140 + `VARIANTS` 17,
  less `remove`, plus `approved` / legacy keys, plus `variants`), built per lint run, coverage build and note check
  (about 160 borrowed entries, no hashing). W_ABBREV lives in `lint::abbrev_forms`; `house::abbrev_hits` is the token
  scan, `house::apply_hits` the fix text. Every W_ABBREV of one text carries the same op (all flagged tokens of that
  field fixed), so an agent that applies several of them in one batch cannot undo one with another. Annotations
  and views `acknowledge` through `lint::AckOwner` (path-prefix match; components keep the id / quoted-id match and no
  longer match warnings on an annotation path). The CLI resolves `kerf.kerfstyle.json` per document
  (`workspace::find_style`, canonical ancestors); `kerf serve` reads it per request from its `--dir`, so edits apply
  without a restart (cached doc-list counts refresh when the document changes).
- MASS 47-detail regression (copies, `kerf check --strict`, no refs/): W_ABBREV 190 in 32 details (167 notes, 21 labels,
  2 titles): variants 99 (BOTT. 87, ADDL. 7, BLK'G 5), dotted_initials 68 (PT 47, EN 7, FN 7, EOR 5, CJ 2),
  period_on_shortened 19 (MAX 13, DWGS 5, BLKG 1), bare_acronyms 4 (PL. 3, GLB. 1); every other code unchanged
  (W_ACK_UNUSED 515, W_OVERLAP 208, W_REFERENCE_FILE 134, ...; W_NOTE_STYLE 0 before and after). The MASS company style
  (defaults, `always: []`) gives the same 190; the example company style (`always` O.C. MIN. MAX. DIA.) 192.
  20 flagged tokens checked by hand: no false positive; borderline: `(MAX)` as Simpson's max-nailing legend.

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
- Reference files (`GET /api/docs/:file/reference?path=`, SPEC 23): the only non-document files served. The path is
  checked as text first (`kerf_core::references::path_problem`: relative, no `..`, no backslash or `:`, an image/PDF
  extension), must be listed in that document's `meta.references`, then `workspace::resolve_reference` canonicalizes it
  and refuses a result outside `dir_abs` (a symlink may point within the folder, never out of it). At most 64 MiB,
  `Cache-Control: no-cache` + the document-style ETag. `kerf check` resolves the same paths against the document's folder
  and passes the misses to the engine as `reference_files` (the engine is `no_std` and sees no files); `apply` and the
  `/api/docs` counts do not look files up.
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

## CI and release (`.github/workflows/`, `.github/actions/`)
- Toolchain: `engine/rust-toolchain.toml` pins one stable (components rustfmt + clippy, target wasm32). CI, release and local
  builds use it; the `toolchain` composite action runs `rustup toolchain install` (plain rustup, no third-party actions)
  and restores `~/.cargo` + `engine/target` keyed on `Cargo.lock`, the toolchain file, runner and job shape.
  Raise the pin together with `rust-version` when a newer language feature is needed.
- `ci.yml` (push to `master` and `dev-hotschmoe`, PR, manual): `lint` (fmt, clippy native and wasm32), `engine` (release
  build, `check_golden.sh`, wasm through binaryen version_123 `wasm-opt` + `wasm_golden` + `wasm_check`, `cli_ergonomics`,
  `serve_smoke`, `platform_serve`, `fuzz.py 300 1` on a debug build), `web` (typecheck, unit, UI embedded in a release
  build for `serve_smoke`, `test:e2e`, `test:serve` mock and `--real`, `build`), `macos` (golden, `cli_ergonomics`,
  `serve_smoke` on `macos-latest`) and `windows` (`windows.yml`).
- `release.yml` (tag `v*`): `version` refuses a tag that differs from `[workspace.package] version`; the Windows gate and
  the Linux gates (wasm-opt required, the UI is built from that wasm) must pass; then six native builds with
  `KERF_UI` set (musl static on `ubuntu-24.04` / `ubuntu-24.04-arm`, MSVC `+crt-static` on `windows-latest` /
  `windows-11-arm`, `macos-15-intel` / `macos-latest`), each smoke-run on its own runner (`kerf version` equals the
  manifest version, `check_golden.mjs`, `serve_smoke.mjs`); `publish` writes `SHA256SUMS` and creates the release
  as the latest release, alphas included (the installers download from `releases/latest`, which skips prereleases), with
  `kerf-<arch>-<os>[.exe]` assets, the names `install.sh` / `install.ps1` download. `workflow_dispatch` is a dry run: same jobs, assets uploaded as the `release-assets` workflow artifact, no release.
- `tools/release.sh <version>` bumps the manifest and `Cargo.lock`, commits, tags and pushes.
- Cross-platform test notes: `serve_smoke.mjs` reads process data from `/proc` where it exists and from `ps` elsewhere
  (macOS); the web e2e tests take `KERF_CHROME`, `KERF_CHROME_ARGS` and `KERF_PYTHON` for runners that are not this machine.

## Windows

All `unsafe` lives in `kerf-cli/src/sys/` (`unix.rs`, `windows.rs`: safe wrappers, `#![deny(unsafe_op_in_unsafe_fn)]`, a `// SAFETY:` per block); zero crates. Built and run on a Windows 11 x86_64 machine (2026-10-31)
(release and debug): `serve_smoke.mjs` 189/189 (190 on Linux: one POSIX-only check), `windows_serve.mjs` 32/32, `cli_ergonomics.mjs` 145/145, `tests/platform_serve.mjs` 16/16,
`tests/ctrl_break.ps1`, `tests/check_golden.mjs` (101 outputs byte-identical to the Linux goldens, DXF/PDF checkers clean), clippy `-D warnings` clean. `windows.yml` runs the same gates on every push against the shipped configuration (release, `+crt-static`, UI embedded) on `windows-latest` (x86_64) and the native `windows-11-arm` (aarch64-pc-windows-msvc); both are required and green, including `ctrl_break.ps1` (exit 130, tree gone) and the 101 goldens byte-identical, with no engine change needed for aarch64. The runners' `%TEMP%` is an 8.3 short path (`RUNNER~1`) while the engine reports the long one, so the tests compare `fs.realpathSync.native`.
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
- **Not covered**: console-close events (the handler is the same code as Ctrl-Break), a network share for the op log.

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
- Steel: no WT, S, HP or double angles; channel flanges draw parallel (no 2:12 inner slope); angle/channel root radii
  are by size, not tabulated; pipe and round HSS seen lengthwise are boxes in 3D; elevation/plan members hide what is
  behind them but things in front of the cut plane are still dropped (an elevation detail sets `cut_z` in front of
  everything and gives in-plane members `z` explicitly, see `tests/docs/steel-beam-cap-plate.kerf.json`); weld symbols
  are not obstacles for the note layout; no plate-on-flange fit check yet.
- Rebar `place` (issue #16 leftovers): `face: "center"` still uses the zone box (an L zone's box centre can be outside;
  that is `E_PLACE_OUTSIDE` now, not a silent bar); side cover on a run whose neighbouring edges lean inward is measured
  along the run, not perpendicular to those edges (`W_COVER` reports it); there is no `place` for path bars and no
  `zone: "<part>"` on hand-drawn polygons (use `in: "comp.part"` on builders with parts, or anchors + Ref offsets).
  `bend_k` / `seg_k_mid` count the corner points as given; a collinear interior point still counts as a bend (its
  `bend_k` is the point itself).
- Annotation layout (issue #17): keyed tags still go to columns (no "near the landing, outside solids" placement); a dimension the leader repair
  pushes can still move up to 2" (paper), now reported as I_ANNOT_MOVED; W_LABEL_OVERLAP does not look at weld
  symbols; the minimum gap between leader note blocks is the style `note_gap_in`, unchanged.
- W_ABBREV (SPEC 25.6) does not read the document `title` (title block; nothing there can `acknowledge`), component
  `label`s, citation text or the keyed-notes list title. A token split by a space (`O. C.`, `GYP BD` as one unit) is
  read word by word. `kerf serve` caches the per-document warning counts of its list by document mtime, so a style edit
  shows there on the next change of each document (check/apply/export use it at once).
- The Zig test suite's unit tests (hostile-input table, OOM sweep, layout and view tests) were not ported: tests here are
  end to end (goldens, `cli_ergonomics`, fuzz, `serve_smoke`), by decision.

## SPEC ISSUES
- Issue #18 asks that "citation `section` strings are never case-folded". The golden `flush-psl-2x6` cites
  `Table R602.3(5)` and printed `TABLE R602.3(5)`; folding nothing would print `Table`. Chosen: words of `section` that
  hold a digit print as written, plain words take the case transform (goldens unchanged, `1926.755(a)(1)` kept).
- Abbreviations that are also plain words (ALT. ARCH. COL. DIM. EXP. FIN. INT. SIM.) expand in coverage only when
  written with the period, and a candidate offers the written word too, so `screw alt` still matches `SCREW ALT.`
  (S-80.04) while `arch` does not match `ARCHITECT`. `NO.` is accepted at the end of a note but never expanded
  (`no pipes` must not match `NUMBER`).
- Lengths: a requested length with units counts only as the length; a bare fraction counts as its digits (so `3/16 fillet
  weld` keeps matching a weld whose size is `3/16`). `2'-2` without the inch mark reads as 2'-2".
- Not done from the issue's proposal: `I_REQUESTED_STYLE` at `kerf new` (with abbreviation matching both ways it has
  nothing left to warn about), table cells (#6) and nailing components (#13) as coverage sources: those features are
  other branches; add them as candidates in `coverage::candidates` when they merge.
- #19 W_CROP_STALE "only warn when the member changed since the crop was set": the engine has no record of when a crop
  was set, so an apply compares the member's extent before and after the edit (moved or resized = changed); `kerf check`
  and new members use the "< 10% inside every view" rule. A crop narrowed on purpose never warns unless < 10% is left.
- #19 polygon slab cover: kept the 3" default for every concrete shape (a footing polygon needs it) and made W_COVER say
  when the default cannot fit the host depth, with the exact op; deriving the default from thickness would silently
  change cover for footings drawn as polygons.
- #19 anchor edge distance: ACI 318-19 17.9.2 ties cast-in anchors to cover (20.5.1.3); the check uses 1 1/2" clear for
  every hook type (post-installed 6d would warn on most IRC sill anchors whose ESRs allow 1 3/4"). Spacing uses 4d
  cast-in / 6d post-installed.
- #19 text mask: hatch knockout under note/dim/label text already exists (SPEC 18), so no `mask` field was added; the DXF
  hatch is still not knocked out (Known gaps).
- SPEC 13 says `kerf export` takes `format: "svg"|"dxf"|"pdf"`; the engine also accepts `png`, and PDF export frames the
  drawing on a sheet by default while SVG/DXF/PNG do not. Worth one sentence in 13/13.2.
- SPEC 12 `3'-0 1/4"` versus `3'-1/4"` for feet with zero whole inches and a fraction: the engine prints the former.
- Issue #5 asks for checks over the components a view draws, "not omitted and not out of `shown`". Read as: compiled,
  `visible: true`, not in the view's `omit` (`shown: "dashed"` members are already exempt). Fills stay in iso sets
  (an iso hides them for drawing reasons, not because they are absent). A document without views keeps today's set,
  invisible components included. W_UNTREATED_CONTACT and W_COVER run per view too (same pair logic); `W_SHORT_SLOPE`
  keeps the first section view's explicit crop for every view (per-view crops would be a separate behaviour change).
  The view list is printed only when a finding is absent from some view, so single-view and omit-free documents read
  as before.
- Issue #15 asks for "an internal assertion in debug builds" when a Diff removes more than the hole's area. Chosen: no
  panic (a debug build would abort a user's drawing); the subtraction is skipped and `W_CUT_FILL` reports it in every
  build, which is visible to agents and tests alike.
- Issue #16 proposes `place: {"in": "cmu.cell", "cell": 2}`. In a wall cross-section there is one cell, and the bars in it
  are path bars (`place` places along-z dots). Chosen: a `cell` part (between the face shells) for along-z bars and the
  anchors `cell_left/center/right(_top)` that path points reference with an offset. Issue #16 item 4 (`{ref, offset}` path
  points) already existed (SPEC 3.3); `start`/`bend_k` anchors make it useful for laps.
- `W_BAR_OUTSIDE` is a new code rather than a `W_COVER` variant: `W_COVER` is about cover in a host, this is about having
  no host. A path bar only warns when a bend is outside or no point is inside, because a straight leg out of the concrete
  is common (dowels into future work, S-80-01's lap bar welded to a column between two pours).
- Issue #17, iso `at`: the issue asks that `{ref, offset}` "land on the offset point of the 3D solid". Chosen: z is the
  near face of the Ref's component (largest drawn z1 for front views, smallest z0 for back views, after cutaway), a
  literal point uses the note's `target`, else `cut_z` (labels: same rule, no target). A point at the far end of a
  member therefore needs a Ref on that member, not a z.
- Issue #17, W_NOTE_LANDING in sections: warning only outside the crop. Inside it an arrow legitimately points into an
  opening or the air ("WALL OPENING PER PLAN" in s-10-02 E); on the 47 MASS details an off-geometry rule also fired on
  5 arrows inside the crop 0.05-0.13" (paper) from a line, which read as intended.
- Issue #17 proposes `I_ANNOT_MOVED` as info: kept info, but `kerf check` now prints it (it filtered infos to a short
  list). The 1/8" (paper) threshold applies to the dimension line's offset and to the label's shift; a text slide along
  the dimension line is not reported (the line stays where the author put it).
- The frame clearance is a constant 1/8" (paper), not a style key; the sheet still centres the view, so the clearance
  shows as at least 1/8" on every side when the view fits.
- Issue #8 (SPEC 25.4): the issue reads as if ops inside one batch had to be ordered around `omit`; ops are validated at
  the end of the batch, so only an `omit` sent in an earlier `apply` than its component failed. Chose: unknown `omit` /
  `only` entries warn W_REF_UNKNOWN (also globs matching nothing), unknown views in component `views` stay
  E_REF_UNKNOWN (a typo there silently drops the member from every view). `views: []` and `only: []` are E_PARAM
  rather than "drawn nowhere" (`visible: false` exists for that).
- W_ACK_UNUSED is one diagnostic per component (not per entry): the MASS set has 1,028 stale entries and a line each
  drowned everything else; the fix is the full `update` op with the remaining entries. It is suppressed while the
  document has errors or views were not built, since missing diagnostics would make live entries look stale. An
  acknowledged `I_*` code now gets W_ACK_UNUSED (SPEC 20 made it a silent no-op; it still is not an error).
- "Compact by-code listing" read as: group by (code, reason), one line each with the count and up to six components;
  per-warning I_ACK lines left the summary (they are still in the JSON and in `--strict`). The DOC line always shows
  `N acknowledged`, so every golden summary.txt changed by that suffix.
- The drawing IR's `acknowledged` is per view: acknowledged warnings whose acknowledging component that view draws.
  It is omitted (not `[]`) for documents without `acknowledge`, so no existing drawing golden changed.
- House abbreviations (SPEC 25.6), choices the engineer may want to revisit (each is one table edit or a company-style
  key): where two spellings were in use and the policy names none, the table keeps the one the 32 sets use most:
  `EL.` over `ELEV.` (2 : 1), `GR.` over `GRD.` (1 : 0), `STRUCT.` over `STRUC.` (8 : 1); plural forms
  (`DWGS.`/`DWG.`, `SPECS.`/`SPEC.`) and different words (`D.F.`/`DF-L`, `BLK.`/`BLKG.`) both stay approved.
  The bare letters of `T.O.`, `I.D.`, `S.A.D.`, `T.O.W.`, `T.O.M.`, `EMBED.`, `EXIST.`, `SECT.` and the variant
  `PLY.` are ordinary words (TO, ID, SAD, TOW, TOM, EMBED, EXIST, SECT, 2-PLY), so they join SIM/COL/FIN as never
  flagged (coverage then reads bare `id`, `tow` as words). `EQ` (dim text) and `GA` are flagged like any shortened
  word.
- `PT` -> `P.T.` is listed as a variant (with the post-tensioned note in its message), but its letters are those of
  `P.T.`, so it breaks `dotted_initials` like `UNO`, `NTS`, `EOR` (switching that rule off allows PT); the rule a
  spelling breaks follows from its letters, not from the list it is in.
- "A fix that rewrites only that token": every W_ABBREV of one text carries the same op, which replaces every flagged
  token of that field by its approved form and nothing else. One op per token would let a batch of fixes undo each other
  (each op sets the whole text).
- W_NOTE_STYLE no longer abbreviates in its own fix (alpha.5 turned ON CENTER into O.C. there); `always` in
  kerf-standard is empty, so no word is forced unless a company lists it.
- Workspace style discovery walks every ancestor up to the filesystem root (no repository or home boundary), like
  .editorconfig; `check`/`apply` print the `STYLE` line only when a file applies, so outputs without one are unchanged.
- A view's `acknowledge` matches only warnings whose id is the view's or absent (W_CUT_FILL sits at `views/A` but names
  its host component, so a view cannot hide it); a component's no longer matches a warning on an annotation path (an
  annotation id equal to a component id). `acknowledged` entries gain `view` and `on` only for annotation and view
  owners, so component entries (and their goldens) are unchanged.

## REQUESTS
- apps/web: the inspector lists `acknowledged` by component id; annotation and view entries now carry `view` and `on`
  (owner path) and should be shown on the note/view. `apps/web/test/fixtures/make-fixtures.mjs` still writes
  `2X8 PT SILL PLATE` (W_ABBREV now asks for `P.T.`); the browser engine takes the served `style.json`, which
  `kerf serve` now merges with the folder's `kerf.kerfstyle.json` (the header's STYLE shows its `id`).
