# Eval: Claude Code (headless) driving Kerf alpha4 vs the baseline

- Same 10 prompts, same agent (Claude Code 2.1.289, `claude-opus-5-5`), each run once, sequentially.
- Before = `kerf-baseline` (report: `2026-10-06-claude-code-baseline.md`). After = frozen `~/kerf-eval/bin/kerf-alpha4`
  (SPEC §18-19: apply errors printed, `kerf schema`, guide with schema + example, `W_UNKNOWN_KEY`, dim direction
  inference, auto crop/scale, `notes_side: both`, `W_LEADER_HIT`, `acknowledge`/`barrier`, anchor-bolt defaults,
  `slope: "@truss"`, grain marks ...).
- Runner changes for this run: read-only shell tools allowed (`cat/ls/head/tail/grep/jq`, Glob, Grep); new gates
  (no unknown keys, `W_VIEW_FIT == 0`, `W_LEADER_HIT == 0`, `W_NOTE_TARGET == 0`); `engine_errors` vs `sandbox_blocked`;
  `probe_calls_before_first_write` / `first_write_call` / `first_write_s` computed from the kerf shim log.
  "Probe" = a raw-API `kerf call` or an `apply` that writes nothing (scratch `-o`, `--dry-run`); `kerf schema` lookups are
  counted separately and not as probes.
- Data: `2026-10-06-alpha4/` (PNGs + `summary.json`); `2026-10-06/summary-restats-alpha4-checker.json` is the baseline run's
  stats recomputed with the new runner, with the baseline documents re-checked by the alpha4 engine.
  Transcripts stay in `~/kerf-eval/runs/2026-10-05T22-47-41-504Z/`.

## Headline

| | baseline | alpha4 |
|---|---|---|
| hand rubric, mean of the 9 built details (0-8) | 6.7 | **7.7** (7.8 with e10 built) |
| wall time, 10 prompts | 37.9 min | **25.5 min** (-33%) |
| kerf calls | 258 | **146** |
| probe calls before the first real write (sum) | 94 | **8** |
| probe calls in total | 138 | **18** |
| tool errors (engine / sandbox) | 24 / 71 | **5 / 19** |
| cost | $12.43 | **$9.23** (-26%) |
| output tokens | 212k | 142k |
| warnings left in final docs | 2 | 0 |

The schema/guide/error-printing changes did what the first report asked: agents stopped reverse-engineering the
format (median first real write moved from call 15 to call 6; e03 went from call 49 of 50 to call 8). The automatic
score is still 1.000 on every case, so it still cannot rank the runs. The rubric difference comes from visibly better
drawings: notes now split left/right with no crossing leaders, vertical dims work, and the e04 stud wall,
e02 HETA, e06 strap/warnings defects are gone. The baseline documents, re-checked with the alpha4 engine, would have
scored 0.85-0.98 on 6 of the 9 built details (12 warnings in total: `W_NOTE_STYLE`, `W_VIEW_FIT`, `W_LEADER_HIT`, `W_UNKNOWN_KEY`,
`W_NEAR_MISS`), so the new checks are real signal even though the final alpha4 documents all pass them.

## Before / after per prompt

Rubric = R1 reads as the expected detail + R2 sizing/position without overlaps or gaps + R3 note grammar/consistency +
R4 citations plausible (each 0-2, hand-scored from the PNG with the same strictness as the baseline).
Auto = `score_v1` (original formula). "probe BFW" = probe calls before the first `apply -w` (and the call number of that
first write). Tool errors = engine + sandbox.

| id | rubric before > after | auto before > after | warn before > after | time before > after | kerf calls | probe BFW (first write call) | tool errors | cost | after PNG |
|---|---|---|---|---|---|---|---|---|---|
| e01 truss CMU | 7 > **8** | 1.00 > 1.00 | 0 > 0 | 6m31 > 5m03 | 36 > 21 | 9 (#13) > 1 (#7) | 1+15 > 1+4 | $1.95 > $1.48 | [A](2026-10-06-alpha4/e01-truss-cmu-A.png) |
| e02 truss HETA | 5 > **7** | 1.00 > 1.00 | 0 > 0 | 4m36 > 4m09 | 28 > 26 | 15 (#19) > 2 (#14) | 2+8 > 3+4 | $1.40 > $1.40 | [A](2026-10-06-alpha4/e02-truss-heta-A.png) |
| e03 monopour recess | 7 > **8** | 1.00 > 1.00 | 0 > 0 | 6m39 > 2m59 | 50 > 17 | 30 (#49) > 3 (#8) | 7+12 > 0+3 | $2.05 > $1.06 | [A](2026-10-06-alpha4/e03-monopour-recess-A.png) |
| e04 monopour plain | 6 > **8** | 1.00 > 1.00 | 0 > 0 | 3m43 > 2m12 | 26 > 21 | 10 (#15) > 1 (#6) | 2+8 > 0+2 | $1.34 > $0.97 | [A](2026-10-06-alpha4/e04-monopour-plain-A.png) |
| e05 flush beam | 8 > **8** | 1.00 > 1.00 | 0 > 0 | 3m59 > 2m49 | 38 > 15 | 11 (#15) > 0 (#6) | 3+11 > 0+1 | $1.45 > $0.99 | [A](2026-10-06-alpha4/e05-flush-beam-A.png) |
| e06 flush beam 2x6 | 6 > **8** | 0.96 > 1.00 | 2 > 0 | 5m18 > 1m41 | 31 > 5 | 9 (#14) > 0 (#3) | 3+6 > 0+1 | $1.58 > $0.56 | [A](2026-10-06-alpha4/e06-flush-beam-2x6-A.png) |
| e07 edit notes | 6 > **6** | 1.00 > 1.00 | 0 > 0 | 0m39 > 0m53 | 5 > 7 | 0 > 1 (dry run) | 0+0 > 0+1 | $0.36 > $0.54 | [A](2026-10-06-alpha4/e07-edit-notes-A.png) |
| e08 edit remove | 8 > **8** | 1.00 > 1.00 | 0 > 0 | 0m54 > 0m22 | 12 > 3 | 0 > 0 | 3+2 > 0+0 | $0.51 > $0.39 | [A](2026-10-06-alpha4/e08-edit-remove-A.png) |
| e09 screenshot | 7 > **8** | 1.00 > 1.00 | 0 > 0 | 5m20 > 2m52 | 31 > 20 | 10 (#15) > 0 (#6) | 3+9 > 1+2 | $1.58 > $1.06 | [A](2026-10-06-alpha4/e09-screenshot-A.png), [reference](2026-10-06/e09-reference.png) |
| e10 ambiguous | n/a (asked) > **8** (built) | 1.00 > 1.00 | 0 > 0 | 0m15 > 2m33 | 1 > 11 | 0 > 0 (#6) | 0+0 > 0+1 | $0.21 > $0.79 | [A](2026-10-06-alpha4/e10-ambiguous-A.png) |

New gates (unknown keys, `W_VIEW_FIT`, `W_LEADER_HIT`, `W_NOTE_TARGET`): all pass on the 10 final alpha4 documents.
That is not because the agent never triggered them: `W_LEADER_HIT` fired 10 times during e01 alone and the agent
iterated until it was 0 (that loop is failure pattern 2 below).

## Per-detail review (alpha4)

- **e01 truss on CMU, 8/8.** Notes split to both sides, 1'-6" overhang dim, `ROOF SHTG.`/`GYP. BD.` abbreviations, 4:12 labelled on the roof, `W/ SILL SEALER` on the PT plate, soffit nailer with masonry screws. Real, apt citations (R802.10, R802.11, R905.2, R905.2.8.5, R317.1). Minor: the H2.5A leader lands at the heel but the tie itself is not legible; the PT-plate and nailer leaders are tight.
- **e02 truss on bond beam with HETA20, 7/8.** The HETA20 is now a full-length outline on the truss face with the embed depth moved into the note, grouted bond beam, `#5 VERT. ... (WHERE OCCURS)` drawn as a dashed ghost (the new `shown: dashed`), no fake barrier. Left: the anchor top pokes into the roof layers, no uplift/wind note for "Florida, high wind" (only `PER MFR.`), and the 4" embed dim was dropped because it was illegible at scale 3/4"=1'-0". R2 1.
- **e03 monopour with recess, 8/8.** Recess, turndown, vapor retarder and gravel along the haunch, notes both sides. Small cosmetic issue: the nested vertical dims `1'-6"` and `1'-0"` overprint each other's text.
- **e04 monopour plain, 8/8.** The anchor bolt is now centred in the 2x6 sill, correct 6" above-grade dim, studs read as a wall panel, `6'-0" O.C. MAX., (2) MIN. PER PLATE, 12" MAX. FROM ENDS` cites R403.1.6 correctly.
- **e05 flush beam, 8/8.** Same end-on elevation as before, now with LVL grain, a working vertical `9 1/4"` dimension, and a real Simpson part (ST6224 strap with a nailing note, R602.6.1).
- **e06 flush beam 2x6 PSL, 8/8.** Different, arguably more standard reading than before: the PSL runs along the wall as a flush header, plates butt to it, 2x6 king + (2) jack studs at its end, CMST14 centred on the joint with dimensions, 0 warnings, citations actually rendered (R602.3.2, R602.7). Note that e05 and e06 now draw the "flush beam" prompt two different ways (see pattern 5).
- **e07 edit, 6/8.** The agent now sets `roof_sheathing.slope: "@truss"` (pitch single-sourced, as the guide intended) and used `--dry-run`. But the drawing is unchanged from baseline in its defects: the sheathing keeps its fixed 66" length and ends short of the truss at the crop, and the heel/bird-block zone is cluttered. The `until`-along-slope feature was not used. R1 1, R2 1.
- **e08 edit, 8/8.** 3 kerf calls, 22 s. Fiber note, slab bars removed, footing steel kept, leaders clean.
- **e09 screenshot, 8/8.** Matches the reference in geometry and wording, citations identical, leaders no longer cross over the footing. The slab bar is still a heavy black bar rather than the reference's light one.
- **e10 ambiguous, 8/8 (built).** Behaviour changed: baseline asked one question and built nothing; alpha4 built a continuous footing under an 8" stem wall with crawlspace (20"x10" footing, #4 dowels hooked into it, J-bolt, sill sealer) and stated the assumption plus the alternatives. The detail is clean and plausible; the `question/assumptions` check passes either way.

## Remaining failure patterns, ranked

1. **Leader/label avoidance is still done by the agent, not the engine.** `W_LEADER_HIT` now exists and is accurate,
   but nothing resolves it: e01 took 6 apply+export rounds ("Placing the plate note by coordinate backfired (left-aligned text in
   the drawing)", "OSB note jumped right and crosses the truss leader"). Ten `W_LEADER_HIT` messages in e01, a label clash
   in e04. Improve: have the layout nudge landing points within the target, move a note to the other side, and push labels/dims
   away; give notes a per-note side hint; fix left-aligned text for `place` on the left column.
2. **Rendering conventions still lose information.** Path-mode rebar draws as a heavy solid bar (e03, e09 vs the
   reference's light line), straps are a hairline, the H2.5A tie is not legible (e01), vertical dims stack with overprinted
   text (e03), and a small embed dim becomes illegible so the agent drops it (e02). Improve: lighter pen for rebar paths,
   dim stacking with automatic offsets, dim text moved outside when it does not fit, a minimum drawn gauge for hardware.
3. **Dependent edits are only half-solved.** `slope: "@truss"` was adopted, but panel `length` is still a literal 66"
   (e07 sheathing/roofing end short of the truss at the crop; the same visible defect as the baseline), and the new
   `until` along the slope was not discovered. Improve: make `schema panel` and the guide example show
   `slope: "@truss", until: "truss@top_chord_end"`, or add a `roof` assembly; emit a warning when a sloped panel ends short of the
   member it is attached to.
4. **Sandbox friction is 19 of 24 tool errors, and the pattern is compound commands**: `kerf new ... && cat > ops.json <<'EOF'`,
   `ls && kerf apply`, and `kerf schema a; kerf schema b` (e02 lost the doc creation: "The doc wasn't created by the blocked
   command"). The guide's own examples do this. Improve: show one-command-per-line recipes (write ops with the Write tool, then
   `kerf apply f ops.json -w --why ...`); let `kerf schema` take several topics; let `kerf new --ops`.
5. **Ambiguity handling is inconsistent.** e10 flipped from asking to building; e05 and e06 (near-identical prompts)
   drew an end-on elevation and an along-wall header with no question and no mention of the other reading. Improve: one
   written policy in `spec/llm/system.md` and the guide (ask only when the structure changes; otherwise build and list the
   alternative reading in the report), and a catalog note on the standard view for beam-in-wall details.
6. **The guide is 44 KB.** Claude Code's tool-result limit spills it to a file ("Output too large ... saved to"), so the agent
   spends a `Read` per run and may read it partially. Improve: `kerf guide` = workflow + view/annotation schema + example
   (a few KB); catalog on demand via `kerf schema <type>` (which agents did use: 0-8 `schema` calls per build, 2-4 typically).
7. **`W_VIEW_FIT` can still be hit with an explicit scale** (e02 at 3"=1'-0"), and `acknowledge` of an info code returns an
   error (e09 tried `I_SOLID_USED`; the message was clear, the agent recovered). Low priority.
8. **Eval signal.** All 10 final documents score 1.000, including e07 whose drawing is visibly worse than the rest. Next
   step: geometric checks (a sloped panel's end vs its host member's end, strap length vs beam width, dim text overlap count
   from `kerf drawing`), and an LLM/visual judge in the loop so the rubric is not hand-scored only.

## What improved most (evidence)

- Discovery cost collapsed: probe calls before the first write 94 > 8; no `strings`/`curl`/WebFetch attempts; no "apply failed with no message" (apply
  errors are printed with a fix; the one engine error in e09 was `E_PARAM ... 'I_SOLID_USED' cannot be acknowledged: only warnings (W_*) can`).
- Unknown keys: 0 in all 10 documents (baseline e06 silently dropped its citation).
- Warnings carried into the final documents: 2 > 0, with `W_NOTE_TARGET`, `W_VIEW_FIT`, `W_COVER` (e09 slab bar cover 2" < 3") each leading to a fix.
- Time and money: 33% faster, 26% cheaper overall; every from-scratch build except e02 dropped by 25-70% in wall time.
