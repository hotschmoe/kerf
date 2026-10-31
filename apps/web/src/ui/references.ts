// REFERENCE panel: the images a detail is redrawn from (`meta.references`, SPEC 23), next to the drawing.
// Workspace mode reads each file through `kerf serve`; a click opens it in a pan/zoom overlay.
import type { App } from '../app';
import type { KerfDoc } from '../types';
import { h, clear, btn } from './dom';
import { ImageViewer } from './panzoom';

export interface Reference { path: string; caption: string }

const OPEN_KEY = 'kerf.refPanel';

export function referencesOf(doc: KerfDoc | null): Reference[] {
  const list = doc?.meta?.references;
  if (!Array.isArray(list)) return [];
  return list.flatMap((r) => {
    const o = r as { path?: unknown; caption?: unknown } | null;
    return o && typeof o.path === 'string' && o.path ? [{ path: o.path, caption: typeof o.caption === 'string' ? o.caption : '' }] : [];
  });
}

interface Loaded { url: string; etag: string | null; pdf: boolean }

export function mountReferences(app: App, onToggle: () => void) {
  const el = h('aside.vref', { 'aria-label': 'reference images' });
  const count = h('span');
  const head = h('div.ph', h('span', 'REFERENCE'), h('span.r', count, btn('×', () => setOpen(false), 'sm x')));
  const list = h('div.reflist');
  el.append(head, list);

  const loaded = new Map<string, Loaded>();
  let refs: Reference[] = [];
  let key = '';
  let open = (() => { try { return localStorage.getItem(OPEN_KEY) !== '0'; } catch { return true; } })();

  function setOpen(v: boolean) {
    open = v;
    try { localStorage.setItem(OPEN_KEY, v ? '1' : '0'); } catch { /* ignore */ }
    el.classList.toggle('hide', !shown());
    if (shown()) void render();
    onToggle();
  }
  const shown = () => open && refs.length > 0;

  function forget() {
    for (const l of loaded.values()) URL.revokeObjectURL(l.url);
    loaded.clear();
  }

  async function load(card: HTMLElement, ref: Reference, file: string) {
    const frame = card.querySelector('.refimg') as HTMLElement;
    const remote = app.remote!;
    try {
      const r = await remote.fetchReference(ref.path);
      if (remote.file !== file || !card.isConnected) return;
      const id = `${file}\n${ref.path}`;
      let l = loaded.get(id);
      if (!l || l.etag === null || l.etag !== r.etag) {
        if (l) URL.revokeObjectURL(l.url);
        l = { url: URL.createObjectURL(r.blob), etag: r.etag, pdf: r.blob.type === 'application/pdf' };
        loaded.set(id, l);
      }
      clear(frame);
      if (l.pdf) {
        frame.append(h('div.refpdf', h('b', 'PDF'), h('a.btn.sm', { href: l.url, target: '_blank', rel: 'noopener' }, 'OPEN PDF')));
        return;
      }
      const img = h('img', { alt: ref.caption || ref.path, draggable: false });
      img.src = l.url;
      frame.append(img);
      const url = l.url;
      frame.classList.add('zoom');
      frame.title = 'CLICK TO ZOOM';
      frame.tabIndex = 0;
      frame.setAttribute('role', 'button');
      frame.addEventListener('click', () => zoom(ref, url));
      frame.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); zoom(ref, url); } });
      await img.decode().catch(() => undefined);
      card.dataset.state = 'ok';
    } catch (e) {
      if (!card.isConnected) return;
      clear(frame);
      frame.append(h('div.refstate.err', `CANNOT SHOW ${ref.path}: ${(e as Error).message ?? e}`));
      card.dataset.state = 'error';
    }
  }

  async function render() {
    clear(list);
    count.textContent = `${refs.length} IMAGE${refs.length === 1 ? '' : 'S'}`;
    const file = app.remote?.file ?? null;
    const jobs: Promise<void>[] = [];
    for (const ref of refs) {
      const frame = h('div.refimg', h('div.refstate', app.remote ? 'LOADING…' : 'REFERENCE FILES ARE READ FROM THE WORKSPACE FOLDER: OPEN IT WITH `kerf serve`'));
      const card = h('figure.refcard', { 'data-path': ref.path, 'data-state': app.remote ? 'loading' : 'static' },
        frame, h('figcaption', ref.caption ? h('b', ref.caption) : null, h('span.sub', ref.path)));
      list.append(card);
      if (app.remote && file) jobs.push(load(card, ref, file));
    }
    await Promise.all(jobs);
  }

  function update(notify = true) {
    refs = referencesOf(app.doc);
    const next = `${app.remote?.file ?? ''}\n${JSON.stringify(refs)}`;
    el.classList.toggle('hide', !shown());
    if (next === key) return;
    if (app.remote?.file !== key.split('\n')[0]) forget();
    key = next;
    if (shown()) void render();
    if (notify) onToggle();
  }

  function zoom(first: Reference, firstUrl: string) {
    const images = refs.flatMap((r) => {
      const l = loaded.get(`${app.remote?.file ?? ''}\n${r.path}`);
      return l && !l.pdf ? [{ ref: r, url: l.url }] : [];
    });
    let i = Math.max(0, images.findIndex((x) => x.url === firstUrl));
    if (!images.length) images.push({ ref: first, url: firstUrl });
    const title = h('span');
    const host = h('div.zb');
    const scrim = h('div.scrim.dim');
    const tools = h('span.r');
    const box = h('div.float.refzoom', { role: 'dialog', 'aria-label': 'reference image' }, h('div.mh', title, tools), host);
    document.body.append(scrim, box);
    const viewer = new ImageViewer(host, 'reference');
    const show = (n: number) => {
      i = (n + images.length) % images.length;
      const it = images[i];
      title.textContent = `${images.length > 1 ? `${i + 1}/${images.length}  ` : ''}${it.ref.caption || it.ref.path}`;
      void viewer.setSrc(it.url, it.ref.caption || it.ref.path);
    };
    const close = () => { scrim.remove(); box.remove(); document.removeEventListener('keydown', onKey); };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
      else if (e.key === 'ArrowRight' && images.length > 1) show(i + 1);
      else if (e.key === 'ArrowLeft' && images.length > 1) show(i - 1);
    };
    if (images.length > 1) tools.append(btn('◀', () => show(i - 1), 'sm'), btn('▶', () => show(i + 1), 'sm'));
    tools.append(btn('−', () => viewer.zoomBy(1 / 1.4), 'sm'), btn('+', () => viewer.zoomBy(1.4), 'sm'), btn('FIT', () => viewer.fit(), 'sm'), btn('CLOSE', close, 'sm'));
    document.addEventListener('keydown', onKey);
    scrim.addEventListener('pointerdown', close);
    show(i);
  }

  app.on('doc', () => update());
  update(false);
  return {
    el,
    get count() { return refs.length; },
    get open() { return shown(); },
    toggle: () => setOpen(!open),
  };
}
