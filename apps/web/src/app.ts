// Application model: the one KerfDoc + style + engine, op log, selection, view state, caches.
import type { ApplyResult, Actor, Diagnostic, Drawing, KerfDoc, Mesh, Op, StrokeFont, View } from './types';
import type { Engine, InspectQuery } from './engine';
import { EngineCallError } from './engine';
import { DrawingModel } from './draw2d';
import { hhmm } from './ui/dom';
import { errorText } from './engine-raw';

export type Mode = 'view' | '3d' | 'sheet';
export type Selection = { id: string; kind: 'comp' | 'note' } | null;

export interface LogEntry {
  n: number;
  who: 'DESIGNER' | 'CLAUDE';
  kind: 'op' | 'open' | 'undo';
  why: string;
  ops: Op[];
  ts: string;
  before: KerfDoc | null;
  changed: string[];
  undone?: boolean;
}

type Ev = 'doc' | 'selection' | 'view' | 'hover' | 'cursor' | 'status' | 'claude' | 'log';

export interface PartRow { no: string; id: string; type: string; params: string; x: string; y: string; raw: string }

export class App {
  doc: KerfDoc | null = null;
  summary = '';
  diagnostics: Diagnostic[] = [];
  opLog: LogEntry[] = [];
  rev = 0;
  activeView = '';
  mode: Mode = 'view';
  selection: Selection = null;
  hover: string | null = null;
  cursor: [number, number] | null = null;
  /** transient status-line message (e.g. EXPORTED ...), cleared after a few seconds */
  message: { text: string; level: 'ok' | 'err' | 'warn' } | null = null;
  claude: { state: 'NO KEY' | 'OK' | 'BUSY' | 'ERR'; round?: number; detail?: string } = { state: 'NO KEY' };
  /** designer edits not yet reported to Claude (HARNESS.md "Designer edits") */
  pendingDesignerEdits: string[] = [];
  perf: Record<string, number> = {};

  private listeners = new Map<Ev, Set<() => void>>();
  private chain: Promise<unknown> = Promise.resolve();
  private drawingCache = new Map<string, Promise<Drawing>>();
  private modelCache = new Map<string, DrawingModel>();
  private meshCache = new Map<string, Promise<Mesh>>();
  private sheetCache = new Map<string, Promise<string>>();
  private msgTimer = 0;

  constructor(public engine: Engine, public style: unknown, public font: StrokeFont) {}

  on(ev: Ev, fn: () => void): () => void {
    let s = this.listeners.get(ev);
    if (!s) this.listeners.set(ev, (s = new Set()));
    s.add(fn);
    return () => s!.delete(fn);
  }
  emit(ev: Ev) { this.listeners.get(ev)?.forEach((f) => f()); }

  /** Serialize engine operations that mutate the doc. */
  private queue<T>(fn: () => Promise<T>): Promise<T> {
    const p = this.chain.then(fn, fn);
    this.chain = p.catch(() => undefined);
    return p;
  }

  // ---- views ----
  get views(): View[] { return this.doc?.views ?? []; }
  get view(): View | undefined { return this.views.find((v) => v.id === this.activeView); }
  setMode(m: Mode) { if (m !== this.mode) { this.mode = m; this.emit('view'); } }
  setActiveView(id: string) { if (id !== this.activeView) { this.activeView = id; if (this.mode !== 'view' && this.mode !== 'sheet') this.mode = 'view'; this.emit('view'); } }

  setSelection(id: string | null) {
    const kind = id ? (this.isNote(id) ? 'note' : 'comp') : null;
    const sel: Selection = id && kind ? { id, kind } : null;
    if (sel?.id === this.selection?.id) return;
    this.selection = sel;
    this.emit('selection');
  }
  isNote(id: string): boolean { return !!this.view?.annotations?.some((a) => a.id === id); }
  setHover(id: string | null) { if (id !== this.hover) { this.hover = id; this.emit('hover'); } }
  setCursor(x: number | null, y: number | null) { this.cursor = x === null || y === null ? null : [x, y]; this.emit('cursor'); }

  flash(text: string, level: 'ok' | 'err' | 'warn' = 'ok', ms = 6000) {
    this.message = { text, level };
    this.emit('status');
    clearTimeout(this.msgTimer);
    this.msgTimer = window.setTimeout(() => { this.message = null; this.emit('status'); }, ms);
  }

  setClaude(state: App['claude']) { this.claude = state; this.emit('claude'); }

  // ---- doc mutation ----
  emptyDoc(): KerfDoc { return { kerf: '0.1', id: 'untitled', title: '', meta: {}, components: [], views: [] }; }

  async applyOps(ops: Op[], actor: Actor, why: string): Promise<ApplyResult> {
    return this.queue(async () => {
      const before = this.doc;
      let res: ApplyResult;
      try {
        res = await this.engine.apply(before ?? this.emptyDoc(), this.style, ops, actor);
      } catch (e) {
        const msg = e instanceof EngineCallError ? e.message : String(e);
        const payload = e instanceof EngineCallError ? e.payload : undefined;
        const diags = payload && typeof payload === 'object' && Array.isArray((payload as { diagnostics?: Diagnostic[] }).diagnostics)
          ? (payload as { diagnostics: Diagnostic[] }).diagnostics : [];
        return { ok: false, diagnostics: diags, summary: '', error: msg } as ApplyResult;
      }
      if (res.ok && res.doc) {
        this.commit(res, before, actor === 'llm' ? 'CLAUDE' : 'DESIGNER', why, ops, 'op');
      }
      return res;
    });
  }

  private commit(res: ApplyResult, before: KerfDoc | null, who: LogEntry['who'], why: string, ops: Op[], kind: LogEntry['kind']) {
    this.doc = res.doc!;
    this.summary = res.summary ?? '';
    this.diagnostics = res.diagnostics ?? [];
    this.rev++;
    this.drawingCache.clear(); this.modelCache.clear(); this.meshCache.clear(); this.sheetCache.clear();
    this.opLog.push({ n: this.opLog.length + 1, who, kind, why, ops, ts: hhmm(), before, changed: res.changed ?? [] });
    if (who === 'DESIGNER') this.pendingDesignerEdits.push(`${kind === 'undo' ? 'UNDO' : kind === 'open' ? 'OPENED' : 'EDIT'}: ${why}`);
    if (!this.views.some((v) => v.id === this.activeView)) this.activeView = this.views[0]?.id ?? '';
    if (this.selection && !this.doc.components.some((c) => c.id === this.selection!.id) && !this.isNote(this.selection.id)) this.selection = null;
    this.emit('doc'); this.emit('log'); this.emit('view'); this.emit('selection');
  }

  /** Replace the whole document as a designer action (open file / sample). */
  async openDoc(doc: KerfDoc, why: string): Promise<ApplyResult> {
    const res = await this.queue(async () => {
      const before = this.doc;
      let r: ApplyResult;
      try {
        r = await this.engine.apply(this.emptyDoc(), this.style, [{ op: 'set', path: 'doc', value: doc }], 'designer');
      } catch (e) {
        return { ok: false, diagnostics: [], summary: '', error: e instanceof EngineCallError ? e.message : String(e) } as ApplyResult;
      }
      if (r.ok && r.doc) {
        this.activeView = '';
        this.commit(r, before, 'DESIGNER', why, [{ op: 'set', path: 'doc', value: '…' }], 'open');
        this.pendingDesignerEdits[this.pendingDesignerEdits.length - 1] = `OPENED ${r.doc.id} (${r.doc.components.length} components, ${r.doc.views.length} views): ${why}. Read it with kerf_inspect before editing.`;
      }
      return r;
    });
    return res;
  }

  /** Undo the last op group (restores its before-snapshot). */
  async undo(): Promise<boolean> {
    const last = [...this.opLog].reverse().find((e) => !e.undone && e.kind !== 'undo');
    if (!last) return false;
    return this.queue(async () => {
      const cur = this.doc;
      if (last.before === null) {
        this.doc = null; this.summary = ''; this.diagnostics = []; this.rev++;
        this.drawingCache.clear(); this.modelCache.clear(); this.meshCache.clear(); this.sheetCache.clear();
        this.activeView = ''; this.selection = null;
      } else {
        const r = await this.engine.apply(this.emptyDoc(), this.style, [{ op: 'set', path: 'doc', value: last.before }], 'designer');
        if (!r.ok || !r.doc) { this.flash('UNDO FAILED: ' + (r.error ?? 'engine rejected the snapshot'), 'err'); return false; }
        this.doc = r.doc; this.summary = r.summary; this.diagnostics = r.diagnostics ?? [];
        this.rev++;
        this.drawingCache.clear(); this.modelCache.clear(); this.meshCache.clear(); this.sheetCache.clear();
      }
      last.undone = true;
      this.opLog.push({ n: this.opLog.length + 1, who: 'DESIGNER', kind: 'undo', why: `UNDO ${last.n}: ${last.why}`, ops: [], ts: hhmm(), before: cur, changed: last.changed });
      this.pendingDesignerEdits.push(`UNDID op group ${last.n} (${last.why}); the document reverted to its state before it`);
      if (!this.views.some((v) => v.id === this.activeView)) this.activeView = this.views[0]?.id ?? '';
      this.emit('doc'); this.emit('log'); this.emit('view'); this.emit('selection');
      return true;
    });
  }

  // ---- derived data (cached per rev) ----
  getDrawing(view = this.activeView): Promise<Drawing> {
    if (!this.doc) return Promise.reject(new Error('no document'));
    let p = this.drawingCache.get(view);
    if (!p) {
      const t0 = performance.now();
      p = this.engine.drawing(this.doc, this.style, view).then((d) => { this.perf.lastDrawingMs = performance.now() - t0; return d; });
      p.catch(() => this.drawingCache.delete(view));
      this.drawingCache.set(view, p);
    }
    return p;
  }
  async getDrawingModel(view = this.activeView): Promise<DrawingModel> {
    const d = await this.getDrawing(view);
    let m = this.modelCache.get(view);
    if (!m || m.drawing !== d) { m = new DrawingModel(d); this.modelCache.set(view, m); }
    return m;
  }
  getMesh(): Promise<Mesh> {
    if (!this.doc) return Promise.reject(new Error('no document'));
    const key = 'mesh';
    let p = this.meshCache.get(key);
    if (!p) {
      const t0 = performance.now();
      p = this.engine.mesh(this.doc, this.style).then((m) => { this.perf.lastMeshMs = performance.now() - t0; return m; });
      p.catch(() => this.meshCache.delete(key));
      this.meshCache.set(key, p);
    }
    return p;
  }
  getSheetSvg(view = this.activeView): Promise<string> {
    if (!this.doc) return Promise.reject(new Error('no document'));
    let p = this.sheetCache.get(view);
    if (!p) {
      const t0 = performance.now();
      p = this.engine.exportBytes(this.doc, this.style, view, 'svg', true).then((b) => { this.perf.lastSheetMs = performance.now() - t0; return new TextDecoder().decode(b); });
      p.catch(() => this.sheetCache.delete(view));
      this.sheetCache.set(view, p);
    }
    return p;
  }

  async inspect(q: InspectQuery): Promise<unknown> {
    if (!this.doc) throw new Error('no document yet; build one with kerf_apply (set doc)');
    return this.engine.inspect(this.doc, this.style, q);
  }

  /** Parsed parts rows from the engine summary text (SPEC §13: id padded 14, type+params padded 44, x range, y range). */
  partsRows(): PartRow[] {
    const rows: PartRow[] = [];
    const lines = this.summary.split('\n');
    const comps = new Set(this.doc?.components.map((c) => c.id) ?? []);
    for (const raw of lines) {
      const m = /^\s*(\S+)\s+(\S+)\s+(.*?)\s+x\s+(.+?)\s+y\s+(.+?)\s*$/.exec(raw);
      if (!m || !comps.has(m[1].replace(/#\d+$/, ''))) continue;
      rows.push({ no: String(rows.length + 1).padStart(2, '0'), id: m[1], type: m[2], params: m[3], x: m[4], y: m[5], raw });
    }
    if (!rows.length && this.doc) {
      this.doc.components.forEach((c, i) => rows.push({ no: String(i + 1).padStart(2, '0'), id: c.id, type: c.type, params: '', x: '', y: '', raw: c.id }));
    }
    return rows;
  }

  counts() {
    const d = this.diagnostics;
    return {
      components: this.doc?.components.length ?? 0,
      err: d.filter((x) => x.level === 'error').length,
      warn: d.filter((x) => x.level === 'warning').length,
      info: d.filter((x) => x.level === 'info').length,
    };
  }

  describeError(e: unknown): string { return e instanceof EngineCallError ? e.message : errorText(e instanceof Error ? e.message : e); }
}
