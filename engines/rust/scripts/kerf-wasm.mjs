// Minimal Node loader/CLI for dist/kerf.wasm (raw ABI, SPEC 13.1).
// Library:  import { loadKerf } from './kerf-wasm.mjs';  const k = await loadKerf(path); k.call('version', {}) -> {ok, text, bytes}
// CLI:      node kerf-wasm.mjs <fn> <input.json> [out-file]    (input "-" = stdin)
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

export async function loadKerf(wasmPath = new URL('../dist/kerf.wasm', import.meta.url)) {
  const bytes = fs.readFileSync(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const x = instance.exports;
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  function put(u8) {
    const p = x.kerf_alloc(u8.length);
    new Uint8Array(x.memory.buffer, p, u8.length).set(u8);
    return p;
  }
  function call(fn, input) {
    const f = enc.encode(fn);
    const i = typeof input === 'string' || input instanceof Uint8Array ? (typeof input === 'string' ? enc.encode(input) : input) : enc.encode(JSON.stringify(input ?? {}));
    const fp = put(f), ip = put(i);
    const code = x.kerf_call(fp, f.length, ip, i.length);
    x.kerf_free(fp, f.length); x.kerf_free(ip, i.length);
    // copy the output before the next call
    const out = new Uint8Array(x.memory.buffer, x.kerf_out_ptr(), x.kerf_out_len()).slice();
    return { ok: code === 0, bytes: out, get text() { return dec.decode(out); } };
  }
  return { call, exports: x };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [fn, inp, outFile] = process.argv.slice(2);
  const k = await loadKerf();
  const input = inp === '-' ? fs.readFileSync(0, 'utf8') : fs.readFileSync(inp, 'utf8');
  const t0 = performance.now();
  const r = k.call(fn, input);
  const ms = performance.now() - t0;
  if (outFile) fs.writeFileSync(outFile, r.bytes); else process.stdout.write(r.bytes);
  console.error(`${fn}: ${r.ok ? 'ok' : 'ERROR'} ${r.bytes.length} bytes in ${ms.toFixed(1)} ms`);
  process.exit(r.ok ? 0 : 1);
}
