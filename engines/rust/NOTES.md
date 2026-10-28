# engines/rust: Kerf engine in Rust (std-only, zero crates)

(Status report is written at the end. This section is the porting contract used while the port was built.)

## Layout
- `kerf-core`: `#![no_std]` + `alloc` library, the whole engine (SPEC 13). One module per Zig file, same name in snake_case.
- `kerf-cli`: binary `kerf` (std). Serve code (`http`, `serve`, `agents`, `proxy`, `events`, `workspace`) lives here, not in core.
- `kerf-wasm`: cdylib, the SPEC 13.1 ABI.

## Port conventions
- Zig `fooBar` becomes `foo_bar`; types keep their names; struct fields keep their names; enum tags become CamelCase variants.
  A module's public API is the Zig module's public API translated mechanically, so ports can be written against the Zig source.
- No allocator parameters. No arenas needed: `Vec`, `String`, owned values. OOM is not modelled (Zig's `error.OutOfMemory` disappears).
- JSON is `json::Value` (owned tree: `Null, Bool, Number(f64), String(String), Array(Vec<Value>), Object(Vec<Member>)`), consumers take `&Value`.
- Output text is built into `Vec<u8>`/`String`. Never format an `f64` with `{}` / `{:?}` / `{:.N}` (float formatting is large and not
  our rule): use `json::fmt_number`, `json::fmt_fixed`. Integers and `&str` may use `format!`/`write!`.
- Float math that `core` lacks (`sqrt sin cos atan2 floor ceil round trunc ...`) comes from `crate::math`, never from `std`/inherent methods
  (`f64::abs`, `min`, `max`, `clamp`, `is_finite`, `signum`, `copysign` are in `core` and fine).
- Errors: typed enums per module (Zig error sets), `Result`. Hostile input must never panic: no `unwrap`/`expect`/unchecked indexing on
  paths reachable from a document; use `get`, `?`, `let ... else`. Arithmetic on integers derived from input uses checked/saturating ops.
- Sorting: only `crate::sort::stable(&mut [T], less)`.
- Determinism: no HashMap/HashSet (use sorted Vec / BTreeMap), no time, no randomness.
- Comments rare. No unit tests. Verification is end to end (goldens, compare_drawings, wasm, CLI, fuzz).

## REQUESTS
(none yet)

## SPEC ISSUES
(none yet)
