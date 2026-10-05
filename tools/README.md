# Kerf eval tools

Shared checkers for all stacks. Run from repo root. One-time: `tools/setup.sh` (idempotent; makes `tools/.venv` via uv, `tools/node_modules`).
Python tools self-re-exec into the venv, so no activation needed. System chromium (`/usr/bin/chromium`) is used; nothing is downloaded.

## Commands

| Tool | Usage |
|---|---|
| DXF check | `tools/dxf_check.py out.dxf [--png out.png] [--json]` |
| PDF check | `tools/pdf_check.py out.pdf [--png out.png] [--page N] [--dpi 150] [--json]` |
| Screenshot | `node tools/shot.mjs <url> <out.png> [--wait-ms 1000] [--width 1280 --height 800] [--webgpu] [--wait-for "<js expr>"] [--eval "<js expr>"] [--full-page] [--dpr N] [--no-gpu] [--args "--flag ..."]` |
| Static server | `node tools/serve.mjs <dir> [port=8080] [--coop] [--host 0.0.0.0]` (run in background: `node tools/serve.mjs dist 8080 &`) |
| Sizes | `tools/size_report.sh dist/*.wasm dist/*.js` (raw / gzip -9 / brotli -q11 + total) |
| Sample DXF | `tools/.venv/bin/python tools/test/make_sample_dxf.py out.dxf` |

- `dxf_check.py`: exit 1 if unreadable or `doc.audit()` has errors. Prints version, `$INSUNITS`, entities per layer/type, extents, text styles, hatch patterns (ANSI31 / AR-CONC / SOLID), linetypes, warnings (unitless, undefined layers). `--png` renders modelspace on a WHITE bg, black ink, via ezdxf drawing addon (matplotlib). View the PNG with the Read tool.
- `pdf_check.py`: exit 1 if unreadable / 0 pages / render fails. Reports page size in inches, `vector=True/False` (path paint ops vs images, recursing into form XObjects), fonts + embedded flag, text sample. Warns on large rasters or blank page. Renders with pypdfium2 (no pdftoppm/mutool on this box).
- `shot.mjs`: prints `[console.*]`, `[pageerror]`, `[requestfailed]`, `[http 4xx]` lines to stdout. Exit 1 on uncaught page error / navigation failure / wait-for timeout. For wasm apps use `--wait-for "window.__ready===true"` (have the app set a flag) rather than guessing `--wait-ms`.
- Typical loop: `node tools/serve.mjs dist 8080 &` then `node tools/shot.mjs http://localhost:8080/ /tmp/s.png --webgpu --wait-ms 2000` then Read the PNG. Kill server: `pkill -f '^node tools/serve.mjs'`.
- `--coop` adds COOP/COEP (needed for SharedArrayBuffer / wasm threads; check `crossOriginIsolated`). Cross-origin subresources then need CORP/CORS.
- Server sends `Cache-Control: no-store`, `Access-Control-Allow-Origin: *`, `application/wasm` for .wasm, `/favicon.ico` -> 204.

## WebGPU / WebGL in headless chromium on THIS machine (aarch64, Mali-G720, Chromium 154) - TESTED

- **WebGPU WORKS, but via SwiftShader (CPU Vulkan), not the Mali GPU.** Adapter: `vendor=google arch=swiftshader desc=SwiftShader Device (LLVM 16.0.0)`. Dawn never selects Mali here, regardless of flags tried (`--use-vulkan=native`, `--use-angle=vulkan`, `--ignore-gpu-blocklist`, ICD overrides...). Expect software speed: fine for correctness screenshots, slow for heavy scenes (use small viewports, e.g. 1280x800 or less).
- **WebGL2 works on hardware Mali** (ANGLE Vulkan) - but only with `--use-angle=vulkan`; with no GPU flags `getContext('webgl2')` returns null. `shot.mjs` adds that flag by default (`--no-gpu` removes it).
- `--webgpu` in shot.mjs passes (all required together; verified canvas renders orange in screenshot):
  `--no-sandbox --ignore-gpu-blocklist --use-angle=vulkan --enable-unsafe-webgpu --enable-webgpu-developer-features --enable-unsafe-swiftshader --enable-features=Vulkan,WebGPU --use-vulkan=swiftshader --use-webgpu-adapter=swiftshader` (headless: `puppeteer headless:'new'`).
  Omit `--enable-unsafe-swiftshader` and you get `requestAdapter() === null` ("No available adapters") or a blank/black WebGPU canvas in screenshots.
- Without `--webgpu`, `navigator.gpu.requestAdapter()` returns null ("Failed to create WebGPU Context Provider").
- Test page: `tools/test/gpu.html` (WebGL2 blue + WebGPU orange canvas, logs adapter). Run: `node tools/serve.mjs tools/test 8123 &` then `node tools/shot.mjs http://localhost:8123/gpu.html /tmp/gpu.png --webgpu --wait-for window.done`. Orange left canvas + blue right canvas = both OK. Always load via `http://localhost` (secure context), never `file://`.

### WebGPU gotchas
- Screenshots capture WebGPU canvases fine, but reading a WebGPU canvas back in-page (`toDataURL`, `createImageBitmap` of the canvas) returned blank in tests. For pixel checks take a screenshot, or render to an offscreen texture + `copyTextureToBuffer` + `mapAsync` (that works: verified).
- Re-render each `requestAnimationFrame` (or at least once after the first frame) before the screenshot; add `--wait-ms 500+`.
- `isFallbackAdapter` is undefined/false even though it is SwiftShader; check `adapter.info.architecture === 'swiftshader'` if you need to know.
- `chrome://gpu` text is not scrapeable via puppeteer; use the test page instead.
- Chromium prints harmless `GPU stall due to ReadPixels` warnings and `Failed to initialize vulkan surface` on stderr.

## Other gotchas
- `python3 -m venv` is broken on this box (no ensurepip); setup.sh uses `uv` (`~/.local/bin/uv`). Without uv it falls back to venv and errors clearly.
- ezdxf `doc.audit()` clean does not mean AutoCAD-clean; also eyeball the PNG (hatch scale/pattern, text height, arc direction, bulge sign).
- DXF `$INSUNITS`: 1 = inches, 4 = mm. dxf_check warns when 0. Hatch pattern names are read from the HATCH entity (`ANSI31`, `AR-CONC`).
- PDF page size is MediaBox in points / 72. Chromium print and Skia produce `vector=True` PDFs; Type3 fonts count as embedded.
- Re-run `tools/setup.sh` after pulling; it only installs what is missing.
