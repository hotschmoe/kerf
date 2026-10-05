#!/usr/bin/env node
// End-to-end test of the rust-egui web build with REAL pointer/keyboard input (puppeteer).
// Usage:  node tools/egui/e2e.mjs [http://localhost:8090/] [outdir=apps/egui/shots]
// Needs: the dist served (node tools/serve.mjs apps/egui/dist 8090). The app publishes its observable
// state as JSON in window.__kerf_state, which is what the assertions read.
import puppeteer from 'puppeteer-core';
import path from 'node:path';
import fs from 'node:fs';

const base = process.argv[2] || 'http://localhost:8090/';
const outDir = path.resolve(process.argv[3] || 'apps/egui/shots');
fs.mkdirSync(outDir, { recursive: true });
const W = 1440, H = 860;
const args = ['--no-sandbox', '--disable-dev-shm-usage', '--hide-scrollbars', `--window-size=${W},${H}`, '--ignore-gpu-blocklist',
  '--use-angle=vulkan', '--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--enable-unsafe-swiftshader',
  '--enable-features=Vulkan,WebGPU', '--use-vulkan=swiftshader', '--use-webgpu-adapter=swiftshader'];
const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args });
const page = await browser.newPage();
await page.setViewport({ width: W, height: H });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });

let failed = 0;
const state = async () => JSON.parse(await page.evaluate(() => window.__kerf_state || '{}'));
const until = async (label, pred, ms = 15000) => {
  const t = Date.now();
  for (;;) {
    const s = await state();
    if (pred(s)) return s;
    if (Date.now() - t > ms) { console.log(`FAIL ${label}: timeout; state=${JSON.stringify(s)}`); failed++; return s; }
    await new Promise((r) => setTimeout(r, 100));
  }
};
const check = (label, cond) => { console.log(`${cond ? 'ok  ' : 'FAIL'} ${label}`); if (!cond) failed++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// egui is immediate-mode and repaints on events: move first so hover is established, then press/release with pauses
const click = async (x, y) => {
  await page.mouse.move(x, y); await sleep(150);
  await page.mouse.down(); await sleep(60);
  await page.mouse.up(); await sleep(200);
};
const shot = (name) => page.screenshot({ path: path.join(outDir, name) });

await page.goto(`${base}?demo=1`, { waitUntil: 'load' });
await page.waitForFunction('window.__ready===true', { timeout: 60000 });
let s = await state();
check('boots with no document', s.doc == null && s.components === 0);

// open a sample through the OPEN menu (real clicks)
await click(1346, 20);                          // OPEN
await shot('e2e-0-menu.png');
await click(1200, 80);                          // first sample row
s = await until('sample loaded', (x) => x.doc === 'truss-bearing-cmu');
check('OPEN menu loads the truss sample', s.components === 14);
await shot('e2e-1-loaded.png');

// select a component by clicking the viewport (CMU wall), then a part row in the inspector
await click(745, 470);
s = await until('viewport click selects', (x) => x.selected != null);
check(`click in viewport selects a component (${s.selected})`, s.selected != null);
await click(1200, 224);              // 4th row of the parts table
s = await until('table click selects', (x) => x.selected === 'sill_plate');
check('inspector row click selects sill_plate', s.selected === 'sill_plate');
await shot('e2e-2-selected.png');

// tabs
await click(479, 56);                           // [3D]
s = await until('3d tab', (x) => x.tab === '3d');
check('[3D] tab', s.tab === '3d');
await new Promise((r) => setTimeout(r, 800));
await shot('e2e-3-3d.png');
await click(542, 56);                           // [SHEET]
s = await until('sheet tab', (x) => x.tab === 'sheet');
check('[SHEET] tab', s.tab === 'sheet');
await new Promise((r) => setTimeout(r, 500));
await shot('e2e-4-sheet.png');
await click(386, 56);                           // [A]
await until('section tab', (x) => x.tab === 'view:A');

// chat: type into the input and press Enter (demo transport replays the scripted turn)
await click(180, 720);
await page.keyboard.type('prefab truss bearing on 8 inch CMU', { delay: 20 });
s = await until('typed', (x) => x.input_len >= 'prefab truss bearing on 8 inch CMU'.length);
check('typing reaches the console input', s.input_len === 'prefab truss bearing on 8 inch CMU'.length);
await page.keyboard.press('Enter');
s = await until('chat started', (x) => x.chat_busy === true, 5000);
await until('chat finished', (x) => x.chat_busy === false && x.history >= 8, 30000);
s = await state();
check('tool loop ran (history >= 8 messages)', s.history >= 8);
check('claude ops are in the log', s.log.some((l) => l.startsWith('CLAUDE:')));
await new Promise((r) => setTimeout(r, 500));
await shot('e2e-5-chat.png');

// drag a note in the viewport (press on a note's text, move, release) => designer op
const before = s.log.length;
await page.mouse.move(1000, 297); await sleep(200);
await page.mouse.down(); await sleep(100);
for (let i = 1; i <= 8; i++) { await page.mouse.move(1000 + i * 4, 297 - i * 12); await sleep(60); }
await page.mouse.up(); await sleep(300);
s = await until('designer op', (x) => x.log.length > before, 5000);
check('note drag recorded as a DESIGNER op', s.log.at(-1)?.startsWith('DESIGNER:'));
await shot('e2e-6-dragged.png');

// pasted image arrives via the JS queue and becomes an attachment
const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
await page.evaluate((b64) => window.__kerf_paste.push({ name: 'pasted.png', mime: 'image/png', b64 }), png);
s = await until('paste attach', (x) => x.attachments === 1, 5000);
check('pasted image becomes an attachment', s.attachments === 1);

check('no page errors', errors.length === 0);
if (errors.length) console.log(errors.slice(0, 5).join('\n'));
await browser.close();
console.log(failed ? `\n${failed} FAILED` : '\nALL OK');
process.exit(failed ? 1 : 0);
