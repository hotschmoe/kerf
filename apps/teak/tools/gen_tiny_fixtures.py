#!/usr/bin/env python3
"""Deterministic unit-test-sized fixtures for apps/teak/src/draw (hand-written, NOT engine output).

Writes fixtures/tiny.drawing.json and fixtures/tiny.mesh.json in the exact SPEC section 10 / 11 shapes.
The drawing covers every item type: closed cut path with a bulge, open dashed path, 2-vertex
circle fill, hatch with a hole and pre-clipped lines (one zero-length dot), left/center/right and
rotated text, a leader + arrowhead fill, and one diagnostic.
Run from anywhere:  python3 apps/teak/tools/gen_tiny_fixtures.py
"""
import json, os

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "..", "fixtures")
os.makedirs(out, exist_ok=True)

def r(x):  # canonical 4-decimal rounding like the engines
    return round(x + 0.0, 4)

pens = {
    "cut": {"width_mm": 0.5, "dash_mm": None},
    "beyond": {"width_mm": 0.18, "dash_mm": None},
    "hidden": {"width_mm": 0.18, "dash_mm": [2, 1]},
    "hatch": {"width_mm": 0.09, "dash_mm": None},
    "anno": {"width_mm": 0.18, "dash_mm": None},
    "dim": {"width_mm": 0.13, "dash_mm": None},
}
layers = [
    {"name": "S-DETL-CUT", "lineweight_mm": 0.5},
    {"name": "S-DETL-BYND", "lineweight_mm": 0.18},
    {"name": "S-DETL-HIDN", "lineweight_mm": 0.18},
    {"name": "S-DETL-PATT", "lineweight_mm": 0.09},
    {"name": "S-ANNO-NOTE", "lineweight_mm": 0.18},
]

block = [[0, 0, 0], [12, 0, 0], [12, 6, 0.5], [8, 8, 0], [0, 8, 0]]        # bulge on the (12,6)->(8,8) edge
hole = [[4, 3, 0], [6, 3, 0], [6, 5, 0], [4, 5, 0]]
import math

def arc_circle(p0, p1, b):
    """Center/radius of the DXF bulge arc p0->p1 (positive = CCW)."""
    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    c = math.hypot(dx, dy)
    mx, my = (p0[0] + p1[0]) / 2, (p0[1] + p1[1]) / 2
    off = (c / 2) * (1 - b * b) / (2 * b)
    cx, cy = mx + (-dy / c) * off, my + (dx / c) * off
    return cx, cy, (c / 2) * (1 + b * b) / (2 * abs(b))

cx, cy, rad = arc_circle((12, 6), (8, 8), 0.5)

def x_right(y):
    """Right boundary of the block at height y (straight edge x=12 up to y=6, then the bulge arc)."""
    if y <= 6:
        return 12.0
    dy = y - cy
    return cx + math.sqrt(max(rad * rad - dy * dy, 0.0))

lines = []
y = 0.25
while y < 8:                                    # horizontal pattern lines clipped to the block minus the hole
    xr = r(x_right(y))
    if 3 < y < 5:
        lines.append([0, r(y), 4, r(y)])
        lines.append([6, r(y), xr, r(y)])
    else:
        lines.append([0, r(y), xr, r(y)])
    y += 0.5
lines.append([2, 2, 2, 2])                      # zero-length dot

items = [
    {"t": "hatch", "layer": "S-DETL-PATT", "pen": "hatch", "src": "block", "pattern": "KERF-LAM", "scale": 1, "angle": 0,
     "loops": [block, hole], "lines": lines},
    {"t": "path", "layer": "S-DETL-CUT", "pen": "cut", "src": "block", "closed": True, "pts": block},
    {"t": "path", "layer": "S-DETL-CUT", "pen": "cut", "src": "hole", "closed": True, "pts": hole},
    {"t": "path", "layer": "S-DETL-HIDN", "pen": "hidden", "src": "block", "closed": False, "pts": [[-2, 4, 0], [0, 4, 0], [0, 7, 0]]},
    {"t": "fill", "layer": "S-DETL-CUT", "src": "bar", "loops": [[[9, 2, 1], [10, 2, 1]]]},
    {"t": "path", "layer": "S-ANNO-NOTE", "pen": "anno", "src": "n1", "closed": False, "pts": [[14, 6, 0], [10, 2.5, 0]]},
    {"t": "fill", "layer": "S-ANNO-NOTE", "src": "n1", "loops": [[[10, 2.5, 0], [10.6, 3.2, 0], [10.9, 2.6, 0]]]},
    {"t": "text", "layer": "S-ANNO-NOTE", "pen": "anno", "src": "n1", "s": "2X8 PT SILL", "x": 14.5, "y": 5.6, "h": 0.75, "rot": 0, "align": "left", "valign": "baseline"},
    {"t": "text", "layer": "S-ANNO-NOTE", "pen": "anno", "src": "n1", "s": "W/ 5/8\" BOLTS", "x": 14.5, "y": 4.4, "h": 0.75, "rot": 0, "align": "left", "valign": "baseline"},
    {"t": "text", "layer": "S-ANNO-NOTE", "pen": "anno", "src": "l1", "s": "EXTERIOR", "x": -4, "y": 4, "h": 0.65, "rot": 0, "align": "right", "valign": "middle"},
    {"t": "text", "layer": "S-ANNO-NOTE", "pen": "anno", "src": "l2", "s": "CENTER", "x": 6, "y": -1.5, "h": 0.75, "rot": 0, "align": "center", "valign": "top"},
    {"t": "text", "layer": "S-ANNO-NOTE", "pen": "dim", "src": "d1", "s": "1'-0\"", "x": -3.5, "y": 1.5, "h": 0.75, "rot": 90, "align": "center", "valign": "baseline"},
]
drawing = {
    "kerf_drawing": "0.1", "doc": "tiny", "view": "A", "kind": "section", "scale": 12,
    "bounds": [-9, -3, 24, 9], "pens": pens, "layers": layers, "items": items,
    "diagnostics": [{"level": "warning", "code": "W_FLOATING", "id": "bar", "message": "bar touches nothing (test fixture)."}],
}
with open(os.path.join(out, "tiny.drawing.json"), "w") as f:
    json.dump(drawing, f, separators=(",", ":"))
    f.write("\n")

# --- mesh: a 2x1x1 box (12 triangles, 12 feature edges) with flat normals + a wedge prism -------------
def box(x0, y0, z0, x1, y1, z1):
    P = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0), (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
    faces = [([0, 3, 2, 1], (0, 0, -1)), ([4, 5, 6, 7], (0, 0, 1)), ([0, 1, 5, 4], (0, -1, 0)),
             ([3, 7, 6, 2], (0, 1, 0)), ([0, 4, 7, 3], (-1, 0, 0)), ([1, 2, 6, 5], (1, 0, 0))]
    pos, nor, idx = [], [], []
    for quad, n in faces:
        base = len(pos) // 3
        for vi in quad:
            pos += list(P[vi]); nor += list(n)
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    ed = []
    for a, b in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)]:
        ed += list(P[a]) + list(P[b])
    return pos, nor, idx, ed

pos, nor, idx, ed = box(0, 0, 0, 2, 1, 1)
p1 = {"src": "sill", "part": None, "instance": 0, "material": "wood", "color": "#C9A46A",
      "positions": pos, "normals": nor, "indices": idx, "edges": ed}
pos, nor, idx, ed = box(0, -1, 0, 2, 0, 1)
p2 = {"src": "cmu", "part": "course_1", "instance": 0, "material": "cmu", "color": "#9C978C",
      "positions": pos, "normals": nor, "indices": idx, "edges": ed}
with open(os.path.join(out, "tiny.mesh.json"), "w") as f:
    json.dump({"kerf_mesh": "0.1", "parts": [p1, p2]}, f, separators=(",", ":"))
    f.write("\n")
print("wrote", os.path.normpath(out))
