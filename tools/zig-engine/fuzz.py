#!/usr/bin/env python3
"""Mutation fuzz: random edits of the reference docs must never crash the engine (no signal / panic).
Usage: fuzz.py [N=300] [seed=1]   (uses engines/zig/zig-out/bin/kerf, a safety-checked Debug build)"""
import json, random, subprocess, sys, glob, copy, os, tempfile
N = int(sys.argv[1]) if len(sys.argv) > 1 else 300
seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
OUT = os.environ.get('KERF_FUZZ_OUT', tempfile.gettempdir())  # where crashing / hanging inputs are saved
random.seed(seed)
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
K = os.path.join(ROOT, 'engines/zig/zig-out/bin/kerf')
docs = [json.load(open(f)) for f in sorted(glob.glob(os.path.join(ROOT, 'spec/details/*.kerf.json')))]
JUNK = [None, True, False, 0, -1, 1e9, -1e9, 0.0001, "", "x", "7 5/8", [], {}, [1], [1, 2, 3, 4], "nope@top_left", "@origin", {"ref": "cmu@top_left"}, "#9", [[0, 0], [1, 1]]]
# REVIEW appendix B: the values that used to panic / hang / OOM (edition 1e30, gauge 1e20, wrap_chars -5, bulge 1e-14, slope "nan", 19-digit
# lengths, scale "1e300:1", offsets 1e300, run +-1e300, crop +-1e300). Python writes inf/nan as Infinity/NaN (invalid JSON: must give E_JSON).
HOSTILE = [1e30, -1e30, 1e-14, 1e14, 1e300, -1e300, 1e15, -5, 0.0, -0.0, float('inf'), float('nan'), 2**53, 2**63, 2**64,
           "nan", "inf", "-inf", "infinity", "1e999", "-1e999", "9999999999999999999", "1e300:1", "1:1e-300", "nan:1", "1:inf", "1:1e300", "1'=1\"", "0:0", "1:0",
           [-1e300, 1e300], [1e300, 0], [0, 0, 1e-14], [0, 0, 1e14], [0, 0, "nan"], {"x": [-1e300, 1e300], "y": [-1e300, 1e300]}, "@@BADUTF8@@", "\ud800"]
STYLES = [{"notes": {"wrap_chars": -5}}, {"notes": {"wrap_chars": 0}}, {"notes": {"wrap_chars": 1e30}}, {"notes": {"wrap_chars": "wide"}}, {"text": {"height_in": 0}},
          {"text": {"height_in": 1e-300}}, {"text": {"line_spacing": 0}}, {"patterns": {"ANSI31": [[45, 0, 0, 0, 0]]}}, {"patterns": {"ANSI31": [[45, 0, 0, 0, 1e-300]]}},
          {"pens": {"hidden": {"dash_mm": [0, 0]}}}, {"sheet": {"size_in": [0, 0]}}, {"sheet": {"margin_in": 1e9}}, {"layers": 5}, {"notes": 5}]
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
            parent[k] = parent[k] * random.choice([0, -1, 0.5, 2, 3, 10, 1.0001, 0.001, 1e30, 1e-14, 1e-300]) + random.choice([0, 0, 0.25, -3, 100])
            continue
        if r < 0.25:
            if isinstance(parent, dict): del parent[k]
            else: parent.pop(k)
        elif r < 0.75: parent[k] = random.choice(JUNK)
        elif r < 0.9: parent[k] = random.choice(HOSTILE)
        else:
            if isinstance(parent, dict): parent[random.choice(['type','shape','size','run','kind','at','z','array','points','place','target'])] = random.choice(JUNK)
    return d
bad = 0
for i in range(N):
    d = mutate(random.choice(docs))
    inp = {'doc': d, 'view': random.choice(['A', 'B']), 'format': random.choice(['svg', 'dxf', 'pdf']), 'ops': []}
    if random.random() < 0.15: inp['style'] = random.choice(STYLES)
    txt = json.dumps(inp)
    data = txt.encode().replace(b'@@BADUTF8@@', b'\xff\xfe')  # raw invalid UTF-8 inside a string
    for fn in random.sample(['check', 'drawing', 'export', 'mesh', 'fmt', 'inspect'], 3):
        try:
            r = subprocess.run([K, 'call', fn], input=data, capture_output=True, timeout=30)
        except subprocess.TimeoutExpired:
            bad += 1
            open(f'{OUT}/kerf-fuzz-hang{bad}.json', 'wb').write(data)
            print('HANG', fn)
            continue
        if r.returncode not in (0, 1) or b'panic' in r.stderr:
            bad += 1
            open(f'{OUT}/kerf-fuzz-crash{bad}.json', 'wb').write(data)
            print('CRASH', fn, r.returncode, r.stderr.decode()[:400])
print(f'{N} mutations x 3 fns: {bad} crashes')
sys.exit(1 if bad else 0)
