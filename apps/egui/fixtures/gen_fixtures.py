#!/usr/bin/env python3
"""Hand-written Drawing IR (SPEC 10) + Mesh (SPEC 11) fixtures used until kerf-core is wired in.
Run: python3 gen_fixtures.py   (writes demo_drawing.json, demo_mesh.json next to this file)"""
import json, math, os
here = os.path.dirname(os.path.abspath(__file__))
font = json.load(open(os.path.join(here, '../../../spec/fonts/kerf-simplex.json')))

def rect(x0, y0, x1, y1): return [[x0, y0, 0], [x1, y0, 0], [x1, y1, 0], [x0, y1, 0]]
def hatch_rect(x0, y0, x1, y1, spacing, ang=45):
    out = []
    # 45 degree family clipped to rect
    k = (y0 - x1); end = (y1 - x0)
    c = k - (k % spacing)
    while c <= end:
        # line y = x + c
        pts = []
        for (px, py) in [(x0, x0 + c), (x1, x1 + c)]:
            pass
        xa = max(x0, y0 - c); xb = min(x1, y1 - c)
        if xa < xb: out.append([xa, xa + c, xb, xb + c])
        c += spacing
    return out
items = []
def path(src, pen, pts, closed=True, layer='S-DETL-CUT'):
    items.append({"t": "path", "layer": layer, "pen": pen, "src": src, "closed": closed, "pts": pts})
def text(src, s, x, y, h=0.75, align='left'):
    items.append({"t": "text", "layer": "S-ANNO-NOTE", "pen": "anno", "src": src, "s": s, "x": x, "y": y, "h": h, "rot": 0, "align": align, "valign": "baseline"})

# CMU wall 7.625 x 32 (cut, hatched)
cmu = (0, -32, 7.625, 0)
path('cmu', 'cut', rect(*cmu))
items.append({"t": "hatch", "layer": "S-DETL-PATT", "pen": "hatch", "src": "cmu", "pattern": "ANSI31", "scale": 0.75, "angle": 0,
              "loops": [rect(*cmu)], "lines": hatch_rect(*cmu, 1.2)})
# grouted cell: inner rect w/ hatch
cell = (1.25, -32, 6.375, 0)
for y in range(-32, 0, 8): path('cmu', 'beyond', [[0, y, 0], [7.625, y, 0]], closed=False, layer='S-DETL-BYND')
# sill plate 2x8 flat: 7.25 x 1.5
sill = (0.19, 0, 7.44, 1.5)
path('sill_plate', 'cut', rect(*sill))
path('sill_plate', 'cut', [[sill[0], sill[1], 0], [sill[2], sill[3], 0]], closed=False)
path('sill_plate', 'cut', [[sill[0], sill[3], 0], [sill[2], sill[1], 0]], closed=False)
# rebar: circles as two-vertex bulge loops
def circle(cx, cy, r): return [[cx - r, cy, 1], [cx + r, cy, 1]]
for i, cx in enumerate([2.4, 5.2]):
    items.append({"t": "fill", "layer": "S-DETL-CUT", "src": "bb_bars", "loops": [circle(cx, -5.5, 0.3125)]})
# a path with a bulge: rounded-end slot (anchor strap)
path('hurricane_tie', 'steel', [[1.5, -1.2, 0], [1.8, -1.2, 0], [1.8, 3.3, 0], [1.5, 3.3, 0]])
# truss tail: sloped parallelogram
s = 4 / 12
path('truss', 'cut', [[0, 1.5, 0], [-18, 1.5 - 18 * s, 0], [-18, 1.5 - 18 * s + 3.5, 0], [0, 1.5 + 3.5 / math.cos(math.atan(s)) + 0, 0], [30, 1.5 + 30 * s + 3.5, 0], [30, 1.5 + 30 * s, 0]])
path('truss', 'cut', [[0, 1.5, 0], [30, 1.5, 0], [30, 5, 0], [0, 5, 0]], closed=True, layer='S-DETL-HIDN')
items[-1]['pen'] = 'hidden'
# break line (zigzag) on the crop edge
zz = [[-24, -32]]
for i in range(1, 17): zz.append([-24 + (i % 2) * 0.125 * 8 * 0.5, -32 + i * 2])
path('break', 'break', [[x, y, 0] for x, y in zz], closed=False, layer='S-DETL-BRKL')
# notes with leaders and arrowheads
def note(id_, lines, x, y, tx, ty):
    h = 0.75
    for i, ln in enumerate(lines): text(id_, ln, x, y - i * h * 1.6, h)
    path(id_, 'anno', [[x - 0.5, y + h / 2, 0], [x - 2, y + h / 2, 0], [tx, ty, 0]], closed=False, layer='S-ANNO-NOTE')
    # arrowhead (fill)
    items.append({"t": "fill", "layer": "S-ANNO-NOTE", "src": id_, "loops": [[[tx, ty, 0], [tx - 1.1, ty + 0.35, 0], [tx - 1.1, ty - 0.35, 0]]]})
note('n_sill', ['2X8 PT SILL PLATE W/ 5/8" DIA.', 'ANCHOR BOLTS @ 48" O.C.'], 12, 6, 7.2, 0.8)
note('n_cmu', ['8" CMU WALL, REINF. W/', '#5 @ 32" O.C. VERT.'], 12, -12, 7.6, -14)
text('title', 'TRUSS BEARING AT CMU WALL', -24, -40, 1.5)
text('scale', "SCALE: 1\" = 1'-0\"", -24, -44, 0.75)
# dimension line (aligned text)
path('d1', 'dim', [[-3, 0, 0], [-3, -32, 0]], closed=False, layer='S-ANNO-DIMS')
text('d1', "2'-8\"", -3.5, -16, 0.75, 'right')

drawing = {"kerf_drawing": "0.1", "doc": "demo", "view": "A", "kind": "section", "scale": 12,
           "bounds": [-26, -46, 42, 12],
           "pens": {"cut": {"width_mm": 0.5, "dash_mm": None}, "profile": {"width_mm": 0.35, "dash_mm": None},
                    "beyond": {"width_mm": 0.18, "dash_mm": None}, "hidden": {"width_mm": 0.18, "dash_mm": [2.0, 1.0]},
                    "hatch": {"width_mm": 0.09, "dash_mm": None}, "steel": {"width_mm": 0.35, "dash_mm": None},
                    "anno": {"width_mm": 0.18, "dash_mm": None}, "dim": {"width_mm": 0.13, "dash_mm": None},
                    "break": {"width_mm": 0.18, "dash_mm": None}},
           "layers": [{"name": "S-DETL-CUT", "lineweight_mm": 0.5}],
           "items": items, "diagnostics": []}
json.dump(drawing, open(os.path.join(here, 'demo_drawing.json'), 'w'), separators=(',', ':'))

# ---- mesh: boxes
parts = []
def box(src, x0, y0, z0, x1, y1, z1, material, color):
    P = []; N = []; I = []
    faces = [((0, 0, 1), [(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]),
             ((0, 0, -1), [(x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0)]),
             ((1, 0, 0), [(x1, y0, z1), (x1, y0, z0), (x1, y1, z0), (x1, y1, z1)]),
             ((-1, 0, 0), [(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)]),
             ((0, 1, 0), [(x0, y1, z1), (x1, y1, z1), (x1, y1, z0), (x0, y1, z0)]),
             ((0, -1, 0), [(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)])]
    for n, vs in faces:
        b = len(P) // 3
        for v in vs: P += list(v); N += list(n)
        I += [b, b + 1, b + 2, b, b + 2, b + 3]
    c = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0), (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
    E = []
    for a, b in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)]: E += list(c[a]) + list(c[b])
    parts.append({"src": src, "part": None, "instance": 0, "material": material, "color": color, "positions": P, "normals": N, "indices": I, "edges": E})
box('cmu', 0, -32, -24, 7.625, 0, 24, 'cmu', '#9C978C')
box('sill_plate', 0.19, 0, -24, 7.44, 1.5, 24, 'wood_treated', '#9DA56C')
box('truss', -18, 1.5, -12.75, 30, 5, -11.25, 'wood', '#C9A46A')
box('truss', -18, 1.5, 11.25, 30, 5, 12.75, 'wood', '#C9A46A')
json.dump({"kerf_mesh": "0.1", "parts": parts}, open(os.path.join(here, 'demo_mesh.json'), 'w'), separators=(',', ':'))
print('ok')
