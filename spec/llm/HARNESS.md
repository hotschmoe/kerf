# Kerf tool loop against the Claude Messages API (eval runner reference)

No app implements this any more: the web chat talks only to local agent CLIs through `kerf serve`
(`spec/SERVE.md`, "Agent bridge"), and `kerf serve` has no `/api/llm` proxy. This is the direct Messages-API
tool loop that `spec/evals/run.mjs` still drives to evaluate models (with `ANTHROPIC_API_KEY`); `system.md` and
`tools.json` are its inputs (`system.md` is also embedded by `kerf guide --full`).

## Request
`POST https://api.anthropic.com/v1/messages`
Headers:
```
content-type: application/json
x-api-key: <designer key>
anthropic-version: 2023-06-01
anthropic-dangerous-direct-browser-access: true      # required for browser (CORS) calls
anthropic-beta: server-side-fallback-2026-07-01      # server-side refusal fallback (see below)
```
Body:
```jsonc
{
  "model": "claude-opus-5-5",                 // picker: claude-opus-5-5 (default), claude-sonnet-5-5
  "max_tokens": 32000,
  "thinking": { "type": "adaptive" },
  "output_config": { "effort": "high" },
  "fallbacks": "default",                      // with the beta header above
  "system": [
    { "type": "text", "text": "<spec/llm/system.md>\n\n# Component catalog\n<engine catalog --markdown>",
      "cache_control": { "type": "ephemeral" } }
  ],
  "tools": <spec/llm/tools.json>,
  "messages": [ … ]
}
```
- Non-streaming is fine for an eval run. The `anthropic-dangerous-direct-browser-access` header is only for browsers.
- Do not send `temperature`, `top_p`, `budget_tokens`, or `tool_choice` `any`/`tool` (all rejected on
  current models). Use `tool_choice` `auto` (default).
- If the beta/fallbacks params are rejected by the API (400 mentioning them), retry once without them
  and remember that for the session.

## Loop
```
messages.push({role:"user", content:[...text, ...images]})
loop:
  resp = POST /v1/messages
  messages.push({role:"assistant", content: resp.content})        // append VERBATIM, incl. thinking blocks
  if resp.stop_reason == "refusal": show resp.stop_details?.explanation; break
  if resp.stop_reason != "tool_use": break
  results = for each tool_use block (in order): run tool → tool_result
  messages.push({role:"user", content: results})                  // ALL results in ONE message
```
- Responses may contain `fallback` blocks (a declined model handed off). Keep them in history verbatim; don't render them except as a console line `▸ FALLBACK <from> → <to>`.
- History is append-only. Never edit or drop earlier turns (thinking blocks are bound to them).
- Tool results: `{"type":"tool_result","tool_use_id":id,"content":[...],"is_error":bool}`.
  - `kerf_apply` → text: `ok`, summary, diagnostics (engine `apply` output, minus the doc). On
    `ok:true` the app replaces its current doc and pushes an op-log entry `{who:"CLAUDE", why, ops}`.
  - `kerf_inspect` → text (JSON or summary).
  - `kerf_render` → `[{"type":"image","source":{"type":"base64","media_type":"image/png","data":<png>}},
    {"type":"text","text":"view A rendered at 1-1/2\"=1'-0\"; 9 notes; 0 errors"}]`. The PNG is the
    view drawing rasterized at ~1400 px wide, white background, true pen weights. (Web: draw the
    Drawing IR on an OffscreenCanvas. Native: rasterize the Drawing IR with the app's renderer or
    the engine's SVG via resvg.)
- Designer images: `{"type":"image","source":{"type":"base64","media_type":"image/png"|"image/jpeg","data":…}}`
  placed before the text block. Downscale so the long side is ≤ 1568 px.
- Cap the loop at 25 tool rounds per designer turn.
- Errors: 401 → "INVALID API KEY"; 429/529 → retry with backoff (2 s, 4 s, 8 s), then show; other
  4xx → show the API error message verbatim in a red console line.
