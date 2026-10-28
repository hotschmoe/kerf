#!/usr/bin/env python3
"""kernel_share.py <folded> [--exclude-png] : share of profile samples inside the modules reserved for the verified
(Verus) versions: json parse, json writers/number formatting, sort, png filters/framing, deflate, pdf writer, raster
kernels. A sample is charged to the innermost such frame. Inlined frames carry short names (addr2line), so names
defined in those files are matched too; attribution of inlined helpers is approximate."""
import collections
import re
import sys

SRC = '/home/hotschmoe/github/kerf-rust/engines/rust/kerf-core/src/'
def fns(path, start=None, end=None):
    txt = open(SRC + path).read()
    if start:
        txt = txt[txt.index(start):txt.index(end) if end else None]
    return set(re.findall(r'fn (\w+)', txt))

groups = {
    'json parse': (r'json::(parse|Parser)', {'object_body', 'array_body', 'value', 'peek', 'ws', 'digits', 'document'}),
    'json write/format': (r'json::(fmt_|push_number|number_string|write|Pretty|round)', set()),
    'sort': (r'kerf_core::sort::', fns('sort.rs')),
    'deflate': (r'png::deflate::', fns('png/deflate.rs') - {'new', 'write', 'key'}),
    'png filter/crc/chunks': (r'kerf_core::png::(encode|filter|row_score|paeth|crc|write_chunk)', {'filter_row', 'row_score', 'paeth', 'crc32_update', 'write_chunk'}),
    'pdf writer': (r'pdf::writer::', set()),
    'raster kernels': (r'raster::kernels::', {'composite', 'union_max'}),
}
excl = '--exclude-png' in sys.argv
share, tot = collections.Counter(), 0
for line in open(sys.argv[1]):
    st, _, w = line.rstrip('\n').rpartition(' ')
    fr = st.split(';')
    if excl and any('raster::' in f or 'png::' in f or f in ('encode', 'rasterize', 'filter_row', 'zlib_compress') for f in fr):
        continue
    w = int(w)
    tot += w
    for f in reversed(fr):
        g = next((g for g, (rx, names) in groups.items() if re.search(rx, f) or f in names), None)
        if g:
            share[g] += w
            break
for g in groups:
    print(f'{100 * share[g] / tot:5.1f}%  {g}')
print(f'{100 * sum(share.values()) / tot:5.1f}%  all reserved modules')
