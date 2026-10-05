// Independent geometric checks computed from the engine's drawing IR (`kerf call drawing`),
// not from the engine's own warnings, so they keep working when W_* codes change.
//
//   import { geometryChecks } from "./geom.mjs";
//   const g = geometryChecks(doc, drawing);   // { applicable, metrics, findings }
//
// Text widths are estimated (the engine stroke font is not exported): ~0.88 x cap height per
// character, measured on rendered PNGs. Counts are therefore approximate; they are meant as gates
// (0 expected) and trends, not exact measurements.

const CHAR_W = 0.88;

const baseId = (src) => String(src ?? "").replace(/#\d+$/, "");

function textRect(t) {
  const n = String(t.s ?? "").length;
  const w = n * CHAR_W * t.h;
  const h = t.h;
  const rot = ((t.rot ?? 0) % 360 + 360) % 360;
  if (rot === 90 || rot === 270) {
    // rotated text: width runs along y; the baseline is the vertical line at x and the glyphs extend
    // to the left (rot 90) or right (rot 270) of it.
    const y0 = t.align === "center" ? t.y - w / 2 : t.align === "right" ? t.y - w : t.y;
    return rot === 90 ? [t.x - h, y0, t.x, y0 + w] : [t.x, y0, t.x + h, y0 + w];
  }
  const x0 = t.align === "center" ? t.x - w / 2 : t.align === "right" ? t.x - w : t.x;
  const y0 = t.valign === "middle" ? t.y - h / 2 : t.valign === "top" ? t.y - h : t.y;
  return [x0, y0, x0 + w, y0 + h];
}

const rectsOverlap = (a, b, eps = 0.02) => a[0] < b[2] - eps && b[0] < a[2] - eps && a[1] < b[3] - eps && b[1] < a[3] - eps;

function segSeg(p1, p2, p3, p4) {
  const d = (a, b, c) => (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0]);
  const d1 = d(p3, p4, p1), d2 = d(p3, p4, p2), d3 = d(p1, p2, p3), d4 = d(p1, p2, p4);
  return d1 * d2 < 0 && d3 * d4 < 0; // proper crossing only (touching endpoints do not count)
}

function segRect(a, b, r) {
  const inside = (p) => p[0] > r[0] && p[0] < r[2] && p[1] > r[1] && p[1] < r[3];
  if (inside(a) || inside(b)) return true;
  const c = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
  for (let i = 0; i < 4; i++) if (segSeg(a, b, c[i], c[(i + 1) % 4])) return true;
  return false;
}

function segments(item) {
  const pts = item.pts ?? [];
  const out = [];
  for (let i = 0; i + 1 < pts.length; i++) out.push([pts[i], pts[i + 1]]);
  if (item.closed && pts.length > 2) out.push([pts[pts.length - 1], pts[0]]);
  return out;
}

function bboxOf(items) {
  let b = null;
  for (const it of items) {
    const pts = it.pts ?? (it.loops ? it.loops.flat() : []);
    for (const p of pts) {
      b ??= [Infinity, Infinity, -Infinity, -Infinity];
      b[0] = Math.min(b[0], p[0]); b[1] = Math.min(b[1], p[1]);
      b[2] = Math.max(b[2], p[0]); b[3] = Math.max(b[3], p[1]);
    }
  }
  return b;
}

// Component geometry: cut regions and drawn paths of the component (all instances), not annotations.
function compItems(drawing, id) {
  return drawing.items.filter((it) => baseId(it.src) === id && (it.t === "region" || it.t === "path" || it.t === "fill") && !String(it.layer ?? "").startsWith("S-ANNO"));
}

function cropRect(doc, drawing) {
  const v = (doc.views ?? []).find((x) => x.id === drawing.view) ?? doc.views?.[0];
  if (v?.crop) return [v.crop.x[0], v.crop.y[0], v.crop.x[1], v.crop.y[1]];
  // auto crop: the engine's crop path items are only break lines, so fall back to the extent of
  // everything drawn (a member touching that extent is treated as possibly clipped).
  return bboxOf(drawing.items.filter((it) => it.t === "region" && !String(it.layer ?? "").startsWith("S-ANNO")));
}

const ENGINEERED = new Set(["lvl", "psl", "lsl", "glulam"]);
const STRAP_RE = /^(CS|CMST|CMSTC|MSTA|LSTA|MST|ST|LST|MSTC|CMSTC)\d/i;

export function geometryChecks(doc, drawing) {
  const metrics = {};
  const findings = [];
  const applicable = [];
  const items = drawing.items ?? [];

  // ---- annotation layout: leader crossings, leader/text hits, dim text overlaps --------------
  const annoTexts = items.filter((it) => it.t === "text" && String(it.layer).startsWith("S-ANNO") && !String(it.src).startsWith("title:") && it.src !== "footnote")
    .map((it) => ({ src: it.src, rect: textRect(it), layer: it.layer, s: it.s }));
  const isDimSrc = (s) => items.some((it) => it.src === s && it.layer === "S-ANNO-DIMS");
  const leaders = new Map(); // note id -> segments
  const dimLines = new Map(); // dim id -> segments
  for (const it of items) {
    if (it.t !== "path") continue;
    if (it.layer === "S-ANNO-NOTE") (leaders.get(it.src) ?? leaders.set(it.src, []).get(it.src)).push(...segments(it));
    if (it.layer === "S-ANNO-DIMS") (dimLines.get(it.src) ?? dimLines.set(it.src, []).get(it.src)).push(...segments(it));
  }

  const ids = [...leaders.keys()];
  let crossings = 0;
  const crossPairs = [];
  for (let i = 0; i < ids.length; i++) for (let j = i + 1; j < ids.length; j++) {
    let hit = false;
    for (const a of leaders.get(ids[i])) { for (const b of leaders.get(ids[j])) if (segSeg(a[0], a[1], b[0], b[1])) { hit = true; break; } if (hit) break; }
    if (hit) { crossings++; crossPairs.push(`${ids[i]} x ${ids[j]}`); }
  }
  metrics.leader_crossings = crossings;
  if (crossings) findings.push(`leaders cross: ${crossPairs.slice(0, 6).join(", ")}`);

  let leaderTextHits = 0;
  const hitList = [];
  for (const [id, segs] of leaders) {
    for (const t of annoTexts) {
      if (t.src === id) continue;
      if (segs.some((s) => segRect(s[0], s[1], t.rect))) { leaderTextHits++; hitList.push(`${id} -> "${t.s}" (${t.src})`); }
    }
  }
  metrics.leader_text_hits = leaderTextHits;
  if (leaderTextHits) findings.push(`leader runs through other text: ${hitList.slice(0, 5).join("; ")}`);

  let dimOverlaps = 0;
  const dimList = [];
  for (const t of annoTexts.filter((x) => isDimSrc(x.src))) {
    let bad = null;
    for (const o of annoTexts) if (o.src !== t.src && rectsOverlap(t.rect, o.rect)) { bad = `text "${o.s}"`; break; }
    if (!bad) for (const [id, segs] of [...leaders, ...dimLines]) {
      if (id === t.src) continue;
      if (segs.some((s) => segRect(s[0], s[1], t.rect))) { bad = `lines of ${id}`; break; }
    }
    if (bad) { dimOverlaps++; dimList.push(`dim text "${t.s}" (${t.src}) overlaps ${bad}`); }
  }
  // dim text sitting on drawn geometry outlines (cut/profile pens) is also a legibility defect
  const geomSegs = items.filter((it) => it.t === "path" && (it.pen === "cut" || it.pen === "profile") && it.layer === "S-DETL-CUT" && !isDimSrc(it.src) && !leaders.has(it.src))
    .flatMap((it) => segments(it));
  let dimOnGeom = 0;
  for (const t of annoTexts.filter((x) => isDimSrc(x.src))) {
    if (geomSegs.some((sg) => segRect(sg[0], sg[1], t.rect))) { dimOnGeom++; dimList.push(`dim text "${t.s}" (${t.src}) sits on drawn geometry`); }
  }
  metrics.dim_text_on_geometry = dimOnGeom;
  metrics.dim_text_overlaps = dimOverlaps;
  if (dimOverlaps || dimOnGeom) findings.push(...dimList.slice(0, 6));

  let textOverlaps = 0;
  for (let i = 0; i < annoTexts.length; i++) for (let j = i + 1; j < annoTexts.length; j++) {
    if (annoTexts[i].src !== annoTexts[j].src && rectsOverlap(annoTexts[i].rect, annoTexts[j].rect)) textOverlaps++;
  }
  metrics.text_overlaps = textOverlaps;
  if (textOverlaps) findings.push(`${textOverlaps} overlapping annotation text box pair(s)`);
  applicable.push("leader_crossings", "leader_text_hits", "dim_text_overlaps", "dim_text_on_geometry", "text_overlaps");

  // ---- sloped panel/membrane end vs its host member -----------------------------------------
  const comps = doc.components ?? [];
  const crop = cropRect(doc, drawing);
  const hosts = comps.filter((c) => c.type === "truss" || (c.type === "lumber" && c.slope));
  const sloped = comps.filter((c) => (c.type === "panel" || c.type === "membrane") && (c.slope || /^roof|shingle|underlay/i.test(c.id)));
  if (hosts.length && sloped.length) {
    applicable.push("sloped_panel_short");
    const short = [];
    const clipped = (b) => crop && (b[2] >= crop[2] - 0.05 || b[3] >= crop[3] - 0.05 || b[0] <= crop[0] + 0.05 || b[1] <= crop[1] + 0.05);
    for (const p of sloped) {
      const pb = bboxOf(compItems(drawing, p.id));
      if (!pb) continue;
      for (const h of hosts) {
        const hb = bboxOf(compItems(drawing, h.id));
        if (!hb) continue;
        // far end: the side where the host extends furthest (horizontally), tail end on the other side
        const farRight = hb[2] - pb[2];
        const tailLeft = pb[0] - hb[0];
        const pClippedFar = crop ? pb[2] >= crop[2] - 0.05 || pb[3] >= crop[3] - 0.05 : false;
        if (farRight > 3 && !pClippedFar && !clipped(pb)) short.push(`${p.id} ends ${farRight.toFixed(1)}" short of ${h.id} (x ${pb[2].toFixed(1)} vs ${hb[2].toFixed(1)})`);
        const pOver = pb[0] - hb[0];
        if (tailLeft > 6 && p.id !== "roofing") short.push(`${p.id} starts ${tailLeft.toFixed(1)}" inside ${h.id}'s tail`);
        void pOver;
      }
    }
    metrics.sloped_panel_short = short.length;
    if (short.length) findings.push(...short);
  }

  // ---- strap length vs beam width / joint ----------------------------------------------------
  const straps = comps.filter((c) => c.type === "connector" && (STRAP_RE.test(c.model ?? "") || /strap/i.test(`${c.id} ${c.label ?? ""}`)));
  const beams = comps.filter((c) => c.type === "lumber" && (ENGINEERED.has(String(c.product ?? "").toLowerCase()) || Number(c.plies ?? 1) > 1));
  if (straps.length && beams.length) {
    applicable.push("strap_overhang");
    const MIN = 6; // inches of strap past each beam face / cut joint
    const bad = [];
    const overall = bboxOf(items.filter((it) => (it.t === "region") && !String(it.layer ?? "").startsWith("S-ANNO")));
    for (const s of straps) {
      const sb = bboxOf(compItems(drawing, s.id));
      if (!sb) continue;
      const beam = beams.map((b) => ({ b, bb: bboxOf(compItems(drawing, b.id)) })).filter((x) => x.bb)
        .sort((a, c) => (c.bb[2] - c.bb[0]) * (c.bb[3] - c.bb[1]) - (a.bb[2] - a.bb[0]) * (a.bb[3] - a.bb[1]))[0];
      if (!beam) continue;
      const bb = beam.bb;
      const leftOpen = overall && bb[0] <= overall[0] + 1;
      const rightOpen = overall && bb[2] >= overall[2] - 1;
      const left = bb[0] - sb[0];
      const right = sb[2] - bb[2];
      const len = sb[2] - sb[0];
      const w = bb[2] - bb[0];
      metrics.strap_length_in = +len.toFixed(2);
      metrics.beam_width_in = +w.toFixed(2);
      if (!leftOpen && left < MIN) bad.push(`${s.id} extends only ${left.toFixed(1)}" past the left face of ${beam.b.id} (min ${MIN}")`);
      if (!rightOpen && right < MIN) bad.push(`${s.id} extends only ${right.toFixed(1)}" past the right face of ${beam.b.id} (min ${MIN}")`);
      if (len < w + 2 * MIN && leftOpen === false && rightOpen === false) bad.push(`${s.id} length ${len.toFixed(1)}" < beam width ${w.toFixed(1)}" + 2x${MIN}"`);
      // a strap that only covers one side of an open-ended beam must still cross the joint by MIN
      if (leftOpen && right < 0) bad.push(`${s.id} does not reach the end of ${beam.b.id}`);
    }
    metrics.strap_problems = bad.length;
    if (bad.length) findings.push(...bad);
  }

  return { applicable, metrics, findings };
}
