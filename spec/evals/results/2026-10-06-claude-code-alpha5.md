# Eval: Claude Code (headless) driving Kerf alpha5 (v0.1.4), 3-way vs baseline and alpha4

- Same 10 prompts, same agent (Claude Code 2.1.289, `claude-opus-5-5`), each run once, sequentially, same permission
  set as the alpha4 run (read-only shell tools allowed). Engine: frozen `kerf-alpha5` (SPEC §20: layout self-repair and
  dim stacking, `column` hint, rebar centerlines, legible hardware, `W_SHORT_SLOPE`, 10.7 KB guide with one-command recipes
  and an ambiguity policy, multi-topic `kerf schema`, `kerf new --ops`).
- Scoring: the calibrated visual judge (`judge.mjs`, prompt v3, run inline: one `claude -p` call per case), the automatic
  checks plus the gates (unknown keys, `W_VIEW_FIT`, `W_LEADER_HIT`, `W_NOTE_TARGET`) and the new geometry checks
  (`geom.mjs`), and a hand spot-check of every PNG. Judge test-retest on the 9 alpha5 PNGs: 89% of criterion scores identical
  on a second pass, mean abs diff 0.11.
- Data: `2026-10-06-alpha5/` (PNGs, `summary.json`). Transcripts stay in `~/kerf-eval/runs/2026-10-06T00-26-40-314Z/`.
  The grader now knows the new note key `column` (the first regrade flagged it as an unknown key; the engine accepts it).

## Headline

alpha5 does not move quality again (judge mean 7.2 on both alpha4 and alpha5, hand 7.4-7.7), but it makes the agent
much cheaper and cleaner: no probing at all, no sandbox friction, 40% fewer kerf calls than alpha4.

| | baseline | alpha4 | alpha5 |
|---|---|---|---|
| judge, mean total of the 9 built details (0-8) | 6.1 | 7.2 | **7.2** |
| hand rubric, same 9 (0-8) | 6.7 | 7.7 | 7.4 |
| auto `score_v1` (mean of 10) | 0.996 | 1.000 | 1.000 |
| gate/geometry failures (sum over 10 docs) | 6 | 1 | **0** |
| warnings left in final docs | 2 | 0 | 0 |
| wall time, 10 prompts | 37.9 min | 25.5 min | **18.0 min** |
| kerf calls | 258 | 146 | **87** |
| probe calls before first real write / in total | 94 / 138 | 8 / 18 | **2 / 2** |
| `kerf schema` lookups | n/a | 24 | 15 |
| tool errors (engine + sandbox) | 24 + 71 | 5 + 19 | **3 + 0** |
| cost | $12.43 | $9.23 | **$7.13** |
| output tokens | 212k | 142k | 101k |

Gate and geometry failures counted with the final `run-cli.mjs` (baseline: 2 leader/dim-text clashes in e01, a dim text
on geometry in e03, a leader through dim text in e08, the dropped `citations` key in e06; alpha4: one dim text sitting on
linework in e05). The independent geometry checks are all 0 on the 9 alpha5 documents (leader crossings, leaders through
text, dim-text overlaps, text overlaps, sloped panel vs host, strap overhang).

## Per prompt, alpha4 > alpha5

Judge = r1 r2 r3 r4 (0-2 each, one call, prompt v3). Hand = my score of the PNG, total of 8. probe BFW = probe calls before
the first `apply -w` (and the call number of that write). Tool errors = engine + sandbox.

| id | judge a4 > a5 | hand a4 > a5 | time | kerf calls | probe BFW (first write) | tool errors | cost | a5 PNG |
|---|---|---|---|---|---|---|---|---|
| e01 truss CMU | 1122 > 2122 (6 > 7) | 8 > 8 | 303 > 168 s | 21 > 13 | 1 (#7) > 0 (#7) | 1+4 > 0+0 | $1.48 > $1.08 | [A](2026-10-06-alpha5/e01-truss-cmu-A.png) |
| e02 truss HETA | 2122 > 2122 (7 > 7) | 7 > 5 | 249 > 154 s | 26 > 10 | 2 (#14) > 0 (#6) | 3+4 > 0+0 | $1.40 > $1.01 | [A](2026-10-06-alpha5/e02-truss-heta-A.png) |
| e03 monopour recess | 2222 > 2222 (8 > 8) | 8 > 8 | 179 > 159 s | 17 > 15 | 3 (#8) > 0 (#8) | 0+3 > 1+0 | $1.06 > $0.89 | [A](2026-10-06-alpha5/e03-monopour-recess-A.png) |
| e04 monopour plain | 2222 > 2122 (8 > 7) | 8 > 8 | 132 > 124 s | 21 > 13 | 1 (#6) > 0 (#6) | 0+2 > 0+0 | $0.97 > $0.94 | [A](2026-10-06-alpha5/e04-monopour-plain-A.png) |
| e05 flush beam | 1222 > 2222 (7 > 8) | 8 > 8 | 169 > 138 s | 15 > 7 | 0 (#6) > 0 (#6) | 0+1 > 0+0 | $0.99 > $0.83 | [A](2026-10-06-alpha5/e05-flush-beam-A.png) |
| e06 flush beam 2x6 | 2222 > 2222 (8 > 8) | 8 > 8 | 101 > 141 s | 5 > 11 | 0 (#3) > 0 (#6) | 0+1 > 1+0 | $0.56 > $0.98 | [A](2026-10-06-alpha5/e06-flush-beam-2x6-A.png) |
| e07 edit notes | 1122 > 1122 (6 > 6) | 6 > 6 | 53 > 57 s | 7 > 5 | 1 (#4) > 1 (#4) | 0+1 > 0+0 | $0.54 > $0.35 | [A](2026-10-06-alpha5/e07-edit-notes-A.png) |
| e08 edit remove | 2122 > 2122 (7 > 7) | 8 > 8 | 22 > 25 s | 3 > 4 | 0 (#2) > 1 (#3) | 0+0 > 1+0 | $0.39 > $0.29 | [A](2026-10-06-alpha5/e08-edit-remove-A.png) |
| e09 screenshot | 2222 > 2122 (8 > 7) | 8 > 8 | 172 > 102 s | 20 > 8 | 0 (#6) > 0 (#7) | 1+2 > 0+0 | $1.06 > $0.60 | [A](2026-10-06-alpha5/e09-screenshot-A.png) |
| e10 ambiguous | built (judge 6) > asked | n/a | 153 > 10 s | 11 > 1 | 0 > 0 | 0+1 > 0+0 | $0.79 > $0.15 | none |

Auto `score_v1` is 1.000 on all ten (the grader remains saturated). The hand column for alpha4 uses the totals from the alpha4
report; the e02 alpha5 hand score fell from 7 to 5 after the judge pointed out the HETA20 is drawn at about 11 in (a 20-in
part) and I rechecked (so alpha5 hand scores are not fully independent of the judge).

## What changed in the drawings (hand review)

- **Rendering fixes landed and show in the PNGs:** hurricane tie now legible with nail dots (e01, e07), HETA20 as a thick bar with a
  "4" EMBED." dim that is readable (e02), the e09 slab bar is a light centerline like the reference, vertical dims no longer
  collide (e03 nests `6"`, `1 1/2"` and a rotated `4"` cleanly), notes use both columns without a single crossing leader.
- **e05/e06 now agree with each other:** both draw a flush header in elevation with an opening, king + jack studs, strap across the
  broken plates with real CMSTC16/CMST14 part numbers and 2'-0" strap extents (alpha4 drew them two different ways).
- **e10:** the new guide's ambiguity policy makes the agent ask before building ("each option is a different structural
  system"), as the baseline did; alpha4 built with stated assumptions. That is consistent with the written policy but a
  headless run ends with no artifact; the three-option question is good.
- **Not fixed:** e07 (6:12 edit) has the same defects as in alpha4 (apart from the now-legible tie): the crop still clips the steeper roof at the top and the
  heel/bird-block zone is cluttered.

## Remaining failure patterns, ranked

1. **Hardware drawn at the wrong length (e02, e04).** The HETA20 anchor is drawn about 11 1/2" long although the part is 20"
   (judge: "should run about 16" up the truss heel, not about 7 1/2""); in e04 the bolt projects 2 5/16" over a 1 5/8" sill
   so it pokes into the stud and the nut/washer are not visible. The agent supplies polyline points and the engine does not
   cross-check them against the `model` length table. Improve: draw the connector from `model` (length from the table, one
   anchor point and a direction), or warn `W_HARDWARE_LENGTH` when drawn length differs from the table by more than 10%.
2. **Edits that change extents do not re-fit the view (e07).** Roof 4:12 to 6:12 leaves the crop clipping the roof raggedly at
   the top (same as baseline and alpha4); `W_SHORT_SLOPE` does not fire because the panel is clipped, not short. Improve: warn
   when an edit makes a previously whole member clipped by an explicit crop (`W_CROP_CLIPS`), or grow an auto-fit crop on apply.
3. **Thin layers get leaders on the wrong layer (e08, judge):** the `VAPOR RETARDER` leader lands on the gravel/soil line about
   8" below T.O. slab instead of on the 1/16" membrane. The landing heuristic ("inside the visible region") has nothing to
   land inside for a thin membrane. Improve: land on the polyline itself for membranes and flashings.
4. **Sloppy note text the lints do not catch (e02):** `UNDERLAYMENT PER FBC-R` (a code name with no section, plus the cite
   machinery), `TRUSS BEARING ON GROUTED BOND BEAM W/ NO PLATE` (commentary), `PER MFR.` repeated twice in one note (e06),
   and a dimension whose text was overridden to `PER MFR.` (e06). Improve: `W_NOTE_STYLE` for a dangling code name, a repeated
   phrase, and a `dim.text` that is not a measurement; the guide's note examples should show citation text only through `cite`.
5. **View edges that look like real ends (e09, judge):** the slab/gravel stop at a closed vertical edge while the soil hatch and
   the gravel underside run about 4" further, so the right edge reads as an unfinished boundary instead of a break line.
   Improve: apply the break-line treatment to every material crossing the crop edge consistently.
6. **Failed-apply output order.** `kerf apply` (and `--dry-run`) now prints the error, which is the fix we wanted, but after the
   old summary whose header says `0 errors 0 warnings` and two INFO lines (e08: the real `E_REF_UNKNOWN` is the last line of
   the output). Improve: print `ERROR ...` and `nothing written` first.
7. **Eval signal remains the limit.** Auto score, warnings, gates and geometry checks are all clean; the only remaining
   differentiators are fidelity items (1-3), which no automatic check sees. Next step: a check that compares drawn connector
   length against the model table, and the judge in CI with its three-run median.

## What improved (evidence)

- Probing is gone: 2 probe calls in total (was 138 in the baseline, 18 in alpha4). First write at call 6-8 on every build
  (guide, schema lookups, new), none at 13-49.
- Sandbox friction is gone: one-command recipes in the guide (`kerf new --ops`, `apply f ops.json -w`) removed the compound
  commands that cost 19 tool errors in alpha4. The three remaining engine errors were all legitimate and clear
  (`E_CYCLE` placement cycle with a fix hint in e06; `E_REF_UNKNOWN` on removing a component still referenced by a note in e08;
  one failed `apply` in e03 whose error line was cut from the digest).
- Time and cost: 18.0 min and $7.13 for the suite, 53% and 43% below baseline.
