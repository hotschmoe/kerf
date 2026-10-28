#!/usr/bin/env python3
"""selfprof.py <perf script output> <binary> : flat self-time per symbol (nm symbols, no inlining), for binaries
without frame pointers such as the Zig fast build."""
import bisect
import collections
import re
import subprocess
import sys

syms = []
for l in subprocess.run(['nm', '-n', '-C', sys.argv[2]], capture_output=True, text=True).stdout.splitlines():
    f = l.split(None, 2)
    if len(f) == 3 and f[1] in 'TtWw' and re.fullmatch('[0-9a-f]+', f[0]):
        syms.append((int(f[0], 16), re.sub(r'::h[0-9a-f]{16}$', '', f[2])))
addrs = [a for a, _ in syms]
c = collections.Counter()
tot = 0
for line in open(sys.argv[1], errors='replace'):
    m = re.match(r'\s*([0-9a-f]+)\s+.*\((.*)\)\s*$', line)
    if not m:
        continue
    a, dso = int(m.group(1), 16), m.group(2)
    tot += 1
    if dso.endswith(sys.argv[2].split('/')[-1]):
        i = bisect.bisect_right(addrs, a) - 1
        c[syms[i][1] if i >= 0 else '?'] += 1
    else:
        c['[' + dso.split('/')[-1] + ']'] += 1
for k, v in c.most_common(int(sys.argv[3]) if len(sys.argv) > 3 else 40):
    print(f'{100 * v / tot:6.2f}%  {k}')
