# Eval: Claude Code (headless) driving the Kerf CLI, baseline engine

- Date: 2026-10-06. Runner: `spec/evals/run-cli.mjs --agent claude` (one run per prompt, sequential).
- Agent: Claude Code 2.1.289, model `claude-opus-5-5` (the default), `--permission-mode acceptEdits`,
  `--allowedTools "Bash(kerf:*)" "Bash(kerf *)" Read Write Edit`, cwd = fresh `kerf init` folder.
- Engine: frozen `kerf-baseline` (`kerf-zig 0.1.0`, spec 0.1), shimmed first on PATH.
- Raw data: `2026-10-06/summary.json`. Transcripts and per-call digests stay in `~/kerf-eval/runs/2026-10-05T21-30-33-782Z/` (not committed, 15 MB).
- Totals: 10 prompts, 258 kerf invocations, 38 min wall, $12.43 (subscription-equivalent cost reported by Claude Code), 212k output tokens.

## Headline

The automatic grader is saturated: mean 0.996 (9 of 10 at 1.00, e06 0.96 for two warnings). The hand
rubric tells a different story: **mean 6.7 / 8 over the nine built details**, with real defects in
e02, e04, e07 and e09. The agent reaches a clean `kerf check` every time, but it gets there by
**reverse-engineering the view/annotation schema from validation errors** (12-26 probe calls and
roughly half of the wall time before the first real write in every from-scratch build), and a
**CLI bug hides the engine's error text** when `kerf apply` fails. Edit tasks, where the schema is
already visible in the file, are fast and clean (39 s and 54 s, $0.36 and $0.51).

## Results

Auto = automatic score from `expect` checks (minus 0.02 per warning). Rubric = hand score 0-2 each:
R1 reads as the expected detail, R2 members sized/placed with no overlaps or gaps, R3 note grammar and
consistency, R4 citations plausible and not invented. "Probe calls" = `kerf call ...` raw-API calls plus
scratch `-o` applies, i.e. schema discovery rather than building.

| id | auto | R1 | R2 | R3 | R4 | rubric | warn | time | kerf calls (probe) | cost | PNG |
|---|---|---|---|---|---|---|---|---|---|---|---|
| e01-truss-cmu | 1.00 (13/13) | 2 | 1 | 2 | 2 | 7 | 0 | 6m31 | 36 (18) | $1.95 | [A](2026-10-06/e01-truss-cmu-A.png) |
| e02-truss-heta | 1.00 (9/9) | 1 | 1 | 1 | 2 | 5 | 0 | 4m36 | 28 (13) | $1.40 | [A](2026-10-06/e02-truss-heta-A.png) |
| e03-monopour-recess | 1.00 (9/9) | 2 | 2 | 1 | 2 | 7 | 0 | 6m39 | 50 (26) | $2.05 | [A](2026-10-06/e03-monopour-recess-A.png) |
| e04-monopour-plain | 1.00 (9/9) | 1 | 1 | 2 | 2 | 6 | 0 | 3m43 | 26 (10) | $1.34 | [A](2026-10-06/e04-monopour-plain-A.png) |
| e05-flush-beam | 1.00 (8/8) | 2 | 2 | 2 | 2 | 8 | 0 | 3m59 | 38 (16) | $1.45 | [A](2026-10-06/e05-flush-beam-A.png) |
| e06-flush-beam-2x6 | 0.96 (7/7) | 2 | 1 | 2 | 1 | 6 | 2 (W_NEAR_MISS x2) | 5m18 | 31 (12) | $1.58 | [A](2026-10-06/e06-flush-beam-2x6-A.png) |
| e07-edit-notes | 1.00 (5/5) | 1 | 1 | 2 | 2 | 6 | 0 | 0m39 | 5 (0) | $0.36 | [A](2026-10-06/e07-edit-notes-A.png) |
| e08-edit-remove | 1.00 (5/5) | 2 | 2 | 2 | 2 | 8 | 0 | 0m54 | 12 (0) | $0.51 | [A](2026-10-06/e08-edit-remove-A.png) |
| e09-screenshot | 1.00 (10/10) | 1 | 2 | 2 | 2 | 7 | 0 | 5m20 | 31 (15) | $1.58 | [A](2026-10-06/e09-screenshot-A.png), [reference](2026-10-06/e09-reference.png) |
| e10-ambiguous | 1.00 (3/3) | n/a | n/a | n/a | n/a | n/a | 0 | 0m15 | 1 (0) | $0.21 | none |

Notes on the harness: 71 of the 95 tool errors across the run were the headless permission sandbox,
not the engine (compound commands with `;`/`|`/heredocs, `/tmp` paths, `WebFetch`, `strings` on the
binary). They cost time but are an artifact of the restricted `--allowedTools`; a repeat should
either allow `cat/ls/head/grep` or run in a throwaway sandbox with `bypassPermissions`.
The `e10` run asked one clarifying question (CMU stem vs stud wall vs monopour vs spread footing,
with a stated default) and wrote no file, which satisfies `asks_question_or_states_assumptions`.

## Per-detail review (strict structural-engineer view)

- **e01 truss on CMU, 7/8.** Reads right: 8" CMU with grouted bond beam and (2) #5, 2x8 PT sill, 5/8" headed bolt, H2.5A tie, 18" overhang dimension, fascia, ceiling. Defects: the bottom chord and the roof layers end at different x (roofing/sheathing stop short of the truss line at the crop), the tie is a bare black bar with no relationship to the heel, notes are fine. Citations R802.10, R802.11, R317.1, R606 are all real and apt.
- **e02 truss on bond beam with HETA20, 5/8.** Truss correctly bears directly on the bond beam, but the HETA20 is drawn as a 10-inch vertical rectangle that pokes through the bottom chord (a 20-inch strap, no wrap onto the truss), the sheathing note says 15/32" while the component is 1/2", and the agent inserted a fake 1/16" membrane "SILL SEALER MOISTURE BARRIER" only to silence `W_UNTREATED_CONTACT`, then wrote a note for it. "TRUSS BEARS DIRECTLY ON BOND BEAM, NO WOOD PLATE" is commentary, not office grammar. No uplift/wind note for "Florida, high wind". It cited `FBC-R` numbers that mirror the IRC; plausible, flagged by the jurisdiction setting.
- **e03 monopour with recess, 7/8.** Best of the from-scratch builds: recess 1-1/2" x 6", 12x18 turndown, vapor retarder and gravel along the haunch, #3 hooked into the footing. Cosmetic: the vertical `1'-6"` dimension text is struck through by its own dimension line. Notes use lowercase `x` (`12" x 18"`) while the other details use `X`, and `1-1/2"` vs `1 1/2"` differs between details.
- **e04 monopour plain, 6/8.** Correct slab edge, PT sill, J-bolt, 6" above grade dimension. But the wall above the sill reads as an empty box: the stud is a lengthwise member drawn outline-only and coincides with the sheathing/gypsum skins, and the `2X6 STUDS @ 16" O.C.` leader lands in the void. The bolt sits in the left third of the plate rather than the centre.
- **e05 flush beam, 8/8.** Plates broken at the (2) LVL, strap across, 4x4 post and king studs, plate X marks correct. The agent could not draw a vertical dimension and dropped it. IRC R602.6.1 and R602.3(1) are right. `(IRC Table R602.3(1))*` renders mixed case inside an uppercase note (engine citation formatting).
- **e06 flush beam 2x6 with PSL, 6/8.** Right idea (CMST14, PSL). Two `W_NEAR_MISS` warnings left in on purpose (a 1/8" gap between 5-1/4" PSL and the king studs beside a 6x6 post). The agent told the designer it attached IRC R602.3.2 to the strap note, but it wrote the key `citations` instead of `cite`: **the engine accepted it silently and nothing renders**, so the doc has no citation at all and the message is false. Beam depth (11-7/8") was assumed, and stated.
- **e07 edit (4:12 to 6:12, 1/2" bolts @ 32"), 6/8.** Surgical (only `anchor_bolt`, `truss`, `roof_sheathing` and the `n_plate` note changed) and fast. But the pitch lives in two places (`truss.pitch`, `roof_sheathing.slope`) and the sheathing/roofing keep their old 66" length, so after the pitch change they end well short of the truss top chord at the crop; the heel zone is cluttered; the agent itself noted a 2" gap under the sheathing at the bird block. The 32" spacing exists only in a note (one bolt drawn). The reference docs are not in the engine's canonical key order (a harmless diff-noise issue the grader had to normalize).
- **e08 edit (remove slab bars for fiber), 8/8.** Clean. Removed both bar components, retargeted `n_slab`, rewrote the note, kept footing steel and said so. Hit the silent-apply-failure bug first (see below).
- **e09 screenshot recreation, 7/8.** Geometry and wording match the reference closely and all citations match. But every note is forced into the right-hand column so nine leaders run long and cross over the footing (the reference splits left/right), the slab bar renders as a heavy black bar instead of the reference's thin bar, and the sill pan flashing is drawn differently.
- **e10 ambiguous.** Asked rather than built; good behaviour per the spec, though the optional default build was not produced.

## Failure patterns and improvements, ranked

1. **`kerf apply` hides the error when it fails (engine/CLI bug, highest impact).** On any failed apply
   (bad op, validation error, remove blocked by a note target) the CLI exits 1 and prints only the
   unchanged-document summary; the `E_*` message and its `fix` hint appear nowhere (verified on the
   baseline and on the installed `~/.local/bin/kerf` 0.1.0: stdout and stderr both carry only the
   `DOC ... 0 errors 0 warnings` line). The raw API (`kerf call apply`) returns the diagnostics, so
   agents learned to bypass the CLI. Agent quotes: "The apply failed without a visible error message.
   Re-running without `-w` to see the error"; "Still no error text ... The `--ops` form seems to be
   ignored silently" (e08); "The CLI isn't printing the error, so I'll run the same ops through the
   raw engine call" (e09). Fix: print `ERROR <code> <path>: <message>  Fix: <fix>` for every
   diagnostic to stderr and say "nothing written" on exit 1. Add `kerf apply --dry-run`.
2. **The guide documents no view or annotation schema.** `kerf guide` says what ops exist but not
   that a section view requires `crop`, nor the `scale`/`number`/`title`/`notes_side` fields, nor the
   annotation types (`note|dim|label`), their fields (`text`, `target`, `at`, `place`, `from`/`to`/`dir`/`offset`),
   the `cite` array shape, or that part targets look like `slab.footing`. It points to a GitHub URL
   the agent cannot fetch (WebFetch was denied in all 7 from-scratch runs; offline users have the same
   problem). Result: 12-26 probe calls per build, nothing real written until 40-60% of the wall time
   (first `-w` apply at call 13/36, 19/28, 49/50, 15/26, 15/38, 14/31, 15/31), and in e01/e03 the
   agent resorted to `strings` on the binary and `curl` against a running server. Quotes: "Still
   probing the annotation schema (the CLI's docs don't list it)"; "Learning how citations attach
   to notes"; "Still pinning down the citation field format". Fix: inline spec §6 (view fields, the three
   annotation shapes, one complete minimal example document with a view, notes, a dim, a label and a cite)
   in the guide; add `kerf schema [view|note|dim|label|cite]` and `kerf new --template section`;
   make `kerf call help` list functions with their input shapes (it currently returns `E_FN` listing names only).
3. **Unknown keys are silently kept and ignored.** `citations` for `cite` (e06, false claim to the
   designer), `side` and `point` on notes (e03, e09: "`side` and `point` are both ignored"), `kind`
   on an annotation, `bogus`. The existing `notes_side` view key (right/left/both) is exactly what
   agents were looking for when they tried `side`, but never found it. Fix: warn or error on unknown
   keys with a suggestion table (`citations`/`citation`/`cites` -> `cite`, `side` -> view `notes_side`,
   `point`/`target_point` -> `at`). The grader now reports `unknown_keys` per case.
4. **Vertical dimensions measure `0"` by default.** `dir` defaults to `h`, so a dim between two
   vertically separated points prints `0"`; `dir: "v"` is only discoverable by trial (e02, e03, e04, e09 found
   it after 4-8 calls; **e05 and e06 never found it and dropped the beam-depth dimension**). Fix:
   infer direction from the dominant axis when `dir` is omitted and emit a warning when a dim
   measures under 1/16".
5. **No way to steer note placement; leaders cross.** Notes always stack in the right column
   (spec `notes_side` is undocumented in the guide). Agents spent many exports reordering `at`
   points to de-cross leaders; e09's final has nine long leaders crossing over the footing.
   Fix: default `notes_side` to `both` (nearer side) when landing points spread across the crop,
   document `place`/`notes_side` in the guide, and make `W_LEADER_HIT` (spec §18) fire so `kerf check`
   reports crossings instead of the agent counting them by eye.
6. **View crop/scale are hand-tuned every time.** `crop` is mandatory (`E_PARAM` on a missing one)
   and `W_VIEW_FIT` fired in 5 of 7 builds (e02, e03, e04, e05, e06), costing a tuning loop each.
   Fix: make `crop` optional (auto-fit to component extents plus margin) and choose the largest
   standard scale that fits; keep `W_VIEW_FIT` only for explicit overrides.
7. **Anchor bolt defaults produce a wrong picture.** Without `at.anchor: "top_of_concrete"` the bolt
   is upside down (e01, e04: "The anchor bolt is upside down - I didn't set its placement anchor"),
   and the default z put it behind the cut plane (e04: `W_NOTE_TARGET`, "bolt is behind the cut
   plane"). Fix: make `top_of_concrete` the default placement anchor, default `z` to the cut plane, and
   state both in the catalog row.
8. **`W_UNTREATED_CONTACT` pushed the agent into a hack (e02).** A truss bearing directly on a bond
   beam is a legitimate detail, but the only way to clear the warning was to add a fake thin
   membrane that then needed a note. Fix: a `barrier` / `sill_seal` parameter on lumber (or a
   catalog `sill_sealer` strip), or an explicit `acknowledge: [W_UNTREATED_CONTACT]` with a reason
   that prints in the log.
9. **Coupled roof parameters.** `truss.pitch`, `roof_sheathing.slope` and the sheathing `length` must
   be edited together (e07 left sheathing and roofing short of the truss, and a 66" fixed length
   no longer reaches the crop). Fix: `slope: "@truss"` / `until: "truss@top_chord_end"` references,
   a `roof` assembly derived from the truss top chord, tie placement anchors on the truss
   (`truss@heel_outer`), instead of literal offsets like `[6.5, 0.15]`. Also an `array` for anchor bolts
   so "@ 32" O.C." can be drawn, not just written in a note.
10. **Lengthwise members look like voids.** A stud, plate or blocking running in the section plane is
    outline-only and merges with adjacent skins, so a stud wall reads as an empty box (e04). Fix:
    draw `beyond` lengthwise lumber with a faint wood-grain/diagonal mark, or add a catalog
    `stud_wall` component that includes plates and studs.
11. **Note grammar is under-specified, so notes drift between details.** `12" x 18"` vs `12" W X 18"`,
    `1-1/2"` vs `1 1/2"`, `GYPSUM BOARD` vs `GYP. BD.`, descriptive sentences (`TRUSS BEARS DIRECTLY ON BOND
    BEAM, NO WOOD PLATE`), and engine-rendered `(IRC Table R602.3(1))` in mixed case. Fix: add fraction format,
    uppercase `X`, the gypsum/sheathing abbreviations and "no sentences" to `spec/llm/system.md` and the
    guide; add a `W_NOTE_STYLE` lint (lowercase letters, trailing period, unknown abbreviation,
    ` x `); uppercase the citation text in the renderer.
12. **Eval gaps.** The automatic score cannot see most defects found by eye (floating or
    clipped geometry, empty-looking walls, dropped dimensions, false citation claims). Next run should
    add: `unknown_keys == 0`, `W_VIEW_FIT == 0`, leader crossings from `kerf drawing`, every
    note target visible, and tool-error classification (sandbox vs engine; this run: 71 sandbox of 95).
    Also let agents `cat/ls/head` so the sandbox does not tax every run.

## What worked

- `kerf init`'s AGENTS.md/CLAUDE.md plus `kerf guide` reliably put the agent on the right workflow: guide first, `-w --why`, export a PNG and look at it. Every agent looked at its render and fixed leaders, dimensions and fit.
- The warnings paid off: `W_OVERLAP` (e01 sheathing/fascia), `W_FLOATING` (e01 shingles), `W_VIEW_FIT`, `W_NOTE_TARGET`, `W_COVER` and `W_UNTREATED_CONTACT` each led to a fix. `E_PARAM` messages that name the allowed values (annotation `type`, scale strings, "needs a `target` or an `at` point") were the useful ones; the `fix` hints were good.
- Edit tasks are cheap: 5 and 12 kerf calls, under a minute.
- Citations were all `suggested`, section numbers were real (R403.1.6, R506.2.3, R602.6.1, R802.11 ...), and the agents told the designer which to verify.
- The agent respected scope on e07 (only the three requested components changed) and asked a good question on e10.
