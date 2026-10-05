// Generates hand-written-style fixtures that match SPEC §10 (Drawing IR) and §11 (Mesh) until the real
// engines are available. Run: node test/fixtures/make-fixtures.mjs   (writes drawing-A.json, mesh.json, sheet-A.svg)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const r4 = (n) => Math.round(n * 1e4) / 1e4;

const items = [];
const rect = (x0, y0, x1, y1) => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
const path_ = (src, pen, pts, closed = false, layer = 'S-DETL-CUT') => items.push({ t: 'path', layer, pen, src, closed, pts });
const hatchRect = (src, [x0, y0, x1, y1], pattern, spacing, angleDeg, layer = 'S-DETL-PATT') => {
  // pre-clipped diagonal lines inside a rect (45 deg or 135 deg)
  const lines = [];
  const sgn = angleDeg === 45 ? 1 : -1;
  const span = (x1 - x0) + (y1 - y0);
  for (let d = spacing; d < span; d += spacing) {
    let ax, ay, bx, by;
    if (sgn > 0) { ax = x0 + d; ay = y0; bx = x0; by = y0 + d; } else { ax = x1 - d; ay = y0; bx = x1; by = y0 + d; }
    // clip to rect
    const clip = (px, py, qx, qy) => {
      let t0 = 0, t1 = 1; const dx = qx - px, dy = qy - py;
      for (const [p, q] of [[-dx, px - x0], [dx, x1 - px], [-dy, py - y0], [dy, y1 - py]]) {
        if (p === 0) { if (q < 0) return null; } else { const t = q / p; if (p < 0) { if (t > t1) return null; if (t > t0) t0 = t; } else { if (t < t0) return null; if (t < t1) t1 = t; } }
      }
      return [px + t0 * dx, py + t0 * dy, px + t1 * dx, py + t1 * dy].map(r4);
    };
    const c = clip(ax, ay, bx, by); if (c) lines.push(c);
  }
  items.push({ t: 'hatch', layer, pen: 'hatch', src, pattern, scale: 1, angle: angleDeg, loops: [rect(x0, y0, x1, y1)], lines });
};
const dot = (src, cx, cy, rad) => items.push({ t: 'fill', layer: 'S-DETL-CUT', src, loops: [[[cx - rad, cy, 1], [cx + rad, cy, 1]]] });
const text = (src, s, x, y, h, extra = {}) => items.push({ t: 'text', layer: 'S-ANNO-NOTE', pen: 'anno', src, s, x, y, h, rot: 0, align: 'left', valign: 'baseline', ...extra });
const arrow = (src, tx, ty, ux, uy) => {
  const L = 0.75, W = 0.25; const bx = tx + ux * L, by = ty + uy * L;
  items.push({ t: 'fill', layer: 'S-ANNO-NOTE', src, loops: [[[tx, ty], [bx - uy * W, by + ux * W], [bx + uy * W, by - ux * W]]] });
};

// --- CMU wall: x 0..7.625, y -31.625..0 (4 courses of 8"), bond beam on top course
const W = 7.625, H = 31.625, shell = 1.25;
for (let i = 0; i < 4; i++) {
  const y0 = -H + i * 8, y1 = y0 + 7.625;
  path_('cmu', 'cut', rect(0, y0, shell, y1), true);
  path_('cmu', 'cut', rect(W - shell, y0, W, y1), true);
  hatchRect('cmu', [0, y0, shell, y1], 'KERF-CMU', 0.5, 45);
  hatchRect('cmu', [W - shell, y0, W, y1], 'KERF-CMU', 0.5, 45);
  if (i === 3) { path_('cmu.bond_beam', 'cut', rect(shell, y0, W - shell, y1), true); hatchRect('cmu.bond_beam', [shell, y0, W - shell, y1], 'KERF-GROUT', 0.8, 135); }
  else path_('cmu', 'beyond', [[shell, y0], [W - shell, y1]], false, 'S-DETL-BYND');
}
// --- sill plate 2x8 flat: 7.25 x 1.5 on top
path_('sill_plate', 'cut', rect(0.2, 0, 7.45, 1.5), true);
path_('sill_plate', 'cut', [[0.2, 0], [7.45, 1.5]]); path_('sill_plate', 'cut', [[0.2, 1.5], [7.45, 0]]);
// --- rebar dots (bond beam bars) and vertical bar line
dot('bb_bars', 2.25, -6.1, 0.31); dot('bb_bars', 5.4, -6.1, 0.31);
path_('vert_bar', 'rebar', [[W / 2 - 0.31, -H], [W / 2 - 0.31, -2], [W / 2 + 0.31, -2], [W / 2 + 0.31, -H]], false, 'S-DETL-REBR');
// --- anchor bolt
path_('anchor_bolt', 'steel', [[3.5, 2.75 + 1.5], [3.5, -7]], false, 'S-DETL-STEL');
// --- truss heel (slope 4:12): bottom chord, top chord
const sl = 4 / 12;
path_('truss', 'profile', [[0, 1.5], [-18, 1.5], [-18, 1.5 + 18 * sl + 0.1], [30, 1.5 + 3.69 + 48 * sl * 0.8 ], [30, 1.5 + 48 * sl * 0.8], [0, 1.5 + 3.5]], true, 'S-DETL-CUT');
path_('truss', 'profile', rect(0, 1.5, 36, 5), true);
path_('truss', 'hidden', rect(-1, 1.5, 9, 9), true, 'S-DETL-HIDN');
// --- hurricane tie
path_('hurricane_tie', 'steel', [[0.1, 1.5], [0.1, 6], [-0.6, 7]], false, 'S-DETL-STEL');
// --- break line below cmu
path_('cmu', 'break', [[-1, -H], [1.5, -H - 0.8], [3, -H + 0.8], [4.5, -H - 0.8], [6, -H + 0.8], [W + 1, -H]], false, 'S-DETL-BRKL');

// --- annotations
const notes = [
  ['n_truss', 'PRE-ENGINEERED WOOD TRUSS @ 24" O.C. PER MFR. DWGS (IRC R802.10*)', 'truss', [20, 11], [18, 4]],
  ['n_plate', '2X8 PT SILL PLATE W/ 5/8" DIA. J-BOLTS @ 48" O.C. (IRC R403.1.6*)', 'sill_plate', [20, 1], [7.45, 0.75]],
  ['n_bb', 'GROUTED BOND BEAM W/ (2) #5 CONT. HORIZ.', 'bb_bars', [20, -9], [5.4, -6.1]],
  ['n_cmu', '8" CMU WALL (IRC R606*)', 'cmu', [20, -18], [6.5, -18]],
];
const wrap = (s, n = 28) => { const out = []; let cur = ''; for (const w of s.split(' ')) { if ((cur + ' ' + w).trim().length > n) { out.push(cur); cur = w; } else cur = (cur + ' ' + w).trim(); } if (cur) out.push(cur); return out; };
for (const [id, s, , [px, py], [tx, ty]] of notes) {
  const lines = wrap(s); const h = 0.75, ls = h * 1.6;
  lines.forEach((ln, i) => text(id, ln, px, py - i * ls, h));
  const midY = py + h / 2 - ((lines.length - 1) * ls) / 2;
  const dx = tx - (px - 1), dy = ty - midY, L = Math.hypot(dx, dy);
  path_(id, 'anno', [[px, midY], [px - 1, midY], [tx, ty]], false, 'S-ANNO-NOTE');
  arrow(id, tx, ty, (px - 1 - tx) / Math.hypot(px - 1 - tx, midY - ty), (midY - ty) / Math.hypot(px - 1 - tx, midY - ty));
  void L;
}
text('l_ext', 'EXTERIOR', -22, -14, 0.625, { align: 'left' });
// dim
const dy0 = -H - 3;
path_('d_wall', 'dim', [[0, -H - 0.5], [0, dy0 - 0.5]], false, 'S-ANNO-DIMS');
path_('d_wall', 'dim', [[W, -H - 0.5], [W, dy0 - 0.5]], false, 'S-ANNO-DIMS');
path_('d_wall', 'dim', [[-1, dy0], [W + 1, dy0]], false, 'S-ANNO-DIMS');
path_('d_wall', 'dim', [[-0.35, dy0 - 0.35], [0.35, dy0 + 0.35]], false, 'S-ANNO-DIMS');
path_('d_wall', 'dim', [[W - 0.35, dy0 - 0.35], [W + 0.35, dy0 + 0.35]], false, 'S-ANNO-DIMS');
text('d_wall', '7 5/8"', W / 2, dy0 + 0.4, 0.625, { align: 'center' });
// title
path_('title_A', 'title', [[-10, -41, 1], [-8, -41, 1]], true, 'S-ANNO-TTLB');
text('title_A', '1', -9, -40.5, 0.9, { align: 'center' });
text('title_A', 'TRUSS BEARING AT CMU WALL', -6, -40.8, 1.25);
path_('title_A', 'title', [[-6, -42], [30, -42]], false, 'S-ANNO-TTLB');
text('title_A', 'SCALE: 1" = 1\'-0"', -6, -44.6, 0.75);

const pens = {
  cut: { width_mm: 0.5, dash_mm: null }, profile: { width_mm: 0.35, dash_mm: null }, beyond: { width_mm: 0.18, dash_mm: null },
  hidden: { width_mm: 0.18, dash_mm: [2, 1] }, hatch: { width_mm: 0.09, dash_mm: null }, rebar: { width_mm: 0.35, dash_mm: null },
  steel: { width_mm: 0.35, dash_mm: null }, anno: { width_mm: 0.18, dash_mm: null }, dim: { width_mm: 0.13, dash_mm: null },
  break: { width_mm: 0.18, dash_mm: null }, title: { width_mm: 0.5, dash_mm: null },
};
const drawing = { kerf_drawing: '0.1', doc: 'fixture-detail', view: 'A', kind: 'section', scale: 12, bounds: [-24, -46, 60, 20], pens, layers: [{ name: 'S-DETL-CUT', lineweight_mm: 0.5 }], items, diagnostics: [] };
fs.writeFileSync(path.join(here, 'drawing-A.json'), JSON.stringify(drawing));

// --- mesh: boxes (extruded rects) with feature edges
const boxes = [
  ['cmu', 'wood', '#B9B4A8', 0, -H, 7.625, 0, -24, 24],
  ['sill_plate', 'wood_treated', '#A89F6A', 0.2, 0, 7.45, 1.5, -24, 24],
  ['truss', 'wood', '#C9A46A', -18, 1.5, 36, 5, -12.75, -11.25],
  ['truss', 'wood', '#C9A46A', -18, 1.5, 36, 5, 11.25, 12.75],
];
const parts = boxes.map(([src, material, color, x0, y0, x1, y1, z0, z1], k) => {
  const V = [[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]];
  const F = [[4, 5, 6, 7, [0, 0, 1]], [1, 0, 3, 2, [0, 0, -1]], [0, 4, 7, 3, [-1, 0, 0]], [5, 1, 2, 6, [1, 0, 0]], [3, 7, 6, 2, [0, 1, 0]], [0, 1, 5, 4, [0, -1, 0]]];
  const positions = [], normals = [], indices = [];
  F.forEach(([a, b, c, d, n], i) => { for (const v of [a, b, c, d]) { positions.push(...V[v]); normals.push(...n); } indices.push(i * 4, i * 4 + 1, i * 4 + 2, i * 4, i * 4 + 2, i * 4 + 3); });
  const E = [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];
  return { src, part: null, instance: k > 2 ? 1 : 0, material, color, positions, normals, indices, edges: E.flatMap(([a, b]) => [...V[a], ...V[b]]) };
});
fs.writeFileSync(path.join(here, 'mesh.json'), JSON.stringify({ kerf_mesh: '0.1', parts }));

// --- sheet svg placeholder
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 11 8.5" width="1056" height="816"><rect width="11" height="8.5" fill="#fff"/><rect x="0.375" y="0.375" width="10.25" height="7.75" fill="none" stroke="#000" stroke-width="0.0276"/><rect x="0.375" y="7.375" width="10.25" height="0.75" fill="none" stroke="#000" stroke-width="0.02"/><text x="0.5" y="7.6" font-family="monospace" font-size="0.1">DETAIL</text></svg>`;
fs.writeFileSync(path.join(here, 'sheet-A.svg'), svg);
console.log('fixtures written', items.length, 'items', parts.length, 'mesh parts');
