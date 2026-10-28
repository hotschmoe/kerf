// Usage: node bench_wasm.mjs <before.wasm> <after.wasm> [N]  -- per-call ms of every engine function on the reference
// docs (the cases of tools/zig-engine/bench.mjs plus PNG export), the two engines alternated per case, best of 3 rounds.
import fs from 'node:fs';
const [wa, wb] = [process.argv[2], process.argv[3]], N = +(process.argv[4] || 10);
const enc = new TextEncoder();
const load = (p) => new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(p)), {}).exports;
const engines = [load(wa), load(wb)];
function call(ex, fn, input) {
  const f = enc.encode(fn), i = enc.encode(input);
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f); new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const st = ex.kerf_call(fp, f.length, ip, i.length);
  const n = ex.kerf_out_len(); ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return { st, n };
}
const dir = new URL('../../../spec/details/', import.meta.url).pathname;
const tot = [0, 0];
console.log('doc                          fn        view   before   after  ratio');
for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.kerf.json')).sort()) {
  const doc = JSON.parse(fs.readFileSync(dir + f, 'utf8'));
  const cases = [['check', {}], ['mesh', {}], ['inspect', { query: { q: 'summary' } }]];
  for (const v of ['A', 'B']) for (const fmt of ['drawing', 'svg', 'dxf', 'pdf', 'png']) cases.push([fmt === 'drawing' ? 'drawing' : 'export', { view: v, format: fmt, sheet: false }]);
  for (const [fn, extra] of cases) {
    const input = JSON.stringify({ doc, ...extra });
    const best = [Infinity, Infinity];
    for (let round = 0; round < 3; round++) for (const e of [0, 1]) {
      call(engines[e], fn, input);
      const t0 = performance.now();
      for (let k = 0; k < N; k++) call(engines[e], fn, input);
      best[e] = Math.min(best[e], (performance.now() - t0) / N);
    }
    tot[0] += best[0]; tot[1] += best[1];
    console.log(`${f.replace('.kerf.json', '').padEnd(28)} ${(fn === 'export' ? extra.format : fn).padEnd(9)} ${(extra.view || '-').padEnd(4)} ${best[0].toFixed(2).padStart(7)} ${best[1].toFixed(2).padStart(7)} ${(best[1] / best[0]).toFixed(3).padStart(6)}`);
  }
}
console.log(`TOTAL ms over all cases: before ${tot[0].toFixed(1)}  after ${tot[1].toFixed(1)}  ratio ${(tot[1] / tot[0]).toFixed(3)}`);
