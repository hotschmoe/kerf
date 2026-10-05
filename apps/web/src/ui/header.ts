import type { App } from '../app';
import { h, clear, btn, download } from './dom';
import { popup } from './popup';
import type { KerfDoc } from '../types';
import type { ChatSession } from '../chat/session';
import type { Workspace } from '../workspace/workspace';
import { openSetup } from './setup';

export interface SampleInfo { file: string; id: string; title: string }

export interface HeaderHooks {
  samples: SampleInfo[];
  openSample(s: SampleInfo): Promise<void>;
  session: ChatSession;
  /** workspace mode: samples and files are IMPORTED into the library folder instead of replacing an unsaved scratch document */
  ws: Workspace | null;
  loadSample(s: SampleInfo): Promise<KerfDoc>;
}

export function mountHeader(app: App, el: HTMLElement, hooks: HeaderHooks) {
  const ws = hooks.ws;
  const doc = h('span.hf.doc');
  const style = h('span.hf.opt');
  const engine = h('span.hf.eng');
  const dir = ws ? h('span.hf.dir', { title: ws.info.dir }, h('span', 'DIR:'), h('b', ws.info.dir.replace(/\/+$/, '').split('/').pop() || ws.info.dir)) : null;
  const sampleBtn = btn(ws ? 'ADD SAMPLE ▾' : 'OPEN SAMPLE ▾', () => {
    const menu = h('div.menu', h('div.mh', ws ? 'COPY A REFERENCE DETAIL INTO THE LIBRARY' : 'REFERENCE DETAILS'));
    let close = () => {};
    for (const s of hooks.samples) {
      menu.append(h('button.mi', { type: 'button', on: { click: () => { close(); void (ws ? hooks.loadSample(s).then((d) => ws.create(s.id, d.title, d)) : hooks.openSample(s)); } } }, h('b', s.id), h('small', s.title)));
    }
    close = popup(sampleBtn, menu, 'right');
  });
  const fileInput = h('input', { type: 'file', accept: '.json,.kerf.json,application/json', style: 'display:none' });
  fileInput.addEventListener('change', async () => {
    const f = fileInput.files?.[0];
    fileInput.value = '';
    if (!f) return;
    try {
      const d = JSON.parse(await f.text()) as KerfDoc;
      if (ws) { // import into the workspace folder
        const name = f.name.replace(/\.kerf\.json$|\.json$/i, '') || d.id;
        if (await ws.create(name, d.title, d)) app.flash(`ADDED ${f.name.toUpperCase()} TO THE LIBRARY`);
        return;
      }
      const r = await app.openDoc(d, `file ${f.name}`);
      if (!r.ok) app.flash(`OPEN FAILED: ${r.error ?? r.diagnostics.map((x) => x.message).join('; ')}`.slice(0, 160), 'err');
      else app.flash(`OPENED ${f.name.toUpperCase()}`);
    } catch (e) { app.flash(`OPEN FAILED: ${(e as Error).message}`, 'err'); }
  });
  const openBtn = btn(ws ? 'IMPORT' : 'OPEN', () => fileInput.click());
  const saveBtn = btn('SAVE', () => {
    if (!app.doc) { app.flash('NOTHING TO SAVE', 'warn'); return; }
    const text = JSON.stringify(app.doc, null, 2) + '\n';
    const name = `${app.doc.id}.kerf.json`;
    download(name, text, 'application/json');
    app.flash(`SAVED ${name.toUpperCase()} (${Math.max(1, Math.round(text.length / 1024))} KB)`);
  });
  const keyBtn = btn('SETUP', () => openSetup(keyBtn, hooks.session));
  keyBtn.id = 'setupbtn';

  el.append(
    h('div.wm', h('b', 'KERF'), h('span.bars', h('i'), h('i'), h('i'))),
    h('span.hsub', 'DETAIL WORKSTATION'),
    dir ?? '', doc, style, engine, h('span.sp'),
    h('div.btns', sampleBtn, openBtn, saveBtn, keyBtn, fileInput),
  );
  const render = () => {
    clear(doc);
    doc.append(h('span', 'DOC:'), h('b', app.doc ? app.doc.id.toUpperCase() : '—'), '  ', h('span', 'REV'), h('b', String(app.opLog.length)));
    clear(style);
    style.append(h('span', 'STYLE:'), h('b', String((app.style as { id?: string })?.id ?? '?').toUpperCase()));
    clear(engine);
    engine.append(h('span', 'ENGINE:'), h('b', `${app.engine.info.engine}`.toUpperCase()), ` ${app.engine.info.version}`, app.engine.compat?.size ? h('span', { title: 'Functions emulated in the browser because this engine build lacks them', style: 'color:#D98E04' }, ` (EMULATED: ${[...app.engine.compat].join(',')})`) : '');
  };
  app.on('doc', render);
  app.on('log', render);
  render();
}
