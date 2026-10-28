// Usage: node [--cpu-prof] bench/wasm_loop.mjs <kerf.wasm> <cases dir> <loops> [--filter S] [--exclude S]
// Runs every case `loops` times (profiling driver; bench_wasm.mjs measures).
import fs from 'node:fs';
const [wasm, dir, loops, ...rest] = process.argv.slice(2);
const opt = (k, d) => { const i = rest.indexOf(k); return i >= 0 ? rest[i + 1] : d; };
const filter = opt('--filter', ''), exclude = opt('--exclude', null);
const ex = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasm)), {}).exports, enc = new TextEncoder();
let total = 0;
for (const line of fs.readFileSync(`${dir}/manifest.tsv`, 'utf8').split('\n').filter(Boolean)) {
  const [label, fn, file] = line.split('\t');
  if (!label.includes(filter) || (exclude && label.includes(exclude))) continue;
  const f = enc.encode(fn), i = new Uint8Array(fs.readFileSync(`${dir}/${file}`));
  for (let k = 0; k < +loops; k++) {
    const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
    new Uint8Array(ex.memory.buffer, fp, f.length).set(f); new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
    ex.kerf_call(fp, f.length, ip, i.length); total += ex.kerf_out_len();
    ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  }
}
console.error(`output bytes ${total}`);
