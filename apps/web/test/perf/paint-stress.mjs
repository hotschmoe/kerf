import puppeteer from 'puppeteer-core';
const b = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: 'new', args: ['--no-sandbox', '--use-angle=vulkan'] });
const p = await b.newPage(); await p.setViewport({ width: 1440, height: 900 });
await p.goto('http://localhost:8201/?sample=truss-bearing-cmu'); await p.waitForFunction('window.__rendered===true');
const r = await p.evaluate(async () => {
  const vp = window.__kerf.vp.vp2; const base = vp.model.drawing;
  const items = [...base.items];
  for (let k = 0; k < 400; k++) { // many hatch lines and paths
    const lines = []; for (let i = 0; i < 100; i++) lines.push([k * 0.5 - 100 + i * 0.01, -30, k * 0.5 - 90 + i * 0.01, 10]);
    items.push({ t: 'hatch', pen: 'hatch', src: 'big' + k, pattern: 'x', loops: [[[0, 0], [1, 0], [1, 1]]], lines });
    items.push({ t: 'text', pen: 'anno', src: 'tx' + k, s: 'TEXT NOTE NUMBER ' + k, x: k % 20, y: -k % 40, h: 0.75 });
  }
  const t0 = performance.now(); vp.setDrawing({ ...base, items }); const build = performance.now() - t0;
  await new Promise(r => requestAnimationFrame(r)); await new Promise(r => requestAnimationFrame(r));
  const times = [];
  for (let i = 0; i < 20; i++) { vp.view = { ...vp.view, tx: vp.view.tx + 5 }; vp.requestRender(); await new Promise(r => requestAnimationFrame(r)); times.push(vp.lastPaintMs); }
  return { items: items.length, build, paintAvg: times.reduce((a, b) => a + b) / times.length, paintMax: Math.max(...times) };
});
console.log(JSON.stringify(r));
await b.close();
