import type { App } from '../app';
import { h, clear, btn, download } from './dom';
import { popup } from './popup';
import { MODELS, settings } from '../settings';
import type { KerfDoc } from '../types';

export interface SampleInfo { file: string; id: string; title: string }

export interface HeaderHooks {
  samples: SampleInfo[];
  openSample(s: SampleInfo): Promise<void>;
  keyChanged(): void;
  useDemo(): void;
  demoActive: boolean;
}

export function mountHeader(app: App, el: HTMLElement, hooks: HeaderHooks) {
  const doc = h('span.hf');
  const style = h('span.hf.opt');
  const engine = h('span.hf.eng');
  const sampleBtn = btn('OPEN SAMPLE ▾', () => {
    const menu = h('div.menu', h('div.mh', 'REFERENCE DETAILS'));
    let close = () => {};
    for (const s of hooks.samples) {
      menu.append(h('button.mi', { type: 'button', on: { click: () => { close(); void hooks.openSample(s); } } }, h('b', s.id), h('small', s.title)));
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
      const r = await app.openDoc(d, `file ${f.name}`);
      if (!r.ok) app.flash(`OPEN FAILED: ${r.error ?? r.diagnostics.map((x) => x.message).join('; ')}`.slice(0, 160), 'err');
      else app.flash(`OPENED ${f.name.toUpperCase()}`);
    } catch (e) { app.flash(`OPEN FAILED: ${(e as Error).message}`, 'err'); }
  });
  const openBtn = btn('OPEN', () => fileInput.click());
  const saveBtn = btn('SAVE', () => {
    if (!app.doc) { app.flash('NOTHING TO SAVE', 'warn'); return; }
    const text = JSON.stringify(app.doc, null, 2) + '\n';
    const name = `${app.doc.id}.kerf.json`;
    download(name, text, 'application/json');
    app.flash(`SAVED ${name.toUpperCase()} (${Math.max(1, Math.round(text.length / 1024))} KB)`);
  });
  const keyBtn = btn('KEY', () => openKeyDialog(keyBtn, hooks));

  el.append(
    h('div.wm', h('b', 'KERF'), h('span.bars', h('i'), h('i'), h('i'))),
    h('span.hsub', 'DETAIL WORKSTATION'),
    doc, style, engine, h('span.sp'),
    h('div.btns', sampleBtn, openBtn, saveBtn, keyBtn, fileInput),
  );
  const render = () => {
    clear(doc);
    doc.append(h('span', 'DOC:'), h('b', app.doc ? app.doc.id.toUpperCase() : '—'), '  ', h('span', 'REV'), h('b', String(app.opLog.length)));
    clear(style);
    style.append(h('span', 'STYLE:'), h('b', String((app.style as { id?: string })?.id ?? '?').toUpperCase()));
    clear(engine);
    engine.append(h('span', 'ENGINE:'), h('b', `${app.engine.info.engine}`.toUpperCase()), ` ${app.engine.info.version}`);
  };
  app.on('doc', render);
  app.on('log', render);
  render();
}

function openKeyDialog(anchor: HTMLElement, hooks: HeaderHooks) {
  const key = h('input', { type: 'password', autocomplete: 'off', spellcheck: false, placeholder: 'sk-ant-…', value: settings.apiKey });
  const model = h('select', ...MODELS.map((m) => h('option', { value: m.id, selected: m.id === settings.model }, m.label)));
  let close = () => {};
  const dlg = h('div.dlg',
    h('div.mh', 'CLAUDE SETTINGS'),
    h('div.db',
      h('div.field', h('label', 'ANTHROPIC API KEY'), key),
      h('div.field', h('label', 'MODEL'), model),
      h('div.note', 'THE KEY STAYS IN THIS BROWSER (LOCALSTORAGE) AND IS SENT ONLY TO API.ANTHROPIC.COM. IT IS NEVER LOGGED.'),
      h('div.acts',
        btn('SAVE', () => { settings.apiKey = key.value; settings.model = model.value; close(); hooks.keyChanged(); }, 'primary'),
        btn('CLEAR KEY', () => { settings.apiKey = ''; key.value = ''; hooks.keyChanged(); }),
        btn(hooks.demoActive ? 'DEMO ACTIVE' : 'USE DEMO', () => { close(); hooks.useDemo(); }),
      ),
    ),
  );
  close = popup(anchor, dlg, 'right');
  key.focus();
}
