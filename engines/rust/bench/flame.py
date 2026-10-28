#!/usr/bin/env python3
"""Profile helpers without third-party tooling (the cix perf has no libelf, so it cannot symbolize).

  flame.py collapse <perf script output> <binary> > folded.txt   symbolize with binutils addr2line -i (inlined frames kept)
  flame.py top <folded.txt> [N]                                   self and inclusive share per function
  flame.py svg <folded.txt> [title] > out.svg                     icicle-free classic flame graph (static SVG, hover titles)

`perf record -g` needs frame pointers: build with RUSTFLAGS="-C force-frame-pointers=yes" (profile `profiling`).
"""
import collections
import html
import re
import subprocess
import sys


def collapse(script_path, binary):
    samples = []  # (weight, [(dso, addr, is_leaf)] leaf-first)
    cur = None
    with open(script_path, errors='replace') as f:
        for line in f:
            if not line.strip():
                if cur:
                    samples.append(cur)
                cur = None
                continue
            if not line.startswith(('\t', ' ')):
                m = re.search(r'\s(\d+)\s+\S+:\s*$', line)
                cur = (int(m.group(1)) if m else 1, [])
                continue
            m = re.match(r'\s*([0-9a-f]+)\s+.*\((.*)\)\s*$', line)
            if m and cur is not None:
                cur[1].append((m.group(2), int(m.group(1), 16)))
    if cur:
        samples.append(cur)
    addrs = set()
    for _, frames in samples:
        for i, (dso, a) in enumerate(frames):
            if dso == binary or dso.endswith('/' + binary.split('/')[-1]):
                addrs.add(a if i == 0 else a - 4)
    addrs = sorted(addrs)
    names = {}
    if addrs:
        out = subprocess.run(['addr2line', '-f', '-i', '-C', '-a', '-e', binary], input='\n'.join(hex(a) for a in addrs),
                             capture_output=True, text=True).stdout.splitlines()
        # addr2line -a -f -i: the address line, then one (function, file:line) pair per inline level, innermost first
        i = 0
        while i < len(out):
            if re.fullmatch(r'0x[0-9a-f]+', out[i]):
                a = int(out[i], 16)
                i += 1
                chain = []
                while i < len(out) and not re.fullmatch(r'0x[0-9a-f]+', out[i]):
                    fn = re.sub(r'::h[0-9a-f]{16}$', '', out[i])
                    chain.append(fn)
                    i += 2
                names[a] = chain  # innermost first
            else:
                i += 1
    folded = collections.Counter()
    for w, frames in samples:
        stack = []
        for i, (dso, a) in enumerate(frames):
            key = a if i == 0 else a - 4
            if key in names:
                stack.extend(names[key])
            elif dso.startswith('[') or a >= 0xffff000000000000:
                stack.append('[kernel]')
            else:
                stack.append('[' + dso.split('/')[-1] + ']')
        stack.reverse()
        dedup = [s for j, s in enumerate(stack) if j == 0 or s != '[kernel]' or stack[j - 1] != '[kernel]']
        folded[';'.join(dedup)] += w
    for k, v in folded.items():
        sys.stdout.write(f'{k} {v}\n')


def load(path):
    rows = []
    for line in open(path):
        stack, _, w = line.rstrip('\n').rpartition(' ')
        rows.append((stack.split(';'), int(w)))
    return rows


def top(path, n=40):
    rows = load(path)
    total = sum(w for _, w in rows)
    selfc, incl = collections.Counter(), collections.Counter()
    for st, w in rows:
        selfc[st[-1]] += w
        for f in set(st):
            incl[f] += w
    print(f'total weight {total}')
    print('--- self ---')
    for f, w in selfc.most_common(n):
        print(f'{100 * w / total:6.2f}%  {f}')
    print('--- inclusive ---')
    for f, w in incl.most_common(n):
        print(f'{100 * w / total:6.2f}%  {f}')


def svg(path, title='flame graph'):
    rows = load(path)
    root = {'n': 'all', 'w': 0, 'c': {}}
    for st, w in rows:
        node = root
        node['w'] += w
        for f in st:
            node = node['c'].setdefault(f, {'n': f, 'w': 0, 'c': {}})
            node['w'] += w
    width, fh, total = 1600, 16, root['w']
    rects, depth_max = [], [0]

    def walk(node, x, d):
        depth_max[0] = max(depth_max[0], d)
        rects.append((x, d, node))
        cx = x
        for ch in sorted(node['c'].values(), key=lambda c: c['n']):
            walk(ch, cx, d + 1)
            cx += ch['w']

    walk(root, 0, 0)
    height = (depth_max[0] + 2) * fh + 30
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" font-family="monospace" font-size="11">',
           f'<rect width="100%" height="100%" fill="#fdfdf6"/><text x="8" y="18" font-size="14">{html.escape(title)}</text>']
    for x, d, node in rects:
        w = node['w'] * (width - 16) / total
        if w < 0.4:
            continue
        px, py = 8 + x * (width - 16) / total, height - (d + 1) * fh - 4
        h = sum(map(ord, node['n'])) % 60
        col = f'rgb({200 + h // 2},{90 + h * 2},{40 + h // 3})'
        label = html.escape(node['n'])
        pct = 100 * node['w'] / total
        out.append(f'<g><title>{label} ({pct:.2f}%)</title><rect x="{px:.1f}" y="{py}" width="{w:.1f}" height="{fh - 1}" fill="{col}"/>')
        chars = int(w / 6.6)
        if chars >= 4:
            txt = node['n'] if len(node['n']) <= chars else node['n'][:chars - 2] + '..'
            out.append(f'<text x="{px + 2:.1f}" y="{py + 11}">{html.escape(txt)}</text>')
        out.append('</g>')
    out.append('</svg>')
    sys.stdout.write('\n'.join(out) + '\n')


if __name__ == '__main__':
    cmd = sys.argv[1]
    if cmd == 'collapse':
        collapse(sys.argv[2], sys.argv[3])
    elif cmd == 'top':
        top(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 40)
    elif cmd == 'svg':
        svg(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else 'flame graph')
