# kerf-mcp: Kerf as an MCP server

A stdio [Model Context Protocol](https://modelcontextprotocol.io) server so Claude Code, Claude Desktop or any
MCP client can build Kerf construction details: the model edits a semantic JSON document with engine tools, looks at
rendered PNGs of its own work, and exports SVG / DXF / PDF. Same tools, same engine, same drafting instructions as the
Kerf apps.

## Install

```sh
cargo install --path engines/rust/kerf-cli      # installs `kerf` (includes `kerf mcp`)
# or, from engines/rust:
cargo build --release                           # target/release/kerf  (and target/release/kerf-mcp, the same server standalone)
```

`kerf mcp [--dir <workspace>] [--style <style.kerfstyle.json>]`. The workspace (default: current directory) holds the
documents. Build the lean CLI without MCP and the resvg dependency with `cargo build -p kerf-cli --no-default-features`.

## Register

Claude Code:

```sh
claude mcp add kerf -- kerf mcp --dir ~/details
```

Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "kerf": { "command": "kerf", "args": ["mcp", "--dir", "/Users/you/details"] }
  }
}
```

Use an absolute path for `command` if `kerf` is not on the PATH the client launches with (e.g. `~/.cargo/bin/kerf`).

Then ask for a detail: "Draw a truss bearing on an 8 inch CMU wall with a bond beam, 2021 IRC". Load the `kerf_detail`
prompt (slash command in Claude Code: `/mcp__kerf__kerf_detail`) to give the model the full drafting instructions.

## Tools

| tool | what it does |
|---|---|
| `kerf_new {id, title, meta?}` | create `<id>.kerf.json` in the workspace, make it active |
| `kerf_open {path}` | make an existing document active (id or `*.kerf.json` path); returns its summary |
| `kerf_list {}` | documents in the workspace |
| `kerf_apply {ops, why, doc?}` | apply edit ops atomically (`add` / `update` / `remove` / `set`); returns ok or FAILED, the summary and diagnostics; saves the canonical JSON and appends `{who, why, ops, changed, ts}` to `<id>.kerf.log.jsonl` |
| `kerf_inspect {q, id?, type?, point?, view?, doc?}` | `summary`, `component`, `anchors`, `at`, `catalog`, `doc` |
| `kerf_render {view?, mode?, doc?}` | PNG of the view (1400 px wide, white) as MCP image content, plus a one-line text (scale, notes, error/warning counts, view diagnostics). `mode: sheet` adds the frame and title block |
| `kerf_export {format, view?, sheet?, path?, doc?}` | write `svg`, `dxf` or `pdf` (PDF is always a sheet) into the workspace (default `exports/<id>-<view>.<ext>`); returns path and size |
| `kerf_catalog {type?}` | component catalog (markdown, or one type as JSON) |

Prompt: `kerf_detail {request?}` = `spec/llm/system.md` + MCP notes + the component catalog (markdown).
Resources: `kerf://catalog`, `kerf://style/kerf-standard`, `kerf://doc/<id>` (template `kerf://doc/{id}`; every workspace document is listed).

## Behaviour worth knowing

- Files are the source of truth. Every call re-reads the document, so you can edit it by hand or switch git branches
  between calls. `kerf_apply` writes canonical pretty JSON (clean diffs).
- Active document: set by `kerf_new` / `kerf_open`; if none is set and the workspace holds exactly one document, that one is used.
  Every doc tool also takes an optional `doc` to target a document explicitly.
- All edits go in as `actor: "llm"`: a citation can never be saved as `verified`, it is downgraded to `suggested`. Verification is a designer action in the Kerf apps.
- Failures are tool results with `isError: true` carrying the engine's actionable message and `Fix:` hint; a failed `kerf_apply` leaves the file untouched.
- Exports can only be written inside the workspace directory.

## Test

```sh
cd engines/rust
cargo test -p kerf-cli --test mcp               # spawns `kerf mcp`, drives raw JSON-RPC (handshake, build, render, export + tools/dxf_check.py)
KERF_MCP_PNG_DIR=/tmp/pngs cargo test -p kerf-cli --test mcp reference -- --nocapture   # keep the three reference details' PNGs
```
