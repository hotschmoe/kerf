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
| `tests/check_golden.sh` | `tests/make_golden.sh` regenerates the drawing JSON, SVG (bare and sheet), DXF, PDF, PNG (bare and sheet), mesh and check summary of every view and every sheet layout (drawing JSON, sheet SVG, DXF, PDF, sheet PNG) of the documents (`../spec/details/*`, `tests/docs/*`) into a temp tree; every file is byte-identical to `tests/golden`, every summary has 0 errors, every DXF passes `dxf_check.py` (ezdxf audit), every PDF is vector (`pdf_check.py`) |
| `tests/wasm_golden.mjs`, `tests/wasm_check.mjs` | the wasm output equals the native goldens byte for byte (58 files, layouts included); the module has 0 imports and exactly the six ABI exports |
| `tests/cli_ergonomics.mjs` | 342 checks of the CLI (stdout, stderr, exit code, files): what an agent sees |
| `tests/fuzz.py N seed` | mutation fuzz of the reference documents (hostile numbers, styles, ops) through check/drawing/export: no crash, no hang. Run it on a debug build too: overflow checks, debug assertions |
| `tests/serve_smoke.mjs` | 227 checks of `kerf serve` on temp folders: routes, tokens, hostile HTTP, SSE, the agent bridge with `fake-agent.mjs`/`fake-grok.mjs`, op log, symlinks, limits, reference files. `tests/windows_serve.mjs` is the field-shaped session written for Windows (it runs everywhere) |
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
- Tables and schedule views (issue #6, SPEC 26.1): `annot/table.rs` reads a table into a cell grid (`owner` = cell index
  per slot, HTML placement), lays it out (`layout`: auto widths, spans widen their auto columns, `text::wrap_width` by font
  width, row heights) and draws it at the origin (`render`); `annot::annotate` places it (`at`, stacked from [0, 0] in a
  schedule view, under everything else in other views) and runs `table_overlaps` (W_TABLE_OVERLAP: other tables,
  `ann_boxes`, path segments, hatch/fill loops). `view::Kind::Schedule` (`"schedule"`, alias `"sheet"`) has
  `Members.none` (draws nothing; `validate::drawn_sets` skips it, so geometry findings never list it), and
  `drawview::schedule_pass` builds it at scale factor 1 with an empty `Section` for the `Env` and the content box as
  crop; `schedule_fit` is its W_VIEW_FIT. Keyed notes without `target`/`at` are `listed` in `Gathered` and kept out of the
  router. Lint (`lint::table_style`) and coverage (`coverage::table_candidates`) read the document through
  `table::texts` / `row_texts`. 47 MASS details: `kerf check --strict` output byte-identical before/after (0 errors, 790
  warnings; none uses a table). S-00.02 rebuilt with three tables (A, B schedule views; C a section with the hook
  sketches and the table at `at`): 143 -> 2 components, 144 -> 2 warnings under --strict (the two acknowledged
  W_FLOATING hook sketches; W_ACK_UNUSED and the 141 cell solids gone), coverage 14/14, 91 KB -> 12 KB of JSON. Wasm
  (wasm-opt -Oz): 788,276 -> 837,511 raw, 347,996 -> 368,624 gzip -9 (layout code and the diagnostic/schema text).
- Subtracted solids (issue #14, SPEC 26.4): `voids.rs`. `apply` runs at the end of `compile` (after every placement, so a
  void never adds a placement dependency or a cycle): it collects each host's `voids` entries and the sleeves whose
  `through` names it, cuts the host's world prisms (z-split at the void ends, `Prism.seam` marks the internal caps,
  `Prism.bond_z0` keeps the CMU bond) and keeps the flattened cutters on `Comp.voids` for W_COVER. A void wholly inside
  a region adds its exact loop as a hole (sleeve circles stay arcs in DXF); anything else is the flattened boolean, and
  then every slab of that prism is flattened the same way so seams match along arcs. Iso (`IsoPrism.seam`) and the mesh
  (`extrude(.., seam)`) draw a seam cap edge only where `geom::uncovered` finds it is not on the touching slab
  (`voids::seam_neighbors`). The mesh now extrudes holes: `bridge_holes` joins each hole to the outer loop (rightmost
  hole vertex to the nearest visible outer vertex) and `ear_clip` skips vertices that coincide with the ear's corners;
  loops without holes give byte-identical meshes. Fill clipping is per view: `voids::clip_fills` in `Section::new`
  (so any 2D view that builds a `Section` gets it) and `clip_fill` per prism in iso `gather` (cutaway caps); `load.rs`
  (`inspect at`) now walks `sec.prisms` because a clipped fill can be several prisms. `shape::shapes_of` gives a hole
  to the innermost outer loop only (nested sleeve rings used to hand the bore to the sleeve ring and the note found no
  landing). Mesh volumes check out exactly (closure pour 434 sq in x 24 less column, plate and grout = 9437.25 cu in; a
  stem less a 5 1/2" sleeve within the flattening). The 47 MASS details as drawn: unchanged except s-10-08 (the
  shoring strut inside the backfill no longer overlaps: its acknowledgement is now W_ACK_UNUSED) and the W_OVERLAP
  fixes in s-80-01 that now print the voids op; 47 docs `kerf check --strict` 0 errors / 790 warnings before and after.
  Rebuilt on copies: s-10-02 A-C and s-10-04 A/B/D with plain earth and base rectangles draw the same as the
  hand-notched polygons (warnings unchanged: 5 and 0); s-80-01 A as one closure polygon `voids: [a_column,
  a_base_plate, a_grout]` draws as the two traced halves did, follows an HSS10x10 with no edit, and the B2 leave-outs
  with `voids: [pm_col]` drop both acknowledged W_OVERLAPs (7 warnings incl. 2 acknowledged -> 5); s-10-08 with three
  `sleeve` components (pipe 4", packing joint_filler) instead of nine nested `solid` circles reads like the reference
  (sleeve and pipe rings, open bore) and has 0 warnings (was 1 acknowledged). Time on the 47: noise (5.04 s -> 4.86 s for
  every view's `kerf drawing` + `check`).
- Plan and elevation views (issue #9, SPEC 26.3). Both go through the section engine: `drawview::flat_view` turns a
  view into (scene, prisms, spec with the cut plane in `cut_z`). Elevation: `Section::new` classes every prism beyond,
  `draw_beyond` uses the profile pen (end-grain X on run-z lumber, loose-fill hatch on its visible part,
  `hidden_pieces` for `hidden_lines`), `break_lines` scans each member's visible region just inside the crop.
  Plan (`plan.rs`): each XY prism becomes strips (x-interval x z range, model y as depth: cut where `cut_y` crosses the
  profile, else below with the profile's top as depth; below strips of one member merge unless the top steps or creases
  by 10 deg or more), rebar becomes centreline bars and dots, membranes crossing the plane lines along z, fills are
  dropped; `plan::scene` clones the components with per-instance plan boxes (`Comp::plan`) so Refs resolve in plan
  coordinates through `Scene::anchor_point`. Rebar `plane: "xz"`: the builder emits legs (`Prism::xz_bar`) for checks,
  iso and mesh, and the flattened 3D centreline (`Prism::path3`, first leg) from which `plan::xz_bars_in_section` makes
  dots and projected lines for sections/elevations. 47 MASS details: every section view as a plan and as an
  elevation (326 views): 0 crashes, slowest 0.34 s. Their 198 existing views draw byte-identical drawing IR (diagnostics
  aside). Wasm 788,276 -> 825,316 bytes (+4.7%; gzip 347,996 -> 363,164).

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
- `solid` / `polygon` profiles with holes are not supported (profiles are single loops); holes come from `voids` and
  sleeves (SPEC 26.4).
- Subtracted solids (#14): the mesh does not clip fills (`mesh` with `include_fills` still shows earth through a footing; only
  section and cutaway-iso caps are clipped); iso fills still do not occlude what is behind them; `rebar.place` and the
  W_COVER host test use the host outline without its voids (a bar placed into a void is not reported as outside; its
  cover is measured to the void edge); a component void is the member's envelope with holes filled, so concrete between
  the flanges of an encased W needs the flanges and web named as parts (`"beam.top_flange"`, ...) or shapes; sleeves
  seen lengthwise (run x/y) are rectangular tubes in 3D (2.5D, like pipe in elevation).
- Hatch under dimension/label text is knocked out in SVG/PDF only (the DXF HATCH uses the pattern, not lines).
- PDF content streams are uncompressed (about 150 KB per sheet).
- Allocation churn: glibc malloc, not a per-call arena (the measured bound is 0.77x in-process; a safe arena for the
  threaded server is real allocator work). Process start-up is dynamic glibc + libgcc_s (0.9 ms). See `PERF.md`.
- Steel: no WT, S, HP or double angles; channel flanges draw parallel (no 2:12 inner slope); angle/channel root radii
  are by size, not tabulated; pipe and round HSS seen lengthwise are boxes in 3D; in a section, `orient`
  elevation/plan members hide what is behind them but things in front of the cut plane are dropped (use a view of
  `kind: "elevation"`, which cuts nothing, SPEC 26.3); weld symbols are not obstacles for the note layout; no
  plate-on-flange fit check yet.
- Plan and elevation views (issue #9, SPEC 26.3): a plan's occlusion depth per strip is the highest point of the profile
  over that strip (a sloped member hides a lower one by its top, not point by point); a plan draws rebar below the cut
  and members `shown: "hidden"` over everything (no occlusion of embedded parts, as in a section); xz bars measure
  `W_COVER` in XY only (their cover along z, to the faces of a wall running along x, is not checked); a diagonal xz leg is
  a staircase of slabs for iso, mesh and the checks (the views draw the true path); two components that meet at a
  corner draw their shared edge (a monolithic L wall needs one polygon component, which only works in one direction in
  2.5D); `cmu_wall` has no face-on elevation (its courses are in the section plane), so a CMU wall elevation is still a
  `solid` per course; there is no `along_y` rebar mode (a vertical bar is a two-point path bar); a plan cannot cut
  through a sloped member exactly where a strip boundary falls on a vertex at the cut (it is cut or below as a whole).
- Rebar `place` (issue #16 leftovers): `face: "center"` still uses the zone box (an L zone's box centre can be outside;
  that is `E_PLACE_OUTSIDE` now, not a silent bar); side cover on a run whose neighbouring edges lean inward is measured
  along the run, not perpendicular to those edges (`W_COVER` reports it); there is no `place` for path bars and no
  `zone: "<part>"` on hand-drawn polygons (use `in: "comp.part"` on builders with parts, or anchors + Ref offsets).
  `bend_k` / `seg_k_mid` count the corner points as given; a collinear interior point still counts as a bend (its
  `bend_k` is the point itself).
- Annotation layout (issue #17): annotations cannot `acknowledge` (W_LEADER_HIT on an intentional crowd stays);
  keyed tags still go to columns (no "near the landing, outside solids" placement); a dimension the leader repair
  pushes can still move up to 2" (paper), now reported as I_ANNOT_MOVED; W_LABEL_OVERLAP does not look at weld
  symbols; the minimum gap between leader note blocks is the style `note_gap_in`, unchanged.
- The Zig test suite's unit tests (hostile-input table, OOM sweep, layout and view tests) were not ported: tests here are
  end to end (goldens, `cli_ergonomics`, fuzz, `serve_smoke`), by decision.
- Tables (issue #6): no rotated cell text (ref 1231's side label LAP SPLICES IN INCHES), no double or per-row heavy rules,
  no shading; one text height (the note height) for title, headers and cells; a table is not an obstacle for leader
  routing (auto placement keeps it clear of everything; one placed with `at` only warns W_TABLE_OVERLAP); a long
  schedule is not split across views automatically (W_VIEW_FIT says to split it); auto-width cells never overflow, so
  W_TABLE_CELL fires only on fixed `width`s. The web UI shows tables (they are plain drawing items) but its inspector
  has no table editor (edit through ops).

- Sheet layouts (issue #7, `layout.rs`, SPEC 26.2): no automatic arrangement search (the W_VIEW_FIT fix proposes one;
  the layout uses what the author wrote); cells of one row align on their bottom, so a view whose title wraps to two
  lines puts its bubble a line higher than its neighbour's; an iso (NTS) view is fitted to an even share of the sheet,
  not to the space its neighbours leave; a shared-key note still needs its text in its own view (the standalone view
  build reads it there); reference bubbles are not obstacles for the note router (like welds); `--review` is per view
  only; the 3D view and the inspector know nothing of layouts (the web UI lists and renders them, nothing more).

## SPEC ISSUES
- Sheet layouts (#7) follow the issue's design with these choices: `notes_side` (right|left|below|above) for where the
  shared list goes; a layout `sheet` size (`"17x11"`), because four scenarios and a list do not fit a letter sheet at a
  useful scale (s-00-01 needs 17x11 at 1/2"; on letter even 1/4" overflows); nested lists in `views` for insets (a
  column beside a main section) instead of a separate inset concept; the shared list is renumbered 1..N in key order
  (I_KEY_RENUMBERED says so) so "no gaps" holds even when the authored keys skip numbers; an unkeyed note joins an
  earlier entry with the same text and cite (the s-10-01 drafts repeat texts across scenarios without keys); scale
  precedence view > layout numeric > document `scale` > `common`; iso views stay NTS in a layout (fitted to their
  share); view titles wrap in layouts only (at least 3" paper wide); the layout's coordinates are paper inches times
  the first view's scale (DXF keeps that view 1:1). The layout W_VIEW_FIT keeps the code W_VIEW_FIT with id = the
  layout and path `layouts/<id>`. The `ref` annotation's leader point is `at` (as for notes) and its bubble `place`.
- Sheet title block DETAIL NO: a layout without `number` prints the sheet alone (`S-10.01`), not `/S-10.01`; the same
  rule now applies to a view whose `number` is the empty string (no golden has one).
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
- Issue #14 `fill.except: [ids]` "restricts the clipping to named components": read as "these components do not clip
  the fill" (the fill keeps its hatch under them); W_OVERLAP stays exempt for them (fill vs solid is never an overlap).
- Issue #14 fills are clipped by the solids the view's plane cuts (z0 < cut_z < z1 of the member's envelope), not by
  beyond members: a fill is a cut region, and a beyond member behind it is hidden by it as before. Fills never clip
  fills (gravel on earth meets edge to edge as drawn).
- Issue #14 voids are geometry, not view membership: a host loses its voids in every view, also where the void member
  is omitted or `visible: false` (the hole is in the concrete).
- Issue #14 proposes `concrete.sleeves: [...]` or "a small `pipe` type". Chose a `sleeve` component (own id, notes target
  `s.sleeve` / `s.fill` / `s.pipe`, normal placement) with `through` naming hosts, which is the same mechanism as host
  `voids` (one code path); a block-out without a sleeve is a `voids` shape. Sizes from the drafting phrase "pipe to
  clear sleeve by 1/2" all around": `pipe` + `clearance` 0.5 + `wall` 0.25, or `od`.
- The drawing IR's `acknowledged` is per view: acknowledged warnings whose acknowledging component that view draws.
  It is omitted (not `[]`) for documents without `acknowledge`, so no existing drawing golden changed.
- Issue #6 proposes `kind: "sheet"` for a geometry-free view. Chosen: `kind: "schedule"` (what the view holds; "sheet"
  already means `--sheet` framing and #7's layouts), with `"sheet"` accepted as an alias. Its drawing IR `kind` is
  `"schedule"` and `scale` 1 (paper inches); `"scale": "NTS"` is accepted, anything else geometric is ignored with W_PARAM
  rather than E_PARAM so a view converted from a section keeps drawing. A section view still rejects NTS (now pointing
  to the schedule kind), since an NTS section has no fit rule.
- Table `at` is the top-left corner of the border (not of the title), so tables align on their grids; auto placement in a
  schedule view puts the first table's border corner at [0, 0] for the same reason.
- `header` rows and column headers in `columns` are exclusive (E_PARAM) instead of stacking, so there is one header form
  per table; a column `unit` still lands in the lowest header cell over its column, or warns when that cell spans.
- `rules` semantics (the issue named the four values only): `header` keeps every rule inside the header (grouped headers
  need them) and the column rules, and drops rules between body rows; `outer` is the border plus the header rule.
- Table `notes` print numbered under `NOTE:`/`NOTES:`; a note starting with `*` is an unnumbered footnote (the issue
  said "notes/footnotes" without a form).
- Units are not linted (`IN.` would be flagged as a trailing period); cells, headers, title and notes are, with no repeat
  check (repeated values are what a table is).
- Coverage: a body row is matched together with the title and all header text, so `#4 bar 38` or `HDU8 holdown` match one
  row; the title with the headers alone is another candidate (`f'c = 4,000 psi`).
- The issue's "in keyed views, a note without target/at becomes a list-only entry": done for keyed views; leader views
  keep the E_PARAM (its message now suggests a keyed or list view).
- The 47-detail drafts were not converted to tables (only scratch copies of S-00.02); `~/github/mass-details` is
  untouched.
- Issue #9 plan coordinates: the issue does not say which way plan y points. Chosen: plan y = -model z, the only
  orientation that is not a mirror image when looking down with x to the right (the section viewer at +z stands at the
  bottom of the plan). Literal points in a plan view are therefore [x, -z]; to soften that, a plan `crop` also takes
  `{"x": [...], "z": [...]}` in model z, and Refs resolve to plan points by themselves.
- Issue #9 default `cut_y` "the top of the tallest member minus 1"": read as the top of the highest drawn member
  (bodies only, loose fill excluded) minus 1".
- Issue #9 "members below draw as visible outlines, or as hidden lines with `hidden_lines: true`": read as the view
  option that also draws the hidden parts of outlines dashed (both kinds), on top of the visible outlines. Members
  `shown: "hidden"` draw their whole footprint dashed in plan, above or below the plane (the "beyond / above" plan
  convention). Embedded parts (rebar) are never hidden in plan either, as in a section: a rebar plan shows the bars in the
  concrete.
- Issue #9 elevation `cut_z`: an elevation draws every member, so its `cut_z` would mean nothing; kept as the default z
  of in-plane members (SPEC 1/19 read it from the first section view; elevations count, plans and isos do not). A plan's
  `cut_z` warns W_PARAM.
- Issue #9 `dashed` compatibility: the issue offers "an alias with a W_UNKNOWN_KEY-style hint". Chosen a warning,
  `W_SHOWN_DASHED`, with both update ops, so `--strict` runs see it: drawings are unchanged. On the 47 MASS details it adds
  39 warnings (11 documents) and changes nothing else.
- Issue #9 titles: the brief asks to decide whether plan titles get "(PLAN)" automatically. Chosen a style template,
  `text.plan_title` = "PLAN - {title}" (the MASS references print "PLAN - CORNER REINFORCING ..." and "PLAN VIEW - ..."),
  skipped when the title already contains the word PLAN; `text.elevation_title` = "{title}" (the references put
  ELEVATION in their own words: "FENCE WALL ELEVATION"). The template applies to the printed title and the title block,
  not to the document, so coverage still reads what the author wrote.
- Issue #9 "path rebar in the XZ plane": no such mode existed. Added rebar `plane: "xz"` (points [x, z] at height `y`)
  instead of a full 3D path: the 2.5D model keeps XY prisms, and only views that can draw the true path (plan; section
  dots and projections) use the centreline. `along_y` from the issue's wording is not a new mode: a vertical bar is a
  two-point path bar, which a plan draws as a dot.

## REQUESTS
- Sheet layouts (#7): print view titles through `drawview::view_title(st, spec)` (it applies `text.plan_title` /
  `text.elevation_title`), and treat `kind` plan/elevation views like sections (they are flat, numeric-scale views; their
  Drawing `kind` is "plan" / "elevation").
- Subtract solids (#14): if voids are applied to the compiled world prisms (`Comp::world`), plans and elevations get them
  for free; if they are applied inside the section engine instead, `plan::prisms` needs the subtracted profiles before it
  slices them.
