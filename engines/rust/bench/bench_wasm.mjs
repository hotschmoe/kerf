// Usage: node bench/bench_wasm.mjs <kerf.wasm> <cases dir> [--filter S] [--exclude S] [--min-ms N] [--min-reps N]
// Wasm twin of bench/bench.rs over the SPEC 13.1 ABI (any engine's kerf.wasm): one TSV row per case, median/min per call.
import fs from 'node:fs';
const [wasm, dir, ...rest] = process.argv.slice(2);
const opt = (k, d) => { const i = rest.indexOf(k); return i >= 0 ? rest[i + 1] : d; };
const filter = opt('--filter', ''), exclude = opt('--exclude', null), minMs = +opt('--min-ms', 300), minReps = +opt('--min-reps', 10);
const inst = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasm)), {});
const ex = inst.exports, enc = new TextEncoder();
function call(f, i) {
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f); new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const st = ex.kerf_call(fp, f.length, ip, i.length);
  const n = ex.kerf_out_len(); ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return { st, n };
}
console.log('label\tfn\tmedian_us\tmin_us\treps\tbytes\tstatus');
for (const line of fs.readFileSync(`${dir}/manifest.tsv`, 'utf8').split('\n').filter(Boolean)) {
  const [label, fn, file] = line.split('\t');
  if (!label.includes(filter) || (exclude && label.includes(exclude))) continue;
  const f = enc.encode(fn), i = new Uint8Array(fs.readFileSync(`${dir}/${file}`));
  call(f, i); call(f, i);
  const times = []; let r; const start = performance.now();
  while (times.length < minReps || (performance.now() - start < minMs && times.length < 5000)) {
    const t = performance.now(); r = call(f, i); times.push((performance.now() - t) * 1e3);
  }
  times.sort((a, b) => a - b);
  console.log(`${label}\t${fn}\t${times[times.length >> 1].toFixed(1)}\t${times[0].toFixed(1)}\t${times.length}\t${r.n}\t${r.st ? 'err' : 'ok'}`);
}
