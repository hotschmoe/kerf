// Center column: view tabs [SECTION A] [ISO B] [3D] [SHEET], tool buttons, and the three viewers.
import type { App } from '../app';
import { Viewport2D } from '../view2d';
import type { View3D, Preset } from '../view3d';
import { h, clear } from './dom';

class SheetViewer {
  private page = h('div.page');
  private img = h('img', { alt: 'sheet', draggable: false });
  private k = 1; private tx = 0; private ty = 0;
  private drag: { x: number; y: number; tx: number; ty: number } | null = null;
  private url = '';
  private iw = 1056; private ih = 816;
  constructor(private host: HTMLElement) {
    this.page.append(this.img);
    host.append(this.page);
    host.addEventListener('wheel', (e) => {
      e.preventDefault();
      const r = host.getBoundingClientRect();
      this.zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-(e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY) * 0.0016));
    }, { passive: false });
    host.addEventListener('pointerdown', (e) => { host.setPointerCapture(e.pointerId); this.drag = { x: e.clientX, y: e.clientY, tx: this.tx, ty: this.ty }; host.classList.add('drag'); });
    host.addEventListener('pointermove', (e) => { if (!this.drag) return; this.tx = this.drag.tx + e.clientX - this.drag.x; this.ty = this.drag.ty + e.clientY - this.drag.y; this.apply(); });
    host.addEventListener('pointerup', () => { this.drag = null; host.classList.remove('drag'); });
    host.addEventListener('dblclick', () => this.fit());
    new ResizeObserver(() => { if (host.clientWidth && this.url && !this.userMoved) this.fit(); }).observe(host);
  }
  private userMoved = false;
  async setSvg(svg: string) {
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
    this.img.src = this.url;
    await this.img.decode().catch(() => undefined);
    this.iw = this.img.naturalWidth || 1056; this.ih = this.img.naturalHeight || 816;
    // present at the sheet's natural pixel size; zoom is applied on the wrapper
    this.img.style.width = this.iw + 'px'; this.img.style.height = this.ih + 'px';
    this.userMoved = false;
    this.fit();
  }
  clear() { if (this.url) URL.revokeObjectURL(this.url); this.url = ''; this.img.removeAttribute('src'); }
  fit() {
    const w = this.host.clientWidth, h = this.host.clientHeight;
    if (!w || !h) return;
    this.k = Math.min((w - 48) / this.iw, (h - 48) / this.ih);
    this.tx = (w - this.iw * this.k) / 2; this.ty = (h - this.ih * this.k) / 2;
    this.apply();
  }
  zoomBy(f: number) { this.zoomAt(this.host.clientWidth / 2, this.host.clientHeight / 2, f); }
  private zoomAt(sx: number, sy: number, f: number) {
    const nk = Math.max(0.05, Math.min(this.k * f, 20));
    const r = nk / this.k;
    this.tx = sx - (sx - this.tx) * r; this.ty = sy - (sy - this.ty) * r; this.k = nk;
    this.userMoved = true;
    this.apply();
  }
  private apply() { this.page.style.transform = `translate(${this.tx}px, ${this.ty}px) scale(${this.k})`; }
}

export function mountViewport(app: App, el: HTMLElement) {
  const tabs = h('div.tabs');
  const tools = h('div.vtools');
  const body = h('div.vbody');
  const host2d = h('div.vhost#host2d');
  const host3d = h('div.vhost.hide#host3d');
  const hostSheet = h('div.vhost.hide#sheetHost');
  const msg = h('div.vmsg');
  body.append(host2d, host3d, hostSheet, msg);
  el.append(h('div.vt', tabs, tools), body);

  const vp2 = new Viewport2D(host2d, {
    onHover: (id) => { app.setHover(id); view3?.setHover(id); },
    onSelect: (id) => app.setSelection(id),
    onCursor: (x, y) => app.setCursor(x, y),
    isNote: (id) => app.isNote(id),
    onNoteMove: (id, place) => {
      const v = app.view;
      if (!v) return;
      void app.applyOps([{ op: 'update', path: `views/${v.id}/annotations/${id}`, value: { place } }], 'designer', `Move note ${id} to [${place[0]}, ${place[1]}]`).then((r) => {
        if (!r.ok) app.flash(`MOVE REJECTED: ${r.error ?? r.diagnostics[0]?.message ?? ''}`.slice(0, 140), 'err');
      });
    },
  });
  const sheet = new SheetViewer(hostSheet);
  let view3: View3D | null = null;
  let loading3 = false;

  const showMsg = (text: string | null, err = false) => {
    clear(msg);
    msg.className = 'vmsg' + (err ? ' err' : '');
    if (text) msg.append(h('div', text));
    msg.style.display = text ? 'flex' : 'none';
  };

  let token = 0;
  async function refresh() {
    const t = ++token;
    const mode = app.mode;
    host2d.classList.toggle('hide', mode !== 'view');
    host3d.classList.toggle('hide', mode !== '3d');
    hostSheet.classList.toggle('hide', mode !== 'sheet');
    renderTabs();
    if (!app.doc) {
      showMsg('NO DETAIL LOADED. DESCRIBE ONE IN THE CONSOLE, OR OPEN A .KERF.JSON.');
      vp2.setModel(null);
      view3?.setMesh(null);
      sheet.clear();
      return;
    }
    showMsg(null);
    try {
      if (mode === 'view') {
        if (!app.activeView) { showMsg('THIS DOCUMENT HAS NO VIEWS YET. ASK CLAUDE TO ADD A SECTION VIEW.'); vp2.setModel(null); return; }
        const model = await app.getDrawingModel(app.activeView);
        if (t !== token) return;
        const sameView = vp2.model?.drawing.view === model.drawing.view && vp2.model?.drawing.doc === model.drawing.doc;
        vp2.setModel(model, { keepView: sameView });
        vp2.setSelection(app.selection?.id ?? null);
        const first = !app.perf.firstRender;
        if (first) requestAnimationFrame(() => { app.perf.firstRender = performance.now(); app.perf.drawingPaintMs = vp2.lastPaintMs; (window as unknown as { __rendered?: boolean }).__rendered = true; });
      } else if (mode === '3d') {
        if (!view3 && !loading3) {
          loading3 = true;
          const { View3D } = await import('../view3d');
          view3 = new View3D(host3d, { onSelect: (id) => app.setSelection(id), onHover: (id) => app.setHover(id) });
          loading3 = false;
          if (!view3.ok) { showMsg('3D UNAVAILABLE: ' + view3.failed, true); return; }
        }
        if (!view3 || !view3.ok) return;
        const mesh = await app.getMesh();
        if (t !== token) return;
        view3.setMesh(mesh);
        view3.setSelection(app.selection?.id ?? null);
        view3.resize();
        (window as unknown as { __rendered3d?: boolean }).__rendered3d = true;
      } else {
        const svg = await app.getSheetSvg(app.activeView);
        if (t !== token) return;
        await sheet.setSvg(svg);
        (window as unknown as { __renderedSheet?: boolean }).__renderedSheet = true;
      }
    } catch (e) {
      if (t !== token) return;
      showMsg(`CANNOT RENDER: ${app.describeError(e)}`, true);
    }
  }

  function tabBtn(label: string, on: boolean, click: () => void, cls = '') {
    return h('button.tab', { type: 'button', class: (on ? 'on ' : '') + cls, on: { click } }, label);
  }
  function renderTabs() {
    clear(tabs); clear(tools);
    for (const v of app.views) {
      tabs.append(tabBtn(`[${(v.kind || 'view').toUpperCase()} ${v.id}]`, app.mode === 'view' && app.activeView === v.id, () => { app.mode = 'view'; app.setActiveView(v.id); app.emit('view'); }));
    }
    tabs.append(tabBtn('[3D]', app.mode === '3d', () => { app.setMode('3d'); }));
    tabs.append(tabBtn('[SHEET]', app.mode === 'sheet', () => { app.setMode('sheet'); }));
    if (app.mode === '3d') {
      for (const [p, l] of [['front', 'FRONT'], ['iso', 'ISO'], ['top', 'TOP'], ['right', 'RIGHT']] as [Preset, string][]) {
        tools.append(tabBtn(`[${l}]`, false, () => view3?.preset(p), 'sm'));
      }
      tools.append(tabBtn('[FIT]', false, () => view3?.fit(), 'sm'));
    } else {
      const z = (f: number) => () => (app.mode === 'sheet' ? sheet.zoomBy(f) : vp2.zoomBy(f));
      tools.append(tabBtn('[−]', false, z(1 / 1.4), 'sm'), tabBtn('[+]', false, z(1.4), 'sm'), tabBtn('[FIT]', false, () => (app.mode === 'sheet' ? sheet.fit() : vp2.fit()), 'sm'));
    }
  }

  app.on('view', () => void refresh());
  app.on('selection', () => { vp2.setSelection(app.selection?.id ?? null); view3?.setSelection(app.selection?.id ?? null); });
  app.on('hover', () => { /* 2D hover is driven by pointer; 3D by its own raycast */ });
  void refresh();
  return { vp2, sheet, get view3() { return view3; }, refresh, showMsg };
}
