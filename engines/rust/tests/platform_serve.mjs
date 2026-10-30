#!/usr/bin/env node
// Platform checks for `kerf serve` that serve_smoke.mjs does not cover (written for Windows, valid everywhere):
// random tokens, the served folder path as the API shows it, a server killed hard takes its agent tree with it,
// the op log held by the server while CLI writers run, and links that must not be followed.
//   node tests/platform_serve.mjs path/to/kerf[.exe]
import { spawn, spawnSync } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const WIN = process.platform === 'win32';
const here = path.dirname(fileURLToPath(import.meta.url));
const KERF = path.resolve(process.argv[2] ?? path.join(here, '..', 'target', 'release', WIN ? 'kerf.exe' : 'kerf'));
const FAKE = path.join(here, '..', '..', 'zig', 'tests', 'fake-agent.mjs');
let passed = 0, failed = 0;
const check = (name, cond, extra) => {
  if (cond) { passed++; console.log('  ok   ' + name); } else { failed++; console.log('  FAIL ' + name + (extra !== undefined ? '  -> ' + JSON.stringify(extra) : '')); }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-plat-'));
const kids = [];
process.on('exit', () => { for (const k of kids) try { k.kill(); } catch {} try { fs.rmSync(tmp, { recursive: true, force: true }); } catch {} });
const env = { ...process.env, PATH: path.dirname(process.execPath) + path.delimiter + process.env.PATH };
delete env.KERF_TOKEN;
const isAlive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };

function start(dir, args = []) {
  return new Promise((resolve, reject) => {
    const p = spawn(KERF, ['serve', '--dir', dir, '--port', '0', ...args], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    kids.push(p);
    let out = '', err = '';
    const t = setTimeout(() => reject(new Error('no banner: ' + out + err)), 8000);
    p.stdout.on('data', (d) => {
      out += d;
      const m = /http:\/\/[\d.]+:(\d+)\/(?:\?token=(\w+))?/.exec(out);
      if (m && out.includes('\n')) { clearTimeout(t); setTimeout(() => resolve({ proc: p, port: +m[1], token: m[2], banner: out }), 80); }
    });
    p.stderr.on('data', (d) => { err += d; });
    p.on('exit', (c) => { clearTimeout(t); reject(new Error('server exited ' + c + ' ' + out + err)); });
  });
}
function api(srv, method, p, body) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? null : JSON.stringify(body);
    const headers = srv.token ? { authorization: 'Bearer ' + srv.token } : {};
    if (data !== null) { headers['content-type'] = 'application/json'; headers['content-length'] = Buffer.byteLength(data); }
    const q = http.request({ host: '127.0.0.1', port: srv.port, method, path: p, headers }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => { const text = Buffer.concat(chunks).toString('utf8'); let json = null; try { json = JSON.parse(text); } catch {} resolve({ status: res.statusCode, text, json }); });
    });
    q.on('error', reject);
    if (data !== null) q.write(data);
    q.end();
  });
}
function events(srv) {
  const frames = [];
  http.get({ host: '127.0.0.1', port: srv.port, path: '/api/events', headers: srv.token ? { authorization: 'Bearer ' + srv.token } : {} }, (res) => {
    res.setEncoding('utf8');
    let buf = '';
    res.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const raw = buf.slice(0, i); buf = buf.slice(i + 2);
        const ev = /^event: (.*)$/m.exec(raw)?.[1]; const dl = /^data: (.*)$/m.exec(raw)?.[1];
        if (ev && dl) { try { frames.push({ event: ev, data: JSON.parse(dl) }); } catch {} }
      }
    });
  }).on('error', () => {});
  return { frames, wait: async (pred, ms) => { for (let t = 0; t < ms; t += 50) { const f = frames.find(pred); if (f) return f; await sleep(50); } return null; } };
}

const dir = path.join(tmp, 'work');
fs.mkdirSync(path.join(dir, '.kerf'), { recursive: true });
fs.writeFileSync(path.join(dir, '.kerf', 'agents.json'), JSON.stringify({ agents: [
  { id: 'fake', detect: [process.execPath, '--version'], argv: [process.execPath, FAKE, '{message}', '{resume}'], resume: ['--resume', '{session_id}'] }] }));
const REF = path.join(here, '..', '..', '..', 'spec', 'details', 'flush-beam-strap.kerf.json');
fs.copyFileSync(REF, path.join(dir, 'a.kerf.json'));

console.log('# tokens and paths');
const s1 = await start(dir, ['--trust-agents']);
const s2 = await start(dir, ['--trust-agents']);
check('the default token is 32 hex digits', /^[0-9a-f]{32}$/.test(s1.token ?? ''), s1.token);
check('two servers get different tokens', s1.token !== s2.token, [s1.token, s2.token]);
s2.proc.kill();
const info = await api(s1, 'GET', '/api/info');
check('info.dir has no verbatim \\\\?\\ prefix', info.json && typeof info.json.dir === 'string' && !info.json.dir.startsWith('\\\\?\\'), info.json?.dir);
check('the banner shows the plain folder path', !s1.banner.includes('\\\\?\\'), s1.banner);
const real = fs.realpathSync(dir);
check('info.dir is the served folder', info.json?.dir?.toLowerCase() === real.toLowerCase().replace(/^\\\\\?\\/, ''), [info.json?.dir, real]);

console.log('\n# op log held by the server while CLI writers run');
const workers = [];
for (let i = 0; i < 8; i++) {
  const ops = JSON.stringify([{ op: 'add', path: 'components', value: { id: 'cli_' + i, type: 'lumber', size: '2x4', at: { x: i, y: 0 } } }]);
  workers.push(new Promise((resolve) => {
    const p = spawn(KERF, ['apply', 'a.kerf.json', '--ops', ops, '-w', '--why', 'cli ' + i], { cwd: dir, env, stdio: 'ignore' });
    p.on('exit', (c) => resolve(c));
  }));
}
const apiEdits = [];
for (let i = 0; i < 4; i++) {
  const ops = [{ op: 'add', path: 'components', value: { id: 'api_' + i, type: 'lumber', size: '2x4', at: { x: i, y: 5 } } }];
  apiEdits.push(api(s1, 'POST', '/api/docs/a.kerf.json/apply', { ops, actor: 'designer', why: 'api ' + i }));
}
const codes = await Promise.all(workers);
const apiRes = await Promise.all(apiEdits);
check('8 CLI applies exit 0 while the server holds the log', codes.every((c) => c === 0), codes);
check('4 API applies answered 200', apiRes.every((r) => r.status === 200), apiRes.map((r) => r.status + r.text.slice(0, 80)));
const logText = fs.readFileSync(path.join(dir, 'a.kerf.json.log.jsonl'), 'utf8');
const lines = logText.split('\n').filter(Boolean);
let validLines = 0; for (const l of lines) { try { JSON.parse(l); validLines++; } catch {} }
check('every log line is whole JSON, one per write', validLines === lines.length && lines.length >= 12, [validLines, lines.length]);
const doc = JSON.parse(fs.readFileSync(path.join(dir, 'a.kerf.json'), 'utf8'));
const ids = new Set(doc.components.map((c) => c.id));
check('no edit lost', [...Array(8).keys()].every((i) => ids.has('cli_' + i)) && [...Array(4).keys()].every((i) => ids.has('api_' + i)), [...ids].filter((x) => /^(cli|api)_/.test(x)));

console.log('\n# links are not followed');
const outside = path.join(tmp, 'outside.kerf.json');
fs.copyFileSync(REF, outside);
let linked = false;
try { fs.symlinkSync(outside, path.join(dir, 'link.kerf.json'), 'file'); linked = true; } catch (e) { console.log('  (file symlink not permitted here: ' + e.code + ')'); }
if (linked) {
  await sleep(1200);
  const list = await api(s1, 'GET', '/api/docs');
  check('a file symlink is not listed', !list.text.includes('link.kerf.json'), list.text.slice(0, 200));
  const got = await api(s1, 'GET', '/api/docs/link.kerf.json');
  check('a file symlink is not served', got.status === 404 || got.status === 400, got.status);
}
const target = path.join(tmp, 'elsewhere'); fs.mkdirSync(target);
const attach = path.join(dir, '.kerf', 'attachments');
fs.symlinkSync(target, attach, 'junction'); // a junction needs no privilege on Windows
const s3 = await start(dir, ['--trust-agents']);
const img = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';
const withImg = await api(s3, 'POST', '/api/agent/run', { agent: 'fake', message: 'hi', images: [{ name: 'x.png', data_base64: img }] });
check('attachments through a linked .kerf/attachments are refused', withImg.status >= 400 && fs.readdirSync(target).length === 0, [withImg.status, withImg.text.slice(0, 160), fs.readdirSync(target)]);
s3.proc.kill();
fs.rmSync(attach, { recursive: true, force: true });

console.log('\n# a server that dies hard takes its agent tree with it');
const ev = events(s1);
await ev.wait((f) => f.event === 'ping', 3000);
const run = await api(s1, 'POST', '/api/agent/run', { agent: 'fake', message: 'TREE please' });
check('TREE run starts', run.status === 200, run.text);
const pids = await ev.wait((f) => f.event === 'agent' && f.data.run_id === run.json?.run_id && f.data.event.type === 'pids', 5000);
check('pids reported', !!pids);
if (pids) {
  const { self, child } = pids.data.event;
  check('agent and grandchild are alive', isAlive(self) && isAlive(child));
  s1.proc.kill(WIN ? undefined : 'SIGKILL'); // TerminateProcess: no cleanup code runs
  let gone = false;
  for (let i = 0; i < 60 && !gone; i++) { await sleep(100); gone = !isAlive(self) && !isAlive(child); }
  if (WIN) check('both are gone shortly after the server was killed (job object closed)', gone, [isAlive(self), isAlive(child)]); // POSIX: a SIGKILLed server orphans its agent (documented)
  for (const pid of [self, child]) try { process.kill(pid); } catch {}
}
console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
