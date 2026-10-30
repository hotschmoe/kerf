// Usage: node bench.mjs <kerf.wasm> [N]   -- per-call timings of every engine function on the reference docs.
import fs from 'node:fs';
const wasm = process.argv[2], N = +(process.argv[3] || 20);
const inst = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasm)), {});
const ex = inst.exports, enc = new TextEncoder();
function call(fn, input) {
  const f = enc.encode(fn), i = enc.encode(input);
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f); new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const st = ex.kerf_call(fp, f.length, ip, i.length);
  const n = ex.kerf_out_len(); ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return { st, n };
}
const dir = new URL('../../spec/details/', import.meta.url).pathname;
console.log('doc                          fn        view  ms/call  out bytes');
for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.kerf.json'))) {
  const doc = JSON.parse(fs.readFileSync(dir + f, 'utf8'));
  const cases = [['check', {}], ['mesh', {}], ['inspect', { query: { q: 'summary' } }]];
  for (const v of ['A', 'B']) for (const fmt of ['drawing', 'svg', 'dxf', 'pdf']) cases.push([fmt === 'drawing' ? 'drawing' : 'export', { view: v, format: fmt, sheet: false }]);
  for (const [fn, extra] of cases) {
    const input = JSON.stringify({ doc, ...extra });
    call(fn, input);
    const t0 = performance.now(); let r;
    for (let k = 0; k < N; k++) r = call(fn, input);
    const ms = (performance.now() - t0) / N;
    console.log(`${f.replace('.kerf.json','').padEnd(28)} ${(fn === 'export' ? extra.format : fn).padEnd(9)} ${(extra.view || '-').padEnd(4)} ${ms.toFixed(1).padStart(7)}  ${r.n}${r.st ? '  ERR' : ''}`);
  }
}
