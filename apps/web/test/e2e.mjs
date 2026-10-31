// End-to-end tests (puppeteer-core + system chromium). usage: node test/e2e.mjs [fixture] [--keep]
// Needs apps/web/dist (npm run build), or dist-fixture (npm run build:fixture) with `fixture`. Exports are validated with tools/dxf_check.py / pdf_check.py.
import puppeteer from 'puppeteer-core';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(web, '../..');
const fixture = process.argv[2] === 'fixture';
const dist = path.join(web, fixture ? 'dist-fixture' : 'dist');
const port = 8300 + Math.floor(Math.random() * 500);
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-e2e-'));

const server = spawn('node', [path.join(repo, 'tools/serve.mjs'), dist, String(port)], { stdio: ['ignore', fs.openSync(path.join(outDir, 'serve.log'), 'a'), fs.openSync(path.join(outDir, 'serve.err'), 'a')] });
await new Promise((r) => setTimeout(r, 600));
const browser = await puppeteer.launch({
  executablePath: process.env.KERF_CHROME ?? '/usr/bin/chromium', headless: 'new', protocolTimeout: 120000,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--window-size=1440,900', ...(process.env.KERF_CHROME_ARGS?.split(' ') ?? [])],
});
const cdp = await browser.target().createCDPSession();
await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: outDir });

let pass = 0, fail = 0, skipped = 0;
const results = [];
async function t(name, fn) {
  try { await fn(); pass++; results.push(['ok', name]); console.log('  ok  ' + name); }
  catch (e) { fail++; results.push(['FAIL', name, String(e.stack || e)]); console.log('FAIL  ' + name + '\n      ' + String(e.message || e).split('\n').join('\n      ')); }
}

async function open(query, { w = 1440, h = 900 } = {}) {
  const page = await browser.newPage();
  await page.setViewport({ width: w, height: h });
  page.errors = [];
  page.on('pageerror', (e) => page.errors.push(String(e.stack || e)));
  page.on('console', (m) => { if (m.type() === 'error') page.errors.push('console.error: ' + m.text()); });
  await page.goto(`http://localhost:${port}/?${query}`, { waitUntil: 'load' });
  await page.waitForFunction('window.__ready===true || window.__bootError', { timeout: 30000 });
  const be = await page.evaluate('window.__bootError');
  if (be) throw new Error('boot error: ' + be);
  return page;
}
const ev = (page, js) => page.evaluate(js);

// ---------------------------------------------------------------- chat is a local-agent feature of `kerf serve`
await t('static mode: chat is off with a note that it needs `kerf serve` and a local agent CLI; the rest works', async () => {
  const page = await open('sample=truss-bearing-cmu');
  await page.waitForFunction('window.__rendered===true');
  const s = await ev(page, `(() => { const k = window.__kerf; return {
    mode: k.mode, available: k.session.available, disabled: document.querySelector('#composer textarea').disabled,
    intro: document.querySelector('#msgs .intro').textContent, status: document.getElementById('status').textContent,
    comps: k.app.doc.components.length, setup: !!document.getElementById('setupbtn'), attach: !!document.querySelector('#composer input[type=file]') } })()`);
  assert.equal(s.mode, 'static'); assert.equal(s.available, false); assert.equal(s.disabled, true);
  assert.match(s.intro, /CHAT NEEDS `kerf serve` AND A LOCAL AGENT CLI \(claude, codex, grok OR pi\)/);
  assert.match(s.status, /CHAT OFF/);
  assert.ok(s.comps >= 10); assert.equal(s.setup, false); assert.equal(s.attach, false);
  assert.deepEqual(page.errors, []);
  await page.screenshot({ path: path.join(outDir, 'static-chat-off.png') });
  await page.close();
});

// ---------------------------------------------------------------- designer edits: notes, verify, undo, drag
await t('notes: edit text, add + verify citation (designer), op log, undo', async () => {
  const page = await open('sample=truss-bearing-cmu');
  await ev(page, `window.__kerf.app.setSelection('n_truss'); document.querySelectorAll('.itabs .tab')[1].click();`);
  await page.waitForSelector('.idetail textarea');
  await page.$eval('.idetail textarea', (el) => { el.value = 'PRE-ENGINEERED WOOD TRUSS PER MFR. (EDITED)'; el.dispatchEvent(new Event('change', { bubbles: true })); });
  await page.waitForFunction(`window.__kerf.app.opLog.length === 2`);
  let n = await ev(page, `window.__kerf.app.view.annotations.find(a => a.id === 'n_truss')`);
  assert.equal(n.text, 'PRE-ENGINEERED WOOD TRUSS PER MFR. (EDITED)');
  assert.equal(await ev(page, `window.__kerf.app.opLog[1].who`), 'DESIGNER');
  // verify the first citation via the stamp
  await page.waitForSelector('.idetail .stamp');
  assert.match(await page.$eval('.idetail .stamp', (e) => e.textContent), /UNVERIFIED/);
  await page.click('.idetail .stamp');
  await page.waitForFunction(`window.__kerf.app.opLog.length === 3`);
  n = await ev(page, `window.__kerf.app.view.annotations.find(a => a.id === 'n_truss')`);
  assert.equal(n.cite[0].status, 'verified');
  await page.waitForFunction(`document.querySelector('.idetail .stamp.ok')`);
  // add a citation
  await page.type('.idetail input[placeholder="R403.1.6"]', 'R802.10.2');
  await ev(page, `[...document.querySelectorAll('.idetail .btn')].find(b => b.textContent === 'ADD').click()`);
  await page.waitForFunction(`window.__kerf.app.opLog.length === 4`);
  n = await ev(page, `window.__kerf.app.view.annotations.find(a => a.id === 'n_truss')`);
  assert.equal(n.cite.length, 2); assert.equal(n.cite[1].status, 'suggested');
  // DIFF tab + undo last
  await ev(page, `document.querySelectorAll('.itabs .tab')[2].click()`);
  await page.waitForSelector('.logrow');
  assert.equal(await page.$$eval('.logrow', (r) => r.length), 4);
  await ev(page, `[...document.querySelectorAll('.logbar .btn')].find(b => b.textContent === 'UNDO LAST').click()`);
  await page.waitForFunction(`window.__kerf.app.opLog.length === 5`);
  n = await ev(page, `window.__kerf.app.view.annotations.find(a => a.id === 'n_truss')`);
  assert.equal(n.cite.length, 1);
  assert.deepEqual(page.errors, []);
  await page.close();
});

// Issue #8: acknowledged warnings stay visible in the diagnostics panel and the status line.
await t('diagnostics: acknowledged warnings get their own section (code, component, reason) and an ACK count', async () => {
  const page = await open('sample=truss-bearing-cmu&tab=diag');
  await page.waitForFunction('window.__rendered===true');
  await ev(page, `window.__kerf.app.applyOps([{ op: 'update', path: 'components/sill_plate', value: { treated: false, acknowledge: [{ code: 'W_UNTREATED_CONTACT', reason: 'sill sealer by others' }] } }], 'designer', 'ack the sill')`);
  await page.waitForFunction(`window.__kerf.app.acknowledged.length === 1`);
  if (fixture) { await page.close(); return; }
  await ev(page, `[...document.querySelectorAll('.itabs .tab')].find(b => /DIAG/.test(b.textContent)).click()`);
  await page.waitForSelector('.diag.ack');
  const s = await ev(page, `(() => ({
    tab: [...document.querySelectorAll('.itabs .tab')].find(b => /DIAG/.test(b.textContent)).textContent,
    bar: document.querySelector('.ackbar').textContent,
    row: document.querySelector('.diag.ack').textContent,
    tag: document.querySelector('.diag.ack .dtag').textContent,
    iack: [...document.querySelectorAll('.diag .dc')].some(e => /^I_ACK/.test(e.textContent)),
    status: document.getElementById('status').textContent,
    warn: window.__kerf.app.counts().warn,
  }))()`);
  assert.match(s.tab, /DIAG 1\u00b71A/);
  assert.match(s.bar, /1 ACKNOWLEDGED WARNING/);
  assert.match(s.row, /W_UNTREATED_CONTACT · sill_plate/);
  assert.match(s.row, /REASON: sill sealer by others/);
  assert.match(s.row, /untreated wood 'sill_plate'/);
  assert.equal(s.tag, 'A');
  assert.equal(s.iack, false);
  assert.match(s.status, /0 WARN 1 ACK/);
  assert.equal(s.warn, 0);
  await page.click('.diag.ack');
  assert.equal(await ev(page, 'window.__kerf.app.selection?.id'), 'sill_plate');
  assert.deepEqual(page.errors, []);
  await page.screenshot({ path: path.join(outDir, 'diag-acknowledged.png') });
  await page.close();
});

await t('2D: click selects by src; dragging a note issues an update op with place', async () => {
  const page = await open('sample=truss-bearing-cmu');
  await page.waitForFunction('window.__rendered===true');
  await new Promise((r) => setTimeout(r, 400));
  if (!(await ev(page, `!!window.__kerf.vp.vp2.model.bySrc.get('n_plate')`))) { console.log('      (engine drawing has no note n_plate yet: SKIP)'); skipped++; await page.close(); return; }
  const box = await page.$eval('#host2d canvas', (c) => { const r = c.getBoundingClientRect(); return { x: r.x, y: r.y }; });
  // locate note n_plate's first text item on screen
  const pos = await ev(page, `(() => { const vp = window.__kerf.vp.vp2; const g = vp.model.bySrc.get('n_plate'); const b = g.texts[0]; const v = vp.view;
     return { x: v.tx + v.zoom * (b[0] + b[2]) / 2, y: v.ty - v.zoom * (b[1] + b[3]) / 2 }; })()`);
  await page.mouse.click(box.x + pos.x, box.y + pos.y);
  await page.waitForFunction(`window.__kerf.app.selection && window.__kerf.app.selection.id === 'n_plate'`);
  assert.equal(await ev(page, `window.__kerf.app.selection.kind`), 'note');
  const before = await ev(page, `JSON.stringify(window.__kerf.app.view.annotations.find(a => a.id === 'n_plate').place ?? null)`);
  await page.mouse.move(box.x + pos.x, box.y + pos.y);
  await page.mouse.down();
  await page.mouse.move(box.x + pos.x + 30, box.y + pos.y + 40, { steps: 6 });
  await page.mouse.up();
  await page.waitForFunction(`window.__kerf.app.opLog.length === 2`, { timeout: 10000 });
  const after = await ev(page, `window.__kerf.app.view.annotations.find(a => a.id === 'n_plate').place`);
  assert.ok(Array.isArray(after) && after.length === 2, 'place set: ' + JSON.stringify(after) + ' was ' + before);
  assert.equal(await ev(page, `window.__kerf.app.opLog[1].who`), 'DESIGNER');
  assert.match(await ev(page, `window.__kerf.app.opLog[1].why`), /Move note n_plate/);
  // click on empty background deselects
  await page.mouse.click(box.x + 40, box.y + 40);
  await page.waitForFunction(`window.__kerf.app.selection === null`);
  assert.deepEqual(page.errors, []);
  await page.close();
});

await t('save / open .kerf.json round trip', async () => {
  const page = await open('sample=truss-bearing-cmu');
  const before = await ev(page, `window.__kerf.app.doc.id`);
  await ev(page, `[...document.querySelectorAll('#hdr .btns button')].find(b => b.textContent === 'SAVE').click()`);
  const f = path.join(outDir, `${before}.kerf.json`);
  for (let i = 0; i < 50 && !fs.existsSync(f); i++) await new Promise((r) => setTimeout(r, 100));
  assert.ok(fs.existsSync(f), 'saved ' + f);
  await new Promise((r) => setTimeout(r, 200));
  const saved = JSON.parse(fs.readFileSync(f, 'utf8'));
  assert.equal(saved.id, before);
  assert.ok(saved.components.length >= 10);
  // open it back into a fresh page (different doc state)
  const page2 = await open('sample=flush-beam-strap');
  assert.equal(await ev(page2, 'window.__kerf.app.doc.id'), 'flush-beam-strap');
  const input = await page2.$('#hdr input[type=file]');
  await input.uploadFile(f);
  await page2.waitForFunction(`window.__kerf.app.doc.id === '${before}'`, { timeout: 15000 });
  assert.equal(await ev(page2, 'window.__kerf.app.opLog.length'), 2);
  await page.close(); await page2.close();
});

await t('engine loader in a Web Worker: calls, bytes, errors (echo.wasm)', async () => {
  if (!fixture) return; // echo.wasm only ships in the fixture build
  const page = await open('');
  const r = await page.evaluate(async () => {
    const e = await window.__kerf.loadEngine({ url: './fixtures/echo.wasm', worker: true });
    const echo = await e.apply({ a: 1 }, null, [], 'llm'); // 'apply' is unknown to echo -> rc 1
    return echo;
  }).catch((err) => ({ err: String(err) }));
  assert.ok(r.err && /boom/.test(r.err), JSON.stringify(r));
  await page.close();
});

// ---------------------------------------------------------------- exports through the real engine
if (!fixture) {
  const py = process.env.KERF_PYTHON ?? path.join(repo, 'tools/.venv/bin/python');
  for (const [fmt, tool] of [['dxf', 'dxf_check.py'], ['pdf', 'pdf_check.py'], ['svg', null]]) {
    await t(`export ${fmt.toUpperCase()} button downloads <docid>-<view>.${fmt} and validates`, async () => {
      const page = await open('sample=truss-bearing-cmu&view=A');
      await page.waitForFunction('window.__rendered===true');
      const before = new Set(fs.readdirSync(outDir));
      await ev(page, `[...document.querySelectorAll('.iexport .btn')].find(b => b.textContent === '${fmt.toUpperCase()}').click()`);
      const name = `truss-bearing-cmu-A.${fmt}`;
      for (let i = 0; i < 100 && !fs.existsSync(path.join(outDir, name)); i++) {
        await new Promise((r) => setTimeout(r, 100));
        const st = await ev(page, 'document.getElementById("status").textContent');
        if (/EXPORT FAILED.*(UNKNOWN FUNCTION|NOT IMPLEMENTED|UNSUPPORTED EXPORT FORMAT|EXPORT FORMAT MUST BE)/i.test(st)) { console.log(`      (engine lacks ${fmt} export: SKIP)`); skipped++; await page.close(); return; }
      }
      assert.ok(fs.existsSync(path.join(outDir, name)), 'download ' + name + ' missing; have ' + fs.readdirSync(outDir).filter((f) => !before.has(f)));
      await new Promise((r) => setTimeout(r, 300));
      const file = path.join(outDir, name);
      if (tool) {
        const r = spawnSync(py, [path.join(repo, 'tools', tool), file, '--png', path.join(outDir, `${fmt}.png`)], { encoding: 'utf8' });
        assert.equal(r.status, 0, (r.stdout || '') + (r.stderr || ''));
      } else {
        const txt = fs.readFileSync(file, 'utf8');
        assert.match(txt.slice(0, 400), /<svg/);
      }
      assert.match(await ev(page, 'document.getElementById("status").textContent'), new RegExp(`EXPORTED TRUSS-BEARING-CMU-A.${fmt.toUpperCase()}`));
      await page.close();
    });
  }
}

await browser.close();
server.kill();
console.log(`\n${pass} passed, ${fail} failed, ${skipped} skipped (engine function missing). artifacts: ${outDir}`);
process.exit(fail ? 1 : 0);
