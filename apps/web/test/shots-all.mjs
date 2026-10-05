// Screenshots every view of every reference detail with one stack: node test/shots-all.mjs <rust|zig|fixture> [outdir]
import puppeteer from 'puppeteer-core';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(web, '../..');
const engine = process.argv[2] || 'rust';
const outDir = path.resolve(process.argv[3] || path.join(web, 'test/shots'));
fs.mkdirSync(outDir, { recursive: true });
const port = 8900 + Math.floor(Math.random() * 90);
const server = spawn('node', [path.join(repo, 'tools/serve.mjs'), path.join(web, `dist-${engine}`), String(port)], { stdio: 'ignore' });
await new Promise((r) => setTimeout(r, 600));
const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args: ['--no-sandbox', '--use-angle=vulkan', '--ignore-gpu-blocklist'] });
const samples = fs.readdirSync(path.join(repo, 'spec/details')).filter((f) => f.endsWith('.kerf.json')).map((f) => f.replace('.kerf.json', ''));
for (const sample of samples) {
  const doc = JSON.parse(fs.readFileSync(path.join(repo, 'spec/details', sample + '.kerf.json'), 'utf8'));
  const jobs = [];
  for (const v of doc.views) jobs.push({ view: v.id, mode: 'view', name: `${v.kind}-${v.id}` });
  jobs.push({ view: doc.views[0].id, mode: '3d', name: '3d' }, { view: doc.views[0].id, mode: 'sheet', name: 'sheet' });
  for (const j of jobs) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1440, height: 900 });
    page.on('pageerror', (e) => console.log('[pageerror]', sample, j.name, e.message));
    await page.goto(`http://localhost:${port}/?sample=${sample}&view=${j.view}&mode=${j.mode}`, { waitUntil: 'load' });
    const flag = j.mode === '3d' ? '__rendered3d' : j.mode === 'sheet' ? '__renderedSheet' : '__rendered';
    await page.waitForFunction(`window.${flag}===true || document.querySelector('.vmsg.err')`, { timeout: 60000 }).catch(() => console.log('[timeout]', sample, j.name));
    await new Promise((r) => setTimeout(r, 500));
    const file = path.join(outDir, `${engine}-${sample}-${j.name}.png`);
    await page.screenshot({ path: file });
    console.log('wrote', file);
    await page.close();
  }
}
await browser.close(); server.kill();
