#!/usr/bin/env python3
"""compare.py name=results.tsv ... [--base NAME] [--markdown] : per-case medians side by side, ratio vs the base column,
and the geometric mean of the ratios per function group (check, mesh, ..., export svg/dxf/pdf/png)."""
import math
import sys

args = [a for a in sys.argv[1:] if not a.startswith('--')]
md = '--markdown' in sys.argv
base = sys.argv[sys.argv.index('--base') + 1] if '--base' in sys.argv else None
cols, data = [], {}
for a in args:
    if '=' not in a:
        continue
    name, path = a.split('=', 1)
    cols.append(name)
    for line in open(path).read().splitlines()[1:]:
        f = line.split('\t')
        data.setdefault(f[0], {})[name] = (float(f[2]), float(f[3]), f[5], f[6])
base = base or cols[0]
labels = list(data)
def group(label):
    parts = label.split(' ')
    return parts[-1]
hdr = ['case'] + [f'{c} ms' for c in cols] + [f'{c}/{base}' for c in cols if c != base]
rows = []
groups = {}
for l in labels:
    d = data[l]
    if base not in d:
        continue
    r = [l] + [f'{d[c][0] / 1000:.3f}' if c in d else '-' for c in cols]
    for c in cols:
        if c == base:
            continue
        if c in d:
            ratio = d[c][0] / d[base][0]
            r.append(f'{ratio:.2f}')
            groups.setdefault(group(l), {}).setdefault(c, []).append(ratio)
            if d[c][2] != d[base][2]:
                r[-1] += ' (bytes differ!)'
        else:
            r.append('-')
    rows.append(r)
def emit(h, rs):
    if md:
        print('| ' + ' | '.join(h) + ' |')
        print('|' + '|'.join(['---'] + ['---:'] * (len(h) - 1)) + '|')
        for r in rs:
            print('| ' + ' | '.join(r) + ' |')
    else:
        w = [max(len(str(x[i])) for x in [h] + rs) for i in range(len(h))]
        for r in [h] + rs:
            print('  '.join(str(x).ljust(w[i]) if i == 0 else str(x).rjust(w[i]) for i, x in enumerate(r)))
emit(hdr, rows)
print()
others = [c for c in cols if c != base]
gh = ['group (geomean of ratio)'] + [f'{c}/{base}' for c in others]
allr = {c: [] for c in others}
grows = []
for g, d in groups.items():
    grows.append([g] + [f'{math.exp(sum(map(math.log, d[c])) / len(d[c])):.2f}' if c in d else '-' for c in others])
    for c in others:
        allr[c] += d.get(c, [])
grows.append(['ALL'] + [f'{math.exp(sum(map(math.log, allr[c])) / len(allr[c])):.2f}' if allr[c] else '-' for c in others])
emit(gh, grows)
