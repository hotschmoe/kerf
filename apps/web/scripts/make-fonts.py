#!/usr/bin/env python3
"""Subset IBM Plex Mono (spec/fonts/*.ttf) to Latin + drafting symbols, add the few geometric
glyphs the 3270 status line needs (U+25AE ▮, U+25B8 ▸, U+25CB ○, U+25CF ●, U+25D0 ◐, U+25B2 ▲, U+25BC ▼,
U+25A0 ■, U+25A1 □), write woff2 to src/fonts/. Run with tools/.venv/bin/python (fonttools + brotli)."""
import math, sys
from pathlib import Path
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.pens.ttGlyphPen import TTGlyphPen

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parents[1] / "src" / "fonts"
OUT.mkdir(parents=True, exist_ok=True)

UNICODES = list(range(0x20, 0x7F)) + list(range(0xA0, 0x100)) + [
    0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026, 0x2032, 0x2033, 0x2190, 0x2191, 0x2192, 0x2193,
    0x2212, 0x2713, 0x2717, 0x2500, 0x2502, 0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524, 0x252C, 0x2534, 0x253C,
    0x2550, 0x255E, 0x2561, 0x2564, 0x2567, 0x2588, 0x2591, 0x2592, 0x2593, 0x258C, 0x2154, 0x00BD,
]

def poly(pen, pts):
    pen.moveTo(pts[0])
    for p in pts[1:]:
        pen.lineTo(p)
    pen.closePath()

def circle(cx, cy, r, n=28, rev=False):
    pts = [(round(cx + r * math.cos(2 * math.pi * i / n)), round(cy + r * math.sin(2 * math.pi * i / n))) for i in range(n)]
    return pts[::-1] if rev else pts

def make_glyphs():
    g = {}
    def new(): return TTGlyphPen(None)
    p = new(); poly(p, [(180, 0), (420, 0), (420, 700), (180, 700)][::-1]); g["blockbar"] = (0x25AE, p.glyph())
    p = new(); poly(p, [(110, 620), (110, 60), (500, 340)][::-1]); g["tri_r"] = (0x25B8, p.glyph())
    p = new(); poly(p, [(100, 120), (500, 120), (300, 540)][::-1]); g["tri_u"] = (0x25B2, p.glyph())
    p = new(); poly(p, [(100, 540), (500, 540), (300, 120)][::-1]); g["tri_d"] = (0x25BC, p.glyph())
    p = new(); poly(p, [(110, 60), (490, 60), (490, 600), (110, 600)][::-1]); g["sq_f"] = (0x25A0, p.glyph())
    p = new(); poly(p, [(110, 60), (490, 60), (490, 600), (110, 600)][::-1]); poly(p, [(160, 110), (160, 550), (440, 550), (440, 110)]); g["sq_o"] = (0x25A1, p.glyph())
    p = new(); poly(p, circle(300, 330, 220)[::-1]); g["dot_f"] = (0x25CF, p.glyph())
    p = new(); poly(p, circle(300, 330, 220)[::-1]); poly(p, circle(300, 330, 160)); g["dot_o"] = (0x25CB, p.glyph())
    # half-filled circle: outer ring + filled right half
    p = new(); poly(p, circle(300, 330, 220)[::-1]); poly(p, circle(300, 330, 160))
    half = [(round(300 + 160 * math.cos(math.pi / 2 - math.pi * i / 14)), round(330 + 160 * math.sin(math.pi / 2 - math.pi * i / 14))) for i in range(15)]
    poly(p, half[::-1])
    g["dot_h"] = (0x25D0, p.glyph())
    return g

for w, name in ((400, "Regular"), (500, "Medium"), (700, "Bold")):
    src = ROOT / "spec" / "fonts" / f"IBMPlexMono-{name}.ttf"
    font = TTFont(src)
    glyf = font["glyf"]; hmtx = font["hmtx"]; cmap = font.getBestCmap()
    order = font.getGlyphOrder()
    for gname, (uni, glyph) in make_glyphs().items():
        n = "kerf_" + gname
        glyf[n] = glyph
        hmtx[n] = (600, 0)
        for t in font["cmap"].tables:
            if t.isUnicode():
                t.cmap[uni] = n
    tmp = OUT / f"_tmp-{w}.ttf"
    font.save(tmp)
    opts = subset.Options()
    opts.flavor = "woff2"; opts.layout_features = ["kern", "liga", "calt"]; opts.name_IDs = [0, 1, 2, 4, 6]
    opts.notdef_outline = True; opts.hinting = False; opts.desubroutinize = True
    f2 = subset.load_font(str(tmp), opts)
    ss = subset.Subsetter(opts); ss.populate(unicodes=UNICODES + [0x25AE, 0x25B8, 0x25CB, 0x25CF, 0x25D0, 0x25B2, 0x25BC, 0x25A0, 0x25A1]); ss.subset(f2)
    out = OUT / f"plex-mono-{w}.woff2"
    subset.save_font(f2, str(out), opts)
    tmp.unlink()
    print(out, out.stat().st_size)
