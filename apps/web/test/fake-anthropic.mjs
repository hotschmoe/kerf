// A tiny fake of POST /v1/messages (streaming SSE) so the REAL @anthropic-ai/sdk code path in the browser can be
// exercised end to end without an API key. Scripts: build detail (apply) -> render -> final report.
// Keys: sk-test-ok (normal) | sk-test-401 | sk-test-refuse | sk-test-529 (first request 529, then ok)
import http from 'node:http';

export function startFake(port = 0, doc) {
  const requests = [];
  let n529 = 0;
  const sse = (res, events) => {
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', 'access-control-allow-origin': '*' });
    for (const [ev, data] of events) res.write(`event: ${ev}\ndata: ${JSON.stringify(data)}\n\n`);
    res.end();
  };
  const message = (content, stop_reason, extra = {}) => {
    const ev = [['message_start', { type: 'message_start', message: { id: 'msg_fake', type: 'message', role: 'assistant', content: [], model: 'claude-opus-5-5', stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 1 } } }]];
    content.forEach((b, i) => {
      if (b.type === 'text') {
        ev.push(['content_block_start', { type: 'content_block_start', index: i, content_block: { type: 'text', text: '' } }]);
        for (let k = 0; k < b.text.length; k += 20) ev.push(['content_block_delta', { type: 'content_block_delta', index: i, delta: { type: 'text_delta', text: b.text.slice(k, k + 20) } }]);
      } else if (b.type === 'tool_use') {
        ev.push(['content_block_start', { type: 'content_block_start', index: i, content_block: { type: 'tool_use', id: b.id, name: b.name, input: {} } }]);
        const j = JSON.stringify(b.input);
        for (let k = 0; k < j.length; k += 400) ev.push(['content_block_delta', { type: 'content_block_delta', index: i, delta: { type: 'input_json_delta', partial_json: j.slice(k, k + 400) } }]);
      }
      ev.push(['content_block_stop', { type: 'content_block_stop', index: i }]);
    });
    ev.push(['message_delta', { type: 'message_delta', delta: { stop_reason, stop_sequence: null, ...extra }, usage: { output_tokens: 20 } }]);
    ev.push(['message_stop', { type: 'message_stop' }]);
    return ev;
  };
  const server = http.createServer((req, res) => {
    const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': 'POST, GET, OPTIONS' };
    if (req.method === 'OPTIONS') { res.writeHead(204, cors); res.end(); return; }
    if (req.method === 'GET' && req.url === '/__requests') { res.writeHead(200, { ...cors, 'content-type': 'application/json' }); res.end(JSON.stringify(requests)); return; }
    if (req.method === 'POST' && req.url.startsWith('/v1/messages')) {
      let body = '';
      req.on('data', (c) => (body += c));
      req.on('end', () => {
        const b = JSON.parse(body);
        const key = req.headers['x-api-key'];
        requests.push({ headers: { ...req.headers, 'x-api-key': key ? '<redacted>' : undefined }, key, body: b });
        const err = (status, type, message) => { res.writeHead(status, { ...cors, 'content-type': 'application/json' }); res.end(JSON.stringify({ type: 'error', error: { type, message } })); };
        if (key === 'sk-test-401') return err(401, 'authentication_error', 'invalid x-api-key');
        if (key === 'sk-test-529' && n529++ === 0) return err(529, 'overloaded_error', 'Overloaded');
        if (key === 'sk-test-refuse') return sse(res, message([{ type: 'text', text: '' }], 'refusal', { stop_details: { type: 'refusal', category: null, explanation: 'Declined by the fake server.' } }));
        const msgs = b.messages;
        const last = msgs[msgs.length - 1];
        const results = last.role === 'user' ? last.content.filter((c) => c.type === 'tool_result') : [];
        let stage = 0;
        for (let i = msgs.length - 1; i >= 0; i--) { const m = msgs[i]; if (m.role === 'user' && m.content.some((c) => c.type === 'text')) break; if (m.role === 'assistant' && m.content.some((c) => c.type === 'tool_use')) stage++; }
        void results;
        const view = doc.views[0].id;
        if (stage === 0) return sse(res, message([{ type: 'text', text: 'Building the detail from the fake API.' }, { type: 'tool_use', id: 'toolu_f1', name: 'kerf_apply', input: { ops: [{ op: 'set', path: 'doc', value: doc }], why: 'Build detail (fake API)' } }], 'tool_use'));
        if (stage === 1) return sse(res, message([{ type: 'tool_use', id: 'toolu_f2', name: 'kerf_render', input: { view } }], 'tool_use'));
        return sse(res, message([{ type: 'text', text: 'Done. The fake API conversation completed.' }], 'end_turn'));
      });
      return;
    }
    res.writeHead(404, cors); res.end('not found');
  });
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve({ server, port: server.address().port, requests, close: () => server.close() })));
}
