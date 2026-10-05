// Operator console: chat history (designer plain, KERF/CLAUDE manila cards with tool activity) + composer.
import type { App } from '../app';
import { Harness, type ChatEvent } from '../chat/harness';
import { blobToImageBlock } from '../chat/images';
import type { ImageBlock } from '../chat/transport';
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
  hasTransport(): boolean;
  openKeyDialog(): void;
}

export function mountConsole(app: App, el: HTMLElement, harness: Harness, hooks: ConsoleHooks) {
  const msgs = h('div#msgs');
  const ta = h('textarea', { rows: 3, placeholder: 'DESCRIBE A DETAIL, OR PASTE A SCREENSHOT TO RECREATE', spellcheck: false, 'aria-label': 'Message to Claude' });
  const attached = h('div#attached');
  const sendBtn = btn('SEND', () => send(), 'primary');
  const attachBtn = btn('ATTACH', () => file.click());
  const file = h('input', { type: 'file', accept: 'image/png,image/jpeg,image/webp,image/gif', multiple: true, style: 'display:none' });
  const hint = h('span.hint', 'ENTER SENDS · SHIFT+ENTER NEWLINE');
  const pending: Pending[] = [];

  el.append(
    h('div.ph', h('span', 'OPERATOR CONSOLE'), h('span.r', '')),
    msgs,
    h('div#composer', attached, h('div.prompt', h('span.gt', '>'), ta), h('div.row', attachBtn, hint, h('span.sp'), sendBtn), file),
  );

  const intro = h('div.intro');
  const renderIntro = () => {
    clear(intro);
    intro.append(
      h('b', 'KERF OPERATOR CONSOLE. '),
      hooks.hasTransport() ? 'Describe the construction detail you need, or attach a screenshot of one to recreate. Claude builds it with the engine; you review and edit it at right.'
        : 'NO API KEY — ENTER KEY TO ENABLE CLAUDE. (KEY button, top right.) You can still open a sample detail and edit its notes.',
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

  const newCard = () => {
    cardBody = h('div.mb');
    card = h('div.msg.llm', h('div.mh', h('span', 'KERF/CLAUDE'), h('span', hhmm())), cardBody);
    msgs.append(card);
    seg = null; tools = null;
  };

  harness.on((e: ChatEvent) => {
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
        if (!card) newCard();
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
    if (harness.busy) { harness.stop(); return; }
    const text = ta.value.trim();
    if (!text && !pending.length) return;
    if (!hooks.hasTransport()) {
      msgs.append(h('div.notice.err', 'NO API KEY — ENTER KEY TO ENABLE CLAUDE.'));
      scroll();
      hooks.openKeyDialog();
      return;
    }
    const images = pending.splice(0);
    clear(attached);
    ta.value = '';
    setBusy();
    void harness.send(text || '(see attached image)', images);
  }

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
    /** programmatic send (demo autoplay, tests) */
    sendText(t: string) { ta.value = t; send(); },
    setText(t: string) { ta.value = t; },
  };
}
