import type { App } from '../app';
import { fmtFtIn } from '../units';
import { h, clear } from './dom';

export function mountStatus(app: App, el: HTMLElement) {
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
    const cl = app.claude;
    const claude: (HTMLElement | string)[] = cl.state === 'BUSY'
      ? ['CLAUDE BUSY ', h('span.spin', '◐'), cl.round ? ` ROUND ${cl.round}` : '']
      : cl.state === 'ERR' ? [h('span', { class: 'bad' }, 'CLAUDE ERR')]
      : cl.state === 'NO KEY' ? [h('span', { class: 'warn' }, 'NO KEY')]
      : ['CLAUDE OK'];
    segs.forEach((s0, i) => {
      const opt = s0[0] === 'opt';
      const s = opt ? s0.slice(1) : s0;
      if (i) el.append(h('span.sep', { class: opt ? 'opt' : '' }, '▮'));
      el.append(h('span.seg', { class: opt ? 'opt' : '' }, ...s));
    });
    el.append(h('span.fill'), h('span.sep', '▮'), h('span.seg', ...claude));
  };
  for (const ev of ['doc', 'selection', 'hover', 'cursor', 'status', 'claude', 'view'] as const) app.on(ev, render);
  draw();
}
