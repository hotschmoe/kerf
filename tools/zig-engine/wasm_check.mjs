// Usage: node wasm_check.mjs <kerf.wasm> [doc.kerf.json view out.svg]
// Verifies the SPEC 13.1 ABI (exact exports, zero imports) and optionally runs an export.
import fs from 'node:fs';
const wasmPath = process.argv[2];
const bytes = fs.readFileSync(wasmPath);
const mod = new WebAssembly.Module(bytes);
const imports = WebAssembly.Module.imports(mod);
const exports = WebAssembly.Module.exports(mod).map(e => `${e.name}:${e.kind}`).sort();
const want = ['kerf_alloc:function','kerf_call:function','kerf_free:function','kerf_out_len:function','kerf_out_ptr:function','memory:memory'];
let ok = true;
if (imports.length) { console.log('FAIL imports:', JSON.stringify(imports)); ok = false; }
if (JSON.stringify(exports) !== JSON.stringify(want)) { console.log('FAIL exports:', exports.join(' ')); ok = false; }
console.log(ok ? 'ABI OK: 0 imports, exports = ' + exports.join(' ') : 'ABI MISMATCH');
const inst = new WebAssembly.Instance(mod, {});
const ex = inst.exports;
const enc = new TextEncoder(), dec = new TextDecoder();
export function call(fn, input) {
  const f = enc.encode(fn), i = enc.encode(input);
  const fp = ex.kerf_alloc(f.length), ip = ex.kerf_alloc(i.length);
  new Uint8Array(ex.memory.buffer, fp, f.length).set(f);
  new Uint8Array(ex.memory.buffer, ip, i.length).set(i);
  const st = ex.kerf_call(fp, f.length, ip, i.length);
  const out = new Uint8Array(ex.memory.buffer, ex.kerf_out_ptr(), ex.kerf_out_len()).slice();
  ex.kerf_free(fp, f.length); ex.kerf_free(ip, i.length);
  return { status: st, bytes: out };
}
console.log(dec.decode(call('version', '{}').bytes).trim());
if (process.argv[3]) {
  const doc = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
  const view = process.argv[4];
  const input = JSON.stringify({ doc, view, format: process.argv[6] || 'svg', sheet: process.argv[7] === 'sheet' });
  let r; const N = 20; const t0 = performance.now();
  for (let k = 0; k < N; k++) r = call('export', input);
  const ms = (performance.now() - t0) / N;
  console.log(`export view ${view}: status ${r.status}, ${r.bytes.length} bytes, ${ms.toFixed(1)} ms/call (avg of ${N})`);
  if (process.argv[5]) fs.writeFileSync(process.argv[5], r.bytes);
}
process.exit(ok ? 0 : 1);
