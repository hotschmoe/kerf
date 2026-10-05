#!/usr/bin/env python3
"""Mutation fuzz: random edits of the reference docs must never crash the engine (no signal / panic).
Usage: fuzz.py [N=300] [seed=1]   (uses engines/zig/zig-out/bin/kerf, a safety-checked Debug build)"""
import json, random, subprocess, sys, glob, copy, os
N = int(sys.argv[1]) if len(sys.argv) > 1 else 300
seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
random.seed(seed)
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
K = os.path.join(ROOT, 'engines/zig/zig-out/bin/kerf')
docs = [json.load(open(f)) for f in sorted(glob.glob(os.path.join(ROOT, 'spec/details/*.kerf.json')))]
JUNK = [None, True, False, 0, -1, 1e9, -1e9, 0.0001, "", "x", "7 5/8", [], {}, [1], [1, 2, 3, 4], "nope@top_left", "@origin", {"ref": "cmu@top_left"}, "#9", [[0, 0], [1, 1]]]
def paths(o, p=()):
    yield p
    if isinstance(o, dict):
        for k, v in o.items(): yield from paths(v, p + (k,))
    elif isinstance(o, list):
        for i, v in enumerate(o): yield from paths(v, p + (i,))
def get(o, p):
    for k in p: o = o[k]
    return o
def mutate(d):
    d = copy.deepcopy(d)
    for _ in range(random.randint(1, 3)):
        ps = [p for p in paths(d) if p]
        p = random.choice(ps)
        parent = get(d, p[:-1]); k = p[-1]
        r = random.random()
        if isinstance(parent[k], (int, float)) and not isinstance(parent[k], bool) and random.random() < 0.7:
            parent[k] = parent[k] * random.choice([0, -1, 0.5, 2, 3, 10, 1.0001, 0.001]) + random.choice([0, 0, 0.25, -3, 100])
            continue
        if r < 0.25:
            if isinstance(parent, dict): del parent[k]
            else: parent.pop(k)
        elif r < 0.9: parent[k] = random.choice(JUNK)
        else:
            if isinstance(parent, dict): parent[random.choice(['type','shape','size','run','kind','at','z','array','points','place','target'])] = random.choice(JUNK)
    return d
bad = 0
for i in range(N):
    d = mutate(random.choice(docs))
    txt = json.dumps({'doc': d, 'view': random.choice(['A', 'B']), 'format': random.choice(['svg', 'dxf', 'pdf']), 'ops': []})
    for fn in random.sample(['check', 'drawing', 'export', 'mesh', 'fmt', 'inspect'], 3):
        r = subprocess.run([K, 'call', fn], input=txt.encode(), capture_output=True, timeout=60)
        if r.returncode not in (0, 1) or b'panic' in r.stderr:
            bad += 1
            open(f'/tmp/claude-1001/-home-hotschmoe-github/74528b23-e8d0-42da-a7a8-4b1990a9db54/scratchpad/zig/crash{bad}.json', 'w').write(txt)
            print('CRASH', fn, r.returncode, r.stderr.decode()[:400])
print(f'{N} mutations x 3 fns: {bad} crashes')
sys.exit(1 if bad else 0)
