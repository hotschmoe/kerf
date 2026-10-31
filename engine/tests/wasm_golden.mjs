// Usage: node tests/wasm_golden.mjs [kerf.wasm]   (default dist/kerf.wasm)  -- wasm output must equal the native goldens byte for byte.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const wasm = process.argv[2] ?? path.join(here, '../dist/kerf.wasm');
const inst = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasm)), {});
const ex = inst.exports, enc = new TextEncoder();
function call(fn, input) {
  const f = enc.encode(fn), i = enc.encode(input);
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f); new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  ex.kerf_call(fp, f.length, ip, i.length);
  const out = Buffer.from(new Uint8Array(ex.memory.buffer, ex.kerf_out_ptr(), ex.kerf_out_len()));
  ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length); return out;
}
let bad = 0, n = 0;
for (const d of ['truss-bearing-cmu', 'monopour-slab-door-recess', 'flush-beam-strap', 'stud-wall-stem-footing', 'hss-column-base-plate']) {
  const doc = JSON.parse(fs.readFileSync(path.join(here, `../../spec/details/${d}.kerf.json`), 'utf8'));
  for (const v of ['A', 'B']) {
    for (const [fmt, file] of [['svg', `${v}.svg`], ['dxf', `${v}.dxf`], ['pdf', `${v}.pdf`]]) {
      const out = call('export', JSON.stringify({ doc, view: v, format: fmt, sheet: false }));
      const gold = fs.readFileSync(path.join(here, `golden/${d}/${file}`));
      n++; if (!out.equals(gold)) { bad++; console.log('DIFF', d, file); }
    }
    const out = call('drawing', JSON.stringify({ doc, view: v }));
    n++; if (!out.equals(fs.readFileSync(path.join(here, `golden/${d}/drawing-${v}.json`)))) { bad++; console.log('DIFF', d, 'drawing', v); }
  }
}
// Issue #8: scenario membership (views/only/omit globs) and the acknowledged list + review overlay.
{
  const d = 'scenarios-shared-origin';
  const doc = JSON.parse(fs.readFileSync(path.join(here, `docs/${d}.kerf.json`), 'utf8'));
  for (const v of ['A', 'B', 'C']) {
    const out = call('drawing', JSON.stringify({ doc, view: v }));
    n++; if (!out.equals(fs.readFileSync(path.join(here, `golden/${d}/drawing-${v}.json`)))) { bad++; console.log('DIFF', d, 'drawing', v); }
    const rev = call('export', JSON.stringify({ doc, view: v, format: 'svg', sheet: true, review: true }));
    n++; if (!rev.equals(fs.readFileSync(path.join(here, `golden/${d}/${v}-review.svg`)))) { bad++; console.log('DIFF', d, `${v}-review.svg`); }
  }
}
// Issue #9: plan and elevation views, rebar plane xz, shown hidden / where_occurs.
for (const [d, views] of [['plan-corner-bars', ['A', 'B', 'C', 'E']], ['elevation-stud-strap', ['A', 'P']]]) {
  const doc = JSON.parse(fs.readFileSync(path.join(here, `docs/${d}.kerf.json`), 'utf8'));
  for (const v of views) {
    const out = call('drawing', JSON.stringify({ doc, view: v }));
    n++; if (!out.equals(fs.readFileSync(path.join(here, `golden/${d}/drawing-${v}.json`)))) { bad++; console.log('DIFF', d, 'drawing', v); }
    const svg = call('export', JSON.stringify({ doc, view: v, format: 'svg', sheet: true }));
    n++; if (!svg.equals(fs.readFileSync(path.join(here, `golden/${d}/${v}-sheet.svg`)))) { bad++; console.log('DIFF', d, `${v}-sheet.svg`); }
  }
}
console.log(`wasm vs native goldens: ${n - bad}/${n} byte-identical`);
if (bad) process.exitCode = 1; // a CI gate: a mismatch must fail the step
