#!/usr/bin/env python3
"""dhat_top.py <dhat-out.json> [N] [depth] : allocation sites by block count (valgrind --tool=dhat), with the first
`depth` frames that are not allocator/collection plumbing."""
import json
import re
import sys

d = json.load(open(sys.argv[1]))
n = int(sys.argv[2]) if len(sys.argv) > 2 else 25
depth = int(sys.argv[3]) if len(sys.argv) > 3 else 3
ftbl = d['ftbl']
skip = re.compile(r'alloc::|core::|RawVec|<alloc|__rust|__rdl|malloc|realloc|calloc|std::|dhat|vg_replace|clone|to_owned|to_vec|'
                  r'to_string|from_iter|spec_|extend|collect|push|grow|reserve|with_capacity|Vec<|String|format')
tot_b = sum(p['tb'] for p in d['pps'])
tot_k = sum(p['tbk'] for p in d['pps'])
agg = {}
for p in d['pps']:
    names = []
    for fi in p['fs']:
        fr = ftbl[fi]
        m = re.search(r': (.*?) \(', fr)
        fn = re.sub(r'::h[0-9a-f]{16}', '', m.group(1) if m else fr)
        if not skip.search(fn):
            names.append(fn)
        if len(names) >= depth:
            break
    key = ' <- '.join(names)
    a = agg.setdefault(key, [0, 0])
    a[0] += p['tbk']
    a[1] += p['tb']
print(f'total {tot_k} blocks, {tot_b} bytes')
for k, (blocks, b) in sorted(agg.items(), key=lambda x: -x[1][0])[:n]:
    print(f'{100 * blocks / tot_k:5.1f}% {blocks:8d} blk {b:10d} B  {k}')
