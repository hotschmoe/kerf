// Kerf stroke font (Hershey Simplex derivative, spec/fonts/kerf-simplex.json) -> polylines.
import type { StrokeFont } from './types';

export type Polyline = number[]; // flat [x0,y0,x1,y1,...]

let font: StrokeFont | null = null;
export function setFont(f: StrokeFont) { font = f; }
export function getFont(): StrokeFont {
  if (!font) throw new Error('stroke font not loaded');
  return font;
}

/** Fold characters the stroke font lacks to their ASCII look-alikes (engine text may contain them). */
export function fold(s: string): string {
  return s.replace(/[\u2013\u2014\u2212]/g, '-').replace(/[\u2018\u2019\u2032]/g, "'").replace(/[\u201C\u201D\u2033]/g, '"')
    .replace(/\u00D7/g, 'X').replace(/\u00B7|\u2022/g, '.').replace(/\u00BD/g, '1/2').replace(/\u00BC/g, '1/4').replace(/\u00BE/g, '3/4')
    .replace(/\u00B0/g, ' DEG').replace(/\u2026/g, '...').replace(/\u00A0/g, ' ');
}

export function textWidth(s0: string, h: number): number {
  const f = getFont();
  const s = fold(s0);
  const k = h / f.cap_height;
  let w = 0;
  for (const ch of s) w += (f.glyphs[ch] ?? f.glyphs['?'])?.adv * k || 0;
  return w;
}

export interface TextSpec {
  s: string; x: number; y: number; h: number; rot?: number; align?: string; valign?: string;
}

/** Strokes in model space for one text item, plus its axis-aligned bounding box. */
export function layoutText(t0: TextSpec): { lines: Polyline[]; bbox: [number, number, number, number]; width: number } {
  const f = getFont();
  const t = { ...t0, s: fold(t0.s) };
  const k = t.h / f.cap_height;
  const width = textWidth(t.s, t.h);
  const ox = t.align === 'center' || t.align === 'middle' ? -width / 2 : t.align === 'right' ? -width : 0;
  const oy = t.valign === 'middle' ? -t.h / 2 : t.valign === 'top' ? -t.h : t.valign === 'bottom' ? 0 : 0;
  const rot = ((t.rot ?? 0) * Math.PI) / 180;
  const c = Math.cos(rot), s = Math.sin(rot);
  const lines: Polyline[] = [];
  let pen = 0;
  for (const ch of t.s) {
    const g = f.glyphs[ch] ?? f.glyphs['?'];
    if (!g) continue;
    for (const stroke of g.strokes) {
      const line: number[] = [];
      for (const [gx, gy] of stroke) {
        const lx = ox + (pen + gx) * k, ly = oy + (gy - f.baseline) * k;
        line.push(t.x + lx * c - ly * s, t.y + lx * s + ly * c);
      }
      lines.push(line);
    }
    pen += g.adv;
  }
  // bbox of the text box (cap height tall), rotated
  const corners: [number, number][] = [[ox, oy], [ox + width, oy], [ox + width, oy + t.h], [ox, oy + t.h]];
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const [lx, ly] of corners) {
    const px = t.x + lx * c - ly * s, py = t.y + lx * s + ly * c;
    x0 = Math.min(x0, px); y0 = Math.min(y0, py); x1 = Math.max(x1, px); y1 = Math.max(y1, py);
  }
  return { lines, bbox: [x0, y0, x1, y1], width };
}
