// Timings of engine calls in wasm (node). Usage: node scripts/bench.mjs [reps=5]
import fs from 'node:fs';
import { loadKerf } from './kerf-wasm.mjs';
const reps = +(process.argv[2] || 5);
const k = await loadKerf();
const dir = new URL('../../../spec/details/', import.meta.url);
const docs = fs.readdirSync(dir).filter(f => f.endsWith('.kerf.json'));
function time(fn, input) {
  let best = Infinity, r;
  for (let i = 0; i < reps; i++) { const t = performance.now(); r = k.call(fn, input); best = Math.min(best, performance.now() - t); }
  return { ms: best, r };
}
console.log('doc'.padEnd(34), 'call'.padEnd(18), 'ms(best)  bytes');
for (const f of docs) {
  const doc = JSON.parse(fs.readFileSync(new URL(f, dir), 'utf8'));
  const views = doc.views.map(v => v.id);
  const rows = [['check', { doc }], ['mesh', { doc }]];
  for (const v of views) {
    rows.push([`drawing ${v}`, { doc, view: v }]);
    for (const fmt of ['svg', 'dxf', 'pdf']) rows.push([`export ${v} ${fmt}`, { doc, view: v, format: fmt }]);
  }
  for (const [name, input] of rows) {
    const fn = name.split(' ')[0];
    const { ms, r } = time(fn, input);
    console.log(f.replace('.kerf.json', '').padEnd(34), name.padEnd(18), ms.toFixed(1).padStart(7), String(r.bytes.length).padStart(9), r.ok ? '' : 'ERR');
  }
}
