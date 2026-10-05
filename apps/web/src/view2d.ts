// Interactive 2D viewport: vellum + blue grid + Drawing IR, pan/zoom/fit, hover/select by src, note dragging.
import type { Drawing } from './types';
import { DrawingModel, baseId, fitView, paintBatches, type View2D } from './draw2d';
import { fmtFtIn } from './units';

export interface Viewport2DCallbacks {
  onHover(id: string | null): void;
  onSelect(id: string | null): void;
  onCursor(x: number | null, y: number | null): void;
  isNote(id: string): boolean;
  onNoteMove(id: string, place: [number, number]): void;
}

function unionBox(b: [number, number, number, number][]): [number, number, number, number] | null {
  if (!b.length) return null;
  const u: [number, number, number, number] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const t of b) { u[0] = Math.min(u[0], t[0]); u[1] = Math.min(u[1], t[1]); u[2] = Math.max(u[2], t[2]); u[3] = Math.max(u[3], t[3]); }
  return u;
}

const COL = { vellum: '#FBFAF5', grid: '#A9C1DD', grid2: '#D3E0EE', ink: '#1A1A1A', blue: '#1D4E9E' };

export class Viewport2D {
  readonly canvas = document.createElement('canvas');
  private ctx = this.canvas.getContext('2d')!;
  model: DrawingModel | null = null;
  view: View2D = { zoom: 10, tx: 0, ty: 0 };
  private drawingKey = '';
  private hover: string | null = null;
  private selected: string | null = null;
  private w = 0; private h = 0; private dpr = 1;
  private raf = 0;
  private drag: null | { kind: 'pan'; sx: number; sy: number; tx: number; ty: number; moved: boolean }
    | { kind: 'note'; id: string; mx: number; my: number; dx: number; dy: number; ref: [number, number]; moved: boolean } = null;
  lastPaintMs = 0;
  /** true once the user pans/zooms; until then every resize re-fits the drawing */
  private touched = false;

  constructor(private host: HTMLElement, private cb: Viewport2DCallbacks) {
    this.canvas.className = 'vp-canvas';
    host.appendChild(this.canvas);
    new ResizeObserver(() => this.resize()).observe(host);
    this.canvas.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    this.canvas.addEventListener('pointerdown', (e) => this.onDown(e));
    this.canvas.addEventListener('pointermove', (e) => this.onMove(e));
    this.canvas.addEventListener('pointerup', (e) => this.onUp(e));
    this.canvas.addEventListener('pointerleave', () => { if (!this.drag) { this.cb.onCursor(null, null); this.setHover(null); } });
    this.canvas.addEventListener('dblclick', () => this.fit());
    this.resize();
  }

  get visible() { return this.host.clientWidth > 0; }

  setModel(model: DrawingModel | null, opts: { keepView?: boolean } = {}) {
    if (!model) { this.model = null; this.requestRender(); return; }
    const d = model.drawing;
    const key = `${d.doc}/${d.view}`;
    this.model = model;
    const first = key !== this.drawingKey;
    this.drawingKey = key;
    if (first || !opts.keepView) { if (this.w > 0) this.fit(); }
    this.requestRender();
  }
  setDrawing(d: Drawing | null, opts: { keepView?: boolean } = {}) { this.setModel(d ? new DrawingModel(d) : null, opts); }

  setSelection(id: string | null) { if (id !== this.selected) { this.selected = id; this.requestRender(); } }
  setHover(id: string | null) { if (id !== this.hover) { this.hover = id; this.cb.onHover(id); this.canvas.style.cursor = id && this.cb.isNote(id) ? 'move' : id ? 'pointer' : 'crosshair'; this.requestRender(); } }

  fit() {
    if (!this.model || this.w === 0) return;
    this.touched = false;
    this.view = fitView(this.model.bounds, this.w, this.h, Math.max(10, 0.05 * Math.min(this.w, this.h)));
    this.requestRender();
  }
  zoomBy(f: number) {
    const cx = this.w / 2, cy = this.h / 2;
    this.zoomAt(cx, cy, f);
  }
  private zoomAt(sx: number, sy: number, f: number) {
    this.touched = true;
    const v = this.view;
    const nz = Math.max(0.05, Math.min(v.zoom * f, 4000));
    const k = nz / v.zoom;
    this.view = { zoom: nz, tx: sx - (sx - v.tx) * k, ty: sy - (sy - v.ty) * k };
    this.requestRender();
  }

  resize() {
    const r = this.host.getBoundingClientRect();
    const w = Math.max(0, Math.round(r.width)), h = Math.max(0, Math.round(r.height));
    const dpr = window.devicePixelRatio || 1;
    if (w === this.w && h === this.h && dpr === this.dpr) return;
    const hadSize = this.w > 0;
    if (hadSize && w > 0 && this.model && !this.touched) { this.w = w; this.h = h; this.dpr = dpr; this.canvas.width = Math.max(1, Math.round(w * dpr)); this.canvas.height = Math.max(1, Math.round(h * dpr)); this.canvas.style.width = w + 'px'; this.canvas.style.height = h + 'px'; this.fit(); return; }
    if (hadSize && w > 0 && this.model) this.view = { ...this.view, tx: this.view.tx + (w - this.w) / 2, ty: this.view.ty + (h - this.h) / 2 }; // keep the view centered
    this.w = w; this.h = h; this.dpr = dpr;
    this.canvas.width = Math.max(1, Math.round(w * dpr));
    this.canvas.height = Math.max(1, Math.round(h * dpr));
    this.canvas.style.width = w + 'px'; this.canvas.style.height = h + 'px';
    if (!hadSize && w > 0 && this.model) this.fit();
    this.requestRender();
  }

  toModel(sx: number, sy: number): [number, number] {
    return [(sx - this.view.tx) / this.view.zoom, (this.view.ty - sy) / this.view.zoom];
  }

  private rel(e: PointerEvent | WheelEvent): [number, number] {
    const r = this.canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  private onWheel(e: WheelEvent) {
    e.preventDefault();
    const [sx, sy] = this.rel(e);
    const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
    this.zoomAt(sx, sy, Math.exp(-dy * (e.ctrlKey ? 0.01 : 0.0016)));
  }

  private pickAt(sx: number, sy: number): string | null {
    if (!this.model) return null;
    const [x, y] = this.toModel(sx, sy);
    return this.model.pick(x, y, 6 / this.view.zoom);
  }

  private onDown(e: PointerEvent) {
    if (!this.model) return;
    this.canvas.setPointerCapture(e.pointerId);
    const [sx, sy] = this.rel(e);
    const id = e.button === 0 ? this.pickAt(sx, sy) : null;
    if (id && this.cb.isNote(id) && e.button === 0) {
      const g = this.model.bySrc.get(id);
      const [x, y] = this.toModel(sx, sy);
      const inText = g?.texts.some(([x0, y0, x1, y1]) => x >= x0 - 0.5 && x <= x1 + 0.5 && y >= y0 - 0.5 && y <= y1 + 0.5);
      if (g && g.textItems.length && inText) {
        const t = g.textItems[0];
        this.drag = { kind: 'note', id, mx: sx, my: sy, dx: 0, dy: 0, ref: [t.x, t.y + t.h], moved: false }; // SPEC: place = top-left of the first text line (cap height above its baseline)
        return;
      }
    }
    this.drag = { kind: 'pan', sx, sy, tx: this.view.tx, ty: this.view.ty, moved: false };
  }

  private onMove(e: PointerEvent) {
    const [sx, sy] = this.rel(e);
    const [mx, my] = this.toModel(sx, sy);
    this.cb.onCursor(mx, my);
    const d = this.drag;
    if (d?.kind === 'pan') {
      const dx = sx - d.sx, dy = sy - d.sy;
      if (!d.moved && Math.hypot(dx, dy) > 3) { d.moved = true; this.canvas.style.cursor = 'grabbing'; }
      if (d.moved) { this.touched = true; this.view = { ...this.view, tx: d.tx + dx, ty: d.ty + dy }; this.requestRender(); }
    } else if (d?.kind === 'note') {
      const dx = (sx - d.mx) / this.view.zoom, dy = -(sy - d.my) / this.view.zoom;
      if (!d.moved && Math.hypot(sx - d.mx, sy - d.my) > 3) d.moved = true;
      if (d.moved) { d.dx = dx; d.dy = dy; this.requestRender(); }
    } else {
      this.setHover(this.pickAt(sx, sy));
    }
  }

  private onUp(e: PointerEvent) {
    const d = this.drag;
    this.drag = null;
    if (!d) return;
    const [sx, sy] = this.rel(e);
    if (d.kind === 'note') {
      if (d.moved) {
        const rnd = (n: number) => Math.round(n * 100) / 100;
        this.cb.onNoteMove(d.id, [rnd(d.ref[0] + d.dx), rnd(d.ref[1] + d.dy)]);
      } else this.cb.onSelect(d.id);
    } else if (!d.moved) {
      this.cb.onSelect(this.pickAt(sx, sy));
    }
    this.canvas.style.cursor = 'crosshair';
    this.requestRender();
  }

  requestRender() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => { this.raf = 0; this.paint(); });
  }

  private drawGrid(ctx: CanvasRenderingContext2D) {
    const { zoom, tx, ty } = this.view;
    const dpr = this.dpr;
    const levels: [number, string][] = [[0.25, COL.grid2], [1, COL.grid], [12, COL.grid], [60, COL.grid]];
    const x0 = -tx / zoom, x1 = (this.w - tx) / zoom;
    const yTop = ty / zoom, yBot = (ty - this.h) / zoom;
    ctx.lineWidth = 1;
    levels.forEach(([sp, col], li) => {
      const px = sp * zoom;
      const alpha = Math.max(0, Math.min(1, (px - 5) / 9));
      if (alpha <= 0.02) return;
      // a coarser level stays visible; a finer one fades out
      ctx.globalAlpha = li === 0 ? alpha * 0.9 : li === 1 ? alpha : alpha * 0.55;
      ctx.strokeStyle = col;
      ctx.beginPath();
      const i0 = Math.floor(x0 / sp), i1 = Math.ceil(x1 / sp);
      for (let i = i0; i <= i1; i++) {
        if (li === 0 && i % 4 === 0) continue; // drawn by the 1" level
        if (li === 1 && px < 60 && i % 12 === 0 && 12 * zoom > 14) continue;
        const sx = Math.round((tx + i * sp * zoom) * dpr) + 0.5;
        ctx.moveTo(sx, 0); ctx.lineTo(sx, this.canvas.height);
      }
      const j0 = Math.floor(yBot / sp), j1 = Math.ceil(yTop / sp);
      for (let j = j0; j <= j1; j++) {
        if (li === 0 && j % 4 === 0) continue;
        if (li === 1 && px < 60 && j % 12 === 0 && 12 * zoom > 14) continue;
        const sy = Math.round((ty - j * sp * zoom) * dpr) + 0.5;
        ctx.moveTo(0, sy); ctx.lineTo(this.canvas.width, sy);
      }
      ctx.stroke();
    });
    ctx.globalAlpha = 1;
  }

  private paint() {
    const t0 = performance.now();
    const ctx = this.ctx;
    const dpr = this.dpr;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = COL.vellum;
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    if (this.w === 0) return;
    this.drawGrid(ctx);
    const m = this.model;
    if (m) {
      const { zoom, tx, ty } = this.view;
      ctx.setTransform(zoom * dpr, 0, 0, -zoom * dpr, tx * dpr, ty * dpr);
      const drag = this.drag?.kind === 'note' && this.drag.moved ? this.drag : null;
      paintBatches(ctx, m, zoom, { ink: COL.ink, dpr, skipTextOf: drag?.id });
      const onePx = 1 / (zoom * dpr);
      // selection tint + outline, hover outline
      const sel = this.selected && m.bySrc.get(this.selected);
      if (sel) {
        ctx.fillStyle = 'rgba(29,78,158,0.15)';
        ctx.fill(sel.region, 'evenodd');
        ctx.strokeStyle = COL.blue; ctx.lineWidth = 2 * onePx * dpr; ctx.lineJoin = 'round';
        ctx.stroke(sel.outline);
        const ub = unionBox(sel.texts);
        if (ub && !drag) {
          ctx.lineWidth = 1.5 * onePx * dpr;
          ctx.strokeRect(ub[0] - 0.3, ub[1] - 0.4, ub[2] - ub[0] + 0.6, ub[3] - ub[1] + 0.8);
        }
      }
      const hov = this.hover && this.hover !== this.selected && m.bySrc.get(this.hover);
      if (hov) {
        ctx.strokeStyle = COL.blue; ctx.lineWidth = 1.5 * onePx * dpr; ctx.lineJoin = 'round';
        ctx.stroke(hov.outline);
        const hb = unionBox(hov.texts);
        if (hb) ctx.strokeRect(hb[0] - 0.3, hb[1] - 0.4, hb[2] - hb[0] + 0.6, hb[3] - hb[1] + 0.8);
      }
      if (drag) {
        ctx.save();
        ctx.translate(drag.dx, drag.dy);
        paintBatches(ctx, m, zoom, { ink: COL.blue, dpr }, (b) => !!b.text && baseId(b.src) === drag.id);
        ctx.restore();
      }
    }
    this.lastPaintMs = performance.now() - t0;
  }

  /** model-space cursor label, e.g. for status line */
  static fmtCursor(x: number, y: number): string { return `X ${fmtFtIn(x)} Y ${fmtFtIn(y)}`; }
}
