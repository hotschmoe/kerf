#!/usr/bin/env node
// Drive the web build in headless Chromium (WebGPU via SwiftShader) with a step script and take screenshots.
//   node apps/teak/tools/drive.mjs <url> '<json steps>' [--width 1440 --height 900 --dpr 1]
// steps: {"wait":ms} {"click":[x,y]} {"move":[x,y]} {"drag":[x0,y0,x1,y1]} {"wheel":[x,y,dy]} {"type":"text"}
//        {"key":"Enter"} {"shot":"path.png"} {"eval":"js"} {"log":"text"} {"shotclip":["path.png",x,y,w,h]}
import { createRequire } from 'module';
const require = createRequire(new URL('../../../tools/package.json', import.meta.url));
const puppeteer = require('puppeteer-core');

const argv = process.argv.slice(2);
const opt = (n, d) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : d; };
const url = argv[0];
const steps = JSON.parse(argv[1] || '[]');
const width = Number(opt('--width', 1440)), height = Number(opt('--height', 900));
const args = ['--no-sandbox', '--disable-dev-shm-usage', '--hide-scrollbars', `--window-size=${width},${height}`, '--ignore-gpu-blocklist',
  '--use-angle=vulkan', '--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--enable-unsafe-swiftshader',
  '--enable-features=Vulkan,WebGPU', '--use-vulkan=swiftshader', '--use-webgpu-adapter=swiftshader'];
const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args, protocolTimeout: 120000 });
let failed = false;
try {
  const page = await browser.newPage();
  await page.setViewport({ width, height, deviceScaleFactor: Number(opt('--dpr', 1)) });
  page.on('console', (m) => console.log(`[console.${m.type()}] ${m.text()}`));
  page.on('pageerror', (e) => { failed = true; console.log(`[pageerror] ${e.stack || e.message || e}`); });
  page.on('requestfailed', (r) => console.log(`[requestfailed] ${r.url()} ${r.failure()?.errorText}`));
  if (opt('--downloads')) { const c = await page.createCDPSession(); await c.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: opt('--downloads') }); }
  await page.goto(url, { waitUntil: 'load', timeout: 60000 });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  for (const s of steps) {
    if (s.wait !== undefined) await sleep(s.wait);
    else if (s.click) { await page.mouse.move(...s.click); await sleep(60); await page.mouse.move(s.click[0] + 1, s.click[1]); await sleep(60); await page.mouse.down(); await sleep(60); await page.mouse.up(); await sleep(120); }
    else if (s.clickfile) { const [x, y, f] = s.clickfile; const chooser = page.waitForFileChooser({ timeout: 8000 }); await page.mouse.move(x, y); await sleep(60); await page.mouse.move(x + 1, y); await sleep(60); await page.mouse.down(); await sleep(60); await page.mouse.up(); try { const fc = await chooser; await fc.accept([f]); console.log('[chooser] accepted', f); } catch (e) { console.log('[chooser] none', e.message); } await sleep(800); }
    else if (s.move) { await page.mouse.move(...s.move); await sleep(60); await page.mouse.move(s.move[0] + 1, s.move[1] + 1, { steps: 2 }); await sleep(100); }
    else if (s.drag) { const [x0, y0, x1, y1] = s.drag; await page.mouse.move(x0, y0); await sleep(60); await page.mouse.down(); await sleep(60); await page.mouse.move(x1, y1, { steps: 12 }); await sleep(80); await page.mouse.up(); await sleep(150); }
    else if (s.wheel) { await page.mouse.move(s.wheel[0], s.wheel[1]); await sleep(60); await page.mouse.wheel({ deltaY: s.wheel[2] }); await sleep(150); }
    else if (s.type) { await page.keyboard.type(s.type, { delay: 15 }); await sleep(100); }
    else if (s.key) { await page.keyboard.press(s.key); await sleep(120); }
    else if (s.eval) { console.log('[eval]', JSON.stringify(await page.evaluate(s.eval))); }
    else if (s.log) console.log('[log]', s.log);
    else if (s.shot) { await page.screenshot({ path: s.shot }); console.log('[shot]', s.shot); }
    else if (s.shotclip) { const [p, x, y, w, h] = s.shotclip; await page.screenshot({ path: p, clip: { x, y, width: w, height: h } }); console.log('[shot]', p); }
  }
} finally { await browser.close(); }
process.exit(failed ? 1 : 0);
