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

await vite.close();
console.log(`${n - failed}/${n} passed`);
process.exit(failed ? 1 : 0);
