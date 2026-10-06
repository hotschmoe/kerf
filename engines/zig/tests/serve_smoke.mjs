#!/usr/bin/env node
// Integration smoke test for `kerf serve` (spec/SERVE.md). Starts the real binary on temp folders and
// exercises every endpoint: CLI edits -> SSE, designer apply + ETag conflicts, op log, exports, the
// agent bridge (fake agent), token auth on 0.0.0.0, Host/Origin checks, and the LLM proxy against a
// local node server (no real LLM API is ever called).
//
//   node tests/serve_smoke.mjs [path/to/kerf]      (default: zig-out/bin/kerf; run `zig build` first)
import { spawn, execFileSync } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const KERF = path.resolve(process.argv[2] ?? path.join(here, '..', 'zig-out', 'bin', 'kerf'));
const REF = path.join(here, '..', '..', '..', 'spec', 'details', 'flush-beam-strap.kerf.json');
const FAKE = path.join(here, 'fake-agent.mjs');
if (!fs.existsSync(KERF)) { console.error('kerf binary not found: ' + KERF + ' (zig build first)'); process.exit(2); }

let passed = 0, failed = 0;
function check(name, cond, extra) {
  if (cond) { passed++; console.log('  ok   ' + name); } else { failed++; console.log('  FAIL ' + name + (extra !== undefined ? '  -> ' + (typeof extra === 'string' ? extra : JSON.stringify(extra)) : '')); }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const section = (t) => console.log('\n# ' + t);

const children = [];
function cleanup() { for (const c of children) { try { c.kill('SIGKILL'); } catch {} } }
process.on('exit', cleanup);
process.on('SIGINT', () => { cleanup(); process.exit(130); });

// The server gets a PATH WITHOUT kerf's directory, to prove the agent bridge adds it.
const bareEnv = { ...process.env, PATH: path.dirname(process.execPath) + ':/usr/bin:/bin', KERF_ACTOR: '' };
delete bareEnv.KERF_ACTOR;

function startServer(dir, extraArgs = []) {
  return new Promise((resolve, reject) => {
    const p = spawn(KERF, ['serve', '--dir', dir, '--port', '0', ...extraArgs], { env: bareEnv, stdio: ['ignore', 'pipe', 'pipe'] });
    children.push(p);
    let out = '', err = '';
    const t = setTimeout(() => reject(new Error('server did not start: ' + out + err)), 8000);
    p.stdout.on('data', (d) => {
      out += d;
      const m = /http:\/\/[\d.]+:(\d+)\//.exec(out);
      if (m && out.includes('\n')) { clearTimeout(t); setTimeout(() => resolve({ proc: p, port: +m[1], banner: out }), 80); }
    });
    p.stderr.on('data', (d) => { err += d; });
    p.on('exit', (c) => { clearTimeout(t); reject(new Error('server exited early ' + c + ' ' + out + err)); });
  });
}

function rawReq(port, method, p, { headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? null : (typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body));
    const h = { ...headers };
    if (data !== null && !h['content-type'] && !h['Content-Type']) h['content-type'] = 'application/json';
    if (data !== null && !h['transfer-encoding'] && !h['content-length']) h['content-length'] = Buffer.byteLength(data);
    const r = http.request({ host: '127.0.0.1', port, method, path: p, headers: h }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    });
    r.on('error', reject);
    if (data !== null) r.write(data);
    r.end();
  });
}
async function api(port, method, p, opts) {
  const r = await rawReq(port, method, p, opts);
  let json = null;
  try { json = JSON.parse(r.body.toString('utf8')); } catch {}
  return { ...r, json, text: r.body.toString('utf8') };
}

// SSE client: collects frames; waitFor(pred, ms) resolves with the first matching frame.
function sse(port, qs = '', headers = {}) {
  const frames = [];
  const waiters = [];
  let buf = '';
  const req = http.get({ host: '127.0.0.1', port, path: '/api/events' + qs, headers }, (res) => {
    res.setEncoding('utf8');
    res.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const raw = buf.slice(0, i); buf = buf.slice(i + 2);
        let ev = 'message', data = '';
        for (const line of raw.split('\n')) {
          if (line.startsWith('event: ')) ev = line.slice(7);
          else if (line.startsWith('data: ')) data += line.slice(6);
        }
        if (!data) continue;
        let j = null; try { j = JSON.parse(data); } catch {}
        const f = { event: ev, data: j };
        frames.push(f);
        for (const w of [...waiters]) if (w.pred(f)) { waiters.splice(waiters.indexOf(w), 1); w.resolve(f); }
      }
    });
  });
  req.on('error', () => {});
  const waitFor = (pred, ms = 5000) => {
    const hit = frames.find(pred);
    if (hit) return Promise.resolve(hit);
    return new Promise((resolve) => {
      const w = { pred, resolve };
      waiters.push(w);
      setTimeout(() => { const i = waiters.indexOf(w); if (i >= 0) { waiters.splice(i, 1); resolve(null); } }, ms);
    });
  };
  return { frames, waitFor, close: () => req.destroy(), req };
}

const cli = (dir, args, env = {}) => {
  try { return { code: 0, out: execFileSync(KERF, args, { cwd: dir, encoding: 'utf8', env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] }) }; }
  catch (e) { return { code: e.status ?? 1, out: (e.stdout ?? '') + (e.stderr ?? '') }; }
};
const isRun_exit = (rid) => (f) => f.event === 'agent' && f.data.run_id === rid && f.data.event.type === 'exit';
const addOp = (id) => [{ op: 'add', path: 'components', value: { id, type: 'lumber', size: '2x4', at: { x: 0, y: 0 } } }];

async function main() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-smoke-'));
  fs.mkdirSync(path.join(dir, '.kerf'));
  fs.writeFileSync(path.join(dir, '.kerf', 'agents.json'), JSON.stringify({
    agents: [{ id: 'fake', name: 'Fake Agent', detect: [process.execPath, '--version'], argv: [process.execPath, FAKE, '{message}', '{resume}'], resume: ['--resume', '{session_id}'] },
             { id: 'ghost', name: 'Ghost', detect: ['definitely-not-installed-kerf-test', '--version'], argv: ['definitely-not-installed-kerf-test'] }],
  }));

  section('CLI: op log');
  let r = cli(dir, ['new', 'a.kerf.json', '--title', 'SMOKE A']);
  check('kerf new', r.code === 0, r.out);
  r = cli(dir, ['apply', 'a.kerf.json', '--ops', JSON.stringify(addOp('p0')), '-w', '--why', 'cli first'], { KERF_ACTOR: 'tester' });
  check('kerf apply -w', r.code === 0, r.out);
  let lines = fs.readFileSync(path.join(dir, 'a.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('new + apply appended 2 entries', lines.length === 2, lines.length);
  check('create entry', lines[0].why === 'create' && lines[0].tool === 'kerf-cli' && lines[0].who === 'agent', lines[0]);
  check('apply entry: who=$KERF_ACTOR, why, ops, changed, summary_head', lines[1].who === 'tester' && lines[1].why === 'cli first' && lines[1].ops.length === 1 && lines[1].changed[0] === 'p0' && /^DOC a/.test(lines[1].summary_head) && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/.test(lines[1].ts), lines[1]);
  r = cli(dir, ['apply', 'a.kerf.json', '--ops', '[{"op":"remove","path":"components/nope"}]', '-w']);
  check('failed apply writes nothing and logs nothing', r.code !== 0 && fs.readFileSync(path.join(dir, 'a.kerf.json.log.jsonl'), 'utf8').trim().split('\n').length === 2);
  fs.rmSync(path.join(dir, 'a.kerf.json.log.jsonl'));
  fs.rmSync(path.join(dir, 'a.kerf.json'));

  section('server: start, info, empty library');
  const { port, banner } = await startServer(dir);
  check('banner prints the URL and the dir', banner.includes('http://127.0.0.1:' + port + '/') && banner.includes(dir.replace(/\/$/, '')), banner);
  let t0 = performance.now();
  let info = await api(port, 'GET', '/api/info');
  const info_ms = performance.now() - t0;
  check('GET /api/info', info.status === 200 && info.json.version && info.json.token_required === false && info.json.proxy === true && Array.isArray(info.json.agents), info.text);
  check('info.dir is the served folder', fs.realpathSync(info.json.dir) === fs.realpathSync(dir), info.json.dir);
  const fake = info.json.agents.find((a) => a.id === 'fake'), ghost = info.json.agents.find((a) => a.id === 'ghost');
  check('agents: override from .kerf/agents.json is available with a version', fake && fake.available === true && /^v\d+/.test(fake.version ?? ''), fake);
  check('agents: missing CLI -> available:false + reason', ghost && ghost.available === false && /not found/.test(ghost.reason ?? ''), ghost);
  check('agents: built-ins claude/grok/codex listed', ['claude', 'grok', 'codex'].every((id) => info.json.agents.some((a) => a.id === id)), info.json.agents.map((a) => a.id));
  let docs = await api(port, 'GET', '/api/docs');
  check('GET /api/docs empty', docs.status === 200 && Array.isArray(docs.json) && docs.json.length === 0, docs.text);
  let root = await api(port, 'GET', '/');
  check('GET / serves something (UI or the built-in page)', root.status === 200 && /html/.test(root.headers['content-type']), root.status);
  let nf = await api(port, 'GET', '/api/nope');
  check('unknown /api path -> 404 JSON error', nf.status === 404 && nf.json?.error?.code === 'E_NOT_FOUND', nf.text);
  check('no-store on API responses', info.headers['cache-control'] === 'no-store');

  section('SSE: CLI edits arrive as doc_added / doc_changed / log');
  const ev = sse(port);
  await ev.waitFor((f) => f.event === 'ping', 2000);
  check('SSE opens with a ping', ev.frames.some((f) => f.event === 'ping'));
  cli(dir, ['new', 'a.kerf.json', '--title', 'SMOKE A']);
  const added = await ev.waitFor((f) => f.event === 'doc_added' && f.data.file === 'a.kerf.json', 3000);
  check('doc_added within 3 s', !!added, ev.frames);
  const logCreate = await ev.waitFor((f) => f.event === 'log' && f.data.file === 'a.kerf.json' && f.data.entry.why === 'create', 3000);
  check('log event for the create entry', !!logCreate);
  cli(dir, ['apply', 'a.kerf.json', '--ops', JSON.stringify(addOp('p1')), '-w', '--why', 'Add p1 via CLI']);
  const chg = await ev.waitFor((f) => f.event === 'doc_changed' && f.data.file === 'a.kerf.json', 3000);
  check('doc_changed (mtime_ms present)', !!chg && typeof chg.data.mtime_ms === 'number', ev.frames.slice(-3));
  const logApply = await ev.waitFor((f) => f.event === 'log' && f.data.entry?.why === 'Add p1 via CLI', 3000);
  check('log event carries who/why/ops/changed', !!logApply && logApply.data.entry.who === 'agent' && logApply.data.entry.changed[0] === 'p1', logApply);

  section('docs: list, get, ETag');
  docs = await api(port, 'GET', '/api/docs');
  check('list has the doc with counts', docs.json.length === 1 && docs.json[0].file === 'a.kerf.json' && docs.json[0].id === 'a' && docs.json[0].title === 'SMOKE A' && docs.json[0].components === 1 && docs.json[0].errors === 0 && typeof docs.json[0].mtime_ms === 'number' && docs.json[0].size > 100, docs.text);
  let g = await api(port, 'GET', '/api/docs/a.kerf.json');
  const etag1 = g.headers['etag'];
  check('GET doc returns JSON + ETag "<mtime_ms>-<size>"', g.status === 200 && g.json.id === 'a' && /^"\d+-\d+"$/.test(etag1), etag1);
  check('ETag matches the list entry', etag1 === `"${docs.json[0].mtime_ms}-${docs.json[0].size}"`, [etag1, docs.json[0]]);
  let g304 = await rawReq(port, 'GET', '/api/docs/a.kerf.json', { headers: { 'if-none-match': etag1 } });
  check('If-None-Match -> 304', g304.status === 304);
  let g404 = await api(port, 'GET', '/api/docs/zzz.kerf.json');
  check('missing doc -> 404', g404.status === 404 && g404.json.error.code === 'E_NOT_FOUND');
  for (const bad of ['..%2Fa.kerf.json', 'a.json', '.hidden.kerf.json', 'x%5Cy.kerf.json']) {
    const b = await api(port, 'GET', '/api/docs/' + bad);
    check('bad file name rejected: ' + bad, b.status === 400 || b.status === 404, b.status);
  }

  section('create via POST /api/docs');
  let c = await api(port, 'POST', '/api/docs', { body: { file: 'b', title: 'BEE' } });
  check('POST /api/docs creates b.kerf.json (201)', c.status === 201 && c.json.file === 'b.kerf.json' && fs.existsSync(path.join(dir, 'b.kerf.json')), c.text);
  const bdoc = JSON.parse(fs.readFileSync(path.join(dir, 'b.kerf.json'), 'utf8'));
  check('new doc has id/title', bdoc.id === 'b' && bdoc.title === 'BEE');
  const chunkedC = await api(port, 'POST', '/api/docs', { headers: { 'transfer-encoding': 'chunked' }, body: { file: 'chunked.kerf.json', title: 'CH' } });
  check('chunked request bodies are accepted', chunkedC.status === 201 && fs.existsSync(path.join(dir, 'chunked.kerf.json')), chunkedC.text);
  fs.rmSync(path.join(dir, 'chunked.kerf.json')); fs.rmSync(path.join(dir, 'chunked.kerf.json.log.jsonl'));
  c = await api(port, 'POST', '/api/docs', { body: { file: 'b.kerf.json' } });
  check('creating again -> 409', c.status === 409 && c.json.error.code === 'E_EXISTS', c.text);
  c = await api(port, 'POST', '/api/docs', { body: { file: '../evil.kerf.json' } });
  check('create outside folder -> 400', c.status === 400, c.text);
  const bAdded = await ev.waitFor((f) => f.event === 'doc_added' && f.data.file === 'b.kerf.json', 2000);
  check('doc_added for server-created doc carries who:designer, only once', bAdded && bAdded.data.who === 'designer' && ev.frames.filter((f) => f.event === 'doc_added' && f.data.file === 'b.kerf.json').length === 1, bAdded);

  section('designer apply: if_match, conflicts, log');
  const ap = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: addOp('d1'), why: 'designer adds d1', actor: 'designer', if_match: etag1.replace(/"/g, '') } });
  check('apply with matching if_match -> 200 ok:true', ap.status === 200 && ap.json.ok === true && ap.json.changed.includes('d1') && typeof ap.json.summary === 'string' && ap.json.doc.components.length === 2, ap.text.slice(0, 300));
  const etag2 = ap.headers['etag'];
  check('new ETag differs and is in the body', etag2 && etag2 !== etag1 && ap.json.etag === etag2, [etag1, etag2, ap.json.etag]);
  const onDisk = JSON.parse(fs.readFileSync(path.join(dir, 'a.kerf.json'), 'utf8'));
  check('document on disk was written', onDisk.components.some((x) => x.id === 'd1'));
  const dchg = await ev.waitFor((f) => f.event === 'doc_changed' && f.data.who === 'designer' && f.data.file === 'a.kerf.json', 3000);
  check('doc_changed with who:designer', !!dchg);
  const dlog = await ev.waitFor((f) => f.event === 'log' && f.data.entry.who === 'designer' && f.data.entry.why === 'designer adds d1', 3000);
  check('designer log event (tool kerf-serve)', !!dlog && dlog.data.entry.tool === 'kerf-serve');
  await sleep(1300); // the poller must not report our own write a second time
  check('no duplicate doc_changed/log from the poller for the designer write', ev.frames.filter((f) => f.event === 'doc_changed' && f.data.file === 'a.kerf.json' && f.data.who === 'designer').length === 1 && ev.frames.filter((f) => f.event === 'log' && f.data.entry?.why === 'designer adds d1').length === 1, ev.frames.slice(-6));
  const stale = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: addOp('d2'), why: 'stale', actor: 'designer', if_match: etag1 } });
  check('stale if_match -> 409 with the current ETag', stale.status === 409 && stale.json.error.code === 'E_CONFLICT' && stale.json.etag === etag2 && stale.headers['etag'] === etag2, stale.text);
  check('conflict wrote nothing', !JSON.parse(fs.readFileSync(path.join(dir, 'a.kerf.json'), 'utf8')).components.some((x) => x.id === 'd2'));
  const bad = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: [{ op: 'remove', path: 'components/ghost' }], why: 'bad', actor: 'designer' } });
  check('invalid ops -> 200 with ok:false and diagnostics, nothing written', bad.status === 200 && bad.json.ok === false && bad.json.diagnostics.length > 0, bad.text.slice(0, 300));
  const noOps = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { why: 'x' } });
  check('missing ops -> 400', noOps.status === 400 && noOps.json.error.code === 'E_INPUT', noOps.text);
  const badJson = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: '{not json' });
  check('invalid JSON body -> 400 E_JSON', badJson.status === 400 && badJson.json.error.code === 'E_JSON', badJson.text);
  const nodoc = await api(port, 'POST', '/api/docs/zzz.kerf.json/apply', { body: { ops: [] } });
  check('apply to a missing doc -> 404', nodoc.status === 404);
  const lg = await api(port, 'GET', '/api/docs/a.kerf.json/log');
  check('GET log: create + cli + designer entries', lg.json.entries.length === 3 && lg.json.next === 3 && lg.json.entries[2].who === 'designer', lg.text.slice(0, 200));
  const lg2 = await api(port, 'GET', '/api/docs/a.kerf.json/log?since=2');
  check('GET log?since=2 -> only the newest, next=3', lg2.json.entries.length === 1 && lg2.json.next === 3 && lg2.json.entries[0].why === 'designer adds d1', lg2.text.slice(0, 200));
  const lg3 = await api(port, 'GET', '/api/docs/a.kerf.json/log?since=99');
  check('GET log?since beyond the end -> empty, next=3', lg3.json.entries.length === 0 && lg3.json.next === 3);

  // SPEC 21: lenient ops (single op object, {ops, why} envelope, bare array body)
  const lenient1 = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: addOp('len1')[0], why: 'single op', actor: 'designer' } });
  check('apply: "ops" may be a single op object', lenient1.status === 200 && lenient1.json.ok === true && lenient1.json.changed.includes('len1'), lenient1.text.slice(0, 300));
  const lenient2 = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: { ops: addOp('len2'), why: 'inner why' }, actor: 'designer' } });
  check('apply: {"ops":{"ops":[...],"why":...}} supplies the why', lenient2.status === 200 && lenient2.json.ok === true && lenient2.json.changed.includes('len2'), lenient2.text.slice(0, 300));
  const lenient3 = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: addOp('len3'), });
  check('apply: the body itself may be the ops array', lenient3.status === 200 && lenient3.json.ok === true && lenient3.json.changed.includes('len3'), lenient3.text.slice(0, 300));
  const lenLog = await api(port, 'GET', '/api/docs/a.kerf.json/log');
  const lenWhys = lenLog.json.entries.slice(-3).map((e) => e.why);
  check('apply: the envelope why reaches the op log', lenWhys[0] === 'single op' && lenWhys[1] === 'inner why', lenWhys);
  const lenBad = await api(port, 'POST', '/api/docs/a.kerf.json/apply', { body: { ops: { foo: 1 } } });
  check('apply: a bad ops shape is a 400 E_PARAM that names the shapes', lenBad.status === 400 && lenBad.json.error.code === 'E_PARAM' && /single op object/.test(lenBad.json.error.message), lenBad.text.slice(0, 300));

  section('export');
  fs.copyFileSync(REF, path.join(dir, 'beam.kerf.json'));
  await sleep(700);
  const png = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=A&format=png&px=900');
  check('png: magic + content-type + filename', png.status === 200 && png.body.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) && png.headers['content-type'] === 'image/png' && /filename="beam-A\.png"/.test(png.headers['content-disposition']), png.headers);
  check('png px=900 -> IHDR width 900', png.body.readUInt32BE(16) === 900, png.body.readUInt32BE(16));
  const svg = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=A&format=svg&sheet=1');
  check('svg sheet', svg.status === 200 && svg.text.startsWith('<svg') || svg.text.includes('<svg'), svg.text.slice(0, 40));
  const dxf = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=B&format=dxf');
  check('dxf', dxf.status === 200 && /SECTION/.test(dxf.text) && /\.dxf"/.test(dxf.headers['content-disposition']));
  const pdf = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=A&format=pdf');
  check('pdf', pdf.status === 200 && pdf.text.startsWith('%PDF') && pdf.headers['content-type'] === 'application/pdf');
  const nv = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=ZZ&format=png');
  check('unknown view -> 404 error JSON', nv.status === 404 && nv.json.error.code === 'E_VIEW', nv.text.slice(0, 120));
  const nf2 = await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=A&format=gif');
  check('unknown format -> 400', nf2.status === 400);
  const nv2 = await api(port, 'GET', '/api/docs/beam.kerf.json/export?format=png');
  check('missing view -> 400', nv2.status === 400);
  const png_t0 = performance.now();
  await api(port, 'GET', '/api/docs/beam.kerf.json/export?view=A&format=png&sheet=1');
  const png_ms = performance.now() - png_t0;

  section('agent bridge (fake agent)');
  const runA = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'Please EDIT the plate', file: 'a.kerf.json' } });
  check('POST /api/agent/run -> run_id', runA.status === 200 && /^r\d+$/.test(runA.json.run_id ?? ''), runA.text);
  const rid = runA.json.run_id;
  const isRun = (f) => f.event === 'agent' && f.data.run_id === rid;
  const exitEv = await ev.waitFor((f) => isRun(f) && f.data.event.type === 'exit', 8000);
  const runEvents = ev.frames.filter(isRun).map((f) => f.data.event);
  check('stdout JSON line arrives as an event object (system/init)', runEvents.some((e) => e.type === 'system' && e.subtype === 'init' && e.session_id === 'fake-session-1'), runEvents.slice(0, 3));
  check('plain stdout line -> {type:text}', runEvents.some((e) => e.type === 'text' && /hello from the fake agent/.test(e.text)));
  check('stderr line -> {type:stderr}', runEvents.some((e) => e.type === 'stderr' && /a line on stderr/.test(e.text)));
  check('message was prefixed with the workspace context + file', runEvents.some((e) => e.type === 'assistant' && /EDIT the plate/.test(JSON.stringify(e))));
  check('exit event: code 0 + session_id captured', !!exitEv && exitEv.data.event.code === 0 && exitEv.data.event.session_id === 'fake-session-1', exitEv?.data);
  check('agent ran in --dir with KERF_ACTOR=agent and kerf on PATH', runEvents.some((e) => e.type === 'system' && e.actor === 'agent' && fs.realpathSync(e.cwd) === fs.realpathSync(dir)) && runEvents.some((e) => e.type === 'tool_result' && /^DOC a/.test(e.text)), runEvents);
  const aLog = await ev.waitFor((f) => f.event === 'log' && f.data.entry.why === 'fake agent edit', 3000);
  check('the agent edit shows up as a log event with who:agent', !!aLog && aLog.data.entry.who === 'agent' && aLog.data.entry.tool === 'kerf-cli', aLog);
  check('the agent\'s log event is published BEFORE its exit event', ev.frames.findIndex((f) => f.event === 'log' && f.data.entry.why === 'fake agent edit') < ev.frames.findIndex(isRun_exit(rid)), ev.frames.map((f) => f.event + ':' + (f.data?.event?.type ?? '')).slice(-12));
  check('the agent edit landed in the document', JSON.parse(fs.readFileSync(path.join(dir, 'a.kerf.json'), 'utf8')).components.some((x) => x.id === 'agent_plate'));

  const png1 = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');
  const runImg = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'look at this', images: [{ name: '../../evil name.png', data_base64: png1.toString('base64') }] } });
  const exitImg = await ev.waitFor((f) => f.event === 'agent' && f.data.run_id === runImg.json.run_id && f.data.event.type === 'exit', 8000);
  const imgMsg = ev.frames.find((f) => f.event === 'agent' && f.data.run_id === runImg.json.run_id && f.data.event.type === 'assistant');
  const attDir = path.join(dir, '.kerf', 'attachments');
  const saved = fs.existsSync(attDir) ? fs.readdirSync(attDir) : [];
  check('images: saved under .kerf/attachments with a sanitized name, bytes intact', saved.length === 1 && /^[0-9a-f]+-evil_name\.png$/.test(saved[0]) && fs.readFileSync(path.join(attDir, saved[0])).equals(png1), saved);
  check('images: the path was appended to the agent message', !!exitImg && /attachments/.test(JSON.stringify(imgMsg?.data.event)), imgMsg?.data);
  const badImg = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'x', images: [{ name: 'a.png', data_base64: '!!!not base64' }] } });
  check('images: invalid base64 -> 400 E_IMAGES', badImg.status === 400 && badImg.json.error.code === 'E_IMAGES', badImg.text);
  const runB = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'continue', session_id: 'sess-xyz' } });
  const ridB = runB.json.run_id;
  const exitB = await ev.waitFor((f) => f.event === 'agent' && f.data.run_id === ridB && f.data.event.type === 'exit', 8000);
  check('resume: session_id is passed through ({resume} expansion)', ev.frames.some((f) => f.event === 'agent' && f.data.run_id === ridB && f.data.event.type === 'text' && f.data.event.text === 'resumed sess-xyz') && exitB?.data.event.session_id === 'sess-xyz', exitB?.data);

  const runC = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'SLOW please' } });
  const ridC = runC.json.run_id;
  await ev.waitFor((f) => f.event === 'agent' && f.data.run_id === ridC && f.data.event.type === 'tick', 5000);
  const busy = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'second at once' } });
  check('one run at a time -> 409 E_BUSY', busy.status === 409 && busy.json.error.code === 'E_BUSY', busy.text);
  const infoBusy = await api(port, 'GET', '/api/info');
  check('info.active_run reports the running run', infoBusy.json.active_run?.run_id === ridC, infoBusy.json.active_run);
  const stopT0 = performance.now();
  const stop = await api(port, 'POST', '/api/agent/stop', { body: { run_id: ridC } });
  check('POST /api/agent/stop', stop.status === 200 && stop.json.stopped === true, stop.text);
  const exitC = await ev.waitFor((f) => f.event === 'agent' && f.data.run_id === ridC && f.data.event.type === 'exit', 5000);
  check('stopped run ends with an exit event (stopped:true) quickly', !!exitC && exitC.data.event.stopped === true && performance.now() - stopT0 < 3000, exitC?.data);
  const stopAgain = await api(port, 'POST', '/api/agent/stop', { body: { run_id: 'r999' } });
  check('stop unknown run -> 404', stopAgain.status === 404);
  const runD = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'FAIL now' } });
  const exitD = await ev.waitFor((f) => f.event === 'agent' && f.data.run_id === runD.json.run_id && f.data.event.type === 'exit', 8000);
  check('non-zero exit code is reported', exitD?.data.event.code === 3, exitD?.data);
  const ghostRun = await api(port, 'POST', '/api/agent/run', { body: { agent: 'ghost', message: 'x' } });
  check('unavailable agent -> 409 E_UNAVAILABLE with the reason', ghostRun.status === 409 && ghostRun.json.error.code === 'E_UNAVAILABLE' && /not found/.test(ghostRun.json.error.message), ghostRun.text);
  const unk = await api(port, 'POST', '/api/agent/run', { body: { agent: 'nope', message: 'x' } });
  check('unknown agent -> 404', unk.status === 404);
  const badSid = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'x', session_id: '--dangerous' } });
  check('session_id that looks like a flag -> 400', badSid.status === 400);
  const badFile = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake', message: 'x', file: '../../etc/passwd' } });
  check('file outside the folder -> 400', badFile.status === 400);
  const noMsg = await api(port, 'POST', '/api/agent/run', { body: { agent: 'fake' } });
  check('missing message -> 400', noMsg.status === 400);

  section('LLM proxy (against a local node server; no real API)');
  const seen = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const body = Buffer.concat(chunks).toString('utf8');
      seen.push({ method: req.method, url: req.url, headers: req.headers, body });
      if (req.url === '/v1/limited') { res.writeHead(429, { 'content-type': 'application/json', 'retry-after': '7' }); res.end('{"error":"slow down"}'); return; }
      if (req.url === '/v1/gz') { res.writeHead(200, { 'content-type': 'text/plain' }); res.end('plain'); return; }
      res.writeHead(200, { 'content-type': 'text/event-stream', 'x-request-id': 'up-1' });
      res.write('data: one\n\n');
      setTimeout(() => res.write('data: two\n\n'), 400);
      setTimeout(() => { res.write('data: [DONE]\n\n'); res.end(); }, 800);
    });
  });
  await new Promise((r) => upstream.listen(0, '127.0.0.1', r));
  const up = upstream.address().port;
  const base = `http://127.0.0.1:${up}`;
  const llmBody = { provider: 'custom', base_url: base + '/', path: '/v1/chat/completions', headers: { authorization: 'Bearer sk-secret', 'x-test': 'yes', 'Content-Type': 'application/json', Host: 'evil.example' }, body: { model: 'm', stream: true, messages: [{ role: 'user', content: 'hi' }] } };
  const firstChunkAt = {};
  const t_start = performance.now();
  const streamed = await new Promise((resolve, reject) => {
    const rq = http.request({ host: '127.0.0.1', port, method: 'POST', path: '/api/llm', headers: { 'content-type': 'application/json' } }, (res) => {
      const parts = [];
      res.on('data', (d) => { if (!firstChunkAt.t) firstChunkAt.t = performance.now() - t_start; parts.push(d.toString()); });
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, text: parts.join(''), chunks: parts.length }));
    });
    rq.on('error', reject);
    rq.end(JSON.stringify(llmBody));
  });
  const total_ms = performance.now() - t_start;
  check('proxy: status + SSE body forwarded verbatim', streamed.status === 200 && streamed.text === 'data: one\n\ndata: two\n\ndata: [DONE]\n\n' && /event-stream/.test(streamed.headers['content-type']), streamed);
  check('proxy: response is streamed (first chunk well before the end)', firstChunkAt.t < 350 && total_ms > 700 && streamed.chunks >= 2, [firstChunkAt.t, total_ms, streamed.chunks]);
  check('proxy: upstream header x-request-id passed back', streamed.headers['x-request-id'] === 'up-1');
  const u = seen[0];
  check('proxy: upstream got POST, path, auth + custom headers, JSON body', u.method === 'POST' && u.url === '/v1/chat/completions' && u.headers.authorization === 'Bearer sk-secret' && u.headers['x-test'] === 'yes' && JSON.parse(u.body).model === 'm', u);
  check('proxy: client-supplied Host is not forwarded', u.headers.host === `127.0.0.1:${up}`, u.headers.host);
  const lim = await api(port, 'POST', '/api/llm', { body: { provider: 'custom', base_url: base, path: '/v1/limited', body: {} } });
  check('proxy: upstream 429 + retry-after pass through', lim.status === 429 && lim.headers['retry-after'] === '7' && lim.json.error === 'slow down', lim.text);
  const getp = await api(port, 'POST', '/api/llm', { body: { provider: 'custom', base_url: base, path: '/v1/gz', method: 'GET' } });
  check('proxy: GET without body', getp.status === 200 && getp.text === 'plain' && seen.at(-1).method === 'GET', getp.text);
  const secretsSeenInEvents = JSON.stringify(ev.frames).includes('sk-secret');
  check('proxy: API keys never leak into SSE/log', !secretsSeenInEvents);
  const rules = [
    ['custom http to a public host', { provider: 'custom', base_url: 'http://example.com', path: '/v1' }],
    ['custom http to a public IP', { provider: 'custom', base_url: 'http://8.8.8.8', path: '/v1' }],
    ['anthropic with http base_url', { provider: 'anthropic', base_url: 'http://api.anthropic.com', path: '/v1/messages' }],
    ['openai pointed at localhost (https)', { provider: 'openai', base_url: 'https://localhost:9', path: '/v1' }],
    ['openai pointed at the metadata IP', { provider: 'openai', base_url: 'https://169.254.169.254', path: '/latest/meta-data' }],
    ['non-custom with a plain-http LAN url', { provider: 'openai', base_url: base, path: '/v1' }],
    ['file:// scheme', { provider: 'custom', base_url: 'file:///etc', path: '/passwd' }],
    ['credentials in the URL', { provider: 'custom', base_url: 'http://u:p@127.0.0.1:9', path: '/x' }],
    ['protocol-relative path', { provider: 'anthropic', path: '//evil.example/x' }],
    ['path without a leading slash', { provider: 'anthropic', path: 'v1/messages' }],
    ['unknown provider', { provider: 'nope', path: '/x' }],
    ['custom without base_url', { provider: 'custom', path: '/x' }],
    ['header injection', { provider: 'anthropic', path: '/x', headers: { a: 'b\r\nHost: evil' } }],
  ];
  for (const [name, body] of rules) {
    const rj = await api(port, 'POST', '/api/llm', { body });
    check('proxy rejects: ' + name, rj.status === 400 && !!rj.json?.error?.code, [rj.status, rj.text.slice(0, 120)]);
  }
  check('rejected requests never reached the upstream', seen.length === 3, seen.length);
  const dead = await api(port, 'POST', '/api/llm', { body: { provider: 'custom', base_url: 'http://127.0.0.1:1', path: '/x', body: {} } });
  check('proxy: unreachable upstream -> 502 E_UPSTREAM', dead.status === 502 && dead.json.error.code === 'E_UPSTREAM', dead.text);
  upstream.close();

  section('security: Host / Origin checks on localhost');
  const evilHost = await rawReq(port, 'GET', '/api/docs', { headers: { host: 'evil.example.com' } });
  check('DNS-rebinding style Host -> 403', evilHost.status === 403, evilHost.status);
  const evilOrigin = await rawReq(port, 'POST', '/api/docs', { headers: { origin: 'http://evil.example.com', 'content-type': 'text/plain' }, body: '{"file":"pwn.kerf.json"}' });
  check('cross-origin POST -> 403 and nothing created', evilOrigin.status === 403 && !fs.existsSync(path.join(dir, 'pwn.kerf.json')), evilOrigin.status);
  const sameOrigin = await rawReq(port, 'GET', '/api/docs', { headers: { origin: `http://127.0.0.1:${port}` } });
  check('same-origin request OK', sameOrigin.status === 200);
  const preflight = await rawReq(port, 'OPTIONS', '/api/docs', { headers: { origin: 'http://evil.example.com' } });
  check('preflight from a foreign origin gets no CORS headers', !preflight.headers['access-control-allow-origin']);

  section('concurrency + latency');
  const sses = [sse(port), sse(port), sse(port)];
  await sleep(100);
  const lat = [];
  for (let i = 0; i < 30; i++) { const t = performance.now(); await api(port, 'GET', '/api/docs'); lat.push(performance.now() - t); }
  lat.sort((a, b) => a - b);
  const burst = await Promise.all(Array.from({ length: 24 }, () => api(port, 'GET', '/api/info')));
  check('parallel requests succeed while 4 SSE streams are open', burst.every((x) => x.status === 200));
  sses.forEach((s) => s.close());
  const rss = (() => { try { return +fs.readFileSync(`/proc/${children[0].pid}/status`, 'utf8').match(/VmRSS:\s+(\d+)/)[1]; } catch { return null; } })();
  console.log(`  numbers: /api/info first call ${info_ms.toFixed(1)} ms; /api/docs p50 ${lat[15].toFixed(2)} ms p95 ${lat[28].toFixed(2)} ms; png sheet export ${png_ms.toFixed(0)} ms; RSS ${rss ? (rss / 1024).toFixed(1) + ' MB' : 'n/a'} (after the above)`);
  ev.close();
  children[0].kill('SIGTERM');

  section('token: --host 0.0.0.0 requires one');
  const dir2 = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-smoke2-'));
  cli(dir2, ['new', 't.kerf.json']);
  const lan = await startServer(dir2, ['--host', '0.0.0.0']);
  const m = /\?token=([0-9a-f]{32})/.exec(lan.banner);
  check('banner prints a LAN URL with an auto token', /LAN:\s+http:\/\/[\d.<>a-z-]+:\d+\/\?token=[0-9a-f]{32}/.test(lan.banner), lan.banner);
  const tok = m && m[1];
  const noTok = await api(lan.port, 'GET', '/api/docs');
  check('no token -> 401 E_TOKEN', noTok.status === 401 && noTok.json.error.code === 'E_TOKEN', noTok.text);
  const wrongTok = await api(lan.port, 'GET', '/api/docs', { headers: { authorization: 'Bearer ' + 'x'.repeat(32) } });
  check('wrong token -> 401', wrongTok.status === 401);
  const okTok = await api(lan.port, 'GET', '/api/docs', { headers: { authorization: 'Bearer ' + tok } });
  check('Authorization: Bearer <token> -> 200', okTok.status === 200 && okTok.json.length === 1, okTok.text);
  const qTok = await api(lan.port, 'GET', '/api/docs?token=' + tok);
  check('?token= works too', qTok.status === 200);
  const infoNo = await api(lan.port, 'GET', '/api/info');
  check('/api/info without a token tells the UI one is needed', infoNo.status === 200 && infoNo.json.token_required === true && infoNo.json.authenticated === false && !infoNo.json.dir, infoNo.text);
  const infoYes = await api(lan.port, 'GET', '/api/info', { headers: { authorization: 'Bearer ' + tok } });
  check('/api/info with the token is the full object', infoYes.json.token_required === true && infoYes.json.authenticated === true && infoYes.json.dir);
  const postNo = await api(lan.port, 'POST', '/api/docs', { body: { file: 'x' } });
  check('POST without token -> 401', postNo.status === 401);
  const evNo = await rawReq(lan.port, 'GET', '/api/events');
  check('SSE without token -> 401', evNo.status === 401);
  const sseTok = sse(lan.port, '?token=' + tok);
  const pingTok = await sseTok.waitFor((f) => f.event === 'ping', 2000);
  check('SSE with ?token= connects', !!pingTok);
  sseTok.close();
  const llmNo = await api(lan.port, 'POST', '/api/llm', { body: { provider: 'anthropic', path: '/x' } });
  check('proxy without token -> 401', llmNo.status === 401);
  const staticNo = await api(lan.port, 'GET', '/');
  check('UI page itself loads without a token (it reads ?token= from the URL)', staticNo.status === 200);
  lan.proc.kill('SIGTERM');

  const lan2 = await startServer(dir2, ['--host', '0.0.0.0', '--token', 'my-fixed-token']);
  const fixed = await api(lan2.port, 'GET', '/api/docs', { headers: { authorization: 'Bearer my-fixed-token' } });
  check('--token T is honored', fixed.status === 200 && lan2.banner.includes('token=my-fixed-token'));
  lan2.proc.kill('SIGTERM');
  const lan3 = await startServer(dir2, ['--host', '0.0.0.0', '--no-token']);
  const nt = await api(lan3.port, 'GET', '/api/docs');
  check('--no-token on 0.0.0.0 serves without a token (and warns)', nt.status === 200 && /NO TOKEN/.test(lan3.banner));
  lan3.proc.kill('SIGTERM');
  const loopTok = await startServer(dir2, ['--token', 'abc']);
  const lt = await api(loopTok.port, 'GET', '/api/docs');
  check('--token on localhost is enforced too', lt.status === 401);
  loopTok.proc.kill('SIGTERM');

  section('misc');
  const badDir = spawn(KERF, ['serve', '--dir', '/definitely/not/here'], { stdio: ['ignore', 'pipe', 'pipe'] });
  let berr = ''; badDir.stderr.on('data', (d) => { berr += d; });
  const bcode = await new Promise((r) => badDir.on('close', r));
  check('bad --dir -> exit 1 with a message', bcode === 1 && /cannot open --dir/.test(berr), berr);
  const portBusy = await startServer(dir2);
  const dup = spawn(KERF, ['serve', '--dir', dir2, '--port', String(portBusy.port)], { stdio: ['ignore', 'pipe', 'pipe'] });
  let derr = ''; dup.stderr.on('data', (d) => { derr += d; });
  const dcode = await new Promise((r) => dup.on('close', r));
  check('port in use -> exit 1 with a hint', dcode === 1 && /already in use/.test(derr), derr);
  portBusy.proc.kill('SIGTERM');

  console.log(`\n${passed} passed, ${failed} failed`);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.rmSync(dir2, { recursive: true, force: true });
  process.exit(failed ? 1 : 0);
}

main().catch((e) => { console.error(e); cleanup(); process.exit(1); });
