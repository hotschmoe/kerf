//! zlib stream with DEFLATE compression, an exact port of Zig 0.17's `std.compress.flate.Compress` at level
//! `.default` fed through one 64 KiB window buffer (the compressed bytes are what the Zig engine emits).
//! Self-contained: `zlib_compress(&[u8]) -> Vec<u8>`.

use alloc::vec;
use alloc::vec::Vec;

const HISTORY_LEN: usize = 32768;
const WINDOW_LEN: usize = HISTORY_LEN * 2;
const MIN_LENGTH: u16 = 3;
const MAX_LENGTH: usize = 258;
const SEQ_BYTES: usize = 3;
const RESERVED: usize = (MAX_LENGTH + 1) + SEQ_BYTES;
const BLOCK_TOKENS: usize = 1 << 15;
const HASH_BITS: u32 = 15;

const OPT_GOOD: u16 = 8;
const OPT_NICE: u16 = 128;
const OPT_LAZY: u16 = 16;
const OPT_CHAIN: u16 = 128;

const NULL_BIT: u16 = 0x8000;
const VALUE_MASK: u16 = 0x7fff;

const CODEGEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const LEN_BASE: [u16; 29] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 255];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 3072, 4096, 6144,
    8192, 12288, 16384, 24576,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn len_code(v: u8) -> usize {
    if v == 255 {
        return 28;
    }
    LEN_BASE[..28].partition_point(|&b| b <= u16::from(v)) - 1
}

fn dist_code(v: u16) -> usize {
    DIST_BASE.partition_point(|&b| b <= v) - 1
}

fn reverse_bits(code: u16, bits: u8) -> u16 {
    if bits == 0 { 0 } else { code.reverse_bits() >> (16 - u32::from(bits)) }
}

fn fixed_lit() -> ([u16; 286], [u8; 286]) {
    let mut codes = [0u16; 286];
    let mut bits = [0u8; 286];
    for i in 0..286usize {
        let (v, n): (u16, u8) = match i {
            0..=143 => (0b0011_0000 + i as u16, 8),
            144..=255 => (0b1_1001_0000 + (i as u16 - 144), 9),
            256..=279 => (i as u16 - 256, 7),
            _ => (0b1100_0000 + (i as u16 - 280), 8),
        };
        codes[i] = reverse_bits(v, n);
        bits[i] = n;
    }
    (codes, bits)
}

fn fixed_dist() -> ([u16; 30], [u8; 30]) {
    let mut codes = [0u16; 30];
    for (i, c) in codes.iter_mut().enumerate() {
        *c = reverse_bits(i as u16, 5);
    }
    (codes, [5; 30])
}

fn hash(x: u32) -> usize {
    (x.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

#[derive(Clone, Copy)]
enum Token {
    Lit(u8),
    Match { dist: u16, len: u8 },
}

struct BitWriter {
    out: Vec<u8>,
    buffered: u64,
    n: u32,
}

impl BitWriter {
    fn write(&mut self, bits: u64, n: u32) {
        self.buffered |= bits << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.buffered as u8);
            self.buffered >>= 8;
            self.n -= 8;
        }
    }

    fn byte_align(&mut self) {
        if self.n != 0 {
            self.out.push(self.buffered as u8);
            self.buffered = 0;
            self.n = 0;
        }
    }
}

struct Node {
    freq: u16,
    depth: u16,
}

impl Node {
    fn key(&self) -> u32 {
        (u32::from(self.freq) << 16) | u32::from(self.depth)
    }
}

const MAX_LEAFS: usize = 286;
const MAX_NODES: usize = MAX_LEAFS * 2;

fn heap_sift_down(nodes: &[Node], heap: &mut [u16], start: usize) {
    let mut i = start;
    loop {
        let mut min = i;
        let l = i * 2 + 1;
        let r = l + 1;
        if l < heap.len() && nodes[usize::from(heap[l])].key() < nodes[usize::from(heap[min])].key() {
            min = l;
        }
        if r < heap.len() && nodes[usize::from(heap[r])].key() < nodes[usize::from(heap[min])].key() {
            min = r;
        }
        if i == min {
            break;
        }
        heap.swap(i, min);
        i = min;
    }
}

fn heap_remove_root(nodes: &[Node], heap: &mut [u16]) {
    let last = heap.len() - 1;
    heap[0] = heap[last];
    heap_sift_down(nodes, &mut heap[..last], 0);
}

/// Huffman code lengths and codes for `freqs` (`out_bits` zero-filled on entry). Returns the total
/// encoded bits and the index of the last used symbol.
fn huffman_build(
    freqs: &[u16],
    out_codes: &mut [u16],
    out_bits: &mut [u8],
    max_bits: u8,
    incomplete_allowed: bool,
) -> (u32, u16) {
    let mut tree_nodes: Vec<Node> = (0..MAX_NODES).map(|_| Node { freq: 0, depth: 0 }).collect();
    let mut parent = vec![0u16; MAX_NODES];
    let mut nodes_end = MAX_LEAFS as u16;
    let mut node_buf = vec![0u16; MAX_NODES];
    let mut heap_end = 0usize;
    let mut sorted_start = MAX_NODES;

    for (n, &freq) in freqs.iter().enumerate() {
        tree_nodes[n] = Node { freq, depth: 0 };
        node_buf[heap_end] = n as u16;
        heap_end += usize::from(freq != 0);
    }

    node_buf[heap_end] = 0;
    heap_end += usize::from(heap_end == 0);
    if heap_end == 1 && incomplete_allowed {
        let s = usize::from(node_buf[0]);
        out_bits[s] = 1;
        out_codes[s] = 0;
        return (u32::from(freqs[s]), node_buf[0]);
    }
    let last_nonzero = node_buf[heap_end - 1].max(1);
    node_buf[heap_end] = u16::from(node_buf[0] == 0);
    heap_end += usize::from(heap_end == 1);

    let mut hi = (heap_end - 2) / 2;
    loop {
        heap_sift_down(&tree_nodes, &mut node_buf[..heap_end], hi);
        if hi == 0 {
            break;
        }
        hi -= 1;
    }

    while heap_end > 1 {
        let a = node_buf[0];
        heap_remove_root(&tree_nodes, &mut node_buf[..heap_end]);
        heap_end -= 1;
        let b = node_buf[0];
        sorted_start -= 2;
        node_buf[sorted_start] = b;
        node_buf[sorted_start + 1] = a;
        let ne = usize::from(nodes_end);
        let (na, nb) = (&tree_nodes[usize::from(a)], &tree_nodes[usize::from(b)]);
        tree_nodes[ne] = Node { freq: na.freq.wrapping_add(nb.freq), depth: na.depth.max(nb.depth) + 1 };
        parent[usize::from(a)] = nodes_end;
        parent[usize::from(b)] = nodes_end;
        node_buf[0] = nodes_end;
        nodes_end += 1;
        heap_sift_down(&tree_nodes, &mut node_buf[..heap_end], 0);
    }
    sorted_start -= 1;
    node_buf[sorted_start] = node_buf[0];

    let mut bit_counts = [0u16; 16];
    build_bits(out_bits, &mut bit_counts, &parent, &node_buf[sorted_start..], max_bits);
    (build_values(freqs, out_codes, out_bits, &bit_counts), last_nonzero)
}

fn build_bits(out_bits: &mut [u8], bit_counts: &mut [u16; 16], parent: &[u16], sorted: &[u16], max_bits: u8) {
    let mut internal = [0u8; MAX_NODES - MAX_LEAFS];
    let mut overflowed = 0u16;
    internal[usize::from(sorted[0]) - MAX_LEAFS] = 0;
    for &i in &sorted[1..] {
        let i = usize::from(i);
        let parent_bits = internal[usize::from(parent[i]) - MAX_LEAFS];
        overflowed += u16::from(parent_bits == max_bits);
        let bits = parent_bits + u8::from(parent_bits != max_bits);
        bit_counts[usize::from(bits)] += u16::from(i < MAX_LEAFS);
        if i >= MAX_LEAFS {
            internal[i - MAX_LEAFS] = bits;
        } else {
            out_bits[i] = bits;
        }
    }
    if overflowed == 0 {
        return;
    }
    'outer: loop {
        let mut deepest = usize::from(max_bits) - 1;
        while bit_counts[deepest] == 0 {
            deepest -= 1;
        }
        while overflowed != 0 {
            bit_counts[deepest] -= 1;
            bit_counts[deepest + 1] += 2;
            bit_counts[usize::from(max_bits)] -= 1;
            overflowed -= 2;
            if overflowed == 0 {
                break 'outer;
            }
            deepest += 1;
            if deepest == usize::from(max_bits) {
                continue 'outer;
            }
        }
    }
    let mut i = 0usize;
    for (bits, &count) in bit_counts.iter().enumerate().take(16).skip(1) {
        let mut remaining = count;
        while remaining != 0 {
            let s = usize::from(sorted[i]);
            i += 1;
            if s >= MAX_LEAFS {
                continue;
            }
            out_bits[s] = bits as u8;
            remaining -= 1;
        }
    }
}

fn build_values(freqs: &[u16], out_codes: &mut [u16], bits: &[u8], bit_counts: &[u16; 16]) -> u32 {
    let mut code = 0u16;
    let mut base = [0u16; 16];
    for b in 1..16 {
        base[b] = code;
        code = code.wrapping_add(bit_counts[b]) << 1;
    }
    let mut total = 0u32;
    for ((c, &b), &f) in out_codes.iter_mut().zip(bits).zip(freqs) {
        let b = usize::from(b);
        *c = reverse_bits(base[b], b as u8);
        base[b] = base[b].wrapping_add(1);
        total += u32::from(f) * b as u32;
    }
    total
}

/// Run-length encodes the code lengths with the code-length alphabet. Returns the extra bit count.
fn build_clen(dyn_bits: &[u8], values: &mut Vec<u8>, extra: &mut Vec<u8>, freqs: &mut [u16; 19]) -> u32 {
    let mut extra_bitsize = 0u32;
    let mut rest = dyn_bits;
    let mut prev = 0u8;
    while let Some(&b) = rest.first() {
        let n_max = usize::from(if b != 0 { if b != prev { 1u8 } else { 6 } } else { 138 }).min(rest.len());
        prev = b;
        let mut n = 0usize;
        loop {
            rest = &rest[1..];
            n += 1;
            if n == n_max || rest.first() != Some(&b) {
                break;
            }
        }
        let (code, ex, xsize): (u8, u8, u32) = match n {
            1..=2 => (b, 0, 0),
            3..=10 => (16 + u8::from(b == 0), (n - 3) as u8, 2 + u32::from(b == 0)),
            _ => (18, (n - 11) as u8, 7),
        };
        loop {
            values.push(code);
            extra.push(ex);
            freqs[usize::from(code)] += 1;
            extra_bitsize += xsize;
            if n != 2 {
                break;
            }
            n = 1;
        }
    }
    extra_bitsize
}

fn clen_hlen(freqs: &[u16; 19]) -> u32 {
    let mut max = 0;
    for (len, &i) in CODEGEN_ORDER[4..].iter().enumerate() {
        if freqs[i] != 0 {
            max = len as u32 + 1;
        }
    }
    max
}

struct Lookup {
    head: Vec<u16>,
    chain: Vec<u16>,
    chain_pos: u16,
}

struct Compressor {
    buf: Vec<u8>,
    end: usize,
    history_len: usize,
    bits: BitWriter,
    tokens: Vec<Token>,
    lit_freqs: [u16; 286],
    dist_freqs: [u16; 30],
    lookup: Lookup,
}

#[derive(Clone, Copy)]
struct Match {
    dist: u16,
    len: u16,
}

impl Compressor {
    fn new() -> Self {
        Compressor {
            buf: vec![0; WINDOW_LEN],
            end: 0,
            history_len: 0,
            bits: BitWriter { out: Vec::new(), buffered: 0, n: 0 },
            tokens: Vec::with_capacity(BLOCK_TOKENS),
            lit_freqs: [0; 286],
            dist_freqs: [0; 30],
            lookup: Lookup { head: vec![0xffff; 1 << HASH_BITS], chain: vec![0xffff; 32768], chain_pos: VALUE_MASK },
        }
    }

    fn write_all(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.end + bytes.len() <= WINDOW_LEN {
                self.buf[self.end..self.end + bytes.len()].copy_from_slice(bytes);
                self.end += bytes.len();
                return;
            }
            let n = WINDOW_LEN - self.end;
            self.buf[self.end..].copy_from_slice(&bytes[..n]);
            self.end = WINDOW_LEN;
            self.rebase(false, false);
            bytes = &bytes[n..];
        }
    }

    fn output_match(&mut self, dist: u16, len: u8) {
        if self.tokens.len() == BLOCK_TOKENS {
            self.write_block(false);
        }
        self.tokens.push(Token::Match { dist, len });
        self.lit_freqs[257 + len_code(len)] += 1;
        self.dist_freqs[dist_code(dist)] += 1;
    }

    fn output_bytes(&mut self, from: usize, to: usize) {
        for i in from..to {
            if self.tokens.len() == BLOCK_TOKENS {
                self.write_block(false);
            }
            let b = self.buf[i];
            self.tokens.push(Token::Lit(b));
            self.lit_freqs[usize::from(b)] += 1;
        }
    }

    fn add_hash(&mut self, i: usize, h: usize) {
        let l = &mut self.lookup;
        l.chain_pos = l.chain_pos.wrapping_add(1) & VALUE_MASK;
        if i >= HISTORY_LEN {
            let r = i - HISTORY_LEN;
            let word = u32::from_be_bytes([self.buf[r], self.buf[r + 1], self.buf[r + 2], self.buf[r + 3]]);
            let rh = hash(word >> 8);
            if l.head[rh] == l.chain_pos {
                l.head[rh] |= NULL_BIT;
            }
        }
        let prev = l.head[h];
        l.chain[usize::from(l.chain_pos)] =
            (l.chain_pos.wrapping_sub(prev & VALUE_MASK) & VALUE_MASK) | (prev & NULL_BIT);
        l.head[h] = l.chain_pos;
    }

    fn match_and_add_hash(&mut self, i: usize, h: usize, gt: u16, max_chain: u16, good_: u16) -> Match {
        let mut chain_limit = max_chain;
        let mut best_dist = 0u16;
        let mut best_len = gt;
        let remaining = self.end - i;
        let nice = OPT_NICE.min(remaining as u16);
        let mut good = good_;
        let head = self.lookup.head[h];
        if head & NULL_BIT == 0 {
            let mut dist = self.lookup.chain_pos.wrapping_sub(head & VALUE_MASK) & VALUE_MASK;
            loop {
                chain_limit = chain_limit.wrapping_sub(1);
                let len = match_len(&self.buf[i - 1 - usize::from(dist)..self.end], &self.buf[i..self.end]);
                if len > best_len {
                    best_dist = dist;
                    best_len = len;
                    if best_len >= nice {
                        break;
                    }
                    if best_len >= good {
                        chain_limit >>= 2;
                        good = 255;
                    }
                }
                if chain_limit == 0 {
                    break;
                }
                let next = usize::from(self.lookup.chain_pos.wrapping_sub(dist & VALUE_MASK) & VALUE_MASK);
                let link = self.lookup.chain[next];
                dist = dist.wrapping_add(link);
                if (dist | link) & 0x8000 != 0 {
                    break;
                }
            }
        }
        self.add_hash(i, h);
        Match { dist: best_dist, len: best_len }
    }

    fn rebase(&mut self, is_flush: bool, is_finish: bool) {
        let len = self.end;
        let start = self.history_len;
        let hashable_len = len.saturating_sub(SEQ_BYTES - 1);
        let matching_end = if is_flush { hashable_len } else { len - RESERVED };

        let mut i = start;
        let mut last_unmatched = i;
        let mut seq: u32 =
            if i >= hashable_len { 0 } else { (u32::from(self.buf[i]) << 8) | u32::from(self.buf[i + 1]) };

        'main: while i < matching_end {
            let mut match_start = i;
            seq = ((seq << 8) | u32::from(self.buf[i + 2])) & 0xff_ffff;
            let mut m = self.match_and_add_hash(i, hash(seq), MIN_LENGTH - 1, OPT_CHAIN, OPT_GOOD);
            i += 1;
            if m.len < MIN_LENGTH {
                continue;
            }
            let mut match_unadded = m.len - 1;
            'lazy: {
                if m.len >= OPT_LAZY || usize::from(m.len) >= len - i {
                    break 'lazy;
                }
                let mut chain = OPT_CHAIN;
                let mut good = OPT_GOOD;
                if m.len >= good {
                    chain >>= 2;
                    good = 255;
                }
                seq = ((seq << 8) | u32::from(self.buf[i + 2])) & 0xff_ffff;
                let lazy = self.match_and_add_hash(i, hash(seq), m.len, chain, good);
                match_unadded -= 1;
                i += 1;
                if lazy.len > m.len {
                    match_start += 1;
                    m = lazy;
                    match_unadded = m.len - 1;
                }
            }

            self.output_bytes(last_unmatched, match_start);
            self.output_match(m.dist, (m.len - MIN_LENGTH) as u8);
            last_unmatched = match_start + usize::from(m.len);

            loop {
                if i >= hashable_len {
                    break 'main;
                }
                seq = ((seq << 8) | u32::from(self.buf[i + 2])) & 0xff_ffff;
                self.add_hash(i, hash(seq));
                i += 1;
                match_unadded -= 1;
                if match_unadded == 0 {
                    break;
                }
            }
        }

        if is_flush {
            self.output_bytes(last_unmatched, len);
            if is_finish {
                self.write_block(true);
                return;
            }
            i = len;
        } else {
            self.output_bytes(last_unmatched, i);
        }

        self.history_len = i.min(HISTORY_LEN);
        let keep_from = i - self.history_len;
        self.buf.copy_within(keep_from..len, 0);
        self.end = len - keep_from;
    }

    fn write_block(&mut self, eos: bool) {
        self.lit_freqs[256] = 1;
        let mut codes = [0u16; 286 + 30];
        let mut bits = [0u8; 286 + 30];

        let (lit_bitsize, last_lit) = huffman_build(&self.lit_freqs, &mut codes[..286], &mut bits[..286], 15, true);
        let lit_len = (usize::from(last_lit) + 1).max(257);
        let (dist_bitsize, last_dist) = huffman_build(
            &self.dist_freqs,
            &mut codes[lit_len..lit_len + 30],
            &mut bits[lit_len..lit_len + 30],
            15,
            true,
        );
        let dist_len = (usize::from(last_dist) + 1).max(1);

        let mut clen_values = Vec::new();
        let mut clen_extra = Vec::new();
        let mut clen_freqs = [0u16; 19];
        let clen_extra_bitsize =
            build_clen(&bits[..lit_len + dist_len], &mut clen_values, &mut clen_extra, &mut clen_freqs);

        let mut clen_codes = [0u16; 19];
        let mut clen_bits = [0u8; 19];
        let (clen_bitsize, _) = huffman_build(&clen_freqs, &mut clen_codes, &mut clen_bits, 7, false);
        let hclen = clen_hlen(&clen_freqs);

        let dynamic_bitsize = 14 + (4 + hclen) * 3 + clen_bitsize + clen_extra_bitsize + lit_bitsize + dist_bitsize;
        let fixed_bitsize = {
            let f = &self.lit_freqs;
            let sum = |r: core::ops::Range<usize>| f[r].iter().map(|&x| u32::from(x)).sum::<u32>();
            7 + sum(0..144) * 8 + sum(144..256) * 9 + sum(257..280) * 12 + sum(280..286) * 13
        };

        let stored_ok = self.dist_freqs.iter().all(|&n| n == 0) && {
            let align = (8 - ((self.bits.n + 3) % 8)) % 8;
            let stored_bitsize = align + 32 + self.tokens.len() as u32 * 8;
            dynamic_bitsize.min(fixed_bitsize) >= stored_bitsize
        };

        if stored_ok {
            self.bits.write(u64::from(eos), 3);
            self.bits.byte_align();
            let n = self.tokens.len() as u16;
            self.bits.out.extend_from_slice(&n.to_le_bytes());
            self.bits.out.extend_from_slice(&(!n).to_le_bytes());
            for t in &self.tokens {
                if let Token::Lit(b) = t {
                    self.bits.out.push(*b);
                }
            }
            self.reset_tokens();
            return;
        }

        let dynamic = dynamic_bitsize < fixed_bitsize;
        let (fl_codes, fl_bits) = fixed_lit();
        let (fd_codes, fd_bits) = fixed_dist();
        let (lit_codes, lit_bits, dist_codes, dist_bits): (&[u16], &[u8], &[u16], &[u8]) = if dynamic {
            let header = u64::from(eos)
                | (2 << 1)
                | (((lit_len - 257) as u64) << 3)
                | (((dist_len - 1) as u64) << 8)
                | ((u64::from(hclen)) << 13);
            self.bits.write(header, 17);
            self.write_clen(hclen, &clen_values, &clen_extra, &clen_codes, &clen_bits);
            (
                &codes[..lit_len],
                &bits[..lit_len],
                &codes[lit_len..lit_len + dist_len],
                &bits[lit_len..lit_len + dist_len],
            )
        } else {
            self.bits.write(u64::from(eos) | (1 << 1), 3);
            (&fl_codes, &fl_bits, &fd_codes, &fd_bits)
        };

        for t in &self.tokens {
            match *t {
                Token::Lit(b) => {
                    let b = usize::from(b);
                    self.bits.write(u64::from(lit_codes[b]), u32::from(lit_bits[b]));
                }
                Token::Match { dist, len } => {
                    let lc = len_code(len);
                    let dc = dist_code(dist);
                    let lit_val = 257 + lc;
                    let mut out = u64::from(lit_codes[lit_val]);
                    let mut n = u32::from(lit_bits[lit_val]);
                    out |= u64::from(u16::from(len) - LEN_BASE[lc]) << n;
                    n += u32::from(LEN_EXTRA[lc]);
                    out |= u64::from(dist_codes[dc]) << n;
                    n += u32::from(dist_bits[dc]);
                    out |= u64::from(dist - DIST_BASE[dc]) << n;
                    n += u32::from(DIST_EXTRA[dc]);
                    self.bits.write(out, n);
                }
            }
        }
        self.bits.write(u64::from(lit_codes[256]), u32::from(lit_bits[256]));
        self.reset_tokens();
    }

    fn reset_tokens(&mut self) {
        self.tokens.clear();
        self.lit_freqs = [0; 286];
        self.dist_freqs = [0; 30];
    }

    fn write_clen(&mut self, hclen: u32, values: &[u8], extra: &[u8], codes: &[u16; 19], bits: &[u8; 19]) {
        let b = |k: usize| u64::from(bits[CODEGEN_ORDER[k]]);
        self.bits.write(b(0) | (b(1) << 3) | (b(2) << 6) | (b(3) << 9), 12);
        let mut table = 0u64;
        for k in (0..hclen as usize).rev() {
            table = (table << 3) | b(4 + k);
        }
        self.bits.write(table, hclen * 3);
        for (&v, &e) in values.iter().zip(extra) {
            let v = usize::from(v);
            let xbits = match v {
                16 => 2,
                17 => 3,
                18 => 7,
                _ => 0,
            };
            self.bits.write(u64::from(codes[v]) | (u64::from(e) << bits[v]), u32::from(bits[v]) + xbits);
        }
    }
}

/// Common prefix length of `prev` and `bytes`, at most 258 and at most `bytes.len()`.
fn match_len(prev: &[u8], bytes: &[u8]) -> u16 {
    let cap = bytes.len().min(MAX_LENGTH);
    let mut n = 0;
    while n < cap && prev[n] == bytes[n] {
        n += 1;
    }
    n as u16
}

/// The raw DEFLATE stream of `data` (no zlib header or trailer: the verified `png::zlib_frame` adds them).
pub fn deflate_raw(data: &[u8]) -> Vec<u8> {
    let mut c = Compressor::new();
    c.write_all(data);
    c.rebase(true, true);
    c.bits.byte_align();
    c.bits.out
}
