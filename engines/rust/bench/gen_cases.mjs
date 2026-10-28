// Usage: node bench/gen_cases.mjs [outdir]   -> <outdir>/manifest.tsv + one input file per case (default target/bench-cases).
// The case set every harness runs (native Rust, native Zig, wasm of both): every reference detail and test doc,
// per doc check / mesh / inspect / fmt / apply, per view drawing and export svg / sheet svg / dxf / pdf / png, plus the
// calls the web app makes on every edit (apply, mesh, drawing per view), which always pass the style (apps/web style.json).
import fs from 'node:fs';
import path from 'node:path';
const here = path.dirname(new URL(import.meta.url).pathname);
const root = path.resolve(here, '../../..');
const out = path.resolve(process.argv[2] || path.join(here, '../target/bench-cases'));
fs.mkdirSync(out, { recursive: true });
const docs = ['spec/details/truss-bearing-cmu', 'spec/details/flush-beam-strap', 'spec/details/monopour-slab-door-recess',
  'engines/zig/tests/docs/palmer-sd1-like', 'engines/zig/tests/docs/flush-psl-2x6'];
const style = JSON.parse(fs.readFileSync(path.join(root, 'spec/styles/kerf-standard.kerfstyle.json'), 'utf8'));
const rows = [];
const add = (label, fn, input) => {
  const file = `${rows.length.toString().padStart(3, '0')}-${label.replace(/[^a-z0-9.-]+/gi, '_')}.json`;
  fs.writeFileSync(path.join(out, file), JSON.stringify(input));
  rows.push(`${label}\t${fn}\t${file}`);
};
for (const d of docs) {
  const doc = JSON.parse(fs.readFileSync(path.join(root, d + '.kerf.json'), 'utf8'));
  const name = path.basename(d);
  add(`${name} check`, 'check', { doc });
  add(`${name} mesh`, 'mesh', { doc });
  add(`${name} inspect`, 'inspect', { doc, query: { q: 'summary' } });
  add(`${name} fmt`, 'fmt', { doc });
  add(`${name} apply`, 'apply', { doc, ops: [{ op: 'update', path: 'meta', value: { title: 'BENCH' } },
    { op: 'update', path: `components/${doc.components[0].id}`, value: { label: 'BENCH' } }] });
  for (const v of doc.views) {
    add(`${name} ${v.id} drawing`, 'drawing', { doc, view: v.id });
    add(`${name} ${v.id} svg`, 'export', { doc, view: v.id, format: 'svg' });
    add(`${name} ${v.id} sheet-svg`, 'export', { doc, view: v.id, format: 'svg', sheet: true });
    add(`${name} ${v.id} dxf`, 'export', { doc, view: v.id, format: 'dxf' });
    add(`${name} ${v.id} pdf`, 'export', { doc, view: v.id, format: 'pdf' });
    add(`${name} ${v.id} png`, 'export', { doc, view: v.id, format: 'png' });
  }
  add(`${name} web-apply`, 'apply', { doc, style, actor: 'designer', ops: [{ op: 'set', path: 'doc', value: doc }] });
  add(`${name} web-mesh`, 'mesh', { doc, style });
  for (const v of doc.views) add(`${name} ${v.id} web-drawing`, 'drawing', { doc, style, view: v.id });
}
fs.writeFileSync(path.join(out, 'manifest.tsv'), rows.join('\n') + '\n');
console.log(`${rows.length} cases -> ${out}`);
