import './styles.css';
import { App } from './app';
import { loadEngine, type Engine } from './engine';
import { setFont } from './strokefont';
import type { KerfDoc, StrokeFont } from './types';
import { h } from './ui/dom';
import { mountHeader, type SampleInfo, type HeaderHooks } from './ui/header';
import { mountConsole } from './ui/console';
import { mountViewport } from './ui/viewport';
import { mountInspector } from './ui/inspector';
import { mountStatus } from './ui/status';
import { MockTransport } from './chat/mock';
import { ChatSession } from './chat/session';
import { Workspace } from './workspace/workspace';
import { WorkspaceClient, ServerError, captureToken } from './workspace/client';
import { mountLibrary } from './ui/library';
import { popup } from './ui/popup';
import { exportActive } from './export';

const t0 = performance.now();
interface Pre { wasm?: Promise<ArrayBuffer>; style?: Promise<unknown>; font?: Promise<StrokeFont>; samples?: Promise<SampleInfo[]>; sample?: Promise<KerfDoc> }
const pre: Pre = (window as unknown as { __pre?: Pre }).__pre ?? {};
const params = new URLSearchParams(location.search);
const ENGINE = (import.meta.env.VITE_ENGINE as string | undefined) ?? 'rust';

async function boot() {
  const getJson = async <T>(u: string): Promise<T> => {
    const r = await fetch(u);
    if (!r.ok) throw new Error(`${u}: HTTP ${r.status}`);
    return r.json() as Promise<T>;
  };
  const useWorker = params.get('worker') === '1'; // measured: main thread is faster for these call sizes (see NOTES)
  const enginePromise: Promise<Engine> = ENGINE === 'fixture'
    ? import('./fixture-engine').then((m) => m.createFixtureEngine())
    : loadEngine({ url: './kerf.wasm', worker: useWorker, preloaded: pre.wasm ?? null });
  // Workspace mode = served by `kerf serve` (GET /api/info answers). Anything else is static mode, exactly as before.
  const origin = params.get('server') ?? location.origin;
  const token = captureToken();
  // Only the `kerf serve` bundle (build:serve), the dev server, or ?server=/?workspace=1 probe for a server: a statically hosted build never makes the request.
  const probe = import.meta.env.VITE_SERVE === '1' || import.meta.env.DEV || params.has('server') || params.get('workspace') === '1';
  const detectPromise = !probe || params.get('static') === '1' || !/^https?:/.test(origin) ? Promise.resolve(null) : WorkspaceClient.detect(origin, token).catch((e) => { throw e instanceof ServerError && e.status === 401 ? new Error('THIS WORKSPACE NEEDS A TOKEN. OPEN THE URL THAT `kerf serve` PRINTED (…/?token=…).') : e; });
  const [engine, style, font, samples, detected] = await Promise.all([
    enginePromise,
    pre.style ?? getJson<unknown>('./style.json'),
    pre.font ?? getJson<StrokeFont>('./kerf-simplex.json'),
    pre.samples ?? getJson<SampleInfo[]>('./samples/index.json'),
    detectPromise,
  ]);
  setFont(font);
  const app = new App(engine, style, font);
  app.perf.engineReady = performance.now() - t0;
  const ws = detected ? new Workspace(app, detected.client, detected.info) : null;

  // ---- chat plumbing ----
  let demo = params.get('demo') === '1';
  let catalogMd = '';
  engine.catalog('markdown').then((c) => { catalogMd = typeof c === 'string' ? c : (c as { markdown?: string })?.markdown ?? JSON.stringify(c); }).catch(() => { catalogMd = '(catalog unavailable)'; });
  const loadSampleDoc = async (id?: string): Promise<KerfDoc> => {
    const s = samples.find((x) => x.id === id) ?? samples.find((x) => x.id === 'truss-bearing-cmu') ?? samples[0];
    return getJson<KerfDoc>(`./samples/${s.file}`);
  };
  const mock = new MockTransport({ loadDoc: () => loadSampleDoc(), chunkMs: params.get('fast') === '1' ? 0 : 12 });
  const apiParam = params.get('api');
  const testBase = apiParam && /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/.test(apiParam) ? apiParam : undefined; // test hook; never a remote host
  const session = new ChatSession({ app, ws, catalogMd: () => catalogMd, mock, demo, testBase });
  const harness = session.harness;

  // ---- UI ----
  const $ = (id: string) => document.getElementById(id)!;
  let con: ReturnType<typeof mountConsole>;
  const hooks: HeaderHooks = {
    samples, session, ws,
    loadSample: (s) => getJson<KerfDoc>(`./samples/${s.file}`),
    openSample: async (s) => { const r = await app.openDoc(await getJson<KerfDoc>(`./samples/${s.file}`), `sample ${s.id}`); if (!r.ok) app.flash('OPEN FAILED: ' + (r.error ?? ''), 'err'); },
  };
  mountHeader(app, $('hdr'), hooks);
  const setupBtn = () => document.getElementById('setupbtn') as HTMLButtonElement;
  if (ws) {
    const lib = h('div#library');
    $('console').append(lib);
    mountLibrary(ws, lib);
  }
  con = mountConsole(app, $('console'), session, { openSetup: () => setupBtn().click() });
  if (ws) { $('console').insertBefore($('library'), $('msgs')); ws.on('card', (c) => con.addAgentCard(c as never)); }
  session.onChange(() => { app.emit('claude'); });
  const vp = mountViewport(app, $('viewport'));
  mountInspector(app, $('inspector'));
  mountStatus(app, $('status'), ws);

  // narrow-screen tabs
  const main = $('main');
  const mtabs = $('mtabs');
  const showPanel = (p: string) => { main.dataset.show = p; mtabs.querySelectorAll('.tab').forEach((b) => b.classList.toggle('on', (b as HTMLElement).dataset.p === p)); vp.vp2.resize(); vp.view3?.resize(); };
  for (const [p, l] of [['console', 'CONSOLE'], ['viewport', 'VIEW'], ['inspector', 'INSPECTOR']]) {
    mtabs.append(h('button.tab', { type: 'button', 'data-p': p, class: p === 'viewport' ? 'on' : '', on: { click: () => showPanel(p) } }, `[${l}]`));
  }

  // keyboard: F = fit
  document.addEventListener('keydown', (e) => {
    if ((e.target as HTMLElement)?.matches?.('input,textarea,select')) return;
    if (e.key === 'f' || e.key === 'F') { if (app.mode === 'view') vp.vp2.fit(); else if (app.mode === 'sheet') vp.sheet.fit(); else vp.view3?.fit(); }
  });

  // ---- debug / test hooks ----
  const w = window as unknown as Record<string, unknown>;
  w.__kerf = { app, harness, session, ws, vp, con, exportActive: (f: 'dxf' | 'pdf' | 'svg' | 'png', save = true) => exportActive(app, f, save), engine, popup, loadEngine, version: 1, params: Object.fromEntries(params), mode: ws ? 'workspace' : 'static' };

  // ---- startup actions from URL ----
  if (ws) {
    ws.start();
    await ws.refreshDocs();
    let last: string | null = null; try { last = localStorage.getItem('kerf.lastDoc'); } catch { /* ignore */ }
    const want = params.get('doc');
    const pick = [want, last].find((f) => f && ws.docs.some((d) => d.file === f)) ?? (want ? null : ws.docs[0]?.file);
    if (want && !pick) app.flash(`NO SUCH DOCUMENT IN THE LIBRARY: ${want}`, 'err');
    if (pick) await ws.open(pick);
  }
  const sample = ws ? null : params.get('sample');
  if (sample || demo) {
    if (sample) {
      const r = await app.openDoc(pre.sample ? await pre.sample.catch(() => loadSampleDoc(sample)) : await loadSampleDoc(sample), `sample ${sample}`);
      if (!r.ok) app.flash('OPEN FAILED: ' + (r.error ?? ''), 'err');
    }
  }
  const view = params.get('view'); if (view) app.setActiveView(view);
  const mode = params.get('mode'); if (mode === '3d' || mode === 'sheet' || mode === 'view') app.setMode(mode);
  const sel = params.get('select'); if (sel) app.setSelection(sel);
  if (params.get('cut') === '1') { const t = setInterval(() => { const v3 = vp.view3; if (v3 && v3.ok && v3.cut === null && (window as unknown as { __rendered3d?: boolean }).__rendered3d) { v3.setCut(app.view?.cut_z ?? 0); clearInterval(t); (window as unknown as { __cut?: boolean }).__cut = true; } }, 100); }
  const tab = params.get('tab'); if (tab) (document.querySelector(`.itabs .tab:nth-child(${['parts', 'notes', 'diff', 'diag'].indexOf(tab) + 1})`) as HTMLElement | null)?.click();
  if (demo && params.get('auto') !== '0') {
    con.sendText('Build a detail of a prefab roof truss bearing on an 8-inch CMU wall with a grouted bond beam, hurricane ties and a PT sill plate.');
  }
  // The chrome must be in IBM Plex Mono before anything signals ready (font-display: block hides text until loaded).
  await Promise.all(['400 13px', '500 12px', '700 22px'].map((f) => document.fonts.load(`${f} "Plex Mono"`))).catch(() => undefined);
  await document.fonts.ready;
  w.__fontOk = document.fonts.check('13px "Plex Mono"');
  document.getElementById('boot')?.remove();
  app.perf.uiReady = performance.now() - t0;
  w.__ready = true;
  if (!app.doc) w.__rendered = true;
  if (params.get('shot3d')) { /* reserved */ }
}

boot().catch((e) => {
  console.error('KERF boot failed:', e?.message ?? e);
  const msg = String(e?.message ?? e);
  const box = document.getElementById('boot');
  const body = h('div', { style: 'max-width:640px;padding:24px;border:1px solid #1A1A1A;background:#FBFAF5;box-shadow:2px 2px 0 #1A1A1A' },
    h('div', { style: 'font-weight:700;letter-spacing:.2em;font-size:18px;color:#1A1A1A' }, 'KERF'),
    h('div', { style: 'margin:10px 0 6px;color:#C8102E;font-weight:700;letter-spacing:.08em' }, 'FAILED TO START'),
    h('div', { style: 'white-space:pre-wrap;overflow-wrap:anywhere;color:#1A1A1A;font-weight:400;letter-spacing:0' }, msg));
  if (box) { box.replaceChildren(body); } else document.body.prepend(body);
  (window as unknown as Record<string, unknown>).__bootError = msg;
});
