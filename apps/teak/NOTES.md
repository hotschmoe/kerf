# zig-teak — Kerf on teak + zunk

All-Zig app: **teak** (Elm-architecture UI, command-buffer renderer, wgpu-native desktop / WebGPU web),
**zunk** (wasm build tool + web bindings), and the Zig engine imported in-process from `engines/zig`.

## Dependencies (local path checkouts)

`build.zig.zon` depends on teak via a path dependency to the owner's local checkout:

```
.teak = .{ .path = "../../../teak" }     // ~/github/teak, branch `kerf`
```

zunk is reached through teak's own `build.zig.zon` (`.zunk = .{ .path = "../zunk" }`, i.e. ~/github/zunk, branch `kerf`).
The Zig engine will be a path dependency on `../../engines/zig` (module `kerf`).

## Status

(in progress — see the sections below as they fill in)

## teak/zunk changes

(filled in as PRs/issues are opened)

## Layout of `src/`

- `draw/`  — Drawing IR parser, stroke font, tessellator (shared by the live viewport and the PNG renderer), picking, CPU rasterizer, PNG encoder.
- `llm/`   — Claude Messages harness as a non-blocking state machine, mock transport, demo script, tool glue.
- `app/`   — the TEA app: session (document + undo + log), docinfo digest, cameras, views.

## REQUESTS

(none yet)

## SPEC ISSUES

(none yet)
