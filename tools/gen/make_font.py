"""Generate spec/fonts/kerf-simplex.json from the public-domain Hershey Roman Simplex
(rowmans) glyphs. Coordinates: x from left side bearing, y up, baseline at 0, cap height 21."""
import json
from HersheyFonts import HersheyFonts

f = HersheyFonts()
f.load_default_font("rowmans")
glyphs = {}
base = None
for code in range(32, 127):
    ch = chr(code)
    g = list(f.glyphs_for_text(ch))[0]
    if base is None:
        base = g.base_line  # y-down units; baseline offset
    strokes = []
    for s in g.strokes:
        strokes.append([[int(x - g.left_offset), int(base - y)] for x, y in s])
    glyphs[ch] = {"adv": g.char_width, "strokes": strokes}
cap = glyphs["H"]["strokes"]
cap_h = max(p[1] for s in cap for p in s) - min(p[1] for s in cap for p in s)
out = {
    "name": "kerf-simplex",
    "source": "Hershey Roman Simplex (rowmans). Hershey fonts: U.S. National Bureau of Standards, public domain; derived data distributed with attribution.",
    "cap_height": int(cap_h),
    "baseline": 0,
    "glyphs": glyphs,
}
json.dump(out, open("spec/fonts/kerf-simplex.json", "w"), separators=(",", ":"))
print("cap", cap_h, "H adv", glyphs["H"]["adv"], "A", glyphs["A"])
