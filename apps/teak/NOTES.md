# zig-teak: Kerf on teak + zunk

All-Zig app: **teak** (Elm-architecture UI, command-buffer renderer; wgpu-native on desktop, WebGPU on the web), **zunk**
(wasm build tool + web bindings), and the Zig engine imported **in-process** from `engines/zig` (`kerf.call(gpa, fn, json)`).
One `App` (Model/Msg/update/view) runs on web (`src/main_web.zig`), native Linux/X11 (`src/main_ui.zig`) and headless (`src/shot_main.zig`).

## Status: complete vertical path, all DESIGN §7 items

| # | feature | state |
|---|---|---|
| 1 | Layout per DESIGN §2 (header / console / viewport tabs / inspector / 3270 status), narrow (<900 px) panel tabs | done |
| 2 | 2D viewport: Drawing IR (bulged paths, fills, pre-clipped hatch, stroke-font text), true pen widths x zoom (min 1 px), vellum + 1"/1/4" blue grid with fade, pan/zoom/fit, hover outline + select tint by `src`, **drag notes -> `update ... place` op**, live ft-in cursor | done |
| 3 | 3D: depth-tested flat-shaded mesh + instanced ink feature edges + ground grid, MSAA, orbit / pan / zoom, `[FRONT][ISO][TOP][RIGHT]`, selection tint | done (web + native) |
| 4 | Chat per HARNESS.md: key entry (stored in localStorage / `~/.config/teak/kerf`), model picker, image attach (web paste/drop, downscaled in JS, thumbnails), tool loop (`kerf_apply/inspect/render`), `kerf_render` = in-app CPU rasterizer + PNG (~1400 px, white bg), tool activity in manila cards (collapsible, op JSON), append-only history, 429/529 backoff 2/4/8 s, 401, refusal, fallbacks-param retry, 25-round cap | done; **demo mode** (scripted Claude, no key) and a **mock Messages API** (`tools/mock_api.mjs`) for the real HTTP path |
| 5 | Inspector: parts table (NO/ID/TYPE), selected component's `inspect` (params, anchors), notes editor with citations + `UNVERIFIED`/`VERIFIED` rubber stamps and VERIFY toggle via `actor:"designer"`, DIFF op log (DESIGNER/CLAUDE, why, ops, time) with undo, diagnostics (E/W/I tags, click selects), open/save `.kerf.json`, export DXF/PDF/SVG, open-sample menu with the three reference details | done |

Not done / known gaps: no multi-line chat input (single line, scrolls); native has no clipboard/paste/drop, no file picker (env `TEAK_OPEN=path`), no IME
(see teak issues #4-#8); the 3D view has no per-part picking (pick by 2D view / table); Win32 is compile-only; real Claude API untested (no key here).

## Build / run / test

```
cd apps/teak            # Zig 0.16.0 at ~/tools/zig-aarch64-linux-0.16.0/zig
zig build test          # pure logic: draw/ (IR, tessellator, raster, PNG), llm/ (harness), app/ logic        (~250 tests)
zig build test-ui       # UI layer: Model -> view -> layout + the whole chat/demo loop on the REAL zig engine
zig build web           # dist/ (ReleaseFast, stripped); -Dweb-optimize=ReleaseSmall for the small build
node ../../tools/serve.mjs dist 8110 &
node tools/drive.mjs 'http://localhost:8110/?sample=truss&tab=3d' '[{"wait":3500},{"shot":"/tmp/x.png"}]'   # headless Chromium (WebGPU/SwiftShader) driver
zig build ui            # native desktop build (needs X11 + Vulkan to run; `zig build run-ui`)
zig build shot -- out.png --sample truss --tab 3d --select sill_plate   # HEADLESS NATIVE screenshot (wgpu-native on the Vulkan device, no display)
zig build bench         # native timings of the engine-facing pipeline
node tools/mock_api.mjs 8111 &   # then ...?key=tool&api=http://localhost:8111/v1/messages  (keys: bad, limit, refuse, tool)
```

Startup / test parameters (web query string; native `--name value` to `shot`): `sample=truss|slab|strap`, `tab=0|1|3d|sheet`, `select=<id>`,
`insp=parts|notes|diff|diag`, `demo=1`, `prompt=<text>`, `key=<api key, not stored>`, `api=<messages url>`.

## Dependencies: local path checkouts

`build.zig.zon` uses path dependencies to the owner's checkouts (branch `kerf` in each, pushed to origin and open as PRs):

```
.teak = .{ .path = "../../../teak" }   // ~/github/teak   (PR #3)
.kerf = .{ .path = "../../engines/zig" }
```
zunk is reached through teak's own `build.zig.zon` (`.zunk = .{ .path = "../zunk" }`, ~/github/zunk, PR #18). Fonts and spec files are embedded through
anonymous imports declared in `build.zig` (Zig forbids `@embedFile` outside the module).

## Architecture

```
src/
  main_web.zig main_ui.zig shot_main.zig bench.zig   entries
  app/   TEA app: model.zig (Model/Msg) update.zig view.zig (DESIGN layout) theme.zig (tokens -> teak styles)
         docflow.zig (load/edit/undo/refresh) chatglue.zig (harness Steps <-> effects, tool callbacks)
         session.zig (doc + undo + op log + diagnostics over the Engine boundary) engine.zig engine_real.zig
         viewport.zig (2D camera, pick, note drag) scene3d.zig (mesh -> GPU data) cam.zig fx.zig (effect queue) stamp.zig ...
  draw/  Drawing IR + mesh parsers, stroke font, TESSELLATOR (one shared by the live viewport AND the kerf_render PNG), pick, CPU raster, PNG
  llm/   Claude Messages harness as a non-blocking state machine (no I/O), mock transport, demo script, tool glue
```
Key decisions: the app never blocks (HTTP is a declared teak **effect**; tools run synchronously in `update`); the 2D view tessellates into
alpha-feathered triangles (no MSAA dependence) handed to a teak canvas as one batch with a content key; what Claude sees in `kerf_render` is the same
tessellation rasterized on the CPU, so it matches what the designer sees; sheet preview = the engine's `withSheet` Drawing (PDF page) tessellated on a desk.

## Measurements (aarch64 box; web = headless Chromium + SwiftShader software WebGPU, so GPU-bound numbers are pessimistic)

| item | value |
|---|---|
| wasm ReleaseFast (stripped) | 2,246,503 B raw / 678,778 gzip-9 / 489,942 brotli-11 |
| wasm ReleaseSmall (stripped) | 1,145,833 B raw / 419,587 gzip / 333,575 brotli |
| generated JS (`app.js`) | 39,105 raw / 12,076 gzip / 10,636 brotli |
| fonts (3 x IBM Plex Mono TTF, fetched separately) | 410 KB raw / ~145 KB brotli |
| web startup (navigation -> first frame, includes wasm compile, WebGPU init, font load) | ~1.8-1.9 s (first frame itself 90-390 ms: pipeline creation) |
| web mean frame cost in wasm during pan/zoom/idle | 7-16 ms (60-frame means; a frame where nothing changed uploads nothing) |
| native pipeline (`zig build bench`, ReleaseFast) truss / slab / strap | load+check 15.6 / 8.8 / 1.4 ms; drawing JSON 1.9 / 0.9 / 0.5 ms; parse 0.9 / 0.8 / 0.2 ms; **tessellate per frame 0.40 / 0.31 / 0.17 ms** (24k / 26k / 13k triangles); kerf_render 1400 px 169 / 183 / 124 ms (PNG 64 / 71 / 48 KB); mesh+scene 3.4 / 0.9 / 0.5 ms; PDF export 2.9 / 2.7 / 1.8 ms |
| tessellator stress (5k mixed items, ReleaseFast) | 3.3 ms |

## Screenshots (`shots/`, all viewed)

Web (Chromium/WebGPU): `web-section.png` (hover/select tint, inspector), `web-3d.png`, `web-iso.png`, `web-sheet.png`, `web-notes.png` (rotated stamp, citation),
`web-diff.png` (note drag recorded as an op), `web-chat-demo.png` (manila cards, tool lines, render thumbnail), `web-empty.png`.
Native (headless wgpu-native + stb text): `native-*.png`.

## teak/zunk changes (all on branch `kerf` in each repo; pushed; PRs against master)

* teak PR https://github.com/hotschmoe/teak/pull/3 (about 40 commits), zunk PR https://github.com/hotschmoe/zunk/pull/18 (13 commits).
* Issues filed: teak #4 (X11 clipboard/drop), #5 (Win32 effects/headless/tracking), #6 (UTF-8 TextField / multi-line text area; the app's `Editor` is a candidate), #7 (X11 IME, image cache eviction), #8 (layout: shrink / wrapped text).
* **teak**: `Runtime` (web shares `teak.run`; web hand-copied loops deleted); input completeness (buttons/edges/modifiers/UTF-8/shared key policy); interactive canvas +
  scroll routing + `windowMsg`; layout model (fixed sizes, align/justify, spacer, scroll zero-basis fix); chrome styling (borders, hover inversion, underline fields, hard shadows,
  weight/tracking); `core/table`; canvas triangle/line batches; `scene3d` + declarative `resources()` (hatch 8) + MSAA; overlay layering fix; declarative effects (hatch 7: HTTP, files,
  storage, clock, clipboard, paste/drop, query params; web + Linux); fonts (web `.fonts`, native `registerFont`, per-weight stb faces; native `size_px` is now em-size like CSS);
  headless native screenshot path (`Gpu.initOffscreen/readFrame`, `HeadlessHost`, `teak.headless`, `linkHeadless`); `nowMs` fixes (web returned 0 so `Sub`s never fired; X11/Win32 used a removed std API).
* **zunk**: WebGPU 3D surface (depth, indexed + instanced draws, MSAA, offscreen targets, readback, `examples/mesh-3d`); `web.fx` host services bridge; `--font` + `letterSpacing`;
  UTF-8 text, modifiers, horizontal wheel, window-scoped mouseup, `preventDefault` policy.

## REQUESTS (engine)

* `drawing` could take `{sheet: true}` and return the Drawing IR WITH the sheet frame/title block (what `export pdf` places). I call `kerf.sheet.withSheet` + `kerf.drawing.toJson`
  in-process (`src/app/engine_real.zig`, pseudo-function `sheet_drawing`), which only exists for the Zig engine; a spec'd flag would make every stack's Sheet tab uniform.
* `inspect {q:"component"}` is JSON; a markdown/summary variant for humans would improve the inspector (today the app pretty-prints the JSON).

## SPEC ISSUES

* Note `place` is "the model-space position of the text": I use **top-left of the first text line** (baseline y + cap height) like the web app does; the spec should say so.
* Drawing `src` for instances is `id#k`; picking uses `srcBase` so a click selects the component (`ir.srcBase`/`srcMatches`).
* The Drawing IR `bounds` of a `sheet_drawing` is the page; the sheet preview fills it white with a 4 px hard shadow on a desk colour (not in DESIGN.md).
* DESIGN §3 tool lines use `▸ ✓ ◐`; Plex Mono lacks these glyphs and stb_truetype does not fall back, so the app maps them to `> OK |/-\` (3270-style) on every backend.
* Console timestamps come from a `clock` effect (wall time) because `view`/`update` cannot read a clock; before the first answer they read `00:00`.
