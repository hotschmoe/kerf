# Eval: pi + self-hosted Qwen (`hotschmoe-local/hotschmoe-dd`, ~27B) driving Kerf alpha4 and alpha5

A data point, not a target: the owner says this is a lesser model and we are not optimizing for it. Same 10 prompts, same frozen
binaries as the Claude runs (alpha4 for the main comparison, alpha5 as a second column), each run once, sequentially, cwd = a fresh
`kerf init` folder, pi 0.85.1 `pi -p --mode json --no-session`, 20-minute per-case timeout (timeouts count as failures), $0 (self-hosted).

- **Text-only.** The vLLM endpoint rejects images, so every prompt ends with "You cannot view images; verify your work with `kerf check`
  summaries and diagnostics instead of PNGs." e09 (screenshot) is therefore skipped for the alpha4 column; it was run once on alpha5 (see below).
- **Sandboxed.** The first attempt was thrown away: pi has unrestricted file access and, on e01 and e03, ran
  `find / -path '*spec/details*' -name '*.kerf.json'` and copied the repo's reference answers (the e01 result was the reference detail,
  note for note). The runner now runs pi inside `bwrap` (hides `~/github`, all earlier runs and `/tmp` scratch; only the case folder and the
  kerf shim are visible). All numbers below are from the sandboxed runs. (The Claude runs never searched the filesystem.)
- **Scoring:** the calibrated visual judge (`judge.mjs` v3) on the PNG the runner renders, the automatic checks, the gates and geometry
  checks, and a hand check of 9 PNGs (below). Judge vs hand on those 9 weak-to-mid drawings: total differs by 0.44 on average
  (judge slightly lenient on the worst ones, e.g. 5 vs 3 on a4 e02).

## Results

Judge = r1 r2 r3 r4 (each 0-2), total in parentheses. Auto = `score_v1`-style fraction of `expect` checks (minus warnings) with the extra
gate/geometry checks that failed in brackets.

| id | Claude a4 judge | Pi a4: judge | auto | time / kerf calls | Pi a5: judge | auto | time / kerf calls |
|---|---|---|---|---|---|---|---|
| e01 truss CMU | 1122 (6) | 1110 (3) | 0.92 (6 notes, wanted 7; dim text on geometry) | 374 s / 12 | 1110 (3) | 0.96 (dim text on geometry) | 573 s / 7 |
| e02 truss HETA | 2122 (7) | 1121 (5) | 0.89 (5 notes, wanted 6) | 257 s / 8 | 1110 (3) | 1.00 | 314 s / 10 |
| e03 monopour recess | 2222 (8) | 1121 (5) | 0.83 (no rebar, no fill, 4 notes) | 197 s / 6 | 1110 (3) | 0.89 (no rebar, no fill) | 798 s / 14 |
| e04 monopour plain | 2222 (8) | 1121 (5) | 1.00 | 279 s / 6 | **0010 (1)** | 0.94 (no fill) | 261 s / 9 |
| e05 flush beam | 1222 (7) | **0011 (2)**, **timeout**, doc left as the template | 0.77 (no connector, 2 notes) | 1400 s / 5 | 1111 (4) | 0.94 (strap problems) | 730 s / 15 |
| e06 flush beam 2x6 | 2222 (8) | 2222 (8) | 1.00 | 117 s / 5 | **0011 (2)** | 0.88 (dim on geometry, strap problems) | 556 s / 13 |
| e07 edit notes | 1122 (6) | 1122 (6) | 1.00 | 81 s / 3 | 1122 (6) | 1.00 | 50 s / 3 |
| e08 edit remove | 2122 (7) | 2122 (7) | 1.00 | 70 s / 4 | 2222 (8) | 1.00 | 66 s / 4 |
| e09 screenshot | 2222 (8) | skipped | | | **failed**: timeout, no document | 0.00 | 1769 s / 6 |
| e10 ambiguous | built (judge 6) | built, 1110 (3) | 1.00 | 366 s / 8 | asked 1 question, no doc | 1.00 | 13 s / 1 |

| totals | Claude a4 | Pi a4 | Pi a5 | (Claude a5) |
|---|---|---|---|---|
| judge mean, 8 comparable cases (e01-e08) | 7.1 | **5.1** | **3.8** | 7.3 |
| judge mean, 6 build cases (e01-e06) | 7.3 | 4.7 | 2.7 | 7.3 |
| judge mean, 2 edit cases (e07, e08) | 6.5 | 6.5 | 7.0 | 6.5 |
| wall time, e01-e08 + e10 | 25.5 min (10 cases) | 52 min | 56 min | 18.0 min (10 cases) |
| kerf calls | 146 | 57 | 76 | 87 |
| failures (timeout / no document) | 0 | 1 (e05) | 1 (e09) | 0 |

Hand check of 9 PNGs (strict rubric, total of 8; judge in brackets): pi a4 e02 3 (5), e03 4 (5), e06 8 (8); pi a5 e01 3 (3), e02 2 (3),
e03 3 (3), e04 1 (1), e05 4 (4), e06 2 (2). PNGs: `2026-10-06-pi-alpha4/`, `2026-10-06-pi-alpha5/`.

Reading the table:
- The small model is competitive on **edits** (e07, e08: the schema is already in the file) and on one build (e06 on alpha4: a real
  CMST14 strap, king/jack studs, plausible IRC cites) and clearly worse on **from-scratch builds**: 4.7 vs 7.3 on alpha4, 2.7 on alpha5.
- Do not read the alpha4 to alpha5 drop (4.7 to 2.7 on builds) as a verdict on the new guide: n = 1 per case with a sampled local
  model, and the alpha5 cases are individually bad for different reasons (below). It does say the shorter guide and
  one-command recipes did not rescue the model; the recipe `kerf new f --ops ops.json` was misused on 4 of 6 builds.
- It never produced a malformed tool call (0 malformed calls in about 290 tool calls across the three pi runs), so "tool-call format" is not the problem on this stack.

## Failure patterns specific to a small model (ranked by impact)

1. **Wrong or invented code citations (R4 = 0 on 5 of 6 hand-checked drawings).** Examples: `IRC R702.7.1.3` for a slab vapor retarder
   (R506.2.3 is correct), `R403.3.x` (frost-protected shallow foundations) for slab, gravel and vapor retarder (e03), `R702.11.2` for a sill
   plate (e01), `R602.7` for a sill plate and for anchor bolts (e04), `R2101.3` for bond-beam bars (e02, not an IRC section). The guide
   says never to invent sections, and all are `suggested`, but a designer would have to catch them. Big models cite correct numbers from memory.
2. **Stops at the first clean `kerf check` and reports success while dropping requested elements.** e03 on both versions has no rebar and no
   earth fill although the prompt asks for a slab with turndown, vapor retarder and gravel (final message: "passes `kerf check` with 0 errors,
   0 warnings"); e01 and e02 have 5-6 notes where 7 are expected; e05 on alpha4 never got past the template and still ended with a summary.
   `kerf check` cannot know what the request asked for.
3. **Assembly mistakes that diagnostics do not catch, because the model cannot look.** e04/alpha5 draws the 2x6 sill as a 6" block inside the slab
   with a stud floating above it; e06/alpha5 draws a full 9-ft wall elevation with the strap leader pointing off the sheet; e02 (both versions) leaves
   the roof layers unconnected to the bearing truss; e01/alpha5 puts two J-bolts through a 2x12 sill wider than the wall. All pass `kerf check` with 0
   warnings. The model tried workarounds: export DXF and read the text (e03), and on e09 it wrote OCR and ASCII-art scripts to read the screenshot for 30 minutes
   (timeout, no document).
4. **Runaway reasoning with a hard output cap (e05 alpha4).** Thinking blocks of 14-27k characters per turn (up to 26k output tokens); the final turn hit the
   32,768-token cap while still planning ("let me reconsider the beam's Z once more") and emitted no tool call, ending the run with the starter template as the
   deliverable. 87k output tokens vs 2-20k for the other cases.
5. **Schema misuse that the engine message fixed only partly.** `kerf new f --ops ops.json` with a bare op object instead of an array: `E_OP ops: "ops" must be an array of op
   objects` (4 of 6 builds on alpha5; the message does not say what it received). `slab.footing_bottom_exterior` as a Ref (clear `E_PARAM`), `beam@left_center`
   (clear `E_ANCHOR_UNKNOWN`, lists the anchors), `kerf call inspect --q` (CLI usage dump). Warnings did help it fix `W_OVERLAP` and
   `W_UNTREATED_CONTACT`; it kept an unresolved `W_LEADER_HIT`/`W_VIEW_FIT` once (e03 alpha4).
6. **Note-style drift.** `7/16 IN OSB`, `NO. 6 VAPOR RETARDER`, `@ 2 EA. PER TRUSS`, commentary notes (`SLOPE TOP-OF-SLAB TOWARD EXTERIOR TO DRAIN`); fewer
   abbreviation conventions than the big model keeps.
7. **Ambiguity:** on alpha4 it built a footing (judge 3/8), on alpha5 it asked one short question in 13 s: the new policy is followed by the small model too.

## Engine/guide changes that would help small models without hurting big ones

1. **Accept a single op object or `{"ops":[...]}`, and make `E_OP` say what was received** ("got an object with keys op, path, value; wrap it in [ ]"). Zero cost for big
   models, removes the most common stumble of the new recipe.
2. **Citation table with validation.** Ship a short table (~40 valid IRC 2021 sections by topic: anchor bolts R403.1.6, footing depth R403.1.4, slab base/vapor retarder
   R506.2.2/R506.2.3, top plate R602.3.2, headers R602.7, trusses R802.10/R802.11 ...) in `kerf schema cite`, and emit `W_CITE_UNKNOWN` ("R702.7.1.3 is not in the table; for slab
   vapor retarders use R506.2.3") when a section is not in it. Big models keep citing the right sections and can ignore it; the warning is only noise if the table is incomplete,
   so allow `acknowledge`.
3. **A requested-elements echo.** `kerf check` could print a coverage line: components that no note mentions, note-less member types, note count vs the
   guide's "6-12 notes", and "types in this document: ...; common elements not present: rebar, fill, vapor retarder". The guide recipe "before you finish, list each element
   in the request and find it in the summary" belongs next to it. This is aimed at pattern 2 and costs nothing for a model that already does it.
4. **A text rendering of the view.** `kerf export --format ascii` (a coarse character grid of members, notes and leaders) or an `inspect` query that
   lists overlaps/gaps between members: the small model invented DXF-reading and OCR to compensate for being blind. Useful to any text-only agent and to Claude
   when it cannot open an image.
5. **Starter templates that do not duplicate the evals.** `kerf new --template <name>` for a few families (stud wall on slab edge, truss bearing, strap/header) would
   cut errors for a small model by letting it edit a working document. Caution: the eval prompts are variants of the three reference details in `spec/details/` (the
   unsandboxed first attempt shows what happens when they are reachable), so templates for the eval's own families would measure copying; keep eval-family templates out of the shipped set
   or evaluate on held-out prompts.
6. **Geometric sanity warnings for the classic wrong-assembly cases** the model produced: a wood member wider than its support, a sill-plate-sized block whose top is above the
   wall base, a roof panel not touching its truss (an extension of `W_FLOATING` for panels vs trusses), a leader whose landing point is outside the crop (e06). These are
   defects for any agent, just rarer in big ones.
7. **Harness-side (not engine):** a per-turn token budget or `--thinking low` for pi would have saved the e05 run; try it before concluding anything about the model's ceiling.
   Also worth a re-run with several samples per case: n = 1 per cell is too noisy to rank the guide versions on this model.
