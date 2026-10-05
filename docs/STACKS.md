# Stack history

Kerf was prototyped in four stacks on 2026-10-05, all built against the same `spec/SPEC.md`.
The full code of every stack is preserved at tag **`archive/bakeoff-2026-10-05`** (also on branches
`archive/rust-engine`, `archive/egui-app`, `archive/teak-app`):

```sh
git checkout archive/bakeoff-2026-10-05     # engines/rust, engines/zig, apps/web, apps/egui, apps/teak
```

| stack | first load (brotli) | first drawing | verdict |
|---|---|---|---|
| **zig-ts** (Zig engine + TS/DOM/three.js) | ~253 KB | ~0.3 s | **chosen** |
| rust-ts (Rust engine + same web UI) | ~286 KB | ~0.3–0.5 s | archived; the Rust engine is useful as a differential-test oracle |
| zig-teak (all Zig: teak + zunk, WebGPU) | ~490 KB (~370 KB with subset fonts) | ~1.8 s (software WebGPU) | **future native track**; revisit when we need native performance |
| rust-egui (egui + wgpu) | ~1.21 MB | ~2.0–2.5 s | archived |

Results report: https://claude.ai/artifact/HRmGuZFHkwpwuHdaCzVgAt (private to the owner).

## Notes for a future teak migration
- The teak app (`apps/teak` in the archive) already runs the full loop on the Zig engine in-process:
  section/iso/3D/sheet, chat harness, notes/citations, export. Its gaps were single-line chat input,
  a raw-JSON inspector, and native clipboard/file picking (teak issues #4–#8).
- The teak/zunk framework work it needed was merged: teak#3 (Runtime, layout, chrome, effects,
  scene3d, fonts, headless screenshots) and zunk#18 (WebGPU 3D surface: depth/indexed/instanced/MSAA,
  host services, input).
- The engine API (`kerf.call(gpa, fn, json)`) is identical in-process and in wasm, so the web UI's
  feature work transfers as product behavior. Only the view layer would be rewritten.

## The Rust engine + MCP server
`engines/rust/kerf-mcp` was a stdio MCP server (tools kerf_new/open/list/apply/inspect/render/export).
The CLI is now the primary agent interface. If MCP is wanted again, port it onto the Zig CLI:
JSON-RPC over stdio, PNG export is already in the Zig engine.
