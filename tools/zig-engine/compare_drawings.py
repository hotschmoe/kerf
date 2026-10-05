#!/usr/bin/env python3
"""Compare two Kerf Drawing IR JSON files (e.g. zig vs rust) with a geometric tolerance.

Usage: compare_drawings.py a.json b.json [--tol 1e-3] [--verbose]

Items are matched by (src, t, layer) and then pairwise in document order. For paths we compare
vertex counts and the max coordinate deviation; for fills/hatch the loop geometry (hatch line sets
are compared by count only); for text the string, anchor, height and rotation. Exit 1 on any
difference above tolerance.
"""
import json, sys, argparse, collections, math

def pts_dev(p, q):
    if len(p) != len(q): return None
    return max((max(abs(a[0]-b[0]), abs(a[1]-b[1])) for a, b in zip(p, q)), default=0.0)

def loops_dev(a, b):
    if len(a) != len(b): return None
    worst = 0.0
    for p, q in zip(a, b):
        d = pts_dev(p, q)
        if d is None: return None
        worst = max(worst, d)
    return worst

def canon_closed(p):
    """Rotate a closed vertex list to start at its lexicographically smallest point (order-insensitive start)."""
    if not p: return p
    i = min(range(len(p)), key=lambda k: (round(p[k][0], 4), round(p[k][1], 4)))
    return p[i:] + p[:i]

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('a'); ap.add_argument('b')
    ap.add_argument('--tol', type=float, default=1e-3)
    ap.add_argument('--verbose', '-v', action='store_true')
    args = ap.parse_args()
    A = json.load(open(args.a)); B = json.load(open(args.b))
    problems = []
    for k in ('doc', 'view', 'kind'):
        if A.get(k) != B.get(k): problems.append(f'{k}: {A.get(k)!r} vs {B.get(k)!r}')
    if abs(A['scale'] - B['scale']) > args.tol: problems.append(f"scale: {A['scale']} vs {B['scale']}")
    bd = max(abs(x-y) for x, y in zip(A['bounds'], B['bounds']))
    if bd > args.tol: problems.append(f"bounds differ by {bd:.4f}: {A['bounds']} vs {B['bounds']}")
    ga = collections.defaultdict(list); gb = collections.defaultdict(list)
    for it in A['items']: ga[(it['src'], it['t'], it['layer'])].append(it)
    for it in B['items']: gb[(it['src'], it['t'], it['layer'])].append(it)
    only_a = sorted(set(ga) - set(gb)); only_b = sorted(set(gb) - set(ga))
    for k in only_a: problems.append(f'only in A: {k} x{len(ga[k])}')
    for k in only_b: problems.append(f'only in B: {k} x{len(gb[k])}')
    nmatch = 0
    for k in sorted(set(ga) & set(gb)):
        la, lb = ga[k], gb[k]
        if len(la) != len(lb):
            problems.append(f'count {k}: {len(la)} vs {len(lb)}')
        for x, y in zip(la, lb):
            nmatch += 1
            t = k[1]
            if t == 'path':
                pa, pb = x['pts'], y['pts']
                if x['closed'] != y['closed']: problems.append(f'closed flag {k}'); continue
                if x['closed']: pa, pb = canon_closed(pa), canon_closed(pb)
                d = pts_dev(pa, pb)
                if d is None: problems.append(f'path {k}: {len(pa)} vs {len(pb)} vertices')
                elif d > args.tol: problems.append(f'path {k}: deviation {d:.4f}')
                if x['pen'] != y['pen']: problems.append(f"pen {k}: {x['pen']} vs {y['pen']}")
            elif t == 'fill':
                d = loops_dev([canon_closed(l) for l in x['loops']], [canon_closed(l) for l in y['loops']])
                if d is None: problems.append(f'fill {k}: loop structure differs')
                elif d > args.tol: problems.append(f'fill {k}: deviation {d:.4f}')
            elif t == 'hatch':
                if x['pattern'] != y['pattern']: problems.append(f"hatch pattern {k}: {x['pattern']} vs {y['pattern']}")
                if len(x['lines']) != len(y['lines']): problems.append(f"hatch lines {k}: {len(x['lines'])} vs {len(y['lines'])}")
            elif t == 'text':
                if x['s'] != y['s']: problems.append(f"text {k}: {x['s']!r} vs {y['s']!r}")
                d = max(abs(x[c]-y[c]) for c in ('x', 'y', 'h', 'rot'))
                if d > args.tol: problems.append(f"text {k} {x['s'][:20]!r}: deviation {d:.4f}")
    print(f'{args.a} vs {args.b}: {len(A["items"])} vs {len(B["items"])} items, {nmatch} matched, {len(problems)} differences (tol {args.tol})')
    shown = problems if args.verbose else problems[:25]
    for p in shown: print('  ', p)
    if len(problems) > len(shown): print(f'   ... {len(problems)-len(shown)} more (use -v)')
    sys.exit(1 if problems else 0)

if __name__ == '__main__':
    main()
