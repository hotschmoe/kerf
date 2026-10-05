# engines/rust: Kerf engine in Rust

Status: **milestone 1 (vertical slice)**. See "Status" at the bottom for what is and is not done.

Workspace: `kerf-core` (lib, all logic), `kerf-cli` (bin `kerf`), `kerf-wasm` (cdylib, raw C ABI, no imports).
Dependencies: `serde_json` (preserve_order) and `i_overlay` (2D polygon booleans). Nothing else.

## Build / run / test

```sh
cd engines/rust
cargo build --release                     # native CLI: target/release/kerf
cargo test                                # unit tests
./build-wasm.sh [--size-report]           # -> engines/rust/dist/kerf.wasm (git-ignored; build it locally)
./target/release/kerf check  ../../spec/details/truss-bearing-cmu.kerf.json
./target/release/kerf export ../../spec/details/truss-bearing-cmu.kerf.json --view A --format svg -o /tmp/a.svg
./target/release/kerf export <doc> --view A --format svg --sheet -o /tmp/a-sheet.svg   # with frame + title block
./scripts/svg2png.sh /tmp/a.svg /tmp/a.png 1600                                         # chromium render, then Read the PNG
```

## Calling the wasm (SPEC 13.1)

`dist/kerf.wasm` exports `memory, kerf_alloc, kerf_free, kerf_call, kerf_out_ptr, kerf_out_len` (plus the linker's
`__data_end`/`__heap_base` globals) and imports nothing: instantiate with an empty import object.

```js
const { instance } = await WebAssembly.instantiate(bytes, {});
const x = instance.exports, enc = new TextEncoder(), dec = new TextDecoder();
function put(u8) { const p = x.kerf_alloc(u8.length); new Uint8Array(x.memory.buffer, p, u8.length).set(u8); return p; }
function call(fn, input) {                    // input: JS object; returns {ok, bytes}
  const f = enc.encode(fn), i = enc.encode(JSON.stringify(input));
  const fp = put(f), ip = put(i);
  const code = x.kerf_call(fp, f.length, ip, i.length);      // 0 ok, 1 error (output is {"error":{code,message}})
  x.kerf_free(fp, f.length); x.kerf_free(ip, i.length);
  const out = new Uint8Array(x.memory.buffer, x.kerf_out_ptr(), x.kerf_out_len()).slice();  // COPY before next call
  return { ok: code === 0, bytes: out };
}
call('export', { doc, view: 'A', format: 'svg', sheet: false }).bytes   // raw SVG bytes
call('drawing', { doc, view: 'A' })                                       // Drawing IR JSON
```
`doc` is the Kerf document as an object (a JSON string also works). `style` is optional (omit = embedded kerf-standard;
a partial style object is merged over the default). A ready-made Node loader is `scripts/kerf-wasm.mjs`
(`import { loadKerf }` or `node scripts/kerf-wasm.mjs export input.json out.svg`). Memory may grow during a call:
re-read `x.memory.buffer` after every `kerf_call` (do not cache typed arrays across calls).

Functions implemented: `version`, `catalog`, `fmt`, `check`, `drawing`, `export` (svg).  *(apply, inspect, mesh, dxf, pdf: next milestones.)*

## SPEC ISSUES
(interpretations made where the spec was ambiguous; the orchestrator should patch the spec)

- Canonical key order: common fields first (`id type label material at rotate slope mirror z array embedded visible`), then type
  params in catalog order, exactly as SPEC 4 lists them (the reference docs put `at` last; `fmt` will reorder).
- `slab_edge.recess`: `depth` is measured at the interior (high) end (`recess_bottom_interior` = (from_edge+width, -depth));
  the floor falls toward the exterior by `recess_slope`, so `recess_bottom_exterior` = (from_edge, -(depth+slope)).
- Point lists: when a component has `at`, literal `[x, y]` entries are relative to the placement point (`at.to` + `offset`); the
  `anchor` is ignored for point-list components (membrane, fill, connector, path rebar, polygon/points profiles). Without `at`
  literals are absolute. Ref entries are always absolute.
- Section: edges shared by two cut prisms of the SAME component (CMU shell/grout/mortar boundaries) are drawn with the light
  `beyond` pen; shared edges between different components keep the heavier pen (SPEC 8.1).
- Wood cut marks (X / diagonal) use pen `beyond`. Marks are drawn for any prism whose length runs along Z and whose material has
  `cut_mark` (lumber run z, wood_board panels), not only lumber.
- `lumber` plies stack along the thickness direction (X for upright, Y for flat) for `run: z`.
- Notes in the left column are left-aligned text blocks whose right edge sits at `crop.x0 - gutter` (SPEC: x = crop.x0 - gutter - text_width).
- Dimension text and note text are folded to printable ASCII (em dash -> `-`, etc.) because the stroke font has no other glyphs.
- Title placement: the title block under a view is placed below the lowest annotation (dims can sit under the crop), not only below the crop.
- `W_COVER` checks `along_z` bars only (path bars end at host boundaries by construction).
- cmu `grout: reinforced` grouts the cut cell of every course (the section always cuts a reinforced cell); `none` leaves cells hollow except bond-beam courses.

## Status / known gaps (milestone 1)
Working: parser/fmt/canonicalization, lumber, panel, cmu_wall, concrete (rect, footing, polygon, slab_edge), rebar (along_z, place, path),
anchor_bolt, connector, truss, membrane, fill, insulation (rect/batt), solid; placement DAG, arrays; section view (cut/beyond/occlusion,
hatch, wood marks, break lines, notes, dims, labels, title); SVG export (view and sheet); wasm ABI; CLI check/fmt/drawing/export.
Not yet: apply/inspect/catalog polish, validation codes (W_*), iso view, mesh, DXF, PDF, golden tests, keynote mode.
