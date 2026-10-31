// Center column: view tabs [SECTION A] [ISO B] [3D] [SHEET] [LAYOUT S1], tool buttons, the three viewers and the REFERENCE panel.
import type { App } from '../app';
import { Viewport2D } from '../view2d';
import type { View3D, Preset } from '../view3d';
import { h, clear } from './dom';
import { ImageViewer } from './panzoom';
import { mountReferences } from './references';

export function mountViewport(app: App, el: HTMLElement) {
  const tabs = h('div.tabs');
  const tools = h('div.vtools');
  const body = h('div.vbody');
  const host2d = h('div.vhost#host2d');
  const host3d = h('div.vhost.hide#host3d');
  const hostSheet = h('div.vhost.hide#sheetHost');
  const msg = h('div.vmsg');
  body.append(host2d, host3d, hostSheet, msg);
  const refs = mountReferences(app, () => renderTabs());
  el.append(h('div.vt', tabs, tools), h('div.vsplit', body, refs.el));

  const vp2 = new Viewport2D(host2d, {
    onHover: (id) => { app.setHover(id); view3?.setHover(id); },
    onSelect: (id) => app.setSelection(id),
    onCursor: (x, y) => app.setCursor(x, y),
    isNote: (id) => app.isNote(id),
    onNoteMove: (id, place) => {
      const v = app.view;
      if (!v) return;
      void app.applyOps([{ op: 'update', path: `views/${v.id}/annotations/${id}`, value: { place } }], 'designer', `Move note ${id} to [${place[0]}, ${place[1]}]`).then((r) => {
        if (!r.ok) app.rejected(r, 'MOVE REJECTED');
      });
    },
  });
  const sheet = new ImageViewer(hostSheet);
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
        if (model.drawing.items.length === 0) showMsg(`THE ENGINE RETURNED NO DRAWING ITEMS FOR VIEW ${app.activeView} (${(app.view?.kind ?? '').toUpperCase()}).${model.drawing.diagnostics?.length ? ' ' + model.drawing.diagnostics[0].message : ''}`.slice(0, 260), true);
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
        const svg = app.activeLayout ? await app.getLayoutSvg(app.activeLayout) : await app.getSheetSvg(app.activeView);
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
      tabs.append(tabBtn(`[${(v.kind || 'view').toUpperCase()} ${v.id}]`, app.mode === 'view' && app.activeView === v.id, () => { app.mode = 'view'; app.activeLayout = ''; app.setActiveView(v.id); app.emit('view'); }));
    }
    tabs.append(tabBtn('[3D]', app.mode === '3d', () => { app.setMode('3d'); }));
    tabs.append(tabBtn('[SHEET]', app.mode === 'sheet' && !app.activeLayout, () => { app.setMode('sheet'); }));
    for (const l of app.layouts) {
      tabs.append(tabBtn(`[LAYOUT ${l.id}]`, app.mode === 'sheet' && app.activeLayout === l.id, () => app.setActiveLayout(l.id)));
    }
    if (refs.count) tools.append(tabBtn(`[REF ${refs.count}]`, refs.open, () => refs.toggle(), 'sm ref'));
    if (app.mode === '3d') {
      for (const [p, l] of [['front', 'FRONT'], ['iso', 'ISO'], ['top', 'TOP'], ['right', 'RIGHT']] as [Preset, string][]) {
        tools.append(tabBtn(`[${l}]`, false, () => view3?.preset(p), 'sm'));
      }
      tools.append(tabBtn('[CUT]', view3?.cut != null, () => { if (!view3) return; view3.setCut(view3.cut === null ? (app.view?.cut_z ?? 0) : null); renderTabs(); }, 'sm'));
      tools.append(tabBtn('[FIT]', false, () => view3?.fit(), 'sm'));
    } else {
      const z = (f: number) => () => (app.mode === 'sheet' ? sheet.zoomBy(f) : vp2.zoomBy(f));
      tools.append(tabBtn('[−]', false, z(1 / 1.4), 'sm'), tabBtn('[+]', false, z(1.4), 'sm'), tabBtn('[FIT]', false, () => (app.mode === 'sheet' ? sheet.fit() : vp2.fit()), 'sm'));
    }
  }

  app.on('view', () => void refresh());
  app.on('selection', () => { vp2.setSelection(app.selection?.id ?? null); view3?.setSelection(app.selection?.id ?? null); });
  app.on('hover', () => { vp2.setHover(app.hover); view3?.setHover(app.hover); });
  void refresh();
  return { vp2, sheet, refs, get view3() { return view3; }, refresh, showMsg };
}
