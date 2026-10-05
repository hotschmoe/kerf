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
