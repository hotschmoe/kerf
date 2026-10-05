# Kerf CLI: guide for agents (Claude Code, Grok, ...)

You are building construction details with the `kerf` command-line engine. Each detail is a
JSON file (`<id>.kerf.json`) in the current folder. Folders are the designer's library. The designer
reviews the details in the Kerf web UI (`kerf serve`, or the hosted app -> OPEN), edits notes,
verifies citations, and exports DXF/PDF.

## Workflow
```sh
kerf new truss-cmu.kerf.json --title "PREFAB TRUSS BEARING AT CMU WALL"   # empty document
kerf new x.kerf.json --template section              # a complete minimal example (2 members, view, 2 notes, dim, label) to edit
kerf schema view                                     # field reference: doc view note dim label cite ops <component type> (also below)
kerf apply truss-cmu.kerf.json ops.json -w --why "Build initial truss bearing detail"   # apply, write back, log
kerf apply truss-cmu.kerf.json -w --ops '[{"op":"update","path":"components/sill_plate","value":{"size":"2x6"}}]'
kerf apply truss-cmu.kerf.json ops.json --dry-run    # validate and print the summary, write nothing
echo '[...ops...]' | kerf apply truss-cmu.kerf.json - -w  # ops from stdin
kerf check truss-cmu.kerf.json                       # summary + diagnostics (exit 1 on errors)
kerf export truss-cmu.kerf.json --view A --format png -o truss-cmu-A.png   # LOOK at this image
kerf export truss-cmu.kerf.json --view A --format pdf --sheet -o truss-cmu-A.pdf
kerf export truss-cmu.kerf.json --view A --format dxf -o truss-cmu-A.dxf
kerf catalog --markdown                              # component reference (also included below)
```
- **Always pass `--why "..."`** with `-w`: one line for the designer saying what and why
  (e.g. `--why "Add HETA20 anchors at 16in o.c."`). It is recorded in `<file>.log.jsonl`, and the designer
  sees it live in the web UI as a LOCAL AGENT card.
- `apply` is atomic. If any op fails, nothing is written and the exit code is 1; every error prints on stderr as
  `ERROR <code> <path>: <message>  Fix: <fix>`, followed by `nothing written`.
  On success it prints the summary: every component with resolved x/y extents in feet-inches, plus
  diagnostics. Read it every time. Fix every error, and fix or justify every warning.
- **Unknown keys warn** (`W_UNKNOWN_KEY` names the path and suggests the right key: `citations` -> `cite`,
  `side` -> the view's `notes_side`, `point` -> `at`, `kind` -> `type`, `pos` -> `place`). They are kept in the file but do nothing.
  Check `kerf schema <topic>` instead of guessing field names. `kerf call help` lists the raw API functions with their input shapes.
- A warning you have judged acceptable can be acknowledged on its component, with a reason that is logged:
  `"acknowledge": [{"code": "W_UNTREATED_CONTACT", "reason": "..."}]` (`kerf schema acknowledge`). For wood on concrete or
  masonry prefer `"barrier": "sill_seal"` on the lumber (draws a 1/8 inch sealer strip under it).
- The first build of a detail is usually one `{"op":"set","path":"doc","value":{...whole document...}}`.
  After that, use small `add` / `update` / `remove` ops so the designer's diffs stay clean.
- **Always render a PNG and look at it** before telling the designer you are done: proportions,
  overlaps, gaps, leaders pointing at the right thing, notes not colliding.
- **Windows PowerShell:** quoting JSON inline (`--ops '[...]'`) breaks. Write the ops to a file
  (`kerf apply x.kerf.json ops.json -w --why "..."`) or pipe a here-string:
  `@'` newline `[...ops...]` newline `'@ | kerf apply x.kerf.json - -w --why "..."`.
- **Dimension direction:** `dir` defaults to the dominant axis between `from` and `to` (|dx| >= |dy| gives `h`, else `v`);
  `W_DIM_ZERO` fires when a dimension measures under 1/16 inch (usually a wrong `dir` or two coincident points).
- **Dimension offset sign:** `dir:"h"` puts the dimension line at `max(y)+offset` when offset > 0 (above
  the higher point) and at `min(y)+offset` when offset < 0 (below the lower point). `dir:"v"` does the same with x:
  positive is right of the rightmost point, negative is left of the leftmost. Example: to dimension a footing
  width below the footing, use `{"from":"ftg@bottom_left","to":"ftg@bottom_right","dir":"h","offset":-6}`.
  To dimension its depth on the outside (left) face, use `{"from":"ftg@bottom_left","to":"ftg@top_left","dir":"v","offset":-6}`.
- Never edit the `.kerf.json` by hand. Go through `kerf apply` so the engine validates and
  canonicalizes it.
- Ops reference: `add` (path `components` | `views` | `views/<id>/annotations`, value),
  `update` (path `components/<id>` | `views/<id>` | `views/<id>/annotations/<id>` | `meta`,
  value = JSON merge patch, null deletes a key), `remove` (path to one item), `set` (path `doc`).
- Full spec: https://github.com/hotschmoe/kerf/blob/main/spec/SPEC.md.
  The three reference details in `spec/details/` are good examples of complete documents.

