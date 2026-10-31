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
| Windows (PowerShell) | `irm https://raw.githubusercontent.com/hotschmoe/kerf/master/install.ps1 \| iex` |
| Linux / macOS | `curl -fsSL https://raw.githubusercontent.com/hotschmoe/kerf/master/install.sh \| sh` |

Or build from source (Rust, no third-party crates; the toolchain is pinned in `engine/rust-toolchain.toml` and rustup
installs it on first use):
```sh
cd engine && cargo build --release        # engine/target/release/kerf
```
With the web workstation embedded in the binary: `cd apps/web && npm ci && npm run build:serve`, then
`cd ../../engine && KERF_UI=../apps/web/dist-serve cargo build --release`. Details in [`engine/README.md`](engine/README.md).

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
kerf serve --host 0.0.0.0               # on your LAN (prints a URL with an access token; tokens are always on)
```
In the browser you see every detail in the folder update live. The agent's edits show up as LOCAL AGENT
cards. You can edit notes, verify citations, and export DXF / PDF / SVG / PNG, or drive Claude Code,
Grok, Codex or pi from the web console (they run on your machine with your own login; custom agents in
`.kerf/agents.json` need `kerf serve --trust-agents`). Chat is local
agents only: no API keys and no provider calls from Kerf; the web chat needs `kerf serve`.


## Repository

| path | what |
|---|---|
| `spec/` | the contract: [`SPEC.md`](spec/SPEC.md), design language, LLM prompts, reference details, evals |
| `engine/` | the engine (Rust, zero crates): library, `kerf` CLI with `kerf serve`, wasm for the browser (zero imports) |
| `apps/web/` | the web workstation (TypeScript + DOM + three.js) |
| `tools/` | shared checkers and build helpers (DXF/PDF audits, wasm build, screenshots) |
| `docs/` | history and decisions ([`STACKS.md`](docs/STACKS.md): the bake-off, and why Rust replaced Zig) |

CI (`.github/workflows/`): `ci.yml` runs on every push to `master` / `dev-hotschmoe` and on pull requests (fmt, clippy,
all engine gates on Linux, the web tests, the CLI gates on macOS) and calls `windows.yml`, which builds the shipped
configuration and runs the gates on `windows-latest` (x86_64) and `windows-11-arm` (aarch64). A release is
`tools/release.sh <version>`: it bumps `engine/Cargo.toml`, tags `v<version>` and pushes; `release.yml` then re-runs the
gates, builds six binaries on native runners and publishes them with `SHA256SUMS`
(`workflow_dispatch` on `release.yml` is a dry run that stops before publishing).

License: MIT. Fonts: IBM Plex Mono (OFL), Hershey Simplex (public domain, NBS).
