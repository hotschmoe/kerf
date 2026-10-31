# KERF — Engine & Document Specification v0.1

> Kerf turns a conversation into a professional construction detail. An LLM, supervised by a
> designer, edits a **Kerf Document** (semantic, diffable source of truth). A deterministic
> **engine** compiles it with a **Kerf Style** (office drafting standard) into views, a 2D
> **Drawing** IR, a 3D mesh, and exports (SVG, DXF, PDF).
>
> The LLM decides *what* (components, relations, note content). The engine and style decide
> *how it looks*. Same document + same style ⇒ byte-identical output. That is the product.

This spec is the contract shared by every stack. Where it says MUST, conformance tests check it.
Where it says SHOULD, deviate only with a note in your stack's `NOTES.md`.

Companion files:
- `spec/DESIGN.md` — UI design language (1970s IBM engineering) shared by all four apps.
- `spec/llm/` — the agent guide (`cli-guide.md`) and system prompt (`system.md`) embedded by `kerf guide`.
- `spec/styles/kerf-standard.kerfstyle.json` — the default style.
- `spec/details/*.kerf.json` — reference documents (golden inputs).
- `spec/details/REFERENCE-CONTENT.md` — domain research for the reference details.
- `spec/fonts/kerf-simplex.json` — the stroke font (Hershey Simplex derivative).

---

## 0. Repository layout

```
spec/                 this contract
engine/               the Kerf engine (Rust): library `kerf-core`, CLI `kerf` (with `kerf serve`), wasm (raw ABI)
apps/web/             TypeScript + DOM + three.js UI; runs engine/dist/kerf.wasm
tools/                shared tooling (dxf_check, pdf_check, build_wasm, shot, serve, size_report)
install.sh / .ps1     one-line installers for the `kerf` CLI (GitHub releases)
```

Stack: a Rust engine (zero crates) with a TypeScript UI. It was chosen in the 2026-10-05 bake-off as Zig and replaced
by its Rust port on 2026-10-31; the other stacks (egui app, teak app) are preserved at tag
`archive/bakeoff-2026-10-05`. See `docs/STACKS.md`.

---

## 1. Coordinates, units, numbers

- Internal unit: **inches**, `f64`. US customary only for MVP.
- Axes: **X** = horizontal in the primary drawing, **Y** = up, **Z** = depth (toward the viewer
  is +Z). The primary view of every detail is looking along **−Z** (from +Z toward −Z) at the XY plane.
- **Kerf is 2.5D:** every solid is a 2D profile in the XY plane extruded along Z over `[z0, z1]`.
  This covers framing, masonry, concrete, rebar, sheathing, straps. It is a deliberate limit; do
  not add general 3D.
- Length inputs accept a number (inches) or a string. The engine MUST parse:
  `12`, `"12"`, `"7 5/8"`, `"7-5/8"`, `"7-5/8\""`, `"3'"`, `"3'-0\""`, `"3'-4 1/2\""`, `"3' 4.5\""`,
  `"-1'-2\""`, `"0.4375"`, `"15/32"`. (A hyphen between whole inches and a fraction is a joiner,
  a hyphen after `'` is a separator, a leading hyphen is a minus sign.)
- Canonical form (what `fmt`/`apply` writes back): numbers in inches, rounded to 1e-4, shortest
  decimal representation (`7.625`, not `7.6250`; `0.4375`; integers without `.0`).
- Human formatting (summaries, dimension text) — **architectural feet-inches**, rounded to the
  nearest 1/16": `0"`, `7 5/8"`, `1'-0"`, `4'-1 1/2"`, `-1'-2"`. Under 12" never shows feet.
  Fractions reduced (`1/2`, never `8/16`).
- Slopes: `"4:12"` = rise:run (atan(4/12) = 18.4349°). Angles otherwise in degrees, CCW positive.

## 2. Determinism (MUST)

1. No wall clock, randomness, hash-map iteration order, locale, or float formatting variance
   reaches any output. Use ordered maps or document order everywhere.
2. Output ordering: components in document order; parts in the order the builder defines;
   annotations in view order.
3. Float output: fixed rule — round to 1e-4 then print shortest round-trip decimal; `-0` prints `0`.
4. DXF handles are sequential from a fixed seed; DXF/PDF carry no timestamps (PDF has no
   `/CreationDate`; DXF `$TDCREATE`/`$TDUPDATE` omitted or fixed to 2451545.0).
5. Same doc + style ⇒ byte-identical SVG, DXF, PDF, Drawing JSON across runs and platforms of
   the same engine. Cross-engine: Drawing JSON equal within 1e-3 in. (tested by
   `tools/conformance.py`).

## 3. The Kerf Document (`*.kerf.json`)

Top level (canonical key order is the order listed):

```jsonc
{
  "kerf": "0.1",                         // schema version
  "id": "truss-bearing-cmu",             // slug, unique in a library
  "title": "PREFAB TRUSS BEARING AT CMU WALL",
  "meta": {                              // free-form but these keys are known:
    "author": "…",
    "discipline": "structural",
    "classification": { "uniformat": "B1020", "masterformat": ["04 22 00", "06 17 53"] },
    "jurisdiction": { "code": "IRC", "edition": 2021 },   // default citation basis
    "references": [                      // reference images the detail is redrawn from (§23)
      { "path": "refs/0123_truss_bearing.png", "caption": "TRUSS BEARING, SET 0123 S-501" }
    ],
    "tags": ["cmu", "truss", "uplift"],
    "forked_from": null                  // id@version when forked (future library)
  },
  "run": [-24, 24],                      // default z-extent for components (inches)
  "components": [ /* §4 */ ],
  "views": [ /* §6 */ ]
}
```

Every component and annotation has an `id`: `[a-z][a-z0-9_]*`, unique within its namespace
(components share one namespace; annotations are unique within a view). IDs are how the LLM, the
designer, diffs, and annotations refer to things — choose meaningful ones (`bond_beam`, `sill_plate`).

### 3.1 References and anchors

A **Ref** is a string naming a point:

```
"<component>@<anchor>"                e.g. "sill_plate@top_left"
"<component>.<part>@<anchor>"         e.g. "truss.top_chord@top_right"
"@origin"                             the point (0,0)
```

Every component and every part has the **9 box anchors** of its *local* profile box (before
rotation; the anchors rotate with the member):

```
top_left      top_center      top_right
middle_left   center          middle_right
bottom_left   bottom_center   bottom_right
```

Builders add **named anchors** (catalog §5). Anchors are 2D (XY). A Ref may carry an offset
anywhere a Ref is accepted: `{ "ref": "footing@bottom_left", "offset": [3, 3] }`.

### 3.2 Placement

```jsonc
"at": { "anchor": "bottom_left", "to": "bond_beam@top_left", "offset": [0, 0] }
```

Translates the component so its own `anchor` lands on the `to` point plus `offset`.
- `anchor` defaults to `"bottom_left"`; `offset` defaults to `[0, 0]`.
- `to` may be a Ref, a Ref-with-offset object, or a literal point `[x, y]`.
- Omitting `at` places `bottom_left` at the origin.
- `rotate` (deg, CCW) or `slope` ("4:12", sloping up toward +X) rotates the member about its
  placement anchor *after* placement. `mirror: true` mirrors the profile about its local
  vertical centerline before placement (use builder `side` params instead where offered).
- Placement references form a DAG. The engine resolves in topological order (ties broken by
  document order). Cycles ⇒ error `E_CYCLE` listing the cycle.

Z placement: `"z": [z0, z1]` (absolute), or `"z": 8` meaning *centered at 8 with the member's
natural z-thickness*. Default: members that run along Z (lumber run z, panels, cmu, concrete,
fills, membranes, along_z rebar, solids) span the document `run`; members with a natural
z-thickness (lumber run x/y, truss members, connectors, path rebar, anchor bolts) are centered on
the middle of `run` with that thickness.

`array`: `{ "axis": "z" | "x" | "y", "count": n, "spacing": s }` repeats the component
(instance k translated by k·spacing). Instances are `id#0..id#n-1` in outputs; refs to `id`
mean instance 0.

### 3.3 Points lists

Builders that take polylines (`solid` polygon, `fill`, `membrane`, `rebar.path`, `connector.path`)
accept entries that are either `[x, y]` (relative to the placement point when `at` is given,
otherwise absolute) or a Ref / Ref-with-offset (always absolute). Optional third element on a
literal point is a DXF-style **bulge** for the segment to the next point (arc; bulge = tan(θ/4)).

## 4. Components — common fields

```jsonc
{
  "id": "sill_plate",
  "type": "lumber",            // catalog §5
  "label": "2x8 PT SILL",      // optional short human label (used by notes and summaries)
  "material": "wood",          // usually implied by type; override allowed (see §7 materials)
  "at": { … }, "rotate": 0, "slope": null, "mirror": false,
  "z": null, "array": null,
  "embedded": false,           // drawn over cut solids (rebar, anchor bolts, embedded straps)
  "visible": true,
  "views": null,               // ["A","C"]: drawn and checked only in these views (default: every view; §25.4)
  // …type-specific params
}
```

## 5. Component catalog (MVP)

Each entry: params (defaults), parts, named anchors, how it draws. "Actual" sizes are what
the engine draws. The engine MUST also emit this catalog as JSON and markdown (`kerf catalog`)
— that generated text is what the LLM system prompt embeds, so keep descriptions tight.

### 5.1 `lumber` — sawn or engineered wood member
| param | default | notes |
|---|---|---|
| `size` | — | sawn nominal `"TxD"`: T 1, 2, 3, 4 with D 3…14 (`"2x10"`, `"3x8"`), T 6, 8 with D 6…12; or actual `"1.75x11.875"` when `product`≠`sawn` |
| `product` | `"sawn"` | `sawn` `lvl` `psl` `lsl` `glulam` |
| `run` | `"z"` | axis the member's length runs along: `z` (seen in cross-section), `x`, `y` |
| `orient` | `"upright"` | for `run:"z"`: `upright` (depth vertical) or `flat` (depth horizontal) |
| `face` | `"wide"` | for `run:"x"/"y"`: which face the viewer (looking −Z) sees: `wide` (depth in-plane) or `narrow` (thickness in-plane) |
| `length` | — | required for `run:"x"/"y"`; optional for `run:"z"`: the member is that long along z, centered on a numeric `z`, else on the first section view's `cut_z` (default: spans the doc `run`) (§25.5) |
| `plies` | 1 | built-up members; plies stack along X for run z, along Z otherwise; draws ply lines |
| `treated` | false | preservative-treated (affects label, validation `W_UNTREATED_CONTACT`) |
| `blocking` | false | section mark becomes single diagonal instead of X (discontinuous member) |
| `grade` | null | free text e.g. `"#2 DF-L"` for notes |

Material: `wood`; `treated:true` ⇒ `wood_treated`; `product`≠`sawn` ⇒ `wood_engineered`.

Sawn actual sizes: 1x → 0.75 thick; 2x → 1.5; 3x → 2.5; 4x → 3.5; 6x → 5.5; 8x → 7.5. Depths: x3 2.5, x4 3.5, x6 5.5,
x8 7.25, x10 9.25, x12 11.25, x14 13.25 (6x and 8x: x6 5.5, x8 7.5, x10 9.5, x12 11.5). For `orient: upright`,
profile = (thickness × depth); `flat` = (depth × thickness).
Draws: cut ⇒ outline + "wood X" mark (diagonals corner-to-corner per ply) per style; beyond ⇒ outline.

### 5.2 `panel` — sheathing, boards, gypsum, fascia/trim boards
| param | default | notes |
|---|---|---|
| `material` | `"osb"` | `osb` `plywood` `gypsum` `fiber_cement` `wood_board` |
| `thickness` | — | `0.4375` (7/16"), `0.46875` (15/32), `0.5`, `0.625`… |
| `length` | — | in-plane extent |
| `run` | `"x"` | in-plane direction of `length` before rotation: `x` or `y` |
Profile: run x ⇒ (length × thickness); run y ⇒ (thickness × length). Use `slope` for roof sheathing.

### 5.3 `cmu_wall` — concrete masonry wall in section
| param | default | notes |
|---|---|---|
| `width` | 8 | nominal 6, 8, 10, 12 ⇒ actual 5.625, 7.625, 9.625, 11.625 |
| `courses` | — | number of 8" courses (7.625 unit + 0.375 mortar joint) |
| `bond_beam_courses` | 0 | top N courses are bond-beam units (grouted, horizontal bars) |
| `grout` | `"reinforced"` | `solid` (all cells), `reinforced` (bond beams + cut cell), `none` |
| `face_shell` | 1.25 | face shell thickness, drawn in section |
| `top_joint` | false | include a mortar joint above the top course |
Profile box: width_actual × (courses·8 − 0.375 [+0.375 if top_joint]). Bottom of the lowest
course sits at the box bottom (a bed joint is assumed below, not drawn).
Parts: `course_1`…`course_n` (1 = bottom), `bond_beam` (union of bond-beam courses), `grout`, `cell` (between the face
shells, full height; §25.1).
Named anchors: `bond_beam_center` (center of bond-beam zone), `top_center`, `cell_center_top`, `cell_left`, `cell_center`,
`cell_right` (inside faces of the face shells and the cell middle, at the wall bottom), `cell_left_top`, `cell_right_top`.
Section draws per course: two face shells (cut, `cmu` hatch), the cell (grouted ⇒ `grout` hatch;
ungrouted ⇒ empty with the cross web shown as a beyond line), mortar joints as cut lines.
3D/iso: units 15.625 long with 0.375 head joints along Z, running bond (alternate courses offset 8").

### 5.4 `concrete` — cast-in-place concrete with shape builders
| param | default | notes |
|---|---|---|
| `shape` | — | `rect`, `polygon`, `slab_edge`, `footing` |
| `material` | `"concrete"` | |
| `cover` | `{ "bottom": 3, "sides": 3, "top": 1.5 }` | REQUIRED clear cover for bars in this host, used by `W_COVER` |
`rect`: `width`, `height`. `footing`: same as rect (semantic name; parts `footing`).
`polygon`: `points` (§3.3).
`slab_edge` — monolithic slab with turned-down (thickened) edge footing:
| param | default | notes |
|---|---|---|
| `exterior` | `"left"` | which side is the exterior edge |
| `slab_thickness` | 4 | |
| `slab_length` | 48 | slab drawn from exterior face inward |
| `footing_width` | 12 | bottom width of turndown |
| `footing_depth` | 18 | top of slab to bottom of footing |
| `haunch` | 45 | inner face slope from horizontal (deg); 90 = vertical inner face |
| `recess` | null | `{ "width": w, "depth": d, "from_edge": e }` depression at top exterior edge (door track/sill). `from_edge` = distance from exterior face to start of recess (0 = recess at edge) |
| `recess_slope` | 0 | fall of recess floor toward exterior, in inches over its width |
Local origin: exterior face at x=0 (for `exterior:"left"` the building is +X; `"right"` mirrors), top of slab at y=0,
footing bottom at y=−footing_depth. Parts: `footing` (rect zone x∈[0,footing_width],
y∈[−footing_depth, −slab_thickness]… i.e. the turndown below the slab), `slab` (the slab zone).
Named anchors: `top_exterior` (the datum point (0,0): top-of-slab plane at the exterior face line, whether or not a recess is there), `slab_top`
(alias of top at inner end), `footing_bottom_exterior`, `footing_bottom_interior`,
`slab_bottom_interior`, `haunch_top` (where haunch meets slab underside), `recess_bottom_exterior`,
`recess_bottom_interior`, `recess_top_interior`.

### 5.5 `rebar`
| param | default | notes |
|---|---|---|
| `size` | `"#4"` | `#3` .375, `#4` .5, `#5` .625, `#6` .75, `#7` .875, `#8` 1.0 |
| `mode` | `"along_z"` | `along_z` (continuous bar seen as a dot in section) or `path` (bar in XY plane) |
| `place` | null | cover-based placement (preferred): `{ "in": "<comp>[.<part>]", "face": "bottom"|"top"|"left"|"right", "cover": 3, "count": 2, "side_cover": 3 }` — bars at clear `cover` from the zone's `face` (the host outline, cut to the part's box; in a polygon the outermost edge run facing that way, §25.1), spread evenly along that run at `side_cover` from its ends (count 1 ⇒ centered). A centre outside the zone is `E_PLACE_OUTSIDE` |
| `points` | — | for `mode:"path"`: polyline (§3.3); bends get radius `bend_radius` (default 3·d_b inside radius for #3–#8, drawn as fillets) |
| `spacing_note` | null | e.g. `"#4 @ 16\" O.C."` used in summaries/notes |
Default `embedded: true`. Cut dots draw solid-filled. Path bars draw as two parallel lines (bar
outline) with pen `rebar`. 3D: swept circle (12 segments).
Path bars add the anchors `start`, `end`, `bend_k` and `seg_k_mid`, all on the bar centreline (§25.1).

### 5.6 `anchor_bolt`
`diameter` 0.5 | 0.625, `embed` (below placement point), `projection` (above), `hook`
`"J"`|`"L"`|`"headed"`|`"none"` (hook length 3 in default), `nut_washer` true.
Placement anchor named `top_of_concrete` (the point where bolt meets the host top surface).
In-plane member at a given `z`. `embedded: true`.

### 5.7 `connector` — schematic steel hardware (straps, ties, embedded anchors)
| param | default | notes |
|---|---|---|
| `model` | null | e.g. `"MSTA36"`, `"H2.5A"`, `"HETA20"`, `"CS16"` — catalog fills `width`/`gauge`/`length` and label text |
| `points` | — | polyline (§3.3) of the strap's bearing face (`lay:"edge"`) or centerline (`lay:"face"`) |
| `lay` | `"edge"` | `edge`: strap seen edge-on — thickness (gauge) in-plane, grows to `side` of the polyline, `width` along Z. `face`: strap seen face-on — `width` in-plane centered on the polyline, gauge along Z |
| `side` | `"left"` | for `lay:"edge"`: which side of the polyline direction the thickness grows (left of a left→right line = up) |
| `gauge` | 18 | 12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359 |
| `width` | 1.25 | extent along Z |
| `fasteners` | null | text, e.g. `"(10) 10d EA. END"` used in notes |
Drawn as a thickened polyline (pen `steel`, filled solid when cut). Schematic only — the note
carries the model; geometry shows location and path. If `model` is in the hardware table
(engine embeds a small table: model → width, gauge, length, kind) unspecified params fill in.

### 5.8 `truss` — prefab wood truss heel (side view)
| param | default | notes |
|---|---|---|
| `exterior` | `"left"` | side of the heel/overhang |
| `pitch` | `"4:12"` | |
| `top_chord` | `"2x4"` | |
| `bottom_chord` | `"2x4"` | |
| `heel` | `"standard"` | `standard` or `raised` |
| `heel_height` | null | raised heel: vertical height of the heel at the bearing outer edge (top of bottom chord to top of top chord) |
| `bearing_width` | 3.5 | width of support under the heel |
| `overhang` | 12 | horizontal distance from outer face of bearing to tail end |
| `tail` | `"plumb"` | `plumb` or `square` cut |
| `span_shown` | 48 | how far into the building to draw (break line at end) |
| `plate` | true | draw truss plate outline at the heel (dashed, pen `hidden`) |
Local origin: outer edge of bearing at bottom of bottom chord. Members are in-plane (z thickness
1.5, use `z` and `array` for spacing). Parts: `top_chord`, `bottom_chord`, `heel_web` (raised
only), `plate`, `tail`. Named anchors: `bearing_outer`, `bearing_inner`, `tail_bottom`,
`tail_top`, `top_chord_at_bearing` (top of top chord directly above `bearing_outer`),
`top_chord_end` (top of top chord at `span_shown`), `bottom_chord_top_inner`.
Geometry: standard heel — top chord lower edge passes through the point (bearing_outer.x,
bottom_chord top) and extends to the tail; bottom chord ends at the bearing outer edge (or at the
tail if `overhang` uses a cantilevered bottom chord — not MVP). Tail: top chord extends to
x = −overhang; plumb cut is vertical.

### 5.9 `membrane` — thin layers (underlayment, vapor retarder, WRB, roofing)
`material`: `underlayment` `vapor_retarder` `wrb` `shingles` `flashing_membrane`;
`points` (§3.3) polyline; `thickness` (draw thickness; default per material from style, e.g.
vapor retarder 0.04); `side`: `"left"|"right"` which side of the polyline the thickness grows.
Drawn per style pen (vapor retarder: dashed heavy line; shingles: thick line with tick marks).

### 5.10 `fill` — earth, gravel, sand, compacted fill
`material`: `earth` `gravel` `sand` `compacted_fill`; `points` polygon (§3.3); `outline`:
`"top"` (default — only the uppermost edge chain is stroked, as a grade line), `"full"`,
`"none"`. Hatched per material. `grade_label` optional text placed by the annotation system.

### 5.11 `insulation`
`form`: `rigid` | `batt`; `width`, `height` (rect) or `points`. Rigid hatched per style; batt
drawn with the batt symbol (sinusoidal loop line fitted to the rect).

### 5.12 `solid` — escape hatch
`profile`: `{ "rect": [w, h] }` | `{ "circle": d }` | `{ "points": [...] }`; `material` required.
Use only when no typed component fits; the summary flags it so a reviewer can see it.

### 5.13 `steel` — structural shapes (AISC table)
| param | default | notes |
|---|---|---|
| `shape` | — | `W8x18`, `HSS4x4x1/4` (rect: height x width x nominal wall), `HSS4.000x0.250` (round), `PIPE3STD` / `PIPE3XS`, `L4x3x3/8`, `C8x11.5`, `MC8x18.7`, `PL1/2x8` (plate thickness x width, any size) |
| `orient` | `"section"` | `section` (cut end-on, runs along z), `elevation` (side view along its length), `plan` (top view along its length) |
| `run` | `"x"` | elevation/plan: in-plane direction of the length, `x` or `y` |
| `length` / `until` | — | elevation/plan: required (as lumber, §17). Section: optional `length` along z, centered on the first section view's `cut_z` (default: spans the doc `run`) |
| `long_leg` | `"vertical"` | angles: LLV or LLH |
| `outstand` | `"back"` | elevation of angles and channels: horizontal leg / flanges point away from the viewer (−z) or toward it |
| `fillets` | true | W k-region and angle/channel root fillets, HSS corner radii (outside 2t, inside t) |
| `hidden_lines` | false | elevation/plan: draw hidden faces dashed |
| `grade` | null | free text (`A992`, `A500 GR. C`, `A36`) |
Dimensions come from an embedded subset of the AISC Shapes Database v16 (W4–W24, square/rect/round HSS, pipe STD/XS,
equal/unequal angles, C/MC); `kerf schema steel` lists every size, an unknown size is `E_PARAM` naming the sizes of that
depth. HSS and pipe walls draw at the design thickness 0.93 × nominal; angle and channel root radii are approximations by
size; channel flanges draw parallel at the tabulated average thickness. Cross-section frame: u across, v up, box (0,0)–(width, depth) with the
bottom-left at the flange tip / heel / outside corner. Section: XY = (u, v) extruded along z; HSS and pipe are two `wall`
halves (simple loops for the mesh). Elevation: XY = (length, v) for run x or (v, length) for run y, u along z (centered on
the component's z; `outstand` picks the sign). Plan: XY = (length, u) / (u, length), v along z (top toward the viewer).
Members seen lengthwise are stacks of rectangular slabs (flanges, web, walls, legs) at their own z (`Prism.zoff`), so iso,
mesh and occlusion are true; pipe and round HSS seen lengthwise are boxes (2.5D limit).
Draws: section cut ⇒ solid steel fill + steel pen (the material's `fill`), beyond ⇒ outline. Elevation and plan never
fill: a prism flagged `lengthwise` whose member reaches behind the cut plane is classified **beyond** even where the plane
passes through it, so the member draws as an elevation: its silhouette in the visible steel pen (a W also shows its
flanges as lines). Hidden faces (HSS/pipe inside walls, back flanges, the web in plan) are omitted unless
`hidden_lines: true`, which draws them dashed in the `hidden` pen. A slab edge lying on the outline of a nearer slab of
the same member draws once in the member's pen (not as a same-component shared edge). Where the crop cuts a lengthwise
member off, it gets a break line (§16 break symbol), merged with the break lines of adjacent cut members.
Named anchors: section — W `top_flange`, `bottom_flange`, `web_center`, `web_left`, `web_right`,
`top_flange_tip_left/right` (underside), `bottom_flange_tip_left/right` (top face); HSS/pipe `inner_top`, `inner_bottom`,
`inner_left`, `inner_right`; angle `heel`, `heel_inside`, `toe_vertical`, `toe_horizontal`, `vertical_leg_face`,
`horizontal_leg_top`; channel `web_back`, `web_face`, `top_flange`, `bottom_flange`, `top_flange_tip`, `bottom_flange_tip`;
plate `edge_left`, `edge_right`, `face_top`, `face_bottom`. Elevation/plan — the levels across the member at mid-length,
each also as `<name>_start` / `<name>_end` at the member ends.

### 5.14 `bolt` — structural bolt group
`diameter` 0.75, `grade` `"A325"` (text), `grip` 1 (ply stack between head and nut), `orient` `side` (axis in-plane, head
up at the placement point, grip downward) | `end` (axis along z, head toward the viewer), `count` 1, `spacing` 3,
`line` `x` | `z` (side) or `x` | `y` (end): the row direction from bolt 1, `washer` true. Heavy hex head and nut 1.5d + 1/8
across flats, head 0.625d, nut d, F436 washer 2d × 5/32 under the nut, stick-out 0.5d. Embedded steel. Placement anchor
defaults to `head_bearing` (local origin, bolt 1's axis at the underside of the head); also `nut_bearing`, `bolt_1`..`bolt_n`.
Natural z thickness: d (side), grip (end). Any `rotate` (180 draws the head below a flange, the grip up) keeps the cut fill of
the plies the bolt passes through (§25.1).

## 6. Views and annotations

```jsonc
{
  "id": "A",
  "kind": "section",                        // section | iso
  "number": "1",                            // detail number in the bubble
  "title": "TRUSS BEARING AT CMU WALL",
  "scale": "1-1/2\"=1'-0\"",                // see §6.1
  "cut_z": 6,                               // section: cut plane z; look toward −Z
  "crop": { "x": [-30, 40], "y": [-12, 48] },  // model-space window; break lines where cut solids are clipped
  "from": "front_right",                    // iso only: front_right | front_left | back_right | back_left
  "cutaway": true,                          // iso only: clip solids to z ≤ cut_z, hatch the cut face
  "notes_side": "right",                    // right | left | both | above (keyed: where the list goes, §24)
  "notes_style": "leader",                  // leader | keyed | list (§24); default: style notes.mode
  "omit": ["roofing", "b_*"],               // components not drawn in this view: ids or `*` globs (§25.4)
  "only": ["shared_*", "a_*"],              // optional: the view draws only matching components (§25.4)
  "annotations": [ … ]
}
```
A view draws a component when the component's `views` (if set) lists the view, the view's `only` (if set) matches its
id, and no `omit` entry matches it (§25.4). Everything a view does (section and iso drawing, beyond outlines, auto crop,
note landings, `inspect at`, the geometry checks of §9, `W_CROP_STALE`) uses that set.

### 6.1 Scales
Accepted: `3"=1'-0"` (factor 4), `1-1/2"=1'-0"` (8), `1"=1'-0"` (12), `3/4"=1'-0"` (16),
`1/2"=1'-0"` (24), `3/8"=1'-0"` (32), `1/4"=1'-0"` (48), and `"1:N"`. Also `"NTS"`
(iso views; engine fits to frame). Paper size of anything = model size / factor.

### 6.2 Annotation types

**note** — leader note (or keyed note, §24):
```jsonc
{ "id": "n1", "type": "note",
  "text": "2X8 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.",
  "target": "sill_plate",                // component id (or "comp.part"); arrow lands inside its visible region
  "at": null,                            // optional Ref: exact arrow landing point
  "place": null,                         // optional [x, y] model-space position of text (designer override)
  "key": null,                           // keyed views: number in the DETAIL NOTES list (§24)
  "cite": [ { "code": "IRC", "edition": 2021, "section": "R403.1.6", "title": "Foundation anchorage",
              "status": "suggested" } ]   // suggested | verified
}
```
**dim** — linear dimension:
```jsonc
{ "id": "d1", "type": "dim", "from": "footing@bottom_left", "to": "footing@bottom_right",
  "dir": "h", "offset": -6, "text": null }   // dir h | v | aligned; offset model inches, sign = side
```
**label** — free text without leader (e.g. `EXTERIOR`, `INTERIOR`, `GRADE`):
```jsonc
{ "id": "l1", "type": "label", "text": "EXTERIOR", "at": "slab@top_exterior", "offset": [-18, 6] }
```
**weld** — AWS weld symbol at a joint (section views; placed exactly, not moved by the note layout):
```jsonc
{ "id": "w1", "type": "weld", "at": "column@bottom_right",   // arrow tip at the joint
  "offset": [6, 6],                  // elbow = at + offset (model inches; default 1/2" paper up-right); the reference line runs away from the arrow
  "weld": "fillet",                  // fillet | cjp | pjp (bevel groove, CJP/PJP in the tail)
  "size": "3/16", "sides": "arrow",  // arrow (symbol below the line) | other (above) | both
  "all_around": true, "field": false, "tail": "TYP.",
  "members": ["column", "base_plate"] }   // checked by W_WELD_SIZE
```
Fillet: right triangle, vertical leg on the left, height = text height; size text right-aligned left of the symbol.

**cite** rules: the LLM may only create `status: "suggested"`. Only the designer (UI action)
sets `verified`. Export renders citations per style `citations` (default: append
` (IRC R403.1.6)`; unverified ones get a trailing `*` and the sheet gets the footnote
`* CODE REFERENCE NOT VERIFIED BY DESIGNER`). The case transform applies to the note text, the code and the format's
literal text; words of `section` that hold a digit (`1926.755(a)(1)`, `R602.3(5)`) print as written (§25.3).

### 6.3 Note layout (deterministic; engines MUST implement this algorithm)
1. Text: style case transform (default UPPERCASE; the tokens of §25.3 keep their case), word-wrap at `style.notes.wrap_chars` (28)
   using stroke-font advance widths × text height; never break inside a word unless longer than the line.
2. Landing point per note: `at` if given (in an iso projected from the near face of its component, §25.2); else the
   target's visible-region **label point**: centroid of its largest visible polygon if inside it, otherwise the point
   inside on the horizontal line through the centroid's y that is midway between the two nearest boundary crossings.
3. Column: notes with `place` are fixed. Others go to a column at `crop.x1 + gutter` (right) or
   `crop.x0 − gutter − text_width` (left) (`gutter` = style paper inches × scale factor). For
   `both`, a note goes to the side nearer its landing point.
4. Ordering: sort column notes by landing y descending (top first); initial y = landing y; then
   resolve overlaps top-down: each note's top must be ≤ previous note's bottom − `note_gap`.
   If the column overflows the crop bottom, shift the whole column up (then down if it overflows the top).
   Leaders of one column that still cross are untangled (§25.2).
5. Leader: from the text's vertical middle on the near side (with a horizontal shoulder of
   `shoulder` paper inches) straight to the landing point; arrowhead per style.
6. Keyed mode (view `notes_style: "keyed"`, or style `notes.mode`): same placement, but the column
   shows the note's number as a tag and the full texts go to a DETAIL NOTES list (§24).

### 6.4 Dimensions (deterministic)
Extension lines from the two points (gap `ext_gap`, overshoot `ext_over`), dimension line at
`offset`, terminators per style (`tick` = 45° heavy slash, default), text centered above the
dimension line, feet-inch format, rotated for vertical dims (reading from the right). If text
does not fit between the extension lines it moves outside to the right/top. Colliding dimensions stack outward and
a dimension text on a note leader first slides along its line (§20, §25.2); a dimension drawn more than 1/8" (paper)
from its `offset` reports `I_ANNOT_MOVED`, and `"pin": true` keeps it at its `offset`.

### 6.5 Title
Under each view: a detail bubble (circle, number on top, sheet on bottom if `meta.sheet`),
the title text underlined (heavy), and `SCALE: 1 1/2" = 1'-0"` beneath. The bubble's full circle is part of the view's
extents, and the view with its notes, list and title keeps 1/8" (paper) clear of the frame and the title block (§25.2).

## 7. Style (`*.kerfstyle.json`)

The style owns ALL appearance. Shape (see `spec/styles/kerf-standard.kerfstyle.json` for the
full default):

```jsonc
{
  "kerfstyle": "0.1", "id": "kerf-standard",
  "pens": { "cut": {"width_mm": 0.50}, "profile": {"width_mm": 0.35}, "beyond": {"width_mm": 0.18},
            "hidden": {"width_mm": 0.18, "dash_mm": [3, 1.5]}, "hatch": {"width_mm": 0.09},
            "rebar": {"width_mm": 0.35}, "steel": {"width_mm": 0.35}, "anno": {"width_mm": 0.18},
            "dim": {"width_mm": 0.13}, "break": {"width_mm": 0.18}, "title": {"width_mm": 0.50} },
  "materials": { "<material>": { "hatch": [ {"pattern": "ANSI31", "scale": 1, "angle": 0} ],
                                 "cut_mark": null | "x" | "diagonal", "color3d": "#RRGGBB",
                                 "layer": "S-DETL-CONC" } },
  "patterns": { "<NAME>": [ [angle, x0, y0, dx, dy, dash1, dash2, …], … ] },   // AutoCAD .pat semantics, paper inches
  "text":  { "font": "kerf-simplex", "height_in": 0.09375, "title_height_in": 0.15625, "case": "upper", "line_spacing": 1.6,
             "keep_case": [] },                   // extra tokens printed as written under "upper" (§25.3)
  "notes": { "mode": "leader", "wrap_chars": 28, "gutter_in": 0.375, "shoulder_in": 0.125, "note_gap_in": 0.0625, "arrow": "closed_filled", "arrow_len_in": 0.09375,
             "key_tag": "plain", "list_title": "DETAIL NOTES",    // mode leader | keyed | list; key_tag plain | circle | hex
             "abbreviations": {} },               // extra house abbreviations, {"BRG.": "BEARING"} (§25.3)
  "dims":  { "terminator": "tick", "tick_len_in": 0.0625, "ext_gap_in": 0.0625, "ext_over_in": 0.0625, "text_gap_in": 0.0469, "precision": 16 },
  "citations": { "format": " ({code} {section})", "unverified": "flag", "flag": "*",
                 "footnote": "* CODE REFERENCE NOT VERIFIED BY DESIGNER" },
  "layers": { "cut": "S-DETL-CUT", "beyond": "S-DETL-BYND", "hidden": "S-DETL-HIDN", "hatch": "S-DETL-PATT",
              "notes": "S-ANNO-NOTE", "dims": "S-ANNO-DIMS", "title": "S-ANNO-TTLB", "break": "S-DETL-BRKL" },
  "sheet": { "size_in": [11, 8.5], "margin_in": 0.375, "title_block": "kerf-strip" },
  "break_line": { "zig_in": 0.125, "period_in": 0.5 }
}
```

All `*_in` / `*_mm` are **paper** units; engines convert to model units by × scale factor
(mm ÷ 25.4 for widths). Pen widths stay in paper units (SVG/PDF stroke widths; DXF lineweight
enum on the layer, nearest standard value).

Hatch rendering: patterns are AutoCAD `.pat` line families in paper inches, multiplied by
`scale`, rotated by `angle`, anchored at model origin (so adjacent regions of the same material
align). Engines clip family lines to the region (even-odd, holes for embedded items).

Materials (MVP): `concrete`, `grout`, `cmu`, `mortar`, `earth`, `gravel`, `sand`,
`compacted_fill`, `wood`, `wood_engineered`, `plywood`, `osb`, `gypsum`, `fiber_cement`,
`wood_board`, `steel`, `rebar`, `insulation_rigid`, `insulation_batt`, `membrane`,
`underlayment`, `vapor_retarder`, `wrb`, `shingles`, `generic`.

## 8. Compilation pipeline

1. **Parse** (accept lengths in any §1 form) → **validate params** (`E_PARAM` with the field path).
2. **Expand builders** into parts: each part is a **Prism** `{ comp, part, instance, material,
   profile: Region (outer loop + holes; segments line or arc), z0, z1, role flags (embedded,
   cut_mark, blocking, outline mode) }`.
3. **Resolve placement** DAG; apply translate / rotate / mirror; expand arrays.
4. **Validate** (§9) → diagnostics.
5. **Views** → Drawing IR (§10): section (§8.1) or iso (§8.2), then annotations, title, crop,
   break lines.
6. **Mesh** (§11) for the 3D viewer.
7. **Export** Drawing → SVG / DXF / PDF (§12).

### 8.1 Section view (exact, 2D)
Look direction −Z, cut plane z = `cut_z`.
- **Cut** prisms: z0 < cut_z < z1. Their profiles are the cut regions (hatched, pen `cut`;
  `embedded` ones drawn last on top). `cut_mark` (wood X / diagonal) is drawn ONLY for members
  whose length runs along Z (lumber `run:"z"`, i.e. seen in cross-section); members cut
  lengthwise (run x/y, truss chords) draw outline only.
- **Beyond** prisms: z1 ≤ cut_z (behind the cut plane, i.e. farther from the viewer)… note the
  viewer is at +Z looking −Z, so "beyond" means z1 ≤ cut_z. Prisms entirely in front of the cut
  (z0 ≥ cut_z) are removed.
- Visibility among beyond prisms: a beyond prism P is occluded by every cut region and by every
  beyond prism Q with Q.z1 > P.z1 (nearer). Visible outline of P = P's profile boundary minus
  the union of occluders' interiors (2D boolean, exact with arcs). Draw with pen `beyond`.
- `embedded` items are never occluded: they draw over cut regions and nearer prisms, in their
  own pen (cut ⇒ `rebar`/`steel` fill; beyond ⇒ outline in `rebar`/`steel` pen).
- Crop: clip everything to `crop`. Where a **cut** region's boundary is clipped by the crop
  rectangle, draw a break line (style `break_line`) along that crop edge segment instead of a
  plain edge, extended `overshoot_in` past the region. Fills (earth, gravel, sand,
  compacted_fill) never get break lines; their hatch simply stops at the crop.
- Coincident edges between abutting cut regions draw once (dedupe collinear overlapping segments
  with tolerance 1e-4; heavier pen wins).

### 8.2 Iso view (hidden-line removal on planar faces)
Projection for `from: front_right`: view direction d = normalize(−1, −1, −1)·… use the
standard isometric: screen x = (x − z)·cos30°, screen y = y + (x + z)·sin30°… precisely:
```
u = (x·cos30° − z·cos30°)          // for front_right; front_left mirrors x
v = (y + x·sin30° + z·sin30°)
depth = (x + z)·k − y·k'  (any monotone depth along the view direction; use the dot product with d)
```
Engines MUST use the true orthographic projection along d = (1, 1, 1)/√3 rotated to the
chosen quadrant, with Y up on screen; the formula above is illustrative.
Algorithm:
1. Tessellate each prism into planar faces: two caps + one side face per profile segment
   (arcs split into chords at 2° max). Mark side edges at arc-interior vertices `smooth`.
2. Back-face cull faces (normal · d ≥ 0 removed as occluders? — no: keep all faces as
   occluders; only front-facing faces contribute visible edges).
3. Candidate edges: sharp edges with ≥1 front-facing adjacent face; smooth edges only where
   adjacent faces differ in facing (silhouettes).
4. For each candidate edge (projected 2D segment with depth along it): split at intersections
   with the projected boundaries of front-facing occluder faces whose screen bbox overlaps;
   for each sub-segment test the midpoint against each occluder (point-in-polygon in screen
   space, then compare depth on the face plane, tolerance 1e-4·scene size). Hidden if any
   occluder is nearer.
5. Merge collinear visible sub-segments; dedupe coincident edges.
6. `cutaway: true`: clip prisms to z ≤ cut_z first; the cap at z = cut_z of each clipped prism
   gets its material hatch mapped through the projection (affine transform of the 2D hatch).
7. Pens: silhouette/outline `profile`, other visible edges `beyond`, cutaway face outlines `cut`.
Iso annotations use the same note algorithm with landing points projected from 3D (the
target's label point on the cut face if cutaway, else the centroid of its visible faces).

## 9. Validation (diagnostics)

Each diagnostic: `{ "level": "error"|"warning"|"info", "code", "id"?, "path"?, "message", "fix"? }`.
Messages are written FOR AN LLM: what is wrong, measured numbers, and the concrete fix.

| code | level | rule |
|---|---|---|
| `E_PARAM` | error | bad/missing param; message lists allowed values |
| `E_DUP_ID` | error | duplicate id |
| `E_REF_UNKNOWN` | error | ref to missing component/part; suggest nearest id (edit distance) |
| `E_ANCHOR_UNKNOWN` | error | unknown anchor; list the component's anchors |
| `E_CYCLE` | error | placement cycle |
| `W_OVERLAP` | warning | two cut regions overlap by > 0.01 in² (report overlap bbox in ft-in). Exempt: `embedded` items, membranes, connectors, and fills vs membranes |
| `W_FLOATING` | warning | a component touches nothing (gap > 1/32" to every other; concrete/masonry pieces facing across a joint < 1" touch), or some instances of an `array` touch nothing while others do (§25.5) |
| `E_PLACE_OUTSIDE` | error | a `rebar.place` bar centre falls outside its zone (cover deeper than the host, or a `station` past the face); the bar is not drawn (§25.1) |
| `W_BAR_OUTSIDE` | warning | an along-z bar centred in no concrete/masonry host drawn with it, or a path bar with a bend outside every host or no point in one; names the nearest host (§25.1) |
| `W_CUT_FILL` | warning | an embedded part emptied a cut member's fill, or a boolean fault was caught and the fill kept (§25.1) |
| `W_COVER` | warning | rebar clear cover < required. Host = the concrete/cmu prism containing the bar center. Each host boundary edge is classified by its outward normal: n.y < −0.5 ⇒ `bottom`, n.y > 0.5 ⇒ `top`, else `sides`. Required cover comes from the host's `cover`, with `cover.parts.<part>` overriding for bars inside that part zone. cmu_wall default cover `{ "sides": 1.5, "top": 1.5, "bottom": 0.5 }`. The message names the knob (`cover.<face>`, `cover.parts.<part>.<face>`, `, default` when unset) and the fix carries the exact `update` op (§25.5) |
| `W_UNTREATED_CONTACT` | warning | wood (`treated:false`) touching concrete/grout/cmu (IRC R317.1) |
| `W_NOTE_TARGET` | warning | note target not visible in its view |
| `W_VIEW_FIT` | warning | view + notes + title (bubble included) exceed the sheet frame, less 1/8" clearance, at the chosen scale; with an auto crop it names the components that stretch it (§25.5) |
| `W_NOTE_LANDING` | warning | a note's explicit `at` puts the arrow on empty paper: outside the crop (section), off every visible face and line (iso) |
| `W_LABEL_OVERLAP` | warning | a label overprints a label or dimension text, a note's text overprints either, or a pinned dimension overprints another |
| `I_ANNOT_MOVED` | info | the layout drew a dimension more than 1/8" (paper) from its `offset`, or moved a label that far (printed by `kerf check`) |
| `I_UNVERIFIED_CITE` | info | citations awaiting designer verification (count) |
| `W_BOLT_EDGE` | warning | a bolt's edge distance to a `steel` ply it passes through (in the drawing plane, or along z to the ply's z extent) is below AISC 360-16 Table J3.4 (1/2" 3/4, 5/8" 7/8, 3/4" 1, 7/8" 1 1/8, 1" 1 1/4, 1 1/8" 1 1/2, 1 1/4" 1 5/8, larger 1.25d); one warning per bolt group and ply, the worst bolt |
| `W_BOLT_SPACING` | warning | bolt spacing < 2 2/3 d (AISC 360 J3.3); two anchor bolts closer than 4d (cast-in) or 6d (wedge/screw) center to center, in plane and z (ACI 318-19 17.9.2) |
| `W_BOLT_EDGE` (anchor bolt) | warning | an `anchor_bolt` shaft (its deeper 60% inside concrete/masonry) < 1 1/2" clear of the host edge (ACI 318-19 17.9.2, cover per 20.5.1.3) |
| `W_WELD_SIZE` | warning | a fillet weld symbol with `members` is larger than the thinner part allows (t, or t − 1/16 when t ≥ 1/4; AISC 360 J2.2b) or smaller than Table J2.4 (1/8, 3/16, 1/4, 5/16 by thinner part ≤ 1/4, 1/2, 3/4, over). Thickness of a W or channel is its thinner element, of an HSS or pipe the design wall |
| `I_SOLID_USED` | info | `solid` escape hatch used |
| `W_REF_UNKNOWN` | warning | a view `omit` / `only` entry (id or `*` glob) that matches no component; it has no effect (§25.4) |
| `W_ACK_UNUSED` | warning | `acknowledge` entries of a component that suppressed nothing in any view (stale, repeated, or an `I_*` code); one per component, the fix is the `update` op that removes them. Only when every view was built and the document has no errors (§25.4) |
| `I_ACK` | info | a warning suppressed by `acknowledge` (code, component, reason, the original message); `diagnostics` only, the summary groups them (§25.4) |

**Per-view geometry checks.** `W_OVERLAP`, `W_FLOATING`, `W_UNTREATED_CONTACT`, `W_COVER`, `W_NEAR_MISS` and
`W_SHORT_SLOPE` run over the components each view draws: compiled, `visible`, and members of that view (component
`views`, view `only` and `omit`, §25.4; iso views included, with their own membership). Views that draw the same components are checked once. A finding is reported once; when
it occurs in some views but not all, its message ends with ` (in view B)` / ` (in views A, C)`. A finding in every view,
and every finding of a document without views (all components checked together, `visible` or not), carries no view
list. So a multi-scenario detail (one view per scenario, the other scenarios' members excluded) gets no warnings between
members that are never drawn together, and needs no `acknowledge` for them. `W_SHORT_SLOPE` keeps using the first
section view's explicit crop for every view.

## 10. Drawing IR (`kerf_drawing` JSON)

Output of a view; input of every exporter and of every UI's 2D renderer. Model-space inches.

```jsonc
{
  "kerf_drawing": "0.1", "doc": "truss-bearing-cmu", "view": "A", "kind": "section",
  "scale": 8, "bounds": [x0, y0, x1, y1],
  "pens": { "cut": {"width_mm": 0.5, "dash_mm": null}, … },
  "layers": [ {"name": "S-DETL-CUT", "lineweight_mm": 0.5}, … ],
  "items": [
    { "t": "path",  "layer": "S-DETL-CUT", "pen": "cut", "src": "sill_plate", "closed": true,
      "pts": [[x, y, bulge], …] },
    { "t": "fill",  "layer": "S-DETL-CUT", "src": "r1", "loops": [[[x,y,b],…]] },      // solid fill (rebar dots, arrowheads, steel)
    { "t": "hatch", "layer": "S-DETL-PATT", "pen": "hatch", "src": "footing", "pattern": "KERF-CONC",
      "scale": 1, "angle": 0, "loops": [[[x,y,b],…], …],                                  // loop 0 outer, rest holes
      "lines": [[x0,y0,x1,y1], …] },                                                       // pre-clipped pattern lines (for SVG/PDF/UI)
    { "t": "text",  "layer": "S-ANNO-NOTE", "pen": "anno", "src": "n1", "s": "2X8 PT SILL…",
      "x": 0, "y": 0, "h": 0.75, "rot": 0, "align": "left", "valign": "baseline" }
  ],
  "diagnostics": [ … ],
  "acknowledged": [ { "code", "id", "reason", "path"?, "message", "fix"? }, … ]   // only when the doc has `acknowledge` (§25.4)
}
```
- Bulge semantics as DXF LWPOLYLINE (bulge on vertex i applies to segment i→i+1).
- `src` ties every item to a component/annotation id (UI picking, highlighting, diff).
- Text `h` is model-space cap height (= paper height × scale). UIs and SVG/PDF render text with
  the stroke font; DXF writes TEXT entities with style `KERF` → `romans.shx`.

## 11. Mesh (for 3D viewers)

```jsonc
{ "kerf_mesh": "0.1",
  "parts": [ { "src": "sill_plate", "part": null, "instance": 0, "material": "wood", "color": "#C9A46A",
               "positions": [x,y,z,…], "normals": [...], "indices": [...],
               "edges": [x0,y0,z0,x1,y1,z1, …] } ] }   // feature edges (sharp + profile outline), not triangle edges
```
Viewers draw `edges` as lines (constant pixel width) over flat-shaded faces. Never derive edges
from triangles.

## 12. Exporters

**SVG** — paper inches as user units ×96 (CSS px), `viewBox` = sheet; black ink on white;
strokes in mm converted; text as stroke paths (stroke font) in a `<g>` per layer with
`inkscape:label`/`id` = layer name; one `<g data-src="…">` per source id.

**DXF** — AutoCAD R2000 (AC1015), ASCII. `$INSUNITS = 1` (inches), `$MEASUREMENT = 0`.
Modelspace at 1:1 model inches. Tables: LTYPE (CONTINUOUS, DASHED for hidden), LAYER (from style,
with lineweights and color 7), STYLE (`KERF` font `romans.shx`), BLOCK_RECORD, plus the
required OBJECTS dictionary. Entities: LWPOLYLINE (with bulges), HATCH (pattern fill with the
pattern definition lines embedded, associative = 0; SOLID fill for `fill` items), TEXT,
SOLID or HATCH for arrowheads. Dimensions are exported exploded (lines + TEXT on the dims
layer) in MVP. MUST pass `tools/dxf_check.py` with zero audit errors and open in LibreCAD.
Sheet frame/title block NOT written to DXF unless `--with-sheet`.

**PDF** — PDF 1.7, one page per view at style sheet size; vector only; detail placed at true
scale inside the frame; title block per `spec/DESIGN.md §Sheet`; text as stroked paths (no font
embedding needed); deterministic object order; no timestamps.

## 13. Engine API

The engine exposes the same functions to its CLI, its wasm build, and in-process callers.
All inputs/outputs are UTF-8 JSON unless noted.

| fn | input | output |
|---|---|---|
| `version` | `{}` | `{ "engine": "kerf", "version": "0.1.0", "spec": "0.1" }` |
| `catalog` | `{ "format": "json"|"markdown" }` | catalog |
| `fmt` | `{ doc }` | `{ doc }` canonicalized |
| `check` | `{ doc, style, strict?: bool, reference_files? }` | `{ diagnostics, acknowledged, summary }` (`reference_files`: §23; `strict`, `acknowledged`: §25.4) |
| `apply` | `{ doc, style, ops, actor?: "llm"|"designer" }` | `{ ok, doc, diagnostics, acknowledged, summary, changed }` — atomic: on any error `ok:false`, doc unchanged |
| `inspect` | `{ doc, style, query }` | see §14 |
| `drawing` | `{ doc, style, view }` | Drawing IR |
| `mesh` | `{ doc, style }` | Mesh |
| `export` | `{ doc, style, view, format: "svg"|"dxf"|"pdf", sheet?: bool, review?: bool }` | **raw bytes** (`review`: §25.4) |

`summary` is the compact text the LLM reads after every change, e.g.:
```
DOC truss-bearing-cmu  14 components  2 views  0 errors  1 warning  0 acknowledged
 cmu          cmu_wall 8" x 6 courses (1 bond beam)   x 0..7 5/8"     y -4'-0"..0"
 sill_plate   lumber 2x8 PT flat run z                 x 0..7 1/4"     y 0..1 1/2"
 …
WARN W_COVER bb_bars: clear cover to cmu.bond_beam top is 1 1/8" < 1 1/2" — set place.cover to 1.5
```
Format: one line per component: id (padded 14), type + key params (padded 44), x range, y range
in ft-in. Then diagnostics. Keep it under ~60 columns of params.

### 13.1 Wasm ABI (raw, no wasm-bindgen / no Emscripten)
The wasm build exports exactly:
```
memory
kerf_alloc(len: u32) -> u32                      // pointer to len bytes
kerf_free(ptr: u32, len: u32)
kerf_call(fn_ptr: u32, fn_len: u32, in_ptr: u32, in_len: u32) -> i32   // 0 ok, 1 error (output is error JSON)
kerf_out_ptr() -> u32
kerf_out_len() -> u32
```
The host writes the fn name and input JSON into allocated buffers, calls `kerf_call`, then
copies `[kerf_out_ptr, +kerf_out_len)` before the next call. The module imports nothing. The output buffer is owned by the engine until the next call.
`apps/web/src/engine.ts` is the single loader.

### 13.2 CLI
```
kerf version
kerf catalog [--markdown]
kerf fmt <doc> [-w]
kerf check <doc> [--style S] [--strict]
kerf apply <doc> <ops.json> [--style S] [-o out.kerf.json]
kerf drawing <doc> --view A [--style S] [-o out.json]
kerf export <doc> --view A --format svg|dxf|pdf [--style S] [--sheet] [--review] -o <file>
kerf mesh <doc> [-o mesh.json]
kerf call <fn> < input.json         # raw API access, used by conformance tests
```
Default style: the embedded copy of `spec/styles/kerf-standard.kerfstyle.json` (compiled in).

## 14. Ops (what the LLM's `kerf_apply` tool sends)

```jsonc
[
  { "op": "add",    "path": "components", "value": { …component… }, "before": "<id>"? },
  { "op": "update", "path": "components/sill_plate", "value": { …JSON merge patch… } },
  { "op": "remove", "path": "components/sill_plate" },
  { "op": "add",    "path": "views", "value": { …view… } },
  { "op": "update", "path": "views/A", "value": { …merge patch (annotations excluded)… } },
  { "op": "add",    "path": "views/A/annotations", "value": { …annotation… } },
  { "op": "update", "path": "views/A/annotations/n3", "value": { …merge patch… } },
  { "op": "remove", "path": "views/A/annotations/n3" },
  { "op": "update", "path": "meta", "value": { … } },
  { "op": "set",    "path": "doc", "value": { …entire document… } }      // replace all (first build)
]
```
- Merge patch = RFC 7396 (null deletes a key).
- `remove` of a component still referenced ⇒ `E_REF_UNKNOWN` naming the dependents (atomic fail).
- With `actor: "llm"` (the default; the LLM tool path always uses it) a citation can never be set
  to `verified`: it is downgraded to `suggested` with an `I_CITE_DOWNGRADED` diagnostic. Only
  `actor: "designer"` (UI actions) may set `verified`. Any later LLM edit to a note's `text` or
  `cite` resets that note's citations to `suggested`.
- `changed`: list of ids touched.

`inspect` queries:
- `{ "q": "summary" }` → summary text.
- `{ "q": "component", "id": "truss" }` → resolved params, parts, every anchor with coordinates (ft-in and decimal), z range.
- `{ "q": "anchors", "id": "truss" }` → anchors only.
- `{ "q": "at", "point": [x, y], "view": "A" }` → which components are at a point.
- `{ "q": "catalog", "type": "truss" }` → that catalog entry.

## 15. Stroke font
`spec/fonts/kerf-simplex.json`: `{ "cap_height": 21, "glyphs": { "A": { "adv": 18, "strokes": [[[x,y],…],…] }, … } }`
in Hershey units (cap height 21 for Roman Simplex). Text of height h scales by h / cap_height.
Unknown glyphs render as `?`. Engines embed the font at build time. DXF uses `romans.shx`,
whose metrics are close; minor differences in CAD are acceptable.

## 16. Clarifications (from building the reference details)

- **Dimension offset:** `dir:"h"` ⇒ dimension line at y = max(from.y, to.y) + offset when
  offset > 0, min(from.y, to.y) + offset when offset < 0. `dir:"v"` ⇒ same with x. `aligned` ⇒
  offset perpendicular to from→to (positive = left of the direction).
- **Label:** `{ "type": "label", "text", "at": Ref|[x,y], "offset": [dx, dy], "align" }`: the text block centered
  vertically on the point, each line's start (`align: "left"`), middle (`"center"`, default) or end (`"right"`) on it; a
  newline in `text` starts a new line (pitch `line_spacing`); label text height (`text.label_height_in`), no leader.
- **Fill outline `top`:** stroke only polygon edges whose outward normal has n.y > 0.01.
- **Membrane / connector thickness side:** for a polyline direction (dx, dy), "left" is the
  normal (−dy, dx).
- **Materials added:** `aluminum` (outline only, no hatch; 3D `#B8BCC2`), used for schematic door
  sills/tracks via `solid`.
- **cmu_wall `cover`** param exists (default in §9) for `W_COVER`.
- **concrete `cover.parts`:** per-part overrides, e.g. `{ "bottom": 3, "sides": 3, "top": 1.5,
  "parts": { "slab": { "bottom": 0.75 } } }` (slab-on-grade mesh over vapor retarder).
- **Canonical JSON (`fmt`, `apply` output):** 2-space indent, UTF-8, `\n` line ends, trailing
  newline. Keys: schema order as listed in this spec, then unknown keys alphabetically. Arrays
  whose elements are all numbers print inline `[a, b]`; all other arrays one element per line.
  Numbers per §1. Strings with minimal JSON escaping (no `\u` escapes for printable non-ASCII).
- **Anchors of connectors / membranes / path rebar:** the 9 box anchors are the axis-aligned
  bounding box of the resolved profile (they have no local frame).
- **Array instance refs:** `id#k@anchor` addresses instance k; `id@anchor` = instance 0.
- **Section "beyond"** includes prisms whose z1 equals cut_z exactly (touching the plane).
- **Detail-1 style check:** `truss` geometry for the standard heel: bottom chord top at
  bearing = 3.5 above bearing (2x4); top chord's lower edge passes through
  (bearing_outer.x, bottom-chord top) at the pitch; `tail_top` = top of top chord at the plumb tail.
- **Thin cut regions:** a cut region whose minimum paper thickness is < 2× the `cut` pen width
  renders as a solid `fill` plus an outline in the material's pen (steel → `steel`). This applies
  to sheet metal, straps, flashing and thin panels. Membranes render at least their pen width.
- **Break lines across groups:** the crop edges of adjacent cut regions (touching or sharing an
  edge) merge into ONE continuous break line spanning the whole member group (e.g. a CMU wall's
  face shells + grout).
- **Notes vs dimensions:** the notes-column x position clears the union of crop, dimension
  geometry and labels. Dimension text and label boxes are obstacles that leaders must not cross.
- **Leader de-crossing:** after the §6.3 y-ordering, repeatedly swap adjacent column notes whose
  leaders intersect (scan top→bottom, at most n² swaps), then re-run the overlap resolution.
- **Accepted interpretations (from engine NOTES):** canonical key order puts the common fields
  (`id type label material at rotate slope mirror z array embedded visible`) first, then type
  params in catalog order. `slab_edge.recess.depth` is measured at the interior end, and the floor
  falls by `recess_slope` toward the exterior. In point lists with `at`, literal points are
  relative to `at.to + offset` and `anchor` is ignored. Shared edges between prisms of the SAME
  component (CMU shell/grout/mortar) draw in the `beyond` pen.
- **`place` anchor:** `place` is the model-space point at the TOP-LEFT of the note's text block,
  i.e. the left end of the first line, one cap height above that line's baseline (for
  right-aligned notes the text grows leftward from `place.x + block width`; `place` remains the
  top-left of the block's bounding box).
- **`drawing` / `export` `view` argument:** the view id string (`"A"`). `export` takes an
  optional `sheet: true` (PDF always produces a sheet).
- **`catalog` markdown:** returned as raw UTF-8 text (not JSON-encoded) from the CLI and wasm.
- **View `omit`:** `"omit": ["roofing", "roof_sheathing"]`, a list of component ids (or `*` globs, §25.4) excluded
  from that view only (all instances). Typical use: peel back sheathing or finishes in an iso so the
  framing reads. An entry that matches no component ⇒ `W_REF_UNKNOWN` (was `E_REF_UNKNOWN` before v0.2.0-alpha.5).
  Annotations targeting an omitted component ⇒ `W_NOTE_TARGET`.
  The geometry checks run per view over what the view draws (§9), so `omit` is also how one document holds
  alternative scenarios.
- **Fills in iso:** `fill` components (earth, gravel, sand, compacted_fill) never render as 3D
  blocks in iso views. With `cutaway: true` only their cut face at `cut_z` is drawn (hatched,
  without a heavy outline except the grade line chain per `outline`). Without cutaway they're omitted.
  The 3D mesh (§11) also omits fills unless a viewer asks for them (`mesh` input `{ include_fills: true }`).
- **Glyph folding (all text, all outputs):** before layout, fold characters the stroke font lacks:
  `— – ‒ −` → `-`, `“ ” „` → `"`, `‘ ’ ‚` → `'`, `×` → `X`, `°` → ` DEG` (the font has no degree
  sign), `½ ¼ ¾ ⅛ ⅜ ⅝ ⅞` → ` 1/2` etc., NBSP → space. Anything else outside ASCII 32–126 → `?`
  with an `I_GLYPH` diagnostic naming the character. DXF TEXT receives the folded string too.

## 17. LLM ergonomics (v0.1.1, from dogfooding)

- **`until` instead of `length`** (lumber run x/y, panel, and any other member that takes `length`):
  `"until": "<Ref>"` makes the member grow from its placement anchor along its run axis until
  its far end reaches the Ref's coordinate on that axis (x for run x, y for run y). The engine
  computes `length` (and reports it in the summary, e.g. `L=7'-0 3/4" (until lower_plate@bottom_left)`).
  Growth direction: away from the anchor (anchor `top_*` grows down, `bottom_*` grows up, `*_left` grows
  right, `*_right` grows left; center anchors are an error). Giving both `length` and `until` ⇒ `E_PARAM`.
  `until` adds a placement dependency (DAG). Example: a jack stud is
  `{"at": {"anchor": "top_left", "to": "beam@bottom_left"}, "until": "bottom_plate@top_left"}`.
- **`W_NEAR_MISS`** (warning): two non-fill, non-annotation components whose extents overlap on
  one axis but leave a gap of 1/32"–3" between facing edges on the other axis (e.g. a stud ending
  3" below the plate it obviously meant to reach). Message names both ids, the gap in ft-in, and
  the fix (`set length to …` or `use "until": "<other>@<anchor>"`). Skip pairs that are already
  separated by a third component in the gap: any drawn component (panels, hardware and other instances included, §25.5).
- **Catalog lists hardware models:** the `connector` entry lists every model in the engine's
  hardware table with kind, width, gauge, and length (one line each), so the LLM knows which
  models auto-fill.
- **Anchor bolt geometry (exact, for cross-engine parity):** with d = diameter, the shaft is centered on the
  placement point's x and runs from +projection down to −embed (relative to `top_of_concrete`).
  `hook:"J"`: at the bottom, a 180° bend with inside radius 1.5·d toward +x, returning upward
  by `hook_len` (default 2") measured from the bend's lowest point. `hook:"L"`: a 90° bend toward +x
  with inside radius 1.5·d and a horizontal leg ending `hook_len` (default 3") from the shaft centerline.
  `headed`: a square head 2·d wide and 0.5·d thick at the bottom. Nut: 1.5·d wide × 0.875·d tall with its
  top at +projection − 0.25·d (thread stick-out). Washer: 2.25·d wide × 0.125 thick, directly under the nut.
  `nut_washer:false` omits both.
- **Feet-inch with zero inches and a fraction:** `3'-0 1/4"` (never `3'-1/4"`); `-0'-0 1/4"` prints as `-1/4"`.
- **Parity decisions (from the cross-engine compare):**
  - Thin-region rule applies to **metal only** (`steel`, `aluminum`, `flashing_membrane`); fill +
    outline in the material pen (`steel`). Thin non-metal panels keep the normal outline.
  - Break line symbol: a polyline crossing the member group with ONE zigzag at its middle:
    `start, a, peak, valley, b, end`, i.e. 6 vertices; peak/valley offsets ±`zig_in`·scale, zig width
    `zig_in`·scale; ends extended by `overshoot_in`·scale. Pen `break`, layer `S-DETL-BRKL`.
  - Note landing ties: when candidate visible polygons tie on area (relative difference < 1e-6), take
    the first in Drawing item order (component order, then part order, then instance order).
  - Vapor retarder always uses pen `vapor` (dashed), never `cut`.
  - Hatch phase: pattern origin at model (0,0) after the family's own x0/y0, never per-region.
  - NTS (iso) fit: start at the factor that fits the cropped geometry in the frame, then grow the
    factor (at least 0.5 per trial, else the factor at which the geometry fits beside the annotations' paper size,
    §25.2) until drawing + notes + title fit. An NTS view raises `W_VIEW_FIT` only when its annotations alone do
    not fit.
- **Region items (for picking, not drawn):** for every visible cut region and every visible
  beyond face in a section/iso view, the Drawing IR also emits
  `{ "t": "region", "src": "<id>", "part": "<part>|null", "instance": k, "cut": true|false, "loops": [[[x,y,b],…],…] }`
  with exact closed loops (loop 0 outer, then holes), in drawing order. Exporters ignore region
  items. UIs use them for hit-testing and selection tint.

## 18. v0.1.2 additions (from the first field test: Grok Build on Windows, Palmer SD1 sheet)

**Annotation quality**
- Note text **and its citations** get the style `case` transform (the citation suffix was printing
  `Table R602.3(5)` in mixed case).
- **Leader routing:** leaders must not cross each other, and must not cross dimension text, labels or
  the title. After the §16 de-crossing swap, if crossings remain, try the other column (`both`), then
  nudge the landing point within the target's visible region. Remaining hits ⇒ `W_LEADER_HIT`
  (warning): a leader or arrowhead within one text height of another leader, note box, dimension
  text, label or title. The message names both items and suggests `place`, `at` or `offset` values.
- **Auto landing** prefers the target's visible-region label point at least 2 text heights away from
  crop edges and break lines (pole of inaccessibility of the visible region minus a crop-edge band).
- **`W_VIEW_FIT`** reports the overflow per edge in paper inches (`left 0.07", bottom 0.00" …`) and
  names the culprit (crop geometry, the notes column, dimensions, the title block). It suggests the
  smallest fix first (shrink the crop, `notes_side`, dim `offset`) and a scale change only when the
  overflow is greater than 15% of the frame.

**Placement & validation**
- `rebar.place.face` also accepts `"center"`: bars are centered in the host zone on both axes. With
  `count > 1` they spread horizontally at `side_cover`, or vertically with `axis: "y"`.
  `place.station: <in>` positions a single bar at that x offset (or y with `axis: "y"`) from the
  zone's left (bottom) face instead of centering it.
- `W_COVER` on a `path` bar names the failing segment (index and endpoints in ft-in) and the face.
- **"Where occurs" graphics:** any component may set `"shown": "dashed"`. It draws all its visible edges
  in the `hidden` pen (no hatch, no cut mark), is exempt from `W_FLOATING` / `W_NEAR_MISS` / `W_OVERLAP`,
  and its notes get the suffix ` (WHERE OCCURS)` unless the text already says so.

**New/extended components**
- `anchor_bolt.hook: "wedge"`: a post-installed expansion anchor. Straight shaft, expansion clip
  drawn at the embedded end (sleeve 0.6·embed long, 1.15·d wide), nut and washer as usual. Also
  `"screw"` (Titen HD style: thread ticks, hex washer head). `embed` is the effective embedment.
- `flashing` (new type): sheet-metal profile. `profile: "z" | "l" | "drip" | "weep_screed" | "points"`
  with `flange` / `leg` / `drop` lengths (defaults: Z 2"/1"/2", drip 1/2" kick, weep screed with
  3-1/2" nailing flange and 1/2" drip), `gauge` (default 26 ga, 0.0179"), placement by `at`. Draws
  as thin metal (§16 thin rule).
- `joint` (new type): `kind: "expansion" | "control" | "tooled_edge" | "sealant"`. Expansion = a
  filler strip `width` (default 1/2") × `depth` with its own hatch (`joint_filler`). Control = a saw
  cut `depth` (default 1/4 slab) drawn as a V notch. Tooled edge = a radius (default 1/4") on a
  concrete corner given by `at`. Sealant = a bead + backer rod circle.
- `slab_edge.base: { material: "gravel"|"sand"|"compacted_fill", thickness }` builds a uniform
  base course under the slab soffit and the haunch, stopping at the footing bottom. It is emitted as
  part `base` with its fill hatch, so authors no longer hand-draw three fill polygons.
- **Hardware table additions** (dimensions from the manufacturer's current catalog; the note
  carries the model): RSP4, A34, A35, LTP4, LSTHD8, STHD14, LUS26, MUS26, HUS26, HHUS26-2, ST2215,
  ST6224, FHA18, LTS12. Unknown models stay legal (they use explicit width/gauge).

**CLI**
- Op-log append failures are errors. `kerf apply -w` / `kerf new` exit 3 with a message if the
  `.log.jsonl` append fails (the document write is still atomic). Logs are opened with a normal write
  handle and seek-to-end, never append-only handles. That fixes AccessDenied on Windows.
- `kerf guide` and `kerf catalog --markdown` output is pure ASCII (PowerShell consoles).

## 19. v0.1.3 agent ergonomics (from the Claude Code headless eval, spec/evals/results/2026-10-06-claude-code-baseline.md)

**CLI**
- A failed `kerf apply` prints every error diagnostic to stderr as
  `ERROR <code> <path>: <message>  Fix: <fix>`, then `nothing written`, and exits 1. Warnings print
  after the summary. Add `kerf apply --dry-run` (validate and print the summary, write nothing; the
  same as omitting `-w`, but explicit).
- `kerf schema [doc|view|note|dim|label|cite|ops|<component type>]` prints the field reference
  for that object (required/optional, types, defaults, one example). `kerf guide` embeds
  `kerf schema view`, `kerf schema note|dim|label|cite`, `kerf schema ops`, and ONE complete minimal
  example document: two components, a section view with crop/scale/notes_side, two notes (one
  citation), one dim, one label.
- `kerf new <file> --template section` writes that minimal example (renamed) instead of an empty doc.
- `kerf call help` lists functions with their input shapes.

**Validation**
- `W_UNKNOWN_KEY`: any key not in the schema for that object warns, names the object path, and
  suggests the nearest valid key (edit distance + a synonym table: `citations|citation|cites → cite`,
  `side → (view) notes_side`, `point|target_point|arrow → at`, `kind → type`, `pos|position → place`).
  Unknown keys are still preserved in the document.
- `dim.dir` defaults to the dominant axis between `from` and `to` (|dx| ≥ |dy| ⇒ `h`, else `v`).
  `W_DIM_ZERO` fires when a dim measures under 1/16".
- `acknowledge: [{ "code": "W_UNTREATED_CONTACT", "reason": "truss seat moisture barrier by mfr." }]`
  on any component suppresses that warning for it. The summary counts it and lists it with the reason (§25.4), and
  `kerf apply` logs it. `lumber.barrier: "sill_seal" | "membrane"` draws a 1/8" sill-sealer strip under
  the member and also clears `W_UNTREATED_CONTACT`.
- `W_NOTE_STYLE` (lint): note text containing lowercase letters (before the case transform; the case-significant
  tokens of §25.3 excepted), a trailing period (not after a house abbreviation, §25.3), ` x ` as a dimension separator (use ` X `), `1-1/2"` style fractions (house style is
  `1 1/2"`), or a word on the abbreviation list spelled out (`GYPSUM BOARD → GYP. BD.`,
  `CONCRETE → CONC.`, `CONTINUOUS → CONT.`, `EACH → EA.`, `ON CENTER → O.C.`, `BOTTOM → BOTT.`,
  `REINFORCING → REINF.`, `MINIMUM → MIN.`, `DIAMETER → DIA.`, `PRESSURE TREATED → PT`). A dashed fraction is
  `<whole>-<num>/<den>` with the whole number starting a token and a 2..64 power-of-two denominator ending it, so
  `HU212-2/HUC212-2` is not one. The fix text never changes a model number (§25.3).

**Defaults that should just work**
- `anchor_bolt` placement anchor defaults to `top_of_concrete`. Its default `z` (and the default z
  of any in-plane member when the doc has a section view) is the first section view's `cut_z`, so
  it is cut, not hidden.
- `crop` is optional. When omitted it auto-fits the bounding box of all non-fill components plus
  6" (fills are clipped to that box). `scale` is optional: when omitted, the engine picks the largest
  standard scale (3", 1-1/2", 1", 3/4", 1/2", 3/8", 1/4") at which the view + notes + title fits the
  frame. `W_VIEW_FIT` fires only when the author set crop or scale explicitly.
- `notes_side` defaults to `"both"`.

**Coupled geometry**
- `slope` accepts a Ref-like string `"@<component>"`, e.g. `"@truss"` (inherits the truss pitch).
  `until` works on sloped panels and membranes along their own run direction, so sheathing can run to
  `truss@top_chord_end`.
- Truss named anchors gain `heel_outer` (outer face of the heel at the bearing) and
  `top_chord_bottom_at_bearing`.
- `array` is allowed on `anchor_bolt` and connectors (z spacing), so "@ 32" O.C." can be drawn in 3D/iso.

**Graphics**
- Lumber cut lengthwise (run x/y, cut by the section plane) gets the `KERF-GRAIN` pattern (a sparse
  wavy grain line, style material `wood` → `grain`), so stud walls in section don't read as voids.

## 20. v0.1.4 (from the alpha.4 eval, spec/evals/results/2026-10-06-claude-code-alpha4.md)

**Layout resolves its own collisions**
- Before emitting `W_LEADER_HIT`, the layout runs a bounded repair loop (deterministic, ≤ 8 passes):
  move the landing point inside the target's visible region; move the note to the other column; push the
  dimension line out by one dim spacing (`offset` += sign·0.25" paper); move a label away from the leader.
  `W_LEADER_HIT` remains only for what repair cannot fix.
- Notes take an optional `column: "left" | "right"` hint (the per-note version of the view's `notes_side`).
- A note with `place` in the left column right-aligns its text block to `place.x + width`, so text never
  runs into the drawing (fixes "placing by coordinate backfired").
- **Dimension stacking:** dims sharing an axis and side whose lines would overlap or whose text would
  overprint are stacked automatically at increments of 0.25" paper, the shortest nearest the object.
- **Text that doesn't fit:** when dim text doesn't fit between the extension lines, it moves outside with a
  short leader. It is never shrunk and never dropped.

**Drawing conventions**
- Rebar in `path` mode draws as a single continuous centerline polyline (bends as arcs) in the `rebar` pen,
  with no filled outline. `along_z` bars remain filled dots. This is the US structural convention: bars read
  as heavy lines that are distinct from concrete outlines and don't blacken small details.
- Hardware/connector minimum visibility: straps and connectors draw at least at the `steel` pen width.
  Face-on ties (`lay:"face"`) draw their outline in the `steel` pen with nail-hole dots at 1" pitch along
  the centerline, so H2.5A-type ties read at 1"=1'-0".

**Coupled geometry & validation**
- `W_SHORT_SLOPE`: a sloped panel or membrane resting on a sloped member (same slope within 0.5°, touching)
  whose upper end stops more than 1/2" short of the member's end, or of the crop boundary if that is nearer,
  warns with the fix `"until": "<member>@top_chord_end"` (or the right anchor).
- `acknowledge` accepts `I_*` codes as a no-op instead of erroring (since §25.4 such an entry is `W_ACK_UNUSED`: it
  suppresses nothing).

**Agent docs**
- `kerf guide` is at most ~12 KB: workflow, one-command-per-line recipes (write the ops file with your
  file-writing tool, then `kerf apply f ops.json -w --why "..."`; never `&&`, `;` chains or heredocs),
  the doc/view/annotation schema, one complete example, a roof-pitch recipe using `slope:"@truss"` +
  `until`, and the list of `kerf schema` topics. `kerf guide --full` keeps the long form (with catalog).
- `kerf schema` takes several topics (`kerf schema note dim cite`). `kerf new <file> --ops ops.json`
  creates and applies in one command.
- Catalog `lumber` notes the standard view for beam-in-wall details: an elevation along the wall
  (beam seen lengthwise, plates interrupted) unless the designer asks for the end-on section.
- **Render specifics (as implemented):** the `rebar` pen is 0.50 mm, and path bars go on DXF layer
  `S-DETL-REBR` (LWPOLYLINE with bulges). Edge-on straps draw at least 0.022" paper thick. Face-on
  ties draw on top of everything with linework beneath them knocked out, and nail dots at 1" pitch.

## 21. v0.1.5 (from the alpha.5 evals: Claude Code + pi/Qwen)

- **Lenient ops input:** `kerf apply` (CLI, API, serve) accepts an ops array, a single op object, or
  `{"ops":[...]}` (also `{"ops":[...],"why":"..."}`, which supplies `--why` when the flag is absent).
  Anything else ⇒ `E_PARAM` with the three accepted shapes in the message.
- **Requested-elements coverage:** documents may carry `meta.requested: ["cmu wall", "bond beam", "H2.5A", ...]`.
  The agent writes the designer's asks there on the first build. `kerf check` prints a `COVERAGE` block,
  one line per item, `ok <component ids>` or `MISSING`, matched against component ids, types, labels,
  `model`s and annotation text with the normalization and sources of §25.3. `W_REQUESTED_MISSING` warns for each missing
  item and names the closest candidate.
  The guide tells agents to fill `meta.requested` from the request and to finish only at full coverage.
- **Re-fit after edits:** when a view has an explicit `crop` and an edit leaves a non-fill component
  with more than 25% of its extent outside the crop (and the component was mostly inside before, or was
  just added), warn `W_CROP_STALE` naming the component and the crop that would contain it, and suggest
  removing `crop` to auto-fit. An existing member warns only when the edit moved or resized it; a crop
  that cuts a member on purpose stays quiet while at least 10% of the member is inside some view (§25.5). In
  `kerf check` (no edit history), it fires only when less than 10% of the component is inside every view's crop.
- **Thin-layer landing:** when a note targets a thin component (membrane, panel < 1/2", connector,
  path rebar), the landing point lies on that component's own geometry, never inside a neighbor. If the
  label point would fall inside another component's region, slide along the thin component to the nearest
  visible stretch.
- **Note text lint (extends `W_NOTE_STYLE`):** flags commentary or dangling text: notes over 130 chars,
  first-person or conversational words (`I `, `WE `, `NOTE:`, `PLEASE`, `SHOULD BE`, `?`), text ending in
  a connector word (`W/`, `AND`, `OR`, `TO`, `@`), and notes whose text repeats another note in the same view.

## 22. Material roles (v0.1.5 refactor)
Style materials may declare `"role"`: one of `generic, soil, wood, masonry, grout, steel, rebar, sheet_metal,
vapor_retarder, void`. The engine's special-case behavior keys off the role, not the material name. That
covers fills in iso, wood cut marks and grain, untreated-contact checks, thin-metal rendering and cover hosts.
Without `role`, it's derived from the name (the old predicates). `wood_treated` deliberately has no
`wood` role, so treated members don't raise `W_UNTREATED_CONTACT`. A material's `"pen"` must name an
engine pen (`cut profile beyond hidden hatch rebar steel membrane vapor anno dim break title frame`).

## 23. Reference images (`meta.references`)
A detail is usually redrawn from one to three snips of real drawing sets. They live in the workspace folder next to
the document and are listed in `meta.references` so `kerf serve` can show them beside the drawing:

```jsonc
"meta": {
  "references": [
    { "path": "refs/0123_exterior_wall_at_stem.png", "caption": "EXTERIOR WALL AT STEM, SET 0123 S-301" },
    { "path": "refs/0123_stem_plan.pdf" }
  ]
}
```
- `path` (required): the file, relative to the folder that holds the `.kerf.json`, with forward slashes. It must not be
  absolute, carry a drive letter or a URL scheme (`:`), contain a `..` segment or backslashes, and must end in `.png`,
  `.jpg`, `.jpeg`, `.webp`, `.gif`, `.svg` or `.pdf` (case-insensitive). `caption` (optional): a string. Canonical key
  order: `path`, `caption`; `references` sits after `requested` in `meta`.
- Shape errors are `E_PARAM` (not an array, an entry that is not an object, a missing or empty `path`, a non-string
  `caption`); other keys in an entry warn `W_UNKNOWN_KEY`.
- `W_REFERENCE_FILE` (warning, path `meta/references/<i>/path`, with a `fix`) when the path text breaks a rule above.
  `check`, `apply` and the wasm report these.
- The engine cannot see files. The host looks up every rule-abiding path and passes the ones it could not resolve as
  `check`'s `reference_files: { "<path as written>": "missing" | "outside" }` (any other value is `E_INPUT`); each listed
  path warns `W_REFERENCE_FILE`: `missing` = no regular file there, `outside` = it resolves outside the document's folder
  through a symbolic link. `kerf check <doc>` fills it, resolving against the document's folder (not the working
  directory). `apply` and the `kerf serve` library counts do not look files up (an added reference is checked by the next
  `kerf check`; the web UI shows a missing image in its reference panel).
- `kerf serve` serves a listed file with `GET /api/docs/:file/reference?path=<path>` (spec/SERVE.md).
## 24. Keyed notes (v0.2)
- **View option** `notes_style`: `leader` (text at the leader end), `keyed` (a number tag at the leader end, the texts in a
  DETAIL NOTES list) or `list` (the list only: no tags, no leaders, `target` not needed; typical-notes blocks). Without it the
  style's `notes.mode` decides (`keynote` is accepted as an old name of `keyed`). Leader is the default; keyed output never
  changes leader output.
- **Numbering** (deterministic): a note's integer `key` (1-999) is its number. The other notes take the lowest unused numbers
  in note order. Notes with the same `key` share one list entry (the first one's text; a later note may omit `text`, and a
  different text warns `W_PARAM`) and each draws its own tag. A bad `key` warns `W_PARAM` and is ignored.
- **Tags** route exactly like leader text (§6.3, §16, §18, §20: columns outside the crop, de-crossing, repair), with the tag as
  the text block: the number's width x one text height (`key_tag: "plain"`), or a circle / flat-top hexagon sized by the widest
  number (`circle`, `hex`; the leader starts on the outline). Tags go to the side nearer their landing (a note's `column` still
  forces a side).
- **List**: heading `notes.list_title` (default `DETAIL NOTES`) over a rule, then `<n>.` and the text wrapped at
  1.25 x `wrap_chars` with a hanging indent, in number order, line pitch as notes, layer notes, text src = the entry's first
  note (heading and rule src `notes_list`). Placement by `notes_side`: `above` (and `both`) = left-aligned with the drawing
  extents (crop, tags, dimensions, labels), its bottom one `gutter_in` above them; `left` / `right` = top-aligned with them, one
  `gutter_in` beside them. It splits into two columns (split where the taller column is shortest, ties: the longer first
  column, every column under its own rule) when it has more than 8 lines and, beside the drawing, is also taller than it.
- `notes_side: "above"` on a leader view warns `W_PARAM` and lays out as `both`. `W_NOTE_STYLE` lints keyed and list note text
  as it does leader text; repeated text under one key is not a repeat, and a repeat in a keyed view suggests sharing the key.
  `W_VIEW_FIT` names the list as a culprit and suggests moving it (`above` <-> `left`/`right`).

## 25. v0.2.0-alpha.5 correctness

### 25.1 alpha.5: cut fill and rebar placement
- **Polygon booleans with coincident edges** (§8.1). Edges of the two operands that run together within `SNAP` (1e-7 in),
  including edges off by rounding noise (a part rotated 90/180/270 degrees has coordinates like 5e-17 for 0), are split at
  each other's endpoints and classified as shared boundary; they no longer cross each other at a stray point. A proper
  crossing found by the exact predicates still splits both edges at one shared point.
- **Cut fill guard.** The cut fill of a non-embedded part is its region minus every embedded cut part. Each subtraction must
  remove no more area than the embedded part has (and never add area); otherwise that subtraction is skipped, the part keeps
  its fill, and `W_CUT_FILL` (warning, id = the part, path `views/<view>`) names the part and the embedded part, once per
  pair and view. An embedded part that covers a part's whole cut also warns `W_CUT_FILL` (the part draws no fill). A filled
  material never loses its fill silently.
- **`rebar.place` follows the host outline** (§5.5). The zone is the host's outline (concrete, cmu) in world coordinates,
  intersected with the part's box for `in: "comp.part"` (a host without an outline: its box). For face `bottom`/`top`/
  `left`/`right` the face is a run of consecutive collinear zone edges whose outward normal is within 60 degrees of the face
  direction, chosen furthest along that direction (ties: the longer run, then the first); e.g. `bottom` of an L slab with a
  thickened edge is the footing's bottom. Bars sit at `cover + d/2` inside that run, from `side_cover + d/2` past its start
  to the same before its end (count 1: the run's middle), ordered left to right (bottom/top) or bottom to top (left/right).
  A rectangle gives exactly the earlier box placement. `station` measures from the zone box's left (bottom) side as before;
  the bar sits `cover + d/2` (perpendicular) inside the outermost face edge crossing that station. `center` keeps the box
  rule. Every bar centre must be inside the zone, else `E_PLACE_OUTSIDE` (error, path `components/<id>/place`) with the
  position, the zone box and the fix; the bar is not drawn.
- **`W_BAR_OUTSIDE`** (warning, id = the bar). Checked with `W_COVER`, per view set (§9). An along-z bar whose centre is in
  no concrete/masonry host (XY, z ranges overlapping) warns; for a placed or arrayed bar one warning gives the count and the
  first centre. A path bar warns when a bend's middle is in no host, or when no point of it is in a host; a straight leg
  may leave the concrete (a dowel, a bar welded to steel). The message names the nearest host and its distance; the fix
  suggests `place` or acknowledging a bar that really stands outside the concrete. Silent when the view set draws no host.
- **Anchors on paths and polygons** (§3.1). Path rebar: `start`, `end` (centreline ends), `bend_k` (k = 1.. for interior
  point k+1: the point of the bar nearest that corner, the middle of the fillet) and `seg_k_mid` (middle of the straight part
  of leg k, from point k to k+1). Point lists of `concrete` polygon, `solid` `profile.points`, `fill`, `insulation` with
  `points`, `membrane` and `connector`: `vertex_k` (point k, 1-based, in the order given, repeated points dropped) and
  `edge_k_mid` (middle of the edge from point k to the next; the arc's middle for a bulge; closed shapes include the edge back
  to point 1). Leaders and dims aimed at these land on the bar or outline instead of a box anchor in empty space. A path
  point may be a Ref to another bar's anchor, e.g. a dowel lapping a wall bar: `{"ref": "wall_bar@start", "offset": [2, 24]}`.
- **CMU cells.** `cmu_wall` adds the part `cell` (between the face shells, full height) for `place` and the anchors
  `cell_left`, `cell_center`, `cell_right`, `cell_left_top`, `cell_right_top` (§5.3), so vertical path bars in a cell follow
  the wall width: `{"ref": "wall@cell_left", "offset": [1.5, -12]}`.

### 25.2 alpha.5: annotation layout
From the MASS dogfood run (issue #17): iso `at` points that did not project, title bubbles over the title block, labels
and dimensions moved without a word, crossing leaders on dense views.
- **Iso `at` points.** A note's or label's `at` (a Ref, `{ref, offset}` or `[x, y]`) is a model point; in an iso it is
  projected like every other point. Its z is the near face of the component it names: the largest drawn z1 of that
  component's prisms (instance and part of the Ref; cutaway applied) for `from` front_*, the smallest z0 for back_*. The
  `offset` of `{ref, offset}` (and a label's `offset`) applies in model space before the projection. A literal point
  (or `@origin`) on a note takes the note's `target` as its component; with none, z = `cut_z`.
- **`W_NOTE_LANDING`.** An explicit `at` that puts the arrowhead on empty paper warns: in a section, a landing more than
  half a text height outside the crop (inside the crop an arrow may point into an opening or the air above grade); in an
  iso, a landing on no visible face and more than half a text height from every drawn line. The message has the `at`
  value and the distance to the nearest line; the fix is to drop `at` (when the note has a `target`) or to point it at a
  Ref on the member.
- **Iso landing candidates.** An iso note's target gets the same alternative landing points as in a section (§16), so
  the search can move an arrow off a crowded spot; the first candidate is still the label point, so a clean layout is
  unchanged.
- **View extents and frame clearance.** Arc segments count in a view's extents (the title bubble reached r below its
  centre line and the sheet drew it over the title block). The area a view must fit is the frame interior above the
  title block less 1/8" (paper) on each side; the auto scale (§19), the NTS fit (§17) and `W_VIEW_FIT` all use it, and
  the sheet centres the view, so the DETAIL NOTES list and the bubble keep at least 1/8" from the frame and the title
  block. An explicit scale that cannot keep it warns `W_VIEW_FIT`. The NTS fit grows its factor to at least
  `geometry / (area - annotations)` per axis (annotations keep their paper size). `W_VIEW_FIT` names the annotation that
  pushes the title down (`the notes column (8 notes) and the view title under it`) and calls the title "the view
  title (bubble, title, scale)", not the title block.
- **No silent moves: `I_ANNOT_MOVED`** (info, printed by `kerf check`). A dimension drawn more than 1/8" (paper) from its
  `offset` (stacking, §20, plus repair pushes) reports where it is drawn, how far it moved (model and paper) and what it
  clears (the dimensions and labels it collides with at its `offset`, the note whose leader pushed it); the fix gives the
  `offset` that keeps it there, or `pin`. A label the repair moved more than 1/8" reports the distance and the note; in a
  section the fix gives the label `offset` that keeps it there.
- **`pin`** (dimension, label; default false): a pinned dimension is drawn exactly at its `offset` (no stacking, no
  repair push, its text does not slide); a pinned label never moves. What then overprints warns `W_LABEL_OVERLAP`, a
  leader through it `W_LEADER_HIT`.
- **Smaller repair moves.** A dimension text hit by a leader first slides along its line (steps of half a text height,
  both directions, nearest first, the text staying between the extension lines) so the line stays at its `offset`; only
  when no slide clears it is the dimension pushed out, in 0.25" (paper) steps to the first position that clears the
  leaders (at most 2" in total); no position clears, no push. Moved text and labels must also stay off every note text
  block and key tag. Labels move at most 6 text heights (was 12).
- **`W_LABEL_OVERLAP`** (warning): a label overprints another label or a dimension's text, a note's text block (or key
  tag) overprints a label or dimension text, or a pinned dimension's text overprints another dimension's. For a label
  the fix gives the smallest `offset` change (half text height steps, 8 directions) that clears it. At most 8 are
  listed per view, then a count.
- **Untangled columns.** After the §16 adjacent swaps, two notes of one column (neither placed) whose leaders still cross
  trade places when that lowers the number of crossing pairs in the column; the first improving pair in note order is
  taken, at most 2n + 4 rounds. A tall column pushes notes far from their landings, so non-neighbours cross: this is what
  fanned leaders across dense views. When hits remain, the §16 search runs from the untangled column and, if untangling
  changed it, also from the plain one; the better result wins (note-to-note hits, then all hits, then cost). A crossing
  that remains is a `W_LEADER_HIT` ("crosses the leader of note ..."). Keyed tags (§24) route through the same code, so
  keyed output changes only where it had crossings.

### 25.3 alpha.5: house style lint and coverage
From the MASS dogfood run (47 details, issue #18): the note lint forced spellings engineers do not use, the renderer
upper-cased tokens whose case is significant, and coverage matched the designer's words literally, so agents reworded
`meta.requested` or stuffed component labels. One table set (engine `house` module) now serves the lint, the case
transform and coverage.

- **Case-significant tokens.** A token is a run of letters, digits and apostrophes. It keeps its case in the lint and on
  the drawing when it is a penny size (digits then `d`: `16d`, `8d`), or, after an optional leading number (`48db`), one
  of the ACI / AISC / NDS symbols and units `db ld ldh ldc ldt lc lst f'c f'ci f'm f'g fy fyt fu fc fr fs Fy Fu Fb Fv Fc
  Fp Fnt Fnv ksi psi psf plf pcf kip kips ksf` (exact case; `’` reads as `'`), or a token of the style's
  `text.keep_case` (array of up to 500 single tokens, 1-60 characters). `W_NOTE_STYLE` flags lowercase letters only
  outside these tokens, and its fix upper-cases everything else. The style case transform (`text.case: "upper"`) does the
  same for note, label, dimension, weld-tail, title and title-block text. Lower-case words that are not on the list (`16d
  nails`) are still flagged.
- **Citations.** The note text and the citation code take the case transform; the words of `section` that hold a digit
  print as written (`OSHA 1926.755(a)(1)`, not `1926.755(A)(1)`); plain words of `section` (`Table`) take the transform.
- **House abbreviations.** A built-in table maps each abbreviation to its words (the first is canonical): EA. EACH,
  CONT. CONTINUOUS, TYP. TYPICAL, MIN. MAX. CLR. DIA. BOTT./BOT. CONC. REINF. FA. FACE, GA. SIM. EMBED. MFR. FTG. FDN. GRD.
  HDR. DBL. STL. GALV. VERT. HORIZ. GYP. BD. SHTG. PLY. STD. SQ. EQ. NOM. APPROX. REQD./REQ'D SPEC. DWG. EXIST. THK. ALT.
  ARCH. EXT. INT. EXP. FIN. FLR. ELEV./EL. STRUCT. ENGR. MECH. ELEC. CTR. JT. JST. BM. COL. CONN. OPNG. PERP. LG. ADDL.
  BLKG. SCHED. SECT. DET. DIM. OPP. INSUL. THRU BTWN. BRG. HT. PL. EOR O.C. E.F. E.W. T&B T.O. B.O. U.N.O./UNO PT CMU.
  The style's `notes.abbreviations` (object, up to 500 entries: abbreviation -> a string or an array of up to 8 strings)
  adds to it. A note may end on any listed abbreviation with its period (`FINISH PER ARCH.`); `NO.`, `NOS.` too. Any other
  trailing period still warns, and the message says to add the word to `notes.abbreviations` if it is an abbreviation.
  The forced spellings of §19 (`EACH -> EA.` ...) are unchanged.
- **Fix safety.** The mechanical fix (abbreviate, upper-case, `1-1/2"` -> `1 1/2"`, drop a trailing period) is computed
  on whole words (letters and digits are word characters), does not double a period (`EACH.` -> `EA.`), checks the
  trailing period after abbreviating, and is offered only if every model number of the text (a token with a letter and a
  digit, e.g. `HU212-2`, `H2.5A`) appears unchanged, upper-cased, in the fixed text; otherwise the fix says to edit by
  hand. `TYPE I`, `CLASS I` (and after GRADE, CATEGORY, ZONE, GROUP, PHASE, LEVEL, TIER, STAGE, OPTION, CASE, PART) is a
  numeral, not first-person commentary.
- **Coverage normalization.** Item and candidate texts become word lists: lower case, `’`/`”` folded; letters joined
  across `.` (`o.c.`), `&` (`t&b`) and `'` (`req'd`, `f'c`); a plural `s` dropped (not after `s`); stop words `w with and
  at of the a an to for in on per` dropped. Lengths become one word: `15'-0"`, `4'`, `2'-2`, `6'-0in`, `1 1/2"`, `5/8"`, `48in`,
  `15 ft`, `2.5 in.` (to the nearest 1/16"). On the item side a length with units counts only as the length (`15 ft` =
  `15'-0"` = `180"`); a bare proper fraction (`3/16`) counts as its digits, and a candidate offers both. Then every
  abbreviation and every spelling of its words is rewritten to the canonical words, longest match first, on both sides
  (`EA. FACE` = `each face` = `E.F.`; `CLR.` = `clear`). An abbreviation that is also a plain word (ALT. ARCH. COL. DIM. EXP.
  FIN. INT. SIM.) is rewritten only when written with its period, and a candidate then offers the written word as well:
  `alt` asks for `ALT`/`ALT.`, `alt.` also matches `ALTERNATE`.
- **Coverage sources.** A candidate is one component (id, type, label, model, size; it reports the id) or one
  annotation: note or label text plus its citations' code, section and title (it reports the target component, else
  `note <id>`/`label <id>`), a dim's `text` or, without text, the length it measures in a section view (`dim <id>`), a
  weld as `<size> fillet weld` (`cjp`/`pjp` complete/partial joint penetration groove weld), plus `all around`, `field
  weld`, `both sides` and its tail (`weld <id>`), and a view `title` (`view <id>`). An item is covered when one candidate
  offers all of its words. A keyed note with no text adds nothing (its key's first note carries the text).
- **Explaining a miss.** For an uncovered item the candidate with the most of its words (first in document order on a
  tie) is the closest. The `COVERAGE` line reads `MISSING  <item>  closest: <source> (lacks "w1", "w2")` and
  `W_REQUESTED_MISSING` adds `closest: <source> "<text>" (<n> of <m> words; lacks ...)`, or `no candidate shares any of
  its words`. Lengths print as feet-inches.
- Coverage is computed once per load, with the compiled scene (`Loaded.coverage`), and both the summary and the lint use it.

### 25.4 alpha.5: scenario membership and acknowledgements
From the MASS dogfood run (47 details, issue #8): every agent kept alternative scenarios in one document by listing the
other scenarios' ids in each view's `omit` and offsetting each scenario in model space; blanket acknowledgements made
before the per-view checks (§9) then hid real findings, and a clean check could mean "everything acknowledged".

**Membership.** Which components a view draws is decided by three optional lists:
- component `views: ["A", "C"]`: the component belongs to these views only (default: every view). Each id must name a
  view (`E_REF_UNKNOWN`, with the view list); `[]` or a non-string entry is `E_PARAM` (use `visible: false` to hide a
  component everywhere).
- view `only: ["shared_*", "a_*"]`: the view draws only components matching an entry. `[]` is `E_PARAM`.
- view `omit: ["b_*", "roofing"]`: components matching an entry are not drawn (as before, now with globs).

Entries of `only` and `omit` are component ids or globs where `*` matches any run of characters (also none); nothing
else is special. A view draws a component iff its `views` allows the view AND (`only` is absent OR an `only` entry
matches) AND no `omit` entry matches. An `only` / `omit` entry that matches no component id is `W_REF_UNKNOWN` (a
warning, so an omit can name a component a later op or a later `apply` adds); a non-array or a non-string entry is
`E_PARAM`. The check runs at document level, not per view build. Ops are validated at the end of the batch (§14), so the
order of ops inside one batch never mattered; what failed before was an `omit` naming a component added by a later
`apply`.

Excluded components are absent from the view: not drawn (no cut, no beyond outline, no hidden line), not in the auto crop,
not a note landing (a note targeting one is `W_NOTE_TARGET` "not drawn in this view (its "views" or the view's "omit" /
"only" exclude it)"), not reported by `inspect at`, not in that view's geometry checks (§9) and not a `W_CROP_STALE`
candidate for that view. So scenarios share one origin: anchors between shared and scenario members keep their meaning,
and a scenario in another z band no longer draws as beyond outlines in the other views. Recommended form (guide): shared
members get no `views`; each scenario's members get `views: ["B"]`; no `omit` lists, no model-space offsets. The 3D mesh
(§11) still contains every visible component.

**Acknowledged warnings stay visible.**
- Summary (§13): the DOC line always ends with the acknowledged count: `DOC x  12 components  3 views  0 errors
  1 warning  23 acknowledged`. The per-warning `I_ACK` lines are replaced by one block after the diagnostics,
  grouped by code and reason, sorted by code:
  ```
  ACKNOWLEDGED  23 warnings suppressed by `acknowledge` (kerf check --strict prints each in full)
   W_OVERLAP           x11  a_slab, a_beam, b_slab, +3 more: "scenario alternates"
  ```
  (component list: the first six, then `+N more`). The `I_ACK` diagnostics stay in `diagnostics`. The op log's `ack`
  array holds the block's lines.
- `check` and `apply` outputs carry `acknowledged`: one object per suppressed warning `{code, id, reason, path?, message,
  fix?}` (`id` = the acknowledging component; `message` / `fix` = the warning as it would have printed).
- `check` input `strict: true` (CLI `kerf check --strict`): acknowledged warnings are kept as warnings, their message
  ending ` [acknowledged on '<id>': <reason>]`; when any warning was acknowledged the DOC line adds ` (--strict: listed as
  warnings)` after the count; the ACKNOWLEDGED block is not printed. The CLI exits 1 when any error or warning remains (without `--strict`, on errors
  only). `--strict` is only accepted by `check`, `--review` only by `export` (usage error, exit 2).
- `W_ACK_UNUSED` (warning, never suppressible): after suppression, every `acknowledge` entry that matched no warning
  in any view. An entry counts as used when its component is one the warning names (its `id`, or quoted in a
  `W_OVERLAP` / `W_NEAR_MISS` / `W_UNTREATED_CONTACT` message) and its code matches; every component the warning names
  that acknowledges it is marked used, the first in document order gets the `I_ACK`. Within one component a second entry
  with the same code is "repeats an earlier entry"; an `I_*` code is "info, never suppressed". One diagnostic per
  component, path `components/<id>/acknowledge`, listing `CODE (#index)` per stale entry; the fix is the exact op
  `{"op":"update","path":"components/<id>","value":{"acknowledge":<the remaining entries or null>}}`. Emitted only when the
  diagnostics are complete: every view was built (`check`, `apply`) and the document has no errors.
- Drawing IR (§10): `acknowledged` lists the suppressed warnings whose component the view draws (document-level checks
  plus that view's own diagnostics); the key is present only when the document has `acknowledge` entries.
- `export` input `review: true` (CLI `kerf export ... --review`): a review overlay on the notes layer. Each acknowledged
  component the view draws gets a dashed (`hidden` pen) box 0.08 paper inch outside its drawn extent and an `ACK n` tag
  on the box's top edge (tags never overprint each other: a colliding tag moves right); a stamp `N ACKNOWLEDGED
  WARNING(S)` (title pen, 1.6 text heights; N counts warnings) heads a legend of one line per tag, `ACK n  <ID>: <CODE>
  X<count> <REASON>; ...` (upper case, at most 110 characters, at most 10 lines, the last then reading `+K MORE
  COMPONENTS: ...`). Without a sheet the stamp and legend sit 0.25 paper inch above everything and the bounds grow; on a
  sheet they go to the first free spot of the frame, scanning from the top left in 0.1 paper inch steps and keeping
  0.15 paper inch from every drawn item, at text height 1, 0.8 or 0.65 x the style's, dropping legend lines from the end
  until it fits (last resort: the frame's top left). The default export is unchanged; `review` with no acknowledged
  warning stamps `0 ACKNOWLEDGED WARNINGS`.

### 25.5 alpha.5: check and CLI papercuts (#19, from the MASS details dogfood run)
**Checks.**
- `W_NEAR_MISS` compares array instances one by one (instances of one component never pair) and treats a gap as closed
  when any other drawn prism (a subfloor panel, a hanger seat, another instance) overlaps the gap rectangle in plan and z.
  Members side by side across x whose long faces see the gap (both narrower along x than across: studs @ 16" o.c. next
  to a jack stud) leave framing spaces, so a gap of 1" or more between them is not a near miss; a smaller one warns
  "`a` and `b` run parallel with a <gap> gap along x ... between their long faces" with a move fix. Stacked members (a gap
  along y) bear on each other and warn at any gap of 1/32"–3" with the move fix (a length change would not close it).
- `W_FLOATING`: concrete and masonry pieces (role masonry or grout) facing each other across less than 1" (a construction,
  control or keyed joint) touch. A component with `array` whose instances partly touch nothing names those instances
  (`id#k`, with their z range) and the fix (an array starts at instance 0; for n instances centered on c, z = c − (n−1)·spacing/2).
- `W_CROP_STALE` (§21): an apply warns for an existing member only when it moved or resized the member out of a crop it
  sat in; a member the crop cuts on purpose (break lines) stays quiet while ≥ 10% of it is inside a view. New members and
  `kerf check` warn when < 10% is inside every view's crop.
- Keyed notes (§24): a note with a shared key and no `text` takes the key note's text **and**, unless it has its own,
  its `cite`. A conflict names the differing field: `... shares key k and its text with note 'n1' but its "cite" differs (it
  reads "...", 'n1' reads "...")` (path `.../cite`), or `... but its "text" differs ("A" vs "B")` (path `.../text`).
- `W_COVER` ends with `(cover.<face>)`, `(cover.<face>, default)` or `(cover.parts.<part>.<face>)`. Its fix gives the
  measured clear cover (down to 1/16") and the full `update` op that sets that face on the override that applies (the part
  zone holding the bar, else the host), keeping every other value. A host using the default cover whose depth cannot hold
  bottom + top + a bar says so and suggests its own cover (slab on grade with mid-depth bars: `{"bottom":1.5,"top":1.5}`).
  The default stays `{bottom:3, sides:3, top:1.5}` for every concrete shape; the catalog says to give slabs their own.
- `anchor_bolt`: `W_BOLT_EDGE` when the straight shaft, over the deeper 60% of its length inside concrete/masonry (so a
  grout bed or topping at the top does not count), is less than 1 1/2" clear of the host edge, measured perpendicular to
  the shaft in the drawing plane (ACI 318-19 17.9.2, cover per 20.5.1.3); one warning per component, at the worst point.
  `W_BOLT_SPACING` when two anchor shafts (other components or instances) are closer than 4d center to center (6d when
  either is `wedge` or `screw`), combining the in-plane distance between shafts and the z distance; one warning per pair.
  Both acknowledge like other warnings (a product ESR may allow less).
- `W_VIEW_FIT` on a section view with an auto crop adds what sets the crop: when the components split into groups along x
  or y (over 2'-0" of empty space), the ones outside the main group (the group the view's annotations name most, then the
  largest) with the first fix `{"op":"update","path":"views/<id>","value":{"omit":[<current omit>, <them>]}}`; else the
  components at each crop edge.
- `weld` takes `length` and `pitch` (positive lengths; `pitch` needs `length` and ≥ it, else `E_PARAM`), printed right of
  the symbol as `L` or `L-P` in inches (AWS A2.4). `members` that is not two component ids, names an unknown id, or names
  a non-steel component warns `W_PARAM` (path `.../members`).

**Drawing and sheet.**
- Title block values: one line at 1/8", shrunk to the cell width; when that would go below 3/4 of the height, two lines
  split at the space that balances them (each at most (title block height − 0.22 − 0.8·label height)/2.5 high) if larger.
  Never wider than the cell.
- Note and list wrapping keeps one value together: a whole number and its fraction (`1 1/2"`), `@` and the spacing after
  it, and a dimension (a word with a digit ending in `"` or `'`) and its qualifier (`MIN. MAX. O.C. TYP. CLR. EA. EMBED.
  LAP`, case-insensitive); a joined piece longer than a line breaks at its spaces.
- Labels: `align` and newlines (§16).
- A dimension whose line is shorter than 2 tick lengths (paper) draws dot terminators (radius 0.02" paper, at most 0.3 x
  the line) on a dimension line run 2 tick lengths past both extension lines, instead of overlapping ticks.
- Text over hatch: hatch lines are already knocked out under note, dimension and label text (§18); no separate mask.

**Docs and CLI.**
- Lumber sizes: §5.1 (3x and 8x listed; 8x is 7 1/2" thick). `run:"z"` lumber takes `length` (§5.1, like steel section).
- `kerf schema view` lists `3/16"=1'-0"` and `1/8"=1'-0"` (any `<paper>"=1'-0"` parses); auto scale picks from 3"…1/4".
- `cut_z`: members wholly in front of it (z0 ≥ cut_z) are not drawn, behind it are beyond outlines; a section used as an
  elevation needs `cut_z` in front of every member it shows.
- `array`: a numeric `z` places instance 0; it does not center the group.
- `kerf version` adds `"build"`: `git describe --tags --always --dirty` of the source tree at build time (`KERF_BUILD`
  overrides; `unknown` outside a checkout). The core API `version` is unchanged (exports never carry it).
