// End-to-end tests (puppeteer-core + system chromium). usage: node test/e2e.mjs [rust|zig|fixture] [--keep]
// Needs apps/web/dist-<engine> (npm run build:<engine>). Exports are validated with tools/dxf_check.py / pdf_check.py.
import puppeteer from 'puppeteer-core';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { startFake } from './fake-anthropic.mjs';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(web, '../..');
const engine = process.argv[2] && !process.argv[2].startsWith('--') ? process.argv[2] : 'rust';
const dist = path.join(web, `dist-${engine}`);
const port = 8300 + Math.floor(Math.random() * 500);
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-e2e-'));
const sample = JSON.parse(fs.readFileSync(path.join(repo, 'spec/details/truss-bearing-cmu.kerf.json'), 'utf8'));

const server = spawn('node', [path.join(repo, 'tools/serve.mjs'), dist, String(port)], { stdio: ['ignore', fs.openSync(path.join(outDir, 'serve.log'), 'a'), fs.openSync(path.join(outDir, 'serve.err'), 'a')] });
await new Promise((r) => setTimeout(r, 600));
const browser = await puppeteer.launch({
  executablePath: '/usr/bin/chromium', headless: 'new', protocolTimeout: 120000,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--window-size=1440,900'],
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
const waitIdle = (page, timeout = 60000) => page.waitForFunction('(()=>{const k=window.__kerf;return k&&!k.harness.busy&&k.app.doc})()', { timeout });
const ev = (page, js) => page.evaluate(js);

// ---------------------------------------------------------------- demo chat end to end
await t('demo mode: scripted Claude builds the detail (apply -> render -> report)', async () => {
  const page = await open('demo=1&fast=1');
  await waitIdle(page);
  const s = await ev(page, `(() => { const k = window.__kerf; return {
    comps: k.app.doc.components.length, views: k.app.doc.views.length, log: k.app.opLog.map(e => e.who + ':' + e.why),
    tools: [...document.querySelectorAll('.tl summary .tt')].map(e => e.textContent),
    status: document.getElementById('status').textContent, claude: k.app.claude.state,
    thumb: document.querySelectorAll('.tl img.thumb').length, msgs: document.querySelectorAll('#msgs .msg').length } })()`);
  assert.ok(s.comps >= 10, 'components ' + s.comps);
  assert.ok(s.log[0].startsWith('CLAUDE:'), s.log.join('|'));
  assert.deepEqual(s.tools.map((x) => x.split(/\s+/)[0]), ['APPLY', 'RENDER']);
  assert.equal(s.thumb, 1, 'render thumbnail');
  assert.match(s.status, /CLAUDE OK/);
  assert.equal(s.msgs, 2);
  const rendered = await ev(page, 'window.__rendered===true');
  assert.ok(rendered, 'drawing rendered');
  assert.deepEqual(page.errors, []);
  // the tool_result history the mock saw is well formed
  const h = await ev(page, 'window.__kerf.harness.messages.map(m => m.role + ":" + m.content.map(b => b.type).join("+"))');
  assert.deepEqual(h, ['user:text', 'assistant:text+tool_use', 'user:tool_result', 'assistant:text+tool_use', 'user:tool_result', 'assistant:text']);
  // render tool result carries a real PNG
  const png = await ev(page, `(() => { const m = window.__kerf.harness.messages[4].content[0]; const c = m.content[0]; return { t: c.type, mt: c.source.media_type, head: atob(c.source.data.slice(0, 12)).slice(1, 4), len: c.source.data.length, cap: m.content[1].text } })()`);
  assert.equal(png.mt, 'image/png'); assert.equal(png.head, 'PNG'); assert.ok(png.len > 20000, 'png size ' + png.len);
  console.log('      render caption: ' + png.cap);
  await page.screenshot({ path: path.join(outDir, 'demo.png') });
  await page.close();
});

// ---------------------------------------------------------------- designer edits: notes, verify, undo, drag
await t('notes: edit text, add + verify citation (designer), op log, undo', async () => {
  const page = await open('demo=1&auto=0&sample=truss-bearing-cmu');
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
  // the next LLM turn is told about it
  const pend = await ev(page, 'window.__kerf.app.pendingDesignerEdits');
  assert.ok(pend.some((p) => /Verify IRC/.test(p)), pend.join('|'));
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

await t('2D: click selects by src; dragging a note issues an update op with place', async () => {
  const page = await open('demo=1&auto=0&sample=truss-bearing-cmu');
  await page.waitForFunction('window.__rendered===true');
  await new Promise((r) => setTimeout(r, 400));
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

await t('image attach: paste/drop/file -> downscaled to <=1568 px and sent before the text block', async () => {
  const page = await open('demo=1&auto=0&fast=1');
  const info = await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 3200; c.height = 1800;
    const g = c.getContext('2d'); g.fillStyle = '#336'; g.fillRect(0, 0, 3200, 1800); g.fillStyle = '#fff'; g.fillRect(100, 100, 800, 400);
    const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
    const input = document.querySelector('#composer input[type=file]');
    const dt = new DataTransfer(); dt.items.add(new File([blob], 'shot.png', { type: 'image/png' }));
    input.files = dt.files; input.dispatchEvent(new Event('change', { bubbles: true }));
    await new Promise((r) => setTimeout(r, 500));
    const attached = document.querySelectorAll('#attached .att').length;
    // paste path
    const dt2 = new DataTransfer(); dt2.items.add(new File([blob], 'p.png', { type: 'image/png' }));
    const ev = new ClipboardEvent('paste', { clipboardData: dt2, bubbles: true, cancelable: true });
    document.querySelector('#composer textarea').dispatchEvent(ev);
    await new Promise((r) => setTimeout(r, 500));
    return { attached, attached2: document.querySelectorAll('#attached .att').length };
  });
  assert.equal(info.attached, 1); assert.equal(info.attached2, 2);
  await ev(page, `window.__kerf.con.sendText('recreate this detail')`);
  await waitIdle(page);
  const m = await ev(page, `(() => { const u = window.__kerf.harness.messages[0].content; return u.map(b => b.type === 'image' ? { t: 'image', mt: b.source.media_type, len: b.source.data.length } : { t: b.type }) })()`);
  assert.deepEqual(m.map((x) => x.t), ['image', 'image', 'text']);
  const dim = await page.evaluate(async (b64) => { const i = new Image(); i.src = 'data:image/png;base64,' + b64; await i.decode(); return [i.naturalWidth, i.naturalHeight]; }, await ev(page, `window.__kerf.harness.messages[0].content[0].source.data`));
  assert.deepEqual(dim, [1568, 882]);
  assert.equal(await page.$$eval('#msgs .msg .atts img', (a) => a.length), 2);
  await page.close();
});

await t('save / open .kerf.json round trip', async () => {
  const page = await open('demo=1&auto=0&sample=truss-bearing-cmu');
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
  const page2 = await open('demo=1&auto=0&sample=flush-beam-strap');
  assert.equal(await ev(page2, 'window.__kerf.app.doc.id'), 'flush-beam-strap');
  const input = await page2.$('#hdr input[type=file]');
  await input.uploadFile(f);
  await page2.waitForFunction(`window.__kerf.app.doc.id === '${before}'`, { timeout: 15000 });
  assert.equal(await ev(page2, 'window.__kerf.app.opLog.length'), 2);
  assert.match(await ev(page2, 'window.__kerf.app.pendingDesignerEdits[1]'), /OPENED truss-bearing-cmu/);
  await page.close(); await page2.close();
});

await t('engine loader in a Web Worker: calls, bytes, errors (echo.wasm)', async () => {
  if (engine !== 'fixture') return; // echo.wasm only ships in the fixture build
  const page = await open('demo=1&auto=0');
  const r = await page.evaluate(async () => {
    const e = await window.__kerf.loadEngine({ url: './fixtures/echo.wasm', worker: true });
    const echo = await e.apply({ a: 1 }, null, [], 'llm'); // 'apply' is unknown to echo -> rc 1
    return echo;
  }).catch((err) => ({ err: String(err) }));
  assert.ok(r.err && /boom/.test(r.err), JSON.stringify(r));
  await page.close();
});

// ---------------------------------------------------------------- real SDK path against a local fake Messages API
await t('real SDK transport: headers, body shape, streaming, tool loop, 401 / refusal / 529 retry', async () => {
  const fake = await startFake(0, sample);
  try {
    const base = `api=${encodeURIComponent('http://127.0.0.1:' + fake.port)}&fast=1&sample=`;
    const run = async (key, text) => {
      const page = await open(base);
      await page.evaluate((k) => { localStorage.setItem('kerf.apiKey', k); localStorage.setItem('kerf.model', 'claude-sonnet-5-5'); }, key);
      await page.reload({ waitUntil: 'load' });
      await page.waitForFunction('window.__ready===true');
      await ev(page, `window.__kerf.con.sendText(${JSON.stringify(text)})`);
      return page;
    };
    let page = await run('sk-test-ok', 'build me a truss bearing detail');
    await waitIdle(page);
    assert.ok((await ev(page, 'window.__kerf.app.doc.components.length')) >= 10);
    const reqs = fake.requests.filter((r) => r.key === 'sk-test-ok');
    assert.equal(reqs.length, 3);
    const r0 = reqs[0];
    assert.equal(r0.headers['anthropic-version'], '2023-06-01');
    assert.equal(r0.headers['anthropic-dangerous-direct-browser-access'], 'true');
    assert.equal(r0.headers['anthropic-beta'], 'server-side-fallback-2026-07-01');
    assert.equal(r0.body.model, 'claude-sonnet-5-5');
    assert.equal(r0.body.max_tokens, 32000);
    assert.deepEqual(r0.body.thinking, { type: 'adaptive' });
    assert.deepEqual(r0.body.output_config, { effort: 'high' });
    assert.equal(r0.body.fallbacks, 'default');
    assert.equal(r0.body.stream, true);
    assert.equal(r0.body.system[0].cache_control.type, 'ephemeral');
    assert.match(r0.body.system[0].text, /drafting engine operator inside \*\*Kerf\*\*/);
    assert.match(r0.body.system[0].text, /# Component catalog/);
    assert.deepEqual(r0.body.tools.map((x) => x.name), ['kerf_apply', 'kerf_inspect', 'kerf_render']);
    for (const bad of ['temperature', 'top_p', 'tool_choice', 'budget_tokens']) assert.ok(!(bad in r0.body), bad + ' must not be sent');
    // third request carries the full history: assistant content verbatim + both tool results in single user messages
    const m3 = reqs[2].body.messages;
    assert.deepEqual(m3.map((m) => m.role), ['user', 'assistant', 'user', 'assistant', 'user']);
    assert.equal(m3[2].content[0].type, 'tool_result');
    assert.equal(m3[4].content[0].content[0].type, 'image');
    assert.ok(!JSON.stringify(fake.requests.map((r) => r.body)).includes('sk-test-ok'), 'key is not in request bodies');
    assert.deepEqual(page.errors.filter((e) => !/ResizeObserver/.test(e)), []);
    await page.close();
    // 401
    page = await run('sk-test-401', 'hello');
    await page.waitForFunction(`[...document.querySelectorAll('.notice')].some(n => /INVALID API KEY/.test(n.textContent))`, { timeout: 20000 });
    assert.match(await ev(page, 'document.getElementById("status").textContent'), /NO KEY/);
    await page.close();
    // refusal
    page = await run('sk-test-refuse', 'hello');
    await page.waitForFunction(`[...document.querySelectorAll('.notice')].some(n => /Declined by the fake server/.test(n.textContent))`, { timeout: 20000 });
    await page.close();
    // 529 -> retry after 2 s
    page = await run('sk-test-529', 'build');
    await page.waitForFunction(`[...document.querySelectorAll('.notice')].some(n => /529/.test(n.textContent))`, { timeout: 20000 });
    await waitIdle(page, 30000);
    await page.close();
  } finally { fake.close(); }
});

// ---------------------------------------------------------------- exports through the real engine
if (engine !== 'fixture') {
  const py = path.join(repo, 'tools/.venv/bin/python');
  for (const [fmt, tool] of [['dxf', 'dxf_check.py'], ['pdf', 'pdf_check.py'], ['svg', null]]) {
    await t(`export ${fmt.toUpperCase()} button downloads <docid>-<view>.${fmt} and validates`, async () => {
      const page = await open('demo=1&auto=0&sample=truss-bearing-cmu&view=A');
      await page.waitForFunction('window.__rendered===true');
      const before = new Set(fs.readdirSync(outDir));
      await ev(page, `[...document.querySelectorAll('.iexport .btn')].find(b => b.textContent === '${fmt.toUpperCase()}').click()`);
      const name = `truss-bearing-cmu-A.${fmt}`;
      for (let i = 0; i < 100 && !fs.existsSync(path.join(outDir, name)); i++) {
        await new Promise((r) => setTimeout(r, 100));
        const st = await ev(page, 'document.getElementById("status").textContent');
        if (/EXPORT FAILED.*(UNKNOWN FUNCTION|NOT IMPLEMENTED|UNSUPPORTED EXPORT FORMAT)/i.test(st)) { console.log(`      (engine lacks ${fmt} export: SKIP)`); skipped++; await page.close(); return; }
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
