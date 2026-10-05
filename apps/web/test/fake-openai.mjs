// A tiny fake of POST /v1/chat/completions (streaming SSE, tool calling) for end-to-end tests of the OpenAI-compatible adapter.
// Script: stage 0 = text + kerf_apply(set doc) + kerf_render in ONE assistant turn (parallel tool calls); stage 1 = final text.
// Keys: sk-fake-ok | sk-fake-401 | sk-fake-reasoning (first request rejected: function tools with reasoning_effort) | (none) = keyless
import http from 'node:http';

export function startFakeOpenAI(port = 0, doc) {
  const requests = [];
  let reasoningRejected = false;
  const server = http.createServer((req, res) => {
    const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': 'POST, OPTIONS' };
    if (req.method === 'OPTIONS') { res.writeHead(204, cors); res.end(); return; }
    if (req.method === 'GET' && req.url === '/__requests') { res.writeHead(200, { ...cors, 'content-type': 'application/json' }); res.end(JSON.stringify(requests)); return; }
    if (req.method === 'POST' && req.url === '/v1/chat/completions') {
      let data = '';
      req.on('data', (c) => (data += c));
      req.on('end', () => {
        const b = JSON.parse(data);
        const key = (req.headers.authorization ?? '').replace(/^Bearer /, '');
        requests.push({ key, body: b, headers: req.headers });
        const json = (status, o) => { res.writeHead(status, { ...cors, 'content-type': 'application/json' }); res.end(JSON.stringify(o)); };
        if (key === 'sk-fake-401') return json(401, { error: { message: 'Incorrect API key provided: sk-fake-401.', type: 'invalid_request_error', code: 'invalid_api_key' } });
        if (key === 'sk-fake-reasoning' && !('reasoning_effort' in b) && !reasoningRejected) {
          reasoningRejected = true;
          return json(400, { error: { message: 'Function tools with reasoning_effort are not supported for fake-1 in /v1/chat/completions. Please use /v1/responses instead.', type: 'invalid_request_error', param: 'reasoning_effort' } });
        }
        const msgs = b.messages;
        let stage = 0;
        for (let i = msgs.length - 1; i >= 0; i--) {
          const m = msgs[i];
          if (m.role === 'user' && !(Array.isArray(m.content) && m.content.some((p) => p.type === 'text' && /^\[image returned by tool call/.test(p.text)))) break;
          if (m.role === 'assistant' && m.tool_calls) stage++;
        }
        res.writeHead(200, { ...cors, 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
        const chunk = (delta, finish = null) => res.write(`data: ${JSON.stringify({ id: 'chatcmpl-fake', object: 'chat.completion.chunk', model: b.model, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`);
        if (stage === 0) {
          chunk({ role: 'assistant', content: 'Building the detail through the fake OpenAI endpoint.' });
          const args = JSON.stringify({ ops: [{ op: 'set', path: 'doc', value: doc }], why: 'Build detail (fake OpenAI)' });
          chunk({ tool_calls: [{ index: 0, id: 'call_f1', type: 'function', function: { name: 'kerf_apply', arguments: '' } }] });
          for (let k = 0; k < args.length; k += 500) chunk({ tool_calls: [{ index: 0, function: { arguments: args.slice(k, k + 500) } }] });
          chunk({ tool_calls: [{ index: 1, id: 'call_f2', type: 'function', function: { name: 'kerf_render', arguments: JSON.stringify({ view: doc.views[0].id }) } }] });
          chunk({}, 'tool_calls');
        } else {
          chunk({ role: 'assistant', content: 'Done. The fake OpenAI conversation completed.' });
          chunk({}, 'stop');
        }
        res.write('data: [DONE]\n\n');
        res.end();
      });
      return;
    }
    res.writeHead(404, cors); res.end('not found');
  });
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve({ server, port: server.address().port, requests, close: () => { server.closeAllConnections?.(); server.close(); } })));
}
