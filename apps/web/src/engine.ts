// Typed, async engine API (SPEC §13) over the raw wasm ABI. Same JS for the rust and zig stacks.
import type { ApplyResult, CheckResult, Drawing, KerfDoc, Mesh, Op, Actor } from './types';
import { RawEngine, EngineCallError, type CallStat } from './engine-raw';
import EngineWorker from './engine.worker?worker';

export { EngineCallError };
export type { CallStat };

export interface EngineVersion { engine: string; version: string; spec: string }
export type InspectQuery =
  | { q: 'summary' }
  | { q: 'component' | 'anchors'; id: string }
  | { q: 'at'; point: [number, number]; view: string }
  | { q: 'catalog'; type: string };

export interface Engine {
  readonly kind: 'wasm' | 'fixture';
  info: EngineVersion;
  stats: CallStat[];
  loadMs: number;
  wasmBytes: number;
  version(): Promise<EngineVersion>;
  catalog(format: 'json' | 'markdown'): Promise<unknown>;
  fmt(doc: KerfDoc): Promise<{ doc: KerfDoc }>;
  check(doc: KerfDoc, style: unknown): Promise<CheckResult>;
  apply(doc: KerfDoc, style: unknown, ops: Op[], actor?: Actor): Promise<ApplyResult>;
  inspect(doc: KerfDoc, style: unknown, query: InspectQuery): Promise<unknown>;
  drawing(doc: KerfDoc, style: unknown, view: string): Promise<Drawing>;
  mesh(doc: KerfDoc, style: unknown): Promise<Mesh>;
  exportBytes(doc: KerfDoc, style: unknown, view: string, format: 'svg' | 'dxf' | 'pdf', sheet?: boolean): Promise<Uint8Array>;
}

type Caller = {
  json<T>(fn: string, input: unknown): Promise<T>;
  bytes(fn: string, input: unknown): Promise<Uint8Array>;
};

function wrap(c: Caller, stats: CallStat[], meta: { loadMs: number; wasmBytes: number }): Engine {
  const e: Engine = {
    kind: 'wasm',
    info: { engine: '?', version: '?', spec: '?' },
    stats,
    get loadMs() { return meta.loadMs; },
    get wasmBytes() { return meta.wasmBytes; },
    version: () => c.json<EngineVersion>('version', {}),
    catalog: (format) => c.json('catalog', { format }),
    fmt: (doc) => c.json('fmt', { doc }),
    check: (doc, style) => c.json('check', { doc, style }),
    apply: (doc, style, ops, actor = 'llm') => c.json('apply', { doc, style, ops, actor }),
    inspect: (doc, style, query) => c.json('inspect', { doc, style, query }),
    drawing: (doc, style, view) => c.json('drawing', { doc, style, view }),
    mesh: (doc, style) => c.json('mesh', { doc, style }),
    exportBytes: (doc, style, view, format, sheet = false) => c.bytes('export', { doc, style, view, format, sheet }),
  };
  return e;
}

export interface LoadOptions { url: string; worker?: boolean }

export async function loadEngine(opts: LoadOptions): Promise<Engine> {
  const stats: CallStat[] = [];
  const meta = { loadMs: 0, wasmBytes: 0 };
  let caller: Caller;
  if (opts.worker) {
    const w = new EngineWorker();
    let seq = 0;
    const pending = new Map<number, { res: (v: any) => void; rej: (e: Error) => void }>();
    w.onmessage = (ev: MessageEvent) => {
      const m = ev.data;
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      if (m.stats) stats.push(...m.stats);
      if (m.ok) p.res(m.result);
      else p.rej(new EngineCallError('engine', m.message, m.payload));
    };
    const send = (msg: Record<string, unknown>, transfer: Transferable[] = []) =>
      new Promise<any>((res, rej) => { const id = ++seq; pending.set(id, { res, rej }); w.postMessage({ id, ...msg }, transfer); });
    const url = new URL(opts.url, location.href).href;
    const init = await send({ kind: 'init', url });
    meta.loadMs = init.loadMs; meta.wasmBytes = init.wasmBytes;
    caller = {
      json: (fn, input) => send({ kind: 'call', fn, input }),
      bytes: (fn, input) => send({ kind: 'call', fn, input, bytes: true }),
    };
  } else {
    const raw = await RawEngine.load(opts.url);
    meta.loadMs = raw.loadMs; meta.wasmBytes = raw.wasmBytes;
    raw.stats = stats;
    caller = {
      json: async (fn, input) => raw.callJson(fn, input),
      bytes: async (fn, input) => raw.callBytes(fn, input),
    };
  }
  const eng = wrap(caller, stats, meta);
  eng.info = await eng.version();
  return eng;
}
