#!/usr/bin/env python3
"""cpuprof.py <node .cpuprofile> > folded.txt : V8 CPU profile -> folded stacks (weight = microseconds), with
Rust legacy symbol demangling, for flame.py top/svg."""
import collections
import json
import re
import sys


def demangle(s):
    m = re.match(r'^_?_ZN(.*)E$', s)
    if not m:
        return s
    body, parts = m.group(1), []
    while body:
        n = re.match(r'^(\d+)', body)
        if not n:
            break
        k = int(n.group(1))
        start = len(n.group(1))
        parts.append(body[start:start + k])
        body = body[start + k:]
    if parts and re.fullmatch(r'h[0-9a-f]{16}', parts[-1]):
        parts.pop()
    out = '::'.join(parts)
    for a, b in (('$LT$', '<'), ('$GT$', '>'), ('$u20$', ' '), ('$u7b$', '{'), ('$u7d$', '}'), ('$RF$', '&'),
                 ('$C$', ','), ('$BP$', '*'), ('..', '::'), ('$u27$', "'"), ('$u5b$', '['), ('$u5d$', ']')):
        out = out.replace(a, b)
    return out


prof = json.load(open(sys.argv[1]))
nodes = {n['id']: n for n in prof['nodes']}
parent = {}
for n in prof['nodes']:
    for c in n.get('children', []):
        parent[c] = n['id']
weight = collections.Counter()
for sid, dt in zip(prof['samples'], prof['timeDeltas']):
    weight[sid] += max(dt, 0)
folded = collections.Counter()
for sid, w in weight.items():
    stack, cur = [], sid
    while cur is not None:
        fn = nodes[cur]['callFrame']['functionName'] or '(anon)'
        stack.append(demangle(fn))
        cur = parent.get(cur)
    stack.reverse()
    stack = [s for s in stack if s not in ('(root)',)]
    folded[';'.join(stack)] += w
for k, v in folded.items():
    print(f'{k} {v}')
