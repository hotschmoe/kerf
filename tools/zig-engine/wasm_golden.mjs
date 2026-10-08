// Usage (from engines/zig): node ../../tools/zig-engine/wasm_golden.mjs dist/kerf.wasm  -- wasm output must equal the native goldens byte for byte.
import fs from 'node:fs';
const inst = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[2])), {});
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
for (const d of ['truss-bearing-cmu', 'monopour-slab-door-recess', 'flush-beam-strap']) {
  const doc = JSON.parse(fs.readFileSync(`../../spec/details/${d}.kerf.json`, 'utf8'));
  for (const v of ['A', 'B']) {
    for (const [fmt, file] of [['svg', `${v}.svg`], ['dxf', `${v}.dxf`], ['pdf', `${v}.pdf`]]) {
      const out = call('export', JSON.stringify({ doc, view: v, format: fmt, sheet: false }));
      const gold = fs.readFileSync(`tests/golden/${d}/${file}`);
      n++; if (!out.equals(gold)) { bad++; console.log('DIFF', d, file); }
    }
    const out = call('drawing', JSON.stringify({ doc, view: v }));
    n++; if (!out.equals(fs.readFileSync(`tests/golden/${d}/drawing-${v}.json`))) { bad++; console.log('DIFF', d, 'drawing', v); }
  }
}
console.log(`wasm vs native goldens: ${n - bad}/${n} byte-identical`);
if (bad) process.exitCode = 1; // a CI gate: a mismatch must fail the step
