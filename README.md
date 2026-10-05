# KERF ▮▮▮

**Conversational construction details.** Describe a detail, or hand it a screenshot. An LLM builds it
in a deterministic drafting engine under your supervision. Review it in section, iso and 3D, edit the
notes, and export DXF / PDF / SVG for your CAD software.

- **Source of truth:** `*.kerf.json` is a semantic, diffable, forkable document (components, relations, notes, code citations).
- **Consistency:** the office style (`*.kerfstyle.json`) owns every visual decision. Same doc + style ⇒ byte-identical drawings.
- **LLM-first:** the engine's API is shaped for LLM tool use. Components are placed relative to each other through anchors, and every edit returns a validation report.
- **US / IRC / IBC**, structural details first.

This repo is currently a **bake-off**: the same spec is implemented in four stacks.

| stack | engine | UI |
|---|---|---|
| rust-ts | `engines/rust` (wasm, raw ABI) | `apps/web` (TypeScript + three.js) |
| zig-ts | `engines/zig` (wasm32-freestanding) | `apps/web` |
| rust-egui | `engines/rust` | `apps/egui` (egui + wgpu) |
| zig-teak | `engines/zig` | `apps/teak` ([teak](https://github.com/hotschmoe/teak) + [zunk](https://github.com/hotschmoe/zunk)) |

Start with [`spec/SPEC.md`](spec/SPEC.md).

License: MIT. Fonts: IBM Plex Mono (OFL), Hershey Simplex (public domain, NBS).
