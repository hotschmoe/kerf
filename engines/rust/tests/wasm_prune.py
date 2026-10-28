#!/usr/bin/env python3
"""Rewrite the export section of a wasm module to the SPEC 13.1 ABI only.
rustc always adds `--export=__heap_base --export=__data_end` for a cdylib; the ABI allows neither.
Usage: wasm_prune.py in.wasm out.wasm"""
import sys

KEEP = {b"memory", b"kerf_alloc", b"kerf_free", b"kerf_call", b"kerf_out_ptr", b"kerf_out_len"}

def leb(b, i):
    r = s = 0
    while True:
        c = b[i]; i += 1
        r |= (c & 0x7F) << s; s += 7
        if not c & 0x80: return r, i

def enc(n):
    out = bytearray()
    while True:
        c = n & 0x7F; n >>= 7
        if n: out.append(c | 0x80)
        else: out.append(c); return bytes(out)

data = open(sys.argv[1], "rb").read()
out = bytearray(data[:8]); i = 8
while i < len(data):
    sid = data[i]; size, j = leb(data, i + 1); body = data[j:j + size]
    if sid == 7:
        n, k = leb(body, 0); kept = []
        for _ in range(n):
            ln, k2 = leb(body, k); name = body[k2:k2 + ln]; k3 = k2 + ln
            _, k4 = leb(body, k3 + 1)
            if name in KEEP: kept.append(body[k:k4])
            k = k4
        body = enc(len(kept)) + b"".join(kept)
    out += bytes([sid]) + enc(len(body)) + body
    i = j + size
open(sys.argv[2], "wb").write(out)
