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


const prov = await load('/src/chat/store.ts');
await t('store: resolveChoice prefers the saved available agent, else the first available; sessions are per agent and document', () => {
  const ls = new Map();
  globalThis.localStorage = { getItem: (k) => ls.get(k) ?? null, setItem: (k, v) => ls.set(k, String(v)), removeItem: (k) => ls.delete(k), get length() { return ls.size; }, key: (i) => [...ls.keys()][i] };
  try {
    const agents = [{ id: 'claude', name: 'Claude Code', available: false }, { id: 'grok', name: 'Grok', available: true }];
    assert.equal(prov.resolveChoice(agents), 'grok'); // first AVAILABLE agent when nothing is saved
    assert.equal(prov.resolveChoice([]), null);
    prov.store.setChoice('claude'); assert.equal(prov.resolveChoice(agents), 'claude');
    prov.store.setChoice('gone'); assert.equal(prov.resolveChoice(agents), 'grok');
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
  const app = { setClaude() {}, claude: { state: 'OK' } };
  const runner = new ag.AgentRunner(app, ws, () => [{ id: agentIdStr, name: agentName, available: true }]);
  const ev = []; runner.on((e) => ev.push(e));
  await runner.send(agentIdStr, 'say done');
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
  assert.equal(ws.last.session, undefined);
  assert.equal(prov.store.session('claude', 'a.kerf.json'), '4b0b2e28-49db-4776-86c3-09c85e775c85');
  assert.deepEqual(kinds(ev), ['user', 'assistant-start', 'tool:start', 'tool:end', 'text', 'done']);
  assert.equal(ev[1].who, 'LOCAL AGENT · CLAUDE CODE');
  const end = ev.find((e) => e.phase === 'end');
  assert.equal(end.title, 'BASH echo hello-kerf'); assert.match(end.detail, /hello-kerf/); assert.equal(end.ok, true);
  assert.equal(ev.find((e) => e.type === 'text').delta.trim(), 'done');
  await runner.send('claude', 'again');
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
