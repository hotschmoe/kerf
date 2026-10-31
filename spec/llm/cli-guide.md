# Kerf CLI: guide for agents (Claude Code, Grok, ...)

You build construction details with the `kerf` command-line engine. A detail is one JSON file
(`<id>.kerf.json`) in the current folder. The designer reviews it in
`kerf serve` and verifies citations. You describe construction
with typed components; the engine places, draws, lays out notes and checks.

## Shell rules (the sandbox rejects compound commands)
- ONE kerf command per call. Do NOT chain with `&&`, `;` or `|`, do NOT use heredocs (`<<EOF`) or `echo '...' | kerf`.
- Create JSON files with your file-writing tool (Write), then point kerf at the file (inline JSON breaks on PowerShell).
  Never hand-edit a `.kerf.json`.

## Workflow
1. Ambiguity: ask ONE short question only if the answer changes the structural system or the load path (CMU vs
   stud wall, truss vs rafter, embedded anchor vs plate, what bears on what). Otherwise build with conventional
   choices and report your assumptions AND the main alternative reading. "Give me a footing detail" alone is
   ambiguous: ask which (wall, pad or slab-edge footing).
2. Write `ops.json` with the Write tool. First build = one op `{"op":"set","path":"doc","value":{...whole document...}}`
   (a single op object or `{"ops":[...],"why":"..."}` is accepted too). Put the designer's asks in `meta.requested`, one short
   phrase each, in their words: `"meta":{"requested":["cmu wall","bond beam","H2.5A ties","#5 rebar","vapor retarder"]}`.
3. Create and apply in one command (nothing is written if an op fails):
```sh
kerf new truss-cmu.kerf.json --ops ops.json --title "PREFAB TRUSS BEARING AT CMU WALL" --why "Build initial detail"
```
4. Read the summary (every component with resolved x/y extents), the `COVERAGE` block and the diagnostics. Fix every ERROR;
   fix or justify every WARN. Every `meta.requested` item must read `ok` (build the `MISSING` ones, or drop an item only if the
   designer dropped it): do not finish before full coverage, a clean check is not a finished detail. Covered = a
   component (id/type/label/model) or note/dim/weld/table/view title holds all its words (EA.=EACH). Then LOOK at it:
```sh
kerf export truss-cmu.kerf.json --view A --format png -o truss-cmu-A.png
```
5. Refine with small ops (a new ops file each time; keep ids stable):
```sh
kerf apply truss-cmu.kerf.json ops.json -w --why "Add HETA20 anchors at 16 in o.c."
kerf apply truss-cmu.kerf.json ops.json --dry-run
kerf check truss-cmu.kerf.json
kerf export truss-cmu.kerf.json --view A --format pdf --sheet -o truss-cmu-A.pdf
kerf schema note dim cite
kerf guide --full
```
- Always pass `--why "..."` with `-w`/`new`: one line for the designer, logged to `<file>.log.jsonl`.
- A failed apply prints `ERROR <code> <path>: <message>  Fix: <fix>` for every error, then `nothing written` (exit 1).
- Ops: `add` (path `components` | `views` | `views/<id>/annotations`), `update` (path `components/<id>` | `views/<id>` |
  `views/<id>/annotations/<id>` | `meta`; value = merge patch, null deletes a key), `remove` (path), `set` (path `doc`).
- Fix warnings; acknowledge one only if it is right for this detail, on the element it names, with a reason:
  `"acknowledge":[{"code":"W_UNTREATED_CONTACT","reason":"..."}]`. The summary counts and lists them (`kerf check --strict`
  prints each). Wood on CMU wants `"barrier":"sill_seal"`. No fake geometry.
- Scenarios: one view each, one origin; scenario members get `"views":["B"]`, shared none; one sheet: `kerf schema layout`.
- Unknown keys warn (`W_UNKNOWN_KEY` names the right one). `kerf schema <topic>` before guessing a field name.

## Diagnostics you will meet (each prints its own fix)
W_NEAR_MISS (use `until`), W_FLOATING / W_OVERLAP (mis-placed), W_COVER, W_UNTREATED_CONTACT (`treated` or `barrier`),
W_SHORT_SLOPE, W_LEADER_HIT / W_VIEW_FIT (layout), W_NOTE_STYLE / W_ABBREV (note text), W_UNKNOWN_KEY,
W_REQUESTED_MISSING, W_CROP_STALE (drop `crop` and `scale` to re-fit), W_REFERENCE_FILE, W_ACK_UNUSED, I_* (info).

## Placement (relative, never computed coordinates)
- `"at": {"anchor": "bottom_left", "to": "bond_beam@top_left", "offset": [0, 0]}`: the member's own anchor lands on the Ref.
  Refs: `"comp@anchor"`, `"comp.part@anchor"`, `"comp#2@anchor"` (array instance), `"@origin"`, `{"ref":"x@center","offset":[dx,dy]}`, `[x,y]`.
- Every component has the 9 box anchors (top_left top_center top_right middle_left center middle_right bottom_left
  bottom_center bottom_right); builders add named ones (`kerf schema <type>`; `kerf schema refs`).
- Inches, X right, Y up, Z toward the viewer; lengths are numbers or strings like `"7-5/8"`, `"3'-4 1/2\""`. Use actual
  sizes through typed components (`"2x6"`, not 1.5x5.5). Rebar: `"place":{"in":"footing","face":"bottom","cover":3,"count":2}`.
- `"until": "<Ref>"` instead of `length` grows a member to another one's edge (no stale numbers).
- Braces/webs: `"from":"tie@top_center","to":"chord@bottom_center"` (ends cut to the faces they meet). Rafter seat:
  `"cuts":[{"type":"birdmouth","on":"plate"}]`; plumb ends, notches: `kerf schema cuts`.
- Steel: `steel` takes AISC shapes (`W8x18`, `HSS4x4x1/4`, `L4x3x3/8`, `PL1/2x8`), `orient`
  section|elevation|plan; `bolt` groups; `weld` annotation = AWS symbol (`kerf schema steel bolt weld`).

## Roof pitch: change it in ONE place
Trusses own the pitch; sheathing and roofing follow it and run to the truss end. Never type a literal length or pitch on them.
```json
{"id":"truss","type":"truss","pitch":"6:12","exterior":"left","overhang":18,"span_shown":36,"at":{"anchor":"bearing_outer","to":"sill_plate@top_left"}}
{"id":"roof_sheathing","type":"panel","material":"osb","thickness":0.4375,"slope":"@truss","until":"truss@top_chord_end","at":{"anchor":"bottom_left","to":"truss@tail_top"}}
```
New pitch: `update components/truss {"pitch":"6:12"}`; `W_SHORT_SLOPE` then prints the fix for each sloped panel/membrane
that stops short (`{"slope":"@truss","until":"truss@top_chord_end","length":null}`).

## Notes: one engineer's voice
UPPERCASE, no trailing period, no sentences: `<SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>`, e.g.
`2X6 P.T. SILL PLATE W/ 5/8" DIA. ANCHOR BOLTS @ 48" O.C.`, `(2) #5 CONT. BOT.`. Fractions `1 1/2"`, ` X ` between sizes.
Spell out or abbreviate; abbreviations in approved form (`TYP.` `U.N.O.` `CMU` `REQ'D`; `kerf schema abbreviations`).
`16d`, `db`, `f'c` keep case.
6-12 notes per detail: every structural element and connection, hardware model in the note, `INSTALL PER MFR.`.
Cite the 2021 IRC unless `meta.jurisdiction` says otherwise, only sections you are sure exist, always `"status":"suggested"`;
the designer verifies. Never present a design as engineered: capacities, spacing, nailing are the mfr.'s / EOR's.

## Views and annotations
`crop` and `scale` are optional (auto-fit: all non-fill components + 6 in; largest standard scale that fits);
set them only to frame deliberately (`W_VIEW_FIT` then checks). The layout places notes and clears leader collisions
(`column` on a note forces a side); a dim or label it moves prints I_ANNOT_MOVED (`"pin":true` keeps it). Keyed notes:
view `"notes_style":"keyed"` (numbered leaders + DETAIL NOTES list; `notes_side` above|left|right; same `key` = one
entry). Dim `dir` default: dominant axis; `offset` > 0 is above (h) / right of (v) the points, < 0 below / left.
Schedules: `table` annotations (`kerf schema table`) in a `"kind":"schedule"` view, not solids/labels.
Plans/elevations: view `kind` `plan` (cut at `cut_y`; plan [x,y] = model [x,-z]) or `elevation` (no cut), same members in
their real orientation; never a faked section. Beam in a wall: draw the elevation unless asked otherwise.
Keep going until the PNG reads as the detail an engineer expects: proportions, gaps, leaders on the right element, no overlaps.

<!-- FULL -->
## Long-form notes (kerf guide --full)
- `apply` on stdin (`kerf apply x.kerf.json - -w`) and inline (`--ops '[...]'`) exist for interactive shells; in an agent
  sandbox prefer an ops file. `kerf call help` lists the raw API functions with their input shapes.
- `kerf fmt <doc> -w` canonicalizes a document; `kerf drawing` / `kerf mesh` print the drawing IR / mesh JSON.
- Acknowledged warnings stay visible: the DOC line counts them (`0 errors  1 warning  3 acknowledged`) and an
  `ACKNOWLEDGED` block lists them by code with components and reasons (logged by `kerf apply -w`). `kerf check --strict`
  prints each as a warning and exits 1 on any warning; `kerf export ... --review` boxes the acknowledged components,
  annotations and titles and stamps the sheet `N ACKNOWLEDGED WARNINGS`. `W_ACK_UNUSED` names entries that suppress nothing and prints the op that
  removes them. Never acknowledge a warning you have not read in this check.
- Scenarios A/B/C in one document: one view per scenario, every scenario at the same origin (no model-space offsets, so
  anchors between shared and scenario members stay meaningful). Shared members get no `views`; a scenario's members get
  `"views":["B"]` (or several). A view can instead list `"only":["shared_*","b_*"]` or `"omit":["a_*","c_*"]` (ids or
  `*` globs; one that matches nothing warns `W_REF_UNKNOWN`, so ops can arrive in any order). Excluded members are not
  drawn (not even as beyond outlines) and skip that view's checks, so never acknowledge cross-scenario overlaps; a
  finding seen in only some views ends with `(in views A, C)`.
- One sheet for the scenarios (`kerf schema layout`): `"layouts":[{"id":"S1","views":["A","B","C","D"],"arrange":"grid",
  "columns":2,"sheet":"17x11"}]`, then `kerf export x.kerf.json --layout S1 --format png --sheet -o x-S1.png`. Each view
  keeps its bubble, title and scale; the keyed notes of all views share one DETAIL NOTES list numbered 1..N without gaps
  (same `key` = one entry across views; an unkeyed note with the same text joins it). Views without their own `scale`
  get the largest standard scale at which the whole sheet fits; a view's own `scale` wins, so a plan inset at 1/2"
  stands beside a 1" section: `"views":["A",["P1","P2"]]` stacks P1 over P2 beside A. Doc-level `"scale"` sets one
  scale for every view. Point at another detail with a `ref` annotation (`{"type":"ref","detail":"3","sheet":"S-50.01",
  "at":"beam@top_right","text":"SIM."}`); a view holding only a ref is a "see other detail" scenario. `W_VIEW_FIT` on
  `layouts/S1` lists every view's size and the fix that fits (larger `sheet`, other `arrange`, list `notes_side`).
- Write `meta.requested` in the designer's words, not in note spelling: coverage reads the house abbreviations both ways
  (`each`=`EA.`, `continuous`=`CONT.`, `clear`=`CLR.`, `face`=`FA.`, `on center`=`O.C.`), ignores case, hyphens and a plural
  s, and compares lengths (`15 ft` = `15'-0"`, `48in` = `4'-0"`). It counts components, notes, labels, dim text (or the
  measured length), welds (`3/16 fillet weld all around`, `cjp`), tables and view titles. A `MISSING` line names the closest
  candidate and the words it lacks; never stuff a component `label` with words to pass.
- Note case: penny sizes (`16d`), ACI symbols (`db`, `ld`, `ldh`, `f'c`, `fy`) and units like `psi` print as written, and a
  citation's numbered section (`1926.755(a)(1)`) keeps its case. A note may end on a house abbreviation (`PER ARCH.`).
  A style adds tokens with `text.keep_case` and abbreviations with `notes.abbreviations.approved` (`{"BRG.": "BEARING"}`).
- Abbreviations (W_ABBREV, one per token): spelling out is always fine; an abbreviation must be the approved form:
  `TYP` -> `TYP.`, `OC`/`UNO`/`EOR`/`NTS` -> `O.C.`/`U.N.O.`/`E.O.R.`/`N.T.S.`, `C.M.U.` -> `CMU`, `BOTT.`/`BTM.` -> `BOT.`,
  `PT` -> `P.T.`, `REQD` -> `REQ'D`, `PL.` -> `PL`. Its fix is the update op that rewrites only the flagged tokens of that
  text (model numbers, `16d`, `f'c` and quoted text are never touched). Where a deviation is right (PT = post-tensioned),
  add `"acknowledge":[{"code":"W_ABBREV","reason":"..."}]` to that note, label, dim, weld or view (title); it is counted
  and listed like a component's. `kerf schema abbreviations` prints the list in force.
- The company style is `kerf.kerfstyle.json` in the document's folder or above (`--style` overrides; `check`/`apply` print
  a `STYLE` line naming it). Its `notes.abbreviations` can approve, remap or remove entries, switch rules off and list
  words always abbreviated. It is the designer's: never edit it to silence a warning.
- Annotation layout (SPEC 25.2): an iso `at` Ref lies on its component's face nearest the viewer (W_NOTE_LANDING when an
  arrow lands on empty paper); dims the stacking or the leader repair move more than 1/8" (paper) and labels the repair
  moves print I_ANNOT_MOVED with the offset that keeps them there; W_LABEL_OVERLAP: labels, dim text or note text that
  overprint. Views keep 1/8" clear of the frame and title block.
- Dimension offset sign, example: below a footing `{"from":"ftg@bottom_left","to":"ftg@bottom_right","dir":"h","offset":-6}`;
  its depth on the outside (left) face `{"from":"ftg@bottom_left","to":"ftg@top_left","dir":"v","offset":-6}`.
- Lumber cuts and two-point members (`kerf schema cuts`). A brace, kicker, truss web or outlooker runs between two
  anchors: `{"id":"brace","type":"lumber","size":"2x4","face":"wide","from":{"ref":"tie@top_left","offset":[0.5,0]},
  "to":"blk@bottom_left","align":"top"}`: no `at`, `length`, `rotate` or `slope`; each end at a Ref is cut to the face of
  that member it meets, so it bears flat on both and follows them when they move (`align` top/bottom: that face runs between
  the points). A rafter on a plate: `{"type":"lumber","size":"2x10","run":"x","slope":"6:12","length":72,"cuts":[{"type":
  "birdmouth","on":"plate","tail":14},{"type":"plumb","end":"start"}]}`, no `at`: the seat lands on the plate's outer top
  corner (anchor `seat_outer`), `tail` is the horizontal overhang past the plate face; sheathing follows with
  `"slope":"@rafter","at":{"to":"rafter@start_top"},"until":"rafter@end_top"`. `W_BIRDMOUTH_DEPTH` past 1/3 of the depth;
  a W_OVERLAP of the rafter with the wall under its plate prints the `heel` that lifts it clear. A flush beam notched for
  a plate: `"cuts":[{"type":"notch","by":"top_plate"}]` (the plate's outline, where it meets the beam in z); a cope or
  end notch: `{"type":"notch","at":"top_right","w":3.5,"d":1.5}`. Steel in elevation/plan takes the same `from`/`to`/`cuts`.
- Rebar in a polygon host: `place` follows the outline (face `bottom` = the lowest bottom edge run, e.g. the thickened
  footing of an L slab; `in: "slab.footing"` cuts it to a part). A bar that cannot fit is `E_PLACE_OUTSIDE`; a bar in
  no concrete or masonry is `W_BAR_OUTSIDE`. Vertical bars in a CMU cell follow the wall width:
  `"points": [{"ref":"wall@cell_left","offset":[1.5,-12]}, {"ref":"wall@cell_left_top","offset":[1.5,-3]}]`.
- Aim notes and dims at bent bars and polygons with their own anchors, not box anchors (those sit in empty space):
  path rebar `start`, `end`, `bend_1`.., `seg_1_mid`..; polygons and point lists `vertex_1`.., `edge_1_mid`...
  A dowel that laps a wall bar: `"points": [{"ref":"ftg@bottom_center","offset":[2,3]}, {"ref":"wall_bar@start","offset":[2,24]}]`.
- `W_CUT_FILL`: an embedded part (bolt, bar) emptied a cut member's fill; the member keeps its fill and the warning
  says which part.
- Schedules (SPEC 26.1): one `table` annotation per schedule, in a view `{"id":"S","kind":"schedule","title":"..."}` (no
  components, no crop or scale; paper inches, caption NTS). Grouped headers go in `header` rows with `colspan`/`rowspan`;
  plain headers in `columns` (`{"header":"EMBED.","unit":"IN.","width":0.75}`); cells may be numbers; `"\n"` breaks a line.
  Every row must cover every column (counting rowspans from rows above), else E_PARAM names the row. General notes of the
  schedule go in the view as notes (a list above the tables; `notes_title` renames DETAIL NOTES) or in the table's `notes`.
  A table can also sit in a section view: it goes under the drawing, or at `at` (top-left corner; W_TABLE_OVERLAP if it
  lands on anything). Cell text is linted like notes and counts for coverage. In a keyed view a general note with no
  `target` or `at` is listed with no tag.
- Subtracted solids (SPEC 26.4): earth and gravel are plain rectangles around footings, stems and pours (a fill draws
  under every solid the view cuts; `except` keeps one from clipping it). A pour around a column is ONE polygon with
  `"voids": ["col", "base_plate", "grout"]`: the steel shows with its own fill and the hole follows a resize. Block-outs:
  `"voids": [{"rect": [8, 4], "at": "stem@top_center", "anchor": "top_center"}]`, `{"circle": 4, "at": ..., "z": [-6, 6]}`.
  Pipe through a wall or footing: `{"type": "sleeve", "pipe": 4, "fill": "joint_filler", "through": "stem", "at": {"to":
  "stem@center"}}` (run x/y + `length` to cut it along its axis). `W_VOID_MISS` prints the op that drops a void that misses.
- Plans and elevations (SPEC 26.3): model once, in the real orientation (Y up, Z toward the section viewer: a wall
  running across the section is a profile extruded along z, a wall running along x is a long profile with its thickness
  in `z`), then add views. `"kind": "plan"` cuts at `cut_y` (default 1" below the highest top): crossed members are
  hatched, studs and posts get the end-grain X, members below are outlines, vertical bars are dots. Plan points are
  [x, -z]; a plan `crop` also takes `{"x": [x0, x1], "z": [z0, z1]}`; Refs resolve by themselves (box anchors on the
  plan footprint). `"kind": "elevation"` cuts nothing: nearer members hide farther ones, the crop break-lines every
  member it cuts, and a strap on a face shows without touching `cut_z`. `"hidden_lines": true` dashes what is hidden.
  Plan titles print as `PLAN - <title>` unless the title says PLAN (style `text.plan_title`): do not type "(PLAN)".
- Corner and intersection bars: rebar `"plane": "xz"`, `"y": <height or Ref>`, `"points": [[x, z], ...]` (bends are
  filleted). The plan draws the bent bar; a section shows a dot where it crosses `cut_z` and a line where it runs behind.
- `shown`: `"hidden"` = dashed, no suffix (a base plate under a pour, a stiffener behind a web); `"where_occurs"` =
  dashed and notes end in (WHERE OCCURS). The old `"dashed"` warns `W_SHOWN_DASHED`: pick one of the two.
- Floor and roof framing (SPEC 27.1): an I-joist is ONE `ijoist` (`"size": "11-7/8 x 2-5/16"`, run x side view or
  run z end-on, `stiffeners`/`squash_blocks` `left|right|both`, anchors `top_flange_end`, `bottom_flange_start`,
  `bearing_left` ... for hangers and notes); never three lumber pieces. A floor truss is ONE `truss` with
  `"profile": "parallel_chord", "depth": 18` (4x2 chords and open webs; `"bearing": "top_chord"` on a ledger or
  plate, `"face"` in a hanger). Trusses parallel to the section: `"orient": "end_on", "depth": <height at the cut>`
  (`plies` for girders). Roof: `"profile": "scissor"` (`bottom_pitch`), `"mono"` (`heel_height` = the high end;
  sheathing `slope: "@truss"` falls with it). Full-height eave blocking: `"until": "truss@heel_block_top"`.
  `W_BEARING` warns when an I-joist bears under 1 3/4" at an end (3 1/2" inside) and prints the length to add.
- Nailing (SPEC 27.4): draw fasteners with a `nailing` component, never notes alone. It is placed from `members` (the
  member the heads are on first), so it moves with them: plate splice `{"type":"nailing","members":["upper_plate",
  "lower_plate"],"size":"16d","count":8,"rows":2,"through":true}` (staggered rows over the joint; `range` limits them),
  toenails `{"pattern":"toenail","members":["blocking","plate"],"size":"8d","count":2}`, sheathing
  `{"pattern":"edge","members":["shtg","sill","stud_end","top_plate"],"size":"8d","spacing":6}` (kept where framing lies
  behind; a bare edge is W_NAIL_EDGE) and `{"pattern":"field","members":["shtg","stud"],"size":"8d","spacing":12}`. Nails in the
  drawing plane draw as a shank with a head (toenails angled), end-on as dots; iso shows dots on the face. A note
  targeting it may omit `text` (it prints `(8) 16d NAILS STAGGERED`, `8d @ 6" O.C. EDGE NAILING`, or the `text` param),
  and coverage reads that text. In a section only nails at or behind the cut and outside cut solids show: an elevation
  (or `cut_z` in front) shows splice nails, W_NAIL_HIDDEN says when a view shows none. W_NAIL_MEMBERS (no touching
  joint), W_NAIL_LENGTH (points out of the far face: `through` or a shorter size), W_NAIL_SPACING print their fixes.
- Full spec: https://github.com/hotschmoe/kerf/blob/master/spec/SPEC.md.
  The reference details in `spec/details/` are complete documents to copy from.
