# engines/zig: Kerf engine in Zig 0.16

Status: milestone 1 (vertical slice). Section views (cut + beyond + hatch + crop + break lines),
SVG export, CLI, wasm ABI. See "Status / gaps" at the bottom.

Zig: `~/tools/zig-aarch64-linux-0.16.0/zig` (0.16.0). std only, no third-party deps.

## Build / run / test

```sh
cd engines/zig
zig build                    # CLI -> zig-out/bin/kerf
zig build test --summary all # unit + reference-document tests (leak-checked by std.testing.allocator)
zig build wasm               # -> dist/kerf.wasm (wasm32-freestanding, ReleaseSmall, zero imports)
zig build wasm -Dwasm-optimize=ReleaseFast
zig-out/bin/kerf export ../../spec/details/truss-bearing-cmu.kerf.json --view A --format svg -o /tmp/a.svg
node ../../tools/zig-engine/wasm_check.mjs dist/kerf.wasm ../../spec/details/truss-bearing-cmu.kerf.json A /tmp/w.svg
../../tools/zig-engine/svg2png.sh /tmp/a.svg /tmp/a.png 1600   # headless chromium render, then view the PNG
../../tools/size_report.sh dist/kerf.wasm
```
`dist/` is git-ignored; build it. The default style and stroke font are embedded from
`src/data/` (copies of `spec/styles/kerf-standard.kerfstyle.json` and `spec/fonts/kerf-simplex.json`;
re-copy when the spec changes).

## Using the engine from Zig (teak app)

In the consumer's `build.zig.zon` add a path dependency on `engines/zig`:
```zig
.dependencies = .{ .kerf = .{ .path = "../kerf/engines/zig" } },
```
In `build.zig`:
```zig
const kerf_dep = b.dependency("kerf", .{ .target = target, .optimize = optimize });
exe_mod.addImport("kerf", kerf_dep.module("kerf"));
```
In code (works natively and on wasm32-freestanding; no OS calls, all allocation goes through the allocator you pass):
```zig
const kerf = @import("kerf");
const r = try kerf.call(gpa, "export", input_json); // r.ok == false => r.bytes is {"error":{code,message}}
defer gpa.free(r.bytes);
```
`kerf.call(gpa, fn_name, input_json)` is exactly SPEC 13 (`version catalog fmt check apply inspect drawing mesh export`).
Typed building blocks are also public: `kerf.drawview.build(...)` returns a `kerf.drawing.Drawing`
(items: path / fill / hatch / text, in model inches) for in-process rendering, `kerf.svg.render`,
`kerf.compile.compile` gives the resolved `Scene` (components, prisms, anchors).

## Calling the wasm (web app)

`dist/kerf.wasm` exports exactly `memory, kerf_alloc, kerf_free, kerf_call, kerf_out_ptr, kerf_out_len`
and imports nothing (`node tools/zig-engine/wasm_check.mjs dist/kerf.wasm` verifies). Instantiate with `{}`:
```js
const {instance} = await WebAssembly.instantiate(bytes, {});
const ex = instance.exports, enc = new TextEncoder(), dec = new TextDecoder();
function call(fn, inputObj) {
  const f = enc.encode(fn), i = enc.encode(JSON.stringify(inputObj));
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f);       // re-read ex.memory.buffer after every alloc/call (memory may grow)
  new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const status = ex.kerf_call(fp, f.length, ip, i.length);      // 0 ok, 1 error (output = error JSON)
  const out = new Uint8Array(ex.memory.buffer, ex.kerf_out_ptr(), ex.kerf_out_len()).slice();
  ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return {status, out};   // JSON text for all fns except export (raw svg/dxf/pdf bytes)
}
```

## Layout

`src/kerf.zig` root (module `kerf`) -> `api.zig` (dispatch) | `json.zig` | `units.zig` (SPEC 1) |
`geom.zig` (bulge arcs, robust orient2d, intersections) | `clip.zig` (polygon booleans) | `pathclip.zig` |
`catalog.zig` | `builders.zig` (component builders) | `compile.zig` (placement DAG, arrays, z) |
`scene.zig` (refs/anchors) | `section.zig` (view) | `hatch.zig` | `drawing.zig` (IR) | `svg.zig` |
`canon.zig` (fmt) | `main.zig` (CLI) | `wasm.zig` (ABI).

## SPEC ISSUES (what I chose, why)

- Note wrap width: `wrap_chars` x the mean advance of A-Z in the stroke font (19.73 units), the only
  reference width that is a pure function of the font.
- `lumber.plies` stack along the *thickness* direction: X for run z upright, Y for run z flat, Z for
  run x/y with a wide face, in-plane for a narrow face (spec says "along Z otherwise").
- Section X marks and ply lines use the `beyond` pen; shared edges between prisms of one component
  draw in `beyond`, outer edges in `cut` (spec 16 accepted interpretation).
- Layers per pen: cut/profile -> cut layer, rebar/steel/membrane/vapor -> steel layer, others by name.
  DXF entities will carry their own lineweight.
- Vapor retarder lines are drawn at least 0.03 paper inch off the host edge so the dashes are visible.
- `src` for array instances / multi-bar `place` is `id#k`.
- Truss standard heel: chords touch only at the heel point, leaving a wedge between them (as the spec geometry gives).

## REQUESTS

(none yet)

## Status / gaps

M1 done: lumber, panel, cmu_wall, concrete (all shapes), rebar (along_z, path, place), anchor_bolt,
connector, truss, membrane, fill, insulation, solid builders; placement DAG; section view; SVG; wasm; CLI.
Not yet: annotations (notes/dims/labels/title), validation codes beyond E_PARAM/E_REF/E_CYCLE,
apply/inspect/summary/check, DXF, PDF, sheet, mesh, iso.
