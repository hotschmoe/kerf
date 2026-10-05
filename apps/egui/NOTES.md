# rust-egui (apps/egui)

Full-Rust Kerf workstation: egui 0.35 + eframe + wgpu, one codebase for native and web (wasm, WebGPU).
Links `engines/rust/kerf-core` **in-process** (no JSON boundary beyond the engine's own API: `kerf_core::api::call`).

## Status
Feature complete for the DESIGN.md §7 checklist; see "Known gaps". Everything below is exercised by
`cargo test` (18 tests), the headless screenshot runner, and the web build driven through `tools/shot.mjs`.

| area | state |
|---|---|
| Layout (DESIGN §2) | header 40px + 2px rule, console 360px, inspector 320px, 3270 status line; < 900px => tabs [CONSOLE][VIEW][INSPECTOR] |
| Look | IBM Plex Mono (subset to 22 KB per weight in `assets/`), paper/vellum/ink tokens, square corners, hard 2px offset shadows, all chrome hand-painted (buttons, tabs, typed-form fields, stamps, tables). egui defaults are overridden via `theme::install`. |
| 2D viewport | Drawing IR on egui's painter: bulge arcs tessellated, `fill` triangulated (earcut), pre-clipped hatch lines, Hershey stroke-font text, dashed pens, true pen widths (`width_mm/25.4 * scale * zoom`, floor 1 device px), vellum + blue grid (1/4", 1", 12" levels fade by pixel spacing), pan/zoom/pinch/FIT, hover outline, 15% blue tint of the selected cut region, note drag => `update ... place` op |
| SHEET | the engine's sheet SVG (what PDF/SVG export write) parsed by `svgprep.rs` into the same `Prep`, drawn on a white page with a hard shadow. "Exactly what will be exported" by construction. |
| 3D | wgpu paint callback: offscreen 4x MSAA colour + depth, flat-shaded faces (style `color3d`), feature edges as constant-pixel-width screen-space quads, ground grid, orthographic orbit/pan/zoom, `[FRONT][ISO][TOP][RIGHT]`, hover/selection tint, ray picking, **section cut** (`[CUT]`): fragments with z > `cut_z` are discarded and caps are built on the CPU by slicing the mesh with z = cut_z (`section3d.rs`), drawn manila (steel/rebar dark) with cut-pen outlines |
| Chat | `chat.rs`: raw HTTP per HARNESS.md (ehttp: ureq+rustls natively, fetch on web), append-only history incl. thinking/fallback blocks verbatim, all tool results in ONE user message, 25-round cap, `CLAUDE BUSY <spin> ROUND n`, retry 2/4/8 s on 429/5xx/network, 401 => INVALID API KEY, 400 naming fallbacks/beta => retry once without them (remembered), `refusal` => REFUSAL line, `max_tokens` with tool_use => error results (tools not run), designer images (decode, downscale to 1568 px, PNG) before the text block, `[designer edits since your last turn: ...]` appended to the next message |
| Tools | `kerf_apply` (engine `apply`, op-log entry on ok), `kerf_inspect` (engine `inspect`, `doc` = full JSON), `kerf_render` (tiny-skia rasterization of the Drawing IR, 1400 px, white bg, true pen weights; `mode:"sheet"` = parsed sheet SVG) |
| Console UI | settings strip (`KEY`: key field, model picker, DEMO MODE), designer blocks, manila cards, collapsible tool lines (`▸ APPLY  6 OPS   ✓ 0 ERR 1 WARN`, expand to op JSON + result), render thumbnails, attach (file dialog; drag-drop; web paste via `window.__kerf_paste`), Enter sends, Shift+Enter newline |
| Inspector | PARTS (zero-padded table, resolved params/ranges/anchors from engine `inspect`), NOTES (edit text, add/remove citations, UNVERIFIED/VERIFIED rubber-stamp toggle => `actor:"designer"`, place), DIFF (op log with DESIGNER/CLAUDE/FILE tags, expand ops, UNDO LAST OP GROUP), diagnostics (E/W/I squares, click selects), EXPORT DXF/PDF/SVG |
| Files | open sample menu (3 reference details embedded), open `.kerf.json` (dialog / drag-drop), save canonical (engine `fmt` text), exports via engine (native: save dialog, web: Blob download) |
| Persistence | API key / model / demo flag: `~/.config/kerf-egui/settings.json` (0600) natively, `localStorage` on web (own code; eframe's `persistence` feature would add `ron`, 157 KB) |

## Build / run / test
```
cd apps/egui
cargo run -p kerf-egui --release                       # native window (Vulkan/Metal/DX12/GL via wgpu)
cargo run --profile fast -- --screenshot out.png --doc truss --tab A   # headless frame -> PNG (CI smoke test)
cargo test --profile fast                              # 18 tests: chat loop/transport, drive-the-real-engine demo, designer ops/undo, svgprep, section caps
trunk build --release                                  # -> dist/ (WebGPU only); serve: node ../../tools/serve.mjs dist 8090
node ../../tools/shot.mjs "http://localhost:8090/?doc=truss-bearing-cmu&tab=3d" shots/x.png --webgpu --wait-for "window.__ready===true"
```
Headless / debug flags: `--screenshot P --doc NAME|FILE --tab A|B|3d|sheet --select ID --insp parts|notes|diff --size WxH --ppp N
--frames N --demo-turns N --attach FILE --settings --open-tools --hover x,y --click x,y --drag x0,y0:x1,y1`,
`--export OUT.{svg,dxf,pdf,json} --doc X --view A`, `--render OUT.png --doc X --view A [--sheet]` (the exact PNG `kerf_render` returns),
`--call FN --doc X --input JSON` (raw engine call). Web query: `?doc=&tab=&select=&insp=&demo=1&bench=1` (`window.__perf`, `window.__ready_ms`).

Demo mode (KEY → DEMO MODE, or `--demo` / `?demo=1`): `demo.rs` replays a scripted conversation through the same loop
(thinking block, a 2-tool message with `kerf_render`, a rejected op and the recovery) against the real engine.

Toolchain note: the box has no display; native verification is the headless runner (real Vulkan on the Mali GPU), web is
headless chromium + SwiftShader WebGPU via `tools/shot.mjs --webgpu`.

## Measurements (aarch64, 12 cores; truss-bearing-cmu loaded)

| metric | value |
|---|---|
| wasm (`trunk build --release`, wasm-opt -Oz) | 4,689,977 B raw / 1,635,857 gzip-9 / 1,197,081 brotli-11 (+ glue js 103 KB / 17 KB / 14 KB) |
| wasm with optional `--features webgl` (WebGL2 fallback, verified rendering in a browser without WebGPU) | 6,192,184 B raw / 2,238,882 gz / 1,649,498 br (+1.5 MB raw, +0.45 MB br) => OFF by default |
| native release binary (lto fat, opt-level z, stripped) | 9.38 MB (14.6 MB before `strip=symbols`) |
| wasm size breakdown (names build, 8.5 MB unstripped) | egui text stack (skrifa + harfrust + vello_cpu + fearless_simd + read_fonts) ~2.1 MB, kerf-core 0.32 MB, tiny-skia 0.27 MB, image png/jpeg 0.24 MB, wgpu+naga+std rest |
| web startup to first frame (headless chromium, SwiftShader WebGPU, local server) | 2.0-2.45 s (`window.__ready_ms`) |
| web UI pass per frame, detail loaded (`?bench=1`, `window.__perf`) | section 2.9-6 ms avg (steady ~1-3 ms), sheet ~3-6 ms, 3D ~10 ms; software GPU limits throughput to 30-60 fps |
| native UI pass (release, Mali Vulkan, 1440x860) | first frame ~28-32 ms (engine compile + font atlas); steady 0.6-0.9 ms section/sheet, 0.4 ms 3D; tessellation 2.0 ms (34k verts) for the section view |
| kerf-core in-process | load + check + drawing of truss-bearing-cmu: included in the first-frame figure above |

Reproduce: `tools/size_report.sh apps/egui/dist/*.wasm`, `node tools/shot.mjs "...?doc=truss&bench=1" x.png --webgpu --wait-for "window.__ready===true" --wait-ms 5000 --eval "JSON.stringify([window.__perf,window.__ready_ms])"`, `./target/release/kerf-egui --screenshot x.png --doc truss --frames 30` (prints per-frame ms and tessellation).

## Screenshots (apps/egui/shots/)
`native-*.png` (headless wgpu, real Vulkan): truss-section, truss-3d-cut, truss-sheet, monopour-3d, chat-demo, notes, diff, narrow (720 px), empty, settings, hidpi (ppp 2).
`web-*.png` (chromium + SwiftShader WebGPU): truss-section, truss-3d-cut, truss-sheet, monopour-section/3d, flush-iso, truss-notes, nowebgpu (notice), webgl-3d (WebGL2 build). `e2e-*.png`: frames from `tools/egui/e2e.mjs` (real mouse/keyboard: OPEN menu, viewport click, table click, param edit, tabs, typed chat -> demo tool loop, note drag, pasted image). `render-A.png`: the exact PNG `kerf_render` sends to Claude.

## Tests
`cargo test --profile fast` (18), `tools/egui/smoke.sh` (3 samples x 4 tabs headless + dxf_check/pdf_check on engine exports), `node tools/egui/e2e.mjs` (web, real input, 13 checks).
Exports: engine DXF/PDF for truss view A pass `tools/dxf_check.py` and `tools/pdf_check.py`.

## Architecture
```
main.rs        native entry (eframe) + wasm entry (WebRunner, #kerf_canvas) + CLI flags
app.rs         KerfApp state, layout, header/status/viewport shell, Host (ToolHost impl for the chat loop), tests
theme.rs       tokens, fonts, Style/Visuals override, hand-painted widgets
ir.rs          Drawing IR + Mesh serde types; Prep (arcs tessellated, fills triangulated, text -> stroke polylines, pick regions)
view2d.rs      2D painter, grid, camera, hover/select/drag
svgprep.rs     sheet SVG -> Prep (M/L/A/Z, pen classes, even-odd fills)
gpu3d.rs/shader3d.wgsl   3D callback renderer, orbit camera, picking
section3d.rs   mesh slice -> cut caps
chat.rs        harness: Chat state machine, Transport (HTTP/mock), tests
demo.rs        scripted mock conversation
engine.rs      facade over kerf_core::api::call (JSON in/out, like the CLI/wasm ABI)
session.rs     document, op log + undo snapshots, drawing/mesh caches keyed by revision
raster.rs      tiny-skia rasterizer for kerf_render
console.rs inspector.rs view3d.rs   panels
platform.rs    file open/save/download/paste, native vs web
settings.rs    persisted key/model
headless.rs    screenshot / export / render / call CLI (native only)
fixtures/      hand-written Drawing IR + Mesh fixtures + generator (used before kerf-core landed; `engine::stub` still falls back to them for any fn kerf-core reports as unknown)
```

## Decisions / findings
- **egui 0.35, not 0.36.** 0.36 builds too (one-line version change + 3 small API edits), but is no smaller; both pull the
  skrifa + harfrust + vello_cpu + fearless_simd text stack (~2.1 MB of the wasm, not removable from outside epaint).
- **Pick regions.** The engine only emits exact region loops (hatch/fill loops) for hatched or filled materials; cut
  linework for wood/panels/steel is deduplicated open polylines. Picking therefore uses: filled dots, then linework within
  4 px, then exact hatch loops (smallest), then the convex hull of the member's cut linework (approximate). See REQUESTS.
- **`place`** is the top-left of the note's text block (first baseline + cap height); verified against the engine
  (`note_drag_sets_place`). A first drag from an engine-placed note uses that value as its base, so it does not jump.
- **Section caps without engine help:** the mesh has no cap faces, so `section3d.rs` slices triangles with z = cut_z and
  chains the segments into loops (even-odd grouping for holes). Overlapping caps at the same z (rebar in concrete) rely on
  draw order (document order); steel/rebar caps are dark so they read over the manila host.
- **No WebGL fallback.** `wgpu/webgl` pulls naga + glow (~2.8 MB per the gemtd_yolo measurement), not cheap. Browsers without
  `navigator.gpu` get the notice in `index.html`.
- **Settings without `ron`:** see Persistence.
- **Time display** (`REV`, chat `15:42`) uses wall-clock HH:MM (UTC natively, local on web). Engine output never sees it.

## SPEC ISSUES
- SPEC §10 does not say how a UI should pick unhatched cut regions (see Pick regions); the IR has no per-component region items.
- SPEC §6.2 `place`: the text anchor is "top-left of the text block" in the engine; the spec only says "position of text".
- SPEC §13 `drawing` has no `sheet` option; the SHEET tab therefore parses the sheet SVG export. A `drawing {sheet:true}` returning
  the page in paper inches (scale 1) would let UIs skip the SVG parse.
- Spec detail `truss-bearing-cmu` view B has `omit` that the current engine rejects with `E_PARAM` (seen as diagnostics in the app).

## REQUESTS
- **rust-engine:** emit an exact closed `loop` for every cut region of every component (e.g. `{"t":"region","src","loops":[...]}`
  items that are not drawn) so UIs can hit-test and tint wood/panel/steel sections exactly. Repro: `truss-bearing-cmu` view A,
  `sill_plate` has only open `cut` polylines and no hatch loops (the app falls back to the convex hull of its linework).
- **rust-engine (nice to have):** mesh cut-cap faces or a `mesh {cut_z}` option, so 3D section caps need no client-side slicing.

## Known gaps
- Streaming responses (non-streaming is allowed by HARNESS.md); a long `high` effort turn shows `CLAUDE BUSY` until it returns.
- No clipboard image paste on native (file dialog + drag-drop work); web paste is implemented in `index.html` and unit-driven only
  by queue (`window.__kerf_paste`), not by a real clipboard event in the headless browser.
- File dialogs cannot be exercised on this headless box (native saves fall back to the working directory when no portal exists).
- Dimension annotations are selectable but not editable; label text is. Only scalar component params are editable (objects/arrays such as `at` are read-only).
- Iso view B has no hover/select tint for faces (only linework picking), same data limitation as above.
