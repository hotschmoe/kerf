//! DXF exporter (SPEC 12): AutoCAD R2000 (AC1015) ASCII, modelspace at 1:1 model inches,
//! $INSUNITS = 1. Entities: LWPOLYLINE (bulges), HATCH (pattern definitions embedded, or SOLID),
//! TEXT (style KERF -> romans.shx). Handles are sequential from a fixed seed; no timestamps.

use crate::drawing::{Align, Drawing, Item, VAlign};
use crate::geom::{self, Pt};
use crate::json;
use crate::pen::Pen;
use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;

const MODEL_SPACE_HANDLE: &str = "1F";
const PAPER_SPACE_HANDLE: &str = "1B";

struct Writer {
    out: Vec<u8>,
    next_handle: u32,
}

fn push_int(out: &mut Vec<u8>, v: i64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    let mut n = v.unsigned_abs();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(&tmp[i..]);
}

impl Writer {
    fn new() -> Writer {
        Writer { out: Vec::new(), next_handle: 0x100 }
    }

    fn code(&mut self, c: u32) {
        push_int(&mut self.out, i64::from(c));
        self.out.push(b'\n');
    }

    fn lit(&mut self, pairs: &[(u32, &str)]) {
        for &(c, v) in pairs {
            self.s(c, v);
        }
    }

    fn s(&mut self, c: u32, v: &str) {
        self.code(c);
        self.out.extend_from_slice(v.as_bytes());
        self.out.push(b'\n');
    }

    fn i(&mut self, c: u32, v: i64) {
        self.code(c);
        push_int(&mut self.out, v);
        self.out.push(b'\n');
    }

    fn f(&mut self, c: u32, v: f64) {
        self.code(c);
        let mut b = [0u8; 40];
        self.out.extend_from_slice(fmt_f(&mut b, v).as_bytes());
        self.out.push(b'\n');
    }

    fn hex(&mut self, c: u32, v: u32) {
        self.code(c);
        let mut tmp = [0u8; 8];
        let mut i = tmp.len();
        let mut n = v;
        loop {
            i -= 1;
            tmp[i] = b"0123456789ABCDEF"[(n & 15) as usize];
            n >>= 4;
            if n == 0 {
                break;
            }
        }
        self.out.extend_from_slice(&tmp[i..]);
        self.out.push(b'\n');
    }

    fn handle(&mut self) {
        let h = self.next_handle;
        self.next_handle += 1;
        self.hex(5, h);
    }
}

/// Shortest decimal of the value rounded to 1e-6 (deterministic, no exponent, never overflows).
pub fn fmt_f(buf: &mut [u8; 40], x: f64) -> &str {
    json::fmt_fixed(buf, x, 6)
}

const STD_LINEWEIGHTS: [i32; 24] =
    [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

pub fn lineweight_for(mm: f64) -> i32 {
    let target = mm * 100.0;
    let mut best = 0;
    let mut bd = 1e9;
    for w in STD_LINEWEIGHTS {
        let d = (f64::from(w) - target).abs();
        if d < bd {
            bd = d;
            best = w;
        }
    }
    best
}

fn linetype_name(pen: &str) -> String {
    if pen == "hidden" {
        return String::from("DASHED");
    }
    let mut out = String::from("KERF_");
    out.extend(pen.chars().map(|c| c.to_ascii_uppercase()));
    out
}

fn pen_lt(st: &Style, pen: Pen) -> Option<String> {
    let p = st.pen(pen)?;
    p.dash_mm.as_ref()?;
    Some(linetype_name(pen.name()))
}

fn entity_header(w: &mut Writer, etype: &str, layer: &str, lw: i32, lt: Option<&str>) {
    w.s(0, etype);
    w.handle();
    w.s(330, MODEL_SPACE_HANDLE);
    w.lit(&[(100, "AcDbEntity")]);
    w.s(8, layer);
    if let Some(l) = lt {
        w.s(6, l);
    }
    w.i(370, i64::from(lw));
}

fn lwpoly(w: &mut Writer, layer: &str, lw: i32, lt: Option<&str>, closed: bool, pts: &[Pt]) {
    if pts.len() < 2 {
        return;
    }
    entity_header(w, "LWPOLYLINE", layer, lw, lt);
    w.lit(&[(100, "AcDbPolyline")]);
    w.i(90, pts.len() as i64);
    w.i(70, i64::from(closed));
    w.lit(&[(43, "0")]);
    for p in pts {
        w.f(10, p.x);
        w.f(20, p.y);
        if p.b != 0.0 {
            w.f(42, p.b);
        }
    }
}

fn hatch_boundary(w: &mut Writer, loops: &[Vec<Pt>]) {
    w.i(91, loops.len() as i64);
    for (k, l) in loops.iter().enumerate() {
        w.i(92, if k == 0 { 3 } else { 2 });
        let has_b = l.iter().any(|p| p.b != 0.0);
        w.i(72, i64::from(has_b));
        w.lit(&[(73, "1")]);
        w.i(93, l.len() as i64);
        for p in l {
            w.f(10, p.x);
            w.f(20, p.y);
            if has_b {
                w.f(42, p.b);
            }
        }
        w.lit(&[(97, "0")]);
    }
}

fn hatch_header(w: &mut Writer, layer: &str, lw: i32, name: &str, solid: bool) {
    entity_header(w, "HATCH", layer, lw, None);
    w.lit(&[(100, "AcDbHatch"), (10, "0"), (20, "0"), (30, "0"), (210, "0"), (220, "0"), (230, "1")]);
    w.s(2, name);
    w.i(70, i64::from(solid));
    w.lit(&[(71, "0")]);
}

pub fn render(d: &Drawing<'_>) -> Vec<u8> {
    let st = d.style;
    let mut w = Writer::new();
    let mut ext = geom::Box::default();
    for it in &d.items {
        match it {
            Item::Path(p) => {
                let lw = lineweight_for(st.pen_width_mm(p.pen));
                lwpoly(&mut w, &p.layer, lw, pen_lt(st, p.pen).as_deref(), p.closed, &p.pts);
                ext.add_box(geom::points_box(&p.pts));
            }
            Item::Fill(f) => {
                hatch_header(&mut w, &f.layer, lineweight_for(st.pen_width_mm(Pen::Cut)), "SOLID", true);
                hatch_boundary(&mut w, &f.loops);
                w.lit(&[(75, "0"), (76, "1"), (98, "0")]);
                for l in &f.loops {
                    ext.add_box(geom::points_box(l));
                }
            }
            Item::Hatch(h) => {
                let pat = st.pattern(&h.pattern);
                if h.loops.is_empty() {
                    continue;
                }
                hatch_header(&mut w, &h.layer, lineweight_for(st.pen_width_mm(h.pen)), &h.pattern, false);
                hatch_boundary(&mut w, &h.loops);
                w.lit(&[(75, "0"), (76, "1")]);
                w.f(52, h.angle);
                w.lit(&[(41, "1"), (77, "0")]);
                let k = d.scale * h.scale;
                if let Some(p) = pat {
                    w.i(78, p.families.len() as i64);
                    for fam in &p.families {
                        w.f(53, fam.angle);
                        w.f(43, fam.x0 * k);
                        w.f(44, fam.y0 * k);
                        w.f(45, fam.dx * k);
                        w.f(46, fam.dy * k);
                        w.i(79, fam.dashes.len() as i64);
                        for dd in &fam.dashes {
                            w.f(49, dd * k);
                        }
                    }
                } else {
                    w.i(78, 0);
                }
                w.lit(&[(98, "0")]);
                for l in &h.loops {
                    ext.add_box(geom::points_box(l));
                }
            }
            Item::Region(_) => {}
            Item::Text(t) => {
                if t.s.is_empty() {
                    continue;
                }
                entity_header(&mut w, "TEXT", &t.layer, lineweight_for(st.pen_width_mm(t.pen)), None);
                w.lit(&[(100, "AcDbText")]);
                w.f(10, t.x);
                w.f(20, t.y);
                w.lit(&[(30, "0")]);
                w.f(40, t.h);
                w.s(1, &t.s);
                if t.rot != 0.0 {
                    w.f(50, t.rot);
                }
                w.s(7, &st.dxf_style);
                let ha = match t.align {
                    Align::Left => 0,
                    Align::Center => 1,
                    Align::Right => 2,
                };
                let va = match t.valign {
                    VAlign::Baseline => 0,
                    VAlign::Middle => 2,
                };
                if ha != 0 || va != 0 {
                    w.i(72, ha);
                    w.f(11, t.x);
                    w.f(21, t.y);
                    w.lit(&[(31, "0")]);
                }
                w.lit(&[(100, "AcDbText")]);
                if va != 0 {
                    w.i(73, va);
                }
                ext.add_point(t.x, t.y);
            }
        }
    }
    if ext.is_empty() {
        ext = geom::Box { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 };
    }
    let entities = w.out;
    let seed = w.next_handle;

    let mut o = Writer::new();
    o.lit(&[(0, "SECTION"), (2, "HEADER"), (9, "$ACADVER"), (1, "AC1015"), (9, "$HANDSEED")]);
    o.hex(5, seed);
    o.lit(&[(9, "$INSUNITS"), (70, "1"), (9, "$MEASUREMENT"), (70, "0"), (9, "$LTSCALE"), (40, "1"), (9, "$EXTMIN")]);
    o.f(10, ext.x0);
    o.f(20, ext.y0);
    o.lit(&[(30, "0"), (9, "$EXTMAX")]);
    o.f(10, ext.x1);
    o.f(20, ext.y1);
    o.lit(&[(30, "0"), (0, "ENDSEC"), (0, "SECTION"), (2, "CLASSES"), (0, "ENDSEC")]);

    // TABLES
    o.lit(&[(0, "SECTION"), (2, "TABLES")]);
    // VPORT
    o.lit(&[
        (0, "TABLE"),
        (2, "VPORT"),
        (5, "8"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "1"),
        (0, "VPORT"),
        (5, "30"),
        (330, "8"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbViewportTableRecord"),
        (2, "*ACTIVE"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (11, "1"),
        (21, "1"),
    ]);
    let cx = (ext.x0 + ext.x1) / 2.0;
    let cy = (ext.y0 + ext.y1) / 2.0;
    o.f(12, cx);
    o.f(22, cy);
    o.lit(&[
        (13, "0"),
        (23, "0"),
        (14, "10"),
        (24, "10"),
        (15, "10"),
        (25, "10"),
        (16, "0"),
        (26, "0"),
        (36, "1"),
        (17, "0"),
        (27, "0"),
        (37, "0"),
    ]);
    o.f(40, (ext.y1 - ext.y0).max(1.0) * 1.1);
    o.f(41, ((ext.x1 - ext.x0) / (ext.y1 - ext.y0).max(1.0)).max(0.1));
    o.lit(&[
        (42, "50"),
        (43, "0"),
        (44, "0"),
        (50, "0"),
        (51, "0"),
        (71, "0"),
        (72, "100"),
        (73, "1"),
        (74, "3"),
        (75, "0"),
        (76, "0"),
        (77, "0"),
        (78, "0"),
        (0, "ENDTAB"),
    ]);

    // LTYPE
    let mut lts: Vec<(String, &[f64])> = Vec::new();
    for p in &st.pens {
        let Some(dm) = p.dash_mm.as_deref() else { continue };
        let used = d.items.iter().any(|it| matches!(it, Item::Path(x) if x.pen.name() == p.name));
        if used || p.name == "hidden" {
            lts.push((linetype_name(&p.name), dm));
        }
    }
    o.lit(&[(0, "TABLE"), (2, "LTYPE"), (5, "5"), (330, "0"), (100, "AcDbSymbolTable")]);
    o.i(70, 3 + lts.len() as i64);
    o.lit(&[
        (0, "LTYPE"),
        (5, "14"),
        (330, "5"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbLinetypeTableRecord"),
        (2, "BYBLOCK"),
        (70, "0"),
        (3, ""),
        (72, "65"),
        (73, "0"),
        (40, "0"),
        (0, "LTYPE"),
        (5, "15"),
        (330, "5"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbLinetypeTableRecord"),
        (2, "BYLAYER"),
        (70, "0"),
        (3, ""),
        (72, "65"),
        (73, "0"),
        (40, "0"),
        (0, "LTYPE"),
        (5, "16"),
        (330, "5"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbLinetypeTableRecord"),
        (2, "CONTINUOUS"),
        (70, "0"),
        (3, "Solid line"),
        (72, "65"),
        (73, "0"),
        (40, "0"),
    ]);
    for (idx, (name, dm)) in lts.iter().enumerate() {
        o.lit(&[(0, "LTYPE")]);
        o.hex(5, 0x40 + idx as u32);
        o.lit(&[(330, "5"), (100, "AcDbSymbolTableRecord"), (100, "AcDbLinetypeTableRecord")]);
        o.s(2, name);
        o.lit(&[(70, "0")]);
        o.s(3, name);
        o.lit(&[(72, "65")]);
        o.i(73, dm.len() as i64);
        let total: f64 = dm.iter().map(|x| x / 25.4 * d.scale).sum();
        o.f(40, total);
        for (k, x) in dm.iter().enumerate() {
            let len = x / 25.4 * d.scale;
            o.f(49, if k % 2 == 0 { len } else { -len });
            o.lit(&[(74, "0")]);
        }
    }
    o.lit(&[(0, "ENDTAB")]);

    // LAYER
    o.lit(&[(0, "TABLE"), (2, "LAYER"), (5, "2"), (330, "0"), (100, "AcDbSymbolTable")]);
    o.i(70, 1 + d.layers.len() as i64);
    o.lit(&[
        (0, "LAYER"),
        (5, "10"),
        (330, "2"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbLayerTableRecord"),
        (2, "0"),
        (70, "0"),
        (62, "7"),
        (6, "CONTINUOUS"),
        (370, "-3"),
    ]);
    for (idx, l) in d.layers.iter().enumerate() {
        o.lit(&[(0, "LAYER")]);
        o.hex(5, 0x50 + idx as u32);
        o.lit(&[(330, "2"), (100, "AcDbSymbolTableRecord"), (100, "AcDbLayerTableRecord")]);
        o.s(2, &l.name);
        o.lit(&[(70, "0"), (62, "7")]);
        o.s(6, if l.linetype == "DASHED" { "DASHED" } else { "CONTINUOUS" });
        o.i(370, i64::from(lineweight_for(l.lineweight_mm)));
    }
    o.lit(&[(0, "ENDTAB")]);

    // STYLE
    o.lit(&[
        (0, "TABLE"),
        (2, "STYLE"),
        (5, "3"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "2"),
        (0, "STYLE"),
        (5, "11"),
        (330, "3"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbTextStyleTableRecord"),
        (2, "STANDARD"),
        (70, "0"),
        (40, "0"),
        (41, "1"),
        (50, "0"),
        (71, "0"),
        (42, "0.2"),
        (3, "txt"),
        (4, ""),
        (0, "STYLE"),
        (5, "12"),
        (330, "3"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbTextStyleTableRecord"),
    ]);
    o.s(2, &st.dxf_style);
    o.lit(&[(70, "0"), (40, "0"), (41, "1"), (50, "0"), (71, "0"), (42, "0.2")]);
    o.s(3, &st.dxf_font);
    o.lit(&[(4, ""), (0, "ENDTAB")]);

    // VIEW, UCS (empty)
    o.lit(&[
        (0, "TABLE"),
        (2, "VIEW"),
        (5, "6"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "0"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "UCS"),
        (5, "7"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "0"),
        (0, "ENDTAB"),
    ]);
    // APPID
    o.lit(&[
        (0, "TABLE"),
        (2, "APPID"),
        (5, "9"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "1"),
        (0, "APPID"),
        (5, "13"),
        (330, "9"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbRegAppTableRecord"),
        (2, "ACAD"),
        (70, "0"),
        (0, "ENDTAB"),
    ]);
    // DIMSTYLE
    o.lit(&[
        (0, "TABLE"),
        (2, "DIMSTYLE"),
        (5, "A"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "1"),
        (100, "AcDbDimStyleTable"),
        (71, "1"),
        (0, "DIMSTYLE"),
        (105, "17"),
        (330, "A"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbDimStyleTableRecord"),
        (2, "STANDARD"),
        (70, "0"),
        (340, "17"),
        (0, "ENDTAB"),
    ]);
    // BLOCK_RECORD
    o.lit(&[
        (0, "TABLE"),
        (2, "BLOCK_RECORD"),
        (5, "1"),
        (330, "0"),
        (100, "AcDbSymbolTable"),
        (70, "2"),
        (0, "BLOCK_RECORD"),
        (5, MODEL_SPACE_HANDLE),
        (330, "1"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbBlockTableRecord"),
        (2, "*Model_Space"),
        (70, "0"),
        (280, "1"),
        (281, "0"),
        (0, "BLOCK_RECORD"),
        (5, PAPER_SPACE_HANDLE),
        (330, "1"),
        (100, "AcDbSymbolTableRecord"),
        (100, "AcDbBlockTableRecord"),
        (2, "*Paper_Space"),
        (70, "0"),
        (280, "1"),
        (281, "0"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
    ]);

    // BLOCKS
    o.lit(&[
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (5, "20"),
        (330, MODEL_SPACE_HANDLE),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbBlockBegin"),
        (2, "*Model_Space"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (3, "*Model_Space"),
        (1, ""),
        (0, "ENDBLK"),
        (5, "21"),
        (330, MODEL_SPACE_HANDLE),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbBlockEnd"),
        (0, "BLOCK"),
        (5, "22"),
        (330, PAPER_SPACE_HANDLE),
        (100, "AcDbEntity"),
        (67, "1"),
        (8, "0"),
        (100, "AcDbBlockBegin"),
        (2, "*Paper_Space"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (3, "*Paper_Space"),
        (1, ""),
        (0, "ENDBLK"),
        (5, "23"),
        (330, PAPER_SPACE_HANDLE),
        (100, "AcDbEntity"),
        (67, "1"),
        (8, "0"),
        (100, "AcDbBlockEnd"),
        (0, "ENDSEC"),
    ]);

    // ENTITIES
    o.lit(&[(0, "SECTION"), (2, "ENTITIES")]);
    o.out.extend_from_slice(&entities);
    o.lit(&[(0, "ENDSEC")]);

    // OBJECTS
    o.lit(&[
        (0, "SECTION"),
        (2, "OBJECTS"),
        (0, "DICTIONARY"),
        (5, "C"),
        (330, "0"),
        (100, "AcDbDictionary"),
        (281, "1"),
        (3, "ACAD_GROUP"),
        (350, "D"),
        (0, "DICTIONARY"),
        (5, "D"),
        (330, "C"),
        (100, "AcDbDictionary"),
        (281, "1"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ]);
    o.out
}
