# Kerf engine (Zig) - quality review

Read-only review of `engines/zig` at commit **db3ace7** (Zig 0.16.0, ~22.4k lines in `src/`, 129 tests, 5 reference
documents with byte-checked goldens). Line numbers refer to that snapshot; the tree has moved since (new `coverage.zig`,
lenient-ops work in `api.zig`/`ops.zig`/`serve.zig`/`main.zig`), so use `git show db3ace7:engines/zig/src/<file>` when a
line does not match.

Method: the whole engine was read (core library, layout, exporters, server stack by four reviewers), then claims were
**checked by running things** in private copies of the tree: Debug and ReleaseFast/ReleaseSmall/ReleaseSafe builds of the
CLI, a ReleaseSmall/ReleaseSafe wasm, a live `kerf serve` probed with raw sockets, a numeric-extremes fuzz pass, timers
inside the layout, twiggy attribution of the wasm, and two Zig prototypes (appendix A). Findings marked *reproduced* were
observed, not inferred. Nothing under `engines/zig` was edited except adding this file.

## 0. Summary

The engine is in good shape for something written in two days by several agents in parallel: the architecture is sound
(arena per call, a single Drawing IR feeding four exporters, diagnostics as data, exact geometry predicates, no hash maps, stable
sorts), the tests and goldens catch regressions, and the code is mostly plain, readable Zig 0.16. What it lacks is
**hardening against hostile input and scale** (every number is trusted; every loop is unbounded), and **a second pass
of consolidation** (a hand-rolled parameter layer with 211 `.?` unwraps, stringly-typed dispatch, domain names hard-coded in
geometry code, 13 leftovers, duplicated helpers, four files of 1.2-1.8k lines).

Headline results (all reproduced unless noted):

| # | Finding | Evidence | Status |
|---|---|---|---|
| 1 | Document/style/op numbers reach `@intFromFloat` unchecked: panic in safe builds (kills `kerf serve`), UB in the shipped ReleaseSmall binaries and the wasm (SAF-1) | `edition: 1e30`, `gauge: 1e20`, `wrap_chars: -5`, bulge `1e-14`, `slope: "nan"`, 19-digit length, scale `1e300:1`: nine distinct sites panic (Debug) or hang/OOM/garbage (ReleaseSmall), appendix B | done (SAF-1: `num.zig`, checked at every site, hostile table test) |
| 2 | One unauthenticated chunked request kills the server (V-1) | `http.zig:142` integer overflow before auth | done by the server agent (V-1) |
| 3 | Layout time is unbounded in the number of annotations (LAY-1) | 100 notes: 15 s, 200 notes: 187 s | done (LAY-1: shared work budget + lazy fixes + `limits`; 130 annotations 14.5 s -> 1.8 s, 260 rejected with E_LIMIT) |
| 4 | Degenerate style values hang the engine (LAY-2) | `wrap_chars: 0`, `text.height_in: 0` never return | done (LAY-2: style validation, `wrap(0)`, zero-step guards) |
| 5 | Release builds ship with safety off, the wasm cannot even be *built* with safety on, and CI runs neither goldens nor fuzzing (SAF-2, TST-2) | `zig build wasm -Dwasm-optimize=ReleaseSafe` fails in `simple_panic`; `release.yml` builds `ReleaseSmall` | partly: `no_panic` + ReleaseSafe wasm gate + CI steps done; release.yml ReleaseSafe and CI gates done by the server agent |
| 6 | `json.zig`: quadratic duplicate keys, `1e999` -> `inf` -> printed as `0`, invalid UTF-8 round-trips (SAF-3) | 1.3 MB body = 16 s CPU; `kerf fmt` rewrites `1e999` to `0` | done (SAF-3: sort-based duplicate index, finite and <= 9e15 numbers, UTF-8, leading zeros, no `std.fmt` in `fmtNumber`) |
| 7 | Opening a folder executes commands from `.kerf/agents.json`; one-agent-at-a-time is a race; stop does not stop (V-2, V-3) | `touch /tmp/pwned` executed at startup; 5 of 6 concurrent runs accepted | done by the server agent (V-2, V-3) |
| 8 | Hand-rolled params layer: `?T` + `.?` x211, catalog/builder/schema triple bookkeeping (IDM-1) | compile-tested typed `Params.parse` prototype in appendix A | deferred (batch 5, typed params: a large mechanical change; the new `Params.field*` helpers remove the worst silent defaults meanwhile) |
| 9 | Silent defaulting where the product promises precise errors (SAF-4) | `{ref, offset:["1/2x",3]}` places the point at the anchor with no diagnostic | done (SAF-4) |
| 10 | wasm size: 14 `std.mem.sort` instantiations cost ~80-110 KB and `parseFloat` 8-25 KB of 866 KB (SIZ-1) | measured: insertion sort -13.0%, `wasm-opt -Oz` -11.8%, both -23.2% | deferred (batch 8). Note: this hardening grew the wasm 902,131 -> 951,291 B (+5.4%) mostly in validation messages |

IDs: `SAF` correctness/safety, `DET` determinism, `IDM` idioms, `TST` tests, `ARC` architecture, `LAY` layout code, `EXP`
exporters, `V` server, `PRF`/`SIZ` performance/size. Severity: **critical** = crash/hang/security from untrusted input or
silent corruption; **major** = real defect or a design issue that will bite soon; **minor** = worth fixing in a normal
cleanup; **nit** = style/readability.

Contents: 1 scope and how to read this - 2 correctness and memory safety - 3 determinism - 4 idiomatic Zig 0.16 and tests - 5
architecture, duplication, dead code (5b layout code) - 6 exporters - 7 server - 8 performance and wasm size - 9 refactor plan -
10 keep as is - 11 status after the engine hardening pass - Appendix A prototypes - Appendix B fuzz results.

## 1. Scope

Files read in full: `api json canon model units geom clip pathclip pathgeom hatch scene compile style section` (core), `ops load catalog
schema builders` (large parts: `builders` about half line by line, the rest through function-size, grep and cast audits; every
function over 90 lines was opened or measured), `annot route lint drawview iso view` (layout), `svg dxf pdf raster png font
textgeom sheet mesh drawing` (exporters), `serve http agents proxy events workspace main wasm` (server and ABI), `build.zig`,
`build.zig.zon`, `.github/workflows/*.yml`, `NOTES.md`, `AGENTS.md`. Skimmed: `validate` (function by function), the `*tests*.zig`
files, `tests/*.sh|mjs`, `tools/zig-engine/*`.

## 2. Correctness and memory safety

### SAF-1 - critical - Untrusted floats reach `@intFromFloat` unchecked (panic in safe builds, UB in shipped ones)

**Where** (snapshot db3ace7):
`drawview.zig:54` (`meta.jurisdiction.edition`), `builders.zig:1188,1213,1620,1660` (`gauge`),
`annot.zig:636,735` (`style.notes.wrap_chars`), `units.zig:124` (`appendFtIn`) and `:196` (`scaleLabel`),
`geom.zig:242` (`arcSteps`), `hatch.zig:57-58,126,167`, `iso.zig:288,507`, `mesh.zig:58`, `pdf.zig:66`, `json.zig:321`,
`section.zig:376` (`drawFaceTie`), `builders.zig:932,1108,1519`, `drawview.zig:142`. The lengths path is the widest:
`appendFtIn` (`units.zig:124`) is called by the `summary` bounds strings (`load.zig:143-150`), by `builders.ftin` and by
every dimension/diagnostic that prints a length, so any component extent beyond ~5.7e17 inches reaches it.

**What.** Every one of these converts an `f64` that came from the document, the style or an op into an integer without
a range check. A document is untrusted input (LLM output, a web client, a file somebody dropped in the served folder),
and a JSON `number` is any double (the parser even admits `1e999` as `inf`, SAF-3). In a Debug/ReleaseSafe build `@intFromFloat` panics when the
value is out of range (NaN, +-inf, >= 2^63 for `i64`, negative for `usize`); in `ReleaseSmall`/`ReleaseFast`
(everything that ships: the wasm module and `release.yml`'s CLI binaries) it is undefined behaviour.

**Reproduced** on the Debug CLI (all exit with `panic: integer part of floating point value out of bounds`):

```sh
# 1. meta.jurisdiction.edition = 1e30 (or 1e999)    -> drawview.zig:54  codeBasis          kerf call check
# 2. connector "gauge": 1e20                        -> builders.zig:1188 buildConnector    kerf call check
# 3. style {"notes":{"wrap_chars":-5}} (or 1e30)    -> annot.zig:636    prepNotes          kerf call export
# 4. a polygon point [0,0,1e-14] (bulge), or 1e14   -> geom.zig:242     arcSteps           kerf call drawing|mesh|export
# 5. component "slope": "nan"  (a string!)          -> units.zig:124    appendFtIn via load.zig:143   kerf call check
# 6. length: "9999999999999999999" (19 digits)      -> units.zig:124    appendFtIn         kerf call check|drawing
# 7. view scale "1e300:1" or "1:1e-300"             -> units.zig:196    scaleLabel         kerf call drawing|check
# 8. connector points[0].offset = [1e300, 0]        -> section.zig:376  drawFaceTie        kerf call drawing
# 9. doc run: [-1e300, 1e300]                       -> iso.zig:507      r() (check); mesh hangs > 15 s
```
Items 1-3 are doc-level inputs in the flush-beam reference with the single field changed; 4-9 were found by the numeric-extremes
pass (appendix B). In `ReleaseSmall` 4 hangs (10 s timeout) or exits `kerf: OutOfMemory` after ~6 s, 5 prints a garbage summary,
6-7 print garbage labels, 8 exits `OutOfMemory` after 2.6 s, 9 returns normally: the same line is a panic, a hang, an OOM
or silence depending on the build and the value. The shipped `tools/zig-engine/fuzz.py 300 1` finished with **0 crashes** on
the same build: it never produces these values (TST-2).

**Why it matters.** `kerf serve` runs the engine in-process on every file in the workspace folder (`apiList` -> `check`),
and a Zig panic aborts the whole process, so one crafted `.kerf.json` (or one POST) kills the server and every SSE client
with it. In the shipped `ReleaseSmall` wasm and CLI there is no trap at all: the cast is undefined behaviour and the
result is garbage or a hang (a safe wasm build would trap with an opaque `RuntimeError: unreachable`; see SAF-2).

**Fix.** Two layers, both small.

1. One checked-conversion helper used at every site (new `num.zig`, or in `geom.zig`):

```zig
/// Float -> integer that never invokes UB: null for NaN/inf/out-of-range, truncates toward zero otherwise.
pub fn toInt(comptime T: type, x: f64) ?T {
    if (!std.math.isFinite(x)) return null;
    const bits = @typeInfo(T).int.bits;
    const signed = @typeInfo(T).int.signedness == .signed;
    const hi: f64 = std.math.ldexp(@as(f64, 1), if (signed) bits - 1 else bits); // 2^n is exact; open bound
    if (x >= hi or (if (signed) x < -hi else x <= -1)) return null;
    return @intFromFloat(x);
}
/// Clamp then convert (NaN -> lo): for counts and step numbers where "too big" should saturate.
pub fn toIntClamped(comptime T: type, x: f64, lo: T, hi: T) T {
    if (std.math.isNan(x)) return lo;
    return @intFromFloat(std.math.clamp(x, @as(f64, @floatFromInt(lo)), @as(f64, @floatFromInt(hi))));
}
```
(compiled and tested on 0.16.0, appendix A.1)

2. Reject the values at the boundary so later code can trust them: `json.parse` rejects non-finite numbers
   (`1e999` currently parses to `inf`, see SAF-3), `style.fromValue` validates ranges (`wrap_chars` in 8..200,
   every `*_in` length in `(0, 100]`, hatch `dy >= 1e-4`) with an `E_STYLE` message naming the key, and `Params.num`
   gains an optional `.{ .min, .max }` so `gauge`, `count`, `plies`... are range-checked where they are read.

**Test.** One table-driven test per site is overkill; instead add `std.testing.fuzz` (0.16 has `std.testing.Smith`) over
`kerf.call(.., "check"|"drawing"|"export", ..)` with a generator that mutates numeric leaves of the reference docs to
`{0, -0, 1e-300, 1e300, 1e15, -1, NaN-via-string}`, see TST-2.

### SAF-2 - major - The shipped binaries have all safety checks compiled out, and the wasm module cannot be built with them

* `release.yml:58` builds every release CLI with `-Doptimize=ReleaseSmall`, and `build.zig:81` defaults the wasm to
  `ReleaseSmall`. In both, overflow, `@intCast`, bounds and `unreachable` are undefined behaviour, not panics. The
  network-facing HTTP parser is in that binary (see V-1: `http.zig:142` wraps instead of panicking).
* `wasm.zig:12` sets `pub const panic = std.debug.simple_panic;`. In Zig 0.16 `simple_panic` prints through
  `std.debug.lockStderr` -> `std.Io.Threaded`, which does not compile for `wasm32-freestanding` as soon as safety is on:
  `zig build wasm -Dwasm-optimize=ReleaseSafe` fails with
  `struct 'posix.system' has no member named 'getrandom'` / `'IOV_MAX'` (verified). So the repo has *never* run its
  tests-on-goldens against a wasm with checks enabled; every latent overflow in SAF-1 is invisible there.

**Fix.** Use the trapping handler and add a safe wasm as a CI gate (not as the shipped artifact):

```zig
// wasm.zig
pub const panic = std.debug.no_panic;        // @trap() on every safety failure; builds in every optimize mode
```

Verified: with that line `-Dwasm-optimize=ReleaseSafe` builds (1,894,920 B raw, 602,812 B gzip vs 866,509 / 322,454 for
ReleaseSmall, i.e. 2.2x, too big to ship) and `ReleaseSmall` is byte-for-byte the same size as before (866,509).
Then run `node tools/zig-engine/wasm_golden.mjs` against the ReleaseSafe wasm in CI: any UB-class bug becomes a trap
with a test failure instead of a wrong drawing. For the native release binaries ship `ReleaseSafe` (the size delta is
measured in section 8) or at least `ReleaseFast` + the SAF-1 hardening; a panicking server is better than a corrupted one,
and the real fix for "a panic kills the server" is SAF-1 plus not trusting folder contents.

### SAF-3 - major - `json.zig`: quadratic duplicate-key check, non-finite numbers, no UTF-8 validation

1. `json.zig:274-281` replaces duplicate keys by scanning all previous members for every new key: O(n^2) per object.
   Measured (ReleaseFast): a 1.3 MB body with one 100,000-key `meta` object takes **16 s of CPU** in `kerf call check`.
   `serve` accepts 64 MiB bodies. Fix: only do the linear scan while `items.len <= 16`, beyond that build a
   `std.StringHashMapUnmanaged(u32)` index (lookup only, never iterated, so determinism is unaffected) or detect
   duplicates after the fact by sorting indices by key (stable `std.mem.sort`, then keep the last).
2. `number()` (`json.zig:124-147`) accepts `1e999` and stores `inf`; `fmtNumber` (`:314`) then prints non-finite values
   as `0`. Measured: `kerf call fmt` of `"length": 1e999` returns `"length": 0`, so `kerf fmt -w` silently rewrites the
   user's file. Reject with `"number out of range"` in the parser (`if (!std.math.isFinite(f)) return self.fail(...)`).
3. Strings are not validated as UTF-8 (`0xff 0xfe` in an `id` passes through `fmt` and is written back, producing an
   invalid JSON file and invalid SSE frames). Add `std.unicode.utf8ValidateSlice` on the fast path (`json.zig:167-177`)
   and on the unescaped runs in the slow path. Lone surrogate escapes are already rejected, good.
4. Leading zeros are accepted (`[01, 02]` parses); RFC 8259 forbids them. Harmless but makes `kerf fmt` "fix" invalid
   JSON silently; one more `if` in `number()`.
5. `fmtNumber`'s large-value fallback (`json.zig:317-320`) calls `std.fmt.bufPrint(buf, "{d}", ..)` into a 40-byte buffer;
   for |x| > ~1e38 it hits `NoSpaceLeft` and returns `"0"`. It also pulls `std.fmt`'s float printer into the wasm (about 1 KB in the full module, 4.5 KB in an isolated probe, section 8). Replace with a clamp: values above 9e11 are not meaningful inches.

### SAF-4 - major - Silent defaulting where the product promises precise errors

The quality bar in `AGENTS.md` is "error messages ... precise and actionable for a model". Several parsers do the
opposite and swallow bad input:

| Site | Behaviour |
|---|---|
| `scene.zig:222`, `compile.zig:285`, `builders.zig:180`, `annot.zig:1348` | `{ref, offset: [dx, dy]}`: `units.parseLength(oa[0]) orelse 0`. A typo such as `"offset": ["1/2x", 3]` places the point at the anchor, no diagnostic. (`at.offset`, `compile.zig:297`, is validated; `at.to.offset` is not: inconsistent.) |
| `builders.zig:152` | `[x, y, bulge]`: `xy[2].num() orelse 0`; a string or `null` bulge becomes a straight segment; non-finite or absurd bulge values are accepted (see SAF-5). |
| `style.zig:172` `numOr` and all of `fromValue` | wrong-typed or negative/zero values in a user style silently fall back or flow through (`wrap_chars: "wide"` is ignored, `-5` crashes: SAF-1). |
| `builders.zig:470` | `cover.parts.<part>.<k>` with a non-length value is skipped silently (the top-level `cover.<k>` at `:459` is validated). |
| `builders.zig:925-926` (`place.cover`, `side_cover`) | `... orelse 1.5` hides a non-numeric `cover`. |

**Fix.** One helper `units.parsePair(v) ?V2` / `Params.point(key)` that returns a diagnostic-bearing result, and
`Style` validation (`E_STYLE` with the JSON path). The `[x, y]` pair parse is hand-rolled ~10 times
(`compile.zig:269-290`, `scene.zig:212-224`, `builders.zig:133-185`, `annot.zig:1348`); collapsing it removes the inconsistency.

### SAF-5 - major - Degenerate geometry parameters are not range-checked (reproduced: bulge `1e-14` panics)

`arcSteps` (`geom.zig:239-244`) computes `step = 2*acos(1 - tol/r)`; for a huge radius (tiny bulge, e.g. `1e-14`) `tol/r`
is below machine epsilon, `acos(1) == 0`, `step == 0`, `|sweep|/step` is `inf` and `@intFromFloat(inf)` is the same
SAF-1 failure. Reproduced: a polygon vertex with bulge `1e-14` makes `kerf call drawing` and `kerf call mesh` abort with
`panic: integer part of floating point value out of bounds` at `geom.zig:242` (Debug) (the same formula is copied in `iso.zig:285-288`, `mesh.zig:45-58`, `pdf.zig:66`). A user-supplied
bulge has no bounds anywhere (`builders.zig:152`). Clamp bulges to `|b| in [1e-6, 1e3]` (zero stays a line) when parsing,
and make `arcSteps` return `std.math.clamp(n, 2, 4096)` computed in float before the cast. Replace the four copies
with one `geom.arcSteps`.

### SAF-5b - major - No global resource limits: a 2.5 KB document can cost 20 s and 1.8 GB

Every dimension has a local cap at best (`array.count <= 500`, `place.count <= 200`, `courses <= 200`) and no global one, so
the caps multiply. Measured (ReleaseFast, `kerf call check`):

| Input | Size | Result |
|---|---|---|
| 40 lumber components, each `array: {count: 500}` (20,000 instances) | 2.5 KB | **22.6 s, 1.8 GB RSS** (no profile taken; the quadratic candidates are `validate.overlaps`/`floating` over instances, plus one prism copy per instance); `export` 18 s, 229 MB |
| JSON object with 100,000 keys | 1.3 MB | 16 s (SAF-3) |
| 10,000 components (a chain of `at` refs / flat), ReleaseSmall | ~1.5 MB | `check` 4.4 s / 3.1 s, `drawing` 2.0 s; 50,000: `check` > 60 s, `drawing` 35.6 s / 8.4 s (by-id linear lookups, O(n^2) passes, PRF-1) |
| 100 notes + 20 dims + 10 labels | ~30 KB | 15 s (LAY-1) |
| `concrete` polygon, 25,600 vertices | 0.9 MB | 0.85 s |

The wasm build has a 4 GiB address space and runs on the browser's main thread; `kerf serve` takes 64 MiB bodies. **Fix:** one
`limits.zig` with documented constants checked where the data enters (`compile`, `view.parse`, `annotate`, builders) and reported
as `E_LIMIT` with the number and the maximum, e.g. `max_components = 2000`, `max_instances_total = 5000`, `max_annotations_per_view = 120`,
`max_points = 5000`, `max_json_bytes = 8 MiB`, `max_hatch_lines` (already 150,000 in `hatch.zig:13`: the one place this exists
today). `validate.overlaps` should bucket instances by box (sort by `x0`) before the O(n^2) loop.

### SAF-5c - major - `std.fmt.parseFloat` accepts `nan`/`inf`, so NaN reaches geometry and the output stays "valid"

`units.zig:173-174` (`parseScale`), `:227-232` (`parseSlope`) and the scanner in `parseLengthStr` use `std.fmt.parseFloat`, which
accepts `nan`, `inf` and `infinity`. Reproduced (all `exit 0`, no diagnostic unless noted):

* `scale: "nan:1"` -> `a <= 0` is false for NaN, so the view gets `scale: 0`; the SVG header reads
  `width="nanin" height="nanin" viewBox="0 0 nan nan"`. `"1:inf"` -> bounds `[0,0,0,0]`; `"1:1e300"` -> silently degenerate view;
  `scale: "1'=1\""` is accepted and takes 5.3 s (Debug) / ~1 s (ReleaseSmall) to return a 35 MB drawing.
* `slope: "nan"` (or `"inf"`, `"1e999"`) -> NaN geometry; the SVG contains `d="Mnan nanLnan nan"`; `check` panics at `load.zig:143`
  in Debug and "succeeds" with a garbage summary in ReleaseSmall.
* view `crop: {x: [-1e300, 1e300], ...}` -> a 16 KB SVG with `width="0in" height="0in" viewBox="0 0 0 0"` and every path `M0 0L0 0`,
  no error. (Inverted, zero or NaN crops *do* give `E_VIEW`.)
* `run: [-1e300, 1e300]` makes `mesh` run for more than 15 s in both Debug and ReleaseSmall.

**Fix.** A single `units.parseFinite(text) ?f64` with the digit grammar the Scan already enforces (and the Clinger fast path of
IDM-4, which cannot produce `nan`/`inf`), a documented range check for scale factors (`1e-3 .. 1e6`), crops and `run`
(`|v| <= 1e6` inches, or whatever `limits.max_coord` is; SAF-5b), each reported as `E_VIEW`/`E_PARAM` with the offending value.

### SAF-6 - minor - Diagnostics are dropped on OOM, and one call patches "the last diagnostic" by index

* `model.Diags.add/addFix` (`model.zig:44-51`) end in `catch return` / `catch {}`. Out of memory therefore *removes an
  error* and `apply` can then report `ok: true` for a document that failed to compile (`api.zig:311`). In the wasm
  build (a 4 GiB linear memory and no overcommit) OOM is a real condition. ~60 more sites follow the same pattern
  (`scene.joinIds`, `model.joinQuoted`, `builders.ftin/fmtNum`, `validate.ftin/boxText`, `ops.zig:30-36` ...).
  Fix with a single sticky flag instead of 60 `try`s: `Diags.oom: bool`, set in the `catch`, checked once in
  `api.call` (`if (diags.oom) return error.OutOfMemory`), so the failure is reported as `E_OOM` instead of vanishing.
* `compile.zig:434-438` calls `p.fail(...)` and then rewrites the code of "whatever diagnostic is last":
  `scene.diags.list.items[scene.diags.list.items.len - 1].code = "E_ANCHOR_UNKNOWN";`. If `p.fail`'s `allocPrint` failed
  nothing was appended, so this either patches an unrelated earlier diagnostic or underflows on an empty list. Give
  `Params.failCode(code, key, fmt, args)` and call that.

### SAF-7 - minor - Global mutable scratch in `canon.zig`

`canon.zig:29` `var type_scratch: [64][]const u8 = undefined;` is a file-level mutable buffer returned as a slice from
`componentKeys` (`:56`). Today the server serialises writes with a mutex so it does not race (the server review confirms
`write_mu` covers it), but `kerf.call` is documented as safe for in-process callers (`NOTES.md`, teak), and two threads
calling `fmt`/`apply` concurrently would corrupt each other's key order. It also silently truncates at 64 names
(`if (n < type_scratch.len)`), changing canonical order without a diagnostic. Fix: put the scratch list in the
`Pretty` writer (it already has an allocator): `KeyCtx` gets an `arena: Allocator` and `componentKeys` returns
`ctx.arena.alloc(...)`.

### SAF-8 - minor - `undefined`, `unreachable` and `.?` carry invariants the compiler cannot check

* `scene.zig:21` `Comp.built: model.Built = undefined` is read by `Scene.partBox`, `anchorPoint`, `validate`... only
  after `state == .ok`; nothing enforces it. Make it `?model.Built` or split `Comp` (declaration) from `Placed`.
* `builders.zig:1779` `build()` ends in `unreachable` after an if-chain of type names; adding a catalog entry without a
  builder is undefined behaviour in ReleaseSmall. Use a comptime table plus a `comptime` completeness check (IDM-2).
* 211 `.?` unwraps (on 161 lines of `builders.zig`) rely on "`p.ok` is still true, so every `?T` is non-null". See IDM-1.
* `style.zig:195` `var s: Style = undefined;` then field-by-field `inline for` defaulting; `font.zig:32` the same for `Font`.
  Safe today, brittle tomorrow; build the struct with `.{ .pens = ..., .materials = ..., }` and `s = .{ .id = ..}`
  using field defaults.

---

## 3. Determinism

Verdict: **good.** No `HashMap`/`AutoHashMap` anywhere in the engine (grep), all 22 sorts use the stable
`std.mem.sort` (never `sortUnstable`), no clocks, no randomness outside a `png.zig` test, atomic output assembly.
`std.fmt` float printing used by `svg.zig`/`pdf.zig` is Zig's own implementation (no libc), so it is identical on every
target. Native and wasm byte-identity of the goldens is already gated (`wasm_golden.mjs`, 24/24). Residual risks:

### DET-1 - minor - `@sin/@cos/@tan` come from whatever `libm` the target links

The code already calls `std.math.atan2/acos/atan` (pure Zig, platform independent) but uses the builtins `@sin/@cos/@tan`
(`geom.zig`, `iso.zig`, `hatch.zig`, `pathgeom.zig`, `font.zig`). Those lower to compiler-rt's `sin/cos/tan` on a libc-free
Linux/wasm build (which is why native and wasm goldens are byte-identical today), but to the system `libm` wherever one is
linked (macOS `libSystem`, any `-Dtarget=*-gnu` that links libc). Last-ulp differences are quantised away by the 1e-4
number format *most* of the time, but not at rounding ties, and the exact classifiers (`orient2d`, midpoint inclusion) can flip on a
one-ulp difference. I could not verify macOS/Windows behaviour from this Linux host, so this is a risk to test, not an observed
difference. CI cross-compiles macOS/Windows (`ci.yml:23`) but never runs `check_golden.sh` there. **Fix:** run the golden gate on a
macOS and a Windows runner, or vendor the ~100 lines of musl-derived `sin/cos/tan` from `lib/compiler_rt/{sin,cos,tan}.zig` into a
`kmath.zig` and call that everywhere.

### DET-2 - nit - Three number formatters with different rules, and `{d}` prints `nan`/`inf`

`json.fmtNumber` (1e-4, integer arithmetic), `svg.zig:37-41` and `pdf.zig:16-20` (1e-3 then `std.fmt` `{d}`),
`drawing.zig:121` (wraps `json.writeNumber`). `{d}` of NaN/inf emits `nan`/`inf`, which is invalid in an SVG path or a PDF
stream; `fmtNumber` maps them to `0`. Put one `fmtFixed(buf, x, decimals)` in `json.zig` next to `fmtNumber`, use it from
svg/pdf (byte-identical for finite values below 1e12: verify with `check_golden.sh`), and drop `std.fmt`'s float printer
from the wasm (see section 8).

### DET-3 - nit - Float keys in sorts

`std.sort.asc(f64)` on values that can be NaN (`pathclip.zig:60,195`, `hatch.zig:89`, `section.zig:793,880`, `iso.zig:245`, `raster.zig:258`, `annot.zig:254`) gives an
inconsistent comparator; the result is still deterministic (block sort does not loop) but meaningless. Moot once SAF-1's
boundary validation lands.

## 4. Idiomatic Zig 0.16

What is already idiomatic and should stay: unmanaged `std.ArrayList(T) = .empty` with the allocator passed per call
everywhere, `std.mem.trimEnd`/`std.Io.Writer` in the CLI, per-call `ArenaAllocator` in `api.call`, `inline else` over the
`drawing.Item` union (`drawing.zig:70-79`), `comptime` field tables in
`schema.zig`/`canon.zig` (`schema.keys`), `@embedFile` anonymous imports instead of codegen, `std.testing.allocator`
for the API-level leak tests, no `usingnamespace`, no `anyerror` in the library.

### IDM-1 - major - Params: `?T` + `.?` (211 unwraps) is a hand-rolled, unchecked schema

Every builder reads parameters through `model.Params` methods that return `?T` *and* set `p.ok = false` on failure,
then checks `if (!p.ok) return null;` once and unwraps with `.?` (`builders.zig`: 211 of them on 161 lines, e.g.
`:719-737` (`slab_edge`), `:1250-1281` (`truss`)). The invariant "every optional is non-null if `p.ok`" is invisible to the compiler: reorder one
line or add a conditional read and you get a null-unwrap panic (a safety panic in Debug, UB in ReleaseSmall). It also
makes the builders long (`buildConcrete` 218 lines, `buildLumber` 145, `buildTruss` 132) because parsing, validation and
geometry are interleaved. And the same facts are written three times: `catalog.zig` prose ("`sawn | lvl | psl | lsl |
glulam`"), the builder (`p.choice("product", "sawn", &.{ "sawn", "lvl", ... })`) and `schema.zig`.

**Suggested shape** (comptime reflection, no allocation, collects *all* errors like today, which is what makes
the messages useful to an LLM):

```zig
const LumberParams = struct {
    size: []const u8,                                   // required: no default
    product: enum { sawn, lvl, psl, lsl, glulam } = .sawn,
    run: enum { z, x, y } = .z,
    orient: enum { upright, flat } = .upright,
    face: enum { wide, narrow } = .wide,
    plies: Int(1, 8) = .{ .v = 1 },
    treated: bool = false,
    length: ?Length = null,                              // `?` only where absence is meaningful
    ...
};
// in buildLumber:
const lp = p.parse(LumberParams) orelse return null;    // one call; every field is final and typed
```

`Params.parse` walks `@typeInfo(T).@"struct".fields` once (`inline for`), dispatching on the field type
(`bool`, `[]const u8`, enum via `std.meta.stringToEnum` with the "must be one of ..." message generated from
`std.meta.fieldNames`, `Length` via `units.parseLength`, ranged ints), emits the same `E_PARAM` texts, and returns `null`
if any diagnostic was produced. The catalog then reads its parameter table *from the struct* (`@typeInfo` gives names,
defaults via `field.defaultValue()`, enum choices) and only the prose descriptions stay hand-written, which removes
the drift between catalog/builder/schema. This is the single highest-leverage readability change for future LLM
maintainers: one declaration per component type, 40 fewer lines per builder, no `.?`.

Migration is incremental: add `Params.parse` next to the current methods, convert one builder at a time (goldens and the
`E_PARAM` message tests must stay byte-identical, `tests/cli_ergonomics.mjs` already checks messages).

### IDM-2 - minor - Stringly-typed dispatch where Zig has enums and `StaticStringMap`

* `api.zig:121-131` dispatches 11 function names with sequential `std.mem.eql`; `builders.zig:1763-1779` does the same for
  14 component types and ends in `unreachable`; `section.zig:701`, `:1133`, `validate.zig:70-84`, `style.zig:157-169`
  (`layerKeyForPen`) are all if-chains over string literals.
  `grep` finds 0 uses of `std.StaticStringMap` and one `std.meta.stringToEnum` (`view.zig`).
* Pen names (`"cut"`, `"beyond"`, `"hidden"`, `"steel"`, `"rebar"`, `"anno"`, `"dim"`, `"break"`, ...) are bare string
  literals in ~100 places (`annot.zig` 13x `"anno"`, 7x `"dim"`; `builders.zig` 13x `"steel"`; `section.zig` 8x `"beyond"`).
  A typo (`"hiden"`) compiles, `penFor` quietly falls back to `"beyond"` (`section.zig:90-92`), and the drawing comes out wrong
  with no diagnostic. `drawing.Drawing.kind`, `PathItem.pen`, `.layer` are `[]const u8` as well.

**Fix.** (a) `const Fn = enum { version, help, catalog, ... }; switch (std.meta.stringToEnum(Fn, name) orelse return unknownFn)`
gives exhaustive switching and an error message generated from `std.meta.fieldNames(Fn)`; same for `catalog.Type` (a
`pub const Type = enum { lumber, panel, ... }` whose tag names are the catalog names). Adding a type without a builder
then fails to compile instead of reaching `unreachable`. (b) `pub const Pen = enum { cut, beyond, hidden, ... }` with
`style.zig` mapping user-style pen *names* to `Pen` once at load; `PathItem.pen: Pen`. (c) `layerKeyForPen` becomes
`switch (pen) { ... }` with no default.

### IDM-3 - minor - Error model: everything is `Allocator.Error`, failure is a side channel

The library's only error is OOM; semantic failure is "append to `Diags`, return `null`, check `p.ok`". That is a
legitimate design (it collects every problem in one pass), but the plumbing is manual and easy to get wrong
(SAF-6, SAF-8). Two small improvements keep the design: (1) name it: `pub const Reported = error{Reported}` and let
`Params.fail` return it so `const x = try p.len("width")` replaces `orelse return null` + `.?`, with the "collect all"
behaviour preserved by `Params.parse` (IDM-1) doing the collecting; (2) public functions whose error set is inferred
(`font.zig:27` `parse(...) !Font`, `serve`/`main` `!u8`) should say `error{BadFont, OutOfMemory}!Font` so callers get an
exhaustive `switch`.

### IDM-4 - minor - Is the custom `json.zig` justified? Yes, with caveats (measured)

What it provides that `std.json` does not give directly: an order-preserving `Value` with `[]Member` objects (std's
`ObjectMap` also preserves insertion order, but is a heap-allocated hash map), line/column parse errors (std's `Scanner`
has `Diagnostics`), last-duplicate-wins-in-place semantics, the Kerf number format (round to 1e-4, no exponent, `-0` ->
`0`), canonical pretty-printing with a key-order hook, RFC 7396 merge patch and deep clone, all allocated from one arena.
Size is a wash: I built two `wasm32-freestanding` ReleaseSmall probes (parse + pretty-print): **std.json 62.7 KB vs
kerf json 54.7 KB**, and most of both is `std.fmt.parseFloat` (**removing it from the custom parser drops 54.7 KB to
28.9 KB**, an upper bound of ~25 KB or 3% of the shipped wasm for the parser alone; in the full module twiggy attributes ~8 KB to
`parseFloat` code, so expect 8-25 KB; the `{d}` fallback in `fmtNumber` is another 4.5 KB in the probe but ~1 KB in the full module).
Recommendation: keep it, but (1) fix SAF-3, (2) replace `std.fmt.parseFloat` with a ~40-line exact fast path
(mantissa <= 2^53 and |exp10| <= 22 is correctly rounded by one division or multiplication, the Clinger fast path;
reject anything else as "number out of range for a drawing"), reused by `units.zig` (`Scan.number`, `parseScale`,
`parseSlope`, which also use `parseFloat` and accept `nan`/`inf` spellings today), and (3) add a differential fuzz test
(`std.testing.fuzz`) that checks accept/reject agreement with `std.json.Scanner` and equal values on accepted input.

### IDM-5 - nit - Style-guide drift

* Locals/params in UPPERCASE or single letters: `const S = env.S;` (`annot.zig:633,677,729`, 26 such declarations),
  `A`, `B` in `clip.zig:69` and `validate.zig:480-500`, `R` in `pathgeom.zig`. The Zig style guide wants `snake_case` for
  variables (`scale`, `a_loops`, `radius`).
* `*_mod` import aliases (65 occurrences: `style_mod`, `font_mod`, `sheet_mod`, ...) exist to dodge shadowing by
  locals named `style`/`font`. Prefer `const style = @import("style.zig")` and name the local `st`/`stl`; or
  import types (`const Style = @import("style.zig").Style`).
* `align_` (`drawing.zig:49`, 9 uses) -> `halign` next to `valign`.
* `geom.Loop` exists but `[]const Pt` is spelled out ~150 times; use one or the other. `V2.eql(a, b, tol)` is an approximate
  comparison named like exact equality: `approxEq`.
* `units.zig:160-166` `Scale` has an orphan doc comment ("Normalised display text...") above `isNts`, and
  `scaleLabel(a, text, factor)` takes `text` only to `_ = text;` (`units.zig:213`).
* Public surface: `kerf.zig` re-exports every module including test modules (`pub const layout_tests`, `views_tests`,
  `tests`, `testdocs`), `testdocs.zig` embeds files that only exist as anonymous imports in the *test* module
  (`build.zig:57-61`), so `kerf.testdocs` fails to compile for any consumer that touches it. Move those to
  `test { _ = @import("tests.zig"); }` blocks and keep `pub` for what teak actually calls (`call`, `Result`, `version`,
  `drawview`, `compile`, `scene`, `svg`, `mesh`, `drawing`).

### IDM-6 - nit - Leftovers that indicate dead or half-finished code (13 `_ = x;` in non-test code)

| Where | What |
|---|---|
| `section.zig:403-406` | `materialNameForPen(self, mat)` returns `mat`; `_ = self;` |
| `section.zig:439-448` | `edgeIsTop`: `if (p.b != 0) { d = q.v().sub(p.v()); }` re-assigns the same value; `_ = self;` |
| `section.zig:495`, `:597-598` | `const crop = ...; _ = crop;` and `const bx = geom.Box{}; _ = bx;` |
| `section.zig:971` | `s2.stroke == s1.stroke and false` (dead condition; precedence also makes it `a or (b and false)`) |
| `section.zig:1034,1050` | `_ = k; _ = nseg;` in `dedupe` |
| `compile.zig:462` | `if (arr_count == 1) {} // no shift` |
| `ops.zig:112` | `return std.mem.eql(u8, head, id) and (at != null or true);` |
| `iso.zig:721-723` | `const comp = &scene.comps[ip.comp]; _ = comp;` and `var inst: u32 = 0; inst = ip.instance;` |
| `validate.zig:141,170` | `_ = a;` in `floating`/`untreated` (unused parameter threaded from `run`) |
| `validate.zig:499` | `const other_lo = ...; _ = other_lo;` |
| `lint.zig:433` | `ackShape(a, scene, ...)`: `_ = scene;` |
| `section.zig:665,165`, `validate.zig:90` | `@import("pathgeom.zig")` / `@import("clip.zig")` inside function bodies although the file imports at the top |

`defer ts.deinit(a)` on an arena-backed list inside a hot loop (`hatch.zig:74`, `section.zig:775`) is the only place the
code frees; everywhere else the arena is relied on. Either hoist the list out of the loop with `clearRetainingCapacity()`
(better: no per-line allocation) or drop the `defer`.

### IDM-7 - minor - Output assembly: `ArrayList(u8)` + final `dupe`, versus `std.Io.Writer`

Exporters build the whole file in an arena `ArrayList(u8)` via `appendSlice`/`print`, `api.call` then `gpa.dupe`s it, and the
CLI writes it out. That is simple and wasm-friendly, but it holds output twice and prevents streaming a 6000-px PNG or a
large DXF. In 0.16 the idiom is to take a `*std.Io.Writer` (`std.Io.Writer.Allocating` for the in-memory case): exporters
become `fn render(w: *std.Io.Writer, d: *const Drawing, ...) std.Io.Writer.Error!void`, the CLI passes its file writer, the
wasm/`call` path passes an `Allocating`. Do this opportunistically, not as a project of its own; it also deletes the three
private `num()` helpers (DET-2) because `w.print("{d}", ..)`-style formatting is then available everywhere.

### TST-1 - minor - Test organisation

129 tests, which is a good base (every function on every reference doc, leak checks, golden byte-identity via shell).
Issues: (1) 85 arena setups, most followed by the same `json.parse(...).?` + `style.load` + `Diags.init` + `compile`
preamble (`tests.zig`, `section.zig`, `compile.zig`); a `testing.zig` with `Fixture.init(src)` returns `{arena, doc, style,
scene, diags}` and cuts roughly 400 lines. (2) Tests are spread across inline blocks, `tests.zig`, `layout_tests.zig`,
`views_tests.zig`, `ergo_tests.zig`, `v015_tests.zig` named after delivery milestones rather than subject; consider
`test/<subject>.zig` per area. (3) `units.zig:258` is the only `std.debug.print` in non-test source.

### TST-2 - major - No in-tree fuzz/property tests; the external fuzzer is not in CI

`tools/zig-engine/fuzz.py` mutates the reference docs through the CLI (300 runs by hand; it finished with **0 crashes** on
the tree that panics on all nine inputs of SAF-1, because its junk values are `0, -1, 1e9, ...`, never `1e30`, `nan` or `1e-14`) and is not run by
`.github/workflows/ci.yml`; neither is `tests/check_golden.sh` (the determinism gate), `tools/zig-engine/wasm_golden.mjs`
nor `tests/cli_ergonomics.mjs`. Zig 0.16 has `std.testing.fuzz(context, testOne, .{})` with `std.testing.Smith`; add
three: `json.parse` (differential vs `std.json`), `units.parseLengthStr`/`parseScale`/`parseSlope` (never panics, finite or
null), and `kerf.call` on mutated numeric leaves of the reference docs (SAF-1). `zig build test` runs the corpus; `zig
build test --fuzz` explores. Add `check_golden.sh` and the wasm golden run to CI (they are cheap: about 70 CLI invocations of tens of
milliseconds each).

---

## 5. Architecture, duplication, dead code

### ARC-1 - major - Domain knowledge is hard-coded by name inside geometry and validation code

Material and component-type names are string literals in the middle of algorithms, so adding a material or letting a
user style define one needs engine edits, and the style's own flags are shadowed by a parallel name list:

* `section.zig:701` `isFillMaterial` ("earth", "gravel", "sand", "compacted_fill") duplicates `validate.zig:79` `isFill`,
  while `style.Material.fill` already exists and is consulted next to it (`section.zig:517`: `isFillMaterial(p.material) or m.fill`).
* `section.zig:1133` `isMetal`, `validate.zig:70` `isMasonry`, `:75` `isUntreatedWood`.
* `section.zig:158` `p.material == "vapor_retarder"` special-cases an offset in the section renderer; `section.zig:400`
  `ty.name == "lumber"`, `compile.zig:244` (`"anchor_bolt"` default anchor), `:492` (`"truss"` pitch), `validate.zig:113,116`
  (`"connector"`).

**Fix.** Give `style.Material` a `role: enum { solid, fill, metal, masonry, wood, membrane, ... }` set from the embedded
style JSON (one new key, defaulted from the name for user styles that omit it), and give `catalog.Entry` the behavioural
flags the engine asks for (`default_anchor`, `has_pitch`, `is_hardware`, `lengthwise_grain`). The renderer/validator then
ask `m.role == .metal`. This is also the clean seam between "layout/render/validation" and "domain catalog".

### ARC-2 - minor - Duplicated helpers (each is a place where a future fix lands in one copy only)

| Concept | Copies |
|---|---|
| number text | `json.fmtNumber`; `svg.zig:37`, `pdf.zig:16` (1e-3, `{d}`); `drawing.zig:121`; `annot.zig:523`, `drawview.zig:97` (`fmtNum` with different rounding: 1e-2 / 1e-1 are applied by the caller), `builders.zig:60` |
| `ftin(a, x) catch "?"` | `builders.zig:56`, `validate.zig:24`, `lint.zig:347` (inline) |
| flatten tolerance | `section.flat_tol` (0.005) and `validate.flat_tol` (0.005); iso uses 0.004 inline; mesh `arcStepFor` |
| arc step count | `geom.arcSteps`, `iso.zig:285`, `mesh.zig:45-58`, `pdf.zig:66` (see SAF-5) |
| `visiblePieces` / `visibleOpen` | `section.zig:755-851` and `:855-941`: ~85% identical (crossing collection with arc/line split, sub-segment visibility, piece assembly). `visiblePieces(loop)` = `visibleOpen(loop, closed=true)` plus the wrap-around join; extract `splitParams()` and `isHidden()`; -90 lines |
| `[x, y]` pair parse | `compile.zig:269-290` (3x), `scene.zig:212-224`, `builders.zig:133-185`, `annot.zig:1348`, `style.zig` (`size_in`) |
| box of an edge | `clip.segBox`, `section.dedupe` inline `Box{...}` twice (`:963`, `:972`), `pathclip.inBox` |
| JSON string escaper | `json.writeString`, `http.zig:233`, `main.zig:231` (differing control-character handling) |
| `id` head of a ref (`comp#k.part@anchor` -> `comp`) | `compile.refCompId`, `ops.referencesComp`, `scene.whereOccursText`, `parseRef` |
| tolerances as bare literals | `1e-9`, `1e-6`, `1e-7`, `1e-12`, `1e-4` appear ~300 times with at least five different "equal within" meanings; `clip.snap` and `section.dedupe_tol` are the only named ones |

A `tol.zig` with named constants (`snap = 1e-7`, `coincident = 1e-9`, `param = 1e-12`, `paper_hairline = ...`) and a
`geom.approxEq` would make the exact-vs-approximate decisions greppable.

### ARC-3 - minor - God functions and files

Longest functions (lines): `dxf.render` 403, `main.run` 368, `iso.build` 261, `annot.annotate` 218, `buildConcrete` 218,
`compile.placeComponent` 204, `clip.boolean` 202, `ops.applyOne` 193, `compile.compile` 184, `style.fromValue` 173,
`buildLumber` 145, `drawing.toJson` 135, `buildTruss` 132, `serve.cliMain` 128, `section.drawCut` 127, `placeRebar` 125,
`validate.cover` 123, `load.inspect` 122, `route.solveColumn` 119. Files: `builders` 1801, `annot` 1565, `serve` 1343,
`section` 1244. Concrete splits that keep behaviour byte-identical:

* `compile.compile` -> `readComponents` (ids, types, unknown keys), `buildDeps`, `topoOrder` (Kahn + cycle report);
  `placeComponent` -> `resolveAt`, `resolveZ`, `resolveArray`, `instantiate`. The 4 stages become unit-testable.
* `ops.applyOne` -> one function per root (`applyDoc`, `applyMeta`, `applyComponents`, `applyViews`), shared
  `unknownId`/`touch`; the `(k[0] == 't' and k.len == 6)` trick in `dependents` (`ops.zig:139`) becomes a named field list.
* `builders.zig` -> `builders/` directory, one file per component family (`lumber.zig`, `masonry.zig`, `concrete.zig`,
  `steel.zig`, `membrane.zig`), common helpers in `builders/common.zig`; `build()` is the comptime table from IDM-2.
* `section.zig` -> `section/{occlusion,dedupe,breaks,hatch}.zig` around the `Section` struct (`visiblePieces`, `dedupe`,
  `chainStrokes`, `breakLines` are already free functions or self-contained methods).
* `style.fromValue` -> table-driven: `const text_keys = .{ .{"height_in", "text_height_in"}, ... }` + `inline for`.

### ARC-4 - minor - Catalog encodes structure in strings

`catalog.Param.name` can be `"a, b"` (aliases) and four places split it again with `splitSequence(u8, p.name, ", ")`
(`catalog.zig:368,378,389`, `canon.zig:44`). `canon.componentKeys` therefore re-derives key order at runtime from prose.
Make it `names: []const []const u8` (first is canonical) so the split disappears and the canon/lint/schema consumers
share it; `catalog.hardware` (58 rows) can be a `std.StaticStringMap(Hardware)`.

### ARC-5 - minor - Build and repo hygiene

* `build.zig:5-6` embeds `../../spec/...` (outside the package root) and `build.zig.zon` `.paths` does not list `spec`,
  so `engines/zig` cannot be consumed as a *fetched* dependency, only as a path dependency inside the monorepo
  (`NOTES.md` admits it). If teak ever wants a tarball dependency, copy `kerf-standard.kerfstyle.json` and
  `kerf-simplex.json` into `engines/zig/data/` through a `tools/sync-spec.sh` + a CI "no drift" check.
* `zig build` defaults to Debug; `b.standardOptimizeOption(.{})` could pass `.preferred_optimize_mode = .ReleaseSafe`
  so the default developer and CI build is the checked-but-fast one (at these document sizes ReleaseSafe and Debug are both
  far inside the 50 ms budget, `NOTES.md` itself says ReleaseFast is not measurably faster for wasm).
* `NOTES.md` quotes wasm sizes of 662,838 / 245,562 / 196,330 B; the current tree builds **866,509 B raw / 322,454 gzip**.
  Measured numbers in notes drift; have `tools/size_report.sh` write `engines/zig/SIZES.txt` in CI and link it.

## 5b. Layout, annotation and render assembly (`annot`, `route`, `lint`, `drawview`, `iso`, `view`)

Measured (ReleaseFast, `truss-bearing-cmu` view A plus synthetic annotations; timers added in a private copy, perf line
attribution disagreed with the timers under inlining, so numbers below are from timers):

| Input | Wall time |
|---|---|
| reference views (3 documents x A/B) | 5-33 ms each |
| 12 notes + 4 dims + 2 labels | 0.16 s |
| 50 notes + 10 dims + 5 labels | 2.1 s |
| 100 notes + 20 dims + 10 labels | **15.0 s** |
| 200 notes + 40 dims + 20 labels | **187 s** |
| 400 notes + 80 dims + 40 labels | > 300 s (killed) |
| 30 notes + 30 dims, explicit scale / scale and crop omitted | 1.1 s / 6.9 s (5 full annotate runs) |

### LAY-1 - critical - Layout work is not bounded by the size of the input (DoS from one document)

`route.zig:671-762`, `annot.zig:1381-1411`. `Ctx.budget` (60 or 20) bounds the *number* of `solveColumn` calls, not their cost.
Each `solveColumn` is ~150 slots per note against all other notes (O(150 n^2)) and every slot penalty recomputes bounding
boxes (`BB.ofLeader`, `BB.ofBox`, `route.zig:354,363,480,484`). `route()` then builds a hit list that is O(n^2) (3.4-4.3k hits at
200 notes) and calls `proposeFix` for **every** hit (`:745-757`; each does a `layout()` plus ~60 `hitsOf` scans) although
`reportHits` prints only 8 (`annot.zig:581`). That is repeated for up to 8 repair passes and the loop does not stop while the
hit count keeps dropping slowly (`stale` resets on any improvement, `annot.zig:1384-1391`). Instrumented at 100 notes:
`renderDimLabels` ~5 ms, `route` 1.4-3.8 s per pass, i.e. >99% of the time, called 8 times. Computing `proposeFix` only for
the 8 reported hits took 100 notes from 15.5 s to 7.3 s and 200 notes from 187 s to 30.6 s; the solver itself is still ~3 s per
pass at 200 notes.

An LLM can trivially author 100 notes, and `kerf serve` and the wasm UI both run this on the request/UI thread. **Fix:**
(1) compute fix proposals lazily for the hits that are printed (pass `max_fixes`, return hits sorted); (2) cap the stored hit
list (64 worst + a total count); (3) precompute `BB` per leader and per box once per `eval`/`solveColumn` and index it;
(4) a global work budget counted in `pairPen` evaluations (not column solves) that falls back to the SPEC 6.3 baseline plus
`decross` and one aggregate `W_LEADER_HIT` when exhausted; (5) stop the repair loop when the hit count did not improve by >= 10%
in the last pass; (6) a regression test with 150 notes asserting the work counter stays under a threshold.

### LAY-2 - major - Hostile or degenerate style values hang or crash the layout

`annot.zig:166-205,636,735`, `:325-326`, `:362-363`, `style.zig:320`. Reproduced (ReleaseFast, `timeout 10` kills it):
`style {"notes":{"wrap_chars":0}}` and `{"text":{"height_in":0}}` both hang. `wrap()` with `chars == 0` takes the
`wl > chars` branch, appends `w[0..0]` and `continue`s without consuming `w` (also allocating without bound);
`height_in: 0` gives `step = 0` in `candidatesFor`/`bandedLabelPoint`, so `while (cells/step^2 > 2500) step *= 1.5` never ends.
`wrap_chars` negative/NaN/huge reaches `@intFromFloat` (SAF-1). **Fix:** validate the style once in `style.fromValue`
(`wrap_chars` 4..200, heights 1/64..2 in, spacings > 0; `E_STYLE` naming the key), `const cap = @max(chars, 1)` in `wrap()`,
`if (!(step > 0)) return` in the two step loops.

### LAY-3 - major - OOM swallowed in layout state, desynchronising parallel arrays

* `route.zig:406-416` `snapshot()` uses `self.a.dupe(...) catch self.left` (same for `ci`, `key`, `ov`): on OOM the snapshot
  *aliases the live buffers*, `restore` becomes a no-op and the search silently keeps a worse layout. Return
  `Allocator.Error!Snap` (the callers already return errors).
* `route.zig:609-614`: `var tmp: [64]usize` with `if (o != i and m < 64)` silently drops notes beyond 64 per column when
  re-keying, leaving stale keys that collide with renumbered ones. Use an arena slice or `c.ord2`.
* `annot.zig:1244-1248`: `defer { per.append(a, its) catch {}; note_slot.append(...) catch {}; meta.append(...) catch {}; }`
  runs on every `continue`; if one append fails the three lists get different lengths and `note_slot.items[k]` (`:1437`) goes
  out of bounds (safe builds) or misassociates (release). Preallocate with `ensureTotalCapacity(a, n)` and
  `appendAssumeCapacity`, or build one `Slot` struct per annotation appended once.
* `iso.zig:103,105` `Iso.landing` uses `catch return null`/`catch null`, so OOM is reported as "target not visible"
  (`W_NOTE_TARGET`); `lint.keyList` returns a partial list (`:81-82`).

### LAY-4 - major - The auto-scale and iso retry loops multiply the whole pipeline

`drawview.zig:258-285,350-389`, `load.zig:287`. `resolveSection` runs a *full* `sectionPass` (section build, annotate with
up to 8 route passes, title) for each of up to 7 standard scales (measured 6.9 s vs 1.25 s for the explicit-scale view); the iso
loop runs up to 17 trials, each with a full HLR build plus annotate; `resolveSpec` (used by `inspect`, `load.zig:287`) repeats
the whole `resolveSection` and `build` then does it again; everything lands in the per-call arena, so memory grows with every
trial. **Fix:** choose the scale from a cheap estimate first (crop + notes extent from a "light" annotate with no repair
passes), run the full repair once at the chosen scale, return the resolved spec from `buildFromScene` (or cache it in `Loaded`)
so `resolveSpec` does not recompute, and use a scratch arena per trial copying out only the winner.

### LAY-5 - minor/major - Dimension stacking rebuilds full item lists for every candidate

`annot.zig:1015-1040,811-924`: `stackDims` tries up to 13 offsets x 6 variants per dimension and each `dimBuild` allocates a
full `ArrayList(Item)` (`fmtFtIn`, `asciiFold`, `font.width`, 7 `pathItem`s, a text item) for a candidate that is mostly
discarded, then everything is re-stacked on every repair pass (~1.7k `dimBuild` calls at 60 dims, 8.8k at 200 notes). Split
into `dimShape` (geometry + text width, allocation-free, `DimShape` is already a fixed-size struct) and `dimEmit`, search on
shapes, emit once; cache `label`/`tw` per `DimSpec`.

### LAY-6 - major - God-functions and layering

* `annot.annotate` (`annot.zig:1227-1441`, 215 lines) mixes annotation parsing and diagnostics (~120 lines), base-segment
  collection, the repair loop, box accounting and result assembly: split into `parseAnnotations -> Parsed`, `repairLoop -> Best`,
  `assemble`; `parseNote/parseDim/parseLabel` return `?Spec` and report their own diagnostics.
  `iso.build` (`iso.zig:477-735`, 259 lines): extract `dedupeEdges`, `emitHatch`, `chainPieces`, `buildVisShapes`.
  `drawview.viewFit` (`:103-190`) is a message builder and belongs elsewhere; `route.solveColumn` (118 lines): separate
  "enumerate slots" from the forward DP so the DP is testable alone.
* **Import cycles:** `iso.zig` <-> `annot.zig` (iso imports `annot.Shape`/`labelPoint` inline at `:74,98,105,654`; annot imports
  `iso.Landing`, `annot.zig:25`). `Shape`, `shapesOf`, `labelPoint`, `shapeContains`, `crossings`, `candidatesFor`,
  `bandedLabelPoint` are pure geometry: move to `landing.zig`. `mesh.zig:287` imports `section.isFillMaterial` (ARC-1); `schema.zig:10`
  imports `canon.zig` and never uses it, while `canon.zig:6` imports `schema` (a 2-file cycle created by an unused import).
* **Inline `@import`s** hide dependencies: `annot.zig:435`, `iso.zig:327-333`, `drawview.zig`, `lint.zig:417,420`, tests.
* `lint.zig` calls into the compiler: `withDimDirsFromDoc` does a whole compile (`lint.zig:417-421`) and `quietPoint` temporarily
  swaps `scene.diags` (`:306-312`); give `Scene.point` a `quiet` parameter. `withDimDirs`/`dominantDir` are document
  transforms, not lints, and use string directions while `annot.DimDir` is an enum: move to `dimdir.zig`/`view.zig`.
  `lint.dimMeasure` (`:319-323`) and `annot.DimSpec.span` (`:771`) implement the same rule twice.
* `drawview.zig` also owns diagnostics text (`viewFit`, `sideCulprit` ~90 lines of fix wording, `codeBasis`, `metaString`).

### LAY-7 - minor - Duplicated geometry in the layout code

`route.ptSegDist` (`route.zig:120`) re-implements `geom.distPointSeg` (`geom.zig:448`); `route.segsCross/onSeg` (`:102-118`)
re-implement `geom.segSeg` with an inline `1e-12` (fine as a fast non-exact version but name it `segsTouchFast`); `route.BB`
(`:170-197`) duplicates `geom.Box` (add `Box.ofPoints/apart`), plus hand-rolled overlaps in `annot.baseHits` (`:977-986`) and
`softPen` (`route.zig:326`); `annot.textPoly` (`:123-142`) duplicates the anchor/rotation maths of `textgeom.strokes`
(`textgeom.zig:11-35`); `drawview.itemBounds` (`:34`) is dead and differs from `annot.itemsBox`; `drawview.fmtNum` vs
`annot.fmtNum` (different rounding); `polysOverlap/polyHitsSeg/convexInterval/clearOfLeaders` (annot) and `polyBoxDist/boxPoly`
(route) all work on `[4]V2` (a small `quad.zig` removes ~80 lines); `sectionPass` (`drawview.zig:201-229`) and the iso trial
(`:350-389`) repeat annotate -> regions -> title -> bounds; `shapeDepth`/`shapeDepthNoCrop` (`annot.zig:277-308`) are two
copies of one loop; `repairLabel` (`:1186`) and `obstacleFix` (`:557`) duplicate an 8-direction array;
`route.NoteIn` and `annot.NoteIn` are different types with the same name (`annot.zig:477`, `route.zig:32`).

### LAY-8 - minor - Magic numbers

Paper-inch literals in annotation geometry (`0.04*S`, `0.14*S` `annot.zig:635-638`, `0.015/0.02*S` `:922,1129,1145`,
`0.25*S` `:997,1166`, `0.35*S`, `1.4*pitch` `:736-747`, `0.3125*S`, `0.4*S`, `0.07*S` `:1521-1550`, caps `24`, `12`, `8.0*step`);
route weights (`W_*` are named; `5, 0.4, 0.02, 0.5, 3, 1.5` in `softPen`/`shapePen` `route.zig:330,340-341` are not); `1e-9/1e-12/1e-6`
used in ~40 places with three meanings (same point / on a segment / strict inequality); `drawview` `1e-6` x4
(`:270,281,384,397`), `0.15`, `0.5`, `6.0` inline. **Fix:** `Tol` constants in `geom.zig` (ARC-2) and a documented
`LayoutParams` struct (many belong in `style.zig`, they are paper-space style constants). Naming: `W_CROSS`-style constants
sit next to `max_repair_passes`/`dp_cands`; std uses snake_case for constants. `Layout.searched` says "Number of crossings + near
hits" (`route.zig:85`) but is a `bool`.

### LAY-9 - minor - Iso, drawview and lint details

* Chain ids collide: `iso.zig:440,448,459` `pi*16 + li` (+8 for the bottom chain) collides at >= 8 loops, `pi*4096 + li*1024 + i`
  at `li >= 4` or `i >= 1024`; only wrong run-merging, but use a struct key `Chain{prism, loop, kind}`.
* Fill landing uses only `ip.loops[0]` (`:658-660`): notes on a fill with holes can land inside a hole. `iso.zig:719-723` has
  `_ = comp;` and `var fi: u32 = 0; fi = ip.instance;`; `:337-339` dead alias `fill_mat = fill_mat0`.
* The HLR grid is a fixed 24x24 regardless of face count (`iso.zig:146-151`): size it from the face count, clamped to [8, 96].
* `drawview.zig:305-310`: `findView` returns the first match but the `idx` loop has no `break`, so for duplicate view ids the
  default `number` (`index + 1`, `view.zig:134`) comes from the *last* match.
* `lint.zig`: `var fixed` appended to but never read (`:226-227`); `ackShape(a, scene, ...)` ignores `scene` (`:433`); the
  W_UNKNOWN_KEY message text is copy-pasted 4 times (`:95-102`); `mentions` returns false for needles over 128 bytes
  (`:459-461`); `checkNoteText` rebuilds the whole string per hit (`:253-261`, bounded by `guard > 50`); `1.0/16.0` three
  times in `dimZero` (`:341,351`).
* `annot.zig:614` `allocPrint("{s}", .{fx})` copies an arena-owned string; `:251-252,279,298` one-element array ceremony;
  `:1255-1257` linear duplicate-id scan (O(n^2)); `Env` accumulates layout results (`notes_box`, `dims_box`, `labels_box`,
  `unverified`, `unknown_glyph`): return an `AnnotResult`.
* `zig fmt --check src` fails on 8 files: `agents api iso ops proxy raster tests validate` (checked on the snapshot). Land one `zig fmt src` commit (whitespace only, goldens unaffected) and add the check to CI.

### LAY-10 - nit - Determinism of comparators

`stackDims` (`annot.zig:1004`) compares with `@abs(sx - sy) > 1e-9`, which is not a strict weak order for chains of near-equal
spans; deterministic for identical input but order may differ for inputs that differ by rounding: quantise
(`@round(span * 1e6)`) and compare integers. `iso.zig:676` face sort has no tie-break (stable sort keeps insertion order:
say so in a comment); exact `==` on f64 keys (`route.zig:249,601`) is fine because keys are integral ranks or `-y`.

### Layout: keep as is

The router's exact per-column DP (half-pitch grid, 6 candidates, +-12 steps) and its lexicographic (hard hits, hits, cost)
ordering in `betterMode`; `route.zig` is pure geometry with no document types (right dependency direction); early bounding-box
rejection in `pairPen/boxPen/softPen` and `pc >= best` pruning; `decross` renumbering keys by rank; the `light` mode with a
reduced budget for repair passes (extend it rather than replace it); every sort has a total tie-break and the NaN key `ov` is
tested; annotate/drawview/iso return `Allocator.Error!`, never `anyerror`, diagnostics carry `fix` text; `Obstacle/Meta/LabelSpec/DimSpec`
helper structs; `asciiFoldFlag` returns the input unchanged when it is already ASCII; hatch knockout via `convexInterval`;
iso edge dedupe by (key, idx) with "heaviest pen, earliest on ties"; the declarative `synonyms`/`spelled_out`/`period_abbrs`
tables in `lint.zig`; module headers that cite SPEC sections.

## 6. Exporters (`svg`, `dxf`, `pdf`, `raster`, `png`, `font`, `textgeom`, `sheet`, `mesh`, `drawing`)

The exporters are the cleanest part of the code base and only ~4% of the wasm. Findings are about hostile numbers and
a few latent issues.

### EXP-1 - major (low likelihood) - Dashed raster strokes are not clipped to the canvas and can spin

`raster.zig:159-205`, loop at `:186` (`while (pos < len)`): dashing walks the whole flattened segment dash by dash without
clipping to the canvas. A 1e9-inch dashed segment is billions of iterations; past ~2^53 `pos += take` stops advancing and the
loop never ends (also for dash lengths near the `1e-6` guard). On wasm this blocks the browser main thread. Measured
(Debug): 1 / 4 / 39 ms for 1e3 / 1e4 / 1e5 inches, i.e. linear and uncapped. Fix: clip each segment to the canvas grown by the pen
radius (Liang-Barsky, `pathclip.clipSeg` already exists) before dashing and advance the dash phase analytically across the
clipped-off part with `@mod(offset, pattern_sum)`. Hatch lines (`raster.zig:361-366`) use the same path.

### EXP-2 - minor - Number text: overflow swallowed, `nan`/`inf` emitted, five formatters

`svg.zig:37-42` (`bufPrint(&b, "{d}") catch "0"` into 40 bytes), `dxf.zig:46-49` (48 bytes), `drawing.zig:141`, `pdf.zig:16-20`.
Any |x| >= ~1e38 prints more than 40 digits so `catch "0"` silently writes `0`; NaN/inf print as `nan`/`inf`, invalid in SVG
(`<path d="M24 984L0 nan">` reproduced), PDF (`0000000000000000000 nan l`; a 1e300 coordinate printed 300 digits) and JSON.
`drawing.zig:136-143` tests `p.b != 0` before rounding, so a bulge of `-1e-9` is written as `-0`. The five formatters are
`svg.num` (3 dp), `pdf.num` (3 dp), `dxf.fmtF` (6 dp), `json.fmtNumber` (4 dp, integer loop), and the bulge writer
(6 dp); the coordinate transform is also written three times (`svg.Map`, `pdf.Cs.px/py`, raster). **Fix:** one
`numfmt.fixed(buf, x, comptime decimals)` generalising the integer-scaled path of `json.fmtNumber` (non-finite -> 0, clamp
to +-1e9, never `catch "0"`); determinism is unaffected (`{d}` in 0.16 is the pure-Zig shortest printer, byte-identical
native vs wasm today).

### EXP-3 - minor - DXF strings and handles

* `dxf.zig:194` `w.s(1, t.s)` and `s()` (`:25`) print `"{d}\n{s}\n"` raw: a `\n`/`\r` in note text injects group codes;
  text over 255 chars is not truncated; `%%d`/`%%c`/`%%p` are interpreted by CAD. `annot.asciiFoldFlag` (`annot.zig:55-61`) only
  touches bytes >= 0x80. Layer names (`dxf.zig:86,381`) are unsanitised although DXF forbids `<>/\":;?*|=\``; `linetypeName`
  (`:68`) uppercases a user pen name verbatim. Fix: one `sanitize()` inside `Writer.s` (control chars -> space, cap 255).
* Handles: LTYPE `0x40 + idx` (`:337`), layers `0x50 + idx` (`:377`), entities from `0x100` (`:20`): more than 16 dashed pens
  collides with layer handles, more than 176 layers with entity handles. Allocate every handle from the one counter.
* SVG/PDF/PNG emit only declared layers; DXF emits every item (`dxf.zig:~97`), an undeclared layer is a DXF audit error.

### EXP-4 - major (API) - Public `render`/`toJson` return `out.items`

`svg.zig:198`, and the same in `pdf.render`, `dxf.render`, `drawing.toJson`, `mesh.toJson`, `textgeom.strokes`: the slice is shorter
than the list capacity, so it is only safe with an arena (how `kerf.call` uses it). `NOTES.md` advertises `kerf.svg.render` and
`kerf.mesh.build` for in-process use: a caller with a real allocator that frees the result trips a size-mismatch assertion in
Debug or leaks. Return `try out.toOwnedSlice(a)` or document "arena required" on every function (and see IDM-7).

### EXP-5 - minor - Raster: memory, per-item allocation, quadratic scans

* A maximum page is 6000x8192 (`raster.zig:28,323`): `pix` and `cov` ~49 MB each (`:65`), plus `filtered` (`png.zig:95`) and the
  compressor state. Wasm memory never shrinks, so one big export leaves the module at 200 MB+. Stream rows into
  `flate.Compress` inside the filter loop (drops `filtered`), free `cov` before encoding.
* `penDashPx` allocates a dash array per item/hatch group/text (`raster.zig:343,361,372`): cache per pen.
* `fillEdges` scans every edge for each of 8 sub-scanlines per row (`:252`) and sorts per sub-scanline; `flush` walks all rows
  after every item (`:81-96`): use an active-edge list and a dirty y-range. Currently fine (ReleaseFast truss sheet: svg 24 ms, pdf 9,
  dxf 7, png 1600 px 61 ms, png 6000 px 477 ms).
* `svg.zig:112-130`: grouping scans the rest of `d.items` for every unemitted item, O(items x distinct srcs x layers): one pass
  bucketing by (layer, first-seen src).

### EXP-6 - minor - PNG and PDF details

* `png.zig:89-92` checks `width > 1<<24` then multiplies `stride * height` in `usize`: on wasm32, 2^26 x 2^24 wraps to 0 and
  a caller with an empty slice passes the check and then indexes out of bounds with safety off. Use `std.math.mul`. `decode`
  (`:173`, test-only) has the same unchecked multiply.
* PNG bytes depend on `std.compress.flate`; a Zig upgrade can silently change them and break the PNG goldens. Pin one
  expected-bytes test; tests only round-trip through the project's own decoder.
* PDF: a bare detail has no margin (`pdf.zig:108-109`; SVG/PNG use 0.25 in; `api.zig:222` always applies a sheet so only library
  callers hit it); `setPen` re-emits `w`/`d` per item (`:116,126,139`); content stream uncompressed (146 KB on the truss sheet);
  `_ = e0;` dead (`:76`); `@intFromFloat` at `:66` is safe only because the sweep is `4*atan(b) < 2*pi` and UB if `b` is NaN.
  Xref offsets, `/Length` and trailer are correct, the Info dictionary is static.
* SVG escaping (`svg.zig:92-101`) is correct for `& < > "`; text is drawn as strokes so notes cannot inject markup; control
  characters (illegal in XML 1.0) and invalid UTF-8 pass through, making the file ill-formed.

### EXP-7 - minor/nit - mesh, font, sheet

* `mesh.zig`: `cmuSplit` (`:262`, used by `iso.zig:333`) is re-implemented inline in `build` (`:310-322`) with a slightly different
  joint condition; the mortar `part` is `""` while the main part uses null (JSON differs); `extrude` re-cleans and re-ear-clips
  the loop for every CMU unit (`earClip` is O(n^3) worst case, `:67-77`); `pr.loops[0]` unguarded (`:305`); `tube` (`:213`)
  normalises a possibly-zero tangent (NaN -> printed as `0`); indices written with `out.print("{d}")` one by one (`:361`).
* `font.zig`: `adv == 0` is the "missing glyph" sentinel (`:46`, use an optional), `orelse 0` hides bad coordinates (`:95`),
  `Font.parse` re-parses the 10.9 KB JSON on every export (`api.zig:215`, `drawview.zig:293,337`), `strokesOf` allocates per stroke.
* `sheet.zig:19-20` `Ctx.d`/`Ctx.font` are never read (and `d` points at a by-value parameter); `fitH` (`:61`) floors at 0.6 and
  lets long titles overflow the cell. `textgeom.zig:1` has a stale comment.

---

## 7. Server: `serve.zig`, `http.zig`, `agents.zig`, `proxy.zig`, `events.zig`, `workspace.zig`, `main.zig`

Method: read in full, then probed a Debug and a ReleaseSmall build of `kerf serve` on temporary folders with curl, raw
sockets and fake agents. The real `claude`/`grok`/`codex` binaries were never started. Everything below marked
"reproduced" was observed; the line numbers are from the frozen snapshot.

### V-1 - critical - One unauthenticated chunked request kills the server (wraps silently in release)

**Status: Fixed (harden(serve), batch 3 + V-1). Chunk sizes are at most 8 hex digits and compared against the remaining cap before adding; unit test `V-1: a huge chunk size...` plus a raw-socket regression in `serve_smoke.mjs` (it kills the pre-fix Debug server).**

`http.zig:142`: `if (body.items.len + size > max_body)`. `size` comes from `parseInt(usize, hex, 16)`, so
`ffffffffffffffff` is accepted and any earlier chunk makes the sum overflow. `serveConn` reads the body (`serve.zig:362`)
*before* `dispatch`/`checkAccess`, so no token, Host or Origin is needed.

* Debug/ReleaseSafe (reproduced): `POST /api/docs` with `Transfer-Encoding: chunked`, one 1-byte chunk, then
  `ffffffffffffffff` -> `panic: integer overflow ... http.zig:142:28 in readChunked`; the whole process dies, with every
  SSE client and the agent supervisor.
* ReleaseSmall (what ships, reproduced): the arithmetic wraps, the connection drops, the server stays up, but the wrapped
  value is attacker-chosen and feeds `readAlloc`.

```zig
if (size > max_body - body.items.len) return error.TooLarge;   // never add before comparing
```

Parse the chunk size as `u32`/bounded first; add a `readChunked` unit test with a huge size.

### V-2 - major - Opening a folder runs arbitrary commands from `.kerf/agents.json`

**Status: Fixed. `.kerf/agents.json` entries are executed only with `kerf serve --trust-agents` or an interactive yes on a TTY; otherwise listed `untrusted`/`available:false`, never detected or started, and they cannot replace built-in ids. Built-in detection is `<binary> --version` only (unit test). Regression: `V-2:` checks (marker files never created). Not done: a per-folder remembered allow-list.**

`agents.zig:82` (`loadTemplates` reads `<dir>/.kerf/agents.json`) and `:158` (`detect` spawns `t.detect`);
`serve.zig:1068/1236` (`prefetchAgents`) does it at startup, and `/api/info` re-detects every 30 s. Reproduced: a folder
with `{"id":"evil","detect":["touch","/tmp/pwned"],"argv":["true"]}` creates `/tmp/pwned` as soon as `kerf serve --dir
that_folder` starts. Same trust problem as `.vscode/tasks.json` or git hooks: cloning a details folder and opening it runs
code. **Fix:** run only built-in templates until the user opts in (`--trust-workspace-agents`, or a per-folder hash
allow-list under `~/.config/kerf/`); list workspace agents as `available:false, reason:"untrusted"`.

### V-3 - major - "One agent run at a time" is a TOCTOU race; stop/timeout/reaping are unreliable

**Status: Fixed. Slot reserved atomically before spawn (6 concurrent starts -> one 200, five 409), own process group (Windows: job object, degraded to the agent alone if the job cannot be made), SIGTERM then SIGKILL after 3 s for the whole group, `--agent-timeout`, reaping concurrent with the pumps, SIGINT/SIGTERM/SIGHUP stop the run. Regressions: `V-3:` checks (race, tree kill, orphaned grandchild, SIGTERM-ignoring agent, timeout, server SIGTERM). A SIGKILLed server still orphans the agent on POSIX (documented).**

* Race (`agents.zig:274-325`): the busy check, `detectCached` (up to 8 s cold) and `m.active = run` are separate critical
  sections. Reproduced: 6 concurrent `POST /api/agent/run` -> 5x `200`, 1x `409`, five child processes alive, only the
  last reachable by `/api/agent/stop`. Reserve the slot under `mu` before spawning (placeholder `Run`), run detection first.
* `stop` only SIGTERMs the direct child (`terminate`, `:358`); `pumps.await` (`:383`) waits for pipe EOF *before*
  `child.wait` (`:392`). Reproduced: a bash agent with a `sleep 30` child stays "active" after `stop` returned
  `stopped:true`; an agent that ignores SIGTERM runs forever (no SIGKILL escalation); a parent that exits while a
  grandchild holds the pipe stays a `<defunct>` zombie for 40 s and returns `E_BUSY` meanwhile; there is no overall run
  timeout; SIGTERM of the server leaves the agent reparented to pid 1.
* **Fix:** spawn the agent in its own process group (`pgid` in `std.process.SpawnOptions`; check the 0.16 field), kill
  `-pgid` (TERM, then KILL after ~3 s) from the supervisor, reap with a `wait` that runs concurrently with the pumps, add
  `max_run_seconds`, and install a SIGINT/SIGTERM handler that stops the active run.

### V-4 - major - No connection, header, body or idle limits (slowloris, memory)

**Status: Fixed. 256-connection cap (503), watchdog deadlines (idle, head, body, handler, SSE write), request cap per connection, body buffer grows as data arrives. Regressions: `V-4:` checks (trickled head, idle socket, stalled body, 270 sockets, 40 x 60 MiB declared bodies < 60 MB RSS).**

`acceptLoop` (`serve.zig:1048-1066`) creates one OS thread per connection (`group.concurrent`; the default
`Io.Threaded` limit is unlimited) with no read/write/idle timeout (`NOTES.md` admits it). Reproduced (Debug):
2000 half-open connections -> 2002 threads, 33 GB VmSize, 1.18 GB RSS, and the thread pool never shrinks; 1000 on
ReleaseSmall ~560 MB RSS; 100 connections each declaring `Content-Length: 64 MiB` and sending nothing -> 5.6 GB RSS in
Debug because `readAlloc(a, content_length)` (`http.zig:123`) allocates the declared length up front. `/api/info` still
answered, 10-100x slower. **Fix:** cap concurrency (`Threaded.concurrent_limit` ~256; the 503 path exists), per-connection
deadlines (`SO_RCVTIMEO`/`SO_SNDTIMEO` through `std.posix.setsockopt`), cap keep-alive requests, grow the body buffer as
data arrives.

### V-5 - major - Bodies are read before auth/routing, with a single 64 MiB cap

**Status: Fixed. Route -> authenticate -> read body with per-route caps (stop 4 KiB, create 64 KiB, apply 16 MiB, llm 32 MiB, agent/run 64 MiB, GET 0); no `100 Continue` and a closed connection for anything refused before its body; trailers bounded. Regressions: `V-5:` checks, unit test `routes are decided before any body is read`.**

`serve.zig:362` vs `:369`; `100 Continue` is sent (`http.zig:112,119`) before any access check; chunked trailers
(`http.zig:152-155`) loop without a bound. Split `serveConn` into parse-head -> `checkAccess`/route -> read-body with a
per-route cap (apply/create/stop 1-8 MiB, `/api/llm` a few MiB, `/api/agent/run` up to the image budget).

### V-6 - major - Agent `message` limit exceeds the OS per-argument limit

**Status: Fixed. Message <= 100,000 bytes (400 `E_INPUT`), OS "argument too long" at spawn is 400. Regression: `V-6:` checks.**

`serve.zig:876` allows 200,000 characters; the message is one argv element and Linux `MAX_ARG_STRLEN` is 131,072.
Reproduced: 150,000 chars -> `500 E_SPAWN`, 100,000 works. Cap at ~100 kB with a `400`, or pass long text via a temp file
or stdin (templates currently set `.stdin = .ignore`); map spawn errors to `4xx`.

### V-7 - major - Default no-token loopback mode plus permissive agent templates

**Status: Fixed. A token is generated by default, also on loopback (`--no-token` opts out), printed in the banner URL; `KERF_TOKEN` is not passed on to agents. Regression: `V-8: a token is generated by default...`.**

`agents.zig:33-71`: `grok` uses `--always-approve`, `claude` allows `Write`/`Edit` under `acceptEdits`. With the default
no-token loopback server any local process or user can `POST /api/agent/run` and drive an autonomous agent as the
server's user, and the prompt is influenced by document and note text (prompt injection). The Origin/Host checks do stop
*browsers* (verified), but not other local clients. Always generate a token (print it in the banner / `?token=` URL), even
on loopback.

### V-8 - minor - Token handling

**Status: Fixed. `randomSecure` (fail closed), `KERF_TOKEN` and `--token-file` (and a `ps` note for `--token`), `Referrer-Policy: no-referrer`, SHA-256 + `timing_safe.eql`, CSP incl. `frame-ancestors none` (UI: script hashes of the embedded inline scripts) and `X-Frame-Options`. Regressions: `V-8:` checks.**

`io.random` (`serve.zig:1155`) is documented as possibly falling back to a weaker source: use `io.randomSecure` and fail
closed; `--token T` is visible in `ps` (observed on this machine: the owner's running `kerf serve --token ...` shows its token to every local user; accept `KERF_TOKEN` or a token file); `?token=` leaks into logs/Referer (add
`Referrer-Policy: no-referrer`); `secretEql` (`http.zig:281`) is fine but prefer `std.crypto.timing_safe.eql`; add
`Content-Security-Policy`/`frame-ancestors 'none'` (same-origin XHR from an iframe passes the Origin check).

### V-9 - minor - Lax HTTP parsing (matters behind a proxy)

**Status: Fixed together with V-1: strict `parseHead` (unit test `V-9: strict head parsing...`, raw-socket checks in `serve_smoke.mjs`).**

`http.zig:29-37,90-102`: `Content-Length : 5` (space before the colon) is ignored and the body parsed as the next request;
duplicate `Content-Length` takes the first; `Transfer-Encoding: chunked` + `Content-Length` resolves to chunked; spaces in the
request target are accepted. Reject all four, header names with whitespace, and bare `\n` in the head (a strict
`parseHead` is trivially unit-testable).

### V-10 - minor - Symlinks in the served folder are followed

**Status: Fixed. Symlinks are not listed, read, exported or written through (documents, op log, `agents.json`, `.kerf/attachments`, which must be a real directory and is written with exclusive creates). Regressions: `V-10:` checks.**

`scanLocked` (`serve.zig:107`) accepts `.sym_link`; `readDoc`/`statFile` follow it. Reproduced: `ln -s /etc/hostname
w1/evil.kerf.json` then `GET /api/docs/evil.kerf.json` returns the target. A prompt-injected agent can create such a link.
Open with `.follow_symlinks = false` / `O_NOFOLLOW`, skip symlinks in the scan, check `.kerf/attachments` too (`:939`).

### V-11 - minor - Durability and lost updates

**Status: Mostly fixed. fsync of data and directory, `flock` + fsync on log appends (`DocLock`, held across the server's read-modify-write), content-hash ETag, `if_match` of a non-string is 400. Open (CLI, `main.zig`, engine agent): `kerf apply -w --if-match` / `DocLock` in the CLI and a non-atomic `kerf fmt -w`; requested in NOTES. Regressions: `V-11:` checks.**

* `workspace.writeFileAtomic` (`workspace.zig:55-60`) does not `fsync` the file or the directory around the rename.
* `appendLine` (`workspace.zig:69-81`) reads the length then `pwrite`s, not `O_APPEND`/`flock`: the "concurrent appenders
  cannot split a line" comment is wrong (not reproduced in 40 parallel appends, but real).
* Reproduced: 40 parallel `kerf apply -w` on one document all succeed and all log, but only one writer's value survives.
  The CLI has no compare-and-swap; add `--if-match <etag>` or a lock file.
* ETag (`serve.zig:311`) is `mtime_ms-size`: two same-size edits in one millisecond collide; hash the bytes on the apply path.
* `kerf fmt -w` (`main.zig:614`) is a truncating non-atomic write and writes no log entry.
* `if_match` of a non-string is silently treated as absent (`serve.zig:670`), making the write unconditional: return `400`.

### V-12 - minor - `/api/llm` proxy

**Status: Fixed for the stall/concurrency parts: `--llm-timeout`, 30 min total and 256 MiB caps, at most 8 calls in flight (429), stalled upstreams are cancelled. `custom` still reaches any host by design. Regressions: `V-12:` checks.**

By design `provider:"custom"` can reach any host (including `169.254.169.254` and loopback) with a chosen body and
headers; token-gated, but in no-token mode any local process can use it. No upstream timeout, no cap on concurrent proxy calls
or streamed bytes (a slow upstream pins a thread). The URL rules, CRLF/header-name checks and blocked-header list are good.

### V-13 - minor - SSE hub and poller

**Status: Fixed. SSE writes have a deadline (watchdog), CR/LF cannot split a frame (agent events are re-serialised, `Hub.publish` neutralises the rest), an oversized log line is skipped, `apiList` runs `check` outside `scan_mu`. Regressions: `V-13:` checks and a hub unit test.**

`apiEvents` blocks in `w.writeAll` without a send timeout (a client that stops reading wedges its thread);
`emitLine`/`publishLog` forward JSON text as raw SSE `data:` so a lone `\r` splits the frame (normalise through the
writer); `readNewLog` (`serve.zig:221`) re-reads 4 MiB every 500 ms forever when a line exceeds 4 MiB; `apiList` holds
`scan_mu` while running the engine `check` on every changed doc (`:575`).

### V-14 - minor - Allocator and error discipline in the server

**Status: Mostly fixed. `detectCached` is single-flight and leak-free, attachments older than 24 h are removed, `px` is printed as the parsed number, the pump drops a line on OOM instead of truncating, `cors()` reports OOM. Open: error logging for `publish`/`scanLocked` OOM and `serveConn`'s silent close on read errors.**

Per-request arena is good. Swallowed errors in network paths: `catch {}`/`catch return` on `publish`, `scanLocked` OOM,
the pump's `line.appendSlice(...) catch {}` (silent truncation), `serveConn`'s `else => {}` on read errors, `cors()` returning
`""` on OOM. `detectCached` has no single-flight (concurrent `/api/info` calls each spawn up to four detect processes of up to
8 s) and leaks the old strings "on purpose" (`agents.zig:238`). Image attachments are never deleted (`saveImages`,
`serve.zig:934`). `px` is echoed raw (`serve.zig:807-809`): print the parsed integer, `+5` currently yields invalid JSON.

### V-15 - minor - Windows names

**Status: Fixed. Device names (`CON PRN AUX NUL COM1-9 LPT1-9`, also with an extension) and stems ending in a dot or space are rejected (unit test, `V-15:` checks).**

`workspace.validDocFile` (`:38`) blocks separators, `:` (ADS), leading `.`, non-ASCII, NUL: good. It accepts `CON.kerf.json`
(device name on Windows); reject `CON PRN AUX NUL COM1-9 LPT1-9` stems and trailing dot/space.

### V-16 - nit - Structure

**Status: Partly fixed (server side): HEAD reports the real Content-Length, routing is a pure, unit-tested `routeOf` + `bodyCap`. Open: handler JSON assembly, a comptime route table, and the `main.zig` items (engine agent).**

`main.run` is a 368-line if-chain with an 800-character one-line option parser (`main.zig:179`) and builds engine input by
raw `{s}` interpolation (`:535,542`): `--view 'A","sheet":true'` injects JSON keys (escape with `json.writeString`);
three JSON escapers (`http.jsonString`, `main.appendJsonString`, `json.writeString`, SAF/ARC-2); `serve.zig` builds JSON by
hand in many handlers (`apiApply` ~90 lines mixing validation, locking, engine call and write) and routes through an
if-chain that would be clearer as a comptime route table; only `scan`, `checkAccess` and `plan` are unit-testable because
handlers need a real `Io`+directory; `static()` answers HEAD with `Content-Length: 0` for embedded assets; `cliMain`
never returns, so `defer`s and `Manager.deinit` are dead and `deinit` does not kill an active run.

### Server: keep as is

Per-request arena inside the connection loop; `validDocFile` whitelist and tests (`..%2f`, `%2e%2e`, `\`, absolute-URI
targets, encoded NUL all `400/404`); Host/Origin checks (evil Host, Origin != Host, rebinding shape all `403`; a token on
loopback overrides Host; Bearer is case-insensitive); CORS off unless exact `--allow-origin`; 20 KB header line -> `431`,
oversized `Content-Length` -> `413`, bad length -> `400`; JSON nesting up to 3M brackets in an apply body -> clean `400`
(the 200-depth cap in `json.zig` earns its keep); atomic temp-file+rename writes and a consistent `write_mu` -> `scan_mu`
lock order; bounded hub ring (a slow SSE client loses old frames, not memory); `{message}` is one argv element with a
fixed prefix, `session_id`/`file` strictly validated, 1 MiB output-line cap; proxy URL rules are thoroughly unit-tested;
`tests/serve_smoke.mjs` as an end-to-end gate.

## 8. Performance and wasm size

### PRF-1 - measurement summary (ReleaseFast, aarch64, 12 cores, loaded machine)

| Case | Time | Peak RSS |
|---|---|---|
| reference docs, section/iso SVG sheet | 13-53 ms (process incl. start) | ~10 MB |
| `check` truss | 16 ms | 10 MB |
| PNG sheet 1600 px / 6000 px | 62 ms / 477 ms | 11 MB / big (EXP-5) |
| synthetic `concrete` polygon, 1,600 / 6,400 / 25,600 vertices, hatched, section SVG | 34 / 74 / **852 ms** | 10 / 11 / 28 MB |
| `cmu_wall` 200 courses | 21 ms | - |
| 3,000 lumber components, `check` | 0.5 s | - |
| JSON object with 100,000 keys (1.3 MB) | **16 s** | - (SAF-3) |
| 40 components x `array.count` 500 (20,000 instances), `check` / `export` | **22.6 s / 18 s** | **1.8 GB** / 229 MB (SAF-5b) |
| 100 / 200 notes with dims and labels | **15 s / 187 s** | - (LAY-1) |

Conclusion: on the reference documents and at realistic sizes there is no hot spot, so there is no case for restructuring
algorithms *now*. There are four input-size cliffs that hostile or LLM-generated input can hit: annotation count (LAY-1),
total instances (SAF-5b), JSON object keys (SAF-3) and polygon vertices (the `clip`/`hatch` rows below). Each is fixable locally
without changing output:

| Where | Complexity | Suggested change |
|---|---|---|
| `json.zig:274` duplicate keys | O(k^2) per object | index for k > 16 (SAF-3) |
| `compile.zig:105-110,147-182` | dup-id check O(n^2), deps O(n^2 d), Kahn O(n^2 d) | `StringHashMapUnmanaged(u32)` id -> index for lookups only (never iterated: deterministic); ready-queue Kahn |
| `scene.find` (`scene.zig:106`) | linear per ref | same id index in `Scene` |
| `clip.boolean` (`clip.zig:69-268`) | pairwise edge pairs with bbox cull; classification `locate`+`sameDirection` O(n) per sub-edge; duplicate-edge and reversed-pair cancel O(k^2) (`:239-262`) | sort directed edges by `(from, to)` and merge; fold the diff pass (below) |
| `hatch.generate` (`hatch.zig:21-120`) | lines x edges (every hatch line scans every loop edge) + per-line `ArrayList` allocation | active-edge table bucketed by `s`; hoist `ts` out of the loop with `clearRetainingCapacity` |
| `hatch.generateGrain` | 3 `locateEvenOdd` per sample x edges | probe against the already clipped per-line intervals instead |
| `section.dedupe` (`:944`) | all segment pairs, bbox first | sort by `bb.x0`, sweep |
| `section.chainStrokes` (`:1073`) | restarts the double loop after every join and `orderedRemove`s: O(n^3) worst case | endpoint index (sort by rounded endpoint), single pass |
| `section.visiblePieces/visibleOpen` | prisms x occluders x edges per segment | box cull already present; fine |
| `validate.overlaps`/`floating` | all prism pairs (box test first), per instance | sort instances by `x0` and sweep (SAF-5b) |

Redundancy worth removing in `clip.boolean`: the `.diff` operand-B pass (`:226-237`) re-runs `geom.locate` for every B
sub-edge to recompute exactly what the classification loop above already decided (`keep = (loc == .inside)`) and only
adds the `swap(from, to)`; fold the swap into the first loop (one `locate` less per B edge, identical output).

### SIZ-1 - wasm: where the 866,509 bytes are

Current tree: **866,509 B raw / 322,454 gzip / 256,550 brotli** (NOTES.md still says 662,838; ReleaseFast 1,866,471; the name
section adds ~2.2 MB of DWARF if not stripped). Function code 746.6 KB (86%) + rodata 121.5 KB (14%). Attribution from
twiggy on the name-bearing build (inlined callees count toward their caller):

| Module | Bytes | | Std area | Bytes |
|---|---|---|---|---|
| builders | 117,236 (`build` alone 64.6 KB) | | `sort.block.*` (14 instantiations) | 26,293 |
| annot | 77,174 | | `array_list` (64 inst.) | 17,981 |
| api | 66,312 | | `fmt` (`allocPrint` 20 x 7 KB, `parseFloat` ~8 KB) | 17,188 |
| iso | 47,406 | | `mem` (`sort` 6.3 KB, 76 `alloc/dupe`) | 15,182 |
| section | 44,377 | | `compress.flate` | 9,471 |
| clip | 30,315 | | `compiler_rt` | 7,847 |
| compile | 30,204 | | `Io.Writer` (`printValue` 3.8 KB) | 7,157 |
| validate | 25,265 | | | |
| ops | 23,813 | | all std | 106.1 KB (12%) |
| drawview / dxf / lint / json / route | 20.7 / 18.5 / 18.1 / 18.0 / 17.4 KB | | all Kerf code | 640.5 KB (74%) |

All exporters except DXF together are ~4% (svg 1.5, pdf 1.8, raster 2.5, png 0.5, font 4.4, sheet 4.0, mesh 3.8 KB).
Rodata: 121.5 KB, of which ~83 KB is printable text (diagnostic messages, format strings) and ~38 KB binary tables; the embedded
style JSON is 11.7 KB (6.4 KB minified), the font 10.9 KB. In the full module float *printing* is only ~1 KB (twiggy) and `parseFloat` code ~8 KB; but in an isolated `wasm32` probe
of just the JSON parser+writer, removing `std.fmt.parseFloat` (code plus its Eisel-Lemire tables and big-decimal fallback) dropped
54.7 KB to 28.9 KB, so the real saving from replacing it is between ~8 KB (what twiggy attributes) and ~25 KB (what the probe
shows); measure on the full module before promising either (IDM-4).

Experiments (each on its own copy; goldens only re-run for the sort one, 24/24 pass):

| Change | Raw | vs 866,509 | gzip | brotli |
|---|---|---|---|---|
| baseline | 866,509 | | 322,460 | 256,550 |
| `std.mem.sort` -> `std.sort.insertion` (stable) | 753,923 | **-13.0%** | 282,296 | 230,280 |
| `std.mem.sort` -> `sortUnstable` | 787,103 | -9.2% | | |
| PNG deflate -> stored-block zlib | 856,092 | -1.2% | | |
| `wasm-opt -Oz` (binaryen v123 at `~/.cache/trunk/wasm-opt-version_123/bin`) | 764,547 | -11.8% | 312,637 | 252,569 |
| insertion sort + `wasm-opt -Oz` | 665,351 | -23.2% | 273,380 | 226,396 |
| `parseFloat` removed (json probe only) | -25.8 KB | ~-3% | | |

Recommendations by return: (1) stop instantiating the generic block sort 14 times: one non-generic stable merge sort over
`[]u32` indices with a comparator, or insertion for n <= 32 + pdq above (measure n at `clip.zig:323,385`, `route.zig:247`):
-80..110 KB raw, -40 KB gzip. Stability matters for determinism, so keep a stable algorithm. (2) optional `wasm-opt -Oz` build step
when the binary is on PATH (-100 KB raw but only -4 KB brotli: mainly a parse-time win). (3) `dxf.render`'s fixed tables as a
comptime string: ~400 `try o.s(...)` calls are 16 KB, -> ~3 KB and faster. (4) replace `parseFloat` (-8..25 KB, see above). (5) ~20
`allocPrint` instantiations (7 KB) + `print`/`printValue` (7 KB): small concat helper (-5..8 KB). (6) minify the embedded style in
`build.zig` (-5.3 KB). (7) PNG behind a comptime flag if the web app can rasterise SVG itself (-13..15 KB; product decision).
(8) Splitting `builders.build` (64 KB in one function) lets dead branches go; do it with IDM-1.

### SIZ-2 - native sizes

CLI without UI, aarch64: ReleaseSmall 1,321,104 B; ReleaseSafe 15,010,240 B unstripped / 2,507,968 B stripped. Shipping
ReleaseSafe for release binaries (SAF-2) costs ~1.2 MB per target, which is noise next to the embedded UI.

## 9. Prioritised refactor plan

Rules for every batch: `zig build test --summary all` green, `tests/check_golden.sh` prints `golden: OK` (byte-identical
svg/dxf/pdf/png/drawing/mesh/summary for the five reference documents), `node tools/zig-engine/wasm_golden.mjs` 24/24, `node
tests/serve_smoke.mjs` green, `python3 tools/zig-engine/fuzz.py 300 1` clean. Effort is for one engineer (or one focused
agent) including review; "risk to goldens" is the chance that a batch changes output bytes by accident.

| # | Batch | Findings | Effort | Risk to goldens | Lands with byte-identical goldens because |
|---|---|---|---|---|---|
| 0 | **Stop the bleeding** (do first, in this order, each its own commit) | V-1, SAF-1 (+ the appendix B sites), SAF-3, SAF-5, SAF-5c (`parseFinite`, scale/slope/crop/`run` bounds), SAF-5b (`limits.zig`: components, instances, annotations, points, JSON bytes), LAY-1 (lazy `proposeFix`, global work budget), LAY-2 (style validation), SAF-2 (`no_panic`, ReleaseSafe wasm CI gate, ReleaseSafe release CLI), TST-2 (goldens + wasm golden + fuzz in CI) | 3-4 days | none | only error/overflow/over-limit paths change; the five reference documents and the 12-note layouts never reach them (assert with `check_golden.sh`) |
| 1 | **`zig fmt src` (own commit, whitespace only), dead code and duplicates** | IDM-6 (13 `_ = x`, `and false`, `or true`, no-op `if`s), ARC-2 (`ftin`, `fmtNum`, `flat_tol`, `arcSteps` x4, JSON escaper x3, `[x,y]` pair parser), EXP-2 (`numfmt.fixed`), merge `visiblePieces`/`visibleOpen`, fold the `clip.boolean` diff pass | 1 day | low (number formatter and `visible*` merge are the only places output could move) | pure refactors; `numfmt` is checked against the goldens first; keep the old function behind a test until the diff is empty |
| 2 | **Diagnostics plumbing** | SAF-6 (`Diags.oom`, `Params.failCode`), SAF-7 (canon scratch), SAF-4 (silent defaults -> diagnostics, `units.parsePair`), SAF-5 (bulge/scale ranges) | 1 day | none | only invalid inputs change; add one `E_*` message test per new diagnostic |
| 3 | **Server hardening** | V-2..V-15 (trust prompt for workspace agents, process groups + kill escalation + timeout, connection/body/idle limits, route-first body reading, strict `parseHead`, symlinks, fsync + `O_APPEND`/lock, `--if-match`, token on loopback, CSP) | 3-4 days | none (engine untouched) | server code only; extend `serve_smoke.mjs` with the reproductions in section 7 as regression tests |
| 4 | **Enums instead of strings** (DONE, section 11) | IDM-2 (`Fn`, `catalog.Type` + comptime builder table with completeness check, `Pen`), `drawing.Kind` enum | 1-1.5 days | none | mapping only; the table is generated from the same names |
| 5 | **Typed params** (DONE, section 11) | IDM-1 (`Params.parse(Struct)`; convert builders one per commit: `panel`, `lumber`, `cmu_wall`, `concrete`, ...), ARC-4 (catalog names as slices, generated param table) | 4-6 days | low | each converted builder is checked against the goldens and `cli_ergonomics.mjs` message snapshots; messages are kept verbatim |
| 6 | **Domain flags instead of names** (DONE, section 11) | ARC-1 (`Material.role`, `catalog.Entry` flags), break the `iso <-> annot`, `mesh -> section`, `canon <-> schema` import cycles (extract `shape.zig`) | 1-2 days | low | defaults reproduce today's name lists exactly; unit test asserts the role table equals the old predicates for every embedded material |
| 7 | **Split the big files** (pure moves) (DONE, section 11) | ARC-3: `compile` stages, `ops` per root, `builders/` directory, `section/` directory, `annot/`, `serve` handlers + route table, `main.run` subcommand functions | 2-3 days | none | `git mv` + `@import` rewrites only; do one file per commit so `git blame` survives |
| 8 | **Performance and size** | PRF-1 table (id index, `clip` edge-set sort, hatch active-edge table, `dedupe` sweep, `chainStrokes` index), SIZ-1 (single stable sort instantiation, `dxf` tables, `parseFloat`, `allocPrint` helper, optional `wasm-opt`), EXP-1 (dash clipping), EXP-5 | 3-4 days | medium for `clip`/`dedupe` (ordering of equal elements) | keep stable ordering by `(key, original index)`; the existing goldens and `layout_tests.zig` catch any reordering |
| 9 | **Exporter tidy** | EXP-1, EXP-3 (DXF sanitising/handles), EXP-4 (`toOwnedSlice`), EXP-5, EXP-6, EXP-7 | 1.5 days | low (DXF handle renumbering changes bytes: regenerate DXF goldens once, review with `dxf_check.py`; this is the only batch allowed to touch goldens, do it alone) | n/a: explicit golden regeneration |
| 10 | **Tests and tooling** | TST-1 (`Fixture`), TST-2 (in-tree `std.testing.fuzz`), message snapshot tests, `SIZES.txt` in CI, `.preferred_optimize_mode = .ReleaseSafe` | 1.5 days | none | tests only |

**Status of batch 3 (server hardening) and V-1:** landed in the `harden(serve):` commits (V-1 first, with its regression test); V-2..V-16 are fixed or mostly fixed as
recorded under each finding in section 7. What is left sits in `main.zig` (engine agent): CLI `--if-match` / `DocLock`, `kerf fmt -w`, `--view` JSON injection, serve usage text.
CI (`.github/workflows/`): golden gate, CLI ergonomics, serve smoke (Debug and ReleaseSafe), fuzz, wasm golden, cross-compiles and a ReleaseSafe release CLI are in; the
`zig fmt --check` and ReleaseSafe-wasm steps are TODO comments until the engine agent's fmt and `no_panic` commits land.

Total: roughly 22-28 working days of focused work; batches 0-2 (5-6 days) remove essentially all of the crash, hang and DoS
risk and should land before the next feature wave. Batches 4-6 are the ones that most improve readability for future LLM maintainers
(fewer places to touch for a new component, no hidden invariants). Batch 8 is worth doing only after 5/7 because the same
files move.

### Suggested ordering inside batch 0 (each is 20-80 lines)

1. `http.zig:142` overflow (V-1) + unit test; reject duplicate/conflicting lengths while there (V-9).
2. `num.toInt` + the cast sites + `json.parse` rejecting non-finite and invalid UTF-8 + `style` range validation (SAF-1, SAF-3.2/3.3, LAY-2).
3. `units.parseFinite` and the scale/slope/crop/`run` bounds (SAF-5c); `arcSteps` clamp and bulge bounds (SAF-5).
4. `wasm.zig` `no_panic`; CI step `zig build wasm -Dwasm-optimize=ReleaseSafe` + `wasm_golden.mjs` against it (SAF-2).
5. CI: add `check_golden.sh`, `cli_ergonomics.mjs`, `zig fmt --check src`, and a fuzz run seeded with the appendix B values (TST-2).
6. `json` duplicate-key index (SAF-3.1) and `limits.zig` (SAF-5b); lazy `proposeFix` + work budget (LAY-1).
7. Release workflow: `-Doptimize=ReleaseSafe` for the CLI (+1.2 MB per target, stripped).

## 10. Keep as is (good, and worth defending in future refactors)

**Design**
* One arena per `kerf.call` (`api.zig:106-113`), results copied out once; the library never frees individually and has no
  ownership puzzles. Peak RSS on the reference documents is ~10 MB. Do not "fix" this with per-object allocators.
* The Drawing IR (`drawing.zig`) as the single seam between layout and four exporters, a tagged union with `inline else`
  accessors. Exporters are ~4% of the wasm and each is a straight pass over `d.items`.
* Diagnostics as data (`model.Diag` with `code`, `id`, `path`, `fix`), collected rather than thrown, with "did you mean" and
  concrete fix text: this is the product's differentiator for LLM users. `Params` collecting *all* parameter errors in one pass
  is right; IDM-1 keeps it.
* Ops are atomic copies of the document value with the citation downgrade rule enforced in one place (`ops.sanitizeCites`).
* `catalog.zig` + `schema.zig` as the single source for `kerf schema`, `kerf guide`, key-order canonicalisation, `W_UNKNOWN_KEY`
  and the minimal example document; `lint.zig`'s declarative synonym tables.
* The wasm ABI: six exports, zero imports, `kerf_alloc/kerf_free/kerf_call/kerf_out_*`, documented and checked by
  `wasm_check.mjs`; the native/wasm byte-identity gate (`wasm_golden.mjs`, 24/24).

**Determinism**
* No hash maps in the engine, every sort is the stable `std.mem.sort`, every comparator has an index tie-break, no clocks,
  integer-arithmetic number formatting (`json.fmtNumber`), outputs quantised to 1e-4.

**Geometry**
* Exact `orient2d` with an expansion fallback (`geom.zig:373-445`), arcs kept as bulges through clipping, occlusion and
  dedupe (arcs stay arcs in the DXF), `clip.boolean` as arrangement + midpoint classification that deals with coincident
  edges deterministically and is tested on abutting/identical/touching operands (`clip.zig` tests), `pathclip` clipping
  bulge polylines exactly, grain/hatch families with `.pat` semantics and a hard `max_lines`.
* The router (`route.zig`): exact per-column DP, lexicographic hard-hit ordering, pure geometry with no document types.

**Exporters**
* DXF R2000 with fixed handle seed and a zero-audit-error gate; PDF vector with correct xref/`/Length`; the CPU rasteriser's
  union-by-max coverage design and analytic spans; the SVG exporter writing text as stroke paths (no markup injection surface).
* `raster.layout` and the `px` clamp bound the allocation size.

**Server**
* Per-request arena in the connection loop; `validDocFile` whitelist; Host/Origin/rebinding checks; `--allow-origin` exact match;
  atomic temp-file + rename writes with a consistent lock order; bounded SSE ring; `{message}` as one argv element; strictly
  validated `session_id`/`file`; the proxy URL rules and their unit tests; `serve_smoke.mjs`.

**Build and tests**
* `build.zig`: anonymous imports of the spec files (single source of truth), a generated `ui_assets` module with `@embedFile`s,
  one source tree producing module + CLI + wasm, `b.resolveTargetQuery` for wasm, cross-compile smoke in CI, `-Dwasm-strip`
  for twiggy. `std.testing.allocator` leak tests at API level, goldens + `dxf_check`/`pdf_check` + byte-identical regeneration
  (`check_golden.sh`), `cli_ergonomics.mjs` for message text.
* Style: unmanaged `ArrayList` everywhere with explicit allocators, `//!` module headers that cite SPEC sections, named error
  sets (no `anyerror` in the library), no `usingnamespace`, no `@cImport`, no libc, no third-party dependencies.

## 11. Status after the engine hardening pass (batches 0 engine part, 1, 2) and the refactor batches 4-7

Branch `harden-engine`; every commit kept `zig build test`, `tests/check_golden.sh` (byte-identical), `wasm_golden.mjs` 24/24 (also
against a ReleaseSafe wasm) and `tests/cli_ergonomics.mjs` green. Hostile-input regressions: `src/hostile_tests.zig` (appendix B table,
the OOM sweep, the SAF-4 table), `json.zig`/`num.zig`/`route.zig`/`canon.zig` unit tests; `tools/zig-engine/fuzz.py` is seeded with the same values.

| Finding | Status | Notes |
|---|---|---|
| SAF-1 | done | `src/num.zig` (`toInt`, `toIntClamped`); every `@intFromFloat` on document/style/op data replaced (raster.zig sites are clamped against NaN already and were left). Plus bounds at the boundary: JSON numbers <= 9e15, lengths <= 1e6 in, scale 0.1..10000, slope, bulge 0 or 1e-6..1e3, gauge/count ranges |
| SAF-2 | done (engine) | `wasm.zig` uses `std.debug.no_panic`; `zig build wasm -Dwasm-optimize=ReleaseSafe` builds (2,105,215 B) and passes the golden; CI steps enabled |
| SAF-3 | done | duplicate keys via stable index sort (1.3 MB / 100k keys: 13.7 s -> 0.04 s), non-finite / out-of-range numbers, invalid UTF-8, leading zeros rejected; `fmtNumber` never uses `std.fmt` (item 5). Differential test against `std.json` (IDM-4) not done |
| SAF-4 | done | every `orelse 0/1.5/1` on offsets, place/array/recess/cover members, dim/label offsets, note `place`: E_PARAM with path, value, accepted forms, fix hint (`model.offsetPairOrDiag`, `Params.field*`) |
| SAF-5 | done | `geom.arcSteps`/`stepsForSweep` clamp to [2, 4096] (one implementation for geom, iso, mesh, pdf), bulge bounds as E_PARAM |
| SAF-5b | done | `src/limits.zig`: 8 MiB input, 2000 components, 5000 instances (20,000-instance doc: 7.0 s / 1.8 GB -> 0.8 s, E_LIMIT), 150 annotations/view (260: 118 s -> instant E_LIMIT), 64 views, 5000 points, 2000 note characters; every message names found / maximum / fix. `validate.overlaps` bucketing not done (the instance cap bounds it) |
| SAF-5c | done | `units.parseFinite` is the one float parser for document strings; scale/slope/crop/run/length bounds with "out of range" messages |
| SAF-6 | done | `oom.Sensor` around the call arena: any refused allocation anywhere -> `error.OutOfMemory` (stronger than a `Diags.oom` flag, covers all ~60 swallowing sites); `Params.failCode` replaces patching the last diagnostic; sweep test over every allocation index |
| SAF-7 | done | `canon.componentKeys` allocates from the writer (`json.KeyCtx.a`), no scratch, no 64-name cap; 6-thread test |
| SAF-8 | partly | done by batches 4-5: `build()` no longer ends in `unreachable` (exhaustive switch), the 211 `.?` unwraps are gone (typed `Params`). Not done: `Comp.built = undefined`, `Style` built field by field from `undefined` |
| LAY-1 | done | `route.Work` budget shared by all passes/trials of a view (40M units ~ 1.5 s), proposals only for the 8 reported hits, aggregate W_LEADER_HIT when the budget ends: 130 annotations 14.5 s -> 1.8 s. Not done: cap of the stored hit list, per-leader BB cache (3), 10% stop rule (5): the budget bounds the cost |
| LAY-2 | done | style validation (E_STYLE lists every bad key with range and default), `wrap(0)`, `gridStep` guards |
| LAY-3 | done | branch `lay-3`: `route` `snapshot()` returns `Allocator.Error!Snap` (`improve` too); the column re-key is `route.rekeyAt` (no buffer, no 64-note cap; unit test with 100 notes in one column); `annot.annotate` reserves `per`/`note_slot`/`meta` once and appends with `appendAssumeCapacity`; `Iso.landing` and `lint.keyList` propagate OOM. Found by the new sweep: `drawview.buildFromScene`/`resolveSpec` and `api` export turned a refused allocation in the embedded font parse into "view could not be built" / an unresolved spec / E_INTERNAL (`font.ParseError` now names `BadFont` vs OOM). Tests: `oom.OneShotFail` (refuses exactly one allocation, later ones succeed); `route()` sweep (every index is OOM or the identical layout); `hostile_tests` single-allocation sweep of a section view (8 notes, 3 dims, 2 labels) and an iso view through `api.dispatch` without the arena (~6,700 calls on 8 threads): before the fixes it crashed (index out of bounds in `renderDimLabels`). Left as is (sanctioned message builders, SAF-6 sensor reports them): `crop.fmtIn`, `units.ftin`/`model` fix text `catch "?"`; not OOM: `lint.mentions` matches ids only up to 126 bytes (fixed `bufPrint` buffer) |
| LAY-4..10 | deferred | not in this pass |
| IDM-6 | done | all leftovers in the table removed (`_ = x`, `and false`, `or true`, empty `if`, inline `@import`s) |
| ARC-2 | partly | done: `visiblePieces`/`visibleOpen`, `ftin`, `fmtNum` (4 copies), `flat_tol`, arc step counts, `[x, y]` offset parsing (one implementation). Not done: JSON escaper (http.zig is the server's), tolerance constants, edge-box helpers, `refCompId` |
| EXP-2 | done | `json.fmtFixed` serves svg, pdf, dxf, drawing bulge and json; 60,000-value differential test against `{d}`; non-finite prints `0` |
| `clip.boolean` diff pass, ARC-5, IDM-3..5,7, TST-1/2 (in-tree `std.testing.fuzz`), EXP-1/3..7, DET-*, PRF-1 table, SIZ-1 | deferred | batches 8-10 (IDM-1, IDM-2, ARC-1, ARC-3, ARC-4 are done: table below) |

### Batches 4-7 (typed vocabulary, typed params, roles, file splits)

Every commit kept all gates green (test, golden byte-identical, wasm golden 24/24, `cli_ergonomics`, `serve_smoke`, `zig fmt --check`; each batch end also the ReleaseSafe
wasm golden and `fuzz.py 300 1`: 0 crashes). Two diff corpora (every param of every type mutated through `kerf call check`: ~5,800 diagnostics + summaries; 156 ops through
`kerf call apply`) were compared against the pre-refactor binary after each conversion. NOTES.md "Architecture" has the module map and the "how to add a component type" recipe.

| Finding | Status | Notes |
|---|---|---|
| IDM-2 | done | `api.Fn` (E_FN list generated), `catalog.Type` (comptime order check; `builders.build` is an exhaustive switch), `pen.Pen` + `pen.LayerKey` (14 pens / 10 layer keys; `Pen.layer()` exhaustive; style pen struct renamed `PenDef`), `drawing.Kind`. **Behaviour change**: a material `pen` in a user style must name an engine pen (E_STYLE); the implicit "pen named like the fill material" lookup is gone (only steel/rebar ever used it: `style.Role`) |
| IDM-1 | done | `params.zig`: `Params.parse` / `parseAll` by comptime reflection (field type -> parser, `spec` for ranges, lengths, hints, catalog text; compile errors for undocumented fields); all 14 builders converted, one commit each; 211 `.?` unwraps removed. `parseAll` keeps the collect-every-problem behaviour where the old code did (panel). Messages verbatim (corpus identical). The dead `lumber` `material` read was dropped (the key was never accepted: unknown-param error) |
| ARC-4 | done (partly) | `catalog.Param.names` slices (no more `"a, b"` splitting at runtime); every catalog row of every type is generated from its `Params` struct: catalog, schema, validation and the canonical key order cannot drift; a test checks that every enum choice appears in its row's text (found one gap: membrane `side`, fixed). Not done: `catalog.hardware` as `StaticStringMap` (its table order is part of the `kerf catalog` output and the lookup is case-insensitive) |
| ARC-1 | done | `style.Role` (soil, wood, masonry, grout, steel, rebar, sheet_metal, vapor_retarder, void) from an optional style key, defaulting from the name (the embedded style is unchanged: a test asserts the role table equals the old predicates for every embedded material); `Prism.role` stamped in `compile.placeComponent`; `catalog.Traits` (11 flags) replace every `ty.type == .x` outside the builders. The import cycles are broken: `shape.zig` (iso no longer imports annot), mesh no longer imports section, `schema_fields.zig` (the REVIEW's "schema imports canon and never uses it" was stale: both used each other; now canon and lint read the field tables only). Not done: `lint.withDimDirsFromDoc` still compiles |
| ARC-3 | done (listed items) | `compile` stages (`readSettings`, `componentItems`, `readComponents`, `placementOrder`; `placeComponent` = `resolveAngle`, `resolveZ`, `resolveArray`, `instanceTransforms`, `worldPrisms`), `ops.applyOne` -> `applyDoc/Meta/Components/Views` on a typed `Op`, `builders/` (one file per component type + `common.zig`; `builders.zig` 1822 -> 90 lines), `section/` (6 files; 1159 -> 393), `annot/` (6 files; 1587 -> 343), `serve/` (7 files + route table; 1794 -> 556), `main.run` -> `cmdVersion/Catalog/Schema/Guide/Init/New/Call/Doc` (`cmdDoc` = `prepareDocCall`, `lockForWrite`, `finishFmt`, `finishApplyCheck`). Not done: `dxf.render` (403 lines), `iso.build`, `style.fromValue` table, `clip.boolean`, `serve.cli.cliMain` (still ~170 lines). Found, left alone: `Section.shingleTicks` is never called (dead since commit 2784409) although the membrane builder sets `Prism.ticks`: either the SPEC tick marks on shingles are not drawn on purpose or this is a regression; goldens agree with "not drawn" |

wasm ReleaseSmall: 951,291 B before; batch 4 949,372; batch 5 964,314 (typed params + generated catalog rows); batch 6 966,523; batch 7 966,990; after `params.readScalar` (one
non-generic reader) **963,851 B raw / 355,588 gzip / 281,466 brotli** (+1.3% overall). The CLI exe is now stripped by default outside Debug (`-Dstrip`): ReleaseSafe Linux 16.0 MB -> 2.5 MB.

Measured before / after (ReleaseFast, `kerf call`):

| Case | Before | After |
|---|---|---|
| 100 notes + 20 dims + 10 labels (130 annotations) | 14.5 s | 1.8 s |
| 200 notes + 40 dims + 20 labels (260) | 118 s | E_LIMIT in 0.01 s |
| 40 x `array.count` 500 (20,000 instances), `check` | 7.0 s | E_LIMIT after 5000 instances, 0.8 s |
| 100,000-key JSON object (1.9 MB), `check` | 13.7 s | 0.04 s |
| wasm ReleaseSmall | 902,131 B (gzip 338,275) | 951,291 B (gzip 353,965): +5.4%, validation messages and checks; SIZ-1 reductions still open |

## Appendix A. Prototypes (compile and pass on Zig 0.16.0)

### A.1 Checked float -> int (SAF-1)

```zig
const std = @import("std");

/// Float -> integer that never invokes UB: null for NaN/inf/out-of-range, truncates toward zero otherwise.
pub fn toInt(comptime T: type, x: f64) ?T {
    if (!std.math.isFinite(x)) return null;
    const bits = @typeInfo(T).int.bits;
    const signed = @typeInfo(T).int.signedness == .signed;
    // 2^bits (or 2^(bits-1)) is exact as an f64; comparing against the open bound avoids the "maxInt rounds up" trap.
    const hi: f64 = std.math.ldexp(@as(f64, 1), if (signed) bits - 1 else bits);
    if (x >= hi or (if (signed) x < -hi else x <= -1)) return null;
    return @intFromFloat(x);
}

/// Clamp then convert; NaN maps to `lo`. For counts/step numbers where "too big" should saturate.
pub fn toIntClamped(comptime T: type, x: f64, lo: T, hi: T) T {
    if (std.math.isNan(x)) return lo;
    return @intFromFloat(std.math.clamp(x, @as(f64, @floatFromInt(lo)), @as(f64, @floatFromInt(hi))));
}
```

Tests that pass: `toInt(i64, 1e30) == null`, NaN -> null, `9223372036854775808.0 -> null`, `-9223372036854775808.0 -> minInt(i64)`,
`toInt(usize, -1) == null`, `toInt(usize, -0.5) == 0`, `toInt(u8, 255.9) == 255`, `toInt(u8, 256) == null`,
`toIntClamped(usize, NaN, 2, 4096) == 2`, `toIntClamped(usize, inf, 2, 4096) == 4096`.

### A.2 Typed `Params.parse` (IDM-1)

Comptime reflection over a struct; collects *all* errors (as today), generates the "must be one of ..." text from the enum,
no allocation except messages. The test below prints exactly the message style the engine uses.

```zig
pub fn Int(comptime lo: i64, comptime hi: i64) type {
    return struct { v: i64 = lo, pub const min = lo; pub const max = hi; };
}

pub fn parse(comptime T: type, a: std.mem.Allocator, node: json.Value, diags: *std.ArrayList(Diag)) std.mem.Allocator.Error!?T {
    var out: T = undefined;
    var ok = true;
    inline for (@typeInfo(T).@"struct".fields) |f| {
        const raw = node.get(f.name);
        if (raw == null or raw.? == .null) {
            if (f.defaultValue()) |d| {
                @field(out, f.name) = d;
            } else if (@typeInfo(f.type) == .optional) {
                @field(out, f.name) = null;
            } else {
                try diags.append(a, .{ .key = f.name, .msg = try std.fmt.allocPrint(a, "param '{s}' is required", .{f.name}) });
                ok = false;
            }
        } else {
            const Base = switch (@typeInfo(f.type)) { .optional => |o| o.child, else => f.type };
            const v = raw.?;
            switch (@typeInfo(Base)) {
                .bool => if (v == .bool) { @field(out, f.name) = v.bool; } else {
                    try diags.append(a, .{ .key = f.name, .msg = try std.fmt.allocPrint(a, "param '{s}' must be true or false (got {s})", .{ f.name, v.kindName() }) });
                    ok = false;
                },
                .float => if (v == .number) { @field(out, f.name) = v.number; } else { /* "must be a number" */ ok = false; },
                .pointer => if (v == .string) { @field(out, f.name) = v.string; } else { /* "must be a string" */ ok = false; },
                .@"enum" => if (if (v == .string) std.meta.stringToEnum(Base, v.string) else null) |e| {
                    @field(out, f.name) = e;
                } else {
                    var list: std.ArrayList(u8) = .empty;
                    inline for (std.meta.fieldNames(Base), 0..) |n, i| {
                        if (i > 0) try list.appendSlice(a, ", ");
                        try list.print(a, "\"{s}\"", .{n});
                    }
                    try diags.append(a, .{ .key = f.name, .msg = try std.fmt.allocPrint(a, "param '{s}' must be one of {s}", .{ f.name, list.items }) });
                    ok = false;
                },
                .@"struct" => if (@hasDecl(Base, "min")) { // Int(lo, hi)
                    if (v == .number and v.number == @round(v.number) and v.number >= Base.min and v.number <= Base.max) {
                        @field(out, f.name) = .{ .v = @intFromFloat(v.number) };
                    } else {
                        try diags.append(a, .{ .key = f.name, .msg = try std.fmt.allocPrint(a, "param '{s}' must be an integer from {d} to {d}", .{ f.name, Base.min, Base.max }) });
                        ok = false;
                    }
                } else @compileError("unsupported param type " ++ @typeName(Base)),
                else => @compileError("unsupported param type " ++ @typeName(Base)),
            }
        }
    }
    return if (ok) out else null;
}

const LumberParams = struct {
    size: []const u8,                                       // required
    product: enum { sawn, lvl, psl, lsl, glulam } = .sawn,
    run: enum { z, x, y } = .z,
    plies: Int(1, 8) = .{ .v = 1 },
    treated: bool = false,
    length: ?f64 = null,
};
// {"run":"w","plies":9,"treated":1}  ->  4 diagnostics:
//   size: param 'size' is required
//   run: param 'run' must be one of "z", "x", "y"
//   plies: param 'plies' must be an integer from 1 to 8
//   treated: param 'treated' must be true or false (got number)
```

(`Length`, `Slope` and `Point` field types slot into the same `switch` by declaring a `pub fn fromJson(v: json.Value) ?@This()`
and testing `@hasDecl(Base, "fromJson")`, which also lets `units.parseLength` stay the only length parser.)


## Appendix B. Numeric-extremes and hostile-input pass (what was tried)

Method: Debug build (all safety on) and ReleaseSmall build of the CLI, `kerf call <fn> < input.json`, `timeout 10`, `ulimit -v 4000000`,
extreme values substituted at the leaves of the five reference documents plus hand-written generators. A leaf-substitution
fuzzer (~42k mutants) was cut short after ~14 results; `fuzz.py 300 1` (shipped) finished with 0 crashes.

**Failures** (all are SAF-1/5/5b/5c): bulge `1e-14`, `1e14`, `1e300` and radius ~1e12 (`geom.zig:242`; Debug panic in
check/drawing/mesh/export; ReleaseSmall: drawing and `bigradius1e12` hit the 10 s timeout, mesh with `1e14` and PNG with
`1e-14` end in `kerf: OutOfMemory` after ~6 s); lengths over ~5.7e17 as numbers or 19-digit strings, `array.spacing: +-1e300`,
connector `points[].offset: 1e300`, `run: [-1e300, ...]` (`units.zig:124`); `slope`/`pitch` of `"nan"`, `"inf"`, `"1e999"`;
scale `"1e300:1"`, `"1:1e-300"` (`units.zig:196`), `"nan:1"`, `"1:inf"`, `"1:1e300"`, `"1'=1\""`; `meta.jurisdiction.edition`
`1e999` or any value >= 1e19, also as the string `"1e999"` (`drawview.zig:54`); connector `points[0].offset: [1e300,..]`
(`section.zig:376`); `run: [-1e300, 1e300]` (`iso.zig:507` in `check`, `mesh` hangs); crop `+-1e300` (silent empty drawing);
style `wrap_chars` `0`, negative, `1e30` and `text.height_in: 0` (`annot.zig:166-205,636,735`, LAY-2); 100k-key object (SAF-3);
20,000 instances (SAF-5b); 100/200 notes (LAY-1); 10k/50k components (SAF-5b). Hatch conversions at `hatch.zig:57-58,126,167`
are suspect by code reading (no reproducer: the style fields were not reached).

**Handled well (clean `E_*` or correct):** JSON nesting 199 accepted, 200 up to 1,000,000 levels gives `E_JSON "nesting too deep"` in
0.03-0.09 s for arrays and objects; 50 MB of whitespace accepted in 0.7 s; array `count` of 0, -1, 501, 1e9, 1e19, 2^53, `"nan"`, `-0.0`, inf;
`array.spacing` `"nan"`, inf, 1e-300, 0, 1e9; `crop` inverted/zero/`"nan"`/1e-300..1e12 (`E_VIEW`); scales `0:0`, `1:0`, `-1:1`, `1:-1`, `inf:1`, `=`,
blank, `1"=1e300'`, `1e308"=1e-308"` (`E_VIEW`); `px` of 0, -1, 1e9, 1e300, inf, 16000, 100000 clamps to 6000 (`"nan"` gives `E_INPUT`; 75 MB peak,
0.7 s ReleaseSmall, 5.5 s Debug); `sheet`, `style: {}`, `[]`, `null`, `{"hatch":1}` on svg/png/pdf/dxf; `rotate` nan/inf/1e300/1e9;
non-numeric bulge `"nan"` rejected; 5,000 labels 0.9 s; `canon.type_scratch` does not overflow (bounded by `n < len`), it is only a
latent race (SAF-7).

**Not covered:** invalid UTF-8 / lone surrogates / NUL / Unicode in note text (the parser rejects lone surrogates and raw control
characters but not invalid UTF-8, SAF-3.3); ops-level cycles and self-references; hatch pattern overrides through `style`; iso `from`
and `cutaway` variants; `inspect`/`apply` with extreme values were exercised only by the leaf fuzzer (no extra sites).
