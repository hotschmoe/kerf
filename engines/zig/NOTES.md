# engines/zig: Kerf engine in Zig 0.16

Status: feature complete for the v0.1 contract (SPEC 1-17), including keynote note mode.
Everything is std-only Zig 0.16; one source tree builds the importable module `kerf`, the CLI and the
wasm32-freestanding ABI module. All three reference details export SVG, DXF (zero audit errors), PDF
(vector only), PNG and mesh, with 0 errors / 0 warnings. Goldens are in `tests/golden/<detail>/`.

Zig: `~/tools/zig-aarch64-linux-0.16.0/zig` (0.16.0). No third-party dependencies.

## Build / run / test

```sh
cd engines/zig
zig build                    # CLI -> zig-out/bin/kerf
zig build test --summary all # unit + reference-document + leak tests (std.testing.allocator)
zig build wasm               # -> dist/kerf.wasm (wasm32-freestanding, ReleaseSmall, zero imports)
zig build wasm -Dwasm-optimize=ReleaseFast   # speed comparison
zig build wasm -Dwasm-strip=false            # keeps names for `twiggy top dist/kerf.wasm`
zig-out/bin/kerf export ../../spec/details/truss-bearing-cmu.kerf.json --view A --format svg --sheet -o /tmp/a.svg
../../tools/zig-engine/svg2png.sh /tmp/a.svg /tmp/a.png 1600      # headless chromium; Read the PNG
node ../../tools/zig-engine/wasm_check.mjs dist/kerf.wasm <doc> A out.svg   # ABI check (0 imports, exact exports) + timing
node ../../tools/zig-engine/bench.mjs dist/kerf.wasm                        # per-function wasm timings
tests/make_golden.sh [--png]   # regenerate tests/golden (svg/dxf/pdf/drawing/mesh/summary, PNGs for review)
tests/check_golden.sh          # byte-identical regeneration + dxf_check (0 audit errors) + pdf_check (vector) gate
../../tools/zig-engine/compare_drawings.py zig.json rust.json [-v]   # cross-engine Drawing IR diff, tol 1e-3"
../../tools/size_report.sh dist/kerf.wasm
node ../../tools/zig-engine/wasm_golden.mjs dist/kerf.wasm   # wasm exports == native goldens (24/24 byte-identical)
python3 ../../tools/zig-engine/fuzz.py 300 1                 # mutation fuzz of the safety-checked CLI
```
`dist/` is git-ignored; build it. The default style and stroke font are embedded straight from
`spec/styles/kerf-standard.kerfstyle.json` and `spec/fonts/kerf-simplex.json` by `build.zig`
(anonymous imports `kerf_style_json` / `kerf_font_json`); the engine therefore builds inside this
monorepo only (as a path dependency it is fine; copying `engines/zig` elsewhere breaks it).

## Using the engine from Zig (teak app)

```zig
// build.zig.zon:  .dependencies = .{ .kerf = .{ .path = "../kerf/engines/zig" } },
const kerf_dep = b.dependency("kerf", .{ .target = target, .optimize = optimize });
exe_mod.addImport("kerf", kerf_dep.module("kerf"));
```
```zig
const kerf = @import("kerf");
const r = try kerf.call(gpa, "export", input_json); // r.ok == false => r.bytes is {"error":{code,message}}
defer gpa.free(r.bytes);
```
`kerf.call(gpa, fn_name, input_json)` is exactly SPEC 13 (`version catalog fmt check apply inspect drawing mesh export`).
Output ownership: everything the call allocates internally lives in a per-call arena; only the result is
duplicated into `gpa`. Typed building blocks are public for in-process rendering: `kerf.drawview.build`
-> `kerf.drawing.Drawing` (items: path / fill / hatch / text in model inches), `kerf.compile.compile` ->
`kerf.scene.Scene` (components, prisms, anchors), `kerf.svg.render`, `kerf.mesh.build`.

## Calling the wasm (web app)

`dist/kerf.wasm` exports exactly `memory, kerf_alloc, kerf_free, kerf_call, kerf_out_ptr, kerf_out_len`
and imports nothing (`wasm_check.mjs` parses the module and fails otherwise).
```js
const {instance} = await WebAssembly.instantiate(bytes, {});
const ex = instance.exports, enc = new TextEncoder();
function call(fn, input) {
  const f = enc.encode(fn), i = enc.encode(JSON.stringify(input));
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f);   // re-read ex.memory.buffer after every alloc/call (memory can grow)
  new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const status = ex.kerf_call(fp, f.length, ip, i.length);   // 0 ok, 1 error (output = {"error":{code,message}})
  const out = new Uint8Array(ex.memory.buffer, ex.kerf_out_ptr(), ex.kerf_out_len()).slice();  // copy before the next call
  ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return {status, out};
}
```
Output is JSON text for every function except `export` (raw svg/dxf/pdf bytes) and `catalog` with
`format: "markdown"` (raw text, SPEC 16).

## Architecture (src/)

`kerf.zig` (module root) -> `api.zig` (dispatch) | `json.zig` (order-preserving parser/writer, Kerf number
format) | `units.zig` (length/scale/slope, ft-in) | `geom.zig` (bulge arcs, exact `orient2d` with
expansion fallback, segment/arc intersection) | `clip.zig` (polygon booleans: arrangement + midpoint
classification, coincident edges handled by direction) | `pathclip.zig` | `pathgeom.zig` (fillets, offsets,
ribbons) | `catalog.zig` (single source of truth for params/validation/markdown) | `builders.zig` (component
builders) | `compile.zig` (placement DAG, arrays, z) | `scene.zig` (refs, anchors) | `validate.zig` |
`ops.zig` (apply) | `load.zig` (summary, inspect) | `section.zig` | `iso.zig` | `hatch.zig` | `annot.zig`
(notes/dims/labels/title) | `drawview.zig` | `drawing.zig` (IR) | `sheet.zig` | `svg.zig` `dxf.zig` `pdf.zig`
| `mesh.zig` | `canon.zig` (fmt) | `main.zig` (CLI) | `wasm.zig` (ABI).

Design points: a component builds *prisms* (profile region with bulges + z range + role flags) in local
coordinates; placement transforms them (translate/rotate/mirror, arrays, z). Section view = exact
2D: beyond outlines are split against occluder boundaries (arcs stay arcs), hatch is clipped
analytically per pattern family, coincident edges are deduped (same-component shared edges draw in
the `beyond` pen). Iso view = planar-face HLR on a uniform grid (SPEC 8.2). Determinism: no hash maps,
no clocks; every output is a pure function of (doc, style).

## Measured numbers

wasm (`tools/size_report.sh`): ReleaseSmall **662,838 B raw / 245,562 gzip / 196,330 brotli**;
ReleaseFast 1,427,110 raw / 422,094 gzip / 304,478 brotli. (Code is ~450 KB of the raw size; `rodata` 83 KB; the
style + font are 23 KB of it. `twiggy top` shows no single hog: builders 59 KB, iso 36 KB, annot 36 KB,
dispatch 35 KB, clip 29 KB.) Zero imports; exports exactly the SPEC 13.1 six.

Timings in node 22 (V8), wasm ReleaseSmall, ms per call (`tools/zig-engine/bench.mjs`):
`check` 4-20, `mesh` 0.7-1.9, `inspect` 0.3-0.5, section `drawing` 1.4-2.4, section svg/dxf/pdf export 2-7,
iso `drawing` 15-18 (HLR + scale-fit loop), iso exports 16-20. Every view is well under the 50 ms target.
ReleaseFast is not measurably faster for these sizes (12 ms vs 12 ms on truss A), so ReleaseSmall ships.
Native debug build: `kerf export` of a section view takes about 30 ms wall including process start.

## Golden PNGs (reviewed by eye)

`tests/golden/<detail>/{A,B}.png` (sheet SVG rendered by chromium), `{A,B}-dxf.png` (ezdxf render of the DXF),
`{A,B}-pdf.png` (pdfium render of the PDF); details: `truss-bearing-cmu`, `monopour-slab-door-recess`,
`flush-beam-strap`.

## PNG export (`format: "png"`, `src/raster.zig` + `src/png.zig`)

`kerf export <doc> --view A --format png [--px 1600] [--sheet] -o out.png`; API input `{"format":"png","px":1600,"sheet":false}`.
8-bit grayscale, white paper, black ink, anti-aliased. Same items, Map, pens, dashes (round caps, odd lists repeat),
even-odd fills, hatch lines and stroke-font text as the SVG exporter; region items ignored. `px` = output width in
pixels (default 1600, clamped 200-6000; if the page is so tall that the height would exceed 8192 the scale is reduced
instead, so the width can come out smaller). Pen widths are true mm at the chosen scale, minimum 1 px. Bare detail
keeps the SVG's 0.25" margin. Output is deterministic and byte-identical between native and wasm (checked on truss A sheet).
Design: own CPU rasterizer (not the teak triangle tessellator): each item goes into a u8 coverage scratch (union by
max, so joins never double-darken), then composited once; strokes are capsules with analytic row spans
(coverage = clamp(r + 0.5 - dist)), fills are 8-sub-scanline even-odd with exact horizontal coverage. PNG: per-row
adaptive filters + std `flate.Compress` zlib level `.default` (`.best` is 4x slower for 3% smaller files).
Timing, native ReleaseFast, wall per `kerf export --format png --sheet` incl. process start: 50-66 ms for all six views
(svg 5-22 ms); wasm ReleaseSmall in node: 184 ms for truss A sheet. wasm grows ~30 KB (662,838 -> 692,209 B raw).
Goldens: `tests/golden/<detail>/{A,B}.raster.png` (bare) and `{A,B}-sheet.raster.png`, byte-checked by `check_golden.sh`,
reviewed by eye against `A.png` (chromium render of the SVG). Total 0.95 MB for 12 files (60-105 KB each).
Tests: `tests.zig` (signature/IHDR/clamp/determinism, white corners + ink inside the CMU hatch region), `raster.zig`
(coverage and even-odd fill), `png.zig` (round trips; `png.decode` is a verification decoder). Gaps: no color; no
text knock-out of hatch (same as SVG).

## Cross-engine comparison with engines/rust (`compare_drawings.py`, tol 1e-3")

Last run against Rust commit 98ce04b (before its parity fixes for SPEC 16 "Parity decisions"). Item counts
zig vs rust: truss A 136/150, B 154/161; slab A 91/90, B 40/44; beam A 79/79, B 54/52. The beam A detail is
down to 3 differences. Residuals, by cause:

1. **Not yet landed in Rust** (decided in SPEC 16, implemented here): note landing tie-break (`n_bb`,
   `n_ftg_bars`, `n_jack` flip columns), break-line zigzag symbol and merged-group extents, hatch phase
   (slab 435 vs 422 lines, cmu 46 vs 43), vapor pen (`vapor` here, `cut` there), metal-only thin rule.
2. **Stroke chaining**: I join touching same-pen strokes into long polylines (e.g. one 18-vertex `cmu` outline);
   Rust emits 15 two-vertex segments. Geometry is identical, only the path structure differs.
3. **Anchor bolt**: same SPEC 16 geometry, but my bolt is clipped/split into closed vs open paths differently
   (2 vs 4 paths) because of the dedupe order against the nut/washer.
4. **Truss beyond outline**: the heel plate (hidden pen) closes in mine, stays open in Rust (1 vs 2 HIDN paths); chord
   edges are split at different occluder intersections.
5. **Title placement**: `title:*` items differ by 0.7" in model units at 1"=1' (lowest-annotation rule).
6. **Iso**: Rust 160 items vs 154; arc tessellation (0.22 rad / 0.004") and silhouette edge rules differ slightly.
7. **Summary ft-in** now agrees (`3'-0 1/4"`).

Re-run after the Rust agent lands its parity commit: build `engines/rust` release, then
`for each doc/view: kerf drawing ... -o x.json; tools/zig-engine/compare_drawings.py zig.json x.json -v`.

## SPEC ISSUES (what I chose, why)

- Note wrap: `wrap_chars` is a *character count* (greedy, words longer than a line split), matching the Rust engine.
  Text widths still come from stroke-font advances. (SPEC says "using advance widths"; character count is the only
  reading both engines can reproduce.)
- `lumber.plies` stack along the *thickness* direction (X for upright, Y for flat, in-plane for a narrow face,
  Z for a wide face of a run x/y member).
- Arc flattening for iso/mesh uses the finer of 0.22 rad and 0.004" sagitta (SPEC says 2 degrees; 2 degrees on a
  #5 bar is 180 faces). Vertical edges at smooth vertices only draw where facing changes (silhouettes).
- Iso projection is the isometric *drawing* projection (axes at true length): u = (sz x - sx z) cos30,
  v = y - (sx x + sz z) sin30 (SPEC's illustrative formula has the wrong sign for a view from above).
- Section X marks and ply lines use the `beyond` pen. Layers per pen: cut/profile/membrane/vapor -> cut layer,
  rebar/steel -> steel layer (same as Rust); DXF entities also carry their own lineweight.
- `src` of array instances is `id#k`; instances created by `rebar.place` (several bars) keep the plain id.
- Vapor retarder lines are drawn at least 0.03 paper inch off the host edge so the dashes stay visible.
- Truss standard heel: chords touch only at the heel point, leaving a wedge (as the spec geometry gives).
- `W_NEAR_MISS` only considers axis-aligned `lumber` components (the members that can take `until`); component
  bounding boxes are compared, and a pair is skipped when a third box intersects the gap. Panels (ceiling,
  sheathing) are excluded because the reference details deliberately leave small gaps there.
- `W_FLOATING` / `W_UNTREATED_CONTACT` use 3D proximity (plan distance and z gap both <= 1/32").
- `W_COVER` host = the concrete/cmu outline containing the bar (not the drawn sub-prisms); path bars skip host faces
  roughly perpendicular to the bar (its ends).
- Canonical `fmt` key order: common fields first (`id type label material at rotate slope mirror z array embedded
  visible`), then type params in catalog order (the reference docs are reordered by `fmt`).
- Glyph folding: dashes -> `-`, smart quotes, x-sign, vulgar fractions, other non-ASCII -> `?` with `I_GLYPH`.

## REQUESTS

- (spec) decide `3'-0 1/4"` vs `3'-1/4"` for feet with zero whole inches and a fraction; the two engines differ.
- (rust) pen for the thin-region outline and break-line symbol (see differences 2-3).

## Keynote mode

`notes.mode: "keynote"` (style): the column shows a hexagonal tag with the note number (note order) at the same
placement/leader as leader mode; the full texts go to a `KEYNOTES` legend block right of the drawing, top aligned
with the crop. Verified by rendering the truss detail with a keynote style.

## Fuzzing

`tools/zig-engine/fuzz.py [N] [seed]` mutates the reference docs (drop/replace/scale values) and runs check/drawing/export/mesh/fmt/inspect
on the safety-checked debug CLI: 2,300+ mutations x 3 calls, 0 crashes.

## Known gaps

- `solid` / `polygon` profiles with holes are not supported (profiles are single loops; the mesh ignores holes).
- Hatch under dimension/label text is knocked out in SVG/PDF only (DXF HATCH uses the pattern, not lines).
- PDF content streams are uncompressed (about 150 KB per sheet).
