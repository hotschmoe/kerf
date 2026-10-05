# kerf-mcp: notes

Status: **done**. stdio MCP server, 8 tools, 1 prompt, 3 resource kinds. Entry: `kerf mcp` (kerf-cli feature `mcp`, on by default) or the
standalone `kerf-mcp` binary. See README.md for tools and registration.

## Decisions
- **Hand-rolled JSON-RPC, not `rmcp`.** rmcp 3.5 needs tokio + schemars + proc-macros for what is a ~250 line synchronous request loop here, and our
  tool schemas are plain JSON mirrored from `spec/llm/tools.json`. Hand-rolled: no async runtime, 4 ms startup, trivially auditable. Protocol checked against
  modelcontextprotocol.io (tools, prompts, resources, lifecycle, stdio).
- **Dual-era protocol.** Legacy `initialize` handshake (echoes the client's version if it is one of 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05,
  else answers 2025-11-25) AND the 2026-07-28 stateless era (`server/discover`, per-request `_meta` protocol version, `resultType:"complete"`, error -32022
  for an unsupported version). The 2026 path follows the spec text only; it has not been exercised by a real client yet. JSON-RPC batches are accepted.
- **Files are the state.** Active doc = a path in memory; content is re-read on every call. Writes are tmp + rename.
- `kerf_inspect q=summary` runs the engine `check` (full per-view diagnostics, W_NOTE_TARGET/W_VIEW_FIT) rather than the lighter inspect summary.
- Op log `<id>.kerf.log.jsonl` entries: `{ts, who:"CLAUDE", why, ops, changed}`. The timestamp is the only non-deterministic datum and lives only in the log.
- `kerf_open` accepts any `*.kerf.json` path (it is the user's file); `kerf_export` is restricted to the workspace dir (the model must not be able to overwrite arbitrary files).
- resvg built with `default-features = false` (no text, no raster images, no system fonts): the engine's SVG is paths only (stroke font), so nothing is lost.
- The release profile is `panic = "abort"` (workspace-wide), so an engine panic would end the server process; the engine fuzz tests say this does not happen. The client
  restarts the server and the file on disk is intact (writes happen only after the engine returns).

## Numbers (aarch64, this box, release, opt-level z + lto)
- `kerf` without MCP: 988,528 bytes. `kerf` with MCP (default): 1,774,960 bytes (+786 KB, resvg/tiny-skia/png). `kerf-mcp` standalone: 1,709,424 bytes.
- Startup to `initialize` response: ~4 ms (process spawn included, `time` says 6 ms wall).
- truss-bearing-cmu: `kerf_apply` (set whole doc) ~53 ms; `kerf_render` view A ~170 ms, iso B ~185 ms (check + drawing + svg + raster + base64).

## Tests
`cargo test -p kerf-mcp` (unit: base64, ids, path normalization, timestamp) and `cargo test -p kerf-cli --test mcp` (end to end over stdio: protocol, errors, new/apply/failing apply
leaves file untouched/citation downgrade/op log/inspect/render PNG header + 1400 px/export svg dxf pdf/export path escape refused/`tools/dxf_check.py` audit clean (skipped without
`tools/.venv`)/the three reference details set + render every view). Reference renders were inspected and match the engine goldens.

## SPEC ISSUES
- `spec/llm/tools.json` has no document-management tools; added `kerf_new`, `kerf_open`, `kerf_list`, `kerf_export`, `kerf_catalog` and an optional `doc` argument on the doc tools.
  `kerf_render` here takes optional `view` (defaults to the first view). `kerf_apply.why` is required in the schema but defaults to "edit" if a client omits it.
- `spec/llm/system.md` says `kerf_inspect {"q":"catalog"}` shows a type; kept working (`type` argument), also reachable as `kerf_catalog`.

## REQUESTS
- (kerf-core) none required. Nice to have: `apply` could return the *before* summary labelled as such on failure; today a validation failure (as opposed to an op error) returns the
  summary of the attempted state, so the MCP layer shows only the first summary line ("Document still: ...") after a failure.
- (orchestrator) `cargo install --path engines/rust/kerf-cli` builds the repo-relative `include_str!`s of `spec/`; the install must run from a full checkout.
