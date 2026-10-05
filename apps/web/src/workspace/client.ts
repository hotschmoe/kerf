// HTTP/SSE client for `kerf serve` (spec/SERVE.md "HTTP API"). Token: ?token= -> sessionStorage -> Authorization: Bearer
// (SSE cannot send headers, so /api/events takes ?token=).
import type { AgentInfo } from '../chat/providers';
import type { ApplyResult, KerfDoc, Op } from '../types';

export interface ServerInfo { version: string; dir: string; token_required: boolean; agents: AgentInfo[]; proxy: boolean }
export interface DocRow { file: string; id: string; title: string; mtime_ms: number; size: number; components: number; views: number; errors: number; warnings: number }
export interface ServerLogEntry { ts: string; who: string; tool?: string; why?: string; ops?: unknown[]; changed?: string[]; summary_head?: string }
export type ServerEvent =
  | { type: 'doc_changed'; file: string; mtime_ms?: number; who?: string }
  | { type: 'doc_added'; file: string }
  | { type: 'doc_removed'; file: string }
  | { type: 'log'; file: string; entry: ServerLogEntry }
  | { type: 'agent'; run_id: string; event: unknown }
  | { type: 'ping' };

export class ServerError extends Error {
  constructor(public status: number, public code: string, message: string, public etag?: string) { super(message); }
}

const TOKEN_KEY = 'kerf.token';
const ss = {
  get: () => { try { return sessionStorage.getItem(TOKEN_KEY) ?? ''; } catch { return ''; } },
  set: (v: string) => { try { sessionStorage.setItem(TOKEN_KEY, v); } catch { /* ignore */ } },
};

/** Take `?token=` from the URL into sessionStorage and scrub it from the address bar. */
export function captureToken(): string {
  const u = new URL(location.href);
  const t = u.searchParams.get('token');
  if (t) {
    ss.set(t);
    u.searchParams.delete('token');
    try { history.replaceState(null, '', u.pathname + (u.search || '') + u.hash); } catch { /* ignore */ }
  }
  return ss.get();
}

export class WorkspaceClient {
  readonly apiBase: string;
  constructor(origin: string, private token: string) { this.apiBase = origin.replace(/\/+$/, '') + '/api'; }

  authHeaders(): Record<string, string> { return this.token ? { authorization: `Bearer ${this.token}` } : {}; }

  /** null = not a kerf server (static mode). Throws ServerError(401) when it is one but needs a token. */
  static async detect(origin: string, token: string, timeoutMs = 2500): Promise<{ client: WorkspaceClient; info: ServerInfo } | null> {
    const client = new WorkspaceClient(origin, token);
    const ac = new AbortController();
    const timer = setTimeout(() => ac.abort(), timeoutMs);
    try {
      const r = await fetch(`${client.apiBase}/info`, { headers: client.authHeaders(), signal: ac.signal });
      const ct = r.headers.get('content-type') ?? '';
      if (!ct.includes('json')) return null; // static hosts answer 404 / index.html
      const j = (await r.json()) as ServerInfo & { error?: { code: string; message: string } };
      if (r.status === 401 && j.error) throw new ServerError(401, j.error.code, j.error.message);
      if (!r.ok || typeof j.version !== 'string' || !Array.isArray(j.agents)) return null;
      return { client, info: j };
    } catch (e) {
      if (e instanceof ServerError) throw e;
      return null;
    } finally { clearTimeout(timer); }
  }

  private async request(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<Response> {
    return fetch(`${this.apiBase}${path}`, {
      method, signal,
      headers: { ...this.authHeaders(), ...(body !== undefined ? { 'content-type': 'application/json' } : {}) },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  }

  private async fail(r: Response): Promise<never> {
    let code = `HTTP_${r.status}`, msg = `HTTP ${r.status}`;
    try { const j = await r.json(); if (j?.error) { code = j.error.code ?? code; msg = j.error.message ?? msg; } } catch { /* not JSON */ }
    throw new ServerError(r.status, code, msg, r.headers.get('etag') ?? undefined);
  }

  async listDocs(): Promise<DocRow[]> {
    const r = await this.request('GET', '/docs');
    if (!r.ok) return this.fail(r);
    return (await r.json()) as DocRow[];
  }

  async createDoc(file: string, title?: string, id?: string): Promise<void> {
    const r = await this.request('POST', '/docs', { file, ...(title ? { title } : {}), ...(id ? { id } : {}) });
    if (!r.ok) return this.fail(r);
  }

  async getDoc(file: string): Promise<{ doc: KerfDoc; etag: string | null }> {
    const r = await this.request('GET', `/docs/${encodeURIComponent(file)}`);
    if (!r.ok) return this.fail(r);
    return { doc: (await r.json()) as KerfDoc, etag: r.headers.get('etag') };
  }

  /** The engine `apply` output. 409 (if_match mismatch) comes back as `{ conflict: true }`. */
  async apply(file: string, ops: Op[], why: string, actor: 'designer' | 'llm', ifMatch?: string | null): Promise<{ result?: ApplyResult; conflict?: boolean; etag: string | null }> {
    const r = await this.request('POST', `/docs/${encodeURIComponent(file)}/apply`, { ops, why, actor, ...(ifMatch ? { if_match: ifMatch } : {}) });
    if (r.status === 409) return { conflict: true, etag: r.headers.get('etag') };
    const etag = r.headers.get('etag');
    let j: unknown = null;
    try { j = await r.json(); } catch { /* handled below */ }
    const o = j as (ApplyResult & { error?: unknown }) | null;
    if (o && typeof o === 'object' && 'ok' in o) {
      const res = { ...o, diagnostics: o.diagnostics ?? [], summary: o.summary ?? '' } as ApplyResult;
      if (typeof o.error === 'object' && o.error) res.error = (o.error as { message?: string }).message; // engine errors may nest
      return { result: res, etag };
    }
    const err = (o as { error?: { code?: string; message?: string } } | null)?.error;
    throw new ServerError(r.status, err?.code ?? `HTTP_${r.status}`, err?.message ?? `HTTP ${r.status}`);
  }

  async exportDoc(file: string, q: { view: string; format: string; sheet?: boolean; px?: number }): Promise<{ blob: Blob; name: string }> {
    const p = new URLSearchParams({ view: q.view, format: q.format });
    if (q.sheet) p.set('sheet', '1');
    if (q.px) p.set('px', String(q.px));
    const r = await this.request('GET', `/docs/${encodeURIComponent(file)}/export?${p}`);
    if (!r.ok) return this.fail(r);
    const cd = r.headers.get('content-disposition') ?? '';
    const m = /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(cd);
    return { blob: await r.blob(), name: m ? decodeURIComponent(m[1]) : `${file.replace(/\.kerf\.json$/, '')}-${q.view}.${q.format}` };
  }

  async agentRun(agent: string, message: string, sessionId?: string, file?: string | null): Promise<string> {
    const r = await this.request('POST', '/agent/run', { agent, message, ...(sessionId ? { session_id: sessionId } : {}), ...(file ? { file } : {}) });
    if (!r.ok) return this.fail(r);
    return ((await r.json()) as { run_id: string }).run_id;
  }

  async agentStop(runId: string): Promise<void> {
    await this.request('POST', '/agent/stop', { run_id: runId }).catch(() => undefined);
  }

  /** SSE feed. Named events (`event: doc_changed`) and unnamed `data: {"type":...}` messages are both accepted. */
  events(on: (e: ServerEvent) => void, status: (online: boolean) => void): () => void {
    const url = `${this.apiBase}/events` + (this.token ? `?token=${encodeURIComponent(this.token)}` : '');
    const es = new EventSource(url);
    const deliver = (type: string, raw: string) => {
      let d: Record<string, unknown> = {};
      try { d = raw ? JSON.parse(raw) : {}; } catch { return; }
      on({ ...d, type: (d.type as string) ?? type } as ServerEvent);
    };
    for (const t of ['doc_changed', 'doc_added', 'doc_removed', 'log', 'agent', 'ping']) es.addEventListener(t, (e) => deliver(t, (e as MessageEvent).data));
    es.onmessage = (e) => deliver('', e.data);
    es.onopen = () => status(true);
    es.onerror = () => status(false); // EventSource reconnects by itself
    return () => es.close();
  }
}
