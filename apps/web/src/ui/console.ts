// Operator console: chat history (designer plain, KERF/CLAUDE manila cards with tool activity) + composer.
import type { App } from '../app';
import type { ChatEvent } from '../chat/agent';
import type { ChatSession } from '../chat/session';
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

/** Minimal inline formatting for the agent's replies: **bold** and `code`. Everything else stays plain text. */
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

export function mountConsole(app: App, el: HTMLElement, session: ChatSession) {
  const msgs = h('div#msgs');
  const ta = h('textarea', { rows: 3, spellcheck: false, 'aria-label': 'Message to the local agent' });
  const sendBtn = btn('SEND', () => send(), 'primary');
  const hint = h('span.hint', 'ENTER SENDS · SHIFT+ENTER NEWLINE');

  const picker = h('select.pick', { 'aria-label': 'Local agent', title: 'Local agent' }) as HTMLSelectElement;
  const newBtn = btn('NEW', () => {
    session.newConversation();
    msgs.append(h('div.notice', 'NEW CONVERSATION. THE AGENT STARTS A FRESH SESSION FOR THIS DETAIL.'));
    scroll();
  }, 'sm');
  newBtn.title = 'Start a fresh local agent session for this document';
  const fillPicker = () => {
    clear(picker);
    for (const a of session.agents()) picker.append(h('option', { value: a.id, disabled: !a.available }, `${a.name.toUpperCase()}${a.available ? '' : ' (NOT FOUND)'}`));
    if (!session.agents().length) picker.append(h('option', { value: '' }, 'NO LOCAL AGENT'));
    picker.value = session.choice ?? '';
    picker.disabled = !session.available || !session.agents().length;
    newBtn.disabled = !session.available;
  };
  fillPicker();
  picker.addEventListener('change', () => {
    session.setChoice(picker.value);
    const r = session.ready();
    if (!r.ok) { msgs.append(h('div.notice.warn', r.why)); scroll(); }
  });
  session.onChange(() => { fillPicker(); renderIntro(); syncComposer(); });
  el.append(
    h('div.ph', h('span', 'OPERATOR CONSOLE'), h('span.r', picker, newBtn)),
    msgs,
    h('div#composer', h('div.prompt', h('span.gt', '>'), ta), h('div.row', hint, h('span.sp'), sendBtn)),
  );

  const intro = h('div.intro');
  const renderIntro = () => {
    clear(intro);
    const r = session.ready();
    intro.append(
      h('b', 'KERF OPERATOR CONSOLE. '),
      r.ok ? 'Describe the change you want. Your local agent edits the detail files in this folder with the kerf CLI; you review and edit them at right.'
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

  const newCard = (who = 'LOCAL AGENT') => {
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
            t.det.querySelector('pre')!.textContent = e.detail || abbrevInput(e.input);
          }
        }
        break;
      }
      case 'notice':
        if (card && cardBody && !cardBody.childElementCount && !cardBody.textContent) card.remove(); // a failed request leaves no empty card behind
        msgs.append(h('div.notice', { class: e.level === 'err' ? 'err' : e.level === 'warn' ? 'warn' : '' }, e.text));
        card = null; cardBody = null; seg = null; tools = null;
        break;
      case 'done':
        sendBtn.textContent = 'SEND';
        sendBtn.classList.add('primary');
        ta.disabled = false;
        newBtn.disabled = !session.available;
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
    if (!text) return;
    const ready = session.ready();
    if (!ready.ok) {
      msgs.append(h('div.notice.err', ready.why));
      scroll();
      return;
    }
    ta.value = '';
    setBusy();
    newBtn.disabled = true;
    void session.send(text);
  }
  const syncComposer = () => {
    const on = session.available;
    ta.disabled = !on;
    sendBtn.disabled = !on;
    ta.placeholder = on ? 'TELL THE LOCAL AGENT WHAT TO CHANGE IN THIS DETAIL' : 'CHAT NEEDS `kerf serve` AND A LOCAL AGENT CLI';
  };
  syncComposer();

  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
  });

  return {
    renderIntro,
    /** a LOCAL AGENT card from the op log (edits made outside this browser) */
    addAgentCard(c: AgentCard) { intro.remove(); msgs.append(agentLogCard(c)); scroll(); },
    notice(text: string, level: 'info' | 'warn' | 'err' = 'info') { msgs.append(h('div.notice', { class: level === 'err' ? 'err' : level === 'warn' ? 'warn' : '' }, text)); scroll(); },
    refreshPicker: fillPicker,
    /** programmatic send (tests) */
    sendText(t: string) { ta.value = t; send(); },
    setText(t: string) { ta.value = t; },
  };
}
