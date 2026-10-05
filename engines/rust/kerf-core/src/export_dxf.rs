//! DXF exporter (SPEC section 12): AutoCAD R2000 (AC1015) ASCII, model space at 1:1 model inches.

use crate::drawing::{Drawing, Item, used_layers};
use crate::geom::*;
use crate::num::fmt_num;
use crate::paper::{Geo, paper_for};
use crate::style::Style;

struct W {
    out: String,
    next: u32,
}

impl W {
    fn g(&mut self, code: i32, val: &str) {
        self.out.push_str(&format!("{}\n{}\n", code, val));
    }
    fn gi(&mut self, code: i32, v: i64) {
        self.g(code, &v.to_string());
    }
    fn gf(&mut self, code: i32, v: f64) {
        self.g(code, &flt(v));
    }
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next += 1;
        h
    }
}

fn flt(v: f64) -> String {
    let s = fmt_num(v);
    if s.contains('.') { s } else { format!("{}.0", s) }
}

const STD_LW: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

fn lineweight(mm: f64) -> i32 {
    let target = mm * 100.0;
    let mut best = STD_LW[1];
    for &w in &STD_LW[1..] {
        if (w as f64 - target).abs() < (best as f64 - target).abs() {
            best = w;
        }
    }
    best
}

struct Ltype {
    name: String,
    dashes: Vec<f64>, // model inches, signed
}

fn ltypes(style: &Style, s: f64) -> Vec<Ltype> {
    let mut out: Vec<Ltype> = vec![];
    for pen in &style.pens {
        if let Some(d) = &pen.dash_mm {
            if d.len() >= 2 {
                let name = if pen.name == "hidden" { "DASHED".to_string() } else { format!("DASH_{}", pen.name.to_uppercase()) };
                let mut v = vec![];
                for (i, x) in d.iter().enumerate() {
                    let len = x / 25.4 * s;
                    v.push(if i % 2 == 0 { len } else { -len });
                }
                out.push(Ltype { name, dashes: v });
            }
        }
    }
    out
}

fn ltype_of_pen(style: &Style, pen: &str) -> Option<String> {
    let p = style.pens.iter().find(|p| p.name == pen)?;
    p.dash_mm.as_ref().filter(|d| d.len() >= 2).map(|_| if pen == "hidden" { "DASHED".to_string() } else { format!("DASH_{}", pen.to_uppercase()) })
}

fn write_header(w: &mut W, d: &Drawing) {
    w.g(0, "SECTION");
    w.g(2, "HEADER");
    w.g(9, "$ACADVER");
    w.g(1, "AC1015");
    w.g(9, "$DWGCODEPAGE");
    w.g(3, "ANSI_1252");
    w.g(9, "$INSBASE");
    w.gf(10, 0.0);
    w.gf(20, 0.0);
    w.gf(30, 0.0);
    w.g(9, "$EXTMIN");
    w.gf(10, d.bounds.x0);
    w.gf(20, d.bounds.y0);
    w.gf(30, 0.0);
    w.g(9, "$EXTMAX");
    w.gf(10, d.bounds.x1);
    w.gf(20, d.bounds.y1);
    w.gf(30, 0.0);
    w.g(9, "$LTSCALE");
    w.gf(40, 1.0);
    w.g(9, "$TEXTSTYLE");
    w.g(7, "KERF");
    w.g(9, "$CLAYER");
    w.g(8, "0");
    w.g(9, "$CELTYPE");
    w.g(6, "BYLAYER");
    w.g(9, "$LWDISPLAY");
    w.gi(290, 1);
    w.g(9, "$MEASUREMENT");
    w.gi(70, 0);
    w.g(9, "$INSUNITS");
    w.gi(70, 1);
    w.g(9, "$TDCREATE");
    w.gf(40, 2451545.0);
    w.g(9, "$TDUPDATE");
    w.gf(40, 2451545.0);
    w.g(9, "$HANDSEED");
    w.g(5, "FFFF");
    w.g(0, "ENDSEC");
}

fn sym_table(w: &mut W, name: &str, handle: &str, count: usize) {
    w.g(0, "TABLE");
    w.g(2, name);
    w.g(5, handle);
    w.g(330, "0");
    w.g(100, "AcDbSymbolTable");
    w.gi(70, count as i64);
}

fn rec_head(w: &mut W, kind: &str, table: &str, sub: &str) {
    let h = w.handle();
    w.g(0, kind);
    w.g(5, &h);
    w.g(330, table);
    w.g(100, "AcDbSymbolTableRecord");
    w.g(100, sub);
}

pub fn export_dxf_drawing(d: &Drawing, style: &Style, with_sheet: bool) -> String {
    let mut w = W { out: String::new(), next: 0x20 };
    write_header(&mut w, d);
    let s = d.scale;
    let lts = ltypes(style, s);
    let layers = used_layers(d, style);
    let mut layer_list = layers.clone();
    if with_sheet && !layer_list.iter().any(|l| l.key == "title") {
        layer_list.push(style.layer("title"));
    }

    // TABLES
    w.g(0, "SECTION");
    w.g(2, "TABLES");
    // VPORT
    sym_table(&mut w, "VPORT", "8", 0);
    w.g(0, "ENDTAB");
    // LTYPE
    sym_table(&mut w, "LTYPE", "4", 3 + lts.len());
    for (name, desc) in [("BYBLOCK", ""), ("BYLAYER", ""), ("CONTINUOUS", "Solid line")] {
        rec_head(&mut w, "LTYPE", "4", "AcDbLinetypeTableRecord");
        w.g(2, name);
        w.gi(70, 0);
        w.g(3, desc);
        w.gi(72, 65);
        w.gi(73, 0);
        w.gf(40, 0.0);
    }
    for lt in &lts {
        rec_head(&mut w, "LTYPE", "4", "AcDbLinetypeTableRecord");
        w.g(2, &lt.name);
        w.gi(70, 0);
        w.g(3, "Kerf dashed");
        w.gi(72, 65);
        w.gi(73, lt.dashes.len() as i64);
        w.gf(40, lt.dashes.iter().map(|x| x.abs()).sum());
        for x in &lt.dashes {
            w.gf(49, *x);
            w.gi(74, 0);
        }
    }
    w.g(0, "ENDTAB");
    // LAYER
    sym_table(&mut w, "LAYER", "2", layer_list.len() + 1);
    rec_head(&mut w, "LAYER", "2", "AcDbLayerTableRecord");
    w.g(2, "0");
    w.gi(70, 0);
    w.gi(62, 7);
    w.g(6, "CONTINUOUS");
    for l in &layer_list {
        rec_head(&mut w, "LAYER", "2", "AcDbLayerTableRecord");
        w.g(2, &l.name);
        w.gi(70, 0);
        w.gi(62, 7);
        let lt = match &l.linetype {
            Some(n) if n.eq_ignore_ascii_case("dashed") && lts.iter().any(|x| x.name == "DASHED") => "DASHED".to_string(),
            _ => "CONTINUOUS".to_string(),
        };
        w.g(6, &lt);
        w.gi(370, lineweight(l.lineweight_mm) as i64);
    }
    w.g(0, "ENDTAB");
    // STYLE
    sym_table(&mut w, "STYLE", "3", 2);
    for (name, font) in [("STANDARD", "txt"), (style.dxf_style.as_str(), style.dxf_font.as_str())] {
        rec_head(&mut w, "STYLE", "3", "AcDbTextStyleTableRecord");
        w.g(2, name);
        w.gi(70, 0);
        w.gf(40, 0.0);
        w.gf(41, 1.0);
        w.gf(50, 0.0);
        w.gi(71, 0);
        w.gf(42, 1.0);
        w.g(3, font);
        w.g(4, "");
    }
    w.g(0, "ENDTAB");
    // VIEW, UCS
    sym_table(&mut w, "VIEW", "5", 0);
    w.g(0, "ENDTAB");
    sym_table(&mut w, "UCS", "6", 0);
    w.g(0, "ENDTAB");
    // APPID
    sym_table(&mut w, "APPID", "9", 1);
    rec_head(&mut w, "APPID", "9", "AcDbRegAppTableRecord");
    w.g(2, "ACAD");
    w.gi(70, 0);
    w.g(0, "ENDTAB");
    // BLOCK_RECORD
    sym_table(&mut w, "BLOCK_RECORD", "1", 2);
    for (name, h) in [("*Model_Space", "10"), ("*Paper_Space", "11")] {
        w.g(0, "BLOCK_RECORD");
        w.g(5, h);
        w.g(330, "1");
        w.g(100, "AcDbSymbolTableRecord");
        w.g(100, "AcDbBlockTableRecord");
        w.g(2, name);
    }
    w.g(0, "ENDTAB");
    w.g(0, "ENDSEC");

    // BLOCKS
    w.g(0, "SECTION");
    w.g(2, "BLOCKS");
    for (name, owner, hb, he) in [("*Model_Space", "10", "12", "13"), ("*Paper_Space", "11", "14", "15")] {
        w.g(0, "BLOCK");
        w.g(5, hb);
        w.g(330, owner);
        w.g(100, "AcDbEntity");
        w.g(8, "0");
        w.g(100, "AcDbBlockBegin");
        w.g(2, name);
        w.gi(70, 0);
        w.gf(10, 0.0);
        w.gf(20, 0.0);
        w.gf(30, 0.0);
        w.g(3, name);
        w.g(1, "");
        w.g(0, "ENDBLK");
        w.g(5, he);
        w.g(330, owner);
        w.g(100, "AcDbEntity");
        w.g(8, "0");
        w.g(100, "AcDbBlockEnd");
    }
    w.g(0, "ENDSEC");

    // ENTITIES
    w.g(0, "SECTION");
    w.g(2, "ENTITIES");
    for it in &d.items {
        entity(&mut w, it, style, s);
    }
    if with_sheet {
        let p = paper_for(d, style, true);
        let (px, py) = (p.origin.0, p.origin.1);
        let (ox, oy) = (d.bounds.x0, d.bounds.y0);
        let f = |q: Pt| pt((q.x - px) * s + ox, (q.y - py) * s + oy);
        let lname = style.layer("title").name;
        for pr in p.prims.iter().filter(|x| x.src == "sheet") {
            let lw = lineweight(style.pen(&pr.pen).width_mm.max(0.13));
            match &pr.geo {
                Geo::Path { pts, closed } => {
                    let v: Vec<V> = pts.iter().map(|q| { let r = f(q.p()); vb(r.x, r.y, q.b) }).collect();
                    lwpoly(&mut w, &lname, &v, *closed, lw, None);
                }
                Geo::Strokes(polys) => {
                    for pl in polys {
                        if pl.len() >= 2 {
                            let v: Vec<V> = pl.iter().map(|q| { let r = f(*q); v_(r.x, r.y) }).collect();
                            lwpoly(&mut w, &lname, &v, false, lw, None);
                        }
                    }
                }
                Geo::Fill { loops } => {
                    let ls: Vec<Vec<V>> = loops.iter().map(|l| l.iter().map(|q| { let r = f(q.p()); vb(r.x, r.y, q.b) }).collect()).collect();
                    hatch_solid(&mut w, &lname, &ls);
                }
                Geo::Lines(_) => {}
            }
        }
    }
    w.g(0, "ENDSEC");

    // OBJECTS
    w.g(0, "SECTION");
    w.g(2, "OBJECTS");
    w.g(0, "DICTIONARY");
    w.g(5, "A");
    w.g(330, "0");
    w.g(100, "AcDbDictionary");
    w.gi(281, 1);
    w.g(3, "ACAD_GROUP");
    w.g(350, "B");
    w.g(0, "DICTIONARY");
    w.g(5, "B");
    w.g(330, "A");
    w.g(100, "AcDbDictionary");
    w.gi(281, 1);
    w.g(0, "ENDSEC");
    w.g(0, "EOF");
    w.out
}

fn v_(x: f64, y: f64) -> V {
    v(x, y)
}

fn ent_head(w: &mut W, kind: &str, layer: &str, sub: &str, lw: Option<i32>, lt: Option<&str>) {
    let h = w.handle();
    w.g(0, kind);
    w.g(5, &h);
    w.g(330, "10");
    w.g(100, "AcDbEntity");
    w.g(8, layer);
    if let Some(l) = lt {
        w.g(6, l);
    }
    if let Some(l) = lw {
        w.gi(370, l as i64);
    }
    w.g(100, sub);
}

fn lwpoly(w: &mut W, layer: &str, pts: &[V], closed: bool, lw: i32, lt: Option<&str>) {
    if pts.len() < 2 {
        return;
    }
    ent_head(w, "LWPOLYLINE", layer, "AcDbPolyline", Some(lw), lt);
    w.gi(90, pts.len() as i64);
    w.gi(70, if closed { 1 } else { 0 });
    for p in pts {
        w.gf(10, p.x);
        w.gf(20, p.y);
        if p.b.abs() > 1e-12 {
            w.g(42, &fmt_num_bulge(p.b));
        }
    }
}

fn fmt_num_bulge(b: f64) -> String {
    // bulges need more than 4 decimals to keep arcs accurate
    let s = format!("{:.8}", b);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.contains('.') { s } else { format!("{}.0", s) }
}

fn hatch_head(w: &mut W, layer: &str, pattern: &str, solid: bool, loops: &[Vec<V>]) {
    ent_head(w, "HATCH", layer, "AcDbHatch", None, None);
    w.gf(10, 0.0);
    w.gf(20, 0.0);
    w.gf(30, 0.0);
    w.gf(210, 0.0);
    w.gf(220, 0.0);
    w.gf(230, 1.0);
    w.g(2, pattern);
    w.gi(70, if solid { 1 } else { 0 });
    w.gi(71, 0);
    w.gi(91, loops.len() as i64);
    for (k, l) in loops.iter().enumerate() {
        w.gi(92, if k == 0 { 3 } else { 2 });
        let has_bulge = l.iter().any(|p| p.b.abs() > 1e-12);
        w.gi(72, if has_bulge { 1 } else { 0 });
        w.gi(73, 1);
        w.gi(93, l.len() as i64);
        for p in l {
            w.gf(10, p.x);
            w.gf(20, p.y);
            if has_bulge {
                w.g(42, &fmt_num_bulge(p.b));
            }
        }
        w.gi(97, 0);
    }
    w.gi(75, 0);
    w.gi(76, 1);
}

fn hatch_solid(w: &mut W, layer: &str, loops: &[Vec<V>]) {
    if loops.is_empty() {
        return;
    }
    hatch_head(w, layer, "SOLID", true, loops);
    w.gi(98, 0);
}

fn entity(w: &mut W, it: &Item, style: &Style, s: f64) {
    match it {
        Item::Path { layer, pen, pts, closed, .. } => {
            let lw = lineweight(style.pen(pen).width_mm);
            let lt = ltype_of_pen(style, pen);
            lwpoly(w, layer, pts, *closed, lw, lt.as_deref());
        }
        Item::Fill { layer, loops, .. } => hatch_solid(w, layer, loops),
        Item::Hatch { layer, pattern, scale, angle, loops, .. } => {
            let Some(pat) = style.pattern(pattern) else { return };
            hatch_head(w, layer, pattern, false, loops);
            w.gf(52, 0.0);
            w.gf(41, 1.0);
            w.gi(77, 0);
            w.gi(78, pat.len() as i64);
            let k = scale * s;
            let rot = angle.to_radians();
            for fam in pat {
                let a = (fam.angle + angle).to_radians();
                let base = pt(fam.x0, fam.y0).rot(rot) * k;
                let (dx, dy) = (fam.dx * k, fam.dy * k);
                let off = pt(dx * a.cos() - dy * a.sin(), dx * a.sin() + dy * a.cos());
                w.gf(53, fam.angle + angle);
                w.gf(43, base.x);
                w.gf(44, base.y);
                w.gf(45, off.x);
                w.gf(46, off.y);
                w.gi(79, fam.dashes.len() as i64);
                for d in &fam.dashes {
                    w.gf(49, d * k);
                }
            }
            w.gi(98, 0);
        }
        Item::Text { layer, s: text, x, y, h, rot, align, valign, .. } => {
            ent_head(w, "TEXT", layer, "AcDbText", None, None);
            w.gf(10, *x);
            w.gf(20, *y);
            w.gf(30, 0.0);
            w.gf(40, *h);
            w.g(1, text);
            if *rot != 0.0 {
                w.gf(50, *rot);
            }
            w.g(7, &style.dxf_style);
            let ha = match align.as_str() {
                "center" => 1,
                "right" => 2,
                _ => 0,
            };
            let va = match valign.as_str() {
                "middle" => 2,
                "top" => 3,
                "bottom" => 1,
                _ => 0,
            };
            if ha != 0 || va != 0 {
                w.gi(72, ha);
                w.gf(11, *x);
                w.gf(21, *y);
                w.gf(31, 0.0);
            }
            w.g(100, "AcDbText");
            w.gi(73, va);
        }
    }
}

pub fn export_dxf(d: &Drawing, style: &Style, with_sheet: bool) -> String {
    export_dxf_drawing(d, style, with_sheet)
}
