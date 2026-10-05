import { h } from './dom';

/** Floating paper panel anchored under `anchor`, closed by a click outside or Escape. */
export function popup(anchor: HTMLElement, content: HTMLElement, align: 'left' | 'right' = 'left'): () => void {
  const scrim = h('div.scrim');
  const box = h('div.float', content);
  document.body.append(scrim, box);
  const r = anchor.getBoundingClientRect();
  box.style.top = `${r.bottom + 4}px`;
  const w = box.offsetWidth;
  const left = align === 'left' ? r.left : r.right - w;
  box.style.left = `${Math.max(8, Math.min(left, window.innerWidth - w - 8))}px`;
  const close = () => { scrim.remove(); box.remove(); document.removeEventListener('keydown', onKey); };
  const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
  document.addEventListener('keydown', onKey);
  scrim.addEventListener('pointerdown', close);
  return close;
}
