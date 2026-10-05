import type { App } from '../app';
import { fmtFtIn } from '../units';
import { h, clear } from './dom';
import type { Workspace } from '../workspace/workspace';

/** 12S / 3M / 2H */
export function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return s < 90 ? `${s}S` : s < 5400 ? `${Math.round(s / 60)}M` : `${Math.round(s / 3600)}H`;
}

export function mountStatus(app: App, el: HTMLElement, ws: Workspace | null = null) {
  let raf = 0;
  const render = () => { if (!raf) raf = requestAnimationFrame(() => { raf = 0; draw(); }); };
  const draw = () => {
    clear(el);
    const c = app.counts();
    const segs: (HTMLElement | string)[][] = [];  // an optional leading 'opt' marks segments hidden on narrow screens
    let state: HTMLElement;
    if (app.message) state = h('span', { class: app.message.level === 'err' ? 'bad' : app.message.level === 'warn' ? 'warn' : '' }, app.message.text);
    else if (app.claude.state === 'BUSY') state = h('span', 'WORKING');
    else if (app.hover) state = h('span', `HOVER ${app.hover}`);
    else if (app.selection) state = h('span', `SELECTED ${app.selection.id}`);
    else state = h('span', app.doc ? 'READY' : 'NO DETAIL LOADED');
    segs.push([state]);
    segs.push(['opt', h('span', `${c.components} COMPONENT${c.components === 1 ? '' : 'S'}`)]);
    segs.push([h('span', { class: c.err ? 'bad' : '' }, `${c.err} ERR`), ' ', h('span', { class: c.warn ? 'warn' : '' }, `${c.warn} WARN`)]);
    const v = app.view;
    if (v && app.mode === 'view') {
      const scale = typeof v.scale === 'string' ? v.scale : '';
      segs.push(['opt', h('span', `VIEW ${v.id}${scale ? ' ' + scale : ''}`)]);
    } else if (app.mode === '3d') segs.push(['opt', h('span', 'VIEW 3D')]);
    else if (app.mode === 'sheet') segs.push(['opt', h('span', `SHEET ${app.activeView}`)]);
    if (app.cursor && app.mode === 'view') segs.push(['opt', h('span', `X ${fmtFtIn(app.cursor[0])} Y ${fmtFtIn(app.cursor[1])}`)]);
    if (ws) {
      if (!ws.online) segs.push([h('span', { class: 'bad' }, 'WORKSPACE OFFLINE')]);
      if (ws.lastAgentEdit !== null) segs.push(['opt', h('span.agentseg', `LOCAL AGENT · LAST EDIT ${ago(Date.now() - ws.lastAgentEdit)} AGO`)]);
    }
    const cl = app.claude;
    const pl = app.providerLabel;
    const claude: (HTMLElement | string)[] = cl.state === 'BUSY'
      ? [`${pl} BUSY `, h('span.spin', '◐'), cl.round ? ` ROUND ${cl.round}` : '']
      : cl.state === 'ERR' ? [h('span', { class: 'bad' }, `${pl} ERR`)]
      : cl.state === 'NO KEY' ? [h('span', { class: 'warn' }, 'NO KEY')]
      : [`${pl} OK`];
    segs.forEach((s0, i) => {
      const opt = s0[0] === 'opt';
      const s = opt ? s0.slice(1) : s0;
      if (i) el.append(h('span.sep', { class: opt ? 'opt' : '' }, '▮'));
      el.append(h('span.seg', { class: opt ? 'opt' : '' }, ...s));
    });
    el.append(h('span.fill'), h('span.sep', '▮'), h('span.seg', ...claude));
  };
  for (const ev of ['doc', 'selection', 'hover', 'cursor', 'status', 'claude', 'view'] as const) app.on(ev, render);
  if (ws) {
    ws.on('online', render); ws.on('agent-edit', render);
    setInterval(() => { if (ws.lastAgentEdit !== null) render(); }, 1000); // the "N S AGO" counter
  }
  draw();
}
