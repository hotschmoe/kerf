#!/usr/bin/env node
// Local stand-in for POST /v1/messages used to exercise the app's REAL HTTP path (fetch, headers, JSON bodies, errors).
//   node apps/teak/tools/mock_api.mjs [port=8111]
// Behavior by x-api-key: "bad" -> 401, "limit" -> 429 twice then OK, "refuse" -> stop_reason refusal,
// "tool" -> one kerf_inspect tool_use then a final text, anything else -> plain text echo.
import http from 'http';
const port = Number(process.argv[2] || 8111);
let limitHits = 0;
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
http.createServer((req, res) => {
  const cors = {
    'access-control-allow-origin': '*',
    'access-control-allow-headers': 'content-type,x-api-key,anthropic-version,anthropic-beta,anthropic-dangerous-direct-browser-access',
    'access-control-allow-methods': 'POST,OPTIONS',
  };
  if (req.method === 'OPTIONS') { res.writeHead(204, cors); return res.end(); }
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const key = req.headers['x-api-key'] || '';
    let j = {}; try { j = JSON.parse(body); } catch {}
    log(req.method, req.url, 'key=' + key, 'beta=' + (req.headers['anthropic-beta'] || '-'), 'bytes=' + body.length,
        'model=' + j.model, 'msgs=' + (j.messages || []).length, 'fallbacks=' + j.fallbacks);
    const send = (status, obj) => { res.writeHead(status, { ...cors, 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };
    if (key === 'bad') return send(401, { type: 'error', error: { type: 'authentication_error', message: 'invalid x-api-key' } });
    if (key === 'limit' && limitHits++ < 2) return send(429, { type: 'error', error: { type: 'rate_limit_error', message: 'slow down' } });
    const usage = { input_tokens: 10, output_tokens: 10 };
    const msgs = j.messages || [];
    const last = msgs[msgs.length - 1] || {};
    const lastIsToolResult = Array.isArray(last.content) && last.content.some((b) => b.type === 'tool_result');
    if (key === 'refuse') return send(200, { id: 'm1', type: 'message', role: 'assistant', model: j.model, content: [], stop_reason: 'refusal', stop_details: { explanation: 'MOCK: this request was refused.' }, usage });
    if (key === 'tool' && !lastIsToolResult) {
      return send(200, { id: 'm2', type: 'message', role: 'assistant', model: j.model, stop_reason: 'tool_use', usage,
        content: [{ type: 'text', text: 'Checking the document.' }, { type: 'tool_use', id: 'toolu_mock1', name: 'kerf_inspect', input: { q: 'summary' } }] });
    }
    const text = lastIsToolResult ? 'MOCK: tool result received, all good.' :
      `MOCK API OK. ${msgs.length} message(s), model ${j.model}, ${body.length} request bytes, ${(j.tools || []).length} tools.`;
    send(200, { id: 'm3', type: 'message', role: 'assistant', model: j.model, content: [{ type: 'text', text }], stop_reason: 'end_turn', usage });
  });
}).listen(port, () => log('mock api on', port));
