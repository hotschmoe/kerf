# Kerf CLI: guide for agents (Claude Code, Grok, ...)

You build construction details with the `kerf` command-line engine. A detail is one JSON file
(`<id>.kerf.json`) in the current folder; the folder is the designer's library. The designer reviews it in
the Kerf web UI (`kerf serve`), edits notes, verifies citations and exports DXF/PDF. You describe construction
with typed components (lumber, cmu_wall, concrete, rebar, truss, ...); the engine places, draws, lays out notes and checks.

## Shell rules (the sandbox rejects compound commands)
- ONE kerf command per call. Do NOT chain with `&&`, `;` or `|`, do NOT use heredocs (`<<EOF`) or `echo '...' | kerf`.
- Create JSON files with your file-writing tool (Write), then point kerf at the file. Never hand-edit a `.kerf.json`.
- Works the same on Windows PowerShell, where inline JSON quoting breaks.

## Workflow
1. Ambiguity: ask ONE short question only if the answer changes the structural system or the load path (CMU vs
   stud wall, truss vs rafter, embedded anchor vs plate, what bears on what). Otherwise build with conventional
   choices and report your assumptions AND the main alternative reading. "Give me a footing detail" with no
   structure named is ambiguous: ask which footing (continuous wall, isolated pad, turned-down slab edge).
2. Write `ops.json` with the Write tool. First build = one op `{"op":"set","path":"doc","value":{...whole document...}}`
   (a single op object or `{"ops":[...],"why":"..."}` is accepted too). Put the designer's asks in `meta.requested`, one short
   phrase each, in their words: `"meta":{"requested":["cmu wall","bond beam","H2.5A ties","#5 rebar","vapor retarder"]}`.
3. Create and apply in one command (nothing is written if an op fails):
```sh
kerf new truss-cmu.kerf.json --ops ops.json --title "PREFAB TRUSS BEARING AT CMU WALL" --why "Build initial detail"
```
4. Read the summary (every component with resolved x/y extents), the `COVERAGE` block and the diagnostics. Fix every ERROR;
   fix or justify every WARN. Every `meta.requested` item must read `ok` (build the `MISSING` ones, or drop an item only if the
   designer dropped it): do not finish before full coverage, a clean check is not a finished detail. An item is covered when
   its words appear in one component's id/type/label/model or in one note, so name components and write notes in the designer's words. Then LOOK at it:
```sh
kerf export truss-cmu.kerf.json --view A --format png -o truss-cmu-A.png
```
5. Refine with small ops (a new ops file each time; keep ids stable so the designer's diffs stay clean):
```sh
kerf apply truss-cmu.kerf.json ops.json -w --why "Add HETA20 anchors at 16 in o.c."
kerf apply truss-cmu.kerf.json ops.json --dry-run
kerf check truss-cmu.kerf.json
kerf export truss-cmu.kerf.json --view A --format pdf --sheet -o truss-cmu-A.pdf
kerf export truss-cmu.kerf.json --view A --format dxf -o truss-cmu-A.dxf
kerf new x.kerf.json --template section
kerf schema note dim cite
kerf schema lumber
kerf guide --full
```
- Always pass `--why "..."` with `-w`/`new`: one line for the designer, logged to `<file>.log.jsonl` and shown live in the web UI.
- A failed apply prints `ERROR <code> <path>: <message>  Fix: <fix>` for every error, then `nothing written` (exit 1).
- Ops: `add` (path `components` | `views` | `views/<id>/annotations`), `update` (path `components/<id>` | `views/<id>` |
  `views/<id>/annotations/<id>` | `meta`; value = merge patch, null deletes a key), `remove` (path), `set` (path `doc`).
- A warning that is right for this detail is acknowledged on the component, with a logged reason:
  `"acknowledge":[{"code":"W_UNTREATED_CONTACT","reason":"..."}]` (I_* codes are accepted and ignored). Wood on concrete
  or CMU usually wants `"barrier":"sill_seal"` instead. Do not add fake geometry to silence a warning.
- Unknown keys warn (`W_UNKNOWN_KEY` suggests the right one: `citations` -> `cite`, `side` -> `notes_side`, `point` -> `at`,
  `kind` -> `type`, `pos` -> `place`). `kerf schema <topic>` before guessing a field name.

## Diagnostics you will meet (each prints its own fix)
W_NEAR_MISS (a member stops short of its neighbour: use `until`), W_FLOATING / W_OVERLAP (mis-placed), W_COVER (rebar cover),
W_UNTREATED_CONTACT (wood on masonry: `treated` or `barrier`), W_SHORT_SLOPE (see below), W_LEADER_HIT / W_VIEW_FIT (layout),
W_NOTE_STYLE (house style and commentary: no sentences, `?`, `NOTE:`, `W/` or `AND` at the end), W_UNKNOWN_KEY (misspelled field),
W_REQUESTED_MISSING (a `meta.requested` item has no component or note), W_CROP_STALE (an edit left a member outside an explicit
`crop`: remove `crop` to auto-fit), I_* (info only).

## Placement (relative, never computed coordinates)
- `"at": {"anchor": "bottom_left", "to": "bond_beam@top_left", "offset": [0, 0]}`: the member's own anchor lands on the Ref.
  Refs: `"comp@anchor"`, `"comp.part@anchor"`, `"comp#2@anchor"` (array instance), `"@origin"`, `{"ref":"x@center","offset":[dx,dy]}`, `[x,y]`.
- Every component has the 9 box anchors (top_left top_center top_right middle_left center middle_right bottom_left
  bottom_center bottom_right); builders add named ones (`kerf schema <type>`; `kerf schema refs`).
- Inches, X right, Y up, Z toward the viewer; lengths are numbers or strings like `"7-5/8"`, `"3'-4 1/2\""`. Use actual
  sizes through typed components (`"2x6"`, not 1.5x5.5). Rebar: `"place":{"in":"footing","face":"bottom","cover":3,"count":2}`.
- `"until": "<Ref>"` instead of `length` grows a member to another one's edge (no stale numbers).

## Roof pitch: change it in ONE place
Trusses own the pitch; sheathing and roofing follow it and run to the truss end. Never type a literal length or pitch on them.
```json
{"id":"truss","type":"truss","pitch":"6:12","exterior":"left","overhang":18,"span_shown":36,"at":{"anchor":"bearing_outer","to":"sill_plate@top_left"}}
{"id":"roof_sheathing","type":"panel","material":"osb","thickness":0.4375,"slope":"@truss","until":"truss@top_chord_end","at":{"anchor":"bottom_left","to":"truss@tail_top"}}
```
To change the pitch of an existing detail, one ops file: `update components/truss {"pitch":"6:12"}`, and on every sloped panel/membrane
`update components/roof_sheathing {"slope":"@truss","until":"truss@top_chord_end","length":null}`. `W_SHORT_SLOPE` warns when a sloped
panel/membrane stops short of the member it rests on and prints this fix.

## Notes: one engineer's voice
UPPERCASE, no trailing period, no sentences: `<SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>`, e.g.
`2X6 PT SILL PLATE W/ 5/8" DIA. ANCHOR BOLTS @ 48" O.C.`, `(2) #5 CONT. BOTT.`. Fractions `1 1/2"`, ` X ` between sizes,
abbreviations W/ O.C. EA. CONT. TYP. PT MIN. DIA. BOTT. CONC. GYP. BD. SHTG. REINF. MFR. U.N.O. (`W_NOTE_STYLE` lints this).
6-12 notes per detail: every structural element and connection, hardware model in the note, `INSTALL PER MFR.`.
Cite the 2021 IRC unless `meta.jurisdiction` says otherwise, only sections you are sure exist, always `"status":"suggested"`;
the designer verifies. Never present a design as engineered: capacities, spacing and nailing are the manufacturer's / EOR's.

## Views and annotations
`crop` and `scale` are optional on a section view (auto-fit: all non-fill components + 6 in; largest standard scale that fits);
set them only to frame deliberately (`W_VIEW_FIT` then checks). Notes split to both sides and the layout resolves leader
collisions itself; add `column` to a note only to force a side. Dim `dir` defaults to the dominant axis. Dim `offset` sign: h > 0
puts the line above the higher point, < 0 below the lower; v > 0 right of the rightmost, < 0 left of the leftmost.
Beam in a wall (flush beam, header): draw the elevation along the wall unless asked for the end-on section; say which you drew.
Keep going until the PNG reads as the detail an engineer expects: proportions, gaps, leaders on the right element, no overlaps.

<!-- FULL -->
## Long-form notes (kerf guide --full)
- `apply` on stdin (`kerf apply x.kerf.json - -w`) and inline (`--ops '[...]'`) exist for interactive shells; in an agent
  sandbox prefer an ops file. `kerf call help` lists the raw API functions with their input shapes.
- `kerf fmt <doc> -w` canonicalizes a document; `kerf drawing` / `kerf mesh` print the drawing IR / mesh JSON.
- `acknowledge` prints an `I_ACK` line per suppressed warning, logged by `kerf apply -w`.
- Dimension offset sign, example: below a footing `{"from":"ftg@bottom_left","to":"ftg@bottom_right","dir":"h","offset":-6}`;
  its depth on the outside (left) face `{"from":"ftg@bottom_left","to":"ftg@top_left","dir":"v","offset":-6}`.
- Full spec: https://github.com/hotschmoe/kerf/blob/main/spec/SPEC.md.
  The three reference details in `spec/details/` are complete documents to copy from.
