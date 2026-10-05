// Inspector: PARTS / NOTES / DIFF / DIAG tabs, selected-item detail (component params + anchors, note editor with
// citations and the designer-only VERIFIED toggle), and the export buttons.
import type { App } from '../app';
import type { Annotation, Citation, Diagnostic, Op } from '../types';
import { h, clear, btn } from './dom';
import { exportActive } from '../export';

type Tab = 'parts' | 'notes' | 'diff' | 'diag';

function fmtVal(v: unknown): string {
  if (v === null || v === undefined) return '—';
  if (typeof v === 'number') return String(Math.round(v * 1e4) / 1e4);
  if (typeof v === 'string' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) return v.every((x) => typeof x !== 'object' || x === null) ? `[${v.map(fmtVal).join(', ')}]` : v.map(fmtVal).join('; ');
  const o = v as Record<string, unknown>;
  return Object.entries(o).map(([k, x]) => `${k}: ${fmtVal(x)}`).join(', ');
}

export function mountInspector(app: App, el: HTMLElement) {
  let tab: Tab = 'parts';
  let compact: boolean | null = null; // null = automatic (compact when there are many parts)
  const phRight = h('span.r');
  const tabsEl = h('div.itabs');
  const body = h('div.ibody');
  const detail = h('div.idetail');
  const exportRow = h('div.iexport', h('span.lbl', 'EXPORT'),
    btn('DXF', () => void exportActive(app, 'dxf'), 'sm'), btn('PDF', () => void exportActive(app, 'pdf'), 'sm'), btn('SVG', () => void exportActive(app, 'svg'), 'sm'),
    app.remote ? btn('PNG', () => void exportActive(app, 'png'), 'sm') : null);
  el.append(h('div.ph', h('span', 'INSPECTOR'), phRight), tabsEl, body, detail, exportRow);

  const setTab = (t: Tab) => { tab = t; renderTabs(); renderBody(); };
  function renderTabs() {
    clear(tabsEl);
    const nd = app.diagnostics.length;
    const defs: [Tab, string][] = [['parts', 'PARTS'], ['notes', 'NOTES'], ['diff', 'DIFF'], ['diag', nd ? `DIAG ${nd}` : 'DIAG']];
    for (const [t, l] of defs) tabsEl.append(h('button.tab', { type: 'button', class: tab === t ? 'on' : '', on: { click: () => setTab(t) } }, `[${l}]`));
    clear(phRight);
    phRight.append(app.view ? `VIEW ${app.view.id}` : '');
  }

  const selId = () => app.selection?.id ?? null;

  function renderBody() {
    clear(body);
    if (tab === 'parts') body.append(partsTable());
    else if (tab === 'notes') body.append(notesTable());
    else if (tab === 'diff') body.append(diffList());
    else body.append(diagList());
  }

  function partsTable(): HTMLElement {
    const rows = app.partsRows();
    if (!rows.length) return h('div.empty', 'NO COMPONENTS.');
    const isCompact = compact ?? rows.length > 12;
    const bar = h('div.logbar', h('span', `${rows.length} PART${rows.length === 1 ? '' : 'S'}`),
      btn(isCompact ? 'EXPAND' : 'COMPACT', () => { compact = !isCompact; renderBody(); }, 'sm'));
    const wrap = h('div', bar);
    const t = h('table.t.' + (isCompact ? 'compact' : 'full'), h('thead', h('tr', h('th', 'NO'), h('th', 'ID'), h('th', 'TYPE'))));
    const tb = h('tbody');
    for (const r of rows) {
      const base = r.id.replace(/#\d+$/, '');
      const tr = h('tr.row', { class: selId() === base ? 'sel' : '', tabindex: 0, on: { click: () => app.setSelection(base), keydown: ((e: KeyboardEvent) => { if (e.key === 'Enter') app.setSelection(base); }) as EventListener } },
        h('td.no', r.no), h('td.id', r.id, r.params && !isCompact ? h('span.sub', r.params) : null), h('td.ty', isCompact ? [r.type, r.params].filter(Boolean).join(' ') : r.type));
      tr.title = r.raw.trim();
      tr.dataset.id = base;
      tr.addEventListener('mouseenter', () => app.setHover(base));
      tr.addEventListener('mouseleave', () => app.setHover(null));
      tb.append(tr);
    }
    t.append(tb);
    wrap.append(t);
    return wrap;
  }

  function notes(): Annotation[] { return app.view?.annotations ?? []; }

  function notesTable(): HTMLElement {
    const list = notes();
    if (!list.length) return h('div.empty', app.view ? 'THIS VIEW HAS NO NOTES.' : 'NO VIEW.');
    const t = h('table.t', h('thead', h('tr', h('th', 'NO'), h('th', 'NOTE'), h('th', 'TYPE'))));
    const tb = h('tbody');
    list.forEach((a, i) => {
      const unv = (a.cite ?? []).some((c) => c.status !== 'verified');
      const tr = h('tr.row', { class: selId() === a.id ? 'sel' : '', tabindex: 0, on: { click: () => app.setSelection(a.id) } },
        h('td.no', String(i + 1).padStart(2, '0')),
        h('td.id', h('span', { style: 'font-weight:400' }, a.text ?? (a.type === 'dim' ? `(dim ${a.id})` : a.id)), h('span.sub', a.id + (unv ? '  *' : ''))),
        h('td.ty', a.type));
      tb.append(tr);
    });
    t.append(tb);
    return t;
  }

  function diffList(): HTMLElement {
    const wrap = h('div');
    const canUndo = app.opLog.some((e) => !e.undone && e.kind !== 'undo');
    const undo = btn('UNDO LAST', async () => { const ok = await app.undo(); if (!ok) app.flash('NOTHING TO UNDO', 'warn'); }, 'sm');
    undo.disabled = !canUndo;
    wrap.append(h('div.logbar', h('span', `${app.opLog.length} OP GROUP${app.opLog.length === 1 ? '' : 'S'}`), undo));
    if (!app.opLog.length) { wrap.append(h('div.empty', 'NO EDITS YET.')); return wrap; }
    for (const e of [...app.opLog].reverse()) {
      const lines = e.ops.map((o) => `${o.op.toUpperCase()} ${o.path}`);
      wrap.append(h('details.logrow', { class: e.undone ? 'undone' : '' },
        h('summary', h('span.ts', String(e.n).padStart(2, '0')), h('span', h('span.who', { class: e.who }, e.who), h('br'), h('span.ts', e.ts)), h('span.why', e.why)),
        lines.length ? h('ul', ...lines.slice(0, 40).map((l) => h('li', l))) : null,
        e.changed.length ? h('ul', h('li', 'CHANGED: ' + e.changed.join(', '))) : null));
    }
    return wrap;
  }

  function diagList(): HTMLElement {
    const ds = app.diagnostics;
    if (!ds.length) return h('div.empty', 'NO DIAGNOSTICS. 0 ERR 0 WARN.');
    const wrap = h('div');
    for (const d of ds) wrap.append(diagRow(d));
    return wrap;
  }
  function diagRow(d: Diagnostic): HTMLElement {
    const L = d.level === 'error' ? 'E' : d.level === 'warning' ? 'W' : 'I';
    const target = d.id ? d.id.replace(/#\d+$/, '') : '';
    return h('div.diag', { title: target ? `Select ${target}` : '', on: { click: () => { if (target) app.setSelection(target); } } },
      h('span.dtag', { class: L }, L),
      h('div', h('div.dc', d.code + (d.id ? ` · ${d.id}` : '')), h('div.dm', d.message), d.fix ? h('div.df', 'FIX: ' + d.fix) : null));
  }

  // ---------- detail pane ----------
  let detailToken = 0;
  async function renderDetail() {
    const t = ++detailToken;
    clear(detail);
    const sel = app.selection;
    if (!sel || !app.doc) return;
    if (sel.kind === 'note') { const a = notes().find((x) => x.id === sel.id); if (a) detail.append(noteEditor(a)); return; }
    const c = app.doc.components.find((x) => x.id === sel.id);
    if (!c) return;
    const sec = h('div.dsec', h('h3', h('span', c.id), h('small', c.type)));
    detail.append(sec);
    const kv = h('div.kv');
    sec.append(kv);
    // ordered field map: declared document fields first, overridden/extended by what the engine resolves
    const fields = new Map<string, unknown>();
    const prow = app.partsRows().find((r) => r.id === c.id || r.id === `${c.id}#0`);
    if (prow) { fields.set('x extent', prow.x); fields.set('y extent', prow.y); }
    for (const [k, v] of Object.entries(c)) if (k !== 'id' && k !== 'type') fields.set(k, v);
    // scalar fields written in the document (size, length, label, ...) are editable: one `update components/<id>` op per change (designer)
    const editable = new Set(Object.entries(c).filter(([k, v]) => k !== 'id' && k !== 'type' && (typeof v === 'string' || typeof v === 'number')).map(([k]) => k));
    const editCell = (k: string, v: unknown): HTMLElement => {
      const inp = h('input.pe', { value: String(v), spellcheck: false, 'aria-label': `Edit ${k}`, 'data-key': k }) as HTMLInputElement;
      const commit = () => {
        const raw = inp.value.trim();
        if (raw === String(v) || raw === '') { inp.value = String(v); return; }
        const val = typeof v === 'number' && Number.isFinite(Number(raw)) ? Number(raw) : raw;
        void app.applyOps([{ op: 'update', path: `components/${c.id}`, value: { [k]: val } }], 'designer', `Set ${c.id} ${k} to ${raw}`).then((r) => { if (!r.ok) { app.rejected(r, 'EDIT REJECTED'); inp.value = String(v); } });
      };
      inp.addEventListener('change', commit);
      inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); commit(); } });
      return h('div.vv', inp);
    };
    const paint = () => { clear(kv); for (const [k, v] of fields) kv.append(h('div.k', k.replace(/_/g, ' ')), editable.has(k) && fields.get(k) === c[k] ? editCell(k, v) : h('div.vv', fmtVal(v))); };
    paint();
    try {
      const r = (await app.inspect({ q: 'component', id: c.id })) as unknown;
      if (t !== detailToken) return;
      if (r && typeof r === 'object') {
        const o = r as Record<string, unknown>;
        const params = (o.params ?? null) as Record<string, unknown> | null;
        if (params && typeof params === 'object') for (const [k, v] of Object.entries(params)) if (k !== 'id' && k !== 'type') fields.set(k, v);
        for (const [k, v] of Object.entries(o)) if (!['q', 'anchors', 'id', 'type', 'params', 'component'].includes(k)) fields.set(k, v);
        paint();
        const anchors = (o.anchors ?? null) as unknown;
        if (anchors && typeof anchors === 'object') {
          sec.append(h('div.subh', 'ANCHORS'));
          const ak = h('div.kv');
          const entries = Array.isArray(anchors) ? (anchors as { name?: string }[]).map((a, i) => [a.name ?? String(i), a] as [string, unknown]) : Object.entries(anchors as Record<string, unknown>);
          for (const [k, v] of entries) {
            const val = v as Record<string, unknown>;
            const txt = val && typeof val === 'object' && !Array.isArray(val) ? Object.entries(val).filter(([kk]) => kk !== 'name').map(([, x]) => fmtVal(x)).join('  ') : fmtVal(v);
            ak.append(h('div.k', k.replace(/_/g, ' ')), h('div.vv', txt));
          }
          sec.append(ak);
        }
      } else if (typeof r === 'string') sec.append(h('pre', { style: 'font-size:11px;white-space:pre-wrap' }, r));
    } catch (e) {
      if (t === detailToken) sec.append(h('div.sub', `INSPECT: ${app.describeError(e)}`));
    }
  }

  function patchNote(a: Annotation, value: Record<string, unknown>, why: string) {
    const v = app.view;
    if (!v) return;
    const ops: Op[] = [{ op: 'update', path: `views/${v.id}/annotations/${a.id}`, value }];
    void app.applyOps(ops, 'designer', why).then((r) => { if (!r.ok) app.rejected(r, 'EDIT REJECTED'); });
  }

  function noteEditor(a: Annotation): HTMLElement {
    const jur = (app.doc?.meta?.jurisdiction ?? {}) as { code?: string; edition?: number };
    const sec = h('div.dsec', h('h3', h('span', a.id), h('small', a.type)));
    const text = h('textarea', { value: a.text ?? '', spellcheck: false, rows: 3, placeholder: a.type === 'dim' ? '(AUTOMATIC DIMENSION TEXT)' : '' });
    const commit = () => {
      const v = text.value.replace(/\s+$/, '');
      if (v === (a.text ?? '')) return;
      patchNote(a, { text: v === '' && a.type === 'dim' ? null : v }, `Edit ${a.type} ${a.id} text`);
    };
    text.addEventListener('change', commit);
    text.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); commit(); } });
    sec.append(h('div.field', h('label', 'TEXT'), text));
    if (a.target) sec.append(h('div.field', h('label', 'TARGET'), h('div.v.ro', String(a.target))));
    if (a.type === 'note') {
      const pl = a.place;
      sec.append(h('div.field', h('label', 'PLACE (MODEL IN.)'), h('div.v.ro', pl ? `${pl[0]}, ${pl[1]}   ` : 'AUTO LAYOUT  ', pl ? btn('CLEAR', () => patchNote(a, { place: null }, `Reset note ${a.id} to automatic placement`), 'sm') : null)));
      sec.append(h('div.subh', 'CODE CITATIONS'));
      const cites = (a.cite ?? []) as Citation[];
      const setCites = (next: Citation[], why: string) => patchNote(a, { cite: next }, why);
      if (!cites.length) sec.append(h('div.sub', 'NONE'));
      cites.forEach((c, i) => {
        const verified = c.status === 'verified';
        const stamp = h('button.stamp', { class: verified ? 'ok' : '', type: 'button', title: verified ? 'Click to mark unverified' : 'Click to verify (designer only)',
          on: { click: () => setCites(cites.map((x, j) => (j === i ? { ...x, status: verified ? 'suggested' : 'verified' } : x)), `${verified ? 'Unverify' : 'Verify'} ${c.code} ${c.section ?? ''} on ${a.id}`) } }, verified ? 'VERIFIED' : 'UNVERIFIED');
        const rm = h('button.x', { type: 'button', title: 'Remove citation', on: { click: () => setCites(cites.filter((_, j) => j !== i), `Remove citation ${c.code} ${c.section ?? ''} from ${a.id}`) } }, '×');
        sec.append(h('div.cite', h('div.ct', `${c.code} ${c.section ?? ''}`.trim()), h('div.ca', stamp, rm), h('div.cs', [c.edition ? String(c.edition) : '', c.title ?? ''].filter(Boolean).join(' · '))));
      });
      const code = h('input', { value: jur.code ?? 'IRC', 'aria-label': 'Code' });
      const ed = h('input', { value: jur.edition ? String(jur.edition) : '2021', 'aria-label': 'Edition' });
      const section = h('input', { placeholder: 'R403.1.6', 'aria-label': 'Section' });
      const title = h('input', { placeholder: 'Foundation anchorage', 'aria-label': 'Title' });
      sec.append(
        h('div.addcite', h('div.field', h('label', 'CODE'), code), h('div.field', h('label', 'ED.'), ed), h('div.field', h('label', 'SECTION'), section)),
        h('div.addcite', { style: 'grid-template-columns:1fr auto;align-items:end' }, h('div.field', h('label', 'TITLE'), title),
          btn('ADD', () => {
            if (!section.value.trim()) { app.flash('CITATION NEEDS A SECTION', 'warn'); section.focus(); return; }
            const c: Citation = { code: code.value.trim() || 'IRC', section: section.value.trim(), status: 'suggested' };
            const e = parseInt(ed.value, 10); if (e) c.edition = e;
            if (title.value.trim()) c.title = title.value.trim();
            setCites([...cites, c], `Add citation ${c.code} ${c.section} to ${a.id}`);
          }, 'sm')),
      );
    }
    return sec;
  }

  const rerender = () => { renderTabs(); renderBody(); void renderDetail(); };
  app.on('doc', rerender);
  app.on('log', () => { if (tab === 'diff') renderBody(); });
  app.on('view', () => { renderTabs(); if (tab === 'notes') renderBody(); void renderDetail(); });
  app.on('selection', () => {
    for (const tr of Array.from(body.querySelectorAll('tr.row'))) tr.classList.remove('sel');
    if (tab === 'parts' || tab === 'notes') renderBody();
    void renderDetail();
    // bring the selected row into view
    body.querySelector('tr.row.sel')?.scrollIntoView({ block: 'nearest' });
  });
  rerender();
  return { setTab };
}
