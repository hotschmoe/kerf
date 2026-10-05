// LIBRARY: the workspace's documents (workspace mode only), live from the server; sits above the console messages.
import type { Workspace } from '../workspace/workspace';
import { slug } from '../workspace/workspace';
import { h, clear, btn } from './dom';
import { popup } from './popup';

const K_COLLAPSED = 'kerf.libCollapsed';
const lsGet = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
const lsSet = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* ignore */ } };

export function mountLibrary(ws: Workspace, el: HTMLElement) {
  let collapsed = lsGet(K_COLLAPSED) === '1';
  const list = h('div.liblist');
  const count = h('span.cnt');
  const toggle = h('button.libtoggle', { type: 'button', title: 'Collapse / expand', on: { click: () => { collapsed = !collapsed; lsSet(K_COLLAPSED, collapsed ? '1' : '0'); render(); } } }, '');
  const newBtn = btn('+ NEW', () => newDialog(newBtn), 'sm');
  newBtn.id = 'libnew';
  el.append(h('div.libhead', toggle, h('span.lt', 'LIBRARY'), count, h('span.sp'), newBtn), list);
  el.classList.add('library');

  function render() {
    el.classList.toggle('collapsed', collapsed);
    toggle.textContent = collapsed ? '▸' : '▾';
    clear(count);
    count.append(`${ws.docs.length} DOC${ws.docs.length === 1 ? '' : 'S'}`);
    clear(list);
    if (!ws.docs.length) { list.append(h('div.empty', 'NO DETAILS IN THIS FOLDER YET. + NEW, OR ASK THE AGENT.')); return; }
    ws.docs.forEach((d, i) => {
      const cur = d.file === ws.file;
      const stem = d.file.replace(/\.kerf\.json$/, '');
      list.append(h('div.librow', {
        class: cur ? 'sel' : '', tabindex: 0, title: `${d.file}\n${d.title ?? ''}\n${d.components} components, ${d.views} views`, 'data-file': d.file,
        on: { click: () => void ws.open(d.file), keydown: ((e: KeyboardEvent) => { if (e.key === 'Enter') void ws.open(d.file); }) as EventListener },
      },
      h('span.no', String(i + 1).padStart(2, '0')),
      h('span.nm', stem),
      h('span.cn', d.errors ? h('span.dtag.E', String(d.errors)) : h('span.z', '·'), d.warnings ? h('span.dtag.W', String(d.warnings)) : h('span.z', '·'))));
    });
  }

  function newDialog(anchor: HTMLElement) {
    const file = h('input', { placeholder: 'truss-bearing-cmu', spellcheck: false, 'aria-label': 'File name' });
    const title = h('input', { placeholder: 'PREFAB TRUSS BEARING AT CMU', spellcheck: false, 'aria-label': 'Title' });
    const hint = h('div.note', '');
    const upd = () => { hint.textContent = file.value.trim() ? `→ ${slug(file.value)}.kerf.json` : 'CREATES <NAME>.KERF.JSON IN THE WORKSPACE FOLDER'; };
    file.addEventListener('input', upd); upd();
    let close = () => {};
    const go = async () => {
      if (!file.value.trim()) { file.focus(); return; }
      close();
      await ws.create(file.value, title.value.trim() || undefined);
    };
    for (const i of [file, title]) i.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Enter') void go(); });
    const dlg = h('div.dlg',
      h('div.mh', 'NEW DETAIL'),
      h('div.db', h('div.field', h('label', 'FILE NAME'), file), h('div.field', h('label', 'TITLE (OPTIONAL)'), title), hint,
        h('div.acts', btn('CREATE', () => void go(), 'primary'), btn('CANCEL', () => close()))));
    close = popup(anchor, dlg, 'left');
    file.focus();
  }

  ws.on('docs', render);
  ws.on('file', render);
  render();
}
