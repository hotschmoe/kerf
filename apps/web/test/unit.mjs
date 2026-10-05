// Unit tests for pure logic, run through Vite's SSR loader (TS on the fly, no browser):
//   node test/unit.mjs
import { createServer } from 'vite';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const vite = await createServer({ root: web, logLevel: 'error', server: { middlewareMode: true, fs: { allow: ['../..'] } }, appType: 'custom', optimizeDeps: { noDiscovery: true } });
const load = (p) => vite.ssrLoadModule(p);
let n = 0, failed = 0;
const t = async (name, fn) => { n++; try { await fn(); console.log('  ok  ' + name); } catch (e) { failed++; console.log('FAIL  ' + name + '\n' + (e.stack || e)); } };

const { fmtFtIn } = await load('/src/units.ts');
await t('fmtFtIn follows SPEC §1', () => {
  assert.equal(fmtFtIn(0), '0"');
  assert.equal(fmtFtIn(7.625), '7 5/8"');
  assert.equal(fmtFtIn(12), `1'-0"`);
  assert.equal(fmtFtIn(49.5), `4'-1 1/2"`);
  assert.equal(fmtFtIn(-14), `-1'-2"`);
  assert.equal(fmtFtIn(0.5), '1/2"');
  assert.equal(fmtFtIn(11.99), `1'-0"`);
  assert.equal(fmtFtIn(0.03), '0"');
});

const font = JSON.parse(fs.readFileSync(path.join(web, '../../spec/fonts/kerf-simplex.json'), 'utf8'));
const sf = await load('/src/strokefont.ts');
sf.setFont(font);
await t('stroke font layout: width, rotation, alignment', () => {
  const a = sf.layoutText({ s: 'AB', x: 10, y: 5, h: 2.1 });
  assert.ok(Math.abs(a.width - (font.glyphs.A.adv + font.glyphs.B.adv) * 0.1) < 1e-9);
  assert.ok(a.bbox[0] === 10 && Math.abs(a.bbox[3] - 7.1) < 1e-9);
  const c = sf.layoutText({ s: 'AB', x: 10, y: 5, h: 2.1, align: 'center' });
  assert.ok(Math.abs(c.bbox[0] - (10 - a.width / 2)) < 1e-9);
  const r = sf.layoutText({ s: 'AB', x: 0, y: 0, h: 2.1, rot: 90 });
  assert.ok(r.bbox[2] <= 0 + 1e-9 && r.bbox[3] > 0);
  assert.ok(sf.layoutText({ s: 'é', x: 0, y: 0, h: 1 }).lines.length > 0); // unknown glyph -> ?
});

await t('stroke font folds characters it lacks', () => {
  assert.equal(sf.fold('A \u2014 B \u00D7 C \u201Cq\u201D'), 'A - B X C "q"');
  assert.ok(Math.abs(sf.textWidth('\u2014', 1) - sf.textWidth('-', 1)) < 1e-9);
});

const { MockTransport, validateHistory } = await load('/src/chat/mock.ts');
const { Harness, MAX_ROUNDS } = await load('/src/chat/harness.ts');
const { ChatError } = await load('/src/chat/transport.ts');
const sample = JSON.parse(fs.readFileSync(path.join(web, '../../spec/details/truss-bearing-cmu.kerf.json'), 'utf8'));

// A stub App with just what the harness/tools touch.
function stubApp() {
  const calls = [];
  return {
    calls, pendingDesignerEdits: [], doc: null, diagnostics: [], summary: '',
    claude: null, setClaude(c) { this.claude = c; },
    async applyOps(ops, actor, why) { calls.push({ ops, actor, why }); this.doc = ops[0].value; return { ok: true, doc: this.doc, diagnostics: [{ level: 'warning', code: 'W_TEST', message: 'x' }], summary: 'DOC ok', changed: [] }; },
    async inspect() { return { summary: 'SUMMARY' }; },
    describeError(e) { return String(e); },
    views: [], get view() { return undefined; },
  };
}
const run = async (mock, text = 'build', app = stubApp(), sleep = async () => {}) => {
  const h = new Harness(app, { transport: () => mock, model: () => 'claude-opus-5-5', catalogMd: () => 'CAT', sleep });
  const ev = []; h.on((e) => ev.push(e));
  await h.send(text);
  return { h, ev, app };
};
await t('harness: tool loop, ALL results in ONE user message, history append-only', async () => {
  const mock = new MockTransport({ loadDoc: async () => sample });
  const { h, ev, app } = await run(mock);
  // build -> apply, (render fails on stub app: no doc views) -> end
  assert.ok(app.calls.length === 1 && app.calls[0].actor === 'llm');
  for (let i = 0; i < h.messages.length - 1; i++) assert.notEqual(h.messages[i].role, 'weird');
  const roles = h.messages.map((m) => m.role).join(',');
  assert.match(roles, /^user,assistant,user,assistant,user,assistant$/);
  assert.equal(ev.filter((e) => e.type === 'tool' && e.phase === 'end').length, 2);
  assert.equal(h.busy, false);
  const first = JSON.stringify(h.messages[0]);
  await h.send('again');
  assert.equal(JSON.stringify(h.messages[0]), first); // never edited
  validateHistory({ messages: h.messages });
});
await t('harness: designer edits are appended to the next user message', async () => {
  const mock = new MockTransport({ loadDoc: async () => sample });
  const app = stubApp(); app.pendingDesignerEdits.push('EDIT: moved note n1', 'UNDO: x');
  const { h } = await run(mock, 'hi', app);
  const txt = h.messages[0].content.filter((b) => b.type === 'text').map((b) => b.text).join('\n');
  assert.match(txt, /\[designer edits since your last turn: EDIT: moved note n1; UNDO: x\]/);
  assert.equal(app.pendingDesignerEdits.length, 0);
});
await t('harness: 529 backs off 2s,4s,8s then surfaces the error; 401 -> INVALID API KEY', async () => {
  const waits = [];
  const sleep = async (ms) => { waits.push(ms); };
  let r = await run(new MockTransport({ loadDoc: async () => sample, failFirst: [529, 529, 529, 529] }), 'x', stubApp(), sleep);
  assert.deepEqual(waits, [2000, 4000, 8000]);
  assert.ok(r.ev.some((e) => e.type === 'notice' && e.level === 'err'));
  waits.length = 0;
  r = await run(new MockTransport({ loadDoc: async () => sample, failFirst: [429] }), 'x', stubApp(), sleep);
  assert.deepEqual(waits, [2000]);
  assert.ok(!r.ev.some((e) => e.type === 'notice' && e.level === 'err')); // recovered
  r = await run(new MockTransport({ loadDoc: async () => sample, failFirst: [401] }), 'x', stubApp(), sleep);
  assert.ok(r.ev.some((e) => e.type === 'notice' && /INVALID API KEY/.test(e.text)));
});
await t('harness: fallbacks param rejected once -> retried without, remembered', async () => {
  let n = 0; const seen = [];
  const tr = { async send(req) { seen.push(req.useFallbacks); n++; if (req.useFallbacks) throw new ChatError(400, 'unknown field: fallbacks'); return { content: [{ type: 'text', text: 'ok' }], stop_reason: 'end_turn' }; } };
  const { h } = await run(tr);
  assert.deepEqual(seen, [true, false]);
  await h.send('again');
  assert.deepEqual(seen, [true, false, false]);
});
await t('harness: refusal shows explanation and stops', async () => {
  const tr = { async send() { return { content: [{ type: 'text', text: '' }], stop_reason: 'refusal', stop_details: { explanation: 'because policy' } }; } };
  const { ev } = await run(tr);
  assert.ok(ev.some((e) => e.type === 'notice' && /because policy/.test(e.text)));
});
await t('harness: tool-round cap', async () => {
  let n = 0;
  const tr = { async send() { n++; return { content: [{ type: 'tool_use', id: 'toolu_' + n, name: 'kerf_inspect', input: { q: 'summary' } }], stop_reason: 'tool_use' }; } };
  const app = stubApp(); app.doc = sample;
  const { ev } = await run(tr, 'x', app);
  assert.equal(n, MAX_ROUNDS);
  assert.ok(ev.some((e) => e.type === 'notice' && /25 TOOL ROUNDS/.test(e.text)));
});
await t('harness: fallback blocks kept verbatim in history + console line', async () => {
  const tr = { async send() { return { content: [{ type: 'fallback', from_model: 'A', to_model: 'B' }, { type: 'text', text: 'hi' }], stop_reason: 'end_turn' }; } };
  const { h, ev } = await run(tr);
  assert.equal(h.messages[1].content[0].type, 'fallback');
  assert.ok(ev.some((e) => e.type === 'fallback' && /A → B/.test(e.text)));
});

const { RawEngine, EngineCallError } = await load('/src/engine-raw.ts');
await t('raw ABI loader: echo (utf-8), memory growth, error rc=1, bytes', async () => {
  const eng = await RawEngine.load(fs.readFileSync(path.join(web, 'test/fixtures/echo.wasm')));
  const v = eng.callJson('version', {});
  assert.equal(v.engine, 'kerf-echo');
  const obj = { s: 'h\u00e9llo \u2192 \u00bd"', n: [1, 2.5], big: 'y'.repeat(100000) };
  assert.deepEqual(eng.callJson('echo', obj), obj);
  const big = eng.callBytes('big', 5000000); // forces memory.grow; views must be re-read
  assert.equal(big.length, 5000000);
  assert.deepEqual(eng.callJson('echo', obj), obj); // still fine after growth
  await assert.rejects(async () => eng.callJson('nope', {}), (e) => e instanceof EngineCallError && /boom/.test(e.message) && e.payload.error === 'boom');
  assert.ok(eng.stats.length >= 4 && eng.stats.every((s) => s.ms >= 0));
});


// ---------------------------------------------------------------- OpenAI-compatible adapter
const oa = await load('/src/chat/openai.ts');
const net = await load('/src/chat/net.ts');
const prov = await load('/src/chat/providers.ts');
const fx = (n) => fs.readFileSync(path.join(web, 'test/fixtures/openai', n), 'utf8');
/** Response whose body arrives in awkward pieces (splits inside lines and inside a multi-byte character) */
const sseResponse = (text, pieces = 7) => {
  const bytes = new TextEncoder().encode(text);
  const stream = new ReadableStream({ start(c) { for (let i = 0; i < bytes.length; i += pieces) c.enqueue(bytes.slice(i, i + pieces)); c.close(); } });
  return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } });
};
const jsonResponse = (text, status = 200) => new Response(text, { status, headers: { 'content-type': 'application/json' } });
const KERF_TOOLS = JSON.parse(fs.readFileSync(path.join(web, '../../spec/llm/tools.json'), 'utf8'));
const cfg0 = { provider: 'openai', baseUrl: 'https://api.openai.com/v1', apiKey: 'sk-test', vision: true };
const PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';
const renderResult = (id, caption = 'view A rendered') => ({ type: 'tool_result', tool_use_id: id, content: [{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: PNG } }, { type: 'text', text: caption }] });

await t('openai: tools translate to function schemas; body has no max_tokens/temperature and streams', () => {
  const body = oa.buildBody({ model: 'gpt-6.1-sol', system: 'SYS', tools: KERF_TOOLS, messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }], useFallbacks: true }, cfg0);
  assert.deepEqual(body.tools.map((x) => x.function.name), ['kerf_apply', 'kerf_inspect', 'kerf_render']);
  assert.equal(body.tools[0].type, 'function');
  assert.deepEqual(body.tools[0].function.parameters, KERF_TOOLS[0].input_schema);
  assert.equal(body.tools[0].function.input_schema, undefined);
  assert.equal(body.stream, true);
  for (const k of ['max_tokens', 'max_completion_tokens', 'temperature', 'tool_choice', 'reasoning_effort', 'thinking']) assert.ok(!(k in body), k);
  assert.deepEqual(body.messages, [{ role: 'system', content: 'SYS' }, { role: 'user', content: 'hi' }]);
  assert.equal(oa.buildBody({ model: 'm', system: '', tools: [], messages: [], useFallbacks: false }, cfg0, { reasoningNone: true }).reasoning_effort, 'none');
});

await t('openai: history translation: images, tool calls, multi-tool results, render image follows ALL tool messages', () => {
  const hist = [
    { role: 'user', content: [{ type: 'image', source: { type: 'base64', media_type: 'image/jpeg', data: 'QUJD' } }, { type: 'text', text: 'recreate this' }, { type: 'text', text: '[designer edits since your last turn: EDIT: x]' }] },
    { role: 'assistant', content: [{ type: 'thinking', thinking: 'hm', signature: 's' }, { type: 'text', text: 'On it.' },
      { type: 'tool_use', id: 'call_1', name: 'kerf_apply', input: { ops: [{ op: 'set', path: 'doc', value: {} }], why: 'build' } },
      { type: 'tool_use', id: 'call_2', name: 'kerf_render', input: { view: 'A' } }] },
    { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'call_1', content: [{ type: 'text', text: 'ok\nDOC x' }] }, renderResult('call_2', 'view A rendered; 9 notes'),] },
    { role: 'assistant', content: [{ type: 'fallback', from_model: 'a', to_model: 'b' }, { type: 'text', text: 'Done.' }] },
  ];
  const m = oa.toOpenAIMessages('SYS', hist, cfg0);
  assert.deepEqual(m.map((x) => x.role), ['system', 'user', 'assistant', 'tool', 'tool', 'user', 'assistant']);
  assert.deepEqual(m[1].content[0], { type: 'image_url', image_url: { url: 'data:image/jpeg;base64,QUJD' } });
  assert.equal(m[1].content.length, 3);
  assert.equal(m[2].content, 'On it.');
  assert.deepEqual(m[2].tool_calls.map((c) => [c.id, c.type, c.function.name]), [['call_1', 'function', 'kerf_apply'], ['call_2', 'function', 'kerf_render']]);
  assert.equal(typeof m[2].tool_calls[0].function.arguments, 'string');
  assert.equal(JSON.parse(m[2].tool_calls[0].function.arguments).why, 'build');
  assert.deepEqual(m[3], { role: 'tool', tool_call_id: 'call_1', content: 'ok\nDOC x' });
  assert.equal(m[4].tool_call_id, 'call_2');
  assert.equal(typeof m[4].content, 'string'); // tool messages are text only
  assert.match(m[4].content, /view A rendered; 9 notes/);
  assert.match(m[4].content, /image rendered by this call follows/);
  const img = m[5].content.find((p) => p.type === 'image_url');
  assert.equal(img.image_url.url, 'data:image/png;base64,' + PNG);
  assert.match(m[5].content[0].text, /call_2/);
  assert.equal(m[6].content, 'Done.'); // fallback block dropped
  assert.ok(!JSON.stringify(m).includes('thinking'));
});

await t('openai: no-vision model: images replaced by a visible stand-in, never sent', () => {
  const hist = [{ role: 'user', content: [{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: PNG } }, { type: 'text', text: 'hi' }] },
    { role: 'assistant', content: [{ type: 'tool_use', id: 'c1', name: 'kerf_render', input: { view: 'A' } }] }, { role: 'user', content: [renderResult('c1')] }];
  const m = oa.toOpenAIMessages('S', hist, { ...cfg0, vision: false });
  assert.ok(!JSON.stringify(m).includes('image_url') && !JSON.stringify(m).includes(PNG));
  assert.match(m[1].content, /1 attached image omitted/);
  assert.equal(m.at(-1).role, 'tool');
  assert.match(m.at(-1).content, /image omitted/);
});

await t('openai: Gemini quirks: tool messages carry name; extra_content (thought signature) is replayed verbatim', async () => {
  const resp = await oa.parseResponse(sseResponse(fx('gemini-stream-toolcall.sse'), 13));
  assert.equal(resp.stop_reason, 'tool_use'); // finish_reason was "stop"
  const tu = resp.content.find((b) => b.type === 'tool_use');
  assert.deepEqual(tu.input, { q: 'summary' });
  assert.ok(tu.id.startsWith('call_kerf_'), 'empty id replaced: ' + tu.id);
  assert.deepEqual(tu.oai_extra, { google: { thought_signature: 'CiQBjz1rX3sig==' } });
  const hist = [{ role: 'user', content: [{ type: 'text', text: 'x' }] }, { role: 'assistant', content: resp.content }, { role: 'user', content: [{ type: 'tool_result', tool_use_id: tu.id, content: [{ type: 'text', text: 'S' }] }] }];
  const m = oa.toOpenAIMessages('S', hist, { ...cfg0, provider: 'gemini', toolMessageName: true });
  assert.deepEqual(m[2].tool_calls[0].extra_content, { google: { thought_signature: 'CiQBjz1rX3sig==' } });
  assert.deepEqual(m[3], { role: 'tool', tool_call_id: tu.id, content: 'S', name: 'kerf_inspect' });
  const plain = oa.toOpenAIMessages('S', hist, cfg0);
  assert.equal(plain[3].name, undefined); // only Gemini gets `name`
});

await t('openai: streamed text deltas, awkward chunking, multi-byte split, [DONE]', async () => {
  const seen = [];
  const r = await oa.parseResponse(sseResponse(fx('openai-stream-text.sse'), 5), (d) => seen.push(d));
  assert.equal(r.content[0].text, 'Built the detail — 8" CMU wall.');
  assert.equal(seen.join(''), r.content[0].text);
  assert.ok(seen.length >= 2);
  assert.equal(r.stop_reason, 'end_turn');
});

await t('openai: streamed parallel tool calls with interleaved argument fragments', async () => {
  const r = await oa.parseResponse(sseResponse(fx('openai-stream-multitool.sse'), 11));
  assert.equal(r.stop_reason, 'tool_use');
  const uses = r.content.filter((b) => b.type === 'tool_use');
  assert.deepEqual(uses.map((u) => [u.id, u.name]), [['call_apply', 'kerf_apply'], ['call_render', 'kerf_render']]);
  assert.deepEqual(uses[0].input, { ops: [{ op: 'update', path: 'meta', value: { k: 1 } }], why: 'tag' });
  assert.deepEqual(uses[1].input, { view: 'A' });
  assert.equal(r.content.find((b) => b.type === 'text').text, 'Applying, then rendering.');
});

await t('openai: non-streaming JSON body (content null + tool_calls) parses the same', async () => {
  const r = await oa.parseResponse(jsonResponse(fx('openai-tool-call.json')));
  assert.equal(r.stop_reason, 'tool_use');
  assert.deepEqual(r.content, [{ type: 'tool_use', id: 'call_a1b2c3', name: 'kerf_inspect', input: { q: 'summary' } }]);
});

await t('openai: OpenRouter keep-alive comments ignored; reasoning_details kept and replayed; usage-only chunk ok', async () => {
  const r = await oa.parseResponse(sseResponse(fx('openrouter-stream.sse'), 17));
  assert.equal(r.stop_reason, 'tool_use');
  const pass = r.content.find((b) => b.type === 'oai_passthrough');
  assert.deepEqual(pass.fields.reasoning_details, [{ type: 'reasoning.text', text: 'Let me look', index: 0 }]);
  const m = oa.toOpenAIMessages('S', [{ role: 'user', content: [{ type: 'text', text: 'x' }] }, { role: 'assistant', content: r.content }], { ...cfg0, provider: 'openrouter' });
  assert.deepEqual(m[2].reasoning_details, pass.fields.reasoning_details);
  assert.equal(m[2].content, 'Checking.');
  assert.equal(m[2].tool_calls[0].id, 'toolu_or1');
  assert.ok(!('reasoning_content' in m[2]));
});

await t('openai: xAI delivers a whole call in one chunk', async () => {
  const r = await oa.parseResponse(sseResponse(fx('xai-stream-whole-call.sse')));
  assert.deepEqual(r.content, [{ type: 'tool_use', id: 'call_14729012', name: 'kerf_render', input: { view: 'A', mode: 'sheet' } }]);
});

await t('openai: invalid tool arguments are surfaced to the model as a tool error, not thrown', async () => {
  const r = await oa.parseResponse(jsonResponse(JSON.stringify({ choices: [{ message: { role: 'assistant', content: null, tool_calls: [{ id: 'c9', type: 'function', function: { name: 'kerf_apply', arguments: '{"ops":[{"op":' } }] }, finish_reason: 'length' }] })));
  assert.equal(r.stop_reason, 'tool_use');
  const { runTool } = await load('/src/chat/tools.ts');
  const out = await runTool({ describeError: String }, 'kerf_apply', r.content[0].input);
  assert.equal(out.is_error, true);
  assert.match(out.content[0].text, /not valid JSON/);
});

await t('openai: transport request shape + error mapping (401, 429, Gemini array error, OpenRouter nested error)', async () => {
  const calls = [];
  const mk = (provider, respond, extra = {}) => new oa.OpenAITransport({ ...cfg0, provider, headers: extra.headers, fetch: async (r) => { calls.push(r); return respond(r); }, ...extra });
  const req = { model: 'm1', system: 'S', tools: KERF_TOOLS, messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }], useFallbacks: true };
  let tr = mk('openai', () => sseResponse(fx('openai-stream-text.sse')));
  await tr.send(req, {});
  assert.equal(calls[0].path, '/chat/completions');
  assert.equal(calls[0].baseUrl, 'https://api.openai.com/v1');
  assert.equal(calls[0].headers.authorization, 'Bearer sk-test');
  assert.equal(calls[0].provider, 'openai');
  assert.equal(calls[0].body.model, 'm1');
  tr = mk('custom', () => sseResponse(fx('openai-stream-text.sse')), { apiKey: '' });
  await tr.send(req, {});
  assert.ok(!('authorization' in calls[1].headers)); // keyless custom endpoint (e.g. Ollama)
  tr = mk('openrouter', () => sseResponse(fx('openai-stream-text.sse')), { headers: { 'X-Title': 'Kerf' } });
  await tr.send(req, {});
  assert.equal(calls[2].headers['X-Title'], 'Kerf');
  const err = async (resp, f = (r) => r) => { try { await mk('openai', () => resp).send(req, {}); } catch (e) { return f(e); } throw new Error('no error'); };
  let e = await err(jsonResponse(fx('error-openai-401.json'), 401));
  assert.ok(e instanceof ChatError && e.status === 401 && !e.retryable && /Incorrect API key/.test(e.message));
  e = await err(jsonResponse('{"error":{"message":"Rate limit reached"}}', 429));
  assert.ok(e.retryable && e.status === 429);
  e = await err(jsonResponse(fx('error-gemini-array.json'), 400));
  assert.match(e.message, /function call turn comes immediately after/);
  e = await err(jsonResponse(fx('error-openrouter-nested.json'), 400));
  assert.match(e.message, /Provider returned error/); assert.match(e.message, /tool_use ids must be unique/);
  e = await err(new Response('upstream gone', { status: 502 }));
  assert.ok(e.retryable && /upstream gone/.test(e.message));
});

await t('openai: reasoning_effort rejection (OpenAI gpt-5.4+/gpt-6 with tools) is retried once with reasoning_effort "none" and remembered', async () => {
  const bodies = [];
  const tr = new oa.OpenAITransport({ ...cfg0, fetch: async (r) => { bodies.push(structuredClone(r.body)); return bodies.length === 1 ? jsonResponse(fx('error-openai-reasoning.json'), 400) : sseResponse(fx('openai-stream-text.sse')); } });
  const req = { model: 'gpt-6.1-sol', system: 'S', tools: KERF_TOOLS, messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }], useFallbacks: true };
  const r = await tr.send(req, {});
  assert.equal(r.stop_reason, 'end_turn');
  assert.deepEqual(bodies.map((b) => b.reasoning_effort), [undefined, 'none']);
  await tr.send(req, {});
  assert.equal(bodies[2].reasoning_effort, 'none');
});

await t('openai: Harness multi-round loop through the adapter (apply + inspect in ONE assistant turn -> two tool messages -> final text)', async () => {
  const requests = [];
  const script = [
    fx('openai-stream-multitool.sse').replace('kerf_render', 'kerf_inspect').replace('{\\"view\\":', '{\\"q\\":').replace('\\"A\\"}', '\\"summary\\"}'),
    fx('openai-stream-text.sse'),
  ];
  const tr = new oa.OpenAITransport({ ...cfg0, fetch: async (r) => { requests.push(structuredClone(r.body)); return sseResponse(script[requests.length - 1], 23); } });
  const app = stubApp();
  app.applyOps = async function (ops, actor, why) { app.calls.push({ ops, actor, why }); app.doc = { id: 'x' }; return { ok: true, doc: app.doc, diagnostics: [], summary: 'DOC x ok', changed: [] }; };
  app.doc = { id: 'x' };
  const h = new Harness(app, { transport: () => tr, model: () => 'gpt-6.1-sol', catalogMd: () => 'CAT', sleep: async () => {}, label: () => 'KERF/OPENAI' });
  const ev = []; h.on((e) => ev.push(e));
  await h.send('go');
  assert.equal(requests.length, 2);
  assert.equal(app.calls.length, 1);
  assert.equal(app.calls[0].why, 'tag');
  const roles2 = requests[1].messages.map((m) => m.role).join(',');
  assert.equal(roles2, 'system,user,assistant,tool,tool');
  assert.deepEqual(requests[1].messages.filter((m) => m.role === 'tool').map((m) => m.tool_call_id), ['call_apply', 'call_render']);
  assert.match(requests[1].messages[3].content, /^ok/);
  assert.match(requests[1].system ?? requests[1].messages[0].content, /CAT/);
  assert.equal(ev.filter((e) => e.type === 'tool' && e.phase === 'end').length, 2);
  assert.ok(ev.some((e) => e.type === 'assistant-start' && e.who === 'KERF/OPENAI'));
  assert.equal(h.messages.at(-1).content[0].text, 'Built the detail — 8" CMU wall.');
  assert.equal(h.busy, false);
});

await t('net: direct fetch failure -> clear CORS message (not retryable); proxy wraps the request for POST /api/llm', async () => {
  const realFetch = globalThis.fetch;
  try {
    globalThis.fetch = async () => { throw new TypeError('Failed to fetch'); };
    await assert.rejects(net.directFetch({ provider: 'xai', baseUrl: 'https://api.x.ai/v1', path: '/chat/completions', headers: {}, body: {} }),
      (e) => e instanceof net.CorsBlockedError && e.retryable === false && /API\.X\.AI/.test(e.message) && /kerf serve/.test(e.message));
    let seen;
    globalThis.fetch = async (u, init) => { seen = { u, init }; return new Response('{}'); };
    await net.proxyFetch({ apiBase: 'http://h/api', authHeaders: () => ({ authorization: 'Bearer T' }) })({ provider: 'gemini', baseUrl: 'https://g/v1beta/openai', path: '/chat/completions', headers: { authorization: 'Bearer K' }, body: { a: 1 } });
    assert.equal(seen.u, 'http://h/api/llm');
    assert.equal(seen.init.headers.authorization, 'Bearer T');
    assert.deepEqual(JSON.parse(seen.init.body), { provider: 'gemini', base_url: 'https://g/v1beta/openai', path: '/chat/completions', headers: { authorization: 'Bearer K' }, body: { a: 1 } });
  } finally { globalThis.fetch = realFetch; }
});

await t('providers: default models, resolveChoice, per-provider key storage', () => {
  const ls = new Map();
  globalThis.localStorage = { getItem: (k) => ls.get(k) ?? null, setItem: (k, v) => ls.set(k, String(v)), removeItem: (k) => ls.delete(k), get length() { return ls.size; }, key: (i) => [...ls.keys()][i] };
  try {
    assert.deepEqual(prov.PROVIDERS.map((p) => p.id), ['anthropic', 'openai', 'gemini', 'xai', 'openrouter', 'custom']);
    for (const p of prov.PROVIDERS.filter((x) => x.id !== 'custom')) assert.ok(p.models.some((m) => m.id === p.defaultModel), p.id);
    assert.equal(prov.store.model('xai'), 'grok-4.7');
    assert.equal(prov.store.model('anthropic'), 'claude-opus-5-5');
    prov.store.setKey('openai', ' sk-1 '); prov.store.setKey('anthropic', 'sk-ant-9');
    assert.equal(prov.store.key('openai'), 'sk-1');
    assert.equal(ls.get('kerf.apiKey'), 'sk-ant-9'); // Anthropic keeps its original storage key
    assert.equal(prov.store.key('gemini'), '');
    assert.ok(prov.configured('openai') && !prov.configured('gemini') && !prov.configured('custom'));
    prov.store.setModel('custom', 'llama3.3');
    assert.ok(prov.configured('custom'));
    const agents = [{ id: 'claude', name: 'Claude Code', available: false }, { id: 'grok', name: 'Grok', available: true }];
    assert.equal(prov.resolveChoice(agents), 'agent:grok'); // first AVAILABLE agent when nothing is saved
    assert.equal(prov.resolveChoice([]), 'anthropic');
    prov.store.setChoice('openrouter'); assert.equal(prov.resolveChoice(agents), 'openrouter');
    prov.store.setChoice('agent:gone'); assert.equal(prov.resolveChoice(agents), 'agent:grok');
    prov.store.setSession('claude', 'a.kerf.json', 'S1'); prov.store.setSession('claude', 'b.kerf.json', 'S2');
    prov.store.clearSessions('a.kerf.json');
    assert.equal(prov.store.session('claude', 'a.kerf.json'), ''); assert.equal(prov.store.session('claude', 'b.kerf.json'), 'S2');
  } finally { delete globalThis.localStorage; }
});

const ag = await load('/src/chat/agent.ts');
await t('agent: tool titles read like Kerf commands; summary status parsed from kerf output', () => {
  assert.equal(ag.toolTitle('Bash', { command: 'kerf apply truss.kerf.json -w --why "Add anchors" <<\'EOF\'\n[{"op":"add"}]\nEOF' }), 'kerf apply truss.kerf.json -w --why "Add anchors" <<');
  assert.equal(ag.toolTitle('Bash', { command: 'ls -la' }), 'BASH ls -la');
  assert.equal(ag.toolTitle('Read', { file_path: '/home/u/work/details/truss.kerf.json' }), 'READ details/truss.kerf.json');
  assert.equal(ag.resultStatus('DOC x  14 components  2 views  0 errors  1 warnings', true), '✓ 0 ERR 1 WARN');
  assert.equal(ag.resultStatus('whatever', false), '✗ ERROR');
});

const agentRun = async (fixture, agentIdStr, agentName, extra = []) => {
  const lines = fs.readFileSync(path.join(web, 'test/fixtures', fixture), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  const handlers = new Map();
  const ws = { file: 'a.kerf.json', uiRunActive: false, endUiRun() { this.uiRunActive = false; }, online: true, on: () => () => {}, onAgentRun: (id, fn) => { fn ? handlers.set(id, fn) : handlers.delete(id); },
    client: { async agentRun(agent, message, session, file) { ws.last = { agent, message, session, file }; setTimeout(() => { for (const l of lines) handlers.get('r1')?.(l); handlers.get('r1')?.({ type: 'exit', code: 0, ...extra[0] }); }, 5); return 'r1'; } } };
  const app = { pendingDesignerEdits: ['EDIT: x'], setClaude() {}, claude: { state: 'OK' } };
  const runner = new ag.AgentRunner(app, ws, () => [{ id: agentIdStr, name: agentName, available: true }]);
  const ev = []; runner.on((e) => ev.push(e));
  await runner.send('agent:' + agentIdStr, 'say done', 0);
  return { ev, ws, app, runner };
};
const withStorage = async (fn) => {
  const ls = new Map();
  globalThis.localStorage = { getItem: (k) => ls.get(k) ?? null, setItem: (k, v) => ls.set(k, String(v)), removeItem: (k) => ls.delete(k), get length() { return ls.size; }, key: (i) => [...ls.keys()][i] };
  globalThis.window = globalThis;
  try { await fn(); } finally { delete globalThis.localStorage; delete globalThis.window; }
};
const kinds = (ev) => ev.map((e) => e.type + (e.phase ? ':' + e.phase : ''));
await t('agent: claude stream-json (recorded from `claude -p --output-format stream-json --verbose`) -> console events; session id persisted and resumed', () => withStorage(async () => {
  const { ev, ws, app, runner } = await agentRun('agent-claude.jsonl', 'claude', 'Claude Code');
  assert.deepEqual(app.pendingDesignerEdits, []);
  assert.equal(ws.last.session, undefined);
  assert.equal(prov.store.session('claude', 'a.kerf.json'), '4b0b2e28-49db-4776-86c3-09c85e775c85');
  assert.deepEqual(kinds(ev), ['user', 'assistant-start', 'tool:start', 'tool:end', 'text', 'done']);
  assert.equal(ev[1].who, 'LOCAL AGENT · CLAUDE CODE');
  const end = ev.find((e) => e.phase === 'end');
  assert.equal(end.title, 'BASH echo hello-kerf'); assert.match(end.detail, /hello-kerf/); assert.equal(end.ok, true);
  assert.equal(ev.find((e) => e.type === 'text').delta.trim(), 'done');
  await runner.send('agent:claude', 'again', 0);
  assert.equal(ws.last.session, '4b0b2e28-49db-4776-86c3-09c85e775c85');
  assert.equal(runner.busy, false);
}));
await t('agent: Grok Build streaming-json (recorded): token deltas concatenate without newlines; tool_call/tool_call_update pair; sessionId', () => withStorage(async () => {
  const { ev } = await agentRun('agent-grok.jsonl', 'grok', 'Grok Build');
  const text = ev.filter((e) => e.type === 'text').map((e) => e.delta).join('');
  assert.equal(text, "I'll run that now.done");
  const tools = ev.filter((e) => e.type === 'tool');
  assert.deepEqual(tools.map((e) => e.phase), ['start', 'end']); // in_progress updates ignored
  assert.equal(tools[1].title, 'BASH echo hello-kerf'); assert.equal(tools[1].ok, true); assert.match(tools[1].detail, /hello-kerf/);
  assert.equal(prov.store.session('grok', 'a.kerf.json'), '01a10d31-6699-76b2-af23-809f3fe34e18');
}));
await t('agent: Codex exec --json (recorded): command_execution unwraps /bin/bash -lc; agent_message is text; thread id kept', () => withStorage(async () => {
  const { ev } = await agentRun('agent-codex.jsonl', 'codex', 'Codex CLI');
  assert.deepEqual(kinds(ev), ['user', 'assistant-start', 'tool:start', 'tool:end', 'text', 'done']);
  assert.equal(ev.find((e) => e.phase === 'end').title, 'BASH echo hello-kerf');
  assert.equal(prov.store.session('codex', 'a.kerf.json'), '01a10d31-9a8e-7a71-8ded-f4184b2892f1');
}));
await t('agent: a non-zero exit shows an actionable notice; unknown events are kept for debugging, not rendered', () => withStorage(async () => {
  const { ev } = await agentRun('agent-codex.jsonl', 'codex', 'Codex CLI', [{ code: 3 }]);
  assert.ok(ev.some((e) => e.type === 'notice' && /EXITED WITH CODE 3/.test(e.text)));
}));

await vite.close();
console.log(`${n - failed}/${n} passed`);
process.exit(failed ? 1 : 0);
