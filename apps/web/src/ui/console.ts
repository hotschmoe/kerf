// Operator console: chat history (designer plain, KERF/CLAUDE manila cards with tool activity) + composer.
import type { App } from '../app';
import type { ChatEvent } from '../chat/harness';
import type { ChatSession } from '../chat/session';
import { PROVIDERS, AGENT_PREFIX } from '../chat/providers';
import { blobToImageBlock } from '../chat/images';
import type { ImageBlock } from '../chat/transport';
import type { AgentCard } from '../workspace/workspace';
import { h, btn, hhmm, clear } from './dom';

/** Tool input for display: a whole-document `set` collapses to a one-line stand-in. */
export function abbrevInput(input: unknown): string {
  return JSON.stringify(input, (k, v) => {
    if (k === 'ops' && Array.isArray(v)) {
      return v.map((o) => (o && o.op === 'set' && o.path === 'doc' && o.value && typeof o.value === 'object'
        ? { ...o, value: `<document ${o.value.id ?? ''}: ${o.value.components?.length ?? 0} components, ${o.value.views?.length ?? 0} views>` } : o));
    }
    return v;
  }, 2);
}

/** Minimal inline formatting for Claude's replies: **bold** and `code`. Everything else stays plain text. */
export function fmtInline(text: string): (Node | string)[] {
  const out: (Node | string)[] = [];
  const re = /\*\*([^*\n]+)\*\*|`([^`\n]+)`/g;
  let last = 0, m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(m[1] ? h('b', m[1]) : h('code', m[2]));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

interface Pending { block: ImageBlock; thumbUrl: string }

export interface ConsoleHooks {
  openSetup(): void;
}

/** LOCAL AGENT card for an op-log entry written by someone other than this browser (the user's agent, a terminal `kerf apply`). */
export function agentLogCard(c: AgentCard): HTMLElement {
  const e = c.entry;
  const n = e.ops?.length ?? 0;
  const body = h('div.mb', h('div.why', e.why || '(no reason given)'));
  body.append(h('div.meta', `${c.file}  ·  ${n} OP${n === 1 ? '' : 'S'}${e.changed?.length ? '  ·  CHANGED ' + e.changed.slice(0, 6).join(', ') + (e.changed.length > 6 ? ' …' : '') : ''}`));
  if (e.summary_head) body.append(h('div.sum', e.summary_head));
  if (n) {
    body.append(h('details.tl', h('summary', h('span.tt', `OPS (${n})`), h('span.ts', '')),
      h('pre', JSON.stringify(e.ops, (k, v) => (k === 'value' && v && typeof v === 'object' && Object.keys(v).length > 8 ? { '…': `${Object.keys(v).length} fields` } : v), 2))));
  }
  const when = e.ts ? new Date(e.ts) : new Date();
  return h('div.msg.llm.agent', h('div.mh', h('span', `LOCAL AGENT${e.who && e.who !== 'agent' ? ' · ' + e.who.toUpperCase() : ''}`), h('span', hhmm(isNaN(+when) ? new Date() : when))), body);
}

export function mountConsole(app: App, el: HTMLElement, session: ChatSession, hooks: ConsoleHooks) {
  const msgs = h('div#msgs');
  const ta = h('textarea', { rows: 3, placeholder: 'DESCRIBE A DETAIL, OR PASTE A SCREENSHOT TO RECREATE', spellcheck: false, 'aria-label': 'Message to the assistant' });
  const attached = h('div#attached');
  const sendBtn = btn('SEND', () => send(), 'primary');
  const attachBtn = btn('ATTACH', () => file.click());
  const file = h('input', { type: 'file', accept: 'image/png,image/jpeg,image/webp,image/gif', multiple: true, style: 'display:none' });
  const hint = h('span.hint', 'ENTER SENDS · SHIFT+ENTER NEWLINE');
  const pending: Pending[] = [];

  const picker = h('select.pick', { 'aria-label': 'Chat provider', title: 'Chat provider' }) as HTMLSelectElement;
  const newBtn = btn('NEW', () => {
    session.newConversation();
    msgs.append(h('div.notice', 'NEW CONVERSATION. THE MODEL STARTS FROM THE DOCUMENT ON SCREEN.'));
    scroll();
  }, 'sm');
  newBtn.title = 'Forget the conversation (and the local agent session for this document)';
  const fillPicker = () => {
    clear(picker);
    const ags = session.agents();
    if (ags.length) {
      const g = h('optgroup', { label: 'LOCAL AGENT (YOUR LOGIN)' });
      for (const a of ags) g.append(h('option', { value: AGENT_PREFIX + a.id, disabled: !a.available }, `${a.name.toUpperCase()}${a.available ? '' : ' (NOT FOUND)'}`));
      picker.append(g);
    }
    const c = h('optgroup', { label: ags.length ? 'API (YOUR KEY)' : 'PROVIDER (YOUR KEY)' });
    for (const p of PROVIDERS) c.append(h('option', { value: p.id }, session.shortLabel(p.id)));
    picker.append(c);
    picker.value = session.choice;
  };
  fillPicker();
  picker.addEventListener('change', () => {
    session.setChoice(picker.value);
    const r = session.ready();
    if (!r.ok) { msgs.append(h('div.notice.warn', r.why)); scroll(); hooks.openSetup(); }
  });
  session.onChange(() => { fillPicker(); renderIntro(); syncComposer(); });
  el.append(
    h('div.ph', h('span', 'OPERATOR CONSOLE'), h('span.r', picker, newBtn)),
    msgs,
    h('div#composer', attached, h('div.prompt', h('span.gt', '>'), ta), h('div.row', attachBtn, hint, h('span.sp'), sendBtn), file),
  );

  const intro = h('div.intro');
  const renderIntro = () => {
    clear(intro);
    const r = session.ready();
    intro.append(
      h('b', 'KERF OPERATOR CONSOLE. '),
      r.ok ? (session.isAgent && !session.demo
        ? 'Describe the change you want. Your local agent edits the detail files in this folder with the kerf CLI; you review and edit them at right.'
        : 'Describe the construction detail you need, or attach a screenshot of one to recreate. The assistant builds it with the engine; you review and edit it at right.')
        : `${r.why} You can still open a detail and edit its notes.`,
    );
  };
  renderIntro();
  msgs.append(intro);

  let card: HTMLElement | null = null;
  let cardBody: HTMLElement | null = null;
  let seg: HTMLElement | null = null;
  let tools: HTMLElement | null = null;
  const toolEls = new Map<string, { det: HTMLElement; status: HTMLElement; title: HTMLElement }>();
  let stick = true;
  msgs.addEventListener('scroll', () => { stick = msgs.scrollTop + msgs.clientHeight >= msgs.scrollHeight - 24; });
  const scroll = () => { if (stick) msgs.scrollTop = msgs.scrollHeight; };

  const newCard = (who = 'KERF/CLAUDE') => {
    cardBody = h('div.mb');
    card = h('div.msg.llm', { class: who.startsWith('LOCAL AGENT') ? 'agent' : '' }, h('div.mh', h('span', who), h('span', hhmm())), cardBody);
    msgs.append(card);
    seg = null; tools = null;
  };

  session.on((e: ChatEvent) => {
    switch (e.type) {
      case 'user': {
        intro.remove();
        const m = h('div.msg', h('div.mh', h('span', 'DESIGNER'), h('span', hhmm())), h('div.mb', e.text));
        if (e.images.length) m.append(h('div.atts', ...e.images.map((u) => h('img', { src: u, alt: 'attachment' }))));
        if (e.edits.length) m.append(h('div.edits', `EDITS REPORTED TO CLAUDE: ${e.edits.join('; ')}`));
        msgs.append(m);
        card = null; cardBody = null;
        stick = true;
        break;
      }
      case 'assistant-start':
        if (!card) newCard(e.who);
        seg = null; // next text starts a new segment
        break;
      case 'text':
        if (!card) newCard();
        if (!seg) { seg = h('div.seg'); seg.dataset.raw = ''; cardBody!.append(seg); tools = null; }
        seg.dataset.raw += e.delta;
        clear(seg);
        seg.append(...fmtInline(seg.dataset.raw ?? ''));
        break;
      case 'tool': {
        if (!card) newCard();
        if (!tools) { tools = h('div.tools'); cardBody!.append(tools); seg = null; }
        if (e.phase === 'start') {
          const status = h('span.ts', h('span.spin', '◐'));
          const title = h('span.tt', e.title ?? '');
          const det = h('details.tl', h('summary', title, status), h('pre', abbrevInput(e.input)));
          tools.append(det);
          toolEls.set(e.id, { det, status, title });
        } else {
          const t = toolEls.get(e.id);
          if (t) {
            t.title.textContent = e.title ?? t.title.textContent;
            clear(t.status);
            t.status.textContent = e.status ?? '';
            if (!e.ok) t.status.classList.add('bad');
            const pre = t.det.querySelector('pre')!;
            if (e.detail) pre.textContent = e.detail;
            else pre.textContent = abbrevInput(e.input);
            if (e.thumb) {
              const url = URL.createObjectURL(e.thumb);
              t.det.append(h('img.thumb', { src: url, alt: 'render' }));
            }
          }
        }
        break;
      }
      case 'fallback':
        if (!card) newCard();
        if (!tools) { tools = h('div.tools'); cardBody!.append(tools); seg = null; }
        tools.append(h('div.tl', h('summary', { style: 'display:flex' }, h('span.tt', e.text))));
        break;
      case 'notice':
        msgs.append(h('div.notice', { class: e.level === 'err' ? 'err' : e.level === 'warn' ? 'warn' : '' }, e.text));
        card = null; cardBody = null; seg = null; tools = null;
        break;
      case 'done':
        sendBtn.textContent = 'SEND';
        sendBtn.classList.add('primary');
        ta.disabled = false;
        newBtn.disabled = false;
        card = null; cardBody = null; seg = null; tools = null;
        break;
      default: break;
    }
    scroll();
  });

  function setBusy() {
    sendBtn.textContent = 'STOP';
    sendBtn.classList.remove('primary');
  }

  function send() {
    if (session.busy) { session.stop(); return; }
    const text = ta.value.trim();
    if (!text && !pending.length) return;
    const ready = session.ready();
    if (!ready.ok) {
      msgs.append(h('div.notice.err', ready.why));
      scroll();
      hooks.openSetup();
      return;
    }
    const images = pending.splice(0);
    clear(attached);
    ta.value = '';
    setBusy();
    newBtn.disabled = true;
    void session.send(text || '(see attached image)', images);
  }
  const syncComposer = () => {
    const ag = session.isAgent && !session.demo;
    attachBtn.disabled = ag;
    attachBtn.title = ag ? 'Local agents take text only' : '';
    ta.placeholder = ag ? 'TELL THE LOCAL AGENT WHAT TO CHANGE IN THIS DETAIL' : 'DESCRIBE A DETAIL, OR PASTE A SCREENSHOT TO RECREATE';
  };
  syncComposer();

  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
  });

  async function addImage(blob: Blob) {
    try {
      const r = await blobToImageBlock(blob);
      const p: Pending = { block: r.block, thumbUrl: r.thumbUrl };
      pending.push(p);
      const x = h('button', { type: 'button', title: 'Remove', on: { click: () => { pending.splice(pending.indexOf(p), 1); a.remove(); } } }, '×');
      const a = h('div.att', h('img', { src: r.thumbUrl, alt: 'attachment' }), x);
      attached.append(a);
    } catch (e) { app.flash('IMAGE NOT ATTACHED: ' + (e as Error).message, 'err'); }
  }
  file.addEventListener('change', async () => { for (const f of Array.from(file.files ?? [])) await addImage(f); file.value = ''; });
  ta.addEventListener('paste', async (e) => {
    const items = Array.from(e.clipboardData?.items ?? []).filter((i) => i.type.startsWith('image/'));
    if (!items.length) return;
    e.preventDefault();
    for (const it of items) { const f = it.getAsFile(); if (f) await addImage(f); }
  });
  el.addEventListener('dragover', (e) => { if (e.dataTransfer?.types.includes('Files')) { e.preventDefault(); el.classList.add('drop'); } });
  el.addEventListener('dragleave', (e) => { if (e.target === el) el.classList.remove('drop'); });
  el.addEventListener('drop', async (e) => {
    el.classList.remove('drop');
    const files = Array.from(e.dataTransfer?.files ?? []).filter((f) => f.type.startsWith('image/'));
    if (!files.length) return;
    e.preventDefault();
    for (const f of files) await addImage(f);
  });

  return {
    renderIntro,
    /** a LOCAL AGENT card from the op log (edits made outside this browser) */
    addAgentCard(c: AgentCard) { intro.remove(); msgs.append(agentLogCard(c)); scroll(); },
    notice(text: string, level: 'info' | 'warn' | 'err' = 'info') { msgs.append(h('div.notice', { class: level === 'err' ? 'err' : level === 'warn' ? 'warn' : '' }, text)); scroll(); },
    refreshPicker: fillPicker,
    /** programmatic send (demo autoplay, tests) */
    sendText(t: string) { ta.value = t; send(); },
    setText(t: string) { ta.value = t; },
  };
}
