# engines/zig: Kerf engine in Zig 0.16

Status: feature complete for the v0.1 contract (SPEC 1-17), including keynote note mode.
Everything is std-only Zig 0.16; one source tree builds the importable module `kerf`, the CLI and the
wasm32-freestanding ABI module. All three reference details export SVG, DXF (zero audit errors), PDF
(vector only) and mesh, with 0 errors / 0 warnings. Goldens are in `tests/golden/<detail>/`.

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

wasm (`tools/size_report.sh`): ReleaseSmall **656,879 B raw / 242,804 gzip / 194,682 brotli**;
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

## Cross-engine comparison with engines/rust (`compare_drawings.py`, tol 1e-3")

After the convergence work (src naming, break-line src `crop`, pen->layer map, stroke chaining, summary
parity) the remaining differences are of these kinds. None is an engine crash; each is a deliberate choice,
an under-specified area, or a stale Rust build:

1. **Anchor bolt geometry**: hook radius/legs, nut and washer sizes differ (spec only gives embed, projection,
   hook length 3"). Affects `anchor_bolt` items and the bolt's extents in the summary.
2. **Break lines**: mine are zigzag (6 vertices, SPEC 16 tail); the Rust build I compared against emits 2-vertex
   straight segments. Positions match; the symbol differs.
3. **Thin cut regions**: SPEC 16 says solid fill + outline in the material pen. I use the `steel` pen for the
   CS16 strap; Rust draws the outline with pen `frame` (layer S-ANNO-TTLB). I also apply the rule to
   thin non-metal panels (3/8" soffit, 7/16" OSB at 1"=1'-0"), Rust does not.
4. **Note landing for tied areas**: equal-area candidates (two bars, several grout cells) resolve to the *first*
   max in mine; Rust differs in a few cases (`n_bb`, `n_cmu`, `n_jack`), which flips left/right columns.
5. **Notes column x**: the column offset differs by 1.375" (0.11" paper) in `flush-beam-strap` A because the
   extents of dimension text boxes are accumulated slightly differently; everything in the column shifts together.
6. **Hatch line counts** differ by ~5-15% (e.g. `slab` 435 vs 422): pattern phase/clipping at region edges.
7. **Vapor retarder pen**: mine `vapor` (dashed); the Rust build used `cut` for it in one drawing.
8. **Iso scale for NTS**: when notes + title do not fit the sheet frame, mine grows the internal fit factor in
   0.5 steps until they do (no W_VIEW_FIT); Rust keeps the first factor.
9. **Summary ft-in**: mine prints `3'-0 1/4"` (architectural, SPEC 1); Rust prints `3'-1/4"`.

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
