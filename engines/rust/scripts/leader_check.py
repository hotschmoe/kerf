#!/usr/bin/env python3
"""Report crossing leaders in a Drawing IR JSON (anno-pen paths of note ids)."""
import json, sys
d = json.load(open(sys.argv[1]))
L = {}
for it in d['items']:
    if it['t'] == 'path' and it['pen'] == 'anno' and len(it['pts']) == 3:
        L[it['src']] = [(p[0], p[1]) for p in it['pts']]
def seg_int(a, b, c, e):
    def cr(o, p, q): return (p[0]-o[0])*(q[1]-o[1]) - (p[1]-o[1])*(q[0]-o[0])
    d1, d2, d3, d4 = cr(a, b, c), cr(a, b, e), cr(c, e, a), cr(c, e, b)
    return d1*d2 < 0 and d3*d4 < 0
ids = list(L)
n = 0
for i in range(len(ids)):
    for j in range(i+1, len(ids)):
        A, B = L[ids[i]], L[ids[j]]
        if any(seg_int(A[s], A[s+1], B[t], B[t+1]) for s in range(2) for t in range(2)):
            print('CROSS', ids[i], ids[j]); n += 1
print(n, 'crossings of', len(ids), 'leaders')
