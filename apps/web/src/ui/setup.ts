// Provider setup: a typed-form panel (UPPERCASE labels over ruled cells) for the chat provider, key, model and base URL.
import type { ChatSession } from '../chat/session';
import { PROVIDERS, AGENT_PREFIX, agentId, isAgentChoice, providerDef, store } from '../chat/providers';
import { h, clear, btn } from './dom';
import { popup } from './popup';

export function openSetup(anchor: HTMLElement, session: ChatSession): () => void {
  let close = () => {};
  let choice = session.choice;
  const draft: Record<string, { key: string; model: string; base: string; vision: boolean }> = {};
  const get = (id: string) => (draft[id] ??= { key: store.key(id), model: store.model(id), base: store.baseUrl(id), vision: store.vision(id) });

  const sel = h('select', { 'aria-label': 'Provider' }) as HTMLSelectElement;
  const ags = session.agents();
  if (ags.length) {
    const g = h('optgroup', { label: 'LOCAL AGENT (NO KEY)' });
    for (const a of ags) g.append(h('option', { value: AGENT_PREFIX + a.id, disabled: !a.available }, `${a.name.toUpperCase()}${a.available ? '' : ' (NOT FOUND)'}`));
    sel.append(g);
  }
  const g2 = h('optgroup', { label: 'API (YOUR KEY)' });
  for (const p of PROVIDERS) g2.append(h('option', { value: p.id }, p.label.toUpperCase()));
  sel.append(g2);
  sel.value = choice;

  const form = h('div.sform');
  const route = session.viaServer
    ? 'API CALLS GO THROUGH YOUR LOCAL KERF SERVER (POST /API/LLM): NO BROWSER CORS LIMITS. THE KEY IS FORWARDED FOR EACH CALL; THE SERVER NEVER STORES OR LOGS IT.'
    : 'API CALLS GO STRAIGHT FROM THIS BROWSER TO THE PROVIDER. IF A PROVIDER REFUSES BROWSER ORIGINS (CORS) THE CONSOLE SAYS SO; `kerf serve` REMOVES THAT LIMIT.';

  const body = () => {
    clear(form);
    if (isAgentChoice(choice)) {
      const a = ags.find((x) => x.id === agentId(choice));
      const sess = store.session(agentId(choice), session.file);
      form.append(
        h('div.field', h('label', 'AGENT'), h('div.v.ro', `${(a?.name ?? agentId(choice)).toUpperCase()}${a?.version ? '  v' + a.version : ''}`)),
        h('div.field', h('label', 'STATUS'), h('div.v.ro', a?.available ? 'FOUND ON THIS MACHINE' : 'NOT FOUND' + (a?.reason ? ': ' + a.reason : ''))),
        h('div.field', h('label', 'SESSION (THIS DETAIL)'), h('div.v.ro', sess ? sess.slice(0, 18) + '…' : 'NONE YET')),
        h('div.note', 'RUNS THE AGENT CLI HEADLESS IN THE WORKSPACE FOLDER WITH YOUR OWN LOGIN OR SUBSCRIPTION. NO API KEY. IT EDITS THE FILES WITH THE kerf CLI AND THIS SCREEN UPDATES AS IT WRITES. THE SESSION IS RESUMED ON THE NEXT MESSAGE; NEW (CONSOLE HEADER) STARTS A FRESH ONE.'),
      );
      return;
    }
    const def = providerDef(choice)!;
    const d = get(def.id);
    const key = h('input', { type: 'password', autocomplete: 'off', spellcheck: false, placeholder: def.keyHint, value: d.key, 'aria-label': 'API key' });
    key.addEventListener('input', () => { d.key = key.value; });
    const dl = h('datalist', { id: 'models-' + def.id }, ...def.models.map((m) => h('option', { value: m.id }, m.label)));
    const model = h('input', { list: 'models-' + def.id, spellcheck: false, value: d.model, placeholder: def.id === 'custom' ? 'llama3.3, qwen3, …' : def.defaultModel, 'aria-label': 'Model' });
    model.addEventListener('input', () => { d.model = model.value; });
    form.append(
      h('div.field', h('label', `${def.id === 'custom' ? 'API KEY (OPTIONAL)' : 'API KEY'}${def.keyUrl ? '   ' + def.keyUrl.toUpperCase() : ''}`), key),
      h('div.field', h('label', 'MODEL (EDITABLE)'), model, dl),
    );
    if (def.id === 'custom') {
      const base = h('input', { spellcheck: false, value: d.base, placeholder: 'http://localhost:11434/v1', 'aria-label': 'Base URL' });
      base.addEventListener('input', () => { d.base = base.value; });
      const vis = h('button.chk', { type: 'button', on: { click: () => { d.vision = !d.vision; vis.textContent = d.vision ? '[X]' : '[ ]'; } } }, d.vision ? '[X]' : '[ ]');
      form.append(
        h('div.field', h('label', 'BASE URL (…/v1, THE PATH /chat/completions IS ADDED)'), base),
        h('div.field', h('label', 'VISION'), h('div.v', vis, ' THE MODEL ACCEPTS IMAGES')),
      );
    } else if (def.id !== 'anthropic') {
      form.append(h('div.field', h('label', 'ENDPOINT'), h('div.v.ro', def.baseUrl + def.path)));
    }
    form.append(h('div.note', route), h('div.note', 'THE KEY STAYS IN THIS BROWSER (LOCALSTORAGE, ONE PER PROVIDER) AND IS NEVER LOGGED.'));
  };
  sel.addEventListener('change', () => { choice = sel.value; body(); });
  body();

  const save = () => {
    if (!isAgentChoice(choice)) {
      const d = get(choice);
      store.setKey(choice, d.key);
      store.setModel(choice, d.model);
      if (choice === 'custom') { store.setBaseUrl(d.base); store.setVision(d.vision); }
    }
    session.setChoice(choice);
    session.refresh();
    close();
  };
  const dlg = h('div.dlg.setup',
    h('div.mh', 'PROVIDER SETUP'),
    h('div.db',
      h('div.field', h('label', 'PROVIDER'), sel),
      form,
      h('div.acts',
        btn('SAVE', save, 'primary'),
        btn('CLEAR KEY', () => { if (!isAgentChoice(choice)) { get(choice).key = ''; store.setKey(choice, ''); body(); session.refresh(); } }),
        btn(session.demo ? 'DEMO ACTIVE' : 'USE DEMO', () => { close(); session.setDemo(true); }),
      ),
    ),
  );
  close = popup(anchor, dlg, 'right');
  return close;
}
