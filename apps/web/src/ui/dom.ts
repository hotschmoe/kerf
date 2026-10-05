type Child = Node | string | number | false | null | undefined;
type Attrs = Record<string, unknown> & { class?: string; style?: string; on?: Record<string, EventListener> };

/** Minimal hyperscript: h('div.card', {title:'x'}, child, ...) */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K | string, attrs?: Attrs | Child, ...children: Child[]): HTMLElementTagNameMap[K] & HTMLElement {
  const tm = /^[a-z0-9-]*/i.exec(tag)![0];
  const el = document.createElement(tm || 'div') as HTMLElement;
  for (const [, kind, name] of tag.slice(tm.length).matchAll(/([#.])([\w-]+)/g)) {
    if (kind === '#') el.id = name; else el.classList.add(name);
  }
  let a = attrs;
  if (a !== undefined && a !== null && (typeof a !== 'object' || a instanceof Node)) { children.unshift(a as Child); a = undefined; }
  if (a) {
    for (const [k, v] of Object.entries(a as Attrs)) {
      if (v === undefined || v === null || v === false) continue;
      if (k === 'class') el.className = (el.className ? el.className + ' ' : '') + v;
      else if (k === 'on') for (const [ev, fn] of Object.entries(v as Record<string, EventListener>)) el.addEventListener(ev, fn);
      else if (k === 'style') el.setAttribute('style', String(v));
      else if (k in el && k !== 'list' && typeof v !== 'object') (el as unknown as Record<string, unknown>)[k] = v;
      else el.setAttribute(k, String(v));
    }
  }
  for (const c of children) {
    if (c === false || c === null || c === undefined) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el as never;
}

export function clear(el: Element) { while (el.firstChild) el.removeChild(el.firstChild); }

export function btn(label: string, onClick: (e: MouseEvent) => void, cls = ''): HTMLButtonElement {
  return h('button.btn', { class: cls, type: 'button', on: { click: onClick as EventListener } }, label) as HTMLButtonElement;
}

export function hhmm(d = new Date()): string {
  return String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0');
}

export function download(name: string, data: BlobPart, mime: string) {
  const url = URL.createObjectURL(new Blob([data], { type: mime }));
  const a = h('a', { href: url, download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}
