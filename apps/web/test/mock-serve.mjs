// A mock of `kerf serve` (spec/SERVE.md): same HTTP/SSE API over a folder of *.kerf.json, backed by the REAL engine wasm
// (engines/zig/dist/kerf.wasm), so the web UI's workspace mode can be tested without the Zig server.
//   node test/mock-serve.mjs [--dir DIR] [--port 7710] [--token T] [--ui apps/web/dist-serve]
// Test hooks (not part of the spec): GET /__state, POST /__reset, POST /__external {file, ops, why, who?} (an agent edit via the CLI:
// writes the file and appends a log line, exactly like `kerf apply -w`), POST /__write {file, doc} (raw write, no log).
// The "local agent" is scripted: it streams Claude-Code-shaped stream-json events and really edits the open document.
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');

// ---------------------------------------------------------------- engine (SPEC §13.1 raw ABI)
async function loadEngine() {
  const mod = await WebAssembly.compile(fs.readFileSync(path.join(repo, 'engines/zig/dist/kerf.wasm')));
  const imports = {};
  for (const i of WebAssembly.Module.imports(mod)) (imports[i.module] ??= {})[i.name] = i.kind === 'function' ? () => 0 : new WebAssembly.Memory({ initial: 32 });
  const x = (await WebAssembly.instantiate(mod, imports)).exports;
  x._initialize?.();
  const enc = new TextEncoder(), dec = new TextDecoder();
  const call = (fn, input) => {
    const fb = enc.encode(fn), ib = enc.encode(JSON.stringify(input ?? {}));
    const fp = x.kerf_alloc(fb.length), ip = x.kerf_alloc(Math.max(ib.length, 1));
    const mem = new Uint8Array(x.memory.buffer); mem.set(fb, fp); mem.set(ib, ip);
    const rc = x.kerf_call(fp, fb.length, ip, ib.length);
    const out = new Uint8Array(x.memory.buffer, x.kerf_out_ptr(), x.kerf_out_len()).slice();
    x.kerf_free(fp, fb.length); x.kerf_free(ip, Math.max(ib.length, 1));
    return { rc, out };
  };
  return {
    json(fn, input) { const { rc, out } = call(fn, input); const t = dec.decode(out); let j; try { j = JSON.parse(t); } catch { j = t; } if (rc !== 0) { const e = new Error(typeof j === 'string' ? j : j?.message ?? JSON.stringify(j)); e.payload = j; throw e; } return j; },
    bytes(fn, input) { const { rc, out } = call(fn, input); if (rc !== 0) throw new Error(dec.decode(out)); return Buffer.from(out); },
  };
}

const sampleDir = path.join(repo, 'spec/details');
const style = JSON.parse(fs.readFileSync(path.join(repo, 'spec/styles/kerf-standard.kerfstyle.json'), 'utf8'));
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.wasm': 'application/wasm', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png', '.txt': 'text/plain' };

export function seedDir(dir, files = ['truss-bearing-cmu', 'flush-beam-strap']) {
  fs.mkdirSync(dir, { recursive: true });
  for (const f of files) fs.copyFileSync(path.join(sampleDir, `${f}.kerf.json`), path.join(dir, `${f}.kerf.json`));
  return dir;
}

export async function startMock({ dir = seedDir(fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-ws-'))), port = 0, token = '', ui = path.join(here, '../dist-serve'), pollMs = 120, allowHttpProviders = ['custom'] } = {}) {
  const engine = await loadEngine();
  const state = { llm: [], agentRuns: [], stops: [], applies: [], sseClients: 0 };
  const clients = new Set();
  const sessions = { n: 0 };
  const runs = new Map();

  const fileOk = (f) => /^[A-Za-z0-9._-]+\.kerf\.json$/.test(f);
  const p = (f) => path.join(dir, f);
  const etagOf = (st) => `"${Math.floor(st.mtimeMs)}-${st.size}"`;
  const stripQ = (s) => String(s ?? '').replace(/^W\//, '').replace(/^"|"$/g, '');
  const readDoc = (f) => JSON.parse(fs.readFileSync(p(f), 'utf8'));
  const writeDoc = (f, doc) => fs.writeFileSync(p(f), JSON.stringify(doc, null, 2) + '\n');
  const logPath = (f) => p(f.replace(/\.kerf\.json$/, '.kerf.json') + '.log.jsonl');
  const appendLog = (f, e) => fs.appendFileSync(logPath(f), JSON.stringify({ ts: new Date().toISOString().replace(/\.\d+Z$/, 'Z'), tool: 'kerf-cli', ...e }) + '\n');
  const headOf = (r) => (r.summary ?? '').split('\n')[0];

  const checkCache = new Map();
  const info = (f) => {
    const st = fs.statSync(p(f));
    const key = `${f}:${st.mtimeMs}:${st.size}`;
    let c = checkCache.get(key);
    if (!c) {
      const doc = readDoc(f);
      let r = { diagnostics: [] };
      try { r = engine.json('check', { doc, style }); } catch { /* keep empty */ }
      c = { file: f, id: doc.id, title: doc.title ?? '', mtime_ms: Math.floor(st.mtimeMs), size: st.size, components: doc.components?.length ?? 0, views: doc.views?.length ?? 0,
        errors: r.diagnostics.filter((d) => d.level === 'error').length, warnings: r.diagnostics.filter((d) => d.level === 'warning').length };
      checkCache.set(key, c);
    }
    return c;
  };
  const listDocs = () => fs.readdirSync(dir).filter(fileOk).sort().map(info);

  // ---- SSE + folder polling (mtime + size) + log tailing
  const send = (type, data) => { const m = `event: ${type}\ndata: ${JSON.stringify(data)}\n\n`; for (const c of clients) c.write(m); };
  const seen = new Map(); // file -> "mtime-size"
  const logPos = new Map();
  const scan = () => {
    const now = new Set();
    for (const f of fs.readdirSync(dir).filter(fileOk)) {
      now.add(f);
      const st = fs.statSync(p(f));
      const sig = `${Math.floor(st.mtimeMs)}-${st.size}`;
      if (!seen.has(f)) { if (seen.size || started) send('doc_added', { file: f }); }
      else if (seen.get(f) !== sig) send('doc_changed', { file: f, mtime_ms: Math.floor(st.mtimeMs) });
      seen.set(f, sig);
      const lp = logPath(f);
      const size = fs.existsSync(lp) ? fs.statSync(lp).size : 0;
      const pos = logPos.get(f) ?? (started ? 0 : size);
      if (size > pos) {
        const lines = fs.readFileSync(lp, 'utf8').slice(pos).split('\n').filter(Boolean);
        for (const l of lines) { try { send('log', { file: f, entry: JSON.parse(l) }); } catch { /* partial */ } }
        logPos.set(f, size);
      } else logPos.set(f, pos);
    }
    for (const f of [...seen.keys()]) if (!now.has(f)) { seen.delete(f); send('doc_removed', { file: f }); }
  };
  let started = false;
  scan(); started = true;
  const poll = setInterval(scan, pollMs);
  const ping = setInterval(() => send('ping', {}), 15000);

  // ---- helpers
  const json = (res, status, body, headers = {}) => { res.writeHead(status, { 'content-type': 'application/json', ...headers }); res.end(JSON.stringify(body)); };
  const err = (res, status, code, message, headers) => json(res, status, { error: { code, message } }, headers);
  const body = (req) => new Promise((resolve) => { let b = ''; req.on('data', (c) => (b += c)); req.on('end', () => { try { resolve(b ? JSON.parse(b) : {}); } catch { resolve(null); } }); });

  const applyOps = (f, ops, why, actor, who) => {
    const doc = readDoc(f);
    const r = engine.json('apply', { doc, style, ops, actor: actor === 'llm' ? 'llm' : 'designer' });
    if (r.ok && r.doc) {
      writeDoc(f, r.doc);
      appendLog(f, { who, tool: who === 'agent' ? 'kerf-cli' : 'kerf-serve', why, ops, changed: r.changed ?? [], summary_head: headOf(r) });
    }
    return r;
  };

  // ---- the scripted local agent
  const AGENTS = [
    { id: 'claude', name: 'Claude Code', available: true, version: '2.1.289' },
    { id: 'grok', name: 'Grok', available: false, version: undefined, reason: 'grok not found on PATH' },
    { id: 'codex', name: 'Codex', available: true, version: '0.9.0' },
  ];
  function runAgent(runId, o) {
    const run = { id: runId, timers: [], stopped: false };
    runs.set(runId, run);
    const emit = (event) => send('agent', { run_id: runId, event });
    const sid = o.session_id || `sess-${++sessions.n}`;
    const file = o.file && fileOk(o.file) && fs.existsSync(p(o.file)) ? o.file : null;
    const doc = file ? readDoc(file) : null;
    const note = doc?.views?.[0]?.annotations?.find((a) => a.type === 'note');
    const steps = [];
    const asst = (content) => ({ type: 'assistant', message: { role: 'assistant', content }, session_id: sid });
    steps.push(() => emit({ type: 'system', subtype: 'init', session_id: sid, cwd: dir, tools: ['Bash', 'Read', 'Write', 'Edit'] }));
    steps.push(() => emit(asst([{ type: 'text', text: 'Running kerf guide, then editing the detail.' }])));
    steps.push(() => emit(asst([{ type: 'tool_use', id: 'toolu_g', name: 'Bash', input: { command: 'kerf guide | head -20', description: 'Read the Kerf guide' } }])));
    steps.push(() => emit({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_g', content: 'KERF GUIDE ...', is_error: false }] }, session_id: sid }));
    if (/fail/i.test(o.message)) {
      steps.push(() => emit(asst([{ type: 'tool_use', id: 'toolu_f', name: 'Bash', input: { command: 'kerf apply nope.kerf.json --ops \'[]\' -w' } }])));
      steps.push(() => emit({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_f', content: 'error: no such file nope.kerf.json', is_error: true }] }, session_id: sid }));
    } else if (file && note) {
      const text = /note/i.test(o.message) ? `${note.text ?? ''} (AGENT)`.trim() : note.text;
      const ops = [{ op: 'update', path: `views/${doc.views[0].id}/annotations/${note.id}`, value: { text } }];
      steps.push(() => emit(asst([{ type: 'tool_use', id: 'toolu_a', name: 'Bash', input: { command: `kerf apply ${file} -w --why "Agent edit" <<'EOF'\n${JSON.stringify(ops)}\nEOF`, description: 'Apply ops' } }])));
      steps.push(() => {
        const r = applyOps(file, ops, 'Agent edit: ' + o.message.slice(0, 60), 'designer', 'agent');
        emit({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_a', content: headOf(r) + '\nok', is_error: !r.ok }] }, session_id: sid });
      });
      steps.push(() => emit(asst([{ type: 'tool_use', id: 'toolu_r', name: 'Read', input: { file_path: p(file) } }])));
      steps.push(() => emit({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_r', content: [{ type: 'text', text: '{ ...document... }' }], is_error: false }] }, session_id: sid }));
    }
    steps.push(() => emit(asst([{ type: 'text', text: 'Done. The note was updated and the document re-checks clean.' }])));
    steps.push(() => emit({ type: 'result', subtype: 'success', is_error: false, result: 'Done.', session_id: sid }));
    const slow = /slow/i.test(o.message);
    steps.push(() => { emit({ type: 'exit', code: 0, session_id: sid }); runs.delete(runId); });
    steps.forEach((s, i) => run.timers.push(setTimeout(s, 30 + i * (slow ? 600 : 40))));
    run.stop = () => { run.timers.forEach(clearTimeout); emit({ type: 'exit', code: 143, session_id: sid }); runs.delete(runId); };
  }

  // ---- HTTP
  const server = http.createServer(async (req, res) => {
    const u = new URL(req.url, 'http://x');
    const pathname = u.pathname;
    const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': 'GET, POST, OPTIONS', 'access-control-expose-headers': 'ETag, Content-Disposition' };
    for (const [k, v] of Object.entries(cors)) res.setHeader(k, v);
    if (req.method === 'OPTIONS') { res.writeHead(204); res.end(); return; }
    try {
      if (pathname.startsWith('/__')) return await testHooks(req, res, pathname);
      if (pathname.startsWith('/api/')) {
        const supplied = (req.headers.authorization ?? '').replace(/^Bearer\s+/i, '') || u.searchParams.get('token') || '';
        if (token && supplied !== token) return err(res, 401, 'unauthorized', 'missing or wrong token');
        return await api(req, res, u);
      }
      // static UI
      let file = path.join(ui, decodeURIComponent(pathname));
      if (!file.startsWith(ui)) { res.writeHead(403); res.end(); return; }
      if (fs.existsSync(file) && fs.statSync(file).isDirectory()) file = path.join(file, 'index.html');
      if (!fs.existsSync(file)) { res.writeHead(404); res.end('not found'); return; }
      res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
      fs.createReadStream(file).pipe(res);
    } catch (e) { if (!res.headersSent) err(res, 500, 'internal', String(e?.stack ?? e)); else res.end(); }
  });

  async function api(req, res, u) {
    const m = req.method, pn = u.pathname;
    if (m === 'GET' && pn === '/api/info') return json(res, 200, { version: '0.1.0-mock', dir, token_required: !!token, agents: AGENTS, proxy: true });
    if (m === 'GET' && pn === '/api/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
      res.write(': hello\n\n');
      clients.add(res); state.sseClients++;
      req.on('close', () => clients.delete(res));
      return;
    }
    if (m === 'GET' && pn === '/api/docs') return json(res, 200, listDocs());
    if (m === 'POST' && pn === '/api/docs') {
      const b = await body(req);
      if (!b || !fileOk(b.file ?? '')) return err(res, 400, 'bad_request', 'file must match [A-Za-z0-9._-]+.kerf.json');
      if (fs.existsSync(p(b.file))) return err(res, 409, 'exists', `${b.file} already exists`);
      const id = b.id ?? b.file.replace(/\.kerf\.json$/, '');
      const doc = engine.json('fmt', { doc: { kerf: '0.1', id, title: b.title ?? '', meta: {}, components: [], views: [] } }).doc;
      writeDoc(b.file, doc);
      appendLog(b.file, { who: 'designer', tool: 'kerf-serve', why: 'create', ops: [], changed: [], summary_head: `DOC ${id} 0 components 0 views 0 errors 0 warnings` });
      return json(res, 201, info(b.file));
    }
    let mm = /^\/api\/docs\/([^/]+)(?:\/(apply|log|export))?$/.exec(pn);
    if (mm) {
      const f = decodeURIComponent(mm[1]), sub = mm[2];
      if (!fileOk(f) || !fs.existsSync(p(f))) return err(res, 404, 'not_found', `no such document: ${f}`);
      const st = fs.statSync(p(f));
      if (m === 'GET' && !sub) { res.writeHead(200, { 'content-type': 'application/json', etag: etagOf(st) }); res.end(fs.readFileSync(p(f))); return; }
      if (m === 'POST' && sub === 'apply') {
        const b = await body(req);
        if (!b || !Array.isArray(b.ops)) return err(res, 400, 'bad_request', 'body needs ops[]');
        state.applies.push({ file: f, actor: b.actor, why: b.why, if_match: b.if_match, ops: b.ops.length });
        if (b.if_match && stripQ(b.if_match) !== stripQ(etagOf(st))) return err(res, 409, 'conflict', 'the document changed on disk', { etag: etagOf(st) });
        let r;
        try { r = applyOps(f, b.ops, b.why ?? '', b.actor ?? 'designer', b.actor === 'llm' ? 'llm' : 'designer'); }
        catch (e) { return json(res, 422, { ok: false, diagnostics: e.payload?.diagnostics ?? [], summary: '', error: e.message }); }
        const nst = fs.statSync(p(f));
        return json(res, r.ok ? 200 : 422, r, { etag: etagOf(nst) });
      }
      if (m === 'GET' && sub === 'log') {
        const since = Number(u.searchParams.get('since') ?? 0);
        const lines = fs.existsSync(logPath(f)) ? fs.readFileSync(logPath(f), 'utf8').split('\n').filter(Boolean) : [];
        return json(res, 200, { entries: lines.slice(since).map((l) => JSON.parse(l)), next: lines.length });
      }
      if (m === 'GET' && sub === 'export') {
        const view = u.searchParams.get('view') ?? readDoc(f).views?.[0]?.id, format = u.searchParams.get('format') ?? 'svg', sheet = u.searchParams.get('sheet') === '1';
        let out;
        try { out = engine.bytes('export', { doc: readDoc(f), style, view, format, sheet }); } catch (e) { return err(res, 400, 'export_failed', e.message); }
        res.writeHead(200, { 'content-type': { svg: 'image/svg+xml', dxf: 'application/dxf', pdf: 'application/pdf', png: 'image/png' }[format] ?? 'application/octet-stream', 'content-disposition': `attachment; filename="${f.replace(/\.kerf\.json$/, '')}-${view}.${format}"` });
        res.end(out); return;
      }
    }
    if (m === 'POST' && pn === '/api/llm') {
      const b = await body(req);
      if (!b || !b.path || !b.provider) return err(res, 400, 'bad_request', 'need provider, path, headers, body');
      let url;
      try { url = new URL(b.path.replace(/^\/?/, '/'), (b.base_url ?? '').replace(/\/+$/, '') + '/'); url = new URL((b.base_url ?? '').replace(/\/+$/, '') + b.path); } catch { return err(res, 400, 'bad_url', 'bad base_url/path'); }
      const local = /^(localhost|127\.|10\.|192\.168\.)/.test(url.hostname);
      if (url.protocol !== 'https:' && !(allowHttpProviders.includes(b.provider) && local && url.protocol === 'http:')) return err(res, 400, 'bad_url', 'only https:// URLs (http://localhost/LAN for custom)');
      state.llm.push({ provider: b.provider, url: url.href, headers: { ...b.headers, authorization: b.headers?.authorization ? '<redacted>' : undefined, 'x-api-key': b.headers?.['x-api-key'] ? '<redacted>' : undefined }, key: b.headers?.authorization ?? b.headers?.['x-api-key'], body: b.body });
      const up = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', ...b.headers }, body: JSON.stringify(b.body) });
      const h = { 'content-type': up.headers.get('content-type') ?? 'application/json' };
      res.writeHead(up.status, h);
      if (!up.body) { res.end(); return; }
      const reader = up.body.getReader();
      for (;;) { const { done, value } = await reader.read(); if (done) break; res.write(value); }
      res.end(); return;
    }
    if (m === 'POST' && pn === '/api/agent/run') {
      const b = await body(req);
      const a = AGENTS.find((x) => x.id === b?.agent);
      if (!a) return err(res, 400, 'unknown_agent', `unknown agent ${b?.agent}`);
      if (!a.available) return err(res, 400, 'unavailable', a.reason);
      if (runs.size) return err(res, 409, 'busy', 'one agent run at a time');
      const runId = `run-${state.agentRuns.length + 1}`;
      state.agentRuns.push({ run_id: runId, agent: b.agent, message: b.message, session_id: b.session_id, file: b.file });
      setTimeout(() => runAgent(runId, b), 5);
      return json(res, 200, { run_id: runId });
    }
    if (m === 'POST' && pn === '/api/agent/stop') {
      const b = await body(req);
      state.stops.push(b?.run_id);
      runs.get(b?.run_id)?.stop();
      return json(res, 200, { ok: true });
    }
    return err(res, 404, 'not_found', `${m} ${pn}`);
  }

  async function testHooks(req, res, pn) {
    if (pn === '/__state') return json(res, 200, { ...state, dir, files: listDocs().map((d) => d.file) });
    const b = await body(req);
    if (pn === '/__reset') { state.llm.length = 0; state.agentRuns.length = 0; state.stops.length = 0; state.applies.length = 0; return json(res, 200, { ok: true }); }
    if (pn === '/__external') { // like `kerf apply <file> --ops ... -w --why ...` run by an agent in a terminal
      const r = applyOps(b.file, b.ops, b.why ?? '', 'designer', b.who ?? 'agent');
      return json(res, 200, { ok: r.ok, summary: headOf(r) });
    }
    if (pn === '/__write') { writeDoc(b.file, b.doc); return json(res, 200, { ok: true }); }
    if (pn === '/__rm') { fs.rmSync(p(b.file), { force: true }); return json(res, 200, { ok: true }); }
    return err(res, 404, 'not_found', pn);
  }

  await new Promise((r) => server.listen(port, '127.0.0.1', r));
  const addr = server.address();
  return {
    port: addr.port, url: `http://127.0.0.1:${addr.port}`, dir, state,
    close: () => { clearInterval(poll); clearInterval(ping); for (const c of clients) c.end(); for (const r of runs.values()) r.timers.forEach(clearTimeout); server.closeAllConnections?.(); server.close(); },
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const a = process.argv.slice(2);
  const opt = (n, d) => { const i = a.indexOf(n); return i >= 0 ? a[i + 1] : d; };
  const dir = opt('--dir');
  const m = await startMock({ dir: dir ? seedDirIfEmpty(dir) : undefined, port: Number(opt('--port', 7710)), token: opt('--token', ''), ui: path.resolve(opt('--ui', path.join(here, '../dist-serve'))) });
  console.log(`mock kerf serve on ${m.url}/  dir=${m.dir}${opt('--token') ? `  token=${opt('--token')}` : ''}`);
}
function seedDirIfEmpty(dir) { fs.mkdirSync(dir, { recursive: true }); if (!fs.readdirSync(dir).some((f) => f.endsWith('.kerf.json'))) seedDir(dir); return dir; }
