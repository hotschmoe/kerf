#!/usr/bin/env python3
"""Validate/inspect a PDF. Usage: pdf_check.py <file.pdf> [--png out.png] [--page N] [--dpi 150] [--json]
Reports page count/sizes (inches), vector-vs-raster (path ops vs images, recursing into form XObjects), fonts, text sample.
Renders with pypdfium2 (pdftoppm/mutool not installed). Exit 1 if unreadable/zero pages/render fails."""
import sys, os, json, argparse, collections

_VENV = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".venv")
if os.path.isdir(_VENV) and os.path.realpath(sys.prefix) != os.path.realpath(_VENV):
    os.execv(os.path.join(_VENV, "bin", "python"), [os.path.join(_VENV, "bin", "python")] + sys.argv)
try:
    import pypdf
    import pypdfium2 as pdfium
except ImportError:
    sys.exit("pypdf/pypdfium2 missing: run tools/setup.sh")

PATH_CONSTRUCT = {b"m", b"l", b"c", b"v", b"y", b"h", b"re"}
PATH_PAINT = {b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*"}
TEXT_OPS = {b"Tj", b"TJ", b"'", b'"'}


def scan(page, resources, ops, depth=0):
    """Count operators of a page / form xobject content, recursing into forms; collect fonts + images."""
    from pypdf.generic import ContentStream
    try:
        src = page.get_contents() if hasattr(page, "get_contents") else page
        if src is None:
            return
        cs = ContentStream(src, getattr(page, "pdf", None))
    except Exception:
        return
    for operands, op in cs.operations:
        if op in PATH_CONSTRUCT:
            ops["path_construct"] += 1
        elif op in PATH_PAINT:
            ops["path_paint"] += 1
        elif op in TEXT_OPS:
            ops["text_show"] += 1
        elif op == b"Do" and depth < 6:
            xo = (resources or {}).get("/XObject")
            if xo is None:
                continue
            obj = xo.get_object().get(operands[0])
            if obj is None:
                continue
            obj = obj.get_object()
            st = obj.get("/Subtype")
            if st == "/Image":
                ops["images"] += 1
                ops["image_pixels"] += int(obj.get("/Width", 0)) * int(obj.get("/Height", 0))
            elif st == "/Form":
                ops["forms"] += 1
                scan(obj, obj.get("/Resources"), ops, depth + 1)
        elif op == b"BI":
            ops["images"] += 1  # inline image
        elif op == b"sh":
            ops["shadings"] += 1


def collect_fonts(resources, out, depth=0):
    if not resources:
        return
    resources = resources.get_object()
    fonts = resources.get("/Font")
    if fonts:
        for k, f in fonts.get_object().items():
            f = f.get_object()
            base = str(f.get("/BaseFont", k))
            desc = f.get("/FontDescriptor")
            embedded = str(f.get("/Subtype")) in ("/Type3",)  # Type3 glyphs live in the file
            if desc is not None:
                d = desc.get_object()
                embedded = any(x in d for x in ("/FontFile", "/FontFile2", "/FontFile3"))
            elif "/DescendantFonts" in f:
                d0 = f["/DescendantFonts"][0].get_object().get("/FontDescriptor")
                if d0 is not None:
                    d0 = d0.get_object(); embedded = any(x in d0 for x in ("/FontFile", "/FontFile2", "/FontFile3"))
            out[base] = {"subtype": str(f.get("/Subtype")), "embedded": embedded}
    xo = resources.get("/XObject")
    if xo and depth < 6:
        for o in xo.get_object().values():
            o = o.get_object()
            if o.get("/Subtype") == "/Form":
                collect_fonts(o.get("/Resources"), out, depth + 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file"); ap.add_argument("--png"); ap.add_argument("--page", type=int, default=1)
    ap.add_argument("--dpi", type=int, default=150); ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    rep = {"file": a.file, "ok": True, "errors": [], "warnings": []}
    try:
        with open(a.file, "rb") as fh:
            head = fh.read(8)
        if not head.startswith(b"%PDF-"):
            raise ValueError("missing %PDF- header")
        rd = pypdf.PdfReader(a.file, strict=False)
        n = len(rd.pages)
    except Exception as e:
        rep.update(ok=False, errors=[f"unreadable: {type(e).__name__}: {e}"])
        return emit(rep, a, 1)
    rep["pdf_version"] = head.decode("latin1").strip()
    rep["pages"] = n
    rep["bytes"] = os.path.getsize(a.file)
    md = rd.metadata or {}
    rep["producer"] = str(md.get("/Producer", "")) or None
    rep["title"] = str(md.get("/Title", "")) or None
    if n == 0:
        rep["ok"] = False; rep["errors"].append("zero pages")
        return emit(rep, a, 1)
    pages = []
    for i, pg in enumerate(rd.pages):
        w, h = float(pg.mediabox.width), float(pg.mediabox.height)
        rot = int(pg.get("/Rotate", 0) or 0)
        ops = collections.Counter()
        fonts = {}
        try:
            scan(pg, pg.get("/Resources"), ops)
            collect_fonts(pg.get("/Resources"), fonts)
        except Exception as e:
            rep["warnings"].append(f"page {i+1}: content scan failed: {type(e).__name__}: {e}")
        text = ""
        try:
            text = (pg.extract_text() or "").strip()
        except Exception:
            pass
        vector = ops["path_paint"] > 0 and ops["image_pixels"] < 500_000
        pages.append({"page": i + 1, "size_in": [round(w / 72, 3), round(h / 72, 3)], "size_pt": [round(w, 2), round(h, 2)],
                      "rotate": rot, "path_construct_ops": ops["path_construct"], "path_paint_ops": ops["path_paint"],
                      "text_show_ops": ops["text_show"], "images": ops["images"], "image_pixels": ops["image_pixels"],
                      "form_xobjects": ops["forms"], "shadings": ops["shadings"], "vector": vector,
                      "fonts": fonts, "text_sample": text[:200]})
        if ops["images"] and ops["image_pixels"] >= 500_000:
            rep["warnings"].append(f"page {i+1}: large raster image(s) ({ops['image_pixels']} px) - output may not be vector")
        if ops["path_paint"] == 0 and ops["images"] == 0 and not text:
            rep["warnings"].append(f"page {i+1}: appears blank (no paths, images, or text)")
    rep["page_info"] = pages if n <= 10 else pages[:10]
    if a.png:
        try:
            doc = pdfium.PdfDocument(a.file)
            pg = doc[min(max(a.page, 1), n) - 1]
            img = pg.render(scale=a.dpi / 72, fill_color=(255, 255, 255, 255)).to_pil()
            img.save(a.png)
            rep["png"] = a.png; rep["png_px"] = list(img.size)
            # blank check
            if img.convert("L").getextrema()[0] > 250:
                rep["warnings"].append("rendered page is entirely white")
        except Exception as e:
            rep["ok"] = False; rep["errors"].append(f"render failed: {type(e).__name__}: {e}")
    return emit(rep, a, 0 if rep["ok"] else 1)


def emit(rep, a, code):
    if a.json:
        print(json.dumps(rep, indent=2, default=str))
    else:
        print(f"{'OK' if rep['ok'] else 'FAIL'}  {rep['file']}")
        if "pages" in rep:
            print(f"{rep['pdf_version']}  pages: {rep['pages']}  bytes: {rep['bytes']}  producer: {rep['producer']}")
            for p in rep["page_info"]:
                print(f"p{p['page']}: {p['size_in'][0]}x{p['size_in'][1]} in  vector={p['vector']}  path_paint={p['path_paint_ops']} "
                      f"path_construct={p['path_construct_ops']} text_ops={p['text_show_ops']} images={p['images']}({p['image_pixels']}px) forms={p['form_xobjects']}")
                for f, i in p["fonts"].items():
                    print(f"    font {f}: {i['subtype']} embedded={i['embedded']}")
                if p["text_sample"]:
                    print(f"    text: {p['text_sample'][:120]!r}")
        for w in rep["warnings"]:
            print(f"WARN: {w}")
        for e in rep["errors"]:
            print(f"ERROR: {e}")
        if rep.get("png"):
            print(f"png: {rep['png']} {rep['png_px']}")
    return code


if __name__ == "__main__":
    sys.exit(main())
