# engines/rust: performance track

Speed, size and build times of the Rust engine against the Zig reference, measured before and after this track.
Every number below was measured on this machine for this report; raw per-case results are in `perf/results/`
(TSV: label, fn, median us, min us, reps, output bytes, status). Outputs stayed byte-identical throughout (gates below).

**Summary** (geometric mean of per-case time ratios over the 126-case set; lower is faster)

| | before | after |
|---|---:|---:|
| native in-process, Rust release / Zig fast | 1.25 | **0.82** |
| native in-process, Rust small / Zig small | 1.03 | **0.69** |
| wasm (node 22), Rust shipped / Zig shipped (Rust opt-level z, Zig small, both `wasm-opt -Oz`) | 1.81 | **0.79** |
| wasm, Rust shipped / Zig fast | 2.78 | 1.21 (Rust `opt-level=s`: 0.86, `=3`: 0.65) |
| CLI processes, Rust release / Zig fast | 1.34 | 1.17 (statically linked: 1.01) |
| wasm size raw / gzip / brotli | 604,675 / 265,004 / 222,335 | **593,144 / 264,087 / 221,786** (Zig: 699,181 / 288,292 / 236,991) |
| optimized rebuild after an edit (release / new `fastbuild` profile) | 33.4 s | 33.4 s / **4.1 s** (Zig fast: 96.9 s) |

Where Zig is still ahead: PNG export (deflate and PNG filtering, reserved for the verified kernels; a prototype
closes it, see the last section), JSON-number-heavy outputs (`mesh`, `fmt`; also reserved code), allocation
(Zig allocates from a per-call arena, Rust from glibc malloc), process start-up (dynamic glibc + libgcc_s), and the
wasm built for speed (Zig fast is 1.21x faster than the size-optimised Rust wasm; Rust built for speed is faster).

## Method

- **Case set** (`bench/gen_cases.mjs`, 126 cases): the 3 reference details and the 2 test documents; per document
  `check`, `mesh`, `inspect`, `fmt`, `apply` (two update ops); per view (13 views) `drawing` and `export` svg, sheet
  svg, dxf, pdf (sheet), png; plus what the web app calls on every edit (`apply` set-doc, `mesh`, `drawing` per view)
  with the style object passed, as `apps/web` does.
- **Native in-process**: `bench/bench.rs` (cargo example of kerf-core, plain system allocator like the CLI) calls
  `kerf_core::api::call`; `bench/zig` builds the same loop over `engines/zig`'s `kerf.api.call` (its `init.gpa`, as
  the Zig CLI). Per case: 2 warm-up calls, then at least 10 calls and 200 ms; median and min reported.
- **Wasm**: `bench/bench_wasm.mjs` drives any engine's `kerf.wasm` through the SPEC 13.1 ABI in node 22; one pass over
  every case first so V8's tier-up is not charged to the first case; then as above.
- **CLI processes**: `bench/bench_cli.py` spawns `kerf` (posix_spawn, output to /dev/null) 30 times per command.
- **Machine**: CIX P1, 12 cores of three kinds (cpu0-1 2.6 GHz A720, cpu10-11 2.5 GHz, cpu6-9 2.2-2.3 GHz, cpu2-5
  1.8 GHz A520). Unpinned runs bounce between core types (2-4x spread), so every comparison here is pinned with
  `taskset -c 1`. A Verus agent was compiling and benchmarking on the same machine (load 2-3.5, one core busy);
  A/B comparisons were run back to back, the final tables in one session (`bench/run_suite.sh`, load 2.0-3.5);
  repeated runs of identical binaries agreed within 1% (geomean), single cases within about 3%.
- **Engines**: "before" = commit 0fc8753 (the finished port); "after" = this branch's head. Zig 0.17.0 from the same
  tree, `fast` and `small` (wasm: `zig build wasm -Dwasm-opt`, binaryen version_123, the same wasm-opt as Rust).
- **Profiles**: `perf record -g` with frame pointers on the `profiling` cargo profile; the cix perf has no libelf, so
  `bench/flame.py` symbolizes with binutils `addr2line -i` (inlined frames kept), folds stacks and writes SVG flame
  graphs. Wasm: `node --cpu-prof` on a build with the name section (`bench/wasm_names.sh`), folded by
  `bench/cpuprof.py`. Allocations: `bench --count` / `--arena` (instrumented allocator, `--cfg kerf_bench_alloc`),
  valgrind DHAT with DWARF 4 (`bench/dhat_top.py`). Size: twiggy, `nm --size-sort`. Tools added outside the project:
  rustup components `llvm-tools` (stable, for `llvm-profdata`) and `rustc-codegen-cranelift-preview` (nightly).
  No crates; `Cargo.lock` still lists only the three workspace packages.
- **Gates** after every committed change: `bench/gates.sh` (check_golden, wasm_golden 24/24, ABI check,
  cli_ergonomics 145/145, diff_fuzz 300). Final: fuzz.py 1000 (0 crashes), serve_smoke 215/215, clippy 0 warnings
  (CLI and wasm), diff_fuzz 1000 with seeds 2 and 3: 0 differences; seed 1: 1 difference that the baseline binary
  (0fc8753) produces byte for byte too: `z range must have z0 < z1 (got [99.9888, 11.25])` where Zig prints `99.98875`
  (its `{d}`; Rust's message uses the 4-decimal `fmt_num`), the deviation NOTES.md documents. Not touched here.

## Speed

### Native, in-process (median ms; geomean of ratios per function group vs Zig fast)

| group | Zig small | Rust before | **Rust after** | Rust small before | **Rust small after** |
|---|---:|---:|---:|---:|---:|
| check | 1.93 | 1.19 | **0.72** | 2.02 | 1.25 |
| mesh | 2.58 | 1.34 | **1.10** | 2.85 | 2.35 |
| inspect | 2.44 | 1.32 | **0.91** | 2.67 | 1.87 |
| fmt | 2.93 | 1.27 | 1.30 | 3.07 | 3.40 |
| apply | 1.94 | 1.20 | **0.74** | 2.04 | 1.29 |
| drawing | 1.98 | 1.23 | **0.78** | 2.14 | 1.35 |
| svg | 2.01 | 1.20 | **0.67** | 2.06 | 1.17 |
| sheet-svg | 2.03 | 1.19 | **0.70** | 2.04 | 1.21 |
| dxf | 1.95 | 1.22 | **0.62** | 2.10 | 1.12 |
| pdf | 2.02 | 1.16 | **0.68** | 2.03 | 1.24 |
| png | 2.81 | 1.32 | 1.26 | 2.15 | 2.10 |
| web-apply | 1.94 | 1.23 | **0.78** | 2.08 | 1.34 |
| web-mesh | 2.48 | 1.55 | 1.33 | 3.00 | 2.58 |
| web-drawing | 1.97 | 1.29 | **0.87** | 2.21 | 1.47 |
| **all 126** | 2.15 | 1.25 | **0.82** | 2.21 | **1.48** |

Selected cases, median ms (min within 1%):

| case | Zig fast | Zig small | Rust before | Rust after | Rust small after |
|---|---:|---:|---:|---:|---:|
| truss check | 12.45 | 29.02 | 15.18 | 10.09 | 17.87 |
| truss apply | 12.84 | 29.82 | 15.51 | 10.46 | 18.52 |
| truss mesh | 0.90 | 2.29 | 1.17 | 1.09 | 2.35 |
| truss inspect | 0.32 | 0.72 | 0.40 | 0.29 | 0.58 |
| truss A drawing | 4.14 | 7.42 | 4.77 | 4.39 | 7.68 |
| truss A svg | 4.89 | 9.14 | 5.51 | 4.87 | 8.43 |
| truss A pdf | 5.11 | 9.58 | 5.75 | 5.13 | 9.03 |
| truss A png | 38.23 | 102.99 | 50.99 | 49.75 | 81.91 |
| truss B svg (iso) | 8.79 | 22.73 | 11.05 | 6.05 | 10.85 |
| monopour B svg (iso) | 10.03 | 19.58 | 13.84 | 4.78 | 8.36 |
| palmer A svg | 2.67 | 4.91 | 3.10 | 2.42 | 3.97 |
| truss A web-drawing | 4.23 | 7.55 | 4.94 | 4.58 | 7.93 |
| truss web-mesh | 0.97 | 2.41 | 1.34 | 1.27 | 2.60 |

### Wasm in node 22 (geomean of ratios vs Zig small, the shipped Zig wasm)

| group | Zig fast | Rust before | **Rust after (shipped)** | Rust after, `wasm-opt -O3` | Rust `opt-level=s` | Rust `opt-level=3` |
|---|---:|---:|---:|---:|---:|---:|
| check | 0.71 | 2.17 | **0.68** | 0.61 | 0.53 | 0.37 |
| mesh | 0.63 | 1.50 | 1.16 | 1.09 | 0.74 | 0.59 |
| inspect | 0.56 | 1.46 | **0.93** | 0.88 | 0.55 | 0.45 |
| fmt | 0.44 | 1.17 | 1.18 | 1.12 | 0.66 | 0.46 |
| apply | 0.69 | 2.13 | **0.69** | 0.62 | 0.53 | 0.37 |
| drawing | 0.68 | 2.04 | **0.73** | 0.66 | 0.53 | 0.39 |
| svg | 0.67 | 1.94 | **0.67** | 0.62 | 0.50 | 0.38 |
| sheet-svg | 0.67 | 1.90 | **0.70** | 0.64 | 0.52 | 0.40 |
| dxf | 0.67 | 2.03 | **0.64** | 0.58 | 0.46 | 0.34 |
| pdf | 0.69 | 1.89 | **0.71** | 0.65 | 0.53 | 0.40 |
| png | 0.63 | 1.31 | 1.06 | 0.78 | 0.71 | 0.62 |
| web-apply | 0.69 | 2.13 | **0.71** | 0.64 | 0.54 | 0.38 |
| web-mesh | 0.62 | 1.59 | 1.29 | 1.22 | 0.79 | 0.64 |
| web-drawing | 0.68 | 2.06 | **0.78** | 0.71 | 0.56 | 0.41 |
| **all** | 0.65 | 1.81 | **0.79** | 0.71 | 0.56 | 0.42 |
| size raw / gzip (bytes) | 1,683,808 / 500,070 (no wasm-opt) | 604,675 / 265,004 | **593,144 / 264,087** | 606,395 / 266,111 | 705,098 / 307,781 | 1,213,581 / 431,249 |

Selected cases, median ms:

| case | Zig small | Zig fast | Rust before | Rust after |
|---|---:|---:|---:|---:|
| truss check | 30.10 | 22.97 | 58.43 | 21.29 |
| truss A svg | 13.74 | 9.19 | 38.96 | 11.05 |
| truss A pdf | 14.01 | 9.54 | 39.36 | 11.47 |
| truss A png | 108.59 | 69.00 | 165.46 | 116.78 |
| truss B svg (iso) | 18.59 | 13.11 | 21.93 | 11.53 |
| monopour B svg (iso) | 21.15 | 15.99 | 28.34 | 8.89 |
| palmer A svg | 6.44 | 4.37 | 18.92 | 5.06 |
| truss A web-drawing | 12.54 | 8.45 | 37.74 | 10.29 |
| truss mesh | 2.11 | 1.45 | 3.25 | 2.82 |

The "42 ms vs Zig 29 ms per export" of NOTES.md is now 11.1 ms (truss A svg) against 13.7 (Zig small) and 9.2 (Zig
fast). Zig ships its small wasm; so does Rust. Building the Rust wasm at `opt-level = "s"` would beat Zig fast
(0.86x) at Zig small's raw size (705 KB vs 699 KB; gzip 308 KB vs 288 KB); kept at `z` (smallest, already 0.79x Zig
small), the menu above is for the owner.

### CLI processes (median ms, pinned; geomean vs Zig fast)

| command | Zig fast | Zig small | Rust before | Rust after | Rust small after | Rust after, static glibc |
|---|---:|---:|---:|---:|---:|---:|
| `version` (start-up) | 0.55 | 0.64 | 0.92 | 0.90 | 0.91 | 0.51 |
| `check` truss | 17.48 | 44.05 | 23.10 | 15.54 | 27.02 | 14.37 |
| `mesh` truss | 2.08 | 3.52 | 3.26 | 2.86 | 4.05 | 2.41 |
| `export` truss A svg | 6.66 | 12.83 | 8.24 | 7.86 | 12.16 | 7.39 |
| `export` truss A png | 56.30 | 132.57 | 72.00 | 72.10 | 114.26 | 74.76 |
| `export` truss B svg | 11.50 | 36.51 | 16.83 | 10.06 | 15.74 | 8.00 |
| `export` palmer A svg | 3.64 | 7.45 | 4.68 | 3.94 | 6.44 | 3.42 |
| **all 17 commands** | 1.00 | 1.95 | 1.34 | **1.17** | 1.78 | **1.01** |

A process pays start-up (dynamic loading of libc and libgcc_s: 0.60 ms task-clock against 0.25 static and 0.30 for
the static Zig binary), reading/writing files and first-touch page faults, so small commands gain less than the
in-process numbers suggest. `-C target-feature=+crt-static` closes the gap to Zig fast (1.01x) but costs +765 KB
(release 2.44 MB); not the default (see "Size"). Non-PIE (`-C relocation-model=static`) start-up 0.54 ms, -53 KB.

## Profile findings (before -> after)

Flame graphs of the final state: `perf/flame-native.svg` (all 126 cases x3), `perf/flame-wasm.svg` (x2).

Baseline, where the time went:

| profile | top items (inclusive unless noted) |
|---|---|
| wasm, all non-PNG cases | `math::sqrt` **39.7%** (software sqrt: `__multi3` 22% self, `__floattidf` 2%); `iso::candidates` 15.7% (of which `sort::sym_merge` + `ptr_rotate` ~19% of all time across callers); `font::Font::parse` 5.6%; `route` 34% (with sqrt) |
| wasm, `inspect` | `style::load` **27%** (the 12 KB default style parsed per call; `json::parse` inside 16.7%) |
| native, all cases | PNG: `filter_row` 17.4% self, deflate `add_hash` 10.6%, `match_len` 7.2%, `match_and_add_hash` 4.7%; glibc 9.7%; `sort::sym_merge` 6.1% |
| native, non-PNG | `iso::candidates` -> `sort::stable` **28.5%**; glibc malloc/free/memcpy 19.8% self; `Font::parse` 9.7% (13.5% once iso was fixed); `geom::orient2d` 7%; `route` (note leaders) 14% |

After: the native whole-workload profile is PNG encoding (`filter_row` 20.4% self, deflate 30.9% inclusive) plus
`geom::orient2d` 4.5%, routing (`seg_seg_dist` 2.6%, `solve_column` 2.4%), `raster::Canvas::capsule` 2.2%,
`json::fmt_fixed` 1.5%. Wasm: deflate `add_hash` 12.5%, `filter_row` 11.6%, `match_and_add_hash` 9.6%, non-inlined
`Vec::index` 7.1% (deflate's `Vec<u16>` hash tables at opt-level z), `route::solve_column` 4.9%.

Allocation (bench `--count`, truss A svg): 8,898 allocations + 2,281 reallocs per call before, 8,023 + 2,209 after
(font and parameter-row churn gone). Sites (DHAT): `clip::boolean` (~20 short-lived vectors per call, 97 calls),
`json::Parser::string` (reserved), per-stroke glyph polylines, `Prism::clone`. Replacing glibc with a per-call bump
arena (measurement only, `bench --arena`) makes the native in-process run **0.77x** (mesh 0.69, inspect 0.63, fmt
0.64, svg 0.78, png 0.95): that is most of the remaining native gap on the JSON-heavy functions.

Size (wasm, twiggy on the `-Oz` build with names; inlined code counts toward its caller): data segments 117 KB
(messages, catalog and schema text, the style, dec2flt tables), `builders::build` 55 KB (all 14 builders inlined, as
in Zig), `annot` 47 KB, `api` 38 KB (`dispatch` 36 KB), `section` 22 KB, `drawview` 22 KB, `iso` 20 KB, core
float formatting 13.8 KB (flt2dec + `fmt::float`: `model::num_text` `{:e}` and one `{}` in `drawview::fixed`), core
float parsing 5.3 KB (dec2flt: `json` and `units`), `sort` 11.5 KB (16 monomorphized copies), `json` 10.8 KB, `png`
(with deflate) 8.1 KB. Twiggy ran before the style minify commit, so data was 5.5 KB smaller after it.
Native CLI (`small`, `nm`): `core` 118 KB, `std` 102 KB, backtrace symbolization pulled in by std's panic hook
(`gimli` 28 KB, `rustc_demangle` 20 KB, `addr2line` 10 KB, `miniz_oxide` 9 KB), `.eh_frame` 116 KB, `.rela.dyn` 71 KB.

## Optimizations kept (each its own commit, gates green)

| commit | change | measured effect |
|---|---|---|
| c22f2e1 | wasm `math::sqrt` = the `f64.sqrt` instruction. `core::arch::wasm32::f64_sqrt` is unstable on 1.96, but compiler-builtins defines the libm symbol `sqrt` on wasm32-unknown-unknown and LLVM lowers a call to it to `f64.sqrt` inline (checked in the disassembly: 62 `f64.sqrt`, 0 imports). NaN/negative keep the canonical NaN of the soft path (wasm NaN payloads are not deterministic). IEEE sqrt is correctly rounded, so bits equal the verified soft sqrt | wasm geomean 1.78x -> 1.17x Zig small; truss A drawing 38.4 -> 12.3 ms |
| af96051 | `iso::candidates`: union of the grid cell lists through a scratch bitmap over face indices instead of concatenate + stable sort + dedup (same ascending unique list) | truss B drawing native 11.1 -> 6.5 ms, wasm 20.8 -> 13.1; monopour B native 14.0 -> 5.3, wasm 27.4 -> 9.7 |
| 0afa4bf | stroke font compiled by `build.rs` into static glyph tables (coordinates as `i8`, exact; the build fails if a future font has non-integers). The build script runs the engine's own `json.rs` (`#[path]` include) over the spec font, so numbers parse identically; the degree octagon is still computed with the same expressions where drawn | native geomean 1.23x -> 0.84x Zig fast (with af96051); palmer A svg wasm 6.37 -> 5.46 ms; wasm -7.8 KB raw |
| 540bbfb | default style tree parsed once per process (`AtomicPtr`, race-safe publish, never freed) and the user style merged into it, instead of parsing 12 KB per call | wasm inspect 0.78x, mesh 0.89x; native inspect 0.81x; all 0.97x |
| 399db92 | `params!` also generates `has_key`; `catalog::allowed_key` no longer builds every parameter row (names `Vec`, default `String`) of the type per document key | native all 0.97x, inspect 0.90x, fmt 0.92x; truss inspect 3,590 -> 2,715 allocations; wasm +1.8 KB |
| 40aced3 | embedded style minified at compile time by a `const fn` (Zig does it with comptime `json.minify`) | wasm 598,633 -> 593,144 raw |
| ca0f877 | `fastbuild` profile: release minus LTO, 16 codegen units, incremental | optimized rebuild after an edit 33.4 -> 4.1 s; runs 1.02x release |

## Tried, measured, not kept

| what | result | why not |
|---|---|---|
| stroke text streamed through a callback (no `Vec` per glyph stroke) | -14% allocations in svg; time 0.99-1.00 native and wasm; wasm +2 KB | no measurable gain |
| per-call bump arena in the wasm allocator (with escape detection: rewind only if every block of the call was freed, else retire its chunks) | 0.99x | the size-class heap was already cheap in wasm; 180 lines of unsafe allocator for 1% |
| CLI one-shot bump allocator (never free, 256 MiB region, `serve` excluded) | in-process the arena bound is 0.77x, but per process 0.97x (fresh pages fault in) and +176 KB binary (allocator inlined at every site under LTO) | not worth the size; the in-process gain would need a thread-safe arena for `serve` |
| PGO (`-Cprofile-generate/use`, trained on 3 documents, evaluated on all 5) | 1.01x overall (check 1.09x slower, png 0.93x); held-out documents 1.03x; +29 s instrumented build | no gain |
| routing: distances minimized as squares, one `sqrt` per polyline pair (bit-identical since sqrt is monotone and correctly rounded) | 0.99x native and wasm | noise-level |
| hardware rounding (FRINTZ/M/P/A on aarch64, wasm `f64.trunc/floor/ceil` via libm symbols); verified bit-identical to the bit-level versions on 1.2e9 inputs natively and 1.8e8 in wasm (each through all four functions) | 0.99x native, 1.00x wasm | adds 7 unsafe blocks for ~1% |
| thin LTO + 16 codegen units for `release` | build 33 -> 11.5 s; runtime 1.01x (fmt 1.06x); +178 KB | slower and bigger for the shipped build; the `fastbuild` profile gives the build win without touching release |
| musl target (static) | start-up 0.39 ms (vs 0.90) but in-process slower (musl malloc): processes 1.04x | slower overall |
| static glibc (`+crt-static`) | processes 1.01x Zig fast (0.86x of dynamic Rust) | +765 KB (release 2.44 MB, small 1.98 MB) |
| `-C force-unwind-tables=no` | no size change (std is prebuilt with tables) | |
| wasm-opt inlining thresholds (`--always-inline-max-function-size 12` / `24` + one-caller 200) | 0.91x / 0.85x time at +14.5 KB / +72 KB | size/speed points on the same curve as `-O3` / `opt-level=s` |
| `-Zthreads=8` (nightly) | dev clean 7.5 -> 5.7 s, incremental 1.4 -> 2.2 s | slower where it matters |
| cranelift backend for dev (nightly) | clean 7.5 -> 6.2 s, incremental 1.44 -> 1.34 s | LLVM dev builds are already 1.4 s |

## Size

| file | raw | gzip -9 | brotli -11 |
|---|---:|---:|---:|
| Zig `kerf.wasm` (small + wasm-opt -Oz) | 699,181 | 288,292 | 236,991 |
| Rust `kerf.wasm` before | 604,675 | 265,004 | 222,335 |
| **Rust `kerf.wasm` after** | **593,144** | **264,087** | **221,786** |
| Rust wasm, nightly `-Zbuild-std` (core/alloc at opt-level z) | 579,617 | 258,727 | 219,328 |
| Rust wasm, nightly build-std + `-Cpanic=immediate-abort` | 573,746 | 258,022 | 218,351 |
| Zig CLI small | 1,260,792 | 702,023 | 581,788 |
| Zig CLI fast | 2,637,456 | 1,201,143 | 921,502 |
| Rust CLI release before / after | 1,677,064 / 1,672,952 | 874,182 / 874,789 | 686,529 / 687,525 |
| Rust CLI small before / after | 1,241,856 / 1,237,760 | 648,876 / 648,955 | 531,592 / 532,083 |
| Rust CLI small, non-PIE | 1,184,432 | 637,561 | 523,274 |
| Rust CLI small, nightly build-std | 1,163,480 | 609,639 | 502,668 |
| Rust CLI small, nightly build-std + immediate-abort | 919,888 | 491,447 | 407,063 |

The Zig CLI contains an outbound TLS client the Rust one lacks. Size moved little on stable: the wins were -7.8 KB
(font tables replace the font JSON), -5.5 KB (minified style), +1.8 KB (`has_key`). The rest is spoken for: the
largest functions are the builders, annotation and the dispatch, as in Zig; float formatting (13.8 KB) serves two
message paths that need shortest round-trip digits (`{:e}` in `model::num_text`, `{}` in `drawview::fixed`),
replacing it means writing a shortest-digits algorithm; on stable the CLI cannot drop std's backtrace symbolizer
(~70 KB, gone with nightly immediate-abort) or its unwind tables.

## Build times

Clean = fresh target or cache directory (toolchain std and Zig's global cache warm); incremental = one constant in
`geom` changed and changed back, mean of the two rebuilds. Pinned nowhere (builds use all cores); `bench/build_times.sh`.

| build | clean | incremental |
|---|---:|---:|
| Rust `cargo check` | 4.2 s | 0.8 s |
| Rust dev CLI | 7.4 s | 1.4 s |
| **Rust `fastbuild` CLI (new)** | 10.6 s | **4.1 s** |
| Rust release CLI (fat LTO, 1 cgu) | 33.2 s | 33.4 s |
| Rust small CLI | 21.4 s | 21.0 s |
| Rust wasm (cargo) + prune + wasm-opt | 12.7 + 1.3 s | 12.1 + 1.3 s |
| Zig debug CLI | 32.8 s | 22.5 s |
| Zig fast CLI | 104.6 s | 96.9 s |
| Zig small CLI | 54.0 s | 46.2 s |
| Zig wasm (small + wasm-opt) | 31.3 s | 21.1 s |

The edit-compile-test loop (edit `geom`, rebuild, run the golden gate, 130 CLI runs + the Python DXF/PDF checkers):
Rust dev 1.4 + 31.1 s = 32.5 s; Rust release 33.4 + 10.9 = 44 s; **Rust fastbuild 4.1 + 10.7 = 15 s**; Zig debug
22.5 + 30.5 = 53 s; Zig fast 96.9 + 11.1 = 108 s. Levers measured for the dev profile (clean / incremental): rust-lld
7.1 / 1.37 s, `debug = "line-tables-only"` 6.8 / 1.35 s, `debug = 0` 6.2 / 1.29 s, both lld and line tables 6.5 /
1.33 s: all within 0.1 s of the default, so the dev profile is unchanged. Release with rust-lld: 33.0 / 32.5 s (LTO
dominates). The crate graph was not split: a `geom` edit invalidates nearly every module that would be split off,
and the 1.4 s incremental dev build leaves little to win.

## For the verified kernels

The modules reserved for the Verus versions (`json.rs`, `sort.rs`, `png.rs` + `png/deflate.rs`, `pdf/writer.rs`,
`raster::kernels`) were profiled but not edited. Their share of the final profiles (`bench/kernel_share.py`; a
sample is charged to the innermost reserved frame; inlined helpers are matched by name, so approximate):

| module | native, all cases | native, non-PNG cases | wasm, all cases | wasm size (twiggy, Oz) |
|---|---:|---:|---:|---:|
| deflate | 30.9% | - | 34.9% | 8.1 KB for `png` + `deflate` |
| png filters, CRC, chunks | 26.6% | - | 11.7% | (same) |
| json writers / number formatting | 1.8% | 4.7% | 2.3% | 10.8 KB `json` total |
| json parser | 1.0% | 2.6% | 1.4% | + core dec2flt 5 KB code + tables |
| sort | 0.4% | 1.2% | 0.6% | 11.5 KB (16 instances) |
| raster kernels | 1.3% | - | inlined | inlined |
| pdf writer | ~0% | ~0% | ~0% | 0.25 KB |

PNG export is the one function group where Rust trails Zig fast (1.26x natively, 1.06x Zig small in wasm), and it
is all in these files. Prototypes, measured in a scratch worktree and checked byte-identical against every golden
(PNG included), are in `perf/` as patches against this branch's files (not applied):

- `perf/proto-png-deflate.patch`: (1) `match_len` compares 8 bytes per step (`u64` XOR + `trailing_zeros`):
  PNG 0.88x; (2) the filter heuristic scores each of the 5 filter types in one specialised pass without writing the
  candidate row (`filter_row` + `row_score` were 37% of a PNG export; the chosen row is filtered once): cumulative
  0.59x; (3) the deflate hash tables as `Box<[u16; N]>` with a masked hash, so the indexing has no bounds checks:
  cumulative **0.57x native (estimated 0.72-0.75x Zig fast), 0.78x wasm** (in wasm the non-inlined `Vec::index` on those
  tables is 7% of all samples).
- `perf/proto-fmt-fixed.patch`: `fmt_fixed` monomorphized on the decimal count (Zig's is `comptime decimals`, so its
  divisions by `10^d` are multiplications) and no `from_utf8` re-validation of the ASCII it just wrote (5% of a native
  `mesh` alone): mesh 0.94x, pdf 0.95x, sheet svg 0.95x, all non-PNG 0.98x; +3 KB wasm. `fmt_fixed` is 24% of a
  native `mesh` and the main reason `mesh` and `fmt` stay above Zig. A verified version can prove the output is ASCII
  and drop the check without `unsafe`-at-a-distance.
- `perf/proto-sort-core.patch`: `stable` as a thin generic wrapper over one non-generic sort of a `u32` index
  permutation (`&mut dyn FnMut`), then the permutation applied in place: wasm -15.5 KB raw (-2.6%), -3.8 KB gzip;
  time +1% wasm, +3% native. Size is the reason to consider it; the in-place SymMerge (rotations) is otherwise fine at
  today's sizes once `iso::candidates` stopped sorting.
- JSON parse: 2.6% of non-PNG native time (documents are ~10 KB); float parsing goes through core's dec2flt (correct
  rounding), which a verified parser has to match.
- `pdf/writer.rs` and `raster::kernels` are not worth optimizing (no measurable share).

## The remaining gap to Zig, and why

- **PNG** (native 1.26x Zig fast; wasm 1.06x Zig small): deflate and the filter heuristic, reserved; the prototypes
  above take native PNG to about 0.75x Zig fast.
- **`mesh`, `fmt`, `web-mesh`** (1.10-1.33x native): JSON number formatting and JSON writing (reserved, prototype
  above) plus allocation churn. Zig allocates from a per-call arena; Rust from glibc. The measured bound for an arena
  is 0.77x overall (0.64-0.69x on these functions), but a safe one for the threaded `serve` is real allocator work;
  not done.
- **Process start-up** (0.90 ms vs 0.55 ms): dynamic glibc and libgcc_s (std links libgcc_s for backtraces even with
  `panic = "abort"`). Static linking reaches parity with Zig fast but adds 765 KB.
- **Wasm vs Zig fast** (1.21x): a size-for-speed choice. Rust is shipped at `opt-level = "z"`, where std's small
  generic helpers (`Vec::index`, `Vec::push`) stay out of line; at `opt-level = "s"` Rust is 0.86x Zig fast with Zig
  small's raw size; at `3`, 0.65x and 1.21 MB (Zig fast 1.68 MB).
- Everything else (check, apply, drawing, svg, dxf, pdf, iso views) is faster than Zig fast natively (0.62-0.91x) and
  faster than Zig small in wasm (0.64-0.93x).

## Reproduce

```sh
cd engines/rust
node bench/gen_cases.mjs                                   # target/bench-cases (126 cases)
cargo build --release --example bench                      # native harness (plain allocator)
(cd bench/zig && zig build -Doptimize=fast --prefix /tmp/zb-fast)   # Zig twin (also small)
bench/run_suite.sh /tmp/res 1 n-rust=target/release/examples/bench n-zig=/tmp/zb-fast/bin/kerf-bench \
    w-rust=dist/kerf.wasm w-zig=../zig/dist/kerf.wasm c-rust=target/release/kerf c-zig=../zig/zig-out/bin/kerf
python3 bench/compare.py zig=/tmp/res/n-zig.tsv rust=/tmp/res/n-rust.tsv --base zig [--markdown]
bench/prof_native.sh /tmp/x.folded --loop 3 [--filter S] [--exclude png]; python3 bench/flame.py top|svg /tmp/x.folded
bench/wasm_names.sh /tmp/n.wasm; node --cpu-prof bench/wasm_loop.mjs /tmp/n.wasm target/bench-cases 2; python3 bench/cpuprof.py CPU*.cpuprofile
RUSTFLAGS="--cfg kerf_bench_alloc" cargo build --release --example bench   # then: bench ... --count | --arena
bench/build_times.sh /tmp/bt [rust|zig]; bench/build_levers.sh /tmp/bl
bench/gates.sh [fuzz N] [--serve]
```
