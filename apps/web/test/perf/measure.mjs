// Measures one stack: usage  node test/perf/measure.mjs <rust|zig> [--runs 5]
// Output: JSON on stdout + a markdown table on stderr. Requires apps/web/dist-<engine>.
import puppeteer from 'puppeteer-core';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const repo = path.resolve(web, '../..');
const engine = process.argv[2] || 'rust';
const runs = Number(process.argv[process.argv.indexOf('--runs') + 1] || 5);
const dist = path.join(web, `dist-${engine}`);
const port = 8800 + Math.floor(Math.random() * 100);
const server = spawn('node', [path.join(repo, 'tools/serve.mjs'), dist, String(port)], { stdio: 'ignore' });
await new Promise((r) => setTimeout(r, 600));
const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args: ['--no-sandbox', '--use-angle=vulkan', '--ignore-gpu-blocklist'] });
const samples = ['truss-bearing-cmu', 'monopour-slab-door-recess', 'flush-beam-strap'];
const med = (a) => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
const out = { engine, runs, sizes: {}, startup: {}, calls: {} };

// sizes (dist JS/CSS/wasm/fonts)
const files = [];
(function walk(d) { for (const f of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, f.name); if (f.isDirectory()) walk(p); else files.push(p); } })(dist);
const sizeOf = (re) => files.filter((f) => re.test(f));
const rep = (list) => { const r = spawnSync(path.join(repo, 'tools/size_report.sh'), list, { encoding: 'utf8' }).stdout.trim().split('\n'); const t = r[r.length - 1].trim().split(/\s+/); return { raw: +t[1], gzip: +t[2], brotli: +t[3] }; };
const initialJs = files.filter((f) => /assets\/index-.*\.js$/.test(f) || /assets\/engine\.worker.*\.js$/.test(f));
out.sizes = {
  initialJs: rep(initialJs.length > 1 ? initialJs : [...initialJs, ...initialJs]).raw ? rep(initialJs) : null,
  css: rep(sizeOf(/\.css$/)), fonts: rep(sizeOf(/\.woff2$/)), wasm: rep(sizeOf(/kerf\.wasm$/)),
  lazy3d: rep(sizeOf(/view3d.*\.js$/)), lazySdk: rep(sizeOf(/sdk.*\.js$/)),
  all: rep(files.filter((f) => /\.(js|css|woff2|wasm)$/.test(f))),
};
out.sizes.files = Object.fromEntries(files.filter((f) => /assets\/.*\.js$/.test(f)).map((f) => [path.basename(f), fs.statSync(f).size]));

for (const worker of [1, 0]) {
  const key = worker ? 'worker' : 'main';
  const acc = {};
  for (const sample of samples) {
    const firsts = [], ready = [], loads = [], uiready = [];
    for (let i = 0; i < runs; i++) {
      const page = await browser.newPage();
      await page.setViewport({ width: 1440, height: 900 });
      await page.goto(`http://localhost:${port}/?sample=${sample}&worker=${worker}`, { waitUntil: 'load' });
      await page.waitForFunction('window.__rendered===true', { timeout: 60000 });
      const m = await page.evaluate(() => { const a = window.__kerf.app; return { first: a.perf.firstRender, ready: a.perf.engineReady, ui: a.perf.uiReady, load: window.__kerf.engine.loadMs, wasm: window.__kerf.engine.wasmBytes }; });
      firsts.push(m.first); ready.push(m.ready); loads.push(m.load); uiready.push(m.ui);
      if (i === runs - 1) {
        // engine call timings on the warm page: repeat each call 10x
        const calls = await page.evaluate(async () => {
          const k = window.__kerf, a = k.app, e = k.engine, doc = a.doc, st = a.style, v = a.activeView;
          const time = async (fn, n = 10) => { try { const ts = []; for (let i = 0; i < n; i++) { const t0 = performance.now(); await fn(); ts.push(performance.now() - t0); } ts.sort((x, y) => x - y); return ts[Math.floor(ts.length / 2)]; } catch (e) { return null; } };
          const res = {};
          res.apply_set = await time(() => e.apply(a.emptyDoc(), st, [{ op: 'set', path: 'doc', value: doc }], 'designer'));
          const note = a.view.annotations.find((x) => x.type === 'note');
          res.apply_update = await time(() => e.apply(doc, st, [{ op: 'update', path: `views/${v}/annotations/${note.id}`, value: { text: note.text + ' ' } }], 'designer'));
          res.check = await time(() => e.check(doc, st));
          res.drawing = await time(() => e.drawing(doc, st, v));
          res.mesh = await time(() => e.mesh(doc, st));
          res.inspect_summary = await time(() => e.inspect(doc, st, { q: 'summary' }));
          res.export_svg = await time(() => e.exportBytes(doc, st, v, 'svg', false), 5);
          res.export_svg_sheet = await time(() => e.exportBytes(doc, st, v, 'svg', true), 5);
          res.export_dxf = await time(() => e.exportBytes(doc, st, v, 'dxf', false), 5);
          res.export_pdf = await time(() => e.exportBytes(doc, st, v, 'pdf', true), 5);
          // edit -> repaint: apply designer op, fetch the new drawing, rebuild model, paint
          const t0 = performance.now();
          await a.applyOps([{ op: 'update', path: `views/${v}/annotations/${note.id}`, value: { text: note.text + ' X' } }], 'designer', 'perf');
          const m = await a.getDrawingModel(v);
          k.vp.vp2.setModel(m, { keepView: true });
          await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
          res.edit_to_repaint = performance.now() - t0;
          res.paint_js = k.vp.vp2.lastPaintMs;
          res.drawing_items = m.drawing.items.length;
          res.drawing_json_bytes = JSON.stringify(m.drawing).length;
          return res;
        });
        acc[sample] = { ...calls, wasmBytes: m.wasm };
      }
      await page.close();
    }
    (out.startup[key] ??= {})[sample] = { ttfr_ms: med(firsts), engine_ready_ms: med(ready), wasm_compile_ms: med(loads), ui_ready_ms: med(uiready) };
  }
  out.calls[key] = acc;
}
await browser.close(); server.kill();
console.log(JSON.stringify(out, null, 1));
const f = (n) => (n === undefined || n === null ? 'n/a' : n.toFixed(n < 10 ? 2 : 1));
console.error(`\n## ${engine} (median of ${runs} loads; calls median of 10)`);
for (const key of ['worker', 'main']) {
  console.error(`\n### engine on ${key === 'worker' ? 'Web Worker' : 'main thread'}\n| sample | TTFR ms | engine ready | wasm compile | ui ready |\n|---|---|---|---|---|`);
  for (const s of samples) { const x = out.startup[key][s]; console.error(`| ${s} | ${f(x.ttfr_ms)} | ${f(x.engine_ready_ms)} | ${f(x.wasm_compile_ms)} | ${f(x.ui_ready_ms)} |`); }
  console.error(`\n| sample | apply set | apply update | check | drawing | mesh | inspect | svg | svg sheet | dxf | pdf | edit->repaint | paint(js) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|`);
  for (const s of samples) { const c = out.calls[key][s]; console.error(`| ${s} | ${[c.apply_set, c.apply_update, c.check, c.drawing, c.mesh, c.inspect_summary, c.export_svg, c.export_svg_sheet, c.export_dxf, c.export_pdf, c.edit_to_repaint, c.paint_js].map(f).join(' | ')} |`); }
}
