#!/usr/bin/env node
// Builds engine/dist/kerf.wasm: cargo (wasm32, profile `small`), the export section pruned to the SPEC 13.1 ABI, then
// binaryen `wasm-opt -Oz` when it is on PATH (`--require-wasm-opt` makes its absence an error, as in the release).
//   node tools/build_wasm.mjs [--out FILE] [--require-wasm-opt | --no-wasm-opt]
// CARGO_TARGET_DIR is honored (relative to engine/).
// rustc always exports `__heap_base` and `__data_end` from a cdylib; the ABI allows neither, hence the pruning.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const engine = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../engine');
const args = process.argv.slice(2);
const flag = (name) => args.indexOf(name);
const out = path.resolve(flag('--out') >= 0 ? args[flag('--out') + 1] : path.join(engine, 'dist/kerf.wasm'));
const target = path.resolve(engine, process.env.CARGO_TARGET_DIR ?? 'target');
const KEEP = new Set(['memory', 'kerf_alloc', 'kerf_free', 'kerf_call', 'kerf_out_ptr', 'kerf_out_len']);

function run(cmd, argv) {
  const r = spawnSync(cmd, argv, { cwd: engine, stdio: 'inherit' });
  if (r.error?.code === 'ENOENT') return false;
  if (r.status !== 0) process.exit(r.status ?? 1);
  return true;
}

function uleb(bytes, i) {
  let value = 0, shift = 0;
  for (;;) {
    const b = bytes[i++];
    value += (b & 0x7f) * 2 ** shift;
    shift += 7;
    if (!(b & 0x80)) return [value, i];
  }
}

function encodeUleb(n) {
  const out = [];
  do {
    const b = n & 0x7f;
    n = Math.floor(n / 128);
    out.push(n ? b | 0x80 : b);
  } while (n);
  return Buffer.from(out);
}

function pruneExports(wasm) {
  const parts = [wasm.subarray(0, 8)];
  let i = 8;
  while (i < wasm.length) {
    const id = wasm[i];
    const [size, body0] = uleb(wasm, i + 1);
    let body = wasm.subarray(body0, body0 + size);
    if (id === 7) {
      const [count, first] = uleb(body, 0);
      const kept = [];
      let at = first;
      for (let n = 0; n < count; n++) {
        const [len, nameAt] = uleb(body, at);
        const name = body.subarray(nameAt, nameAt + len).toString();
        const [, end] = uleb(body, nameAt + len + 1);
        if (KEEP.has(name)) kept.push(body.subarray(at, end));
        at = end;
      }
      body = Buffer.concat([encodeUleb(kept.length), ...kept]);
    }
    parts.push(Buffer.from([id]), encodeUleb(body.length), body);
    i = body0 + size;
  }
  return Buffer.concat(parts);
}

run('cargo', ['build', '--profile', 'small', '-p', 'kerf-wasm', '--target', 'wasm32-unknown-unknown']);
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, pruneExports(fs.readFileSync(path.join(target, 'wasm32-unknown-unknown/small/kerf_wasm.wasm'))));
const optimized = flag('--no-wasm-opt') < 0 && run('wasm-opt', ['-Oz', '--enable-bulk-memory', '--enable-sign-ext', '--enable-nontrapping-float-to-int',
  '--enable-mutable-globals', '--strip-debug', '--strip-producers', out, '-o', out]);
if (!optimized && flag('--require-wasm-opt') >= 0) {
  console.error('build_wasm: wasm-opt (binaryen) is not on PATH');
  process.exit(1);
}
console.log(`${out}: ${fs.statSync(out).size} bytes${optimized ? '' : ' (no wasm-opt)'}`);
