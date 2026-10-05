#!/usr/bin/env python3
"""Generate a sample DXF exercising lines/arcs/LWPOLYLINE bulge/HATCH ANSI31+AR-CONC/MTEXT/leader/dimension. Usage: make_sample_dxf.py out.dxf"""
import sys, ezdxf
from ezdxf.enums import TextEntityAlignment
doc = ezdxf.new("R2018", setup=True)
doc.units = 1  # inches
for n, c in (("A-WALL", 7), ("A-HATCH", 8), ("A-ANNO", 1), ("A-DIMS", 3)):
    doc.layers.add(n, color=c)
msp = doc.modelspace()
msp.add_line((0, 0), (10, 0), dxfattribs={"layer": "A-WALL"})
msp.add_line((10, 0), (10, 6), dxfattribs={"layer": "A-WALL"})
msp.add_arc((5, 6), 5, 0, 180, dxfattribs={"layer": "A-WALL"})
msp.add_lwpolyline([(0, -4, 0, 0, 0.5), (4, -4, 0, 0, -0.5), (4, -2), (0, -2)], format="xyseb", close=True, dxfattribs={"layer": "A-WALL"})
h1 = msp.add_hatch(dxfattribs={"layer": "A-HATCH"}); h1.set_pattern_fill("ANSI31", scale=0.25)
h1.paths.add_polyline_path([(5, -4), (9, -4), (9, -2), (5, -2)], is_closed=True)
h2 = msp.add_hatch(dxfattribs={"layer": "A-HATCH"}); h2.set_pattern_fill("AR-CONC", scale=0.02)
h2.paths.add_polyline_path([(10, -4), (14, -4), (14, -2), (10, -2)], is_closed=True)
msp.add_mtext("2x4 STUD @ 16\" O.C.\\PTYP.", dxfattribs={"layer": "A-ANNO", "char_height": 0.3, "insert": (11, 2)})
msp.add_text("NOTE", height=0.25, dxfattribs={"layer": "A-ANNO"}).set_placement((11, 4), align=TextEntityAlignment.LEFT)
msp.add_leader([(11, 2), (9.5, 1), (8, 1)], dxfattribs={"layer": "A-ANNO"})
msp.add_linear_dim(base=(0, -1), p1=(0, 0), p2=(10, 0), dxfattribs={"layer": "A-DIMS"}).render()
doc.saveas(sys.argv[1])
