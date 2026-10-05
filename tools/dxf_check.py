#!/usr/bin/env python3
"""Validate/inspect a DXF. Usage: dxf_check.py <file.dxf> [--png out.png] [--json] [--png-size PX]
Exit 1 if file unreadable or doc.audit() reports errors (or unrecoverable fixes need attention)."""
import sys, os, json, argparse, collections

# Re-exec under tools/.venv if not already in it (so `./dxf_check.py` works without activating).
_VENV = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".venv")
if os.path.isdir(_VENV) and os.path.realpath(sys.prefix) != os.path.realpath(_VENV):
    os.execv(os.path.join(_VENV, "bin", "python"), [os.path.join(_VENV, "bin", "python")] + sys.argv)
try:
    import ezdxf
    from ezdxf import recover
except ImportError:
    sys.exit("ezdxf missing: run tools/setup.sh")

INSUNITS = {0: "unitless", 1: "inches", 2: "feet", 3: "miles", 4: "mm", 5: "cm", 6: "m", 7: "km",
            8: "microinches", 9: "mils", 10: "yards", 14: "dm"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file")
    ap.add_argument("--png")
    ap.add_argument("--png-size", type=int, default=2000, help="long side px (default 2000)")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()

    rep = {"file": a.file, "ok": True, "errors": [], "fixes": []}
    try:
        doc = ezdxf.readfile(a.file)
    except Exception as e:  # try recover to give a better diagnosis, but still fail
        rep.update(ok=False, errors=[f"unreadable: {type(e).__name__}: {e}"])
        try:
            doc, aud = recover.readfile(a.file)
            rep["errors"].append(f"recover() could load it with {len(aud.errors)} errors, {len(aud.fixes)} fixes")
        except Exception as e2:
            rep["errors"].append(f"recover failed: {e2}")
        return emit(rep, a, 1)

    aud = doc.audit()
    rep["errors"] = [f"{e.code}: {e.message}" for e in aud.errors]
    rep["fixes"] = [f"{f.code}: {f.message}" for f in aud.fixes]
    if aud.errors:
        rep["ok"] = False

    hv = doc.header
    units = hv.get("$INSUNITS", 0)
    rep["dxf_version"] = doc.dxfversion
    rep["acad_release"] = doc.acad_release
    rep["insunits"] = {"code": units, "name": INSUNITS.get(units, "?")}
    rep["measurement"] = {0: "imperial", 1: "metric"}.get(hv.get("$MEASUREMENT"), None)

    msp = doc.modelspace()
    by_layer = collections.defaultdict(collections.Counter)
    by_type = collections.Counter()
    hatch_patterns = collections.Counter()
    linetypes_used = collections.Counter()
    text_styles_used = collections.Counter()
    for e in msp:
        t = e.dxftype()
        by_layer[e.dxf.layer][t] += 1
        by_type[t] += 1
        linetypes_used[e.dxf.get("linetype", "BYLAYER")] += 1
        if t == "HATCH":
            hatch_patterns[("SOLID" if e.dxf.solid_fill else e.dxf.pattern_name)] += 1
        if t in ("TEXT", "MTEXT"):
            text_styles_used[e.dxf.get("style", "Standard")] += 1
    rep["entities_total"] = sum(by_type.values())
    rep["entities_by_type"] = dict(by_type)
    rep["layers_used"] = {k: dict(v) for k, v in sorted(by_layer.items())}
    rep["layers_defined"] = {l.dxf.name: {"color": l.dxf.color, "linetype": l.dxf.linetype} for l in doc.layers}
    rep["linetypes_defined"] = sorted(l.dxf.name for l in doc.linetypes)
    rep["linetypes_used"] = dict(linetypes_used)
    rep["text_styles_defined"] = {s.dxf.name: s.dxf.font for s in doc.styles}
    rep["text_styles_used"] = dict(text_styles_used)
    rep["dimstyles"] = sorted(d.dxf.name for d in doc.dimstyles)
    rep["hatch_patterns"] = dict(hatch_patterns)
    rep["blocks"] = sorted(b.name for b in doc.blocks if not b.name.startswith("*"))
    rep["paperspace_layouts"] = [l for l in doc.layout_names() if l != "Model"]

    try:
        from ezdxf import bbox
        ext = bbox.extents(msp, fast=False)
        if ext.has_data:
            rep["extents"] = {"min": [round(v, 4) for v in ext.extmin], "max": [round(v, 4) for v in ext.extmax],
                              "size": [round(v, 4) for v in ext.size]}
        else:
            rep["extents"] = None
    except Exception as e:
        rep["extents"] = f"error: {e}"
    # sanity warnings
    warn = []
    if units == 0:
        warn.append("$INSUNITS is 0 (unitless); set 1 (inches) or 4 (mm) explicitly")
    if rep["entities_total"] == 0:
        warn.append("modelspace is empty")
    for lyr in rep["layers_used"]:
        if lyr not in rep["layers_defined"]:
            warn.append(f"layer '{lyr}' used but not in layer table")
    rep["warnings"] = warn

    if a.png:
        try:
            render_png(doc, a.png, a.png_size)
            rep["png"] = a.png
        except Exception as e:
            rep["ok"] = False
            rep["errors"].append(f"png render failed: {type(e).__name__}: {e}")
    return emit(rep, a, 0 if rep["ok"] else 1)


def render_png(doc, out, px):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from ezdxf.addons.drawing import Frontend, RenderContext
    from ezdxf.addons.drawing.matplotlib import MatplotlibBackend
    from ezdxf.addons.drawing.config import Configuration, BackgroundPolicy, ColorPolicy
    fig = plt.figure(figsize=(px / 200, px / 200), dpi=200)
    ax = fig.add_axes([0, 0, 1, 1])
    ctx = RenderContext(doc)
    cfg = Configuration(background_policy=BackgroundPolicy.WHITE, color_policy=ColorPolicy.BLACK)
    Frontend(ctx, MatplotlibBackend(ax), config=cfg).draw_layout(doc.modelspace(), finalize=True)
    fig.savefig(out, dpi=200, facecolor="white")
    plt.close(fig)


def emit(rep, a, code):
    if a.json:
        print(json.dumps(rep, indent=2, default=str))
    else:
        print(f"{'OK' if rep['ok'] else 'FAIL'}  {rep['file']}")
        if "dxf_version" in rep:
            print(f"version: {rep['dxf_version']} ({rep['acad_release']})  INSUNITS: {rep['insunits']['code']} ({rep['insunits']['name']})  measurement: {rep['measurement']}")
            print(f"entities: {rep['entities_total']}  by type: {rep['entities_by_type']}")
            print("layers used:")
            for l, c in rep["layers_used"].items():
                print(f"  {l:24s} {c}")
            print(f"layers defined: {list(rep['layers_defined'])}")
            print(f"linetypes used: {rep['linetypes_used']}  (defined: {len(rep['linetypes_defined'])}, see --json)")
            fonts = {k: rep['text_styles_defined'].get(k) for k in rep['text_styles_used']}
            print(f"text styles used: {rep['text_styles_used']} fonts: {fonts}  (defined: {len(rep['text_styles_defined'])})")
            print(f"dimstyles defined: {len(rep['dimstyles'])}  blocks: {rep['blocks']}  paperspace: {rep['paperspace_layouts']}")
            print(f"hatch patterns: {rep['hatch_patterns']}")
            print(f"extents: {rep['extents']}")
            for w in rep["warnings"]:
                print(f"WARN: {w}")
        for f in rep["fixes"]:
            print(f"audit fix: {f}")
        for e in rep["errors"]:
            print(f"ERROR: {e}")
        if rep.get("png"):
            print(f"png: {rep['png']}")
    return code


if __name__ == "__main__":
    sys.exit(main())
