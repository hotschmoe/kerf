#!/usr/bin/env python3
"""bench_cli.py <kerf binary> [N=30] : wall time of whole CLI processes (start, read, compute, write, exit), median and
min over N runs per command, as a user or an agent sees `kerf`. Output: TSV like bench.rs (label, fn, median_us, min_us, ...)."""
import os
import statistics
import sys
import tempfile
import time

K = sys.argv[1]
N = int(sys.argv[2]) if len(sys.argv) > 2 else 30
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..')
docs = {'truss': 'spec/details/truss-bearing-cmu.kerf.json', 'palmer': 'engines/zig/tests/docs/palmer-sd1-like.kerf.json'}
out = os.path.join(tempfile.mkdtemp(), 'o')
cases = [('version', ['version'])]
for name, d in docs.items():
    p = os.path.join(ROOT, d)
    cases += [(f'{name} check', ['check', p]), (f'{name} mesh', ['mesh', p, '-o', out]),
              (f'{name} A drawing', ['drawing', p, '--view', 'A', '-o', out])]
    for fmt in ['svg', 'dxf', 'pdf', 'png']:
        cases.append((f'{name} A {fmt}', ['export', p, '--view', 'A', '--format', fmt, '-o', out]))
    cases.append((f'{name} B svg', ['export', p, '--view', 'B', '--format', 'svg', '-o', out]))
devnull = [(os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0), (os.POSIX_SPAWN_OPEN, 2, os.devnull, os.O_WRONLY, 0)]
print('label\tfn\tmedian_us\tmin_us\treps\tbytes\tstatus')
for label, args in cases:
    ts = []
    for k in range(N + 2):
        t = time.perf_counter()
        pid = os.posix_spawn(K, [K] + args, os.environ, file_actions=devnull)
        _, status = os.waitpid(pid, 0)
        dt = (time.perf_counter() - t) * 1e6
        rc = os.waitstatus_to_exitcode(status)
        if k >= 2:
            ts.append(dt)
    size = os.path.getsize(out) if '-o' in args and os.path.exists(out) else 0
    print(f'{label}\t{args[0]}\t{statistics.median(ts):.1f}\t{min(ts):.1f}\t{N}\t{size}\t{"ok" if rc in (0, 1) else "err"}')
