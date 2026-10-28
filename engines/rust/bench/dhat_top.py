#!/usr/bin/env python3
"""dhat_top.py <dhat-out.json> [N] [depth] : allocation sites by block count (valgrind --tool=dhat), keyed by the
first `depth` engine frames (kerf-core/src file:line). Build with RUSTFLAGS="-C dwarf-version=4" and full debug info."""
import json
import re
import sys

d = json.load(open(sys.argv[1]))
n = int(sys.argv[2]) if len(sys.argv) > 2 else 25
depth = int(sys.argv[3]) if len(sys.argv) > 3 else 2
ftbl = d['ftbl']
tot_b = sum(p['tb'] for p in d['pps'])
tot_k = sum(p['tbk'] for p in d['pps'])
agg = {}
for p in d['pps']:
    names = []
    for fi in p['fs']:
        m = re.search(r': (.*) \((.*?):(\d+)\)$', ftbl[fi])
        if not m or '/library/' in m.group(2) or not m.group(2).endswith('.rs') or m.group(2).startswith('library'):
            continue
        if 'bench.rs' in m.group(2):
            continue
        fn = m.group(1) if m.group(1) != 'UnknownInlinedFun' else ''
        names.append(f'{m.group(2)}:{m.group(3)} {fn}'.strip())
        if len(names) >= depth:
            break
    key = ' <- '.join(names) or '(outside the engine)'
    a = agg.setdefault(key, [0, 0])
    a[0] += p['tbk']
    a[1] += p['tb']
print(f'total {tot_k} blocks, {tot_b} bytes')
for k, (blocks, b) in sorted(agg.items(), key=lambda x: -x[1][0])[:n]:
    print(f'{100 * blocks / tot_k:5.1f}% {blocks:7d} blk {b:9d} B  {k}')
