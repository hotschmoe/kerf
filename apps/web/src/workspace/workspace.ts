// Workspace mode controller: library list, the open file, live events (SSE), and the App's `Remote` (all writes go to the server).
import type { App, Remote } from '../app';
import type { ApplyResult, Actor, KerfDoc, Op } from '../types';
import { ServerError, type DocRow, type ServerEvent, type ServerInfo, type ServerLogEntry, type WorkspaceClient } from './client';

export type WsEvent = 'docs' | 'file' | 'online' | 'agent-edit';

/** key-sorted JSON, so the same document compares equal however a writer ordered its keys */
export function stable(v: unknown): string {
  return JSON.stringify(v, (_k, x) => (x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.entries(x as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : 1))) : x));
}

export const slug = (s: string) => s.toLowerCase().replace(/\.kerf\.json$/, '').replace(/[^a-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '').replace(/^\.+/, '') || 'untitled';
export const fileFor = (s: string) => `${slug(s)}.kerf.json`;

export interface AgentCard { file: string; entry: ServerLogEntry }

export class Workspace implements Remote {
  docs: DocRow[] = [];
  file: string | null = null;
  etag: string | null = null;
  online = false;
  /** wall-clock ms of the last agent log entry seen (status line: LOCAL AGENT · LAST EDIT 12S AGO) */
  lastAgentEdit: number | null = null;
  /** a run started from this UI is active: its own tool cards already tell the story, so log cards are suppressed */
  uiRunActive = false;
  /** the server polls the folder every 500 ms, so log lines of a finished UI run trail its exit event */
  private suppressUntil = 0;
  endUiRun() { this.uiRunActive = false; this.suppressUntil = Date.now() + 2000; }
  private known: string[] = [];
  private listeners = new Map<string, Set<(a?: unknown) => void>>();
  private agentHandlers = new Map<string, (ev: unknown) => void>();
  /** agent events that arrived before the run's handler was registered (the POST answer races the SSE stream) */
  private agentBuffer = new Map<string, unknown[]>();
  private stopEvents: (() => void) | null = null;
  private docsTimer = 0;
  private syncTimer = 0;

  constructor(public app: App, public client: WorkspaceClient, public info: ServerInfo) {
    app.remote = this;
  }

  on(ev: WsEvent | 'card', fn: (a?: unknown) => void) {
    let s = this.listeners.get(ev);
    if (!s) this.listeners.set(ev, (s = new Set()));
    s.add(fn);
    return () => s!.delete(fn);
  }
  private emit(ev: WsEvent | 'card', a?: unknown) { this.listeners.get(ev)?.forEach((f) => f(a)); }

  // ---------------- Remote (used by App) ----------------
  noteKnown(doc: KerfDoc) { this.known.unshift(stable(doc)); this.known.length = Math.min(this.known.length, 6); }
  isKnown(doc: KerfDoc) { return this.known.includes(stable(doc)); }

  async fetchCurrent(): Promise<{ doc: KerfDoc } | null> {
    if (!this.file) return null;
    const r = await this.client.getDoc(this.file);
    this.etag = r.etag;
    return { doc: r.doc };
  }

  async exportFile(view: string, format: string, sheet: boolean) {
    if (!this.file) return null;
    const r = await this.client.exportDoc(this.file, { view, format, sheet, px: format === 'png' ? 1600 : undefined });
    return { name: r.name, bytes: new Uint8Array(await r.blob.arrayBuffer()) };
  }

  async apply(ops: Op[], actor: Actor, why: string): Promise<ApplyResult & { conflict?: boolean }> {
    const fail = (error: string): ApplyResult => ({ ok: false, diagnostics: [], summary: '', error });
    if (!this.file) {
      const set = ops.find((o) => o.op === 'set' && o.path === 'doc');
      const v = set?.value as { id?: string; title?: string } | undefined;
      if (!v) return fail('no document is open. Build one with kerf_apply (op "set", path "doc") first, or open one from the library.');
      const file = await this.createUnique(v.id ?? 'untitled', v.title);
      this.file = file; this.etag = null;
      this.emit('file');
    }
    try {
      const r = await this.client.apply(this.file!, ops, why, actor, this.etag);
      if (r.conflict) return { ...fail('DOCUMENT CHANGED ON DISK — RELOADED'), conflict: true };
      const res = r.result!;
      if (res.ok && res.doc) {
        this.noteKnown(res.doc);
        this.etag = r.etag ?? (await this.client.getDoc(this.file!).then((d) => d.etag).catch(() => null));
        this.scheduleDocs();
      }
      return res;
    } catch (e) {
      return fail(e instanceof ServerError ? `${e.code}: ${e.message}` : (e as Error).message);
    }
  }

  private async createUnique(id: string, title?: string): Promise<string> {
    for (let n = 1; n < 100; n++) {
      const file = n === 1 ? fileFor(id) : fileFor(`${id}-${n}`);
      try { await this.client.createDoc(file, title, slug(id)); return file; }
      catch (e) { if (!(e instanceof ServerError && e.status === 409)) throw e; }
    }
    throw new Error('could not find a free file name');
  }

  // ---------------- library ----------------
  async refreshDocs() {
    try { this.docs = await this.client.listDocs(); } catch { return; }
    this.emit('docs');
  }
  private scheduleDocs() { clearTimeout(this.docsTimer); this.docsTimer = window.setTimeout(() => void this.refreshDocs(), 120); }

  async open(file: string): Promise<boolean> {
    try {
      const { doc, etag } = await this.client.getDoc(file);
      this.file = file; this.etag = etag;
      this.known = [];
      this.noteKnown(doc);
      try { localStorage.setItem('kerf.lastDoc', file); } catch { /* ignore */ }
      const r = await this.app.openDoc(doc, `library ${file}`);
      if (!r.ok) { this.app.flash(`OPEN FAILED: ${r.error ?? r.diagnostics[0]?.message ?? file}`.slice(0, 160), 'err'); return false; }
      this.emit('file');
      return true;
    } catch (e) {
      this.app.flash(`OPEN FAILED: ${(e as Error).message}`.slice(0, 160), 'err');
      return false;
    }
  }

  /** Create a library document (like `kerf new`) and open it. With `content`, the document body is imported through apply. */
  async create(name: string, title?: string, content?: KerfDoc): Promise<string | null> {
    const file = fileFor(name);
    try {
      await this.client.createDoc(file, title, slug(name));
    } catch (e) {
      if (e instanceof ServerError && e.status === 409) {
        if (content) { this.app.flash(`${file.toUpperCase()} ALREADY EXISTS — OPENED IT`, 'warn'); await this.open(file); return file; }
        this.app.flash(`${file.toUpperCase()} ALREADY EXISTS`, 'err');
        return null;
      }
      this.app.flash(`CREATE FAILED: ${(e as Error).message}`.slice(0, 160), 'err');
      return null;
    }
    await this.refreshDocs();
    if (!(await this.open(file))) return null;
    if (content) {
      const r = await this.app.applyOps([{ op: 'set', path: 'doc', value: { ...content, id: slug(name) } }], 'designer', `Import ${content.title ?? content.id}`);
      if (!r.ok) this.app.flash(`IMPORT FAILED: ${r.error ?? r.diagnostics[0]?.message ?? ''}`.slice(0, 160), 'err');
    }
    return file;
  }

  // ---------------- events ----------------
  start() {
    this.stopEvents = this.client.events((e) => this.onEvent(e), (online) => {
      const was = this.online;
      this.online = online;
      this.emit('online');
      if (online && !was) { void this.refreshDocs(); if (this.file && this.app.doc) this.scheduleSync(); }
    });
    void this.refreshDocs();
  }
  stop() { this.stopEvents?.(); }

  onAgentRun(runId: string, fn: ((ev: unknown) => void) | null) {
    if (!fn) { this.agentHandlers.delete(runId); this.agentBuffer.delete(runId); return; }
    this.agentHandlers.set(runId, fn);
    for (const ev of this.agentBuffer.get(runId) ?? []) fn(ev);
    this.agentBuffer.delete(runId);
  }

  private scheduleSync() {
    clearTimeout(this.syncTimer);
    this.syncTimer = window.setTimeout(() => void this.app.syncFromRemote().then((changed) => { if (changed) this.emit('file'); }).catch(() => undefined), 80);
  }

  private onEvent(e: ServerEvent) {
    switch (e.type) {
      case 'doc_changed':
        this.scheduleDocs();
        if (e.file === this.file) this.scheduleSync();
        break;
      case 'doc_added': this.scheduleDocs(); break;
      case 'doc_removed':
        this.scheduleDocs();
        if (e.file === this.file) { this.file = null; this.etag = null; this.app.flash(`${e.file.toUpperCase()} WAS REMOVED FROM THE FOLDER`, 'warn'); this.emit('file'); }
        break;
      case 'log': {
        const who = (e.entry?.who ?? '').toLowerCase();
        if (who === 'designer' || who === 'llm' || !e.entry) break; // our own writes (and other browser tabs); agents get cards
        this.lastAgentEdit = Date.now();
        this.emit('agent-edit');
        if (!this.uiRunActive && Date.now() >= this.suppressUntil) this.emit('card', { file: e.file, entry: e.entry } satisfies AgentCard);
        break;
      }
      case 'agent': {
        const h = this.agentHandlers.get(e.run_id);
        if (h) h(e.event);
        else { const b = this.agentBuffer.get(e.run_id) ?? []; if (b.length < 2000) b.push(e.event); this.agentBuffer.set(e.run_id, b); if (this.agentBuffer.size > 8) this.agentBuffer.delete(this.agentBuffer.keys().next().value!); }
        break;
      }
      default: break;
    }
  }
}
