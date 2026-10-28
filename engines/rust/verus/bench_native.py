#!/usr/bin/env python3
"""Usage: bench_native.py <before kerf> <after kerf> [N=10] -- per-call ms of `kerf call <fn>` (process start included)
on the reference docs, the two binaries alternated per case, best of 3 rounds of N calls."""
import glob, json, os, subprocess, sys, time
A, B = sys.argv[1], sys.argv[2]
N = int(sys.argv[3]) if len(sys.argv) > 3 else 10
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..')
tot = [0.0, 0.0]
print('doc                          fn        view   before   after  ratio')
for path in sorted(glob.glob(os.path.join(ROOT, 'spec/details/*.kerf.json'))):
    doc = json.load(open(path))
    cases = [('check', {}), ('mesh', {}), ('inspect', {'query': {'q': 'summary'}})]
    for v in ('A', 'B'):
        for fmt in ('drawing', 'svg', 'dxf', 'pdf', 'png'):
            cases.append(('drawing' if fmt == 'drawing' else 'export', {'view': v, 'format': fmt, 'sheet': False}))
    for fn, extra in cases:
        data = json.dumps({'doc': doc, **extra}).encode()
        best = [1e9, 1e9]
        for _ in range(3):
            for e, k in enumerate((A, B)):
                subprocess.run([k, 'call', fn], input=data, capture_output=True)
                t0 = time.perf_counter()
                for _ in range(N):
                    subprocess.run([k, 'call', fn], input=data, capture_output=True)
                best[e] = min(best[e], (time.perf_counter() - t0) / N * 1000)
        tot[0] += best[0]; tot[1] += best[1]
        label = extra.get('format', fn) if fn == 'export' else fn
        print(f"{os.path.basename(path).replace('.kerf.json',''):28} {label:9} {extra.get('view','-'):4} {best[0]:7.2f} {best[1]:7.2f} {best[1]/best[0]:6.3f}")
print(f'TOTAL ms over all cases: before {tot[0]:.1f}  after {tot[1]:.1f}  ratio {tot[1]/tot[0]:.3f}')
