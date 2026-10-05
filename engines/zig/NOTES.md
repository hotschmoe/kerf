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
node tests/serve_smoke.mjs   # integration test of `kerf serve` (129 checks; starts the built zig-out/bin/kerf on temp folders)
zig build -Dtarget=x86_64-windows-gnu -Doptimize=ReleaseSmall   # cross-compiles (also aarch64-windows-gnu, *-linux-musl, *-macos)
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

CLI-only files (not in the `kerf` module, never in the wasm): `workspace.zig` (op log, atomic writes, file-name rules,
new-doc text) | `serve.zig` (server: connections, routing, folder scan, handlers, startup) | `http.zig` (HTTP/1.1 parsing and
responses) | `events.zig` (SSE hub) | `agents.zig` (agent bridge) | `proxy.zig` (`/api/llm` URL rules + streaming forward) |
`ui_stub.zig` (empty `ui_assets`; `-Dui=` replaces it with a generated module of `@embedFile`s).

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

## REQUESTS

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
kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token] [--allow-origin URL]
```
- First stdout line is `kerf serve: http://127.0.0.1:<port>/  (dir <abs>)` (scripts parse it; `--port 0` picks a free port).
  `--host 0.0.0.0` prints `LAN:  http://<lan-ip>:7700/?token=<32 hex>` (token auto-generated from `io.random`; `--token T` fixes it;
  `--no-token` opts out and says so in the banner). The LAN IP comes from a connected UDP socket toward 192.0.2.1 (nothing is sent).
- Exit 1 with a hint when the port is busy (std sets SO_REUSEPORT with SO_REUSEADDR, which would let a second server share the port
  silently, so it probes with a connect first) or `--dir` cannot be opened.
- Everything in SERVE.md is implemented: `/api/info docs docs/:file apply log export events llm agent/run agent/stop`.
  Not in the spec, added: `GET /api/info` also returns `authenticated` and `active_run` (`{run_id, agent}` or null) and, without a valid token,
  just `{version, token_required:true, authenticated:false}` (so the UI can ask for the token); `POST /api/agent/run` also takes
  `images:[{name, data_base64}]` (saved in `<dir>/.kerf/attachments/`, absolute paths appended to the message; web agent's request);
  `GET .../export` also takes `inline=1` (Content-Disposition inline).
- Concurrency: `std.Io.Threaded` (the default `init.io`); every connection is a `Group.concurrent` task (one OS thread, 16 MB
  virtual stack), so SSE streams never block other requests. The poller, agent supervisors and agent output pumps are tasks too.
  Request bodies: Content-Length or chunked, `Expect: 100-continue`, max 64 MiB. Keep-alive on; no idle timeout (std has no read
  timeout), so an idle keep-alive socket holds a thread.
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
- Agent bridge (`agents.zig`): templates (built-in + `<dir>/.kerf/agents.json`, same id replaces) with `{message} {session_id} {dir}
  {file}` and a `{resume}` element; spawn with `std.process.spawn`, cwd = `--dir`, stdin ignored, stdout/stderr piped, env = server env
  with the server binary's directory prepended to PATH (so `kerf` resolves even when it is not installed) and `KERF_ACTOR=agent`.
  stdout lines -> SSE `agent {run_id, event}`: JSON object lines verbatim, anything else `{type:"text", text}`; stderr lines ->
  `{type:"stderr", text}`; over-long (>1 MiB) lines -> `{type:"truncated"}`. `session_id` = latest top-level `session_id` / `sessionId` /
  `thread_id` string seen in any JSON line. At the end: the folder is scanned once (so the agent's `doc_changed`/`log` events come BEFORE
  `exit`), then `{type:"exit", code, session_id?, stopped?}`. Stop = SIGTERM / TerminateProcess, the supervisor reaps. One run at a
  time (409 `E_BUSY`). Detection (`<cmd> --version`, 8 s timeout, cached 30 s, prefetched at start) fills `available/version/reason`.
  The events race the HTTP response of `/api/agent/run` (first events can arrive before the client has the run_id): buffer by run_id.
- Windows: everything is std (Io.Threaded netListen/netAccept on AFD, Child via CreateProcess, NtTerminateProcess for stop). It compiles for
  x86_64/aarch64-windows-gnu but could NOT be run here (wine32 missing, aarch64 host). `claude.cmd`-style shims are not resolved by
  CreateProcess; the official `claude.exe`/`grok.exe`/`codex.exe` work, others can be wrapped via `.kerf/agents.json` (`cmd /c ...`).

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
`ci.yml` (push/PR): `zig build test`, `zig build wasm`, cross-compile smoke (windows-gnu, macos, linux-musl), `serve_smoke.mjs`, and a web job
(wasm -> `npm ci` -> typecheck -> `test:unit` -> `build:serve` -> `zig build -Dui`). `release.yml` (tag `v*`): wasm -> web `build:serve`
(falls back to `build:zig`) -> tests + smoke -> `zig build -Dtarget=<t> -Doptimize=ReleaseSmall -Dui=../../apps/web/dist-serve` for
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
- Tests: `zig build test` = 61 engine tests + 20 serve tests (HTTP parsing/chunked, access rules, proxy URL rules, agent templates, op-log entry
  shape, folder scan events, SSE hub); `tests/serve_smoke.mjs` 129 checks; the web agent's `node test/e2e-serve.mjs --real` (puppeteer) passes against
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
