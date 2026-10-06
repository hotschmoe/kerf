# engines/zig: Kerf engine in Zig 0.16

Status: feature complete for the v0.1 contract (SPEC 1-17), including keynote note mode.
Everything is std-only Zig 0.16; one source tree builds the importable module `kerf`, the CLI and the
wasm32-freestanding ABI module. All three reference details export SVG, DXF (zero audit errors), PDF
(vector only), PNG and mesh, with 0 errors / 0 warnings. Goldens are in `tests/golden/<detail>/`.

Also here: `kerf serve` (the local workspace server: embedded web UI, folder API, SSE, LLM proxy, agent bridge) and the
op log (`<file>.log.jsonl`). See "kerf serve" below.

Zig: `~/tools/zig-aarch64-linux-0.16.0/zig` (0.16.0). No third-party dependencies.

## Build / run / test

```sh
cd engines/zig
zig build                    # CLI -> zig-out/bin/kerf
zig build test --summary all # unit + reference-document + leak tests (std.testing.allocator)
zig build wasm               # -> dist/kerf.wasm (wasm32-freestanding, ReleaseSmall, zero imports)
zig build -Dui=../../apps/web/dist-serve -Doptimize=ReleaseSmall   # CLI with the web UI embedded (needs `npm run build:serve` in apps/web first)
node tests/serve_smoke.mjs   # integration test of `kerf serve` (215 checks; starts the built zig-out/bin/kerf on temp folders)
zig build -Dtarget=x86_64-windows-gnu -Doptimize=ReleaseSmall   # cross-compiles (also aarch64-windows-gnu, *-linux-musl, *-macos)
zig build wasm -Dwasm-optimize=ReleaseFast   # speed comparison
zig build wasm -Dwasm-strip=false            # keeps names for `twiggy top dist/kerf.wasm`
zig build -Doptimize=ReleaseSafe -Dstrip=false   # CLI with debug info (default: stripped outside Debug, 2.5 MB instead of 16 MB)
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

Read this before changing the engine: it says where things live and how to extend them. (REVIEW.md has the reasoning; this is the
map. All of it is checked by the compiler or by a test where that was possible.)

### Data flow of one `kerf.call` (one arena per call, results copied out once)

```
api.call(name, json)            api.Fn enum -> one function per API function (check, apply, drawing, export, mesh, ...)
  json.parse                    order-preserving Value; numbers finite and <= 9e15; limits.zig caps everything else
  style.load                    embedded style (spec/styles) merged with the user's, validated once (E_STYLE)
  compile.compile               document -> Scene: readSettings, componentItems, readComponents, placementOrder, then per component
                                  placeComponent = parseAt, resolveAngle, builders.build (typed Params), resolveZ, resolveArray,
                                  instanceTransforms, worldPrisms (each prism gets its material's style.Role)
  validate.run / lint / coverage  checks on the Scene and the document (warnings and errors as diagnostics)
  drawview.build                view -> Drawing (the IR every exporter reads): section.Section or iso.build, then annot.annotate
  svg / dxf / pdf / raster+png / mesh   exporters over the Drawing (or the Scene for mesh)
```

### Module map

- **Foundation**: `json` (parser/writer, Kerf number format) | `units` (lengths, scales, slopes, ft-in) | `num` (checked float->int) |
  `limits` | `oom` (allocation sensor) | `geom` (exact predicates, bulge arcs) | `clip` `pathclip` `pathgeom` (polygon booleans, path
  clipping, fillets/ribbons) | `hatch` | `font` `textgeom` | `view` (view parsing) | `model` (diagnostics, `Params`, `Prism`, `Built`).
- **Vocabulary (enums, no strings)**: `api.Fn` (API functions) | `catalog.Type` (component types) | `pen.Pen` and `pen.LayerKey` (what the
  drawing code draws with; `Pen.layer()` is an exhaustive switch) | `drawing.Kind` / `view.Kind` | `style.Role` (what a material is) |
  `catalog.Traits` (what a component type is). Code asks `p.role == .soil` or `ty.traits.hardware`, never `std.mem.eql(u8, name, "gravel")`.
- **Components**: `catalog.zig` (prose, parts/anchors/example, traits; the parameter rows are *generated*) | `params.zig` (typed
  parameter parsing and the catalog rows from one struct) | `builders.zig` (registry) + `builders/<type>.zig` (14 files, each has `Params`
  and `build`) + `builders/common.zig` (shared helpers) | `scene.zig` (refs, anchors) | `compile.zig`.
- **Views**: `section.zig` (the `Section` struct: classes, strokes, regions, `build`/`finish`) + `section/{occlusion,strokes,breaks,
  hardware,cut,beyond}.zig` (visibility, dedupe/chaining, crop break lines, thin hardware, cut regions, beyond outlines) | `iso.zig` |
  `drawview.zig` | `crop.zig` | `thinland.zig` | `shape.zig` (visible-region shapes and label points, shared by section and iso).
- **Annotations**: `annot.zig` (`Env`, `annotate`) + `annot/{text,notes,dims,repair,knockout,title}.zig` | `route.zig` (leader router, pure
  geometry) | `lint.zig` (note/dim lints) | `sheet.zig` (title block).
- **Documents**: `schema_fields.zig` (field tables of doc/view/note/dim/...) | `schema.zig` (renders `kerf schema`, the guide, the example doc) |
  `canon.zig` (`fmt`) | `ops.zig` (apply: validate the op, then `applyDoc/Meta/Components/Views`) | `load.zig` (summary, inspect) | `validate.zig`.
- **Exporters**: `drawing.zig` (IR) | `svg.zig` `dxf.zig` `pdf.zig` `raster.zig` `png.zig` | `mesh.zig`.
- **Entry points**: `api.zig` | `kerf.zig` (the importable module) | `wasm.zig` (ABI) | `main.zig` (CLI: `run` parses options and dispatches to
  `cmdVersion/Catalog/Schema/Guide/Init/New/Call/Doc`).

CLI-only files (not in the `kerf` module, never in the wasm): `workspace.zig` (op log, atomic writes, file-name rules, new-doc text) |
`serve.zig` (`Server`: folder scan, edit log, ETag) + `serve/{cli,conn,routes,docs,agent,stream,ui}.zig` (startup and arguments; limits,
deadlines and access rules; the route table and dispatcher; document / agent / SSE+LLM handlers; static UI and CSP) | `http.zig` (HTTP/1.1) |
`events.zig` (SSE hub) | `agents.zig` (agent bridge) | `proxy.zig` (`/api/llm`) | `ui_stub.zig` (empty `ui_assets`; `-Dui=` replaces it).

Import rules: no cycles between "layers" any more (shape.zig is shared by annot and iso; schema_fields by canon, lint and schema; mesh does
not import section). Inside a directory the split files import their parent for its main type (`const Section = @import("../section.zig").Section`)
and the parent re-exports the moved functions as method aliases (`pub const drawCut = cut.drawCut;`), so `self.drawCut(...)` call sites did not change.

### How to add a component type (the recipe)

1. `catalog.zig`: add the tag to `catalog.Type` and an entry to `catalog.entries` at the same position (a comptime check names the entry
   that is out of place). Write `summary`, `parts`, `anchors`, `draws`, `example`; set `traits` if the engine should treat it specially
   (`default_anchor`, `has_pitch`, `hardware`, `reinforcement`, `escape_hatch`, `line_like`, `lengthwise_grain`, `near_miss`, `sheet`,
   `rigid_sheet`, `slope_host`). Set `.params = params.rows(builders.<type>.Params)`.
2. `builders/<type>.zig`: `pub const Params = struct { ... pub const spec = .{ ... }; };` and `pub fn build(ctx: *Ctx) BuildError!?Built`.
   Copy the nearest existing file. Rules for `Params`:
   - field name = JSON key, in the order the catalog and `kerf fmt` should list them;
   - field type picks the parser: `bool`, `[]const u8`, `enum { a, b }` (the tags are the accepted strings, the "must be one of" message is generated),
     `f64` (number; `.len = .any|.pos` in `spec` makes it a length), integers (need `.min`/`.max`), `json.Value` / `?json.Value` (raw, for
     structured members that `build` validates: points, until, cover, place);
   - no default = required; a default or `?T = null` = optional; defaults that depend on other params are `?T = null` and applied in `build`;
   - every field needs a `spec` entry with `.desc` (compile error otherwise); `.def` overrides the generated default text ("required for run x/y"),
     `.hint` extends the "is required" message of a length, `.also` + `.row = false` share one catalog row between keys (`width, height`).
   `build` starts with `const pp = p.parse(Params) orelse return null;` (every problem reported in one pass) or `p.parseAll(Params)` when it
   goes on to check things that do not depend on the failed fields; after that no `.?` unwrapping of parameters is needed. Use
   `p.missing(key, hint)` for a conditionally required key. Shared helpers: `builders/common.zig` (`parsePointList`, `lengthOrUntil`,
   `parseCover`, `materialOk`, `mirrorBuilt`, `zoneRect`, `onePrism`, ...).
3. `builders.zig`: `pub const <type> = @import("builders/<type>.zig");`, a `.<type> => <type>.build(ctx)` arm in `build`, and the struct in `param_structs`.
   The switch and the `param_structs` length are compile-checked against `catalog.Type`.
4. Tests that already cover you: `builders` test (every enum choice appears in the catalog text), `catalog` test (traits name documented
   anchors/parts), the reference documents. Add a document under `tests/docs` if the type needs geometry goldens (`tests/make_golden.sh`).
5. If the type uses a new material, add it to the style (spec/styles) and, if the engine must treat it specially, a `style.Role` (see below).

Adding a parameter to an existing type is one field plus one `spec` entry in its `Params`; the parser, `kerf catalog`, `kerf schema <type>`,
the W_UNKNOWN_KEY list and the canonical key order all follow. Do not edit the catalog text for it.

### Other recipes

- **A material that behaves like steel/soil/wood/...**: give it `"role": "steel"` (any `style.Role` name) in the style JSON; without the key the role
  comes from the name (`style.defaultRole`: what the embedded materials are). New roles go in `style.Role`; a test asserts the table equals the old
  name predicates for every embedded material.
- **An API function**: a tag in `api.Fn`, a `switch` arm in `api.dispatch` (exhaustive), the function; the E_FN list is generated.
- **A pen / layer**: tags in `pen.Pen` / `pen.LayerKey` and the `Pen.layer()` switch; the style must define the pen (`style.pen(.x)`) or drawing falls back.
- **A server route**: a row in `serve/routes.zig` (`endpoints` or `doc_actions`), a `Kind` tag, a body cap in `bodyCap`, the handler in `serve/<area>.zig`
  and an arm in `handleRequest` (a test checks every `Kind` is reachable through the table).
- **A CLI subcommand**: a `cmdX` function in `main.zig` and one line in `run`.

Design points: a component builds *prisms* (profile region with bulges + z range + role flags) in local
coordinates; placement transforms them (translate/rotate/mirror, arrays, z). Section view = exact
2D: beyond outlines are split against occluder boundaries (arcs stay arcs), hatch is clipped
analytically per pattern family, coincident edges are deduped (same-component shared edges draw in
the `beyond` pen). Iso view = planar-face HLR on a uniform grid (SPEC 8.2). Determinism: no hash maps,
no clocks; every output is a pure function of (doc, style).

## Measured numbers

wasm (`tools/size_report.sh`, after refactor batches 4-7): ReleaseSmall **963,851 B raw / 355,588 gzip / 281,466 brotli** (951,291 B before
the batches: the typed `Params` machinery and the catalog row tables cost about 12 KB, the enums saved 2 KB; per batch: 4 -> 949,372, 5 -> 964,314,
6 -> 966,523, 7 -> 966,990, `params.readScalar` -> 963,851). `twiggy top` (`zig build wasm -Dwasm-strip=false`): `builders.build` 68 KB (all builders
inlined into the dispatch), annot 51 KB, iso 36 KB, dispatch 36 KB, clip 29 KB, rodata 132 KB. Older figures (662,838 B) predate v0.1.2-0.1.5.
Zero imports; exports exactly the SPEC 13.1 six. The CLI is built with `-Dstrip` by default outside Debug (ReleaseSafe Linux CLI 16.0 MB -> 2.5 MB;
`-Dstrip=false` keeps debug info).

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

- serve: `POST .../apply` with ops the engine rejects returns HTTP 200 with `ok:false` + diagnostics (the engine's own output, nothing written), not
  4xx: SERVE.md says errors are `{error:{code,message}}` but the engine answer for bad ops is a normal result the UI renders. Real errors
  (bad JSON, missing ops, no such file, conflict) are 4xx with the error shape; a 409 conflict also carries `etag`.
- serve: `doc_changed.who` is derived from the newest log line seen in the same poll tick; if the CLI's doc write and log append straddle two ticks it is absent
  (the `log` event still follows). Designer writes always carry `who`.
- serve: agent argv templates, session-id capture and the `{resume}` element are my design (SERVE.md only says "same template shape"); documented in NOTES.
- serve: `Bash(kerf:*)` (SERVE.md) is the legacy permission syntax; Claude Code's docs now write `Bash(kerf *)`. Both are passed.
- serve: Grok `--always-approve` and Codex `--sandbox workspace-write` are the headless permission choices; SERVE.md left them open.

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

## Engine hardening (REVIEW batches 0 engine part, 1, 2; status table in REVIEW.md section 11)

Rules that now hold for every input (document, style, ops):

- **No raw `@intFromFloat` on untrusted data**: use `num.toInt` / `num.toIntClamped`. Numbers are validated where they enter: `json.parse`
  rejects non-finite and > 9e15 numbers, invalid UTF-8 and leading zeros; `units.parseFinite` is the one float parser for strings (no
  `nan`/`inf`); lengths are limited to +-`limits.max_coord_in` (1e6 in), scales to 0.1..10000, polygon bulges to 0 or 1e-6..1e3; `Style`
  is validated once in `style.loadWhy` (E_STYLE lists every bad key with its range and default).
- **`limits.zig`** holds the global caps (input bytes, components, instances, annotations per view, views, points, note characters). Each
  is reported as `E_LIMIT` with found / maximum / what to do. The server may quote `kerf.limits`.
- **Layout effort is bounded**: `route.Work` (units of penalty evaluations) is shared by every pass and trial of one view build
  (`drawview.buildFromScene`, `resolveSpec`); when it is spent the notes keep the SPEC 6.3 + de-crossing layout and one aggregate
  W_LEADER_HIT says so. Fix proposals are computed only for the first `route.max_fix_hits` hits (the ones `reportHits` prints).
- **Out of memory is an error**: `kerf.call` wraps its arena in `oom.Sensor`; a refused allocation anywhere becomes `error.OutOfMemory`
  (CLI `kerf: OutOfMemory`, wasm `E_OOM`). New code may keep using `catch "?"` in message builders.
- **No silent defaults**: malformed offsets, `place`/`array`/`recess`/`cover` members and dim/label offsets are E_PARAM with a fix hint;
  use `model.offsetPairOrDiag`, `model.lengthOrDiag`, `Params.fieldLen/fieldInt/fieldChoice/offsetPair` for new optional members.
- **One number formatter**: `json.fmtFixed(buf, x, decimals)` (svg/pdf 3, json 4, dxf and bulges 6); never `{d}` of a float into a fixed buffer.
- wasm: `zig build wasm -Dwasm-optimize=ReleaseSafe` builds (`std.debug.no_panic`) and must pass `wasm_golden.mjs`; CI runs it. Size
  after the hardening: 951,291 B raw (was 902,131; gzip 353,965).
- CLI (`main.zig`): `apply -w` / `fmt -w` hold `workspace.DocLock` (shared with `kerf serve`) and write atomically; `apply -w --if-match
  <etag>` refuses a stale document (E_STALE); user strings in the engine input go through the JSON escaper; `--px` must be a number.
  The server-agent REQUESTS about these are done (the usage text lists the new `kerf serve` flags).
- Tests: `src/hostile_tests.zig` (appendix-B table, OOM sweep over every allocation index, SAF-4 table), `tools/zig-engine/fuzz.py`
  (seeded with the hostile values and hostile styles; a hang counts as a failure).

Done since (refactor batches 4-7, below): typed params, enums for functions/pens/types/kinds, domain roles and traits, the file splits.
Not done (see REVIEW section 11): performance/size work, exporter tidy (DXF handles, dash clipping), in-tree `std.testing.fuzz`,
`clip.boolean` diff pass, LAY-3..10, the remaining long functions (`dxf.render`, `iso.build`, `style.fromValue`, `clip.boolean`).

## Refactor batches 4-7 (typed vocabulary, typed params, roles, file splits; status table in REVIEW.md section 11)

Every commit kept `zig build test`, `tests/check_golden.sh` (byte-identical), `wasm_golden.mjs` 24/24, `cli_ergonomics.mjs`, `serve_smoke.mjs` and `zig fmt --check`
green; the end of each batch also ran the ReleaseSafe wasm golden and `fuzz.py 300 1` (0 crashes). Two extra safety nets were used while refactoring and are
worth reusing: a *message corpus* (every param of every component type mutated through `kerf call check`, 5,800 diagnostics + summaries compared before/after each
conversion) and an *ops corpus* (156 valid and invalid ops through `kerf call apply`, compared against the pre-refactor binary): both identical except for the
deliberate changes listed here.

Deliberate behaviour changes (all in error paths or documentation, none in any output of a valid document):
- `lumber` no longer reads `material`: the key was never in the catalog, so a document using it already got an unknown-param E_PARAM and never built.
- A material's `"pen"` in a user style must name an engine pen (`pen.Pen`); before, an unknown name was silently ignored (E_STYLE now, with the pen list).
  A pen named after a fill material was picked up implicitly for every fill material (only `steel` and `rebar` ever were); now `style.Role` decides.
- `Params` structs validate every declared key for every shape (`concrete` with `shape: polygon` and a malformed `slab_thickness` is now E_PARAM; before the
  key was ignored). The catalog text of membrane `side` now names both choices (a new test found it).
- Component params are all parsed in one pass; when a *required-by-other-param* check follows a failed field, `parseAll` keeps collecting (panel: a bad thickness
  does not hide a bad length) but other builders stop at the first failing struct (they used to continue for a few more independent reads).

Added tests: `params.zig` (parse messages, rows), `builders` (every enum choice is documented), `catalog` (traits name documented anchors/parts), `style` (role table equals
the old predicates for every embedded material; `role` and `pen` validation), `compile` (stages: placement order, cycle report, bad ids/types), `serve/routes` (every handler
kind reachable through the table), `pen` (every pen has a layer).

## REQUESTS

- (orchestrator, `spec/SPEC.md` section 7 and `spec/styles/kerf-standard.kerfstyle.json`) materials accept an optional `"role"` (one of generic, soil, wood, masonry,
  grout, steel, rebar, sheet_metal, vapor_retarder, void; unknown values are E_STYLE). The engine derives it from the material name when the key is absent, so the
  embedded style works unchanged; adding `"role"` to the embedded materials (soil: earth gravel sand compacted_fill; wood: wood wood_engineered wood_board; masonry: concrete cmu mortar;
  grout; steel; rebar; sheet_metal: aluminum flashing_membrane; vapor_retarder; void) would make the style the source of truth. A material `"pen"` must now be one of the
  engine's pens (cut profile beyond hidden hatch rebar steel membrane vapor anno dim break title frame).
- (DONE by the engine hardening pass: `main.zig` takes the DocLock, `--if-match`, atomic `fmt -w`, escaped `--view`, usage text) (engine agent, `main.zig`, V-11/V-16) `kerf apply -w`: wrap read -> apply -> `writeFileAtomic` -> log append in `workspace.DocLock.acquire(io, dir, logPath)` ... `lk.appendLine(io, line)`
  (an advisory lock shared with `kerf serve`) and add `--if-match <etag>` (compare with `"<mtime_ms>-<size>-<wyhash64 hex16 of the bytes>"`, the server's format: see
  `Server.etagOf`; or just the content hash) so 40 parallel writers cannot lose edits; `kerf fmt -w` should use `writeFileAtomic` and log; escape `--view`
  and other raw `{s}` interpolations into engine JSON with `json.writeString`; mention `--trust-agents`, `--token-file`, `KERF_TOKEN` in the `kerf serve` usage text.
- (orchestrator, `spec/SERVE.md`, `README.md`, `spec/llm/cli-guide.md`) a token is now the default on loopback too (`--no-token` opts out; first banner line carries `?token=`);
  `ETag` is `"<mtime_ms>-<size>-<hash>"` (opaque to clients) and list rows have `etag`; `.kerf/agents.json` agents need `--trust-agents` or an interactive yes;
  per-route body caps (413), 429/503 on caps, new flags `--trust-agents --token-file --agent-timeout --llm-timeout`, env `KERF_TOKEN`; the exit event can carry `timeout:true`;
  `/api/info` agents carry `source` and `untrusted`; the server sends CSP/X-Frame-Options/Referrer-Policy.
- (orchestrator, `spec/llm/cli-guide.md`) wording for the log: add under the workflow block
  "`kerf apply ... -w --why "one line: what and why"` records your edit in the folder's op log (`<file>.log.jsonl`); the designer sees it in the
  web UI as a LOCAL AGENT card with your reason. Always pass `--why`, written for the designer (for example `--why "Add HETA20 anchors at 16in o.c."`)."
- (orchestrator, `spec/SERVE.md`) please state: `if_match` accepts the ETag with or without quotes; `apply` with invalid ops is HTTP 200 + `ok:false`
  (writes nothing); `GET /api/info` without a token returns only `{version, token_required, authenticated:false}`; `/api/agent/run` takes
  `images`; agent templates use `{message} {session_id} {dir} {file}` and a `{resume}` element (see the table in NOTES); the server also
  scans the folder right before an agent's `exit` event.

- (spec) decide `3'-0 1/4"` vs `3'-1/4"` for feet with zero whole inches and a fraction; the two engines differ.
- (rust) pen for the thin-region outline and break-line symbol (see differences 2-3).

## Keynote mode

`notes.mode: "keynote"` (style): the column shows a hexagonal tag with the note number (note order) at the same
placement/leader as leader mode; the full texts go to a `KEYNOTES` legend block right of the drawing, top aligned
with the crop. Verified by rendering the truss detail with a keynote style.

## Fuzzing

`tools/zig-engine/fuzz.py [N] [seed]` mutates the reference docs (drop/replace/scale values) and runs check/drawing/export/mesh/fmt/inspect
on the safety-checked debug CLI: 2,300+ mutations x 3 calls, 0 crashes.

## kerf serve (spec/SERVE.md)

```sh
kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --token-file F | --no-token] [--allow-origin URL]
           [--trust-agents] [--agent-timeout S] [--llm-timeout S] [--timeouts-ms IDLE,HEAD,WRITE]     # `kerf serve --help` has the text
```
- First stdout line is `kerf serve: http://127.0.0.1:<port>/?token=<32 hex>  (dir <abs>)` (scripts parse it; `--port 0` picks a free port).
  A token is the DEFAULT, also on loopback (V-7; generated with `io.randomSecure`, fail closed); `--token-file F` / `KERF_TOKEN` fix it without
  showing it in `ps`, `--token T` still works (with a stderr note), `--no-token` opts out (banner says so on a LAN bind; on loopback the
  Host/Origin checks then apply as before). `--host 0.0.0.0` also prints `LAN:  http://<lan-ip>:7700/?token=<32 hex>`. The LAN IP comes from a connected UDP socket toward 192.0.2.1 (nothing is sent).
- Exit 1 with a hint when the port is busy (std sets SO_REUSEPORT with SO_REUSEADDR, which would let a second server share the port
  silently, so it probes with a connect first) or `--dir` cannot be opened.
- Everything in SERVE.md is implemented: `/api/info docs docs/:file apply log export events llm agent/run agent/stop`.
  Not in the spec, added: `GET /api/info` also returns `authenticated` and `active_run` (`{run_id, agent}` or null) and, without a valid token,
  just `{version, token_required:true, authenticated:false}` (so the UI can ask for the token); `POST /api/agent/run` also takes
  `images:[{name, data_base64}]` (saved in `<dir>/.kerf/attachments/`, absolute paths appended to the message; web agent's request);
  `GET .../export` also takes `inline=1` (Content-Disposition inline).
- Concurrency: `std.Io.Threaded` (the default `init.io`); every connection is a `Group.concurrent` task (one OS thread, 16 MB
  virtual stack), so SSE streams never block other requests. The poller, agent supervisors and agent output pumps are tasks too.
  Request bodies: Content-Length or chunked, `Expect: 100-continue`; per-route caps (see "Server hardening"). Keep-alive on, with idle,
  head, body and write deadlines enforced by a watchdog task that `shutdown()`s sockets past their deadline (std has no read timeout).
- Folder scan (`serve.zig scan`): 500 ms poller, and every `GET /api/docs` scans first. Per `*.kerf.json` it tracks (mtime, size);
  per `<file>.log.jsonl` the byte offset already reported. Emits `doc_added/doc_changed/doc_removed` and one `log` event per new
  complete valid-JSON line (a torn last line waits for its newline). `doc_changed.who` = `who` of the newest log line seen in the same
  tick (CLI writes the doc first and the log second, so it is sometimes absent). Designer writes update the scan state themselves, so
  the poller never reports them twice (tested). A document first seen after startup replays at most its newest 200 log lines.
  Pings every 15 s. SSE frames live in a bounded ring (512 frames / 8 MiB); a client that falls further behind silently skips.
- Writes: `apply` serializes writers (`write_mu`), re-reads the document, checks `if_match` (the UI's quoted ETag and the bare value both
  work), calls the engine `apply`, then writes with temp file + rename and appends the log. The engine `actor` is `designer` only when
  `actor:"designer"`; any other actor (`llm`, `agent`) is an LLM edit for the engine (citations reset). `ok:false` (invalid ops) is HTTP 200 with
  the engine's diagnostics and writes nothing (see SPEC ISSUES). The response is the engine output plus `etag` (also the `ETag` header).
- Security (the server can edit files and start programs): no CORS by default; every `/api` request with an `Origin` that is not the
  server's own origin gets 403 (`--allow-origin` whitelists one dev origin). Without a token on a loopback bind the `Host` header must be
  localhost/127.0.0.1/[::1] (DNS rebinding). Token compare is constant time; it is accepted as `Authorization: Bearer` or `?token=`.
  Static files are served without a token so the page can read `?token=`. File names are `[A-Za-z0-9._ ()-]+.kerf.json`, no separators.
- `/api/llm` rules (`proxy.zig plan`, unit tested): provider in anthropic|openai|gemini|xai|openrouter|custom with defaults
  (`https://api.anthropic.com`, `https://api.openai.com`, `https://generativelanguage.googleapis.com`, `https://api.x.ai`,
  `https://openrouter.ai/api`); `base_url` overrides (custom requires it); URL = base_url + `/path` (path must start with a single `/`,
  no spaces/backslashes/`://`); no userinfo, no fragment. Non-custom providers: https only AND a public DNS name (no IP literals, no
  localhost/LAN/.local/single-label, no numeric-looking hosts such as `2130706433`). `custom`: https anywhere, http only for
  localhost/127/10/172.16-31/192.168/169.254/CGNAT/::1/fc00::/7/fe80::/10/`.local`/single-label hosts. `headers` values must not contain
  CR/LF; host, content-length, connection, transfer-encoding, accept-encoding, te, upgrade, expect, proxy-*, forwarded are dropped.
  `body`: object -> compact JSON, string -> raw. Redirects are not followed. Response: status, content-type, content-encoding,
  retry-after, request-id, x-ratelimit-*, anthropic-ratelimit-*, openai-* pass through; the body is streamed chunk by chunk (tested: first
  chunk 0.4 s before the end) and the connection closes at the end. Upstream failure before any byte: 502 `E_UPSTREAM`.
  Not covered: a public DNS name that resolves to a private address. Keys are never stored or logged (tested: they appear nowhere in SSE).
  TLS is `std.http.Client` (std.crypto.tls, system cert bundle scanned on first https use); only the http path is exercised in tests (no network).
- Agent bridge (`agents.zig`): templates (built-in + `<dir>/.kerf/agents.json`, which is executed only when trusted: `--trust-agents` or a yes
  at the interactive prompt; otherwise listed `untrusted` and never detected or started; a trusted entry with a built-in id replaces it) with `{message} {session_id} {dir}
  {file}` and a `{resume}` element; spawn with `std.process.spawn`, cwd = `--dir`, stdin ignored, stdout/stderr piped, env = server env
  with the server binary's directory prepended to PATH (so `kerf` resolves even when it is not installed) and `KERF_ACTOR=agent`.
  stdout lines -> SSE `agent {run_id, event}`: JSON object lines verbatim, anything else `{type:"text", text}`; stderr lines ->
  `{type:"stderr", text}`; over-long (>1 MiB) lines -> `{type:"truncated"}`. `session_id` = latest top-level `session_id` / `sessionId` /
  `thread_id` string seen in any JSON line. At the end: the folder is scanned once (so the agent's `doc_changed`/`log` events come BEFORE
  `exit`), then `{type:"exit", code, session_id?, stopped?, timeout?}`. The agent runs in its own process group (Windows: a job object with
  KILL_ON_JOB_CLOSE); stop / timeout / server shutdown send SIGTERM to the whole group, SIGKILL after 3 s; the supervisor reaps the agent
  while the pumps still read (no zombies, no wedge when a grandchild keeps the pipe). One run at a time (409 `E_BUSY`), reserved atomically
  under a mutex before the process is created; the slot is released before the `exit` event is published. Detection (`<cmd> --version`, 8 s timeout, cached 30 s, prefetched at start) fills `available/version/reason`.
  The events race the HTTP response of `/api/agent/run` (first events can arrive before the client has the run_id): buffer by run_id.
- Windows: everything is std (Io.Threaded netListen/netAccept on AFD, Child via CreateProcess) plus five `kernel32` job-object calls declared in
  `agents.zig` (spawn suspended -> assign to a KILL_ON_JOB_CLOSE job -> resume; stop = TerminateJobObject; if the job cannot be created the run
  degrades to NtTerminateProcess of the agent alone, grandchildren may survive). There is no SIGTERM stage on Windows (stop is a hard kill), no
  advisory lock on the op log (`DocLock` is a no-op there; the server still serialises its own writes), no directory fsync. It compiles for
  x86_64/aarch64-windows-gnu (CI cross-compiles it) but could NOT be run here (wine32 missing, aarch64 host). `claude.cmd`-style shims are not resolved by
  CreateProcess; the official `claude.exe`/`grok.exe`/`codex.exe` work, others can be wrapped via `.kerf/agents.json` (`cmd /c ...`).

### Server hardening (REVIEW section 7 / refactor batch 3: V-1..V-16)

Every item has a regression test in `tests/serve_smoke.mjs` (raw sockets for the HTTP ones; each marked `V-n:`) and/or a unit test.
- **V-1/V-9 (`http.zig`)**: chunk sizes are at most 8 hex digits and compared to the remaining cap before any addition; bad chunks, unbounded trailers
  and short bodies are 400. `parseHead` rejects bare CR/LF, control bytes, obs-fold, whitespace in names or before the colon, spaces in the target,
  duplicate/conflicting `Content-Length`/`Transfer-Encoding`/`Host`, CL+TE, non-decimal lengths, > 100 headers.
- **V-2**: see "Agent bridge". `GET /api/info` lists workspace agents with `source:"workspace"`, `untrusted:true`, `available:false` and the reason;
  running one is 409 `E_UNTRUSTED`. Built-in detection is `<binary> --version` for claude/grok/codex/pi only (unit-tested), run with the server's cwd
  (not the served folder). There is no per-folder remembered allow-list (yet): the interactive prompt asks every start.
- **V-3**: slot reserved under `mu`; process group / job object; watchdog (`--agent-timeout`, default 1800 s, 0 = none; timeout = TERM, KILL after 3 s,
  `exit` event gets `timeout:true`); SIGINT/SIGTERM/SIGHUP stop the active run and exit (POSIX; a SIGKILLed server still orphans the agent on POSIX,
  Windows kills it via the job). Detection is single-flight and no longer leaks. `KERF_TOKEN` is removed from the agent's environment.
- **V-4/V-5/V-13 (`serve.zig`)**: at most 256 connections (503 beyond), 32 SSE streams, 8 concurrent LLM calls (429), 1000 requests per connection;
  watchdog deadlines: idle 30 s, request head 10 s (from the first byte), body 3x head + 1 s per 512 KiB declared, handler 120 s, SSE write 20 s
  (`--timeouts-ms IDLE,HEAD,WRITE` for tuning/tests). The body buffer grows as data arrives. `handleRequest` routes (`routeOf`, pure and unit-tested),
  authenticates, then reads the body with a per-route cap: stop 4 KiB, create 64 KiB, apply 16 MiB, llm 32 MiB, agent/run 64 MiB, GET routes 0 (a body
  is 400). Everything answered before the body was read closes the connection and never sends `100 Continue`. A log line over 4 MiB is skipped
  instead of re-read every tick; `apiList` runs the engine `check` outside `scan_mu`; SSE data lines never contain CR/LF.
- **V-6**: message <= 100,000 bytes (400 `E_INPUT`); an OS "argument too long" at spawn is 400, not 500.
- **V-7/V-8**: token by default (see the first bullet), `randomSecure`, SHA-256-then-`timing_safe.eql` compare; every response carries
  `X-Content-Type-Options`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, and a CSP: API/exports
  `default-src 'none'; style-src 'unsafe-inline'; img-src data:; frame-ancestors 'none'`, the UI `default-src 'self'; script-src 'self'
  'wasm-unsafe-eval' 'sha256-<each inline script of the embedded index.html, computed at startup>'; style-src 'self' 'unsafe-inline'; img-src 'self' data:
  blob:; connect-src 'self' data: blob:; worker-src 'self' blob:; frame-ancestors 'none'; ...`.
- **V-10**: symlinks are never served, listed or written through: scan and `readDoc` use no-follow; the op log, `.kerf/agents.json`, `.kerf/attachments`
  refuse symlinks (attachments are created exclusively; `.kerf` and `.kerf/attachments` must be real directories).
- **V-11 (`workspace.zig`)**: `writeFileAtomic` fsyncs the data and the directory; `appendLine` and `DocLock` take an exclusive `flock` on the log
  file (POSIX) and fsync; the server holds `DocLock` across read-modify-write of `apply`. ETag = `"<mtime_ms>-<size>-<wyhash of the bytes>"` (GET, list
  rows carry the same `etag`). `if_match` of a non-string is 400. Still open (CLI, engine agent's files): `kerf apply -w --if-match` and taking
  `DocLock` around the CLI's read-modify-write (the 40 parallel appliers now leave intact log lines but only one edit survives), `kerf fmt -w` non-atomic.
- **V-12**: LLM calls run in a task watched by the handler: `--llm-timeout S` (first byte and gaps; default 300 s / 120 s), 30 min total, 256 MiB;
  stalls are cancelled (502 `E_UPSTREAM` before the head, a cut stream after). `custom` still reaches any host by design (token-gated).
- **V-14/V-15/V-16**: `px` printed as the parsed integer; attachments older than 24 h are removed (at start and on each save); the pump drops a line on OOM
  instead of truncating; `cors()` reports OOM; Windows device names (`CON`, `NUL`, `COM1`...) and stems ending in dot/space are not document names;
  HEAD answers with the real Content-Length. Not done: error logging for `publish`/`scanLocked` OOM (still best effort), `serveConn`'s silent close on
  read errors, `main.zig` option parsing and JSON injection through `--view` (engine agent's file), a comptime route table for the handlers.

### Op log
`<file>.log.jsonl` next to the document, one JSON object per line: `{ts, who, tool, why, ops, changed, summary_head}` (spec/SERVE.md).
CLI: `kerf apply ... -w [--why "..."]` (who = `$KERF_ACTOR` or `agent`, tool `kerf-cli`) and `kerf new` (a `create` entry); failed applies
write nothing and log nothing. `-w` and `new` now write atomically (temp + rename). Server: `who` = the request's `actor`, tool `kerf-serve`.
Timestamps (UTC, seconds) exist only in the log.

### Agent CLI research (docs + each tool's own `--help` on this machine, 2026-10-06)
| agent | headless command the bridge runs | session | events | sources |
|---|---|---|---|---|
| Claude Code `claude` (2.1.289) | `claude -p <msg> --output-format stream-json --verbose [--resume <id>] --permission-mode acceptEdits --allowedTools "Bash(kerf:*)" "Bash(kerf *)" Read Write Edit` | `--resume <session-id>` (also `--continue`, `--session-id <uuid>`, `--fork-session`) | NDJSON; first `{"type":"system","subtype":"init","session_id",...}`, `assistant`/`user` messages, last line `{"type":"result","session_id","total_cost_usd",...}`; `--include-partial-messages` adds `stream_event` deltas; `stream-json` needs `--verbose` with `-p` | code.claude.com/docs/en/headless, /en/cli-reference |
| Grok Build `grok` (1.0.41) | `grok -p <msg> --output-format streaming-json --always-approve --cwd <dir> [--resume <id>]` | `-r/--resume <id or title>`, `-c`; `-s/--session-id <uuid>` only creates NEW sessions | `--output-format plain\|json\|streaming-json\|streaming-messages-json`; streaming-json = ACP updates: `available_commands`, `thought`, `tool_call`, `tool_call_update`, `text` (`{"type":"text","data":"token"}`), `usage`, `end` (`stopReason`, `sessionId`), `error`; exit codes 0/1/130/143 | docs.x.ai/build/cli/headless-scripting, github.com/xai-org/grok-build (docs/user-guide/14-headless-mode.md); verified live with `grok -p hi --output-format streaming-json --always-approve` |
| Codex CLI `codex` (0.157.1) | `codex exec --sandbox workspace-write --skip-git-repo-check --cd <dir> --json [resume <id>] <msg>` | `codex exec resume <SESSION_ID> "<prompt>"` (or `--last`); `resume` does NOT accept `--sandbox/--cd`, so shared options go before it | `--json` JSONL: `thread.started {thread_id}`, `turn.started`, `item.started/completed`, `turn.completed`, `error`; `--full-auto` is deprecated for `--sandbox workspace-write` | developers.openai.com/codex/cli/reference (redirects to learn.chatgpt.com/docs/developer-commands), `codex exec --help`, `codex exec resume --help` |

Verified live through the bridge on this machine (the three CLIs are installed here): a trivial prompt and then a `session_id` resume for each of
grok 1.0.41, claude 2.1.289 and codex-cli 0.157.1 all ran to `exit` code 0 with the session id captured from `end.sessionId`, `result.session_id`
and `thread.started.thread_id`, and the resumed run answered from the earlier turn. (codex prints `Reading additional input from stdin...`
on stderr when stdin is /dev/null; it arrives as a `{type:"stderr"}` event.)

Choices: Claude gets only Bash(kerf), Read, Write, Edit pre-approved plus `acceptEdits` (no prompts can be answered headless; `--bare` is not
used because it ignores the subscription login and CLAUDE.md, which is where `kerf init` puts the "run kerf guide" instruction; both the
legacy `Bash(kerf:*)` and the current `Bash(kerf *)` rule syntax are passed). Grok has no per-tool allowlist semantics we could verify for
Bash, so `--always-approve` (= bypassPermissions) is used: the agent can run any command in the library folder, as it could in a terminal.
Codex runs in the `workspace-write` sandbox (writes confined to the folder; it can run `kerf`). Users who want tighter control override
the template in `<dir>/.kerf/agents.json`.

### Release CI (`.github/workflows/`)
`ci.yml` (push/PR): `zig build test`, Debug build -> `tests/check_golden.sh` -> `cli_ergonomics.mjs` -> `serve_smoke.mjs` -> `fuzz.py 300 1`, `zig build wasm`
+ `wasm_golden.mjs`, a ReleaseSafe build (what ships) re-running `serve_smoke.mjs` and `cli_ergonomics.mjs`, cross-compile smoke (windows x86_64/aarch64,
macos, linux-musl x86_64/aarch64, ReleaseSafe), and a web job (wasm -> `npm ci` -> typecheck -> `test:unit` -> `build:serve` -> `zig build -Dui` ->
`serve_smoke.mjs` against the embedded UI). TODO comments in `ci.yml`: `zig fmt --check src` (after the whitespace-only fmt commit) and
`zig build wasm -Dwasm-optimize=ReleaseSafe` + golden (after `no_panic`). `release.yml` (tag `v*`): wasm -> web `build:serve`
(falls back to `build:zig`) -> tests + golden + smoke on the ReleaseSafe build -> `zig build -Dtarget=<t> -Doptimize=ReleaseSafe -Dui=../../apps/web/dist-serve` for
x86_64/aarch64 x windows-gnu/linux-musl/macos -> assets `kerf-<arch>-<os>[.exe]` (what install.sh/install.ps1 download) + `SHA256SUMS`
-> `gh release create/upload` with `GITHUB_TOKEN`. Not run here (no GitHub); every command in it was run locally except the gh calls.

### Numbers (aarch64 Linux, this box)
- Binary, ReleaseSmall native aarch64: **1,148,792 B without the UI, 2,814,520 B with `dist-serve` embedded** (the UI is 1.7 MB raw, 20 files, not
  compressed: LAN/localhost serving does not need it). Cross builds without the UI: x86_64-linux-musl 1,358,560 B, x86_64-windows-gnu 1,650,688 B.
  The embed step adds ~50 s to a ReleaseSmall build (the 1.7 MB of `@embedFile`s are compiled in).
- `kerf serve` (ReleaseSmall, UI embedded, 3 reference docs): idle RSS **1.8 MB** (3 threads), 2.1 MB after 200 `/api/info` calls; peak RSS (VmHWM) 24 MB
  after a PNG sheet export. Debug build RSS after the whole smoke test: 21-27 MB.
- Latency (curl, localhost, ReleaseSmall): `/api/info` 0.55 ms (agent detection is cached 30 s), `/api/docs` 0.7 ms (per-document check cached by mtime+size),
  UI index 0.5 ms, SVG export (section view, no sheet) 13 ms, PNG sheet export 118 ms. Smoke test, ReleaseSmall: `/api/docs` p50 1.5 ms / p95 3.3 ms with
  4 SSE streams open, 24 parallel `/api/info` requests fine.
- Poller: a CLI edit shows up as `doc_changed` within 500 ms (tested with a 3 s allowance); agent edits show up before the run's `exit` event.
- Tests: `zig build test` = engine tests + serve tests (strict head parsing, chunked overflow regression, routing and body caps, access rules, proxy URL
  rules, agent templates and the untrusted-agent guarantee, op-log entry shape and locking, folder scan events, SSE hub); `tests/serve_smoke.mjs` 215 checks; the web agent's `node test/e2e-serve.mjs --real` (puppeteer) passes against
  this binary (KERF_SERVE_BIN=... with a CURRENT dist-serve embedded; a stale embedded UI makes it fail on newer UI features).

## Known gaps

- `solid` / `polygon` profiles with holes are not supported (profiles are single loops; the mesh ignores holes).
- Hatch under dimension/label text is knocked out in SVG/PDF only (DXF HATCH uses the pattern, not lines).
- PDF content streams are uncompressed (about 150 KB per sheet).

## v0.1.2 catalog/CLI (zig-catalog agent)

**Windows op-log AccessDenied (root cause, fixed).** `Io.Dir.createFile(.read = false)` opens with GENERIC_WRITE only; the old
`appendLine` then called `File.length`, which on Windows is `NtQueryInformationFile(FileAllInformation)` and needs
FILE_READ_ATTRIBUTES, which GENERIC_WRITE lacks => AccessDenied (read from lib/std/Io/Threaded.zig `fileStatWindows`,
`dirCreateFileWindows`; no wine on this aarch64 host, x86_64/aarch64 `-Dtarget=*-windows-gnu` cross-compile is clean).
Fix in `workspace.appendLine`: normal read+write handle (`.read = true`, create if missing, never truncate), `length`, one
positional write of `line + "\n"` at the end (two writes only above 4 KiB). Never append-only. `kerf new` / `kerf apply -w`
exit 3 with `kerf: error: could not append to op log ...` when the append fails (document write stays atomic);
`kerf serve` uses the same helper and prints a warning on failure. Tests: `workspace.zig` (create / no-truncate / big line),
manual check with the log path made a directory (exit 3). Also fixed: `kerf fmt` printed the `{doc,text}` wrapper; it now writes the canonical text.

**ASCII.** `kerf guide` and `kerf catalog --markdown` pass through `main.asciiFold` (dashes, quotes, x, degree, fractions,
NBSP, arrows => ASCII, else `?`). `tests.zig` asserts the raw catalog markdown/json are already ASCII and `main.zig` that the
embedded guide sources are; `build.zig` now also runs the CLI tests.

**Rebar `place`:** `face:"center"` (both axes centered; `count>1` spreads along `axis` x|y at `side_cover`), `station` (one bar
at that offset from the zone's left / bottom face; also works with the other faces as the along-face position).

**W_COVER on path bars** names `segment k of n (x.., y.. to x.., y..)` (authored vertices, bends extended to the corner) or
`the bend after segment k of n`, plus the face and numbers. Host selection and the "skip faces perpendicular to the bar" rule are unchanged.

**`shown:"dashed"`** (any component): prisms become `kind=.ghost`, `pen="hidden"`, `dashed=true` (model.Prism.dashed,
scene.Comp.dashed). Section views: outline in the hidden pen, never occludes, notes can still land on it (section.zig
`visibleRegion`/`regionItems` admit dashed ghosts), exempt from W_OVERLAP/W_FLOATING/W_NEAR_MISS. The ` (WHERE OCCURS)`
suffix is added by `scene.whereOccursText`, called from one line in `annot.zig` (note text). Mesh includes dashed components;
iso omits them (gap).

**New components / options.** `anchor_bolt.hook` `wedge` (clip 1.15 d wide x 0.6 embed, chamfered tip, default embed 4) and
`screw` (exaggerated thread strips 0.22 d deep at 0.5 d pitch, hex washer head 1.5 d wide, no nut; parts threads/washer/head).
`flashing` (z/l/drip/weep_screed/points; thin metal, embedded, spans run; gauges 24/26/28 added to the table). `joint`
(expansion filler + optional sealant `cap`; control V notch; tooled_edge radius void at a `corner`; sealant bead + backer rod;
optional `in` host for default depth). Voids are material `void`, embedded (cut a hole in the host hatch), skipped in mesh/iso.
`concrete.slab_edge.base` (part/zone `base`, anchors `base_bottom_interior`, `base_bottom_footing`; box anchors unchanged).
Style additions (spec/styles/kerf-standard.kerfstyle.json): materials `joint_filler` (hatch KERF-FILLER), `sealant`
(fill), `backer_rod`, `void`; pattern `KERF-FILLER` (45/135 deg lines at 0.0625"). Builders fall back to `generic`/`steel`
when a custom style lacks them.

**Hardware table additions** (width = strap width / seat width / angle length along z; length = strap length, hanger
height H, angle developed legs 2 x 1-7/16). Sources (strongtie.com pages are JS-rendered and could not be fetched; values are
from Simpson catalog PDFs, ICC-ES ESR-3096, and retailer listings):
RSP4 20 ga 2-1/8 x 4-1/2 (ESR-3096); A34 18 ga 1-7/16 legs x 2-1/2 wide, A35 18 ga 1-7/16 legs x 4-1/2 wide (ESR-3096 gauge,
retailer size); LTP4 20 ga 3 x 4-1/4 (ESR + retailer); LSTHD8 14 ga 3 x 18-5/8 and STHD14 12 ga 3 x 26-1/8 (Simpson LSTHD/STHD data
sheet; the 3" width is retailer-only); LUS26 18 ga 1-9/16 x H 4-3/4, MUS26 18 ga 1-9/16 x H 5-3/16, HUS26 16 ga 1-5/8 x H 5-3/8,
HHUS26-2 14 ga 3-5/16 x H 5-3/8 (Simpson catalog p.193-195); ST2215 20 ga 2-1/16 x 16-5/16, ST6224 16 ga 2-1/16 x 23-5/16
(Simpson catalog p.269); FHA18 12 ga 1-7/16 x 17-3/4 (catalog W/L, gauge retailer-only; it is a strap tie, not a foundation anchor);
LTS12 18 ga 1-1/4 x 12 (twist strap sheet; gauge retailer-only). Not verified against strongtie.com: A34/A35/LTP4 sizes, LSTHD8/STHD14
width, FHA18/LTS12 gauge.

**Test doc:** `tests/docs/palmer-sd1-like.kerf.json` (5 views: wall footing with centered stem bar, dowel, wedge anchor, dashed
STHD14, weep screed; turndown slab with base, expansion/control joints, tooled edge). 0 errors / 0 warnings; embedded in
`tests.zig` (all views x svg/dxf/pdf/png + mesh); dxf_check and pdf_check clean; PNGs reviewed by eye.

**Known gaps / state at hand-off:** goldens NOT regenerated: with the layout agent's uncommitted annotation work the A views of the
three reference docs differ (B views match), so regenerate after that lands. `zig build wasm` currently fails inside the layout
agent's uncommitted `annot.zig` (debug code pulls `std.Io.Threaded` into wasm); nothing in this section touches I/O in wasm.
Iso omits `shown:"dashed"` components. Control-joint V and tooled-edge radius are tiny at scales below 1"=1'-0".

REQUESTS (to the layout agent): commit the 2-line `scene_mod.whereOccursText` hook in `annot.zig` (note text) with your changes; do not drop it.

## Layout v0.1.2 (zig-layout agent: SPEC 18 annotation quality)

Files: `src/route.zig` (new: note routing, pure geometry), `src/annot.zig` (landing candidates, obstacles, diagnostics,
items), `src/drawview.zig` (`W_VIEW_FIT`), tests in `src/layout_tests.zig` (+ `route.zig` unit tests).

- **Case:** the style `case` transform now covers the whole rendered note, citation suffix included
  (`(IRC TABLE R602.3(5))*`).
- **Landing:** the SPEC 6.3 label point is kept when it is at least 2 text heights from the crop edges. Otherwise
  (the label point is in the crop band, i.e. near a break line) the point of the visible region with the best
  clearance is used, where distance to a crop edge counts only up to 2 text heights and the crop edges themselves are
  ignored as region boundary (`bandedLabelPoint`). `at` is never touched. A note with `"at": null` and a target now lands
  on its target (it used to be dropped silently).
- **Candidates:** per target up to ~9 landing candidates inside the visible region and the inset crop: the label point,
  four nearest grid points, the extremes in 8 directions (grid step <= 1 text height, finer for thin members like straps).
- **Routing (`route.zig`):** baseline = SPEC 6.3 + SPEC 16 adjacent swaps. A baseline with no hit is returned unchanged
  (so clean layouts keep the spec positions). If a leader crosses, or is within 1 text height of, another leader, a note
  box, a dimension text or a label, each column is re-solved by dynamic programming (Viterbi): per note a landing
  candidate and a text y on a half-pitch grid around the landing (+-12 steps), cost = obstacle hits + dimension-line
  crossings + steepness/length + distance from the preferred landing (unary) + leader/leader hits of adjacent notes
  (pair); non-adjacent notes of the column and the other column's leaders count as fixed (two passes). Then discrete
  changes are tried one at a time (note to the other column when `notes_side:"both"`, re-insertion in the column) and
  re-solved; first change that lowers (hits, cost) is kept. Work is bounded by a budget of 60 column solves. The text
  column may stand a little above/below the crop to make room (it was clamped to the crop before).
  Everything is deterministic (no RNG, fixed iteration order).
- **`W_LEADER_HIT`** (warning, per leader/other pair, at most 8 per view plus a "more" line): names both items, the
  distance in paper inches and one concrete fix: `set note 'n' "at": [x, y]` (alternative landing inside the target),
  `"place": [x, y]`, `set dim 'd' "offset": N (now M)` or `set label 'l' "offset": [dx, dy]`. The title block is not
  checked: it is placed below the lowest annotation (0.4 paper in), so it can never be within a text height of a leader.
- **`W_VIEW_FIT`:** `Overflow by edge (sheet centered on the view): left a", right b", top c", bottom d".`, then per
  overflowing axis the breakdown crop + side extensions with the culprit of each side (notes column, a named dimension or
  label, the title block, or the crop itself), then fixes smallest first: `narrow crop.x by N in (now [..])` /
  `shorten crop.y`, the offending dim's `offset`, `notes_side`, shorter notes, and `use a smaller scale (e.g. ...)` only when
  the overflow is more than 15% of the frame (the suggested scale is the first standard scale that fits).
- **Numbers:** 3 reference details (6 views), the PSL detail and the Palmer SD1 doc (5 views): leader crossings 0 -> 0,
  leaders within 1 text height of another leader or a dim/label 7 -> 0 (truss A 3, slab A 3, PSL A 1). Dense synthetic
  stress (20 random notes, single column) is not guaranteed hit-free; it reports `W_LEADER_HIT` with fixes.
- **Known gaps:** leaders still cross unrelated component geometry and dimension extension lines (soft cost only);
  non-adjacent column notes are only handled by coordinate descent; `place`d notes are obstacles, never moved.

## v0.1.3 views (zig-views agent: SPEC 19 auto crop / auto scale / notes_side / grain)

Files: `src/view.zig` (`has_scale`; crop and scale optional; `notes_side` default `both`), `src/drawview.zig` (`autoCrop`,
`resolveSection`, `sectionPass`, `resolveSpec`), `src/section.zig` + `src/hatch.zig` + `src/style.zig` (grain), tests in `src/views_tests.zig`
(+ a unit test in `hatch.zig`).

- **Auto crop (section views, `crop` omitted):** bbox of all components visible in the section (cut or beyond, `omit` respected) whose
  material is not a fill (`earth`, `gravel`, `sand`, `compacted_fill`; rebar/steel/anchors count) plus 6" on every side. Fills are clipped to it
  (break lines drawn as for any crop). If only fills exist, their bbox is used. `drawing.crop` carries the resolved window. Iso views are unchanged
  (they already fitted an omitted crop and `NTS`).
- **Auto scale (section views, `scale` omitted):** candidates in the fixed order 3", 1-1/2", 1", 3/4", 1/2", 3/8", 1/4". For each candidate the
  whole view is built (geometry, notes routed at that scale, dims, title) and measured; the first whose bounds fit the sheet frame wins. A candidate
  whose bare crop already exceeds the frame is skipped without a pass; the last candidate (1/4") is the fallback. No oscillation is possible: it is a
  single ordered scan, each candidate is evaluated from scratch, and only the winner's diagnostics are kept. Cost: up to 7 passes (truss detail
  with auto crop+scale: 0.2 s total).
- **`W_VIEW_FIT`:** emitted whenever the final view overflows the frame; with auto scale that can only happen when even 1/4" does not fit, with an explicit
  scale (crop auto or explicit) exactly as before. Docs that set both crop and scale take the same code path as before: goldens of the reference details
  differ only by grain.
- **`notes_side`** defaults to `both` (was `right`). Every reference doc sets it explicitly.
- **Grain (`KERF-GRAIN`):** style material `wood` and `wood_treated` got `"grain": {"pattern":"KERF-GRAIN","scale":1,"amplitude":0.01,"wavelength":1.1}`
  (paper inches). Applied in `section.drawCut` to a lumber component's cut region when it has no end-grain quads (run x or y), along the direction of the
  member's longest edge. The pattern is a sparse straight dashed line family (spacing 0.085" paper, dashes 1.6/0.4/0.8/0.5, phase shift per line) and
  `hatch.generateGrain` turns each dash into a wave: per-line phase, amplitude (55-100%) and wavelength (75-125%) from a hash of the quantised line position
  (so a dash continues its own wave), plus a 0.35 second harmonic; amplitude tapers toward the long edges; lines keep 0.03" paper clear of every boundary
  (probe points must be inside the region, holes respected). Output is a `hatch` item (pen `hatch`, 0.09 mm, layer S-DETL-PATT) with the wavy
  polyline as short segments. Members too narrow for the inset (1.5" edges at 1/2" scale and smaller) simply get no grain lines. The straight pattern
  is what DXF HATCH carries.
- **Numbers:** goldens changed: flush-beam-strap A, flush-psl-2x6 A, palmer-sd1-like A and C (all and only views with lengthwise lumber); I looked at every
  regenerated sheet PNG.

### SPEC ISSUES
- `wood_engineered` got no `grain` (SPEC 19 says wood, wood_treated and wood_engineered): it already carries the `KERF-LAM` lamination lines, and the two
  together read as a blob on an LVL/PSL beam. The style schema allows adding it back with one key.
- DXF: the HATCH writer emits `h.angle` as group 52 but leaves the family angles unrotated, so a grain (or earth) hatch with a non-zero angle shows at
  the pattern's own angle in strict viewers (run-y members get horizontal grain lines in the fallback). Pre-existing for `EARTH` at 45 degrees; fix in
  `dxf.zig` by adding `h.angle` to each family angle (53) and zeroing 52.

### REQUESTS (zig-ux / orchestrator)
- `load.zig` `inspect` (around the `view_mod.parse` call): after parsing, resolve the spec so inspect uses the same crop/scale as the drawing:
  `spec.* = (try drawview.resolveSpec(a, l.doc, l.style, l.scene, &spec_v)).*;` (it is a no-op for fully explicit specs and iso views). Without it a
  view with no crop inspects against an empty crop.
- catalog/schema/`kerf schema view` text: `crop` is optional ("omitted: auto-fit to the non-fill components' bounding box + 6 in; fills are clipped to
  it"), `scale` is optional ("omitted: the largest of 3\", 1-1/2\", 1\", 3/4\", 1/2\", 3/8\", 1/4\" at which view, notes and title fit the sheet; W_VIEW_FIT
  only fires for an explicit crop or scale"), and `notes_side` defaults to `both`. The guide's minimal example may drop crop/scale.
- `view.zig` is shared: I added `has_scale` and made crop optional there (no other edits).

## v0.1.3 agent ergonomics (zig-ux agent: SPEC 19 CLI, validation, defaults, coupled geometry)

Files: `src/schema.zig` (field tables + `kerf schema` + the example doc), `src/lint.zig` (W_UNKNOWN_KEY, W_NOTE_STYLE, W_DIM_ZERO, acknowledge, dim `dir`
default), `src/main.zig`, `src/api.zig` (`schema`, `help`), `src/catalog.zig` (`example` per type, new params/anchors/text), `src/builders.zig`
(`barrier`, `until` along the slope, membrane `until`, truss anchors), `src/compile.zig` (`slope: "@comp"`, anchor_bolt anchor default, default z),
tests in `src/ergo_tests.zig`, `tests/cli_ergonomics.mjs` (node, runs the real binary: `node tests/cli_ergonomics.mjs [kerf]`).

- **Failed `apply`:** every error diagnostic on stderr as `ERROR <code> <path>: <message>  Fix: <fix>`, then `nothing written`, exit 1 (also for
  malformed ops JSON: `ERROR E_JSON ops:` and for engine-level failures). `--dry-run` = summary, write nothing (`--dry-run -w` is exit 2).
  Side fix: the op log's `ops` was empty/garbage when the ops came from a file (freed buffer); now kept.
- **`kerf schema [topic]`:** topics doc view note dim label cite ops at array acknowledge common refs + each component type. Non-component objects
  come from the tables in `schema.zig`; component types from `catalog.zig` (the same table that validates params, so no drift). `canon.zig` takes
  the key order of doc/view/note/dim/label/cite/at/array from `schema.zig` (a test guards `catalog.common` vs canon). `omit` is now a view key and
  sorts before `annotations` in canonical output (docs with `omit` get a one-time key-order diff on the next `apply -w`/`fmt`).
  `kerf guide` = cli-guide + generated schema (view note dim label cite ops) + the example document + drafting rules + catalog.
  API: `schema {topic?}` -> `{topic,text}`; `help {}` lists every function with input/output shape (`kerf call help` does not read stdin).
- **`kerf new --template section`:** `schema.example_doc` renamed (id from the file, `--title` optional): 2 lumber members, a section view
  (crop, scale, notes_side), 2 notes (one cited), 1 dim, 1 label; checks with 0 errors and 0 warnings (a test pins it).
- **W_UNKNOWN_KEY:** doc top level, views, annotations (per `type`), cite entries, component `at`/`array`/`acknowledge` entries (component params
  were already E_PARAM; that message now suggests the nearest key). Synonym table in `lint.zig` first (`citations|citation|cites`->cite,
  `side`->view `notes_side`, with a different hint on a note, `point|target_point|arrow`->at, `kind`->type on annotations, `pos|position`->place,
  `ref`->`to` in `at`), then edit distance (<=1 for keys of 4 letters or fewer, else <=2). Keys are never dropped. `meta` is not linted (free-form).
- **dim `dir`:** default = dominant axis (|dx| >= |dy| -> h). Applied by `lint.withDimDirs` to a COPY used for drawing (load, api `drawing`/`export`);
  the stored document is unchanged. `api` pays one extra compile only when some dim has no `dir`. `W_DIM_ZERO` (< 1/16") names both extents and the other `dir`.
- **acknowledge:** `[{code, reason}]` on a component; matches a warning whose `id` is that component, or (W_OVERLAP / W_NEAR_MISS /
  W_UNTREATED_CONTACT) whose message quotes it. The warning becomes `INFO I_ACK <comp>: <code> ... acknowledged: <reason> (was: <message>)` (printed in
  the summary) and `kerf apply -w` writes the I_ACK lines to the log entry as `"ack": [...]`. Errors (non `W_`) and entries without a reason are E_PARAM.
- **`lumber.barrier`** `sill_seal | membrane`: part `barrier`, a 1/8" strip (style material `sill_seal`, solid fill; both values draw it) UNDER the member.
  The box (and the bottom_* anchors) start at the strip underside, so the member rises 1/8" when it is placed on a support by bottom_left. Clears
  W_UNTREATED_CONTACT because the wood no longer touches the masonry. NOTE: `spec/styles/kerf-standard.kerfstyle.json` gained `sill_seal`.
- **W_NOTE_STYLE** (one per note, lists every problem and gives the corrected text as the Fix): lowercase letters, ` x `, `1-1/2"`, trailing period (not after
  a known abbreviation or a dotted token such as U.N.O.), spelled-out GYPSUM BOARD / PRESSURE TREATED / ON CENTER / CONCRETE / CONTINUOUS / EACH / BOTTOM /
  REINFORCING / MINIMUM / DIAMETER. Applied to `text` only. The three reference docs, psl and palmer have 0 warnings, no note edits needed.
- **Defaults:** anchor_bolt `at.anchor` defaults to `top_of_concrete`. In-plane members (natural z thickness: lumber run x/y, truss, connector,
  path rebar, anchor_bolt) default to z centred on the FIRST section view's `cut_z` when it sets one (else the middle of `run`). All five
  reference/test docs have cut_z at the middle of run, so goldens are unchanged.
- **Coupled geometry:** `slope: "@truss"` (any component: its rotation + truss pitch; exterior right = negative; dependency edge, so ops refuse removing the
  followed component and ordering is automatic). `until` on lumber/panels measures along the rotated run direction (square end cut at the Ref's projection);
  membranes take `until` on the last segment. Truss anchors `heel_outer` (middle of the heel's outer face above the bottom chord) and
  `top_chord_bottom_at_bearing`. `array` axis z works on anchor_bolt and connector (tested, drawn in iso, one cut instance in section).
  Roof recipe: sheathing `{"slope":"@truss","until":"truss@top_chord_end","at":{"anchor":"bottom_left","to":"truss@tail_top"}}`, roofing membrane
  `{"slope":"@truss","points":[[0,0],[10,0]],"until":"truss@top_chord_end"}` (for exterior right use anchor bottom_right and negative x points).
- Done from the views agent's REQUESTS: `inspect` calls `drawview.resolveSpec`.

### SPEC ISSUES
- The `barrier: "membrane"` value is drawn exactly like `sill_seal` (one 1/8" strip); only the name differs (for the note text).
- `heel_outer` is not defined precisely in SPEC 19; chosen as the middle of the outer vertical face of the heel above the bottom chord.
- DXF hatch angle (views NOTES SPEC ISSUES) not fixed: it needs the family offsets rotated as well, which changes DXF goldens; left to the views owner.


## v0.1.4 cli/docs (cli-docs agent: SPEC 20 "Coupled geometry & validation", "Agent docs")
- **`kerf guide` = 10.7 KB** (was 44 KB; limit 12 KB, tested in `tests/cli_ergonomics.mjs`), pure ASCII. Source: `spec/llm/cli-guide.md`
  above the `<!-- FULL -->` marker + `schema.compactSection` (one generated line per object: `name*` required, `:type`, `=default`;
  the example document; topic list; one line per component type). Content: shell rules (ONE command per call, no `&&` `;` `|`
  heredocs), workflow with the ambiguity policy, one-command-per-line recipes (ops file written with the agent's file tool, then
  `kerf new f --ops ops.json --why ...` / `kerf apply f ops.json -w --why ...`), diagnostics, placement/Refs, the roof-pitch recipe
  (`slope:"@truss"` + `until:"truss@top_chord_end"`), note grammar, view rules. `kerf guide --full` = short guide + the text below the
  marker + the full per-object schema + `spec/llm/system.md` + the catalog (47 KB).
- `kerf schema a b c`: topics print in order separated by `----`; a bad topic reports on stderr (with the suggestion), exit 1, the
  good ones still print. `Opts.pos` grew to 16 positionals.
- `kerf new <file> --ops <file|inline json> [--why] [--title] [--template section]`: creates the document in memory, applies the ops,
  and only then writes the file (a failing op: `ERROR ...` + `nothing written`, no file, exit 1). Two log lines: `create` and the
  apply with `--why`. `kerf apply --ops` also accepts a file path now (inline JSON must start with `[` or `{`).
- `W_SHORT_SLOPE` (`validate.zig: shortSlope`): a sloped panel/membrane (instance 0) whose axis matches a touching lumber/truss
  (`top_chord` part)/panel (membranes only) within 0.5 deg, and whose upper end is more than 1/2" short of that member's upper end
  or of the first explicit section-view crop edge (along the slope), whichever is nearer. Message names both ids and the shortfall;
  the fix is `"until": "<member>@top_chord_end"` (trusses; `top_right`/`top_left` for lumber/panel hosts) with `slope:"@<member>"`.
  Acknowledgeable. The reference truss detail stays at 0 warnings; tests: `ergo_tests.zig` (e07 repro with pitch 6:12 + literal
  length 40, long length, `until` fix, acknowledge, membrane short of its deck) and `cli_ergonomics.mjs` section 6.
- `acknowledge` of an `I_*` code is accepted and ignored (nothing to suppress); `E_*` is still E_PARAM.
- Catalog `lumber` summary carries the standard beam-in-wall view (elevation along the wall; end-on only on request).
- Observation: in the alpha4 e07 final document the 66" sheathing at 6:12 actually overshoots the crop (the sheathing is clipped by the
  crop, the truss lines beyond the section cut are not), so the visual "sheathing ends short" there is a render clipping difference
  between cut and beyond members, not a short panel; `W_SHORT_SLOPE` fires for the literal-length-too-short case (SPEC 20 rule).
- Golden files differ at the time of this commit because the layout/render agents changed drawings; regenerate with `tests/make_golden.sh`
  after all three agents are done. `zig build test` had one failing layout test (`W_LEADER_HIT: a label on a leader gets an offset proposal`)
  from the layout agent's in-progress work; everything in this lane passes.

## v0.1.4 render (render agent: SPEC 20 "Drawing conventions")

- **Path rebar** (`rebar` `mode:"path"`) draws in section views as ONE open centerline polyline (true arcs at the bends) in the
  `rebar` pen, no fill, no outline (`section.isPathBar`). The old ribbon only clears the host hatch (a halo of bar width).
  `along_z` dots, iso and mesh are unchanged (3D tube). Geometry-based checks (`W_COVER`, ...) still use the ribbon/centerline.
- **New style layer `rebar` = `S-DETL-REBR`** (0.5 mm), key `rebar` in `layers`; `style.layerKeyForPen("rebar")` now returns it
  (a user style without it falls back to the `steel` layer). `drawview.layer_draw_order` got `"rebar"` (one-token edit in the
  layout agent's file; needed or the layer is not declared and SVG/raster drop its items). **Pen `rebar` is now 0.5 mm** (was
  0.35): path bars read as heavy lines like the cut pen, distinct from the 0.35 vapor/membrane lines. DXF: LWPOLYLINE with bulges
  on `S-DETL-REBR`; rebar dots too.
- **Face-on hardware** (`connector lay:"face"`, H2.5A/HETA/META style): `Prism.face_tie`. Drawn as a symbol on top of everything:
  outline in the `steel` pen, never filled/occluded/hatch-hole, nail-hole dots (fills) at 1" pitch along the centerline
  (pitch widened to whole inches so dots stay >= 0.07" apart on paper; dot radius 0.016" on paper, min 0.05"); lines of
  ordinary members under the tie are knocked out (`knockOutFaceTies`, `visibleOpen`). Embedded strokes are not knocked out.
- **Edge-lay sheet metal** (straps, flashing; `isMetal`, centerline ribbon): drawn thickness is raised to at least
  `min_metal_paper_in` = 0.022" on paper (0.55 mm), on the side(s) it already occupies (`thickenThinHardware`), so CS16 /
  CMST14 straps read as a band beside the 0.5 mm member outline instead of a hairline merged with it. True gauge stays in 3D.
- Looked at (PNG 1600 px + zoom crops): truss-bearing-cmu A (H2.5A, 4 dots, 3" and 1/2" scale variants with HETA20),
  flush-beam-strap A, flush-psl-2x6 A, monopour A/B, palmer A-D, DXF/PDF renders of palmer A and truss A.
- Crop clipping (orchestrator note on e07): verified, not a bug. All member linework (cut/beyond/hidden/steel/rebar/membrane/vapor)
  is inside the crop (test `all member linework ... stays inside the crop`). In the e07 doc at 6:12 the sheathing/roofing exit
  through the crop TOP edge (break mark at the top) while the truss chord exits through the RIGHT edge, so the crop corner
  makes the sheathing look short. Fix is a bigger crop / `W_SHORT_SLOPE`, not clipping.
- Goldens regenerated from HEAD + render changes only (layout agent was still editing annot/route): re-run
  `tests/make_golden.sh --png` once the layout work is committed.
- Tests: 3 new unit tests + crop test in `section.zig`. `zig build test` 122/122 on HEAD + render; wasm 24/24 vs goldens;
  serve_smoke 129/129; cli_ergonomics 104/104; check_golden OK incl. dxf_check 0 errors.

### REQUESTS (render)
- apps/web: `apps/web/public/style.json` mirrors the style; copy `spec/styles/kerf-standard.kerfstyle.json` (pen `rebar` 0.5, layer `rebar`).
- orchestrator: SPEC 7 style example (`layers`) and 20: add `rebar: S-DETL-REBR`; pen `rebar` 0.5 mm; mention the 0.022" minimum
  paper thickness for sheet-metal hardware and the knock-out of member lines under face-on ties.

## v0.1.4 layout (layout agent: SPEC 20 "Layout resolves its own collisions")

Files: `src/annot.zig` (dim stacking, outside text, repair loop, `column`, place alignment), `src/route.zig` (column hint,
hard/soft hit ranking, `light` re-route), tests in `src/layout_tests.zig`.

- **Repair loop** (`annotate`): annotations are parsed into note / dim / label specs first; dims and labels are rendered from
  them per pass. Pass 0 = stack dims, route notes (`route.route`: landing candidates, columns, DP; this is the "move the landing
  point" and "other column" repair). While leaders still hit dimension text or labels (`W_LEADER_HIT` candidates), each pass pushes
  every hit dim out by whole 0.25" paper steps (first step count that clears all leaders, at most 8 steps in total per dim) and
  moves every hit label to the nearest spot (up, down, sideways, diagonals; at most 12 text heights; first without sitting on
  drawn outlines) where no leader is within a text height and it overprints no other label/dim text, then re-routes
  (light search). At most 8 passes; stops when clean or after 2 passes without improvement; the pass with the fewest hits wins
  (earlier pass on ties, so a clean layout is never touched). `W_LEADER_HIT` is raised only for what is left, with the old fix texts.
- **Router:** `hits` now also has `hard` = hits between notes (leader/leader, leader/text block); the search ranks `hard` first,
  because dim/label hits are repaired afterwards. `Params.light` = one search with budget 20 (repair re-routes); full route = budget 60.
  `NoteIn.column` (note `column: "left"|"right"`) fixes a note's column and disables the other-column move for it; it overrides
  the view's `notes_side` (a `right` hint on a `left` view creates a right column). A bad value warns `W_PARAM` and is ignored.
  Schema: `note.column` (the one minimal edit in schema.zig), so no `W_UNKNOWN_KEY`.
- **place:** a note with `place` whose block sits left of its arrow (the existing `place.x + w/2 < landing.x` rule) draws its
  lines right-aligned to `place.x + block width` (block bbox / top-left stay `place`, SPEC 16), leader leaves the right edge.
  Auto-placed left-column notes keep left-aligned lines (unchanged, goldens stable).
- **Dim stacking** (`stackDims`): dims are placed shortest first (ties by document order). A dim's candidate = authored offset
  (+ repair push) moved out in 0.25" paper steps (k = 0..12) until it has no conflict with the dims already placed or with labels:
  conflict = a dimension line within 0.125" paper of another one with a shared stretch (parallel), or its text box touching another dim's
  text, line, extension line, tick or leader (either direction). If none is free the candidate with the fewest conflicts wins.
  Dims that do not conflict keep their authored offsets exactly. Works for h, v and aligned dims (iso views have no dims).
- **Text that does not fit** (same criterion as before, `tw + 2 tgap > len - tick`): never shrunk, never dropped. Drawn on the axis
  beyond the end of the dimension line (`variant` 0 far end / 1 near end) with a short leader (2 tick lengths) from the dim line end
  to the text, vertically centred on the axis; variants 2-5 lift the text one text height out of the axis (outward / inward of the
  object) with a diagonal leader. Chosen per k: the first variant with no dim/label conflict, preferring fewer hits on drawn
  outlines (`base_segs`). Goldens changed only where a dim text did not fit (truss `7 5/8"`, slab `1 1/2"`, PSL `3"`), looked at.
- **Numbers** (W_LEADER_HIT, before = HEAD cb19d64 engine, after = this): replay of the alpha4 eval's op logs
  (`~/kerf-eval/runs/2026-10-05T22-47-41-504Z/*/workspace/*.log.jsonl`, apply step by step, `kerf check` each state):
  e01 9 -> 4, e04 1 -> 0, e05 1 -> 0, e09 1 -> 0, others 0; total 12 -> 4. The 4 left are e01 states where the agent set `place`
  / explicit `at` on 6 notes (fixed notes cannot be moved; the other notes are re-routed around them). 6 hand-made stress docs
  (12 notes of the truss reference squeezed into a small crop at 1"=1'-0", one or both columns, 4 dims, 2 labels on the leaders'
  way): 25 -> 16; right-only and left-only single column cases keep note/note near-hits, and a left-only column with dims on the
  same side keeps dim-text hits (every leader must cross the dim stack; pushing out cannot help).
  Time (ReleaseFast native, per `drawing`): truss A 31 -> 15 ms, stress 98 -> 160 ms, 109 -> 240 ms; wasm ReleaseSmall stress 0.4-0.7 s
  (typical views 15-20 ms). wasm 866,509 B raw.
- Looked at: eval docs e01-e10 before/after (only e01 plate note right-aligned, e02/e06/e09 outside-text dims), e03 with
  3 same-offset vertical dims (before: lines and texts overprinted; after: stacked 0.25" apart), e02 with the 4" HETA embed dim
  restored (text outside with a leader), replays of e01 steps 2-5, goldens truss/slab/PSL A.

### SPEC ISSUES (layout)
- SPEC 20 "right-aligns its text block to place.x + width": implemented as lines right-aligned at `place.x + w` inside the SPEC 16
  top-left block; a `place` far inside the drawing still overlaps it (the engine does not relocate designer-placed text).
- "Other column" repair only when `notes_side` is `both` (a one-sided view stays one-sided); `column` is the explicit override.
- Repair never changes the sign (side) of a dim and never drops/shrinks one; dims sliding their text along the line is not done.
- The new `W_PARAM` (bad `column` value) is not in SPEC 9's list.

### REQUESTS (layout)
- cli-docs: `kerf schema dim` / the guide could mention that overlapping dims stack automatically and that small dims keep
  their text outside with a leader; `kerf schema note`/guide: `column: "left"|"right"` (already in the schema field list).
- orchestrator: SPEC 6.4 text-outside sentence and SPEC 9 (`W_PARAM`); SPEC 20 mentions "label away from the leader" (moves up to 12 text heights).

## v0.1.5 (SPEC 21: lenient ops, requested-elements coverage, W_CROP_STALE, thin-layer landing, note lint)

Gate at the last commit: `zig build test` (139 tests), `zig build wasm` (902,131 B raw / 337,058 gzip / 266,769 brotli; wasm == native goldens 24/24),
`tests/check_golden.sh`, `node tests/cli_ergonomics.mjs` (136 checks), `node tests/serve_smoke.mjs` (134 checks). New Zig tests: `src/v015_tests.zig`
(ops shapes, coverage, `meta.requested` shape, W_CROP_STALE), `src/layout_tests.zig` (thin landing), `src/lint.zig` (note lint).

- **Lenient ops** (`ops.zig: normalize`): array, single op object (has `"op"`), `{"ops":[...]}` or `{"ops":[...],"why":"..."}` (also `"ops":<single op>`).
  `ops.apply` normalizes itself, so `api.apply`, `kerf apply`, `kerf new --ops` and `POST /api/docs/<f>/apply` all accept every shape
  (serve also takes a bare array / single op as the whole body). Anything else is `E_PARAM ops: the ops input must be an array of op
  objects [...], a single op object {...}, or {"ops":[...], "why":"..."}; got an object with keys a, b  Fix: ...` (CLI prints it as an
  `ERROR` line, nothing written). The CLI (`main.zig: prepareOps`) writes the normalized array to the op log; the envelope `why` is used
  when `--why` is absent (serve: when the body has no `why`). A non-op element is `E_OP ... got a <type>`.
- **Coverage** (`coverage.zig`, shown by `load.zig: coverageBlock`, warned by `lint.zig: requestedMissing`): `meta.requested` (array of
  non-empty strings, else `E_PARAM`; key order `requested` after `jurisdiction` in `canon.zig`). An item is covered when all its words
  (lowercase alphanumeric tokens, trailing plural `s` dropped, stop words `w with and at of the a an to for in on per` ignored) occur in ONE
  candidate: a component's `id type label model size`, or one note/label annotation's text. The `ok` line lists the component ids (a note
  contributes its target component, else `note <id>`). The block sits after the component lines and before the diagnostics, only when the
  document has `meta.requested`; `W_REQUESTED_MISSING` (path `meta/requested/<i>`) per missing item. The three reference details and
  `tests/docs/*` have no `meta.requested`, so nothing changes for them. Guide (`spec/llm/cli-guide.md`, now 11.6 KB of 12 KB) and
  `spec/llm/system.md` tell agents to fill it on the first build and finish only at full coverage; `kerf schema doc` documents it.
- **W_CROP_STALE** (`crop.zig`, called from `load.zig: loadAfterEdit`; `api.apply` loads the pre-edit document first). Per explicit-crop
  section view and non-fill component (visible prisms at `cut_z`, bounding box) with more than 25% of the box outside the crop:
  with history: warn when the component was mostly inside the same crop before (or the view auto-fitted), stay quiet when it was already
  mostly outside (deliberate cut), and for a NEW component warn unless some view shows at least 75% of it; without history (`kerf check`,
  new view): warn only when NO view shows any of it. The fix suggests removing `crop` (and `scale`, if set, else `W_VIEW_FIT` follows)
  or the widened crop `{"x":[..],"y":[..]}`. Acknowledgeable on the component. Effect on the e07 recipe: after the pitch change the
  clipped roof now warns (`cli_ergonomics.mjs` section 6/9).
- **Thin-layer landing** (`thinland.zig`, called from `annot.zig: targetLanding`; `section.zig: linePoints` is the drawn polyline of a
  membrane, shared with the stroke drawing). Targets: membrane / flashing / connector / path rebar prisms (own `centerline`; membranes
  use the drawn stroke, vapor retarders are drawn 0.03" paper off their ribbon, which is why the old landing sat beside the line) and
  panels thinner than 1/2" (midline of the quad). The regular label point is kept when it is on that line, at least one text height from
  every neighbor edge that crosses the line and (for exposed members) not inside a neighbor body. Otherwise the candidates are samples
  along the line (step half a text height) that are shown (crop band, visible region), not inside a neighbor body, ranked by clearance
  from crossing neighbor edges (cap two text heights), then distance to the old label point; up to 6 are passed to the router.
  Goldens regenerated; I looked at all five changed `A.png`s: the vapor retarder arrow is on the dashed line mid-slope instead of at the
  gravel junction, the CS16 strap arrow sits on the strap instead of beside it, roofing/sheathing and the weep screed land on their lines.
  Known cosmetic side effect: in flush-beam-strap the router now puts the LVL beam arrow at the beam's lower-left corner (an existing
  "extreme" candidate chosen to avoid the new strap leader).
- **Note lint** (`lint.zig: checkNoteText`, `noteStyle`): `W_NOTE_STYLE` also flags commentary (`?`, `NOTE:`, `PLEASE`, `SHOULD BE`, whole
  words `WE` and `I` followed by a space/apostrophe), a text ending on `W/ AND OR TO @ & PER WITH FOR OF`, notes longer than 130
  characters, and a note whose text equals an earlier note of the same view (case, whitespace and trailing period ignored). Problems that
  need a rewrite get a "rewrite it as <SIZE/QTY> ..." fix instead of `set text to`.

### SPEC ISSUES (v0.1.5)
- W_CROP_STALE "for `kerf check` without history, fire when > 25% outside regardless" would flag every reference detail (they cut studs and
  slabs on purpose and crop one model several ways: 10 warnings on palmer-sd1-like alone) and the task requires them to stay at 0 warnings.
  Chose: without history it fires only for a component that no view shows at all (all explicit crops miss it entirely, no iso/auto view);
  the 25% rule applies with history (`apply`). SPEC 21 should say so.
- Note length limit is 130, not 120: the reference flush-beam strap note is 121 characters.
- The connector-word list is longer than SPEC 21's (adds `&`, `PER`, `WITH`, `FOR`, `OF`).
- Coverage matching also uses a component's `size` (so "2x6 sill" matches the lumber) and label-annotation text; both beyond SPEC 21's list.
- `{"ops": <single op>}` and a bare array/op as the serve body are accepted too (superset of SPEC 21).
