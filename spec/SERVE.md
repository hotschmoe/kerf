# `kerf serve`: local workspace server (spec v0.1)

One binary serves a folder of details to the web UI. Agents (Claude Code, Grok, …) edit the folder
with the CLI. The designer watches and edits in the browser. **The folder is the source of truth.**

```
kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token]
```
- Serves the web UI, embedded in the binary at build time (`apps/web` zig build), at `/`.
- `--host 0.0.0.0` (LAN) requires a token. One is auto-generated and printed as
  `http://<lan-ip>:7700/?token=<t>`. On localhost no token is needed. The UI stores the token in
  sessionStorage and sends `Authorization: Bearer <t>` on every `/api` call (SSE: `?token=`).
- Single process. Reads `*.kerf.json` in `--dir` (non-recursive in v0.1).

## Op log (shared by CLI and server)
Every successful write appends one line to `<file>.log.jsonl` next to the document:
```json
{"ts":"2026-10-06T01:23:45Z","who":"agent","tool":"kerf-cli","why":"Add HETA20 anchors","ops":[…],"changed":["heta"],"summary_head":"DOC truss-cmu 15 components 2 views 0 errors 0 warnings"}
```
- CLI: `kerf apply … -w [--why "…"]` appends with `who` = `$KERF_ACTOR` or `"agent"`. `kerf new` appends a `create` entry.
- Server: designer edits append `who:"designer"`. The agent bridge appends `who:"agent"` (the agent itself writes through the CLI).
- Timestamps are the ONLY nondeterministic bytes in Kerf, and they live only in the log, never in documents or exports.

## HTTP API (JSON; errors are `{ "error": { "code", "message" } }` with 4xx/5xx)
| method & path | body | returns |
|---|---|---|
| `GET /api/info` | | `{ version, dir, token_required, agents: [{ id, name, available, version? }], proxy: true }` |
| `GET /api/docs` | | `[{ file, id, title, mtime_ms, size, components, views, errors, warnings }]`, sorted by file |
| `POST /api/docs` | `{ file, title?, id? }` | creates like `kerf new`; 409 if it exists |
| `GET /api/docs/:file` | | the document JSON. `ETag: "<mtime_ms>-<size>"` |
| `POST /api/docs/:file/apply` | `{ ops, why, actor: "designer", if_match? }` | engine `apply` output (`ok, doc, diagnostics, summary, changed`). Writes on ok and appends to the log. `if_match` mismatch ⇒ 409 with the current ETag (someone else edited) |
| `GET /api/docs/:file/log?since=N` | | `{ entries: [...], next: N }` (N = line index) |
| `GET /api/docs/:file/export?view=A&format=png\|svg\|dxf\|pdf&sheet=1&px=1600` | | bytes with a download filename |
| `GET /api/events` | | **SSE**: `doc_changed {file, mtime_ms, who?}`, `doc_added {file}`, `doc_removed {file}`, `log {file, entry}`, `agent {run_id, event}`, `ping` every 15 s. The server polls the folder every 500 ms (mtime + size) |
| `POST /api/llm` | `{ provider, base_url?, path, headers, body }` | forwards one HTTP request to an LLM provider (avoids browser CORS). Only `https://` URLs, plus `http://localhost`/LAN URLs for `custom`. The response body is streamed back verbatim. API keys travel in `headers` from the browser and are never stored or logged by the server |
| `POST /api/agent/run` | `{ agent: "claude"\|"grok"\|…, message, session_id?, file? }` | starts a local agent CLI headless in `--dir`. Returns `{ run_id }`. Output streams as SSE `agent` events (one per stdout JSON line, or `{type:"text", text}` for plain lines), then `{type:"exit", code, session_id?}` |
| `POST /api/agent/stop` | `{ run_id }` | kills the run |

## Agent bridge
Agents are command templates, so new CLIs are config, not code. Built-ins:

| id | detection | command (cwd = --dir) |
|---|---|---|
| `claude` | `claude --version` | `claude -p <message> --output-format stream-json --verbose [--resume <session_id>] --allowedTools "Bash(kerf:*)" "Read" "Write" "Edit"` |
| `grok` | `grok --version` (verify the real CLI name/flags; record findings in NOTES) | headless equivalent if one exists, else `available:false` with a reason |
| `codex` | `codex --version` | `codex exec <message> --json` (verify) |

The message is prefixed with the workspace context: `You are working in a Kerf details library. Run 'kerf guide' first if you haven't this session. Current document: <file>.`
Exact flags must be verified against each tool's current docs and `--help` output, not guessed.
Results go in `engines/zig/NOTES.md`. Users can add or override agents in `<dir>/.kerf/agents.json`
(same template shape). One run at a time per server in v0.1.

## UI modes (apps/web)
- **Static** (no `/api/info`, e.g. hosted or file-opened): current behavior: OPEN/SAVE files, browser chat with keys.
- **Workspace** (served by `kerf serve`):
  - The left panel gets a LIBRARY list (docs with error/warn counts, live).
  - Opening a doc subscribes to its changes. External edits reload the doc in place, keeping
    view/zoom/selection, and appear in the console as `LOCAL AGENT` cards built from the op log
    (who, why, ops count, summary head).
  - Designer edits POST to `/apply` with `if_match`.
  - The status line shows `LOCAL AGENT · last edit 12 s ago` when agent log entries arrive.
- **Chat providers** (console header picker; key + model per provider stored in localStorage):
  - `Local agent: Claude Code` / `Grok` (workspace mode only, via `/api/agent/run`; no key; uses the
    user's own subscription/login).
  - `Anthropic` (Messages API, existing harness).
  - `OpenAI`, `Gemini`, `xAI (Grok API)`, `OpenRouter`, `Custom (OpenAI-compatible base URL)`. All
    use ONE OpenAI-compatible chat-completions tool-calling adapter (Gemini via its OpenAI
    compatibility endpoint). Tools are the same three kerf tools translated to function schemas.
    Images go as `image_url` data URLs where the model supports vision.
  - In workspace mode provider calls go through `POST /api/llm`; in static mode they go direct
    (CORS permitting, with a clear error otherwise).

## Clarifications (from the implementation)
- `if_match` accepts the ETag with or without quotes. `apply` with invalid ops returns HTTP 200 with
  `ok:false` and writes nothing.
- `GET /api/info` without a valid token returns only `{version, token_required, authenticated:false}`.
- `/api/agent/run` also takes `images: [{media_type, data}]` (saved to `.kerf/tmp/` and referenced in the message).
- Agent templates (`.kerf/agents.json`) use the placeholders `{message} {session_id} {dir} {file}` and a `{resume}`
  element that expands only when a session id exists. The server rescans the folder right before
  emitting an agent's `exit` event, so its `log` events arrive first.
- Verified headless invocations (2026-10-06): Claude Code 2.1.289 `claude -p … --output-format stream-json --verbose
  --permission-mode acceptEdits --allowedTools …`; Grok Build 1.0.41 `grok -p … --output-format streaming-json
  --always-approve --cwd <dir> [-r <id>]`; Codex 0.157.1 `codex exec --sandbox workspace-write --skip-git-repo-check
  --cd <dir> --json [resume <id>] <msg>`.
