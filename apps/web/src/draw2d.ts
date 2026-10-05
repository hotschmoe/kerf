// Drawing IR (SPEC §10) -> Canvas2D. Used by the interactive viewport AND by the kerf_render rasterizer.
import type { BPt, Drawing, DrawItem } from './types';
import { layoutText } from './strokefont';

export interface View2D { zoom: number; tx: number; ty: number } // screen px = tx + zoom*x ; ty - zoom*y

export interface Batch {
  kind: 'stroke' | 'fill';
  pen: string;
  path: Path2D;
  src?: string;
  text?: boolean;
}

export interface SrcGeom {
  id: string;
  closed: number[][]; // flattened closed loops [x,y,x,y,...] (closed paths, hatch outers, fills)
  open: number[][];   // flattened open polylines
  texts: [number, number, number, number][]; // text boxes
  textItems: { x: number; y: number; h: number; s: string }[];
  outline: Path2D;    // true-arc outline of everything (for hover)
  region: Path2D;     // closed loops with even-odd (for selection tint)
  bbox: [number, number, number, number];
}

export function baseId(src: string | undefined): string | undefined {
  if (!src) return undefined;
  const i = src.search(/[.#]/);
  return i < 0 ? src : src.slice(0, i);
}

/** Add polyline-with-bulges to a Path2D using true arcs. */
export function addBulgePath(p: Path2D, pts: BPt[], closed: boolean) {
  const n = pts.length;
  if (!n) return;
  p.moveTo(pts[0][0], pts[0][1]);
  const segs = closed ? n : n - 1;
  for (let i = 0; i < segs; i++) {
    const a = pts[i], b = pts[(i + 1) % n];
    const bulge = (a[2] as number | undefined) ?? 0;
    if (Math.abs(bulge) < 1e-9) { p.lineTo(b[0], b[1]); continue; }
    const arc = arcOf(a[0], a[1], b[0], b[1], bulge);
    if (!arc) { p.lineTo(b[0], b[1]); continue; }
    p.arc(arc.cx, arc.cy, arc.r, arc.a0, arc.a0 + arc.sweep, arc.sweep < 0);
  }
  if (closed) p.closePath();
}

export function arcOf(x0: number, y0: number, x1: number, y1: number, bulge: number) {
  const dx = x1 - x0, dy = y1 - y0;
  const c = Math.hypot(dx, dy);
  if (c < 1e-12) return null;
  const sweep = 4 * Math.atan(bulge);
  const r = (c / 2) / Math.sin(Math.abs(sweep) / 2);
  const off = (c / 2) * ((1 - bulge * bulge) / (2 * bulge));
  const mx = (x0 + x1) / 2, my = (y0 + y1) / 2;
  const cx = mx + (-dy / c) * off, cy = my + (dx / c) * off;
  return { cx, cy, r, a0: Math.atan2(y0 - cy, x0 - cx), sweep };
}

/** Flatten a bulge polyline to [x,y,...] with chord angle step ~ 8 degrees. */
export function flatten(pts: BPt[], closed: boolean): number[] {
  const out: number[] = [];
  const n = pts.length;
  const segs = closed ? n : n - 1;
  for (let i = 0; i < segs; i++) {
    const a = pts[i], b = pts[(i + 1) % n];
    out.push(a[0], a[1]);
    const bulge = (a[2] as number | undefined) ?? 0;
    if (Math.abs(bulge) > 1e-9) {
      const arc = arcOf(a[0], a[1], b[0], b[1], bulge);
      if (arc) {
        const steps = Math.max(2, Math.ceil(Math.abs(arc.sweep) / (Math.PI / 22)));
        for (let k = 1; k < steps; k++) {
          const ang = arc.a0 + (arc.sweep * k) / steps;
          out.push(arc.cx + arc.r * Math.cos(ang), arc.cy + arc.r * Math.sin(ang));
        }
      }
    }
  }
  if (!closed && n) out.push(pts[n - 1][0], pts[n - 1][1]);
  return out;
}

function polyArea(l: number[]): number {
  let a = 0;
  const n = l.length / 2;
  for (let i = 0; i < n; i++) {
    const j = (i + 1) % n;
    a += l[2 * i] * l[2 * j + 1] - l[2 * j] * l[2 * i + 1];
  }
  return Math.abs(a) / 2;
}

export class DrawingModel {
  batches: Batch[] = [];
  bySrc = new Map<string, SrcGeom>();
  /** bounds of ALL items (incl. text), model inches */
  bounds: [number, number, number, number];

  constructor(public drawing: Drawing) {
    let last: Batch | null = null;
    const push = (kind: Batch['kind'], pen: string, src: string | undefined, text: boolean): Batch => {
      if (last && !text && !last.text && last.kind === kind && last.pen === pen) return last;
      last = { kind, pen, path: new Path2D(), src, text };
      this.batches.push(last);
      return last;
    };
    const geom = (src: string | undefined): SrcGeom | null => {
      const id = baseId(src);
      if (!id) return null;
      let g = this.bySrc.get(id);
      if (!g) {
        g = { id, closed: [], open: [], texts: [], textItems: [], outline: new Path2D(), region: new Path2D(), bbox: [Infinity, Infinity, -Infinity, -Infinity] };
        this.bySrc.set(id, g);
      }
      return g;
    };
    const grow = (g: SrcGeom | null, pts: number[]) => {
      if (!g) return;
      for (let i = 0; i < pts.length; i += 2) {
        g.bbox[0] = Math.min(g.bbox[0], pts[i]); g.bbox[1] = Math.min(g.bbox[1], pts[i + 1]);
        g.bbox[2] = Math.max(g.bbox[2], pts[i]); g.bbox[3] = Math.max(g.bbox[3], pts[i + 1]);
      }
    };
    let bx0 = Infinity, by0 = Infinity, bx1 = -Infinity, by1 = -Infinity;
    const gb = (pts: number[]) => {
      for (let i = 0; i < pts.length; i += 2) {
        bx0 = Math.min(bx0, pts[i]); by0 = Math.min(by0, pts[i + 1]);
        bx1 = Math.max(bx1, pts[i]); by1 = Math.max(by1, pts[i + 1]);
      }
    };

    for (const it of drawing.items as DrawItem[]) {
      const g = geom(it.src);
      switch (it.t) {
        case 'path': {
          const b = push('stroke', it.pen, it.src, false);
          addBulgePath(b.path, it.pts, !!it.closed);
          const flat = flatten(it.pts, !!it.closed);
          gb(flat); grow(g, flat);
          if (g) {
            addBulgePath(g.outline, it.pts, !!it.closed);
            if (it.closed) { g.closed.push(flat); addBulgePath(g.region, it.pts, true); } else g.open.push(flat);
          }
          break;
        }
        case 'fill': {
          const b = push('fill', '', it.src, false);
          for (const loop of it.loops) {
            addBulgePath(b.path, loop, true);
            const flat = flatten(loop, true);
            gb(flat); grow(g, flat);
            if (g) { g.closed.push(flat); addBulgePath(g.outline, loop, true); addBulgePath(g.region, loop, true); }
          }
          break;
        }
        case 'hatch': {
          const b = push('stroke', it.pen ?? 'hatch', it.src, false);
          for (const l of it.lines) { b.path.moveTo(l[0], l[1]); b.path.lineTo(l[2], l[3]); }
          it.loops.forEach((loop, i) => {
            const flat = flatten(loop, true);
            gb(flat); grow(g, flat);
            if (g && i === 0) { g.closed.push(flat); addBulgePath(g.outline, loop, true); addBulgePath(g.region, loop, true); }
            else if (g) { addBulgePath(g.region, loop, true); }
          });
          break;
        }
        case 'text': {
          const lay = layoutText(it);
          const b = push('stroke', it.pen ?? 'anno', it.src, true);
          for (const l of lay.lines) {
            b.path.moveTo(l[0], l[1]);
            for (let i = 2; i < l.length; i += 2) b.path.lineTo(l[i], l[i + 1]);
            if (l.length === 2) b.path.lineTo(l[0], l[1]);
          }
          gb([lay.bbox[0], lay.bbox[1], lay.bbox[2], lay.bbox[3]]);
          if (g) {
            g.texts.push(lay.bbox);
            g.textItems.push({ x: it.x, y: it.y, h: it.h, s: it.s });
            grow(g, [lay.bbox[0], lay.bbox[1], lay.bbox[2], lay.bbox[3]]);
          }
          break;
        }
      }
    }
    this.bounds = isFinite(bx0) ? [bx0, by0, bx1, by1] : (drawing.bounds ?? [0, 0, 1, 1]);
  }

  /** Pick the best src at a model point. tol = pick tolerance in model inches. */
  pick(x: number, y: number, tol: number): string | null {
    // 1. text boxes (notes are the thing designers grab)
    for (const g of this.bySrc.values()) {
      for (const [x0, y0, x1, y1] of g.texts) {
        if (x >= x0 - tol * 0.3 && x <= x1 + tol * 0.3 && y >= y0 - tol * 0.5 && y <= y1 + tol * 0.5) return g.id;
      }
    }
    // 2. near an open polyline / closed outline
    let bestD = tol, best: string | null = null;
    for (const g of this.bySrc.values()) {
      if (x < g.bbox[0] - tol || x > g.bbox[2] + tol || y < g.bbox[1] - tol || y > g.bbox[3] + tol) continue;
      for (const l of g.open) {
        const d = distPoly(l, x, y, false);
        if (d < bestD) { bestD = d; best = g.id; }
      }
    }
    if (best) return best;
    // 3. smallest closed loop containing the point
    let bestA = Infinity;
    for (const g of this.bySrc.values()) {
      if (x < g.bbox[0] || x > g.bbox[2] || y < g.bbox[1] || y > g.bbox[3]) continue;
      for (const l of g.closed) {
        if (pointInPoly(l, x, y)) {
          const a = polyArea(l);
          if (a < bestA) { bestA = a; best = g.id; }
        }
      }
    }
    if (best) return best;
    // 4. near a closed outline
    bestD = tol;
    for (const g of this.bySrc.values()) {
      for (const l of g.closed) {
        const d = distPoly(l, x, y, true);
        if (d < bestD) { bestD = d; best = g.id; }
      }
    }
    return best;
  }
}

function distPoly(l: number[], px: number, py: number, closed: boolean): number {
  let best = Infinity;
  const n = l.length / 2;
  const segs = closed ? n : n - 1;
  for (let i = 0; i < segs; i++) {
    const j = (i + 1) % n;
    const x0 = l[2 * i], y0 = l[2 * i + 1], x1 = l[2 * j], y1 = l[2 * j + 1];
    const dx = x1 - x0, dy = y1 - y0;
    const len2 = dx * dx + dy * dy;
    let t = len2 > 0 ? ((px - x0) * dx + (py - y0) * dy) / len2 : 0;
    t = Math.max(0, Math.min(1, t));
    const d = Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
    if (d < best) best = d;
  }
  return best;
}

function pointInPoly(l: number[], px: number, py: number): boolean {
  let inside = false;
  const n = l.length / 2;
  for (let i = 0, j = n - 1; i < n; j = i++) {
    const xi = l[2 * i], yi = l[2 * i + 1], xj = l[2 * j], yj = l[2 * j + 1];
    if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

export interface PaintOpts {
  ink: string;
  dpr: number;
  /** note being dragged: its text batches are skipped and returned for the caller to draw offset */
  skipTextOf?: string;
  /** minimum stroke in device px */
  minPx?: number;
}

export function penModelWidth(model: DrawingModel, pen: string): number {
  const p = model.drawing.pens[pen] ?? { width_mm: 0.18 };
  return (p.width_mm / 25.4) * model.drawing.scale;
}

/** Paint all batches. Context transform must already map model space to device pixels (y flipped). */
export function paintBatches(ctx: CanvasRenderingContext2D, model: DrawingModel, zoomPx: number, opts: PaintOpts, only?: (b: Batch) => boolean) {
  const minW = (opts.minPx ?? 1) / (zoomPx * opts.dpr);
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  ctx.strokeStyle = opts.ink;
  ctx.fillStyle = opts.ink;
  for (const b of model.batches) {
    if (only && !only(b)) continue;
    if (opts.skipTextOf && b.text && baseId(b.src) === opts.skipTextOf) continue;
    if (b.kind === 'fill') {
      ctx.fill(b.path, 'evenodd');
      continue;
    }
    const pen = model.drawing.pens[b.pen] ?? { width_mm: 0.18, dash_mm: null };
    const w = (pen.width_mm / 25.4) * model.drawing.scale;
    ctx.lineWidth = Math.max(w, minW);
    if (pen.dash_mm && pen.dash_mm.length) {
      const k = model.drawing.scale / 25.4;
      ctx.setLineDash(pen.dash_mm.map((d) => d * k));
    } else ctx.setLineDash([]);
    ctx.stroke(b.path);
  }
  ctx.setLineDash([]);
}

/** Fit transform for a model-space box into a w x h CSS px area with padding px. */
export function fitView(bounds: [number, number, number, number], w: number, h: number, pad = 32): View2D {
  const bw = Math.max(bounds[2] - bounds[0], 1e-6), bh = Math.max(bounds[3] - bounds[1], 1e-6);
  const zoom = Math.max(1e-4, Math.min((w - 2 * pad) / bw, (h - 2 * pad) / bh));
  const cx = (bounds[0] + bounds[2]) / 2, cy = (bounds[1] + bounds[3]) / 2;
  return { zoom, tx: w / 2 - cx * zoom, ty: h / 2 + cy * zoom };
}
