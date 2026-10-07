#!/usr/bin/env node
// Field-shaped `kerf serve` session, written for Windows (v0.1.0-alpha.7 crashed there) and run on every platform:
// `kerf init` in a fresh folder, `kerf serve --open`, then the sequence the web UI performs when a designer opens a
// document (info, docs list, document, log, SSE, exports), then a 30 s agent run through an npm-style `grok` shim
// (`grok.cmd` on Windows) that edits the folder through `kerf apply -w` while the UI is connected. The server must stay
// alive and answer to the end. Any panic or early exit fails the test and prints the server's stderr.
//
//   node tests/windows_serve.mjs [path/to/kerf[.exe]]
import { spawn, spawnSync } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const WIN = process.platform === 'win32';
const here = path.dirname(fileURLToPath(import.meta.url));
const KERF = path.resolve(process.argv[2] ?? path.join(here, '..', 'zig-out', 'bin', WIN ? 'kerf.exe' : 'kerf'));
const REF = path.join(here, '..', '..', '..', 'spec', 'details', 'flush-beam-strap.kerf.json');
const FAKE_GROK = path.join(here, 'fake-grok.mjs');
if (!fs.existsSync(KERF)) { console.error('kerf binary not found: ' + KERF); process.exit(2); }

let passed = 0, failed = 0;
const check = (name, cond, extra) => {
  if (cond) { passed++; console.log('  ok   ' + name); } else { failed++; console.log('  FAIL ' + name + (extra !== undefined ? '  -> ' + (typeof extra === 'string' ? extra : JSON.stringify(extra)) : '')); }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-win-'));
const shims = path.join(tmp, 'shims');
const work = path.join(tmp, 'details');
fs.mkdirSync(shims); fs.mkdirSync(work);

// npm-style shims for every built-in agent name. grok really runs; the others only answer --version.
const nodeExe = process.execPath;
if (WIN) {
  fs.writeFileSync(path.join(shims, 'grok.cmd'), `@echo off\r\nif "%~1"=="--version" (echo grok 0.9.9 & exit /b 0)\r\n"${nodeExe}" "${FAKE_GROK}" %*\r\n`);
  for (const n of ['claude', 'codex']) fs.writeFileSync(path.join(shims, n + '.cmd'), `@echo off\r\necho ${n} 1.2.3 (shim)\r\n`);
  fs.writeFileSync(path.join(shims, 'pi.ps1'), "Write-Output 'pi 9'\r\n"); // not runnable by CreateProcess: pi must report unavailable, not crash
} else {
  fs.writeFileSync(path.join(shims, 'grok'), `#!/bin/sh\nif [ "$1" = "--version" ]; then echo "grok 0.9.9"; exit 0; fi\nexec "${nodeExe}" "${FAKE_GROK}" "$@"\n`, { mode: 0o755 });
  for (const n of ['claude', 'codex']) fs.writeFileSync(path.join(shims, n), `#!/bin/sh\necho "${n} 1.2.3 (shim)"\n`, { mode: 0o755 });
}
const env = { ...process.env, PATH: shims + path.delimiter + path.dirname(nodeExe) + path.delimiter + process.env.PATH };
delete env.KERF_TOKEN;

const run = (args, opts = {}) => spawnSync(KERF, args, { cwd: work, env, encoding: 'utf8', ...opts });

console.log('# kerf init');
let r = run(['init']);
check('kerf init exits 0', r.status === 0, r.stdout + r.stderr);
fs.copyFileSync(REF, path.join(work, 'beam.kerf.json'));
r = run(['new', 'a.kerf.json', '--title', 'WIN A']);
check('kerf new', r.status === 0, r.stdout + r.stderr);

console.log('\n# kerf serve --open');
const server = spawn(KERF, ['serve', '--open', '--port', '0'], { cwd: work, env, stdio: ['ignore', 'pipe', 'pipe'] });
let sout = '', serr = '', exited = null;
server.stdout.on('data', (d) => { sout += d; });
server.stderr.on('data', (d) => { serr += d; });
server.on('exit', (code, sig) => { exited = { code, sig }; });
process.on('exit', () => { try { server.kill('SIGKILL'); } catch {} try { fs.rmSync(tmp, { recursive: true, force: true }); } catch {} });
for (let i = 0; i < 100 && !/\?token=\w+/.test(sout) && !exited; i++) await sleep(100);
const m = /http:\/\/127\.0\.0\.1:(\d+)\/\?token=(\w+)/.exec(sout);
check('banner with URL and token', !!m, sout + serr);
if (!m) { console.log(serr); process.exit(1); }
const port = +m[1], token = m[2];
const alive = (what) => check('server alive after ' + what, exited === null, { exited, stderr: serr.slice(-2000) });

function req(method, p, { body, auth = true } = {}) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? null : JSON.stringify(body);
    const headers = auth ? { authorization: 'Bearer ' + token } : {};
    if (data !== null) { headers['content-type'] = 'application/json'; headers['content-length'] = Buffer.byteLength(data); }
    const q = http.request({ host: '127.0.0.1', port, method, path: p, headers }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => { const b = Buffer.concat(chunks); let json = null; try { json = JSON.parse(b.toString('utf8')); } catch {} resolve({ status: res.statusCode, headers: res.headers, body: b, json }); });
    });
    q.on('error', reject);
    if (data !== null) q.write(data);
    q.end();
  });
}
const frames = [];
function sse() {
  const q = http.get({ host: '127.0.0.1', port, path: '/api/events', headers: { authorization: 'Bearer ' + token } }, (res) => {
    let buf = ''; res.setEncoding('utf8');
    res.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\n\n')) >= 0) { const raw = buf.slice(0, i); buf = buf.slice(i + 2); const ev = /event: (.*)/.exec(raw)?.[1]; const dt = /data: (.*)/.exec(raw)?.[1]; let j = null; try { j = JSON.parse(dt); } catch {} frames.push({ event: ev, data: j }); } });
  });
  q.on('error', () => {});
  return q;
}
const waitFor = async (pred, ms) => { const t = Date.now(); while (Date.now() - t < ms) { const f = frames.find(pred); if (f) return f; if (exited) return null; await sleep(100); } return null; };

await sleep(1500); // the startup prefetch of agent detection has run
alive('startup (1.5 s: banner, agent detection, --open)');
let info = await req('GET', '/api/info');
check('GET /api/info (token)', info.status === 200 && Array.isArray(info.json?.agents), info.body.toString());
const grok = info.json?.agents?.find((a) => a.id === 'grok');
check('the grok shim is detected (available, version)', grok?.available === true && /grok 0\.9\.9/.test(grok.version ?? ''), grok);
console.log('       agents: ' + JSON.stringify(info.json?.agents?.map((a) => [a.id, a.available, a.version ?? a.reason])));
for (const a of info.json?.agents ?? []) check(`agent ${a.id}: available or has a reason`, a.available === true || typeof a.reason === 'string', a);

console.log('\n# the UI opens a document');
const ev = sse();
const docs = await req('GET', '/api/docs');
check('GET /api/docs lists both documents', docs.status === 200 && (docs.json?.docs ?? docs.json)?.length === 2, docs.body.toString());
const get = await req('GET', '/api/docs/beam.kerf.json');
check('GET document + etag', get.status === 200 && !!get.headers.etag, get.status);
const etag = get.headers.etag;
const lg = await req('GET', '/api/docs/beam.kerf.json/log');
check('GET log', lg.status === 200, lg.body.toString().slice(0, 200));
for (const [fmt, q] of [['svg', 'view=A&format=svg'], ['png', 'view=A&format=png&px=600'], ['dxf', 'view=B&format=dxf'], ['pdf', 'view=A&format=pdf']]) {
  const e = await req('GET', `/api/docs/beam.kerf.json/export?${q}`);
  check('export ' + fmt, e.status === 200 && e.body.length > 100, [e.status, e.body.toString().slice(0, 200)]);
}
const ap = await req('POST', '/api/docs/a.kerf.json/apply', { body: { ops: [{ op: 'add', path: 'components', value: { id: 'd1', type: 'lumber', size: '2x4', at: { x: 0, y: 0 } } }], why: 'designer adds d1', actor: 'designer' } });
check('designer apply (atomic write, fsync, log append)', ap.status === 200, ap.body.toString().slice(0, 300));
const lg2 = await req('GET', '/api/docs/a.kerf.json/log');
check('the log shows the edit', lg2.status === 200 && JSON.stringify(lg2.json).includes('designer adds d1'), lg2.body.toString().slice(0, 200));
const stale = await req('POST', '/api/docs/beam.kerf.json/apply', { body: { ops: [], if_match: '"nope"' } });
check('stale if_match is an error response, not a crash', stale.status >= 400 && stale.status < 500, stale.status);
fs.writeFileSync(path.join(work, 'beam.kerf.json'), fs.readFileSync(REF, 'utf8') + ' '); // an outside edit; the poller must report it
const chg = await waitFor((f) => f.event === 'doc_changed' && f.data?.file === 'beam.kerf.json', 4000);
check('poller reports the outside edit over SSE', !!chg);
alive('the document-open sequence');

console.log('\n# agent run through the grok shim (30 s, edits the folder)');
const run1 = await req('POST', '/api/agent/run', { body: { agent: 'grok', message: 'WORK15 add studs', file: 'a.kerf.json' } });
check('POST /api/agent/run -> 200 with a run id', run1.status === 200 && !!run1.json?.run_id, run1.body.toString());
const rid = run1.json?.run_id;
const first = await waitFor((f) => f.event === 'agent' && f.data?.run_id === rid && f.data.event?.type === 'system', 8000);
check('agent output arrives over SSE', !!first, frames.slice(-5));
for (let i = 0; i < 6; i++) { // while it works: keep fetching like the UI does
  await sleep(5000);
  const d = await req('GET', '/api/docs/a.kerf.json');
  const l = await req('GET', '/api/docs/a.kerf.json/log');
  if (d.status !== 200 || l.status !== 200 || exited) { check('UI requests keep working during the run', false, [d.status, l.status, exited, serr.slice(-1500)]); break; }
}
const done = await waitFor((f) => f.event === 'agent' && f.data?.run_id === rid && f.data.event?.type === 'exit', 20000);
check('the run ends with exit code 0', done?.data?.event?.code === 0, [done?.data, serr.slice(-1500)]);
const logEvents = frames.filter((f) => f.event === 'log' && /grok round/.test(f.data?.entry?.why ?? '')).length;
check('the agent\'s 15 edits were reported as log events', logEvents >= 15, logEvents);
const finalDoc = await req('GET', '/api/docs/a.kerf.json');
check('the document holds the agent\'s components', finalDoc.status === 200 && (finalDoc.body.toString().match(/"id": ?"g[a-z0-9]+"/g) ?? []).length >= 15, finalDoc.body.toString().slice(0, 200));
alive('the 30 s agent run');

// a second run, stopped half way (Windows: the job object takes the process tree down)
const run2 = await req('POST', '/api/agent/run', { body: { agent: 'grok', message: 'WORK30 more', file: 'a.kerf.json' } });
check('second run starts', run2.status === 200, run2.body.toString());
await sleep(3000);
const stop = await req('POST', '/api/agent/stop', { body: { run_id: run2.json?.run_id } });
check('stop is accepted', stop.status === 200, stop.body.toString());
const done2 = await waitFor((f) => f.event === 'agent' && f.data?.run_id === run2.json?.run_id && f.data.event?.type === 'exit', 10000);
check('the stopped run reports exit', !!done2, serr.slice(-1500));
alive('stopping a run');
ev.destroy();

server.kill();
await sleep(500);
console.log(`\n${passed} passed, ${failed} failed`);
if (failed || serr.trim()) console.log('--- server stderr:\n' + serr);
process.exit(failed ? 1 : 0);
