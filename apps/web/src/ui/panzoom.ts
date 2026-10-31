// One image on a paper page with wheel zoom, drag pan and double-click fit (the SHEET tab, reference zoom).
import { h } from './dom';

export class ImageViewer {
  private page = h('div.page');
  private img = h('img', { alt: '', draggable: false });
  private k = 1; private tx = 0; private ty = 0;
  private drag: { x: number; y: number; tx: number; ty: number } | null = null;
  private url = '';
  /** object URL created by this viewer (setSvg), revoked when the next image replaces it */
  private owned = '';
  private iw = 1056; private ih = 816;
  private userMoved = false;
  constructor(private host: HTMLElement, alt = 'sheet') {
    this.img.alt = alt;
    this.page.append(this.img);
    host.classList.add('pzhost');
    host.append(this.page);
    host.addEventListener('wheel', (e) => {
      e.preventDefault();
      const r = host.getBoundingClientRect();
      this.zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-(e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY) * 0.0016));
    }, { passive: false });
    host.addEventListener('pointerdown', (e) => { host.setPointerCapture(e.pointerId); this.drag = { x: e.clientX, y: e.clientY, tx: this.tx, ty: this.ty }; host.classList.add('drag'); });
    host.addEventListener('pointermove', (e) => { if (!this.drag) return; this.tx = this.drag.tx + e.clientX - this.drag.x; this.ty = this.drag.ty + e.clientY - this.drag.y; this.apply(); });
    host.addEventListener('pointerup', () => { this.drag = null; host.classList.remove('drag'); });
    host.addEventListener('dblclick', () => this.fit());
    new ResizeObserver(() => { if (host.clientWidth && this.url && !this.userMoved) this.fit(); }).observe(host);
  }
  /** Show SVG text; the object URL belongs to this viewer. */
  async setSvg(svg: string) {
    this.release();
    this.owned = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
    await this.load(this.owned);
  }
  /** Show an image whose URL the caller owns (it stays valid when this viewer moves on). */
  async setSrc(src: string, alt?: string) {
    this.release();
    if (alt !== undefined) this.img.alt = alt;
    await this.load(src);
  }
  clear() { this.release(); this.url = ''; this.img.removeAttribute('src'); }
  fit() {
    const w = this.host.clientWidth, h = this.host.clientHeight;
    if (!w || !h) return;
    this.k = Math.min((w - 48) / this.iw, (h - 48) / this.ih);
    this.tx = (w - this.iw * this.k) / 2; this.ty = (h - this.ih * this.k) / 2;
    this.apply();
  }
  zoomBy(f: number) { this.zoomAt(this.host.clientWidth / 2, this.host.clientHeight / 2, f); }
  private release() { if (this.owned) URL.revokeObjectURL(this.owned); this.owned = ''; }
  private async load(src: string) {
    this.url = src;
    this.img.src = src;
    await this.img.decode().catch(() => undefined);
    this.iw = this.img.naturalWidth || 1056; this.ih = this.img.naturalHeight || 816;
    // present at the image's natural pixel size; zoom is applied on the wrapper
    this.img.style.width = this.iw + 'px'; this.img.style.height = this.ih + 'px';
    this.userMoved = false;
    this.fit();
  }
  private zoomAt(sx: number, sy: number, f: number) {
    const nk = Math.max(0.05, Math.min(this.k * f, 20));
    const r = nk / this.k;
    this.tx = sx - (sx - this.tx) * r; this.ty = sy - (sy - this.ty) * r; this.k = nk;
    this.userMoved = true;
    this.apply();
  }
  private apply() { this.page.style.transform = `translate(${this.tx}px, ${this.ty}px) scale(${this.k})`; }
}
