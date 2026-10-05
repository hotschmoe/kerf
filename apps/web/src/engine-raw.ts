// Raw wasm ABI loader (SPEC §13.1). The ONE place that talks to kerf.wasm for either engine
// (engines/rust/dist/kerf.wasm or engines/zig/dist/kerf.wasm). Synchronous; wrapped by engine.ts
// (direct, or inside a Web Worker).

interface Exports {
  memory: WebAssembly.Memory;
  kerf_alloc(len: number): number;
  kerf_free(ptr: number, len: number): void;
  kerf_call(fnPtr: number, fnLen: number, inPtr: number, inLen: number): number;
  kerf_out_ptr(): number;
  kerf_out_len(): number;
  _initialize?: () => void;
}

export interface CallStat { fn: string; ms: number; inBytes: number; outBytes: number }

export class EngineCallError extends Error {
  constructor(public fn: string, message: string, public payload: unknown) {
    super(`${fn}: ${message}`);
  }
}

const enc = new TextEncoder();
const dec = new TextDecoder();

export class RawEngine {
  private x!: Exports;
  stats: CallStat[] = [];
  wasmBytes = 0;

  static async load(wasm: BufferSource | Response | string): Promise<RawEngine> {
    const e = new RawEngine();
    let module: WebAssembly.Module;
    const t0 = performance.now();
    if (typeof wasm === 'string') wasm = await fetch(wasm);
    if (typeof Response !== 'undefined' && wasm instanceof Response) {
      const buf = await wasm.arrayBuffer();
      e.wasmBytes = buf.byteLength;
      module = await WebAssembly.compile(buf);
    } else {
      e.wasmBytes = (wasm as ArrayBuffer).byteLength;
      module = await WebAssembly.compile(wasm as BufferSource);
    }
    // The spec says no imports are required; stub any that exist so a stray import never blocks loading.
    const imports: WebAssembly.Imports = {};
    for (const imp of WebAssembly.Module.imports(module)) {
      (imports[imp.module] ??= {})[imp.name] =
        imp.kind === 'function' ? (() => 0) as WebAssembly.ImportValue :
        imp.kind === 'memory' ? new WebAssembly.Memory({ initial: 32, maximum: 65536 }) :
        imp.kind === 'table' ? new WebAssembly.Table({ initial: 0, element: 'anyfunc' }) :
        new WebAssembly.Global({ value: 'i32', mutable: false }, 0);
    }
    const inst = await WebAssembly.instantiate(module, imports);
    e.x = inst.exports as unknown as Exports;
    e.x._initialize?.();
    e.loadMs = performance.now() - t0;
    return e;
  }
  loadMs = 0;

  /** Low-level call; returns [rc, bytes]. */
  callRaw(fn: string, input: Uint8Array): [number, Uint8Array] {
    const x = this.x;
    const fb = enc.encode(fn);
    const fp = x.kerf_alloc(fb.length);
    const ip = x.kerf_alloc(Math.max(input.length, 1));
    // memory may have grown during alloc: take views AFTER allocating
    const mem = new Uint8Array(x.memory.buffer);
    mem.set(fb, fp);
    mem.set(input, ip);
    const rc = x.kerf_call(fp, fb.length, ip, input.length);
    const op = x.kerf_out_ptr(), ol = x.kerf_out_len();
    const out = new Uint8Array(x.memory.buffer, op, ol).slice();
    x.kerf_free(fp, fb.length);
    x.kerf_free(ip, Math.max(input.length, 1));
    return [rc, out];
  }

  private record(fn: string, t0: number, i: number, o: number) {
    this.stats.push({ fn, ms: performance.now() - t0, inBytes: i, outBytes: o });
    if (this.stats.length > 500) this.stats.splice(0, 250);
  }

  /** JSON in, JSON out. Throws EngineCallError (with the parsed error payload) when the engine returns rc=1. */
  callJson<T>(fn: string, input: unknown): T {
    const t0 = performance.now();
    const bytes = enc.encode(JSON.stringify(input ?? {}));
    const [rc, out] = this.callRaw(fn, bytes);
    this.record(fn, t0, bytes.length, out.length);
    const text = dec.decode(out);
    let parsed: unknown = text;
    try { parsed = JSON.parse(text); } catch { /* keep text */ }
    if (rc !== 0) throw new EngineCallError(fn, errorText(parsed), parsed);
    return parsed as T;
  }

  /** JSON in, raw bytes out (export). */
  callBytes(fn: string, input: unknown): Uint8Array {
    const t0 = performance.now();
    const bytes = enc.encode(JSON.stringify(input ?? {}));
    const [rc, out] = this.callRaw(fn, bytes);
    this.record(fn, t0, bytes.length, out.length);
    if (rc !== 0) {
      const text = dec.decode(out);
      let parsed: unknown = text;
      try { parsed = JSON.parse(text); } catch { /* text */ }
      throw new EngineCallError(fn, errorText(parsed), parsed);
    }
    return out;
  }
}

export function errorText(p: unknown): string {
  if (typeof p === 'string') return p;
  if (p && typeof p === 'object') {
    const o = p as Record<string, unknown>;
    if (typeof o.message === 'string') return (typeof o.code === 'string' ? `${o.code}: ` : '') + o.message;
    if (typeof o.error === 'string') return o.error;
    if (o.error && typeof o.error === 'object') return errorText(o.error);
    return JSON.stringify(p);
  }
  return String(p);
}
