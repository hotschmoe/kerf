// Workspace-mode end-to-end tests (puppeteer + system chromium) against a `kerf serve` implementation.
//   node test/e2e-serve.mjs                       # against test/mock-serve.mjs (real engine wasm, scripted agent)
//   KERF_SERVE_BIN=../../engines/zig/zig-out/bin/kerf node test/e2e-serve.mjs --real     # against the real server (see runReal below)
// Needs apps/web/dist-serve (npm run build:serve). Screenshots of every workspace UI state go to $KERF_SHOTS (default: a temp dir).
import puppeteer from 'puppeteer-core';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { startMock, seedDir } from './mock-serve.mjs';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(web, '../..');
const REAL = process.argv.includes('--real');
const only = process.argv.find((a) => a.startsWith('--only='))?.slice(7);
const dist = path.join(web, 'dist-serve');
const shots = process.env.KERF_SHOTS || fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-shots-'));
fs.mkdirSync(shots, { recursive: true });
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-e2e-serve-'));
const sample = JSON.parse(fs.readFileSync(path.join(repo, 'spec/details/truss-bearing-cmu.kerf.json'), 'utf8'));
const TRUSS = 'truss-bearing-cmu.kerf.json';
const FLUSH = 'flush-beam-strap.kerf.json';

if (!fs.existsSync(path.join(dist, 'index.html'))) { console.error('dist-serve missing: run `npm run build:serve`'); process.exit(2); }

/** The server under test: the mock in-process, or the real `kerf serve` binary in a scratch folder. */
async function server(opts = {}) {
  if (!REAL) return startMock({ ui: dist, ...opts });
  const bin = path.resolve(process.env.KERF_SERVE_BIN || path.join(repo, 'engines/zig/zig-out/bin/kerf'));
  const dir = opts.dir ?? seedDir(fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-real-')), opts.files);
  const port = 7800 + Math.floor(Math.random() * 400);
  const args = ['serve', '--dir', dir, '--port', String(port), '--trust-agents', ...(opts.token ? ['--token', opts.token] : ['--no-token'])];
  const child = spawn(bin, args, { stdio: 'ignore' });
  for (let i = 0; i < 50; i++) { try { await fetch(`http://127.0.0.1:${port}/api/info`, { headers: opts.token ? { authorization: `Bearer ${opts.token}` } : {} }); break; } catch { await new Promise((r) => setTimeout(r, 100)); } }
  const state = new Proxy({}, { get: () => [] });
  return { port, url: `http://127.0.0.1:${port}`, dir, state, real: true, close: () => child.kill() };
}

const browser = await puppeteer.launch({
  executablePath: '/usr/bin/chromium', headless: 'new', protocolTimeout: 120000,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--window-size=1440,900'],
});
const cdp = await browser.target().createCDPSession();
await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: outDir });

let pass = 0, fail = 0, skipped = 0;
async function t(name, fn, { mockOnly = false, realOnly = false } = {}) {
  if (only && !name.includes(only)) return;
  if (REAL && mockOnly) { skipped++; console.log('  skip ' + name + ' (mock-only)'); return; }
  if (!REAL && realOnly) { skipped++; console.log('  skip ' + name + ' (real server only)'); return; }
  const servers = [];
  const track = async (opts) => { const s = await server(opts); servers.push(s); return s; };
  try { await fn(track); pass++; console.log('  ok  ' + name); }
  catch (e) { fail++; console.log('FAIL  ' + name + '\n      ' + String(e.stack || e).split('\n').slice(0, 12).join('\n      ')); }
  finally { for (const s of servers) s.close(); }
}

const ev = (page, js) => page.evaluate(js);
async function open(m, query = '', { w = 1440, h = 900, token } = {}) {
  const page = await browser.newPage();
  await page.setViewport({ width: w, height: h });
  page.errors = [];
  page.on('pageerror', (e) => page.errors.push(String(e.stack || e)));
  page.on('console', (msg) => { if (msg.type() === 'error' && !/favicon/.test(msg.text())) page.errors.push('console.error: ' + msg.text()); });
  await page.goto(`${m.url}/?${query}${token ? `&token=${token}` : ''}`, { waitUntil: 'load' });
  await page.waitForFunction('window.__ready===true || window.__bootError', { timeout: 30000 });
  const be = await page.evaluate('window.__bootError');
  if (be) throw new Error('boot error: ' + be);
  return page;
}
/** An agent's `kerf apply <file> --ops ... -w --why ...` in a terminal (the real CLI for the real server, a hook for the mock). */
async function external(m, file, ops, why, who = 'agent') {
  if (!m.real) return post(m, '/__external', { file, ops, why, who });
  const bin = path.resolve(process.env.KERF_SERVE_BIN || path.join(repo, 'engines/zig/zig-out/bin/kerf'));
  const r = spawnSync(bin, ['apply', file, '--ops', JSON.stringify(ops), '-w', '--why', why], { cwd: m.dir, encoding: 'utf8', env: { ...process.env, KERF_ACTOR: who } });
  if (r.status !== 0) throw new Error('kerf apply failed: ' + r.stdout + r.stderr);
  return { ok: true };
}
const post = (m, p, b) => fetch(m.url + p, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(b) }).then((r) => r.json());
const readDoc = (m, f) => JSON.parse(fs.readFileSync(path.join(m.dir, f), 'utf8'));
const readLog = (m, f) => (fs.existsSync(path.join(m.dir, f + '.log.jsonl')) ? fs.readFileSync(path.join(m.dir, f + '.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l)) : []);
const note = (m, f, id) => readDoc(m, f).views[0].annotations.find((a) => a.id === id);
const shot = async (page, name) => { await new Promise((r) => setTimeout(r, 250)); await page.screenshot({ path: path.join(shots, name + '.png') }); };
const status = (page) => ev(page, 'document.getElementById("status").textContent');
const waitFor = (page, js, timeout = 15000) => page.waitForFunction(js, { timeout }).catch(async (e) => { e.message += '\n      condition: ' + js.slice(0, 200) + '\n      console: ' + (await page.evaluate(`document.getElementById('msgs')?.innerText?.slice(-600)`).catch(() => '?')); throw e; });
const idle = (page, timeout = 30000) => waitFor(page, 'window.__kerf && !window.__kerf.session.busy', timeout);
const noErrors = (page, allow = /ResizeObserver/) => assert.deepEqual(page.errors.filter((e) => !allow.test(e)), []);
const noteText = (page, id) => ev(page, `window.__kerf.app.view.annotations.find(a => a.id === '${id}')?.text`);

// ======================================================================================= detection, library, opening
await t('workspace mode is detected; library lists docs live; first doc opens; header/status show the workspace', async (track) => {
  const m = await track();
  const page = await open(m, 'fast=1');
  assert.equal(await ev(page, 'window.__kerf.mode'), 'workspace');
  await waitFor(page, 'window.__rendered===true');
  const rows = await page.$$eval('.librow', (r) => r.map((x) => x.dataset.file));
  assert.deepEqual(rows, [FLUSH, TRUSS]);
  assert.equal(await ev(page, 'window.__kerf.app.doc.id'), 'flush-beam-strap'); // first by file name
  assert.match(await ev(page, 'document.querySelector("#hdr").textContent'), /DIR:/);
  assert.match(await status(page), /AGENT OK/);
  assert.equal(await ev(page, 'window.__kerf.session.choice'), 'claude'); // first available local agent by default
  assert.equal(await ev(page, '!!document.querySelector(".librow.sel")'), true);
  await shot(page, '01-workspace');
  // a document dropped into the folder appears (doc_added), counts update live (warnings from the engine's check)
  fs.copyFileSync(path.join(repo, 'spec/details/monopour-slab-door-recess.kerf.json'), path.join(m.dir, 'monopour.kerf.json'));
  await waitFor(page, 'document.querySelectorAll(".librow").length === 3');
  // click a row: opens that doc and remembers it
  await page.click('.librow[data-file="truss-bearing-cmu.kerf.json"]');
  await waitFor(page, 'window.__kerf.app.doc.id === "truss-bearing-cmu"');
  assert.equal(await ev(page, 'window.__kerf.ws.file'), TRUSS);
  assert.equal(await ev(page, 'localStorage.getItem("kerf.lastDoc")'), TRUSS);
  // a removed file disappears
  fs.rmSync(path.join(m.dir, 'monopour.kerf.json'));
  await waitFor(page, 'document.querySelectorAll(".librow").length === 2');
  noErrors(page);
  await page.close();
});

await t('static mode is untouched: no workspace UI, no /api probe when static=1; chat is off with a clear note', async (track) => {
  const m = await track();
  const page = await open(m, 'static=1&sample=truss-bearing-cmu');
  assert.equal(await ev(page, 'window.__kerf.mode'), 'static');
  assert.equal(await ev(page, '!!document.getElementById("library")'), false);
  assert.equal(await ev(page, 'window.__kerf.app.remote'), null);
  assert.equal(await ev(page, 'window.__kerf.session.available'), false);
  assert.equal(await ev(page, 'document.querySelector("#composer textarea").disabled'), true);
  assert.equal(await ev(page, 'document.querySelector("select.pick").disabled'), true);
  assert.match(await ev(page, 'document.querySelector("#msgs .intro").textContent'), /CHAT NEEDS `kerf serve` AND A LOCAL AGENT CLI \(claude, codex, grok OR pi\)/);
  assert.match(await status(page), /CHAT OFF/);
  await shot(page, '09-static-chat-off');
  await page.close();
});

await t('token: ?token= is stored in sessionStorage and scrubbed from the URL; API uses Authorization: Bearer; SSE uses ?token=', async (track) => {
  const m = await track({ token: 's3cret' });
  // without a token the UI says so
  const bad = await browser.newPage();
  await bad.goto(`${m.url}/`, { waitUntil: 'load' });
  await bad.waitForFunction('window.__bootError', { timeout: 15000 });
  assert.match(await bad.evaluate('window.__bootError'), /NEEDS A TOKEN/);
  await bad.close();
  const page = await open(m, 'fast=1', { token: 's3cret' });
  assert.equal(await ev(page, 'sessionStorage.getItem("kerf.token")'), 's3cret');
  assert.ok(!(await ev(page, 'location.search')).includes('token'));
  assert.equal(await ev(page, 'window.__kerf.app.doc.id'), 'flush-beam-strap');
  // the live feed works with the token (external edit is seen)
  await external(m, FLUSH, [{ op: 'update', path: 'meta', value: { tok: 1 } }], 'token test');
  await waitFor(page, 'window.__kerf.app.opLog.some(e => e.kind === "external")');
  await page.reload({ waitUntil: 'load' });
  await waitFor(page, 'window.__ready===true'); // token survives a reload (sessionStorage)
  assert.equal(await ev(page, 'window.__kerf.mode'), 'workspace');
  await page.close();
}, { mockOnly: true });

// ======================================================================================= live reload + LOCAL AGENT cards
await t('external edit: reloads in place (view, zoom, selection kept) and shows a LOCAL AGENT card + status line', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await waitFor(page, 'window.__rendered===true');
  await ev(page, `window.__kerf.app.setSelection('n_truss'); window.__kerf.vp.vp2.zoomBy(2.5);`);
  const before = await ev(page, 'JSON.stringify(window.__kerf.vp.vp2.view)');
  const rev = await ev(page, 'window.__kerf.app.rev');
  const ops = [{ op: 'update', path: 'views/A/annotations/n_truss', value: { text: 'EXTERNAL REWORD OF TRUSS NOTE' } }, { op: 'update', path: 'meta', value: { ext: 1 } }];
  await external(m, TRUSS, ops, 'Reword the truss note per RFI-12');
  await waitFor(page, `window.__kerf.app.rev > ${rev}`);
  await waitFor(page, `document.querySelectorAll('.msg.agent').length === 1`);
  assert.equal(await noteText(page, 'n_truss'), 'EXTERNAL REWORD OF TRUSS NOTE');
  assert.equal(await ev(page, 'JSON.stringify(window.__kerf.vp.vp2.view)'), before, 'zoom/pan preserved');
  assert.equal(await ev(page, 'window.__kerf.app.selection?.id'), 'n_truss');
  assert.equal(await ev(page, 'window.__kerf.app.activeView'), 'A');
  const card = await ev(page, `(() => { const c = document.querySelector('.msg.agent'); return { head: c.querySelector('.mh').textContent, why: c.querySelector('.why').textContent, meta: c.querySelector('.meta').textContent, sum: c.querySelector('.sum')?.textContent, ops: c.querySelector('details pre')?.textContent } })()`);
  assert.match(card.head, /LOCAL AGENT/);
  assert.equal(card.why, 'Reword the truss note per RFI-12');
  assert.match(card.meta, /2 OPS/);
  assert.match(card.sum, /^DOC truss-bearing-cmu/);
  assert.match(card.ops, /EXTERNAL REWORD/);
  assert.match(await status(page), /LOCAL AGENT · LAST EDIT \d+S AGO/);
  // log entries of an unrelated file also produce a card (labelled with the file)
  await shot(page, '02-local-agent-card');
  // no reload loop: nothing further happens
  await new Promise((r) => setTimeout(r, 600));
  assert.equal(await ev(page, 'window.__kerf.app.rev'), rev + 1);
  noErrors(page);
  await page.close();
});

await t('designer edits go to /apply (actor designer, if_match), land on disk, are logged, and do NOT reload or card', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await waitFor(page, 'window.__rendered===true');
  await ev(page, `window.__kerf.app.setSelection('n_truss'); document.querySelectorAll('.itabs .tab')[1].click();`);
  await page.waitForSelector('.idetail textarea');
  await page.$eval('.idetail textarea', (el) => { el.value = 'DESIGNER EDITED NOTE'; el.dispatchEvent(new Event('change', { bubbles: true })); });
  await waitFor(page, 'window.__kerf.app.opLog.filter(e => e.kind === "op").length === 1');
  assert.equal(note(m, TRUSS, 'n_truss').text, 'DESIGNER EDITED NOTE');
  if (!m.real) {
    const a = m.state.applies.at(-1);
    assert.equal(a.actor, 'designer'); assert.equal(a.file, TRUSS);
    assert.match(a.if_match, /^"?\d+-\d+"?$/, 'if_match sent: ' + a.if_match);
  }
  const log = readLog(m, TRUSS).at(-1);
  assert.equal(log.who, 'designer'); assert.match(log.why, /Edit note n_truss text/);
  // verify a citation (a designer-only stamp) and drag-style note edits use the same path
  await page.click('.idetail .stamp');
  await waitFor(page, 'window.__kerf.app.opLog.filter(e => e.kind === "op").length === 2');
  assert.equal(note(m, TRUSS, 'n_truss').cite[0].status, 'verified');
  await new Promise((r) => setTimeout(r, 700)); // the echo of our own writes must not reload or add cards
  assert.equal(await ev(page, 'window.__kerf.app.opLog.filter(e => e.kind === "external").length'), 0);
  assert.equal(await ev(page, 'document.querySelectorAll(".msg.agent").length'), 0);
  assert.equal(await ev(page, 'window.__kerf.ws.lastAgentEdit'), null);
  // a component parameter edit (scalar field in the inspector) is also a designer apply
  await ev(page, `window.__kerf.app.setSelection('anchor_bolt'); document.querySelectorAll('.itabs .tab')[0].click();`);
  await page.waitForSelector('.idetail input.pe[data-key="embed"]');
  await page.$eval('.idetail input.pe[data-key="embed"]', (el) => { el.value = '9'; el.dispatchEvent(new Event('change', { bubbles: true })); });
  await waitFor(page, 'window.__kerf.app.opLog.filter(e => e.kind === "op").length === 3');
  assert.equal(readDoc(m, TRUSS).components.find((c) => c.id === 'anchor_bolt').embed, 9); // numbers stay numbers
  assert.match(readLog(m, TRUSS).at(-1).why, /Set anchor_bolt embed to 9/);
  await shot(page, '14-param-edit');
  // undo writes the previous document back
  await ev(page, `document.querySelectorAll('.itabs .tab')[2].click()`);
  await page.waitForSelector('.logrow');
  await ev(page, `[...document.querySelectorAll('.logbar .btn')].find(b => b.textContent === 'UNDO LAST').click()`);
  await waitFor(page, 'window.__kerf.app.opLog.some(e => e.kind === "undo")');
  assert.equal(readDoc(m, TRUSS).components.find((c) => c.id === 'anchor_bolt').embed, 7); // the param edit was the last op group
  assert.equal(note(m, TRUSS, 'n_truss').cite[0].status, 'verified');
  assert.equal(note(m, TRUSS, 'n_truss').text, 'DESIGNER EDITED NOTE');
  assert.match(readLog(m, TRUSS).at(-1).why, /^Undo: /);
  noErrors(page);
  await page.close();
});

await t('409 conflict: stale if_match reloads the document and toasts DOCUMENT CHANGED ON DISK — RELOADED', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await waitFor(page, 'window.__rendered===true');
  // someone else edits the file; make our remembered ETag stale before the live feed would have refreshed it
  await ev(page, `window.__kerf.ws.etag = '"1-1"'`);
  await ev(page, `window.__kerf.app.setSelection('n_truss'); document.querySelectorAll('.itabs .tab')[1].click();`);
  await page.waitForSelector('.idetail textarea');
  await external(m, TRUSS, [{ op: 'update', path: 'views/A/annotations/n_truss', value: { text: 'AGENT WON THE RACE' } }], 'race');
  await page.$eval('.idetail textarea', (el) => { el.value = 'DESIGNER LOST THE RACE'; el.dispatchEvent(new Event('change', { bubbles: true })); });
  await waitFor(page, `document.getElementById('status').textContent.includes('DOCUMENT CHANGED ON DISK')`);
  assert.match(await status(page), /DOCUMENT CHANGED ON DISK — RELOADED/);
  assert.equal(note(m, TRUSS, 'n_truss').text, 'AGENT WON THE RACE'); // the stale edit was NOT written
  assert.equal(await noteText(page, 'n_truss'), 'AGENT WON THE RACE'); // and the UI shows the disk version
  assert.equal(await ev(page, 'window.__kerf.app.selection?.id'), 'n_truss');
  await shot(page, '03-conflict-toast');
  noErrors(page, /ResizeObserver|409 \(Conflict\)/);
  await page.close();
});

await t('NEW DETAIL form creates a library document and opens it; ADD SAMPLE imports a reference detail', async (track) => {
  const m = await track({ dir: fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-empty-')) });
  const page = await open(m, 'fast=1');
  assert.equal(await ev(page, 'window.__kerf.app.doc'), null);
  assert.match(await ev(page, 'document.querySelector(".liblist").textContent'), /NO DETAILS IN THIS FOLDER YET/);
  await shot(page, '04-empty-workspace');
  await page.click('#libnew');
  await page.waitForSelector('.dlg input');
  await page.type('.dlg input', 'Slab Edge Detail');
  await shot(page, '05-new-detail-form');
  await ev(page, `[...document.querySelectorAll('.dlg .btn')].find(b => b.textContent === 'CREATE').click()`);
  await waitFor(page, 'window.__kerf.ws.file === "slab-edge-detail.kerf.json" && window.__kerf.app.doc');
  assert.ok(fs.existsSync(path.join(m.dir, 'slab-edge-detail.kerf.json')));
  await waitFor(page, 'document.querySelectorAll(".librow").length === 1');
  // import a sample into the library
  await ev(page, `[...document.querySelectorAll('#hdr .btn')].find(b => /ADD SAMPLE/.test(b.textContent)).click()`);
  await page.waitForSelector('.menu .mi');
  await ev(page, `[...document.querySelectorAll('.menu .mi')].find(b => /truss-bearing-cmu/.test(b.textContent)).click()`);
  await waitFor(page, 'window.__kerf.ws.file === "truss-bearing-cmu.kerf.json" && window.__kerf.app.doc.components.length > 10');
  assert.equal(readDoc(m, TRUSS).components.length, sample.components.length);
  assert.equal(await ev(page, 'document.querySelectorAll(".librow").length'), 2);
  noErrors(page);
  await page.close();
});

await t('export buttons use /api/docs/:file/export in workspace mode (svg, pdf)', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}&view=A`);
  await waitFor(page, 'window.__rendered===true');
  for (const fmt of ['svg', 'pdf']) {
    const name = `truss-bearing-cmu-A.${fmt}`;
    fs.rmSync(path.join(outDir, name), { force: true });
    await ev(page, `[...document.querySelectorAll('.iexport .btn')].find(b => b.textContent === '${fmt.toUpperCase()}').click()`);
    for (let i = 0; i < 100 && !fs.existsSync(path.join(outDir, name)); i++) await new Promise((r) => setTimeout(r, 100));
    assert.ok(fs.existsSync(path.join(outDir, name)), name);
    await new Promise((r) => setTimeout(r, 200));
    const buf = fs.readFileSync(path.join(outDir, name));
    assert.ok(fmt === 'svg' ? /<svg/.test(buf.subarray(0, 400).toString()) : buf.subarray(0, 4).toString() === '%PDF');
    assert.match(await status(page), new RegExp(`EXPORTED TRUSS-BEARING-CMU-A.${fmt.toUpperCase()}`));
  }
  assert.ok(await ev(page, '[...document.querySelectorAll(".iexport .btn")].some(b => b.textContent === "PNG")'), 'PNG button in workspace mode');
  await page.close();
});

await t('connection loss shows WORKSPACE OFFLINE in the status line', async (track) => {
  const m = await track();
  const page = await open(m, 'fast=1');
  await new Promise((r) => setTimeout(r, 400));
  m.close();
  await waitFor(page, `document.getElementById('status').textContent.includes('WORKSPACE OFFLINE')`, 20000);
  await page.close();
}, { mockOnly: true });

// ======================================================================================= local agent
await t('local agent: run via /api/agent/run, manila card with ▸ KERF APPLY lines, doc reloads, session persisted + resumed, no duplicate log card', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await waitFor(page, 'window.__rendered===true');
  assert.equal(await ev(page, '[...document.querySelectorAll("select.pick option")].find(o => /GROK/.test(o.textContent)).disabled'), true, 'unavailable agent is disabled');
  await ev(page, `window.__kerf.con.sendText('please update the truss note')`);
  await waitFor(page, `window.__kerf.app.opLog.some(e => e.kind === 'external')`);
  await idle(page);
  const card = await ev(page, `(() => { const c = document.querySelector('.msg.agent'); return { head: c.querySelector('.mh').textContent, tools: [...c.querySelectorAll('.tl summary .tt')].map(e => e.textContent), status: [...c.querySelectorAll('.tl summary .ts')].map(e => e.textContent), text: c.querySelector('.seg')?.textContent } })()`);
  assert.match(card.head, /LOCAL AGENT · CLAUDE CODE/);
  assert.deepEqual(card.tools.map((x) => x.split(' ')[0]), ['kerf', 'kerf', 'READ']);
  assert.match(card.tools[1], /^kerf apply truss-bearing-cmu\.kerf\.json -w --why/);
  assert.match(card.status[1], /^✓ 0 ERR 0 WARN$/);
  assert.match(card.text, /Running kerf guide/);
  assert.equal(await ev(page, 'document.querySelectorAll(".msg.agent").length'), 1, 'log card suppressed during the UI-started run');
  const firstNote = sample.views[0].annotations.find((a) => a.type === 'note').id;
  assert.match(await noteText(page, firstNote), /\(AGENT\)$/);
  assert.match(await status(page), /LOCAL AGENT · LAST EDIT/);
  assert.equal(await ev(page, 'window.__kerf.session.busy'), false);
  assert.equal(await ev(page, `document.querySelector('#composer .btn.primary').textContent`), 'SEND');
  await shot(page, '06-agent-run');
  if (!m.real) {
    assert.equal(m.state.agentRuns[0].agent, 'claude'); assert.equal(m.state.agentRuns[0].file, TRUSS); assert.equal(m.state.agentRuns[0].session_id, undefined);
  }
  const sid = await ev(page, `localStorage.getItem('kerf.session.claude.${TRUSS}')`);
  assert.match(sid, /^sess-1$/);
  await ev(page, `window.__kerf.con.sendText('and once more')`);
  await idle(page);
  if (!m.real) assert.equal(m.state.agentRuns[1].session_id, sid, 'second message resumes the session');
  // NEW forgets the session
  await ev(page, `[...document.querySelectorAll('#console .btn')].find(b => b.textContent === 'NEW').click()`);
  assert.equal(await ev(page, `localStorage.getItem('kerf.session.claude.${TRUSS}')`), null);
  noErrors(page);
  await page.close();
}, { mockOnly: true });

await t('local agent: STOP kills the run; a failing tool call renders ✗', async (track) => {
  const m = await track();
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await waitFor(page, 'window.__rendered===true');
  await ev(page, `window.__kerf.con.sendText('please fail and be slow')`);
  await waitFor(page, `[...document.querySelectorAll('#composer .btn')].at(-1).textContent === 'STOP'`);
  await waitFor(page, `document.querySelectorAll('.msg.agent .tl').length >= 2`);
  assert.match(await status(page), /AGENT BUSY/);
  await shot(page, '07-agent-busy');
  await ev(page, `[...document.querySelectorAll('#composer .btn')].at(-1).click()`);
  await idle(page);
  assert.deepEqual(m.state.stops, ['run-1']);
  assert.ok(await ev(page, `[...document.querySelectorAll('.notice.err')].some(n => /EXITED WITH CODE 143/.test(n.textContent))`));
  assert.ok(await ev(page, `[...document.querySelectorAll('.msg.agent .ts.bad')].length >= 0`));
  noErrors(page);
  await page.close();
}, { mockOnly: true });

await t('REAL agent bridge (.kerf/agents.json + scripted stream-json agent): card, CLI edit reloads the doc, session id resumed, STOP -> exit 143', async (track) => {
  const dir = seedDir(fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-agent-')));
  fs.mkdirSync(path.join(dir, '.kerf'));
  fs.writeFileSync(path.join(dir, '.kerf/agents.json'), JSON.stringify([{ id: 'fakeagent', name: 'Fake Agent', detect: ['node', '--version'],
    argv: ['node', path.join(web, 'test/fake-agent.mjs'), '{message}', '{resume}'], resume: ['--resume', '{session_id}'] }]));
  const m = await track({ dir });
  const page = await open(m, `fast=1&doc=${TRUSS}`);
  await page.evaluate(() => localStorage.setItem('kerf.agent', 'fakeagent'));
  await page.reload({ waitUntil: 'load' });
  await waitFor(page, 'window.__ready===true && window.__rendered===true');
  assert.equal(await ev(page, 'window.__kerf.session.choice'), 'fakeagent');
  await ev(page, `window.__kerf.con.sendText('EDIT the first note')`);
  await waitFor(page, `window.__kerf.app.opLog.some(e => e.kind === 'external')`, 30000);
  await idle(page, 30000);
  const card = await ev(page, `(() => { const c = document.querySelector('.msg.agent'); return { head: c.querySelector('.mh').textContent, tools: [...c.querySelectorAll('.tl summary .tt')].map(e => e.textContent), status: [...c.querySelectorAll('.tl summary .ts')].map(e => e.textContent) } })()`);
  assert.match(card.head, /LOCAL AGENT · FAKE AGENT/);
  assert.match(card.tools[1], /^kerf apply truss-bearing-cmu\.kerf\.json -w --why/);
  assert.match(card.status[1], /^✓ 0 ERR 0 WARN$/);
  assert.equal(await ev(page, 'document.querySelectorAll(".msg.agent").length'), 1, 'no duplicate op-log card for the UI-started run');
  const firstNote = readDoc(m, TRUSS).views[0].annotations.find((a) => a.type === 'note');
  assert.match(firstNote.text, /\(AGENT\)$/);
  assert.equal(readLog(m, TRUSS).at(-1).who, 'agent');
  assert.equal(await ev(page, `localStorage.getItem('kerf.session.fakeagent.${TRUSS}')`), 'fake-sess-1');
  await shot(page, '20-real-agent-run');
  // STOP
  await ev(page, `window.__kerf.con.sendText('SLOW please')`);
  await waitFor(page, `[...document.querySelectorAll('#composer .btn')].at(-1).textContent === 'STOP'`);
  await waitFor(page, `document.querySelectorAll('.msg.agent').length >= 2 && document.querySelectorAll('.msg.agent:last-of-type .tl').length >= 1`);
  await ev(page, `[...document.querySelectorAll('#composer .btn')].at(-1).click()`);
  await idle(page, 30000);
  assert.ok(await ev(page, `[...document.querySelectorAll('.notice.err')].some(n => /EXITED WITH CODE/.test(n.textContent))`));
  noErrors(page);
  await page.close();
}, { realOnly: true });

// ======================================================================================= local agents only
await t('no direct provider path: POST /api/llm is 404 and /api/info has no proxy field', async (track) => {
  const m = await track();
  const llm = await fetch(`${m.url}/api/llm`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"provider":"custom","path":"/x"}' });
  assert.equal(llm.status, 404);
  const info = await (await fetch(`${m.url}/api/info`)).json();
  assert.ok(!('proxy' in info), Object.keys(info).join(','));
});

await t('the console lists local agents only: no API key, setup or provider storage', async (track) => {
  const m = await track();
  const page = await open(m, 'fast=1');
  const opts = await ev(page, `[...document.querySelectorAll('select.pick option')].map(o => o.value + (o.disabled ? '!' : ''))`);
  assert.deepEqual(opts, ['claude', 'grok!', 'codex']);
  assert.equal(await ev(page, '!document.getElementById("setupbtn")'), true);
  assert.equal(await ev(page, 'Object.keys(localStorage).some(k => /apiKey|\\.key\\./.test(k))'), false);
  await page.select('select.pick', 'codex');
  assert.equal(await ev(page, `localStorage.getItem('kerf.agent')`), 'codex');
  await shot(page, '10-agent-picker');
  await page.setViewport({ width: 420, height: 800 });
  await ev(page, `document.querySelector('#mtabs .tab[data-p=console]').click()`);
  await shot(page, '13-narrow-console');
  noErrors(page);
  await page.close();
}, { mockOnly: true });

await browser.close();
console.log(`\n${pass} passed, ${fail} failed, ${skipped} skipped. screenshots: ${shots}`);
process.exit(fail ? 1 : 0);
