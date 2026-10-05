# engines/rust: Kerf engine in Rust

Status: **milestones 1-4 done** (all component types, validation, apply/inspect/catalog, section + iso HLR, SVG/DXF/PDF/sheet, mesh, goldens, tests). See the bottom for numbers and known gaps.

Workspace: `kerf-core` (lib, all logic), `kerf-cli` (bin `kerf`), `kerf-wasm` (cdylib, raw C ABI, no imports).
Dependencies: `serde_json` (preserve_order) and `i_overlay` (2D polygon booleans). Nothing else.

## Build / run / test

```sh
cd engines/rust
scripts/golden.sh                         # regenerate tests/golden/** + PNGs + dxf_check/pdf_check audits (review the PNGs!)
KERF_FUZZ_SEED=7 KERF_FUZZ_N=60 cargo test -p kerf-core --test fuzz   # mutation robustness (no panics)
node scripts/bench.mjs                    # wasm timings
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

Functions: `version`, `catalog` (markdown = raw UTF-8), `fmt`, `check`, `apply`, `inspect`, `drawing`, `mesh` (`include_fills`), `export` (`svg|dxf|pdf`, `sheet`; PDF always a sheet).
`kerf mesh`/`drawing`/`export` take `--style` and the same inputs as the API.

## SPEC ISSUES
(interpretations made where the spec was ambiguous; the orchestrator has accepted the first group, they are repeated for the record)

- Canonical key order: common fields first, then type params in catalog order (SPEC 4 literal reading).
- `slab_edge.recess.depth` is measured at the interior end; the floor falls by `recess_slope` toward the exterior.
- Point lists with `at`: literals relative to `at.to + offset`, `anchor` ignored; refs absolute.
- Shared edges between prisms of the SAME component draw in `beyond`; between different components the heavier pen wins.
- Wood cut marks use pen `beyond`; marks apply to any prism whose length runs along Z whose material has `cut_mark` (lumber run z, wood_board panels).
- `lumber` plies stack along the thickness direction (X upright, Y flat) for `run: z`; along Z for run x/y (face wide only).
- `cmu_wall grout: reinforced` grouts the cut cell of every course; `none` leaves cells hollow except bond-beam courses. 3D: face shells are split into 15 5/8" units with 3/8" head joints (running bond, alternate courses offset 8") as `Only::Solid3d` prisms; the section uses full-run shells.
- New in this pass: thin METAL cut regions (steel/aluminum/rebar, `fill_solid`) thinner than 2x the pen on paper render as solid fill + stroke in pen `frame` (0.7 mm) so straps/flashing read as a bold line (the orchestrator rule said "material's pen"; the steel pen alone (0.35) was not bold enough beside 0.5 mm cut lines). Thin membranes draw a single centerline stroke in their pen (dashed `vapor` stays dashed), offset away from the host's cut line by (cut+pen)/2 paper so it does not hide under it. Thin wood/panels/mortar are NOT filled (a 7/16" OSB or 3/8" mortar joint at 1"=1' would turn into black bars).
- `W_COVER` checks `along_z` bars only; host for cmu is the bond-beam/course zone, for concrete the prism with `cover.parts` zone override.
- Break lines are drawn for CUT regions only (SPEC 8.1); beyond members (e.g. the truss in detail 1) just stop at the crop. Break pen is style `break` (0.18 mm), which reads faintly on the sheet: suggest style `break` 0.25-0.35 mm.
- Notes: left column text blocks are left-aligned with their right edge at the gutter; columns clear dims/labels (SPEC 16); leader de-crossing is adjacent swaps on the y-order (bounded), then a nudge pass moves a note up/down (<= 8 cap heights) to keep leaders off dim/label text. Dimension text slides along its line when it would sit on linework; hatch is knocked out under dim/label text.
- Title is placed below the lowest annotation. Footnote `* CODE REFERENCE NOT VERIFIED BY DESIGNER` is part of the view drawing; the sheet moves it to the footer.
- Iso: true orthographic projection, drafting isometric (unit axis scale): u = sz*x - sx*z (cos30), v = y - (sx*x + sz*z) sin30 with camera quadrant (sx, sz); arcs split by chord tolerance 0.004" (max step 0.22 rad), not a flat 2 degrees (circles would be 180-gons). NTS scale = ceil2(max(w,h)/5.5) so the view is <= 5.5" on paper. Iso dimensions are ignored; labels are projected at z = cut_z. Iso crop = XY window clipping the solids; the drawing extent for notes is the projected geometry.
- Iso notes: landing = label point of the best unoccluded front face (cut cap first), with a sample-grid fallback; a fully hidden target gives `W_NOTE_TARGET`.
- `check`/`apply` render every view to collect `W_NOTE_TARGET`/`W_VIEW_FIT`; `drawing` diagnostics are the view's own.
- Reference docs: `flush-beam-strap` view A reports `W_VIEW_FIT` (needs ~14.0" x 4.8" of paper at 1 1/2"=1'-0" with notes on both sides; sheet area is 10.25" x 7"): the doc's scale or notes are too big, the engine is right. `truss-bearing-cmu` view B reports `W_NOTE_TARGET n_tie` because `hurricane_tie` has z = -11.2, inside the bird block's z range [-11.25, 11.25], so it is hidden in the iso (doc issue: move the tie to z <= -11.5 or off the block) and `W_VIEW_FIT` borderline for view B (10.70" wide with notes).
- `truss`: `plate: true` draws a dashed rectangle (0.5..5.5" from the bearing, 0.5" up to just under the top chord lower edge); `heel: raised` adds a 1.5" wide `heel_web` prism. Standard-heel geometry verified against SPEC 5.8/16 (see tests/behavior.rs).

## REQUESTS
- spec/styles: `beyond` 0.18 mm reads hairline on members that are the subject of a detail (the beyond truss); REFERENCE-CONTENT 2.1 says 0.25-0.35 mm for members beyond. Suggest `beyond` 0.25 and `break` 0.25.
- `.gitignore` ignores `engines/rust/dist/`; consumers must run `engines/rust/build-wasm.sh` (about 20 s cold) to get `dist/kerf.wasm`.

## Measured numbers (aarch64, this box)
wasm `dist/kerf.wasm` (release, lto, codegen-units 1, panic abort, strip):

| opt-level | raw | gzip -9 | brotli -q11 | node call time (truss detail, export svg) |
|---|---|---|---|---|
| `z` (shipped) | 740,389 | 277,689 | 224,968 | ~17-25 ms |
| `s` | 881,367 | 326,518 | 259,343 | ~8-12 ms |

(`s`/`z` earlier measurement before the last additions: s 881,367 / z 730,711.) Switch with `opt-level` in `Cargo.toml`. Imports: none (verified with `wasm-tools print`). Extra exports `__data_end`, `__heap_base` come from the linker.
Per call in wasm (node, best of 7, `scripts/bench.mjs`): `drawing` 2.5-24 ms per view, `export svg/dxf/pdf` 3-26 ms, `mesh` 2-9 ms, `check` (renders all views) 4-27 ms. Target < 50 ms per view: met with margin.
Native (debug) `kerf export` of a detail is ~150 ms wall including process start; release is a few ms.

## Tests and goldens
`cargo test` runs: unit tests (lengths, ft-in, scales, geometry, hatch, font wrap, stroke), `tests/behavior.rs` (placement, rotate/slope, truss and slab anchors, E_REF_UNKNOWN/E_ANCHOR_UNKNOWN/E_CYCLE/E_DUP_ID messages, W_COVER numbers, W_OVERLAP/W_FLOATING/W_UNTREATED_CONTACT, apply atomicity, citation downgrade, merge patch, leaders never cross, note wrap width), `tests/golden.rs` (byte-identical summary/mesh/drawing JSON/SVG/DXF/PDF/sheet SVG for every view of the three reference details, run twice for determinism, `fmt` idempotence, zero errors), `tests/fuzz.rs` (mutated reference docs through check/mesh/fmt/drawing/export never panic; 20+ seeds run clean).
Golden artifacts and reviewed PNGs: `engines/rust/tests/golden/<detail>/{A,B}.svg|.dxf|.pdf`, `drawing-{A,B}.json`, `summary.txt`, `mesh.json`, `{A,B}.png` (view SVG), `{A,B}-sheet.png` (sheet SVG), `{A,B}-dxf.png`, `{A,B}-pdf.png`, and `*-dxf-check.txt` / `*-pdf-check.txt` (tools/dxf_check.py: zero audit errors on every DXF; tools/pdf_check.py: vector=True, 1 page 11x8.5 in, no raster).

## Known gaps
- Not implemented: DXF `--with-sheet` is implemented but only checked by audit (no visual review); `fill.grade_label`; insulation batt symbol is approximate; keynote legend is a simple text block; `solid` with `rect`/`circle` has no `points` anchors beyond the box; no `PNG` export (resvg optional feature skipped).
- The hurricane tie / anchor-bolt hardware is schematic (J hook as a filleted bar).
- Iso uses no break lines at crop; section break lines only for cut solids.
- CMU 3D grout cells are full-run (reinforced = solid in 3D).
- Notes are not placed around dimension lines, only dimension/label text.
