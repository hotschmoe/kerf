# KERF ▮▮▮

**Conversational construction details.** You describe a detail or hand Kerf a screenshot. An LLM
builds it with a deterministic drafting engine while you supervise. You review it in section, iso
and 3D, edit the notes, and export DXF / PDF / SVG for your CAD software.

- **Source of truth:** `*.kerf.json` is a semantic, diffable, forkable document holding components,
  relations, notes, and code citations.
- **Consistency:** the office style (`*.kerfstyle.json`) owns every visual decision, so the same
  doc + style gives byte-identical drawings.
- **LLM-first:** components are placed relative to each other through anchors, and every edit
  returns a validation report written for a model.
- **US / IRC / IBC**, structural details first.

## Install the CLI

| platform | command |
|---|---|
| Windows (PowerShell) | `irm https://raw.githubusercontent.com/hotschmoe/kerf/main/install.ps1 \| iex` |
| Linux / macOS | `curl -fsSL https://raw.githubusercontent.com/hotschmoe/kerf/main/install.sh \| sh` |

## Use it with Claude Code, Grok, or any coding agent

```sh
mkdir details && cd details
kerf init        # writes AGENTS.md + CLAUDE.md: "run `kerf guide` first"
claude           # or grok; then ask: "detail of a prefab truss bearing on an 8in CMU wall"
```
The agent runs `kerf guide`, builds the detail with `kerf apply … -w --why "…"`, checks its work by
exporting and reading PNGs, and leaves `*.kerf.json` files in the folder.

```sh
kerf serve --open                       # web workstation for this folder (live as the agent edits)
kerf serve --host 0.0.0.0               # on your LAN (prints a URL with an access token)
```
In the browser you see every detail in the folder update live. The agent's edits show up as LOCAL AGENT
cards. You can edit notes, verify citations, and export DXF / PDF / SVG / PNG, or drive Claude Code,
Grok or Codex from the web console (they run on your machine with your own login). Cloud providers
(Anthropic, OpenAI, Gemini, xAI, OpenRouter, custom/local OpenAI-compatible) work with your API key.

## Repository

| path | what |
|---|---|
| `spec/` | the contract: [`SPEC.md`](spec/SPEC.md), design language, LLM prompts, reference details, evals |
| `engines/zig/` | the engine (Zig 0.16): library, `kerf` CLI, wasm for the browser (zero imports) |
| `apps/web/` | the web workstation (TypeScript + DOM + three.js) |
| `docs/STACKS.md` | the 4-stack bake-off and where the archived stacks live |

License: MIT. Fonts: IBM Plex Mono (OFL), Hershey Simplex (public domain, NBS).
