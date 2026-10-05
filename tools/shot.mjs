#!/usr/bin/env node
// Headless chromium screenshot with console/pageerror capture.
// node tools/shot.mjs <url> <out.png> [--wait-ms N] [--width W --height H] [--webgpu] [--webgpu-sw] [--no-gpu]
//   [--wait-for "<js expr truthy>"] [--eval "<js expr>"] [--timeout-ms N] [--full-page] [--dpr N] [--args "--flag1 --flag2"]
// Exit code: 0 ok, 1 page errors (uncaught exceptions) or navigation failure, 2 usage.
import puppeteer from 'puppeteer-core';

const argv = process.argv.slice(2);
const opt = (n, d) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : d; };
const has = (n) => argv.includes(n);
const valueFlags = ['--wait-ms','--width','--height','--wait-for','--eval','--timeout-ms','--dpr','--args'];
const pos = argv.filter((a, i) => !a.startsWith('--') && !valueFlags.includes(argv[i - 1]));
if (pos.length < 2) { console.error('usage: shot.mjs <url> <out.png> [--wait-ms N] [--width W --height H] [--webgpu|--webgpu-sw] [--wait-for expr] [--eval expr] [--full-page]'); process.exit(2); }
const [url, out] = pos;
const waitMs = Number(opt('--wait-ms', 1000));
const width = Number(opt('--width', 1280)), height = Number(opt('--height', 800));
const timeout = Number(opt('--timeout-ms', 30000));

const args = ['--no-sandbox', '--disable-dev-shm-usage', '--hide-scrollbars', `--window-size=${width},${height}`, '--ignore-gpu-blocklist'];
// WebGL2 needs ANGLE->Vulkan here (hardware Mali-G720); without it WebGL2 is unavailable in headless. --no-gpu opts out.
if (!has('--no-gpu')) args.push('--use-angle=vulkan');
if (has('--webgpu') || has('--webgpu-sw')) {
  // Verified recipe (aarch64 Mali-G720, Chromium 154): WebGPU runs on SwiftShader (CPU Vulkan); Dawn does not pick Mali.
  // ALL of these are needed: without --enable-unsafe-swiftshader the adapter is null or the canvas screenshots blank/black.
  // WebGL2 stays on hardware Mali via --use-angle=vulkan (added above).
  args.push('--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--enable-unsafe-swiftshader',
            '--enable-features=Vulkan,WebGPU', '--use-vulkan=swiftshader', '--use-webgpu-adapter=swiftshader');
}
if (opt('--args')) args.push(...opt('--args').split(/\s+/).filter(Boolean));

const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args, protocolTimeout: timeout + 30000 });
let failed = false;
try {
  const page = await browser.newPage();
  await page.setViewport({ width, height, deviceScaleFactor: Number(opt('--dpr', 1)) });
  page.on('console', async (m) => {
    let text = m.text();
    try { // expand JSHandle args (e.g. console.log(obj))
      const parts = await Promise.all(m.args().map((a) => a.jsonValue().catch(() => null)));
      if (parts.length && parts.every((x) => x !== null)) text = parts.map((x) => typeof x === 'string' ? x : JSON.stringify(x)).join(' ');
    } catch {}
    console.log(`[console.${m.type()}] ${text}`);
  });
  page.on('pageerror', (e) => { failed = true; console.log(`[pageerror] ${e.stack || e.message || e}`); });
  page.on('requestfailed', (r) => console.log(`[requestfailed] ${r.url()} ${r.failure()?.errorText}`));
  page.on('response', (r) => { if (r.status() >= 400) console.log(`[http ${r.status()}] ${r.url()}`); });
  try { await page.goto(url, { waitUntil: 'load', timeout }); }
  catch (e) { failed = true; console.log(`[navigation-error] ${e.message}`); }
  if (opt('--wait-for')) {
    try { await page.waitForFunction(opt('--wait-for'), { timeout }); }
    catch (e) { failed = true; console.log(`[wait-for-timeout] ${opt('--wait-for')}`); }
  }
  if (waitMs > 0) await new Promise((r) => setTimeout(r, waitMs));
  if (opt('--eval')) {
    try { console.log('[eval]', JSON.stringify(await page.evaluate(opt('--eval')))); }
    catch (e) { console.log(`[eval-error] ${e.message}`); }
  }
  await page.screenshot({ path: out, fullPage: has('--full-page') });
  console.log(`[shot] wrote ${out} (${width}x${height})`);
} finally {
  await browser.close();
}
process.exit(failed ? 1 : 0);
