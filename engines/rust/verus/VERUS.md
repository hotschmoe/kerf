# Rust + Verus on Kerf: what it costs, what it proves, how it feels

Verification track of the Rust port. Five kernels of the Zig engine were re-implemented in Rust and
proven with Verus. They ship as plain `no_std` Rust with **no dependencies and no proc macros**, and
are checked end to end against the Zig engine's real outputs. Everything here was measured on this
machine (aarch64, Debian 12, 12 cores) on 2026-10-28.

| | |
|---|---|
| Crate | `engines/rust/kerf-verified` (the verified sources *are* the shipped sources) |
| Verify | `engines/rust/verus/verify.sh`: 188 items, about 6 s wall, then plain stable builds for native and wasm32 |
| End-to-end checks | `engines/rust/verus/check/run.sh`: every check identical to Zig (about 3.5 min) |
| Toolchain | `engines/rust/verus/setup.sh`: Verus 0.2026.10.26.d17fbd0 and Z3 4.16.0, both built from source |

## 1. Verus in one page

Verus checks Rust code against specifications written in Rust-like syntax. A function carries
`requires` (preconditions) and `ensures` (postconditions). Loops carry `invariant` and `decreases`.
Spec functions are mathematical definitions over `int`, `nat`, `Seq` and `Multiset`. Proof functions
(lemmas) and `proof { }` blocks guide the prover.

Verus turns each function into SMT queries for Z3. Every function is verified on its own, against
the *specs* of the functions it calls, never their bodies. Several checks come for free on all exec
code:

- arithmetic overflow and underflow
- out-of-bounds indexing
- `unwrap` of `None`
- termination: every loop and recursion needs a `decreases` measure

Ghost code (specs, proofs, ghost variables) is erased before compilation, so it costs nothing at run
time.

What it is not:

- **Floats are not modelled.** `f64` values are uninterpreted and nothing about IEEE arithmetic is
  proven.
- **Bounds checks stay in the binary.** Proving an index in bounds does not let rustc drop the check.
- **Quantifier triggers can bite.** Proofs depend on Z3 picking the right instances of `forall`
  facts, and section 7 lists the three times this mattered here.

## 2. Phase A: toolchain on this machine

**The official binaries do not run here.** The first arm64-linux release zip (0.2026.10.26, 461 MB,
14 s to download) needs glibc 2.39, and its bundled z3 needs glibc 2.38 and GLIBCXX_3.4.32. Debian
12 has glibc 2.36:

```
./verus: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by ./verus)
./z3: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found (required by ./z3)
```

Z3's own arm64 release is also glibc 2.38. So both were built from source.

**Z3 4.16.0 does not build with the system compiler.** It uses C++20 `<format>`, which Debian's
GCC 12 and clang 14/19 (with libstdc++) lack:

```
fatal error: format: No such file or directory
```

Zig 0.17, already Kerf's own toolchain, solved it: `zig c++ -target aarch64-linux-gnu.2.36` brings
clang plus libc++. That gives a Z3 linked against this machine's glibc, with libc++ static. No cmake
was needed (Z3's `mk_make.py`).

**Verus is built with vargo.** The standalone `verus` binary needs `vstd.vir` and the builtin crates
in `target-verus/release`, which only the vargo flow produces. `cargo make build` covers only
cargo-verus projects.

`setup.sh` reproduces it all. These are clean-run numbers:

| step | wall time | disk |
|---|---:|---:|
| `git clone --depth 1` Verus at `release/0.2026.10.26.d17fbd0` | 4.6 s | 37 MB |
| `rustup toolchain install` (pinned 1.99.1, with `rustc-dev` and `llvm-tools`) | 17.0 s | 1.27 GB |
| Z3 4.16.0 from source with `zig c++`, stripped | 559.6 s | 21 MB (293 MB unstripped) |
| vargo build: verifier, builtin crates, and vstd itself verified (2102 items) | 204.7 s | 1.01 GB |
| Verus examples (vectors, bitvector_basic, mergesort, rw2022_script) | 5.8 s | – |
| **total** | **≈ 13.2 min** | **≈ 2.3 GB** |

On Ubuntu 24.04 or newer (glibc 2.39) the release zip works as is: about 30 s and 1.6 GB unpacked.

One side effect: the first run let rustup self-update (1.29.1 → 1.29.2). The script now passes
`--no-self-update`.

## 3. Phase B: how verified code ships

Every option was measured with one driver crate that calls all five kernels, built with stable 1.96
and `panic=abort`, `lto`, `codegen-units=1`. Times are the median of 3 runs.

| | 1. vstd and macro crates from crates.io | 2. Verus's erasure, generated copy (`erase.py`) | **3. Cfg-gated attribute syntax (chosen)** |
|---|---:|---:|---:|
| builds on stable 1.96, wasm32, `no_std` + `alloc` | yes | yes | yes |
| crates in the build | 21 (2 proc macros, `syn` ×2, `verus_syn`, …) | 2 | 2 |
| clean release build, wasm32 | 12.97 s | 0.98 s | 0.93 s |
| clean release build, native | 12.31 s | 0.96 s | 0.96 s |
| clean debug build, native | 11.99 s | 0.39 s | 0.41 s |
| incremental (touch json.rs), wasm32 release | 1.12 s | 0.81 s | 0.87 s |
| `target/` size | 243 MB | 9 MB | 9 MB |
| wasm size | 52,645 B | 52,621 B | 52,757 B |

The wasm differences are only the embedded panic-location paths (`proof/src/…`, `src/…`, a longer
scratch path). twiggy shows the same function-size profile (identical hash) for all three options:
the shipped machine code is the same.

**1. vstd from crates.io works.** It builds on stable, wasm32 and `no_std`, but it costs 13x the
clean build time and 19 third-party crates. The two proc macros run on every build. That fails the
owner's constraints.

**2. Erasure plus a generated copy works but carries a lot of machinery.** `erase.py` compiles the
proofs as plain Rust (the `verus!` macro erases ghost code) and prints `-Zunpretty=expanded`. It then
cleans the result: `{}` left by proof blocks, vstd imports, `#[doc]` attributes, and `derive`s that
expand into unstable `TrivialClone` impls and had to be folded back. The output is idiomatic.

Two pitfalls showed up:

- **Ghost parameters survive erasure.** Verus keeps `Ghost<…>` parameters in signatures, as
  `Ghost::assume_new_fallback(|| unreachable!())`, so vstd survives erasure. Rule adopted: no
  `Ghost`/`Tracked` in exec signatures; callees return ghost facts through `exists` in `ensures`. The
  sort proof was rewritten to follow it.
- **vstd extension methods** (`v.set(i, x)`) do not exist without vstd. Use `v[i] = x`.

It also needs two copies of every file, a generator in the trusted base, and a regenerate-and-diff
CI job that itself needs the Verus toolchain. It was used up to commit `3f4d940`.

**3. Cfg-gated attribute syntax (chosen).** Exec functions are plain Rust with
`#[cfg_attr(verus_keep_ghost, verus_spec(requires …, ensures …))]`. Loops take the same attribute
with `invariant`/`decreases`. Proof blocks are `#[cfg(verus_keep_ghost)] proof! { … }` and ghost
variables `#[cfg(verus_keep_ghost)] proof_decl! { let ghost … }`. Spec functions and lemmas sit in
`#[cfg(verus_keep_ghost)] verus! { … }` blocks.

Verus sets `verus_keep_ghost`; stable rustc does not, so it sees ordinary Rust. Verus's test suite
covers this setup ("specifications gated on a cfg, for code also built without Verus").

- **The shipped file is the verified file.** No generator, no `-Zunpretty`, no regex post-processing
  in the trusted base.
- **A developer without Verus can still build and edit it.** CI's Verus job catches a broken proof.
- **The whole crate converted mechanically.** A one-off script (not committed) converted the
  `verus!` sources and they verified unchanged (188 items).
- **Cost: syntax noise.** The `cfg_attr`/`proof!` wrapping makes the sources about 15% longer (4,685 vs 4,087 lines) and
  less pleasant than `verus!` syntax. rustfmt does not format inside the attributes.
- **Cost: less-trodden path.** This syntax is newer and less documented than `verus!`.

A short example from `pdf.rs`:

```rust
#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + xref_entry(o as nat),
))]
fn push_entry(out: &mut Vec<u8>, o: usize) {
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_inuse); }
    push_dec_fixed(out, o, 10);
    push_bytes(out, &[32u8, 48, 48, 48, 48, 48, 32, 110, 32, 10]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + xref_entry(o as nat)); }
}
```

## 4. Phase C: the kernels, what is proven, and the evidence

None of the public functions has a precondition an unverified caller must meet, except the
`PdfWriter` and `Canvas` methods. Those require a well-formedness invariant over private fields: the
constructor establishes it, every method preserves it, and safe Rust cannot break it from outside.

### 4.1 PDF writer object table and xref (`pdf.rs`, `dec.rs`)

API: `PdfWriter::new`, `begin_object() -> N`, `push(&[u8])`, `push_usize`,
`finish(root, info) -> Option<Vec<u8>>`. It follows the object sequence of `pdf.zig`.

**Proven** (`finish`'s `ensures`):

- The output is exactly `pdf_file(body, offsets, root, info)`: body, then
  `xref\n0 {n+1}\n0000000000 65535 f \n`, then one entry per object, then the trailer. This is a
  full functional spec.
- `xref_valid` holds. For every object i:
  - its entry sits at `x + len(head) + 20·i`, so entries are in object order and exactly 20 bytes;
  - the entry equals `{offset:0>10} 00000 n \n`, and its first 10 bytes are digits whose value is
    the offset;
  - the bytes at that offset are `"{i+1} 0 obj\n"` and lie before the xref.
- The `xref` keyword is at x. The file ends `startxref\n` + digits whose value is x + `\n%%EOF\n`.
- `push_dec` appends `dec(n)`, and `digits_val(dec(n)) == n`.
- `None` is returned only when an object starts at byte 10^10 or later.

**Finding:** Zig's `{d:0>10}` would silently print an 11-digit entry at that size, a corrupt xref.
It cannot happen below 10 GB, but the proof forced the case to be handled. All arithmetic is in
`u64` or proven in range, so it is also correct on 32-bit wasm.

**Evidence:** all **13/13 golden PDFs** are rebuilt byte for byte from their own content streams.

### 4.2 Stable merge sort (`sort.rs`)

API: `stable_sort_by(v: &mut [T], less: F)` with `T: Copy` and `F: Fn(&T, &T) -> bool`.

The algorithm is top-down merge sort with binary insertion sort on runs of at most 20. Each merge
copies the left run into a buffer and merges front to back, like `mergeLow` in `sort.zig`.

**Proven for any closure** (deterministic or not, consistent or not):

- no panic, no overflow;
- terminates;
- the output is a permutation of the input: there are g and its inverse with g a bijection on
  `[0, n)` and `out[k] == in[g[k]]`.

**Proven additionally when** the closure is deterministic and its relation is a strict weak ordering
(irreflexive, transitive, negatively transitive). Then for all k < l, `lt(out[k], out[l])`, or the
two are equivalent and `g[k] < g[l]`. That is sorted **and** stable.

This is the strongest spec here, since a stable sort's output is unique. It is the hardest proof too:
412 proof lines for 94 exec lines.

**Evidence:**

- **2448/2448** generated cases match the engine's `sort.stable` exactly (`sort_dump.zig`).
- With NaN keys (`<` on floats with NaN is not a strict weak order), **277/301 differ**.

**Port hazard:** the outputs only agree for well-behaved comparators. The Zig engine (raster fill)
sorts f64 crossings with `<`. A port that wants bit-identical output even with NaN needs Zig's exact
algorithm. The parallel port has an exact SymMerge copy. Verifying SymMerge with its rotations is
not done; it is the natural next proof.

### 4.3 JSON parser (`json.rs`)

API: `parse(&[u8]) -> Result<Value, ParseError>`. Strings are `Vec<u8>` proven valid UTF-8.
`ParseError` carries json.zig's line, column and message.

**Proven:**

- **Total:** no panic, overflow or out-of-bounds access on any byte input.
- **Terminates:** a lexicographic measure covers the `value` → `array`/`object` mutual recursion
  (remaining bytes, kind), and every loop has a `decreases`.
- **Nesting at most 200** (json.zig's limit): on success, `value_ok(v, 200)`.
- **UTF-8:** every string and key satisfies vstd's `valid_utf8` (RFC 3629: no overlongs, no
  surrogates, ≤ U+10FFFF).
- **The UTF-8 validator is exact:** `utf8_valid(s) == valid_utf8(s)`. Its per-width conditions are
  proven by bit-vector lemmas against vstd's decoding formulas.
- **Duplicate-key handling is memory-safe:** the sorted index array is in bounds because of the
  sort's permutation guarantee. This is a nice composition of two proofs.

**Not proven:**

- **Grammar:** the accepted language is not shown equal to a spec grammar. Instead it is checked
  against json.zig by fuzzing.
- **Duplicate-key semantics:** "last value wins in the first key's position" is not proven, only
  memory safety.
- **The input size cap is not inside `parse`:** `limits.max_json_bytes` (8 MiB) is checked by Zig's
  API layer, not by `parse`, and the same holds here.
- **Numbers are trusted:** `number_value` uses core's `f64::from_str` (section 5).

**Findings:**

- json.zig's line and column counters are `u32` and would trap on overflow beyond 4 GiB of input.
  Here they saturate, which is identical below that size.
- Its member index cast `@intCast(i)` to `u32` would trap beyond 2^32 members. Here it is `usize`.

**Evidence:** a **100,000-input differential fuzz** (mutated `spec/details/*.kerf.json` plus targeted
cases: escapes, surrogates, invalid UTF-8, nesting 199/200/201/5000, number edge cases, duplicate
keys). All outputs are identical to json.zig: 29,185 accepted with the same tree and f64 bits,
70,815 rejected with the same line, column and message.

### 4.4 PNG framing (`png.rs`)

**Proven:**

- **CRC-32 table:** `crc_table()[n]` equals 8 steps of the reflected division by 0xEDB88320, for all
  256 entries.
- **CRC-32 update:** the table-driven `crc_feed` equals the bitwise definition. Proven by GF(2)
  linearity of a step and a bit-vector lemma: `step⁸(c ^ b) == table[(c ^ b) & 0xff] ^ (c >> 8)`.
- **Adler-32:** `adler32_of` equals RFC 1950's closed form, A = 1 + ΣD and B = Σ of the A's (mod
  65521), and it defers the modulo to every 5552 bytes as zlib and Zig do. The NMAX bound
  `2b ≤ 131040(t+1) + 255·t(t+1) ≤ 2·(2³²−1)` is proven by nonlinear arithmetic.
- **zlib frame:** header `78 9C` with CM 8, CINFO 7, FDICT 0 and the header a multiple of 31, then
  the deflate stream, then the big-endian Adler-32.
- **Container:** `png_container` equals signature + IHDR + IDAT chunks of at most 64 KiB + IEND. Each
  chunk is BE length + tag + data + BE CRC-32 of tag and data (exact spec).
- **Filters:** every byte `filter_image` writes equals the spec of its filter (None, Sub, Up, Avg,
  Paeth), and the filter-type byte is at most 4. Dimension arithmetic is done in `u64`; the result is
  `None` if `(stride+1)·height` does not fit `usize`.

**Not proven:**

- that the chosen filter minimizes the score (the heuristic is checked end to end);
- the deflate compressor (Zig's `std.compress.flate`; the port has its own);
- the bit writer and Huffman code-length limits, which were on the list and **are not done**.

**Findings:**

- `png.zig` computes `stride * height` and `(stride + 1) * height` in `usize` before any check. On
  wasm32 (ReleaseSmall, no safety checks) that can wrap for dimensions near its 2^24 limit.
- Its only caller (`raster.render`) passes dimensions of at most 8192, so it is latent, not
  reachable.

**Evidence:** for all **26/26 golden raster PNGs**:

- the filter choice and filtered bytes reproduce the golden's inflated IDAT;
- `zlib_frame` rebuilds the golden zlib stream around its deflate data (header and Adler-32);
- `png_container` rebuilds the golden file byte for byte.

### 4.5 Raster kernels (`raster.rs`)

API: `Canvas::new(w, h)` (1..8192 each), `stroke::<M>(points, half_width, dash)`, `fill::<M>(edges)`,
`flush()`, `pixels()`. `M: FloatMath` supplies floor, ceil, sqrt and round, which core lacks on
stable `no_std`. The port's `math` module has all four.

**Proven:**

- **No out-of-bounds writes:** no write outside the pixel, coverage, accumulator or span buffers, for
  any `f64` inputs (NaN, ±inf, 1e308) and any `FloatMath` implementation.
- **Invariants maintained:** buffers are sized `w·h` and `w+2`, and every dirty span lies inside its
  row.
- **No overflow:** all integer arithmetic is overflow-free (`w·h ≤ 2^26`), and the blend
  `(d·(255−c)+127)/255 ≤ 255`.
- **Termination:** `capsule`, `fill` and `flush` terminate.

How this works without float reasoning: float→index conversions are saturating `as` casts followed
by integer clamps such as `min(…, h-1)`, `ia, ib ≤ w` and `if ix0 <= ix1`. These are no-ops whenever
the float logic behaves as Zig's does, so the proof needs no float facts.

**Not proven:** termination of `stroke`'s dash walker. It advances by float amounts; the function is
marked `exec_allows_no_decreases_clause`. It does terminate, because the style accepts only dashes of
at least 0.01 mm, but its iteration count is bounded only by paper length divided by dash length.

**Finding:** Verus rejected my first canvas invariant. It allowed a clean row (`rmax < 0`) with any
`rmin`, under which `mark` could create a negative span. The real invariant, implicit in raster.zig,
is `rmax < 0 ⇒ rmin == i32::MAX`. Verus also showed that the `acc[ib]` write with `ib == w` is in
bounds only because the accumulator has `w+2` entries.

**Evidence:** a scratch copy of the Zig engine (`check/trace_raster.py`) logs every canvas call of
`kerf export --format png` plus the final pixels. Replaying the logs through the verified `Canvas`
gives **identical pixels for 52/52 traces**: 13 views × {default, sheet, 200 px, 3000 px}, 57,831
strokes, 459 fills, 113 M pixels.

### 4.6 Overflow and robustness findings, collected

Everything Verus forced me to handle, in order of how real it is:

1. **raster:** the canvas span invariant was weaker than the code relied on (fixed in the spec, and
   documented).
2. **png:** `stride*height` wrap on wasm32 in png.zig (latent, unreachable).
3. **pdf:** 11-digit xref entries at 10 GB or more (Zig silently corrupts; here it returns `None`).
4. **png:** a capacity hint `z.len()/65536*12 + z.len() + 64` in my own first draft could overflow;
   removed.
5. **json:** `u32` line/column and member-index overflow beyond 4 GiB of input or 2^32 members.

None is reachable through Kerf's current limits. The value was that every one of them now has an
explicit, checked answer.

## 5. Trusted base

What the proofs rely on without proving it:

1. **Verus itself:**
   - its encoding to SMT and Z3 4.16.0 (built here);
   - rustc 1.99.1 as its front end;
   - vstd's axioms and `assume_specification`s for std: `Vec::push`/`pop`/`extend_from_slice`,
     slice indexing and range slicing, `mem::swap`, `Option::take`, closure call semantics, and the
     `axiom_vec_index_decreases` axiom used for recursion through `Vec`.
2. **vstd's definition of `valid_utf8`,** used as the UTF-8 spec. Using it is a feature: the spec is
   not mine.
3. **My `external_body` functions**, 70 lines by Verus's `line_count`:
   - `json::number_value` (6 lines): `str::from_utf8` + `parse::<f64>` + `abs() <= 9e15`. Correctly
     rounded parsing in core, like Zig's `parseFloat`.
   - raster's float helpers: `fmin`, `fmax`, `add`, `sub`, `mul`, `div`, `lt`, `le`, `abs`,
     `to_index` (saturating `as usize`), `to_u8`, `from_usize`, plus `acc_add` and `acc_byte` (the
     f32 accumulator). They claim nothing except `acc_add` preserving the vector length. The two
     helpers that index have `requires i < len` proven at every call site, and Rust still
     bounds-checks inside them.
4. **The cfg split:** that rustc with `verus_keep_ghost` off compiles the same exec code Verus
   verified with it on. It removes only gated items, attributes and statements.
5. **The caller:** for the sort, that `less` is a deterministic strict weak ordering *if* sortedness
   is wanted. The permutation guarantee needs nothing.
6. **Not modelled:** allocation failure (Verus assumes `Vec::push` succeeds) and stack depth. JSON
   recursion is bounded at 200 levels but stack size is not modelled. Floats are opaque everywhere.

## 6. Measurements

### 6.1 Verification time

Whole crate, `verify.sh`, default `--rlimit 10`:

- **wall:** 5.9–6.0 s across three consecutive runs; 4.2 s of that is verification, 1.3 s the rustc
  front end;
- **CPU:** about 13 s on 7 threads;
- **result:** 188 items, the same in every run.

Per module (summed SMT and AIR time across threads):

| module | time | heaviest function (Z3 resource units) |
|---|---:|---|
| png | 3.1 s | `png_container` 1.6M |
| json | 2.0 s | `dedupe` 0.5M |
| sort | 1.6 s | `sort_range` 7.9M (the heaviest in the crate) |
| pdf | 1.6 s | `lemma_entry_in_file` 2.9M |
| raster | 1.4 s | `Canvas::fill` 0.8M |
| dec | 0.2 s | |

**Margin:** everything verifies at `--rlimit 3`. At 1–2 only `sort_range` fails, so the default
leaves at least 3x headroom.

**Flakiness:** none across the three timed runs. One flaky proof occurred during development:
`lemma_tail` (PDF trailer) verified alone but hit the rlimit inside the whole-module run. The
surrounding context changes Z3's search. Making its 9-way sequence concatenation reasoning explicit
fixed it, and it was stable afterwards.

**One hang:** a whole-module run with three failing functions ran for more than 5 minutes without
output, while each function verified or failed in 1.5 s alone. It was never reproduced once the
failures were fixed. The workaround is `--verify-function`.

### 6.2 Proof-to-code ratio

Verus's `line_count`, on the `verus!`-syntax sources (commit `3f4d940`, same content as shipped):

| module | spec | proof | exec | trusted | (spec+proof)/exec |
|---|---:|---:|---:|---:|---:|
| dec | 64 | 41 | 15 | 0 | 7.0 |
| pdf | 239 | 282 | 88 | 0 | 5.9 |
| sort | 92 | 412 | 94 | 0 | 5.4 |
| png | 228 | 290 | 203 | 0 | 2.6 |
| json | 130 | 285 | 594 | 6 | 0.7 |
| raster | 63 | 147 | 366 | 64 | 0.6 |
| **total** | **816** | **1457** | **1360** | **70** | **1.7** |

The pattern: a full functional spec (output = a mathematical definition, as for pdf, sort and dec)
costs 5–7 lines per exec line. Safety-only specs (json, raster) cost under 1.

### 6.3 Speed

Best of 5 rounds, aarch64. Verified vs. an unverified baseline where verification shaped the code,
and vs. the Zig engine (`check/bench.zig`, same inputs, ReleaseFast).

| kernel | verified | baseline | ratio | Zig | ratio |
|---|---:|---:|---:|---:|---:|
| sort 1M (u32 key, idx) | 95.0 ms | std `sort_by` 24.1 ms | 3.94x | 126.5 ms | **0.75x** |
| sort 1000 | 19.4 µs | 10.1 µs | 1.93x | 32.5 µs | 0.60x |
| sort 16 | 157 ns | 67 ns | 2.36x | 140 ns | 1.12x |
| sort 1M f64 | 112.2 ms | 31.0 ms | 3.62x | 147.1 ms | 0.76x |
| JSON, largest detail (9 KB) | 48.8 µs | core UTF-8 check 52.7 µs | 0.93x | 22.3 µs | 2.19x |
| JSON, 64 details (559 KB) | 3.71 ms | 3.89 ms | 0.95x | 1.84 ms | 2.01x |
| CRC-32, 16 MiB | 45.3 ms | – | – | 45.4 ms | 1.00x |
| Adler-32, 16 MiB | 11.2 ms | deferred mod 9.3 ms | 1.20x | 6.9 ms | 1.61x |
| PDF file, 1 MiB stream | 35.7 µs | `core::fmt` 35.9 µs | 0.99x | – | |
| PNG filters, 1600×1237 gray | 18.8 ms | – | – | – | |
| raster replay, 52 traces, 113 M px | 0.56 s total | – | – | – | |

How to read it:

- **Sort:** faster than Zig's no-allocation SymMerge (it trades an n/2 buffer for simpler merges),
  but std's driftsort is 2–4x faster than both. Matching driftsort under proof would be a project of
  its own.
- **JSON:** the verified parser is as fast as its unverified twin. The 2x gap to Zig is
  representation: `perf` puts 53% of the time in libc (malloc, free, memcpy). Every string and array
  here is an owned `Vec`; json.zig uses an arena and aliases unescaped strings into the source.
  Borrowed spans would close most of it; this is not a verification cost.
- **Adler-32:** my first verified version (a subtraction per byte, an easy proof) was 2.8x slower
  than the deferred-modulo version. Proving NMAX took about 15 minutes and closed it to 1.2x. What
  remains is the bounds checks Verus proves but rustc keeps (the baseline iterates `chunks()`), plus
  Zig's 16-way unrolling.

### 6.4 Day-to-day feel

**Edit-verify cycle:**

| scope | time |
|---|---:|
| one function (`--verify-only-module M --verify-function f`) | ≈ 1.5 s |
| one module | ≈ 4.3 s |
| whole crate | ≈ 6 s |

There is no incremental cache between runs, but at this size it does not matter.

**What a failure looks like.** Real messages from injected bugs.

Off-by-one in the JSON depth check (`depth > MAX_DEPTH` instead of `>=`):

```
error: postcondition not satisfied
   --> src/json.rs:613:9
    |
613 |         result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ failed this postcondition
...
626 |         return Ok((Value::Array(items), p + 1));
    |         --------------------------------------- at this exit

error: invariant not satisfied before loop
   --> src/json.rs:631:13
    |
631 |             depth < MAX_DEPTH,
    |             ^^^^^^^^^^^^^^^^^
```

It points at exactly the exit that would return an empty array nested 201 deep.

An unstable merge (take the right element on ties):

```
error: assertion failed
   --> src/sort.rs:524:66
    |
524 |                     assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[a + i], a + i) by {
    |                                                                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
```

This says that the element just taken does not come before the left head in stable order. That is
precisely the stability violation, but you have to know your own invariant to read it.

The first attempt at that injection failed differently: a helper lemma's precondition no longer
held. That is typical. Failures surface where the proof's assumptions break, not where the bug is.

**Three things that cost the most time**, all about Z3's quantifier triggers:

1. A `#[trigger]` inside a conjunction (`g[k] == k && inv[k] == k`) silently left half the
   invariant unusable. Splitting it into two `forall`s fixed it.
2. A recursive spec with a `forall` inside (`value_ok` over `Vec<Value>`) could not be unfolded
   against hypotheses until the trigger moved from the recursive call to the element `xs@[i]`.
3. Large `seq![…]` byte literals blew the rlimit until they were made `#[verifier::opaque]`, with
   only their lengths revealed.

**Other friction:**

- `b"..."` literals have no known contents in Verus. Array literals `[b'x', …]` do.
- Loops forget every outer fact not restated in `invariant`.
- `--expand-errors` is the most useful debugging flag.

## 7. Honest limits

- **Floats:** nothing about floats is proven. Raster correctness ("same pixels as Zig") rests on the
  52-trace replay, not on proof. Only memory safety and termination are proven there.
- **Not done from the brief:**
  - deflate: bit-writer bounds and Huffman code lengths;
  - JSON: equality of the accepted language with a spec grammar;
  - dash-walker termination;
  - SymMerge, the sort Zig actually uses.
- **Verified code is not fastest code.** Bounds checks stay. Designs that are easy to specify (owned
  JSON values, a buffered merge) were chosen over the fastest ones.
- **Toolchain weight:** 13 minutes and 2.3 GB to build Verus on a fresh Debian 12 machine, plus Zig
  for Z3. Verus is a moving target, with weekly date-versioned releases and a pinned nightly-class
  toolchain. Pin it.
- **Ergonomics:** the shipped attribute syntax is noisier than `verus!` and rustfmt ignores the
  insides of the attributes.

## 8. Recommendation

**Use Verus for kernels with crisp, byte-level specs.** Formats and framing (PDF xref, PNG chunks,
CRC/Adler), parsers' safety and limits, index arithmetic, sorting.

- **The cost is modest:** about 1.7 lines of spec and proof per exec line overall.
- **The guarantee is strong:** "for every input", not "for the inputs we tried".
- **It found real ones:** latent overflow and invariant issues that tests never would.

**Ship option 3:** cfg-gated attribute syntax, one source, zero dependencies, no proc macros. Builds
cost the same as plain Rust (0.9 s clean), and the shipped file is the verified file.

**Keep Verus out of float-heavy geometry,** where it can prove only memory safety, and keep the Zig
differential checks (`check/run.sh`) as the behavioural oracle. They caught nothing here, which is
the point: proofs and end-to-end checks together, no unit tests.

**Integration with the port** (`engines/rust` on `rust-verus`; I only read it):

- **`kerf-verified` has no dependencies;** add it as a path dependency.
- **pdf:** `pdf::PdfWriter` replaces the file assembly in `pdf.rs`. Content stream formatting stays
  in the port.
- **png:** `png::{crc_table, crc_feed, adler32_of, zlib_frame, filter_image, png_container}` can
  replace the port's CRC, filter and container code. The port's own `deflate` stays.
- **raster:** implement `raster::FloatMath` with the port's `math::{floor, ceil, sqrt, round}` (four
  one-liners). `Canvas::{stroke, fill, flush}` then map 1:1 onto raster.zig's `strokeFlat`,
  `fillEdges` and `flush`.
- **json:** the port's `Value::String` is `String`. Converting the proven-UTF-8 `Vec<u8>` costs a
  re-check (`String::from_utf8`) or an `unsafe` that the proof justifies. Alternatively, adopt
  `Vec<u8>` or borrowed spans.
- **sort:** keep the port's exact SymMerge if NaN-identical output with Zig matters; use
  `sort::stable_sort_by` where comparators are known to be strict weak orderings (`T: Copy`). The
  next proof worth doing is SymMerge itself.

## 9. Files and commands

```
engines/rust/verus/setup.sh          toolchain (Phase A); logs per-step time and disk
engines/rust/verus/verify.sh         Verus on the crate, then plain stable builds (native, wasm32)
engines/rust/verus/check/run.sh      every end-to-end check against the Zig engine
engines/rust/verus/check/            kvcheck (Rust) and the Zig oracles: json_dump.zig, sort_dump.zig,
                                     png_inflate.zig, bench.zig, trace_raster.py
engines/rust/kerf-verified/src/      dec, pdf, sort, json, png, raster (verified and shipped)
kvcheck bench <zig bench> <dir> spec/details/*.kerf.json    the speed table
```

Commits on `rust-verus-proofs`:

| commit | what |
|---|---|
| `cd038aa` | setup, PDF and sort |
| `49b8b62` | erasure pipeline and e2e checks |
| `b357b32` | PNG |
| `b0eae19` | JSON |
| `c203a5b` | raster |
| `05aa317` | `run.sh` |
| `3f4d940` | Adler NMAX, last `verus!`-syntax state with `erase.py` |
| `ad4c434` | option 3 adopted |
| this report | the commit that adds VERUS.md |
