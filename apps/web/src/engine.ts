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
  /** engine functions this build lacks and the app emulates client-side (empty once the engine is complete) */
  compat?: Set<string>;
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

/** Client-side SPEC §14 op application, used ONLY while an engine build has no `apply` yet (reported in `engine.compat`). */
function applyOpsLocal(doc: KerfDoc, ops: Op[]): { doc: KerfDoc; changed: string[] } {
  const merge = (t: any, p: any): any => {
    if (p === null || typeof p !== 'object' || Array.isArray(p)) return p;
    const o = t && typeof t === 'object' && !Array.isArray(t) ? { ...t } : {};
    for (const [k, v] of Object.entries(p)) { if (v === null) delete o[k]; else o[k] = merge(o[k], v); }
    return o;
  };
  let d: any = JSON.parse(JSON.stringify(doc));
  const changed: string[] = [];
  for (const op of ops) {
    const seg = op.path.split('/');
    const v = op.value as any;
    const fail = (m: string) => { throw new EngineCallError('apply', `E_PARAM: ${m} (op ${op.op} ${op.path})`, null); };
    if (op.op === 'set' && op.path === 'doc') { d = JSON.parse(JSON.stringify(v)); continue; }
    if (seg[0] === 'meta') { d.meta = merge(d.meta ?? {}, v); continue; }
    if (seg[0] === 'components') {
      const list: any[] = d.components;
      const i = seg[1] ? list.findIndex((c) => c.id === seg[1]) : -1;
      if (op.op === 'add') { const b = op.before ? list.findIndex((c) => c.id === op.before) : -1; if (b >= 0) list.splice(b, 0, v); else list.push(v); changed.push(v.id); }
      else if (i < 0) fail(`no component "${seg[1]}"`);
      else if (op.op === 'update') { list[i] = merge(list[i], v); changed.push(seg[1]); }
      else if (op.op === 'remove') { list.splice(i, 1); changed.push(seg[1]); }
      continue;
    }
    if (seg[0] === 'views') {
      if (!seg[1]) { if (op.op === 'add') { d.views.push(v); changed.push(v.id); continue; } fail('bad views path'); }
      const view = d.views.find((x: any) => x.id === seg[1]);
      if (!view) fail(`no view "${seg[1]}"`);
      if (!seg[2]) { if (op.op === 'update') { const { annotations, ...rest } = v ?? {}; void annotations; Object.assign(view, merge(view, rest)); } else if (op.op === 'remove') d.views = d.views.filter((x: any) => x !== view); changed.push(seg[1]); continue; }
      const ann: any[] = (view.annotations ??= []);
      const j = seg[3] ? ann.findIndex((a) => a.id === seg[3]) : -1;
      if (op.op === 'add') { ann.push(v); changed.push(v.id); }
      else if (j < 0) fail(`no annotation "${seg[3]}" in view ${seg[1]}`);
      else if (op.op === 'update') { ann[j] = merge(ann[j], v); changed.push(seg[3]); }
      else if (op.op === 'remove') { ann.splice(j, 1); changed.push(seg[3]); }
      continue;
    }
    fail('unsupported path');
  }
  return { doc: d, changed };
}

function wrap(c: Caller, stats: CallStat[], meta: { loadMs: number; wasmBytes: number }): Engine {
  const compat = new Set<string>();
  const missing = (e: unknown) => /unknown function/i.test((e as Error)?.message ?? '');
  const e: Engine = {
    compat,
    kind: 'wasm',
    info: { engine: '?', version: '?', spec: '?' },
    stats,
    get loadMs() { return meta.loadMs; },
    get wasmBytes() { return meta.wasmBytes; },
    version: () => c.json<EngineVersion>('version', {}),
    catalog: (format) => c.json('catalog', { format }),
    fmt: (doc) => c.json('fmt', { doc }),
    check: (doc, style) => c.json('check', { doc, style }),
    apply: async (doc, style, ops, actor = 'llm') => {
      if (!compat.has('apply')) {
        try { return await c.json<ApplyResult>('apply', { doc, style, ops, actor }); } catch (err) { if (!missing(err)) throw err; compat.add('apply'); }
      }
      const r = applyOpsLocal(doc, ops);
      const canon = await c.json<{ doc: KerfDoc }>('fmt', { doc: r.doc });
      const chk = await c.json<CheckResult>('check', { doc: canon.doc, style });
      return { ok: true, doc: canon.doc, diagnostics: chk.diagnostics, summary: chk.summary, changed: r.changed };
    },
    inspect: async (doc, style, query) => {
      if (!compat.has('inspect')) {
        try { return await c.json('inspect', { doc, style, query }); } catch (err) { if (!missing(err)) throw err; compat.add('inspect'); }
      }
      if (query.q === 'summary') return (await c.json<CheckResult>('check', { doc, style })).summary;
      if (query.q === 'component') return doc.components.find((x) => x.id === query.id) ?? { error: `no component ${query.id}` };
      if (query.q === 'catalog') return c.json('catalog', { format: 'markdown' });
      return { note: 'inspect is not implemented by this engine build' };
    },
    drawing: (doc, style, view) => c.json('drawing', { doc, style, view }),
    mesh: (doc, style) => c.json('mesh', { doc, style }),
    exportBytes: (doc, style, view, format, sheet = false) => c.bytes('export', { doc, style, view, format, sheet }),
  };
  return e;
}

export interface LoadOptions { url: string; worker?: boolean; /** wasm bytes already being fetched (index.html starts the fetch early) */ preloaded?: Promise<ArrayBuffer> | null }

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
    const bytes = await (opts.preloaded ?? fetch(opts.url).then((r) => { if (!r.ok) throw new Error(`${opts.url}: HTTP ${r.status}`); return r.arrayBuffer(); }));
    const init = await send({ kind: 'init', bytes }, [bytes]);
    meta.loadMs = init.loadMs; meta.wasmBytes = init.wasmBytes;
    caller = {
      json: (fn, input) => send({ kind: 'call', fn, input }),
      bytes: (fn, input) => send({ kind: 'call', fn, input, binary: true }),
    };
  } else {
    const raw = await RawEngine.load(opts.preloaded ? await opts.preloaded : opts.url);
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
