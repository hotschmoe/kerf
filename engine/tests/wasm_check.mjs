// Usage: node tests/wasm_check.mjs <kerf.wasm> [doc.kerf.json view out.svg [format [sheet]]]
// Runs the module version and optional export workflows.
import fs from 'node:fs';
const wasmPath = process.argv[2];
const bytes = fs.readFileSync(wasmPath);
const mod = new WebAssembly.Module(bytes);
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
const version = call('version', '{}');
if (version.status !== 0 || !JSON.parse(dec.decode(version.bytes)).version) throw new Error('wasm version call failed');
console.log('wasm version call passed');
if (process.argv[3]) {
  const doc = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
  const view = process.argv[4];
  const input = JSON.stringify({ doc, view, format: process.argv[6] || 'svg', sheet: process.argv[7] === 'sheet' });
  const r = call('export', input);
  if (r.status !== 0 || r.bytes.length === 0) throw new Error('wasm export failed');
  if (process.argv[5]) fs.writeFileSync(process.argv[5], r.bytes);
}
