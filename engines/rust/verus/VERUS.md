# Rust + Verus on Kerf: what it costs, what it proves, how it feels

Verification track of the Rust port. Five kernels of the Zig engine were re-implemented in Rust and
proven with Verus. They ship as plain `no_std` Rust with **no dependencies and no proc macros**, and
are checked end to end against the Zig engine's real outputs. Sections 2–8 describe that work as a
standalone crate (Phases A–C). **Section 9 is the current state: the verified kernels replaced
kerf-core's own `sort`, `json`, `png`, `pdf/writer` and raster canvas, and ship in the CLI and wasm.**
Everything here was measured on this machine (aarch64, Debian 12, 12 cores) on 2026-10-28.

| | |
|---|---|
| Code | `kerf-core/src/{sort,json,png,pdf/writer,raster/kernels}.rs` (the verified sources *are* the shipped sources) |
| Verify | `engines/rust/verus/verify.sh`: 280 items, about 11 s wall, then plain stable builds of kerf-core for native and wasm32 |
| End-to-end checks | `engines/rust/verus/check/run.sh`: every check identical to Zig (about 3.5 min), plus the engine's own gates (9.4) |
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

**Integration with the port** (`engines/rust` on `rust-verus`; I only read it). This was the plan
before Phase D; section 9 is what was done, including a different answer for the sort:

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

## 9. Integrated: the verified kernels are the port's own modules (Phase D)

`rust-verus` (the zero-dependency port) was merged into `rust-verus-proofs` (`abbbb68`). Then five
of kerf-core's own modules were replaced by the verified kernels, adapted to the port's data
structures (`e6bea62`), and made at least as fast as the port where they were slower (`5866556`).
After the performance track was merged (`5bc76a5`), its kernel prototypes went into the verified
modules with their proofs (`08f0261`, section 9.8).
The standalone `kerf-verified` crate is gone: **the files kerf-core compiles are the files Verus
verifies.** `verus/verified.rs` is a verification root that includes them by `#[path]`, with `math`
and `num` along as unverified context.

### 9.1 What ships verified

| module | replaces in the port | proven |
|---|---|---|
| `sort.rs` | Go-style in-place SymMerge | **Zig's exact `sort.stable`**, generic over any `T`: always a permutation, never panics, terminates; sorted and stable when `less` is a strict weak ordering |
| `json.rs` (parser) | the port's parser | produces the port's own `Value`/`Member`/`ParseError`; total (no panic, overflow or non-termination on any input); on success every string and key is valid UTF-8 and nesting is at most 200 |
| `png.rs` | CRC, Adler, zlib frame, filters, container | every row's filter type is `best_filter` (lowest heuristic score, earliest on ties) and every filtered byte its `filter_byte`; `encode`'s output is exactly `png_file(...)`: signature, IHDR of (width, height, color), IDAT chunks with their CRCs, IEND, around a zlib header + DEFLATE body + Adler trailer; `InvalidDimensions` only for a zero side, a side above 2^24, or a filtered size that overflows `usize`. The body is the trusted `deflate_raw`, so `encode`'s spec does not tie the payload to the pixels; `filter_image`, CRC-32 and Adler-32 are proven separately as in 4.4 |
| `png/deflate.rs`, `match_len` only | the port's byte loop | the common prefix length of the two slices, at most 258, 8 bytes per step |
| `json.rs`, `write_fixed` only | the digit loop of `fmt_fixed` | never writes outside its 40-byte buffer, and writes only ASCII (so the `&str` needs no re-validation) |
| `pdf/writer.rs` | the port's object table and xref | `finish` returns exactly `pdf_file(body, offsets, root, info)`; `offsets_fit ==> xref_valid` |
| `raster/kernels.rs` | `Canvas` (from `raster.rs`) | no write outside any buffer for any float input; integer arithmetic overflow-free |

Writers and formatters in `json.rs` (`write_string`, `fmt_number`, ...) stay unverified in the same
file. `png/deflate.rs` stays the unverified exact port of Zig's `std.compress.flate`. Its `adler32`
and `zlib_compress` were removed; `deflate_raw` returns the bare stream and the verified
`zlib_frame` adds header and trailer. Verifying DEFLATE was not cheap enough for this phase.

**Adaptations to the port:**

- **pdf:** the port's API (`Writer::new`, `begin_object`, `put`, `push_usize`, `end_object`,
  `object`, `stream_object`, `finish(root, info) -> Vec<u8>`). `finish` is now total: entries are
  padded to *at least* 10 digits like Zig's `{d:0>10}`, so the 11-digit case of 4.1 behaves as in
  Zig, and the xref is proven valid whenever every offset is below 10^10. `dec.rs` was inlined.
- **json:** the parser builds `String`s. `utf8_string` is the one `unsafe`
  (`String::from_utf8_unchecked`), and its `requires valid_utf8(bytes@)` is proven at every call
  from the parser's validator. The UTF-8 conjunct of `value_ok` is now carried by `String` itself,
  so `value_ok` states depth only. The parser's `string` became `parse_string` because the port
  already exports `json::string`; `MAX_DEPTH` became `pub`.
- **png:** `ColorType` (`channels` with spec `channels_of`), `EncodeError` and `encode` are
  verified; `deflate_raw` is trusted.
- **raster:** `Canvas::new(w, h)` is total, clamping each side to 1..8192. The float helpers call
  `crate::math::{floor, ceil, sqrt, round}`. Clamping now follows Zig on NaN (the port's
  `f64::clamp` returns NaN for NaN; Zig's `std.math.clamp`, `@max(lo, @min(x, hi))`, returns a bound).

### 9.2 The sort: the Zig-exact algorithm is verified, so NaN may reach it

Of the two options (verify Zig's algorithm, or make sure NaN cannot reach a sort), the first was
done. `sort::stable` is `sort.zig`'s `stable` comparison for comparison:

- n ≤ 20: binary insertion in place (`insertion_swap`).
- n > 20: an index array is sorted: blocks of 20, bottom-up passes, each merge is an in-order check,
  `merge_low` (prefix skip) or `merge_high` (suffix skip, back to front) through Zig's 4 KiB buffer
  (`cap = 4096 / size_of::<T>()`), or SymMerge with rotations (`sym_split`, `sym_tail`,
  `reverse`). The permutation is then applied once (`apply_perm`).

Why it must be Zig-exact: the port sorts with comparators that are *not* strict weak orders. f64
`<` with NaN is one; thinland's tolerance comparator (`|Δscore| > 1e-6 ? ... : ...`) is another,
and is not transitive in general (equality within 1e-6 is not). For those, no spec of "the stable order" exists and only
the same comparison sequence gives Zig's bytes. The proof covers every comparator for safety and
permutation; sortedness and stability are proven under `well_behaved(less)`. That the comparison
sequence equals Zig's is checked end to end, not proven (Zig is not modelled):

- 2448/2448 generated strict-weak-order cases identical to `sort.zig`;
- **301/301 f64 cases with NaN identical**; the port's previous SymMerge matched only 24/301.

Generic over `T` because `section::Stroke` and thinland's `Sample` are not `Copy`. The comparator
is passed as `&dyn IdxLess` into the merge phase so it is compiled once, not per call site (the
generic first version cost +26.6 KB of wasm). `IdxLess` has spec methods, so it is declared twice:
in a gated `verus!` block, and as a plain trait for the shipping build.

### 9.3 Trusted base, as integrated

Section 5 still applies. Changes and additions:

- `json::utf8_string`: one `unsafe`, justified by a proven precondition; trusted that vstd's
  `valid_utf8` is the definition `String` requires.
- `json::owned`, `json::key_bytes`: `String::from(&str)` and `as_bytes()`, nothing assumed.
- `json::number_value`: unchanged.
- `json::str_of`: `from_utf8_unchecked`, the same pattern as `utf8_string`: `requires valid_utf8`,
  proven at its one call from `write_fixed`'s ASCII postcondition.
- `deflate::match_len` relies on vstd's `assume_specification` of `u64::trailing_zeros` (bit-level
  axioms that vstd proves from its own recursive definition, trusting only that the instruction
  computes that definition).
- `sort::swap`: `slice::swap`, with the obvious `ensures`.
- `sort::IdxLess`: that the plain trait and the gated `verus!` trait declare the same methods.
- `png::deflate_raw`: trusted, and `encode`'s spec says only "some byte sequence `d`" there.
- raster float helpers now delegate to `math`; `math` and `num` are unverified.
- `Canvas::stroke`'s dash walker has `exec_allows_no_decreases_clause`: termination is argued (dashes
  are at least 0.01 mm) but not proven.
- `impl Default for pdf::writer::Writer` is `external` (it calls the verified `new`).

### 9.4 Gates

All on the final engine (`08f0261`), aarch64:

| gate | result |
|---|---|
| `verus/verify.sh` | `280 verified, 0 errors` at the default `--rlimit 10`; native and wasm32 builds with `-D warnings` |
| rlimit margin | 280/280 at `--rlimit 6`; at 5 and below only `lemma_symmerge_compose` fails (16.6M units): about 2x headroom |
| `cargo clippy --release` (CLI and wasm32) | 0 warnings |
| `tests/check_golden.sh` | `golden: OK` |
| `wasm_golden.mjs` | `24/24 byte-identical` |
| `wasm_check.mjs` | `ABI OK: 0 imports`, `A.svg equals golden` |
| `cli_ergonomics.mjs` | `145 passed, 0 failed` |
| `diff_fuzz.py` | 2000 × seed 1: 1 difference; 1000 × seeds 2 and 3: 0 |
| `fuzz.py` (debug, overflow checks) | 1500 × 3: 0 crashes |
| `serve_smoke.mjs` | `215 passed, 0 failed` |
| `verus/check/run.sh` | pdf 13, png 26, raster 52, sort 2448 + NaN 301, json 100000: all identical to Zig |

The one `diff_fuzz` difference is pre-existing: the engine before this round (`5bc76a5`) gives the
byte-identical output on the same mutant, as did the unchanged port (`abbbb68`) in Phase D. It is the known number-format difference in an `E_PARAM` message (`99.9888`
against Zig's `99.98875`) from unverified number formatting.

### 9.5 Before and after (Phase D, `abbbb68` against `5866556`)

Same machine, before = `abbbb68`, after = `5866556`. A second agent was benchmarking on the machine
throughout (load average 3–4 on 12 cores), so engines were always alternated per case.

**Sizes** (bytes; `tools/size_report.sh`):

| artifact | before raw / gzip / brotli | after raw / gzip / brotli |
|---|---|---|
| CLI, release (stripped) | 1,677,064 / 874,184 / 686,529 | 1,659,048 / 863,918 / 680,735 (-1.1%) |
| CLI, `small` | 1,241,856 | 1,232,760 (-0.7%) |
| wasm (`small` + wasm-opt) | 604,675 / 265,006 / 222,335 | 596,066 / 262,957 / 221,662 (-1.4% raw) |

**Wasm, per function** (`verus/bench_wasm.mjs`, best of 3 rounds of 10 calls, in-process): total
over 39 cases **1482.4 → 1441.8 ms (0.973)**.

- `drawing`/`svg`/`dxf`/`pdf` of view B: 0.81–0.89 (likely the index-array sort: large elements
  such as `Stroke` move once instead of through rotations);
- `check` 0.88–0.97, `inspect` 0.86–0.90, `mesh` 0.90–0.96;
- view A exports 0.97–1.00 (0.966–0.997);
- PNG export 0.99–1.02.

**Native, per function** (`verus/bench_native.py`, includes process start): total
**940.4 → 945.7 ms (1.006)**. Individual cases scatter from 0.69 to 1.89 in both directions with no
pattern; under the background load this harness only shows "no change".

**Native, in process** (`sortbench`, a scratch harness linking both kerf-cores, release with LTO,
best of 7):

| case | before | after | ratio |
|---|---:|---:|---:|
| sort f64, n = 8 | 70 ns | 49 ns | 0.71 |
| sort f64, n = 20 | 223 ns | 182 ns | 0.82 |
| sort f64, n = 100 | 2.41 µs | 3.48 µs | **1.44** |
| sort f64, n = 1000 | 55.9 µs | 55.1 µs | 0.99 |
| sort f64, n = 10⁴ | 1.38 ms | 1.17 ms | 0.85 |
| sort f64, n = 10⁵ | 16.4 ms | 16.5 ms | 1.01 |
| sort 48-byte record, n = 20 | 275 ns | 409 ns | **1.49** |
| sort 48-byte record, n = 100 | 3.56 µs | 4.07 µs | **1.15** |
| sort 48-byte record, n = 1000 | 89.8 µs | 70.2 µs | 0.78 |
| sort 48-byte record, n = 10⁵ | 27.0 ms | 21.6 ms | 0.80 |
| JSON parse, the 3 reference docs | 128.5 µs | 127.5 µs | 0.99 |

The sort is not the same algorithm before and after, so this compares two algorithms, not a proof's
overhead. Two cases are slower, and both are left as they are:

- **n = 21 to a few hundred** pays for the index phase (four `usize` arrays: indices, merge buffer,
  and the `at`/`pos` maps of `apply_perm`) and a `dyn` call per comparison, which cannot be
  inlined. The `dyn` is what keeps the merge phase compiled once
  (26.6 KB of wasm), and Zig's own sort also calls the comparator through a function pointer.
- **Records with n ≤ 20:** the comparisons are Zig's (and were the port's), but the verified
  insertion moves the element down with `swap`s, three moves per step, where Zig and the port shift
  (`rotate_right(1)`, one move). Cheap for `f64` (0.71–0.82 above), visible for 48-byte records. A
  trusted `rotate_right` wrapper with a sequence `ensures`, like `swap`'s, would close it; it was
  not done because it adds to the trusted base for a gap that does not show end to end.

Neither shows in the per-function totals above, which do not get slower.

**Build times** (`CARGO_TARGET_DIR` fresh per side, two interleaved rounds; incremental = `touch
kerf-core/src/lib.rs` and rebuild):

| build | before clean | after clean | before incremental | after incremental |
|---|---:|---:|---:|---:|
| CLI release (LTO, 1 CGU) | 35.0 / 33.1 s | 32.8 / 33.5 s | 33.7 / 33.0 s | 32.2 / 32.6 s |
| CLI dev | 6.7 / 6.9 s | 6.6 / 7.0 s | 1.34 / 1.33 s | 1.26 / 1.31 s |
| wasm `small` | 12.2 / 12.5 s | 12.1 / 11.7 s | 12.0 / 11.8 s | 11.8 / 12.6 s |

No difference beyond noise. The gated ghost code is removed by `cfg` before type checking, so the
five modules growing from 1,556 to 7,157 lines, nearly all of it specs and proofs, costs the plain
build nothing measurable.

**Verification:** 244 items in 10.6 s wall (about 20 s CPU), against 188 items in 6 s for the
standalone crate.

Per module (`--time-expanded`; summed across 7 threads):

| module | time | heaviest function (Z3 resource units) |
|---|---:|---|
| sort | 6.6 s | `lemma_symmerge_compose` 16.6M, `merge_high` 9.3M |
| png | 4.2 s | `filter_image` 6.2M |
| json | 2.0 s | `parse_string` 0.6M |
| pdf/writer | 2.0 s | `push_trailer` 2.4M |
| raster/kernels | 1.5 s | `fill` 0.7M |

The rustc front end takes 2.1 s of the 10.9 s wall. **Margin:** everything verifies at `--rlimit 6`.
At 5 and below, `lemma_symmerge_compose` fails, so the default of 10 leaves about 2x headroom (it was
3x in Phase C: the Zig-exact sort's SymMerge composition is the heaviest proof in the repository).

### 9.6 Where the verified form was slower, and the faster proven form

The first integration (`e6bea62`) was 1.206x slower in wasm in total. The wasm profile is `small`
(opt-level z), which leaves per-element bounds and capacity checks inside indexed and `push` loops
where opt-level 3 hoists or vectorizes them. Each fix is proven:

- **PNG export, +26–46%:** `filter_row` built each filtered row with `Vec` indexing and per-byte
  `push`. It now writes into a `&mut [u8]` scratch slice, the output is assembled with `push` +
  `extend_from_slice`, and buffers are created with `vec![x; n]`. PNG is now within 1–3% of the port.
- **Drawing view B, +8–12%:** the sort's index phase used push loops and repeated indexing. It now
  fills preallocated slices and applies the permutation once through `at`/`pos` maps. View B is now
  11–19% faster than the port.
- **wasm +26.6 KB:** one `merge` per comparator type; fixed by `&dyn IdxLess` (9.2).

### 9.7 Merging `rust-verus` work

Edits stay inside `sort.rs`, `json.rs`, `png.rs`, `png/deflate.rs`, `pdf/writer.rs`,
`raster/kernels.rs`, `kerf-core/Cargo.toml` (a `check-cfg` lint for `verus_keep_ghost`) and
`engines/rust/verus/`, **with one exception: `raster.rs`.** The port's `raster.rs` had an inline
`pub mod kernels` (`composite`, `union_max`) next to its private `Canvas`. The verified `Canvas` now
lives in `raster/kernels.rs` (with `V`, `Edge`, `MAX_SIDE`), and `raster.rs` lost about 300 lines
and gained `pub mod kernels; use kernels::{Canvas, Edge, V};`. A later change to `Canvas` on
`rust-verus` would conflict there.

`rust-verus` was merged at `5bc76a5` (`f532ae1`); the only conflict was `kerf-core/Cargo.toml`, and
Verus stayed at 244/0. `math.rs` is compiled into the verification run as unverified context, so
`verify.sh` must be rerun after every merge.

### 9.8 Final round: the performance track's kernel prototypes, proven (`08f0261`)

The performance track (`engines/rust/PERF.md`) left the verified modules alone and wrote prototypes
for them (`perf/proto-*.patch`). PNG export was the one group where Rust trailed Zig fast (deflate and
filtering were 57% of native time). The prototypes were written against the pre-verification
modules. Here they went into the verified ones, with no spec weakened.

**PNG filtering** (`png.rs`, proven):

- `filter_score(kind, row, prev, bpp)` computes a filter type's heuristic score without writing the
  row. There is one pass per type, split where `i` reaches `bpp`, and it is proven equal to
  `row_score`.
- `filter_row` is specialised the same way for the one pass that writes the chosen row.
- **The spec got stronger.** `filter_image` now also proves that each row's filter byte is
  `best_filter`: the lowest score, the earliest type on ties. Before, it stated only `<= 4`; the
  heuristic itself was checked only end to end.
- **The first proven version was 5x slower per pass than the prototype.** The magnitude
  `if f < 128 { f as u64 } else { 256 - f as u64 }` selects in 64-bit lanes and does not vectorize.
  Computing it in `u8` (`0u8.wrapping_sub(f)`) and widening afterwards matches the prototype's
  `(v as i8).unsigned_abs()`, pass for pass. That was the whole difference.

**DEFLATE** (`png/deflate.rs`, still the unverified exact port of Zig's flate, except `match_len`):

- The hash tables are now `Box<[u16; N]>` with a masked hash, so their indexing has no bounds checks.
  They are allocated directly on the heap.
- `match_len` compares 8 bytes per step and finds the first difference with `trailing_zeros` of the
  XOR. **It is now proven:**
  - the result is the length of the common prefix, at most 258 and at most either slice length;
  - this covers the 8-byte loads (`le64` injectivity and the first set bit's byte, both
    `by (bit_vector)`) and the case of `prev` being shorter than `bytes`, where the old loop would
    have panicked.
  - Two intermediate proven forms lost speed:
    - Indexing `s[i + k]` eight times kept eight bounds checks, which blocked load folding. Slicing
      `&s[i..i + 8]` once folds them into one `ldr`.
    - Locating the difference by halving (4, 2, 1) instead of `trailing_zeros` cost 4%.

**`fmt_fixed`** (`json.rs`):

- It is monomorphized per decimal count, so `10^decimals` divides by a constant, as Zig's `comptime`.
  The float part stays unverified.
- The digits are written by the verified `write_fixed`. It is proven never to write outside the 40
  bytes and to write only ASCII.
- `str_of` (`from_utf8_unchecked`, `requires valid_utf8`, proven from the ASCII postcondition through
  `lemma_ascii_utf8`) therefore replaces `from_utf8(..).unwrap_or("0")`. The prototype's bare
  `unsafe` becomes the `utf8_string` pattern.
- **The writer's loops have guards that can never fire** (`digits > 0` when stripping zeros, `i > 1`
  for integer digits). They give the bounds proof without power-of-ten reasoning, and the output is
  unchanged for every input.

**Not taken:**

- **The non-generic sort core** (`proto-sort-core.patch`, -15.5 KB wasm against the port's 16
  monomorphized SymMerges). The verified sort already sorts an index permutation behind
  `&dyn IdxLess`, so the merge phase is compiled once (1.4 KB). What a non-generic core could still
  remove is the per-type small-slice insertion: dropping it measured -1.85 KB (586,130 -> 584,280
  bytes). That is not worth a heap index array for every sort of 20 or fewer elements.
- **The small-sort regression** stays: 20 records of 48 bytes take 409 ns against 275 ns for the
  port's sort (1.49x; 100 records 1.15x; f64 slices of 20 or fewer are 0.71–0.82x).
  - Zig and the port shift elements (`rotate_right`); the verified insertion swaps them.
  - Shifting a generic `T` needs `rotate_right` or `ptr::copy`, which vstd does not specify. That
    would mean a new trusted function.
  - The proven alternative, sorting indices and then applying the permutation, needs scratch arrays.
    Heap scratch costs more than it saves at n <= 20, and Verus has no spec for slicing a stack
    array mutably.
  - It does not show in any per-function number.
- **Adler-32** (4% of a native PNG export) keeps the Phase C loop.

**Effect on PNG export**, native, each step against `5bc76a5`, geomean of the 13 PNG cases
(pinned, best of the harness's medians):

| step | PNG time |
|---|---:|
| hash tables + masked hash + prototype `match_len` | 0.84 |
| the same with the proven `match_len` | 0.85 |
| + proven `filter_score`, first form (64-bit magnitude) | 0.79 |
| + proven `filter_score`, `u8` magnitude | 0.65 |
| + proven specialised `filter_row` | **0.60** |
| (the perf track's prototype applied to its own tree `f532ae1`, same harness) | 0.63 |

`fmt_fixed`: `mesh` 0.94, `pdf` 0.96, `sheet-svg` 0.96, all non-PNG cases 0.98, matching the
prototype.

**Final engine against Zig.** The perf track's harness (`bench/run_suite.sh`) was used, with every
engine run in one session pinned to cpu1 (load average 2.2–2.4). The ratio is the geomean over the
126 cases.

| | before this round (`5bc76a5`) | **final (`08f0261`)** |
|---|---:|---:|
| native in-process, Rust release / Zig fast | 0.80 | **0.75** |
| native, PNG group, release / Zig fast | 1.17 | **0.69** |
| native, Rust small / Zig small | not run | **0.67** (PNG 0.51) |
| wasm, Rust shipped / Zig small (shipped) | 0.77 | **0.74** |
| wasm, PNG group / Zig small | 1.09 | **0.77** |
| wasm, Rust shipped / Zig fast | 1.18 | **1.13** (PNG 1.23, was 1.75) |
| CLI processes, release / Zig fast | not run | 1.06 |

PNG rows, median ms:

| case | Zig fast | Zig small | Rust before | **Rust final** | Rust small final |
|---|---:|---:|---:|---:|---:|
| truss A png, native | 37.80 | 102.82 | 46.52 | **29.75** | 58.35 |
| truss B png, native | 48.09 | 141.51 | 56.97 | **34.05** | 70.00 |
| palmer A png, native | 48.06 | 141.00 | 57.61 | **34.31** | 73.17 |
| truss A png, wasm | 69.10 | 108.83 | 119.74 | **88.26** | |
| truss B png, wasm | 84.25 | 138.56 | 144.83 | **102.27** | |
| palmer A png, wasm | 91.03 | 142.32 | 156.98 | **110.81** | |

**Where Zig is still ahead:**

- natively, `fmt` (1.25), `web-mesh` (1.25) and `mesh` (1.01): JSON writing and allocation (see
  PERF.md);
- the wasm against Zig fast, a size-for-speed choice.

**Sizes** (bytes; raw / gzip / brotli):

| artifact | before this round (`5bc76a5`) | final |
|---|---:|---:|
| CLI release | 1,656,600 / 865,139 / 682,169 | 1,663,352 / 868,181 / 684,480 |
| CLI small | 1,229,432 / 644,797 / 529,983 | 1,233,192 / 646,678 / 531,360 |
| wasm | 584,657 / 262,054 / 221,302 | 588,538 / 263,487 / 222,092 |

Of the +3.9 KB of wasm, about 1.5 KB is PNG (ten specialised loops, two copies) and 2.4 KB is the
ten `fmt_fixed` instances. Zig's shipped wasm is 699,181 / 288,292 / 236,991.

**Verification:** 280 items, 10.5 s wall (21.9 s CPU).

| module | time |
|---|---:|
| sort | 6.1 s |
| png | 5.1 s |
| json | 2.4 s |
| pdf/writer | 1.7 s |
| raster/kernels | 1.4 s |
| png::deflate | 0.6 s |

Each new proof took one to three attempts. The rlimit margin is unchanged (9.4). The heaviest PNG
function, `filter_image`, now takes 4.9M units (6.2M before), under a third of
`lemma_symmerge_compose`.

## 10. Files and commands

```
engines/rust/verus/setup.sh          toolchain (Phase A); logs per-step time and disk
engines/rust/verus/verify.sh         Verus on verified.rs, then plain stable builds of kerf-core (native, wasm32)
engines/rust/verus/verified.rs       verification root: includes the five kerf-core files by #[path]
engines/rust/verus/check/run.sh      every end-to-end check against the Zig engine
engines/rust/verus/check/            kvcheck (Rust) and the Zig oracles: json_dump.zig, sort_dump.zig,
                                     png_inflate.zig, bench.zig, trace_raster.py
engines/rust/kerf-core/src/          sort.rs, json.rs, png.rs, pdf/writer.rs, raster/kernels.rs (verified and shipped)
engines/rust/verus/bench_wasm.mjs    per-function wasm timings, two engines alternated
engines/rust/verus/bench_native.py   per-function CLI timings, two binaries alternated
kvcheck bench <zig bench> <dir> spec/details/*.kerf.json    the speed table of 6.3 (now against kerf-core)
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
| `16f6d12` | VERUS.md (Phases A–C) |
| `abbbb68` | merge of `rust-verus` (the port) |
| `e6bea62` | verified kernels integrated into kerf-core; `kerf-verified` removed |
| `5866556` | faster proven forms where the integration was slower |
| `1f92334` | section 9 |
| `5bc76a5` | merge of `rust-verus` (performance track, `f532ae1`) |
| `08f0261` | the performance track's kernel prototypes as verified code |
| this commit | section 9.8 and PERF.md's final numbers |
