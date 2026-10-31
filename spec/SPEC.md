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
- `spec/styles/kerf-standard.kerfstyle.json` — the default style; `example-company.kerfstyle.json` — a company style
  (§25.6); `ABBREVIATIONS.md` — the built-in abbreviation list (generated, §25.3).
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
  "scale": "3/4\"=1'-0\"",                // optional: default scale of views without their own (§26.2)
  "components": [ /* §4 */ ],
  "views": [ /* §6 */ ],
  "layouts": [ /* §26.2: several views on one sheet */ ]
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
- `anchor` defaults to `"bottom_left"`; `offset` defaults to `[0, 0]`. A shape built from a point list puts its
  local origin on `to` when `anchor` is omitted, and its named anchor when one is written (§28.1).
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
otherwise absolute) or a Ref / Ref-with-offset (always absolute). With `at.anchor` the literal points are local to the
shape and its anchor lands on `at.to`; Refs then are `E_PARAM` (§28.1). Optional third element on a
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
  "shown": "solid",            // solid | hidden | where_occurs (dashed graphics, §16, §26.3)
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
| `from`, `to`, `align` | null | two-point member: runs between two points, ends trimmed to the members they meet (§27.3) |
| `cuts` | null | run x/y: end cuts, birdmouth seat, notches (§27.3; `kerf schema cuts`) |
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
| `voids` | null | what the host loses: component ids (`"col"`, `"plate.part"`) and shapes (`rect`, `circle`, `points`, optional `z`); also on `cmu_wall`; a `sleeve` cuts the hosts in its `through` (§26.4) |
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
In-plane member at a given `z`. `embedded: true`. Base plate rods (`plate`, `leveling_nut`, `plate_washer`), adhesive
anchors and `projection: 0`: §27.2.

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
Hangers, post caps, post bases, holdowns (`kind`), straps flush on a member face (`on`) and model families: §27.2.

### 5.8 `truss` — prefab wood truss heel (side view)
Profiles (scissor, mono, parallel chord), bearings (top chord, face) and end-on trusses: §27.1.
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
A fill draws under every solid a view cuts with it: its region (hatch, outline, picking) is clipped by those solids, and
fill vs solid is exempt from `W_OVERLAP`, so earth around a footing is a plain rectangle; `except: [ids]` names solids
that do not clip it (§26.4).

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
`kind` `weld_stud` / `headed_stud`: §27.2. Natural z thickness: d (side), grip (end). Any `rotate` (180 draws the head below a flange, the grip up) keeps the cut fill of
the plies the bolt passes through (§25.1).

## 6. Views and annotations

```jsonc
{
  "id": "A",
  "kind": "section",                        // section | plan | elevation | iso | schedule (§26.3, §26.1)
  "number": "1",                            // detail number in the bubble
  "title": "TRUSS BEARING AT CMU WALL",
  "scale": "1-1/2\"=1'-0\"",                // see §6.1
  "cut_z": 6,                               // section: cut plane z; look toward −Z
  "cut_y": 40,                              // plan only: height of the horizontal cut plane (§26.3)
  "hidden_lines": false,                    // plan, elevation: hidden parts of outlines drawn dashed (§26.3)
  "crop": { "x": [-30, 40], "y": [-12, 48] },  // model-space window; break lines where cut solids are clipped
  "from": "front_right",                    // iso only: front_right | front_left | back_right | back_left
  "cutaway": true,                          // iso only: clip solids to z ≤ cut_z, hatch the cut face
  "notes_side": "right",                    // right | left | both | above (keyed: where the list goes, §24)
  "notes_style": "leader",                  // leader | keyed | list (§24); default: style notes.mode
  "notes_title": "DETAIL NOTES",            // heading of the notes list (§26.1); default: style notes.list_title
  "omit": ["roofing", "b_*"],               // components not drawn in this view: ids or `*` globs (§25.4)
  "only": ["shared_*", "a_*"],              // optional: the view draws only matching components (§25.4)
  "annotations": [ … ]
}
```
Plan and elevation views follow every rule of this section for section views (scales, crop and auto crop, note layout,
dimensions, title); a plan's `crop`, `at`, `place` and literal points are in plan coordinates (x, −z), §26.3.
A view draws a component when the component's `views` (if set) lists the view, the view's `only` (if set) matches its
id, and no `omit` entry matches it (§25.4). Everything a view does (section and iso drawing, beyond outlines, auto crop,
note landings, `inspect at`, the geometry checks of §9, `W_CROP_STALE`) uses that set.

### 6.1 Scales
Accepted: `3"=1'-0"` (factor 4), `1-1/2"=1'-0"` (8), `1"=1'-0"` (12), `3/4"=1'-0"` (16),
`1/2"=1'-0"` (24), `3/8"=1'-0"` (32), `1/4"=1'-0"` (48), and `"1:N"`. Also `"NTS"`
(iso views; engine fits to frame; schedule views, which have no model scale, §26.1). Paper size of anything = model size / factor.

### 6.2 Annotation types

**note** — leader note (or keyed note, §24):
```jsonc
{ "id": "n1", "type": "note",
  "text": "2X8 P.T. SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.",
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

**table** — a schedule: ruled grid of cells in paper inches (§26.1).

**ref** — reference bubble (detail number over sheet number), optional leader and text; placed exactly (§26.2):
```jsonc
{ "id": "r1", "type": "ref", "detail": "3", "sheet": "S-50.01", "at": "beam@top_right", "place": null, "text": "SIM." }
```

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
does not fit between the extension lines it moves outside to the right/top, beside the dimension line extended past the
end, never on the line (§28.3). Collinear dimensions sharing end points chain on one line (§28.3). Colliding dimensions stack outward and
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
             "keep_case": [],                     // extra tokens printed as written under "upper" (§25.3)
             "plan_title": "PLAN - {title}", "elevation_title": "{title}" },   // plan/elevation view titles (§26.3)
  "notes": { "mode": "leader", "wrap_chars": 28, "gutter_in": 0.375, "shoulder_in": 0.125, "note_gap_in": 0.0625, "arrow": "closed_filled", "arrow_len_in": 0.09375,
             "key_tag": "plain", "list_title": "DETAIL NOTES",    // mode leader | keyed | list; key_tag plain | circle | hex
             "abbreviations": { "extends": "us-structural", "rules": {...}, "approved": {"BRG.": "BEARING"},
                               "variants": {"BTM.": "BOT."}, "remove": [], "always": [] } },  // company abbreviation rules (§25.6)
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
3. **Resolve placement** DAG; apply translate / rotate / mirror; expand arrays. Then subtract voids and sleeves
   from their hosts (§26.4): hosts are split into z slabs, each slab minus the voids that span it.
4. **Validate** (§9) → diagnostics.
5. **Views** → Drawing IR (§10): section (§8.1), plan or elevation (§26.3: both reduce to §8.1) or iso (§8.2), then
   annotations, title, crop, break lines.
6. **Mesh** (§11) for the 3D viewer.
7. **Export** Drawing → SVG / DXF / PDF (§12).

### 8.1 Section view (exact, 2D)
Look direction −Z, cut plane z = `cut_z`. Plan and elevation views are drawn by this same algorithm (§26.3): an
elevation classes every prism as beyond, a plan first turns the prisms into plan prisms with model y as their depth.
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
- Fills (role soil) that are cut are first clipped by the solids the plane cuts with them (§26.4).
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
| `W_OVERLAP` | warning | two cut regions overlap by > 0.01 in² (report overlap bbox in ft-in). Exempt: `embedded` items, membranes, connectors, and fills vs any non-fill solid (§26.4). A member inside concrete/cmu gets the `voids` (or sleeve `through`) op as its fix |
| `W_BIRDMOUTH_DEPTH` | warning | a birdmouth seat cuts more than 1/3 of the member depth (§27.3) |
| `W_CUT_MISS` | warning | a lumber/steel cut that misses: a seat off its bearing member, a trim that meets nothing, a notch `by` that removes nothing (§27.3) |
| `W_VOID_MISS` | warning | a `voids` entry or a sleeve `through` host that removes nothing from its host; the fix drops it (§26.4) |
| `W_FLOATING` | warning | a component touches nothing (gap > 1/32" to every other; concrete/masonry pieces facing across a joint < 1" touch), or some instances of an `array` touch nothing while others do (§25.5) |
| `E_PLACE_OUTSIDE` | error | a `rebar.place` bar centre falls outside its zone (cover deeper than the host, or a `station` past the face); the bar is not drawn (§25.1) |
| `W_BAR_OUTSIDE` | warning | an along-z bar centred in no concrete/masonry host drawn with it, or a path bar with a bend outside every host or no point in one; names the nearest host (§25.1) |
| `W_CUT_FILL` | warning | an embedded part emptied a cut member's fill, or a boolean fault was caught and the fill kept (§25.1) |
| `W_COVER` | warning | rebar clear cover < required. Host = the concrete/cmu prism containing the bar center. Each host boundary edge is classified by its outward normal: n.y < −0.5 ⇒ `bottom`, n.y > 0.5 ⇒ `top`, else `sides`. Required cover comes from the host's `cover`, with `cover.parts.<part>` overriding for bars inside that part zone. cmu_wall default cover `{ "sides": 1.5, "top": 1.5, "bottom": 0.5 }`. The message names the knob (`cover.<face>`, `cover.parts.<part>.<face>`, `, default` when unset) and the fix carries the exact `update` op (§25.5) |
| `W_UNTREATED_CONTACT` | warning | wood (`treated:false`) touching concrete/grout/cmu (IRC R317.1) |
| `W_NOTE_TARGET` | warning | note target not visible in its view (plan and elevation: the fix names `cut_y` / `hidden_lines`, §26.3) |
| `W_SHOWN_DASHED` | warning | a component uses the old `"shown": "dashed"` (drawn as `where_occurs`); the fix gives the `where_occurs` and the `hidden` update ops (§26.3) |
| `W_VIEW_FIT` | warning | view + notes + title (bubble included) exceed the sheet frame, less 1/8" clearance, at the chosen scale; with an auto crop it names the components that stretch it (§25.5) |
| `W_NOTE_LANDING` | warning | a note's explicit `at` puts the arrow on empty paper: outside the crop (section), off every visible face and line (iso) |
| `W_LABEL_OVERLAP` | warning | a label overprints a label or dimension text, a note's text overprints either, or a pinned dimension overprints another |
| `I_ANNOT_MOVED` | info | the layout drew a dimension more than 1/8" (paper) from its `offset`, or moved a label that far (printed by `kerf check`) |
| `I_KEY_RENUMBERED` | info | a layout with shared notes prints a keyed note under another number than its `key` so the list has no gaps (§26.2) |
| `I_UNVERIFIED_CITE` | info | citations awaiting designer verification (count) |
| `W_BOLT_EDGE` | warning | a bolt's edge distance to a `steel` ply it passes through (in the drawing plane, or along z to the ply's z extent) is below AISC 360-16 Table J3.4 (1/2" 3/4, 5/8" 7/8, 3/4" 1, 7/8" 1 1/8, 1" 1 1/4, 1 1/8" 1 1/2, 1 1/4" 1 5/8, larger 1.25d); one warning per bolt group and ply, the worst bolt |
| `W_BOLT_SPACING` | warning | bolt spacing < 2 2/3 d (AISC 360 J3.3); two anchor bolts closer than 4d (cast-in) or 6d (wedge/screw) center to center, in plane and z (ACI 318-19 17.9.2) |
| `W_BOLT_EDGE` (anchor bolt) | warning | an `anchor_bolt` shaft (its deeper 60% inside concrete/masonry) < 1 1/2" clear of the host edge (ACI 318-19 17.9.2, cover per 20.5.1.3) |
| `W_WELD_SIZE` | warning | a fillet weld symbol with `members` is larger than the thinner part allows (t, or t − 1/16 when t ≥ 1/4; AISC 360 J2.2b) or smaller than Table J2.4 (1/8, 3/16, 1/4, 5/16 by thinner part ≤ 1/4, 1/2, 3/4, over). Thickness of a W or channel is its thinner element, of an HSS or pipe the design wall |
| `W_BEARING` | warning | an `ijoist` laid along x bears less than 1 3/4" on what is under one of its ends, or less than 3 1/2" on an intermediate support; the fix gives the length to add and the x to reach (§27.1) |
| `W_UNKNOWN_MODEL` | warning | a connector `model` in neither the hardware table nor a family, without `gauge` (and `width` for a strap); names the nearest model (§27.2) |
| `W_HANGER_SIZE` | warning | a hanger too short, too tall, too narrow or wide, or a model sized for another member (§27.2) |
| `W_HOLDOWN_ROD` | warning | a holdown without a rod, a rod that ends below its seat and nut, or a rod anchored in no concrete, masonry or steel (§27.2) |
| `W_POST_BASE` | warning | a post base whose plate bears on nothing; the fix is the `standoff` that makes it bear (§27.2) |
| `I_SOLID_USED` | info | `solid` escape hatch used |
| `W_REF_UNKNOWN` | warning | a view `omit` / `only` entry (id or `*` glob) that matches no component; it has no effect (§25.4) |
| `W_ABBREV` | warning | an abbreviation in a note, label, dim text, weld tail or view title not written in the company style's approved form (`TYP` -> `TYP.`, `OC` -> `O.C.`, `C.M.U.` -> `CMU`, `BOTT.` -> `BOT.`), or a spelled-out word its `always` list abbreviates; one per token, the fix is the `update` op that rewrites only the flagged tokens (§25.6) |
| `W_ACK_UNUSED` | warning | `acknowledge` entries of a component (or a view or annotation, §25.6) that suppressed nothing in any view (stale, repeated, or an `I_*` code); one per owner, the fix is the `update` op that removes them. Only when every view was built and the document has no errors (§25.4) |
| `I_ACK` | info | a warning suppressed by `acknowledge` (code, component, reason, the original message); `diagnostics` only, the summary groups them (§25.4) |

**Per-view geometry checks.** `W_OVERLAP`, `W_FLOATING`, `W_UNTREATED_CONTACT`, `W_COVER`, `W_BAR_OUTSIDE`,
`W_NEAR_MISS`, `W_SHORT_SLOPE`, `W_BEARING`, and the checks between components of §28.1 (`W_BOLT_EDGE`, anchor
`W_BOLT_SPACING`, `W_POST_BASE`, the anchorage of `W_HOLDOWN_ROD`) run over the components each view draws: compiled, `visible`, and members of that view (component
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
| `drawing` | `{ doc, style, view }` or `{ doc, style, layout }` | Drawing IR (a layout: §26.2) |
| `mesh` | `{ doc, style }` | Mesh |
| `export` | `{ doc, style, view (or layout), format: "svg"|"dxf"|"pdf", sheet?: bool, review?: bool }` | **raw bytes** (`review`: §25.4; `layout`: §26.2) |

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
kerf drawing <doc> --view A|--layout S1 [--style S] [-o out.json]
kerf export <doc> --view A|--layout S1 --format svg|dxf|pdf [--style S] [--sheet] [--review] -o <file>
kerf mesh <doc> [-o mesh.json]
kerf call <fn> < input.json         # raw API access, used by conformance tests
```
Default style: the embedded copy of `spec/styles/kerf-standard.kerfstyle.json` (compiled in), merged with the company
style: `--style S`, else the nearest `kerf.kerfstyle.json` in the document's folder or an ancestor (§25.6).

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
- **"Where occurs" graphics:** any component may set `"shown": "where_occurs"`. It draws all its visible edges
  in the `hidden` pen (no hatch, no cut mark), is exempt from `W_FLOATING` / `W_NEAR_MISS` / `W_OVERLAP`,
  and its notes get the suffix ` (WHERE OCCURS)` unless the text already says so. `"shown": "hidden"` draws the same
  way without the suffix (a member hidden behind or below others); the old `"dashed"` is `where_occurs` and warns
  `W_SHOWN_DASHED` (§26.3).

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
  on any component suppresses that warning for it (on a view or an annotation, a warning about that view or annotation,
  §25.6). The summary counts it and lists it with the reason (§25.4), and
  `kerf apply` logs it. `lumber.barrier: "sill_seal" | "membrane"` draws a 1/8" sill-sealer strip under
  the member and also clears `W_UNTREATED_CONTACT`.
- `W_NOTE_STYLE` (lint): note text containing lowercase letters (before the case transform; the case-significant
  tokens of §25.3 excepted), a trailing period (not after a house abbreviation, §25.3), ` x ` as a dimension separator (use ` X `), or `1-1/2"` style fractions (house style is
  `1 1/2"`). Abbreviation forms (`TYP` for `TYP.`, `BOTT.` for `BOT.`) and spelled-out words the company style always
  abbreviates are `W_ABBREV` (§25.6), not `W_NOTE_STYLE`; no word is forced by default. A dashed fraction is
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
  DETAIL NOTES list; a note with neither `target` nor `at` is listed with no tag, §26.1) or `list` (the list only: no tags,
  no leaders, `target` not needed; typical-notes blocks). Without it the
  style's `notes.mode` decides (`keynote` is accepted as an old name of `keyed`). Leader is the default; keyed output never
  changes leader output.
- **Numbering** (deterministic): a note's integer `key` (1-999) is its number. The other notes take the lowest unused numbers
  in note order. Notes with the same `key` share one list entry (the first one's text; a later note may omit `text` and `cite`, also in another view, §28.2; a
  different text warns `W_PARAM`) and each draws its own tag. A bad `key` warns `W_PARAM` and is ignored.
- **Tags** route exactly like leader text (§6.3, §16, §18, §20: columns outside the crop, de-crossing, repair), with the tag as
  the text block: the number's width x one text height (`key_tag: "plain"`), or a circle / flat-top hexagon sized by the widest
  number (`circle`, `hex`; the leader starts on the outline). Tags go to the side nearer their landing (a note's `column` still
  forces a side).
- **List**: heading `notes.list_title` (default `DETAIL NOTES`; the view's `notes_title` replaces it, §26.1) over a rule, then `<n>.` and the text wrapped at
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
- **House abbreviations.** The built-in list `us-structural` (engine `house::ABBREVIATIONS`, 140 approved forms, and
  `house::VARIANTS`, 17 other spellings in use with the approved form each maps to) maps each abbreviation to its words
  (the first is canonical). It is US structural practice as written in the MASS reference set (32 US Southwest drawing
  sets), with the forms the MASS principal engineer chose (§25.6): periods on shortened words (`TYP.`, `CONT.`,
  `REINF.`), contractions with the apostrophe (`REQ'D`, `ADD'L`), dotted initials for phrases (`O.C.`, `U.N.O.`, `E.N.`,
  `T.O.F.`, `A.B.`, `N.T.S.`, `E.O.R.`, `P.T.`), no periods on material and product acronyms (`CMU`, `LVL`, `PSL`, `HSS`,
  `OSB`, `GLB`, `PL`). The full list with meanings, counts and the spellings each approved form replaces is
  `spec/styles/ABBREVIATIONS.md` (generated from the table; `kerf schema abbreviations` prints the list in force). The
  style's `notes.abbreviations` changes it (§25.6). A note may end on any listed spelling with its period (`FINISH PER
  ARCH.`); `NO.`, `NOS.` too. Any other trailing period still warns, and the message says to add the word to
  `notes.abbreviations.approved` if it is an abbreviation.
- **Spelled out or abbreviated.** In the reference sets most words appear spelled out about as often as abbreviated
  (CONCRETE 228 : CONC. 61, BOTTOM 103 : BOT. 11, EACH 132 : EA. 122), so both are house style and nothing is forced by
  default (alpha.5 forced `O.C.`, `MIN.`, `MAX.`, `DIA.`, `GYP. BD.`; that is now the company style's `always` list,
  empty in kerf-standard). An abbreviation, when used, must be the approved form (`W_ABBREV`, §25.6). Consistency within a
  sheet is the author's call.
- **Fix safety.** The mechanical fix of `W_NOTE_STYLE` (upper-case, `1-1/2"` -> `1 1/2"`, drop a trailing period) is
  computed on whole words (letters and digits are word characters) and is offered only if every model number of the text (a token with a letter and a
  digit, e.g. `HU212-2`, `H2.5A`) appears unchanged, upper-cased, in the fixed text; otherwise the fix says to edit by
  hand. `TYPE I`, `CLASS I` (and after GRADE, CATEGORY, ZONE, GROUP, PHASE, LEVEL, TIER, STAGE, OPTION, CASE, PART) is a
  numeral, not first-person commentary.
- **Coverage normalization.** Item and candidate texts become word lists: lower case, `’`/`”` folded; letters joined
  across `.` (`o.c.`), `&` (`t&b`) and `'` (`req'd`, `f'c`); a plural `s` dropped (not after `s`); stop words `w with and
  at of the a an to for in on per` dropped. Lengths become one word: `15'-0"`, `4'`, `2'-2`, `6'-0in`, `1 1/2"`, `5/8"`, `48in`,
  `15 ft`, `2.5 in.` (to the nearest 1/16"). On the item side a length with units counts only as the length (`15 ft` =
  `15'-0"` = `180"`); a bare proper fraction (`3/16`) counts as its digits, and a candidate offers both. Then every
  abbreviation and every spelling of its words is rewritten to the canonical words, longest match first, on both sides
  (`EA. FACE` = `each face` = `E.F.`; `CLR.` = `clear`). Every variant reads as the words of its approved form (`BOTT.` = `bottom`), and a dotted-initials
  form also reads without its periods (`oc`, `uno`, `eor`). An abbreviation that is also a plain word (ALT. ARCH. COL. DIM.
  EXP. FIN. INT. SIM. SPA. STAG. STIFF. EMBED. EXIST. SECT. T.O. I.D. S.A.D. T.O.W. T.O.M., the variant PLY.) is rewritten
  only when written with its period(s), and a candidate then offers the written word as well:
  `alt` asks for `ALT`/`ALT.`, `alt.` also matches `ALTERNATE`.
- **Coverage sources.** A candidate is one component (id, type, label, model, size, and a `nailing` its text form, §27.4; it reports the id) or one
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
  X<count> <REASON>; ...` (upper case, at most 110 characters, at most 10 lines, the last then reading `+K MORE: ...`). Without a sheet the stamp and legend sit 0.25 paper inch above everything and the bounds grow; on a
  sheet they go to the first free spot of the frame, scanning from the top left in 0.1 paper inch steps and keeping
  0.15 paper inch from every drawn item, at text height 1, 0.8 or 0.65 x the style's, dropping legend lines from the end
  until it fits (last resort: the frame's top left). Annotations and views that acknowledge a warning are marked the
  same way (§25.6). The default export is unchanged; `review` with no acknowledged
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
  elevation needs `cut_z` in front of every member it shows (since beta.1: use `"kind": "elevation"`, §26.3).
- `array`: a numeric `z` places instance 0; it does not center the group.
- `kerf version` adds `"build"`: `git describe --tags --always --dirty` of the source tree at build time (`KERF_BUILD`
  overrides; `unknown` outside a checkout). The core API `version` is unchanged (exports never carry it).

### 25.6 company abbreviation rules and annotation acknowledgements
From the MASS principal engineer's review of the alpha.5 house list: spelling out or abbreviating is the author's choice,
but an abbreviation is written one way; the rules and lists belong to the company, and every rule can be overridden
visibly.

**W_ABBREV** (warning, lint, one per offending token). It reads the text of every note, label, dim (`text`), weld (`tail`)
and view `title`. A token is a run of letters, digits, `.`, `'` (`’`), `&` and `-`, with leading `. ' - &` and trailing
`' - &` stripped. Never read: a token with a digit (`HU212-2`, `16d`, `2X6`, `H2.5A`, `#4`), a case-significant token
(§25.3: `f'c`, `db`, `psi`), text inside double quotes (`"..."`, `“...”`; a `"` or `”` right after a digit is an inch
mark). A token with a `-` that is not itself a listed spelling (`DF-L`) is read part by part (`PT-DF`). Tokens compare
case-insensitively; their "letters" are the token without periods. Against the style's list (below) a token is:
- an approved form as written: fine;
- a listed variant as written (`BOTT.`, `REQD`, `PLY.`): flagged with that variant's rule;
- else, when its letters are those of a listed spelling (approved forms preferred), a deviation from that entry. An entry
  whose bare letters are an ordinary word (`dot`: ALT. ARCH. COL. DIM. EXP. FIN. INT. SIM. SPA. STAG. STIFF. EMBED. EXIST.
  SECT. T.O. I.D. S.A.D. T.O.W. T.O.M., the variant PLY.) counts only when the token has a period, and for dotted initials
  a period inside it (`T.O` yes, `TO.` and `TO` no): the ordinary word is never flagged.
The rule a deviation breaks, from the approved form it should be: a shortened word (`TYP.`) → `period_on_shortened`
(`TYP`); dotted initials (`O.C.`) → `dotted_initials` (`OC`, `O.C`, `UNO`, `NTS`, `EOR`, `PT`); no periods (`CMU`, `PL`,
`THRU`) → `bare_acronyms` (`C.M.U.`, `PL.`); a spelling with other letters (`BOTT.` → `BOT.`, `REQD` → `REQ'D`,
`ADDL.` → `ADD'L`, `BLK'G` → `BLKG.`, `EXIST'G` → `EXIST.`, `MFR'S` → `MFR.`, `PLY.` → `PLYWD.`, `ELEV.` → `EL.`, `GRD.` →
`GR.`, `STRUC.` → `STRUCT.`) or a deviation from a contraction → `variants`. A fifth rule, `always`: a spelled-out
spelling of the words of an approved form the style lists in `always` (whole words, case-insensitive, outside quotes,
longest first; a following period is absorbed when the abbreviation ends with one) asks for the abbreviation.
Message: `<note 'n1' in view A | label ... | dim ... | weld ... | the title of view A> (<path>): "<token>" breaks rule
<rule> (<what the rule says>): write "<approved>" (<meaning>)<extra>. Text: "<text>"` (`always`: `"<words>" is spelled out,
and the company style always abbreviates it (rule always): write "<approved>"`); `PT` adds that PT also reads as
post-tensioned. Path: `views/<v>/annotations/<id>/text` (`.../tail`, `views/<v>/title`); id: the annotation or view.
Fix: the exact op `{"op":"update","path":"<owner>","value":{"<field>":"<text>"}}` whose text replaces every flagged token
of that field by its approved form and changes nothing else (so the fixes of one text are the same op and may be applied
in any number), then the overrides: `acknowledge` W_ABBREV on that owner with a reason, or in the company style the
switch (`notes.abbreviations.rules.<rule>: false`, add the variant to `approved`, or remove the entry from `always`).
Coverage (§25.3) still reads every variant as its words.

**Company style: `notes.abbreviations`.** An object; every key is optional:
- `extends`: `"us-structural"` (the built-in list, default) or `"none"` (start empty);
- `rules`: `{"period_on_shortened", "dotted_initials", "bare_acronyms", "variants"}` booleans, default all true;
- `approved`: abbreviation → word(s) (string or up to 8 strings, 1-60 characters), added as approved forms; one that is
  already listed (approved or a variant) replaces that entry and keeps its ordinary-word mark;
- `variants`: spelling → the approved form it is written as;
- `remove`: spellings (approved or variants) dropped from the built-in list;
- `always`: approved forms whose spelled-out words warn;
- any other key: an approved abbreviation → word(s), the alpha.5 form (`{"BRG.": "BEARING"}` keeps working).
The list is built in that order (built-in, less `remove`, plus `approved`, plus `variants`) and checked when the style
loads: a variant whose target is not approved, an `always` entry that is not approved (with "did you mean" by letters), a
non-boolean or unknown rule, a bad `extends` or a wrong shape is `E_STYLE` naming the key. Up to 500 entries per list.
`kerf-standard` spells out the defaults (`extends: "us-structural"`, all rules on, `always: []`).
`spec/styles/example-company.kerfstyle.json` shows every key. `kerf schema abbreviations` prints the list in force (rules,
`always`, one row per approved form with its meaning, form, reference-set count and the spellings flagged to it);
`spec/styles/ABBREVIATIONS.md` is that output for `kerf-standard`.

**Workspace style.** Without `--style`, every CLI document command (`check`, `apply`, `new --ops`, `export`, `drawing`,
`mesh`) applies the nearest `kerf.kerfstyle.json` in the document's folder or an ancestor; `--style` wins. `check`
and `apply` (and `new --ops`) print a first line `STYLE <path> (workspace style; --style overrides)` or `STYLE <path>
(--style)` when a style file applies (none without one). `kerf schema abbreviations` takes `--style` or the one found from
the current folder. `kerf serve` applies the one found from its `--dir` to every check, apply and export, serves it merged
over the default as `style.json` (so the browser engine and the header's STYLE name use it) and names it in its banner
(document info is cached per document, so a style edit shows on the next document change). `kerf init` writes a starter
`kerf.kerfstyle.json` (the defaults, `id: "company"`) unless one already applies from a folder above, and AGENTS.md
says it is the designer's.

**Acknowledgements on annotations and views.** Notes, dims, labels, welds and views take `acknowledge` (same entries as
components, §25.4). An annotation's entry matches a warning whose path is `views/<v>/annotations/<id>` or below it and
whose id (if any) is the annotation's (W_ABBREV, W_NOTE_STYLE, W_LEADER_HIT, W_LABEL_OVERLAP, W_NOTE_LANDING,
W_NOTE_TARGET, W_DIM_ZERO, W_WELD_SIZE, W_UNKNOWN_KEY ...); a view's matches a warning whose path is `views/<v>` or below it
but not under `annotations`, and whose id (if any) is the view's (W_ABBREV on the title, W_VIEW_FIT, a view-level
W_LEADER_HIT); a component's entry no longer matches a warning on an annotation path. Everything else is as for components:
I_ACK (`on '<label>'`), the DOC count, the ACKNOWLEDGED block (owners listed as `<id>` for components, `<view>/<id>` for
annotations, `view <v>` for views), `--strict` (` [acknowledged on '<label>': <reason>]`), W_ACK_UNUSED (path
`<owner path>/acknowledge`, "no CODE (#k) is about annotation 'n1' in view A", fix = the update op on the owner), shape
errors (`E_PARAM` at `<owner path>/acknowledge/<i>`), unknown keys. `acknowledged` entries of an annotation or view add
`"view"` and `"on"` (the owner path); component entries are unchanged. The drawing IR of a view lists the acknowledged
warnings of the components it draws and of its own annotations and title. `export --review` boxes an acknowledging
annotation's text items (else all its items) or a view's title block, tags it `ACK n` and lists it in the legend like a
component.

## 26. v0.2.0-beta.1 drawing kinds

### 26.1 beta.1: tables and schedule views
From the MASS dogfood run (issue #6): every schedule in the library (lap and development lengths, anchor embedment,
holdowns, headers, hangers, base-plate rods) was built from one `solid` per cell rule and one `label` per cell, or flattened
into list notes, and a view holding only a schedule needed a dummy component, printed a model scale and could not rename
DETAIL NOTES.

**Table annotation** (`type: "table"`, any view kind):
```jsonc
{ "id": "t_lap", "type": "table",
  "title": "CLASS B TENSION LAP SPLICE (IN.)",           // optional, centred and underlined above the border
  "columns": [{ "width": 0.6 }, {}, {}, {}, {}],         // one entry per column (1-40)
  "header": [                                            // optional grouped header rows
    [{ "text": "BAR\nSIZE", "rowspan": 2 }, { "text": "f'c = 3,000 PSI", "colspan": 2 }, { "text": "f'c = 4,000 PSI", "colspan": 2 }],
    ["OTHER", "TOP", "OTHER", "TOP"] ],
  "rows": [ ["#3", 24, 28, 24, 25], ["#6", { "text": "SEE NOTE 2", "colspan": 2 }, 37, 49] ],
  "notes": ["VERT. BARS: USE OTHER VALUES", "* VALUES IN INCHES"],   // optional, under the table
  "rules": "all",                                        // all | header | outer | none
  "at": [0, 0] }                                         // optional top-left corner of the border
```
- **Columns.** An entry is a header string or `{header, unit, width, align}`. `unit` prints as `(UNIT)` on its own line
  under the header (a unit already in parentheses prints as written); with `header` rows it goes into the lowest header
  cell over the column when that cell spans only this column, else `W_PARAM` says it is not printed. `width` is the printed
  column width in paper inches, 0.1 to 30 (`E_PARAM` otherwise). `align` (left | center | right, default center) applies to
  body cells; header cells are centred.
- **Cells.** A cell is a string, a number (printed as written in JSON), null (empty) or `{text, colspan, rowspan, align}`.
  `\n` in a text starts a new line. Placement is HTML's: each cell takes the next column of its row not covered by a
  rowspan from a row above; a rowspan stays inside its section (header or rows). Every row must cover every column exactly:
  a row with too many cells, a colspan past the last column or over a rowspan from above, or a row that leaves a column
  empty is `E_PARAM` (path `.../rows/<r>` or `.../rows/<r>/<c>`), naming the columns and those covered by rowspans from above.
  `header` rows and column headers in `columns` together are `E_PARAM` (one header form per table). Without `header`, the
  column headers and units make one header row when any is given. A table needs at least one header or body row; at most
  300 rows (header + body). Cell keys other than text/colspan/rowspan/align, and column keys other than
  header/unit/width/align, warn `W_UNKNOWN_KEY`.
- **Layout** (deterministic, paper inches times the view scale factor). Text height `h` = the style's note text height,
  line pitch = `h` x `line_spacing`, cell padding 0.6 `h` left/right and 0.7 `h` top/bottom. A column without `width` is
  as wide as its widest one-column cell: the cell's longest line, at most `wrap_chars` x 0.75 `h`, but never less than its
  widest word, plus padding (at least 3 `h` + padding). Then spanning cells, fewest columns first, widen the columns they
  span that have no `width` evenly by what they lack. Text wraps at its cell width (by stroke-font width, keeping a value
  and its fraction or qualifier together, §25.5); a word never splits. Row height: the tallest one-row cell; spanning
  cells, fewest rows first, add what they lack to their last row. Text blocks sit in the vertical middle of their cell,
  aligned per cell.
- **Rules** (layer notes): the border is a closed rectangle in the `title` pen; the rule under the header rows in the
  `profile` pen; every other rule in the `anno` pen. A rule is drawn only between slots of different cells (spans cut it).
  `all`: everything; `header`: no rules between body rows; `outer`: the border and the header rule; `none`: text only.
- **Title** wraps at the table width, centred above the border with its last baseline 0.95 `h` above it, underlined
  (`anno` pen) 0.6 `h` above the border. **Notes**: the first baseline 2 `h` under the border, `NOTE:` / `NOTES:` (by the count of numbered
  notes), then `1.` `2.` ... with a hanging indent, wrapped at the larger of the table width and 1.25 x `wrap_chars` x 0.75 `h`;
  a note starting with `*` is a footnote: no number, flush left.
- **Placement.** `at` (a Ref or `[x, y]`; in an iso projected like a label's) is the top-left corner of the border. A
  schedule view stacks its tables without `at` from `[0, 0]` (border corner) downward in annotation order, one `gutter_in`
  apart (block to block, titles and notes included). Other views place them under everything else (the crop, notes,
  dimensions, labels, the notes list), left-aligned with those extents, one `gutter_in` below, in order; the view title
  then goes under them.
- **Text rules.** Title, headers, cells and notes take the style case transform (§25.3: `f'c`, `db`, `16d` keep their case)
  and `W_NOTE_STYLE` (path `.../title`, `.../columns/<k>`, `.../header/<r>/<c>`, `.../rows/<r>/<c>`, `.../notes/<k>`; no
  repeat check, units are not linted; after 8 lines per table one more lists the other paths). Coverage (§25.3) gets three kinds of candidate per table, all reporting
  `table <id>`: the title with every column header, unit and header cell; each body row (its cells, numbers included) with
  that heading; each note.
- **`W_TABLE_CELL`** (warning, id = the table, path = the cell): a cell's text (a word wider than its fixed-width columns)
  runs over the rule. The message gives the paper width it needs and the columns' width; the fix the `width` that fits
  (rounded up to 0.05"), or removing `width`.
- **`W_TABLE_OVERLAP`** (warning, path `.../at`): a table placed with `at` whose block (1/20" paper inside its extents)
  overlaps another table, a note, dimension, label or the notes list, or the drawing (a line crosses it or it sits on a
  hatched or filled region). The fix: remove `at`, or (section and schedule views) an `at` that puts it under everything.
- `W_VIEW_FIT` names a table that sets an overflowing side (`table '<id>'`) with the fix to narrow its columns or move it.
- DXF/PDF/SVG/PNG: plain lines and text (DXF `TEXT`/`LWPOLYLINE` on the notes layer), editable in CAD.

**Schedule views** (`kind: "schedule"`; `"sheet"` is accepted as the same kind). A view of tables, notes and labels with
no geometry:
- It draws no components: its membership set is empty (§25.4), so it has no crop, no geometry checks (it is not one of
  the view sets of §9) and no `W_CROP_STALE`. `scale`, `crop`, `cut_z`, `from`, `cutaway`, `omit` and `only` are ignored with a
  `W_PARAM` each (`"scale": "NTS"` is accepted silently). The drawing IR has `kind: "schedule"` and `scale: 1`: coordinates
  are paper inches, y up, with the first auto-placed table's border corner at `[0, 0]`; the caption and title block print
  `SCALE: NTS`.
- Notes are always list notes (no target, no leader); the list goes by `notes_side`: `both` (default) and `above` put it
  above the tables, `left`/`right` beside them (§24). Labels take `at` in paper inches. A `dim` or `weld` is `E_PARAM` (it
  needs a section view).
- The title goes under the lowest annotation, left-aligned with the content. When content and title exceed the sheet area
  (§25.2), `W_VIEW_FIT` reads `schedule view <id> with its title needs W x H paper inches ...` with the size of each table,
  the list and each label, and fixes to narrow the widest table, split the tallest, or move the list (`notes_side`).
- A section view still rejects `"NTS"`; the message now points to `kind: "schedule"`.

**Notes list heading and untargeted keyed notes.**
- View `notes_title` (1-60 characters, `E_PARAM` otherwise) replaces `notes.list_title` (DETAIL NOTES) as the heading of
  that view's notes list (§24), under the case transform: `"HEADER SCHEDULE NOTES"`.
- In a keyed view a note with neither `target` nor `at` is a list-only entry: it is numbered like any keyed note (§24) and
  listed, with no tag and no leader (it was `E_PARAM`). A leader view still requires `target` or `at`; the error now
  suggests a keyed or list view for general notes.

### 26.2 beta.1: sheet layouts
From the MASS dogfood run (issue #7): a company detail is one sheet with several scenarios side by side (A-D), one shared
DETAIL NOTES list, plan insets beside a main section at their own scale, and "SEE 3/S-50.01" callouts. Kerf exported one view
per sheet, so a 4-scenario detail became 4 sheets with 4 lists, gaps in each list (`1,2,3,4,6`) and a different auto scale
per scenario.

**Document `layouts`** (array; ops: `add layouts`, `update|set|remove layouts/<id>`; `kerf schema layout`):
```jsonc
{ "id": "S1",                       // required; `kerf export <doc> --layout S1`
  "views": ["A", "B", ["P1", "P2"]], // required: view ids in reading order; a nested list stacks across the arrangement
  "arrange": "row",                 // row | column | grid; default row, or grid when `columns` is set
  "columns": 2,                     // grid: entries per row (1-12); with row/column it warns W_PARAM and is ignored
  "notes": "shared",                // shared (default) | per_view
  "notes_side": "right",            // right (default) | left | below | above: where the shared list goes
  "scale": "common",                // common (default) | a numeric scale for the views without their own
  "gap": 0.5,                       // paper inches between entries (0-4)
  "sheet": "17x11",                 // paper size "WxH" or [w, h] in inches (5-60 each); default the style's sheet
  "title": "...", "number": "1" }   // title block DETAIL (default doc title) and DETAIL NO (default empty)
```
Errors are `E_PARAM` (shape, values) and `E_REF_UNKNOWN` (a view id not in `views[]`, with the list and a did-you-mean); a
view named twice is `E_PARAM`, a duplicate layout id `E_DUP_ID`; unknown keys warn `W_UNKNOWN_KEY` against the layout
table. Removing a view a layout names fails the apply with that `E_REF_UNKNOWN`.

**Arrangement** (paper inches). Each view is built as on its own sheet (any view kind: the layout calls the normal view
build), then measured by its full extents (drawing, notes, list, title). `row` puts entries side by side, `column` one above
the other, `grid` in rows of `columns` entries (column width = widest entry of the column, row height = tallest of the row).
A nested list in `views` is a column inside a row or grid, a row inside a column. Inside its box an entry is centred across
and sits on the box's bottom, so the titles of one row share a line; columns stack from the top; `gap` separates entries.
The shared list goes `gutter_in` beside (top-aligned) or above/below (left-aligned) the arranged views; the whole is
centred on the sheet. The layout's drawing coordinates are paper inches times the scale of its first view: a view at that
scale keeps model inches 1:1 (DXF), a view at another scale is scaled by the ratio (its text heights too; hatch pattern
size per paper inch unchanged).

**Views in a layout** keep their bubble, title and `SCALE:` line (each cell shows its own scale; the title block reads the
common scale or `AS NOTED`). A view title wider than the view (less the bubble; at least 3" paper) wraps at spaces in a
layout only. A view's own `W_VIEW_FIT` against the whole sheet is not checked inside a layout (the layout checks its cells), nor in `kerf check` for a view any layout names (§28.2).

**Scale.** Precedence for each view: its own `scale` > the layout's numeric `scale` > the document `scale` > `common`.
`common` builds the remaining views at each standard scale, 3" ... 1/4" = 1'-0" (largest first), and takes the first at
which the arranged views and the list fit the sheet area (frame interior above the title block, 1/8" clear, §25.2); none
fits: 1/4" and `W_VIEW_FIT`. An iso view without a scale is drawn at the common scale over `iso_ratio` (§28.2); with no scale to pick it stays NTS, fitted to its share of the sheet area (the area divided
among the entries along each axis of the arrangement). The document `scale` (numeric, else `E_PARAM` path `scale`) applies to every view without its
own except iso views (NTS by default), also outside layouts; ops `set scale` / `remove scale`.

**Shared notes** (`notes: "shared"`). The keyed notes (`notes_style` keyed or list, §24) of all the layout's views, schedule views
included (§28.2), form one DETAIL NOTES list. Grouping, in layout order (views, then annotations): a note with a `key` joins the entry with that key
in any view; a note without a key joins an earlier entry with the same `text` (trimmed) and the same `cite`; else it opens a
new entry. Numbering: keyed entries take their key, the others the lowest free numbers in order; the entries are then
numbered 1..N in that order, so the list has no gaps. When a key prints under another number, `I_KEY_RENUMBERED` (info,
id = layout, path `layouts/<id>`; printed by `kerf check`) lists `key k prints as n`. Tags in every view show the layout
number; the views draw no list of their own; the list is drawn once (§24 look; side per `notes_side`). Notes under one
number whose texts differ warn `W_PARAM` naming both notes and views. `per_view` keeps each view's own list and numbers.
Leader-style views are unaffected. Single-view export (`--view`) is unchanged: a view keeps its own numbering and list.

**`W_VIEW_FIT` on a layout** (id = layout, path `layouts/<id>`): the size the layout needs against the sheet area, each
view's paper size and scale, the list size and side, the largest view; fixes in order: a larger standard sheet (17x11, 22x17, 24x18, 36x24)
that holds it as arranged; for a numeric layout scale, the standard scale that fits (else `common`); when every view has
its own scale, the ops that remove them (and the document `scale`); a view whose own scale is larger than the common one;
the first other arrangement (row, column, grid 2..n-1, with the list right or below) that fits; the list on another side;
last, shrink the largest view's crop or split the layout.

**`ref` annotation** (any view; `kerf schema ref`): a bubble split by a horizontal line, `detail` (required, string or
number) in the top half at text height, `sheet` (optional) in the bottom half at 0.9 label height (no sheet: a plain
bubble); radius max(1/4", half the wider text + 0.06") paper. `at` (Ref, `{ref, offset}` or [x, y]; projected in an iso
like a note `at`) draws a leader from the bubble edge with the style arrowhead; `place` ([x, y]) is the bubble centre,
default `at` + 3/4" paper up and right; neither is `E_PARAM`. `text` (case transform) sits beside the bubble, on the side
away from the leader. Notes layer, anno pen, src = the ref id; placed exactly (not moved by the note layout); its extent
counts for the view fit. A view whose annotations are all refs is a "see other detail" scenario: it draws no model (§28.2).

**Outputs.** `drawing` with `layout`: the Drawing IR (§10) with `"kind":"layout"`, `view` = the layout id, `scale` = the
first view's, and `"cells":[{"view","scale","scale_label","bounds"}]` (the views' extents in layout coordinates); its
`diagnostics` are the views' plus the layout's. `export` with `layout`: svg/png/dxf/pdf as for a view; the sheet (PDF always,
others with `sheet`) uses the layout's `sheet` size. `review` with `layout` is `E_INPUT`; `view` and `layout` together are
`E_INPUT` (CLI: usage error, exit 2; `--layout` only with `drawing`/`export`). `kerf check` builds every layout and adds
its layout-level diagnostics (W_VIEW_FIT, I_KEY_RENUMBERED, shared-text W_PARAM) once.

### 26.3 beta.1: plan and elevation views

From the MASS details dogfood run (issue #9): plans were faked as sections with XY as the plan (vertical bars as
`along_z` dots, titles "(PLAN)" by hand, `steel orient: plan` with z as up), elevations were sections whose members in
front of `cut_z` silently vanished, and `shown: "dashed"` meant both "hidden" and "where occurs". One model now gives
every view: members keep their real orientation (Y up, Z toward the section viewer) and the view kind picks the drawing.

**`"kind": "plan"`** looks down (−Y) at the horizontal cut plane y = `cut_y` (default: 1" below the top of the highest
drawn member, fills excluded).
- Plan coordinates: plan x = model x, plan y = −model z, so the viewer of a section (at +z) stands at the bottom of the
  plan and the drawing is not mirrored. A plan's `crop` is in plan coordinates; `{"x": [x0, x1], "z": [z0, z1]}` (model
  z) is accepted for it. Literal points (`at`, `place`, dim `from`/`to` as `[x, y]`) are plan coordinates. A Ref
  `comp@<box anchor>` is that box anchor of the instance's plan box (its x extent times its z range, a part's box for
  `comp.part@anchor`); a named anchor (`bend_1`, `vertex_3`, ...) is at its model x and the z of the instance's centre
  (an xz bar's anchors at their own z).
- Each prism (an XY profile over [z0, z1]) becomes plan prisms: rectangles x-interval × [−z1, −z0] whose depth is model
  y. The x-intervals come from the profile: where the line y = `cut_y` crosses the profile the interval is **cut**
  (hatched by material, cut pen, lumber running along y such as studs and posts gets its end-grain X); between the
  profile's vertices below the plane each interval is **below**, its depth the profile's top there. Neighbouring
  below-intervals of one member merge when their tops meet without a step or a crease (less than 10°), so a plan line
  appears only where the top surface breaks (a stem above a footing: the stem's faces). Members wholly above the plane
  are not drawn. Then the section algorithm of §8.1 runs with `cut_y` as the cut plane: below-intervals are outlines
  hidden by cut regions and by higher intervals.
- Rebar: a bar along z (`along_z`) and the part of a path bar below the plane draw as a line along their run (the
  rebar pen, at the bar's z); where a path bar crosses the plane, and along each vertical leg below it, it is a dot
  (filled, the bar's diameter). Embedded parts cut square (anchor bolts) draw as dots too. Membranes crossing the plane
  are a line along z; loose fill (earth, gravel) is not drawn in plan.
- `"shown": "hidden"` / `"where_occurs"` members draw the outline of their whole plan footprint in the hidden pen, above
  or below the plane.
- A plan view ignores `cut_z` (`W_PARAM`); `cut_y` on any other kind warns `W_PARAM` and is ignored.

**`"kind": "elevation"`** looks toward −Z and cuts nothing.
- Every drawn member is beyond (§8.1) and nothing is in front of a cut plane: a member draws where no nearer member
  (larger z1) covers it, in the `profile` pen (fill materials keep their steel/rebar pen). Embedded members are never
  hidden (as in a section).
- Lumber seen end-on (run z: joists, blocking perpendicular to the elevation) keeps its X (or diagonal for `blocking`)
  where it is not hidden. Loose fill (earth) keeps its hatch on its visible part.
- Crop: every member the crop cuts gets a break line along that crop edge (lumber, concrete, steel: all but loose fill
  and dashed members), measured on its visible part, so a member hidden at the crop edge gets none.
- An elevation's `cut_z` only sets the default z of in-plane members (§1); `W_CROP_STALE` (§21) checks elevation views
  like section views.

**Both.** `"hidden_lines": true` draws the hidden parts of outlines (below cut regions or higher members in plan, behind
nearer members in elevation) as dashed lines in the `hidden` pen, and lets a note land on a member that is hidden
everywhere (on its hidden outline). Without it, a note targeting a member hidden everywhere is `W_NOTE_TARGET`, whose
fix names `cut_y` (plan) or `hidden_lines`. `hidden_lines` on a section or iso view warns `W_PARAM` and is ignored.
Dimensions, labels, welds, keyed notes and the title work as in a section view, in the view's own plane (§28.3);
`inspect at` takes plan coordinates in a plan view. Auto crop and auto scale (§19) work on the plan or elevation geometry.

**Titles.** The printed title (view title and title block) of a plan view is the style's `text.plan_title` with
`{title}` replaced by the view's `title` (default `"PLAN - {title}"`, the MASS house form "PLAN - CORNER REINFORCING"),
and of an elevation view `text.elevation_title` (default `"{title}"`: unchanged). The template is not applied when the
title already contains the word PLAN (PLANS) or ELEVATION (ELEVATIONS, ELEV.), so "BASE PLATE (BP-A) PLAN" prints as
written. Coverage (§21) reads the title as written in the document.

**`shown`.** `"solid"` (default), `"hidden"`: dashed graphics (§16), no hatch or cut mark, never hides anything, exempt
from `W_FLOATING` / `W_NEAR_MISS` / `W_OVERLAP`, notes keep their text; `"where_occurs"`: the same graphics, and notes
targeting it end in ` (WHERE OCCURS)`. The old `"dashed"` behaves as `where_occurs` and warns `W_SHOWN_DASHED` with
both update ops (pick `hidden` for a member that is only behind or below others).

**Rebar `plane: "xz"`** (corner and intersection bars, bars around openings): `points` are `[x, z]` pairs at the height
`y` (a length or a Ref, whose y is used), bends filleted at `bend_radius`. `z` (a number) shifts the bar along z;
`array` works as for other bars. A plan draws the bar as its path (cut when `cut_y` passes within its radius). A section
draws a dot where the bar crosses `cut_z` and its centreline projected on XY where it runs behind the cut; an elevation
draws the projection. The checks, iso and the mesh use legs: a run along x is a bar seen lengthwise, a run along z a dot
extruded along z, a diagonal run a staircase of short slabs (at most 48). `W_COVER` reports the worst leg once per host
face; it measures cover in XY only. Named anchors: `start`, `end`, `bend_k`, `seg_k_mid` (§25.1), at the bar's height.

**Composition.** The kind is a property of one view and its Drawing (`"kind": "plan"` / `"elevation"` in the IR, §10);
anything that places views (sheet layouts) places these like any other. Plans and elevations are built from the same
compiled prisms as sections, so whatever changes those prisms before a view draws them (e.g. subtracted solids) shows in
every kind.

### 26.4 beta.1: fills under solids, voids and sleeves
Solids subtract each other (#14), so no fill, pour or wall is traced by hand around what it surrounds.

**Fills under solids** (§5.10). In a section view a cut fill (role soil) is clipped by every solid the cut plane also
cuts in that view: per component instance whose bodies (not `embedded`, not `shown: dashed`, not soil or void) reach
across `cut_z`, the union of their outer outlines (holes filled, so a tube or a W seen lengthwise clips as its whole
silhouette). The fill keeps the rest, split into one region per piece; its hatch, picking region and `top` / `full`
outline follow the clipped region, so the grade line stops at a stem. Fills are not clipped by fills (gravel over
earth still meets edge to edge). `fill.except: [ids]` names solids that do not clip it. Cutaway iso views clip the fill's
cap the same way. A fill overlapping a non-fill solid is exempt from `W_OVERLAP` (§9); fill against fill is still checked.
So earth around a footing and stem is one rectangle.

**Voids** (§5.4). `concrete` and `cmu_wall` take `voids`: a list of
- a component id, `"col"`, or a part, `"col.part"`: per instance, the union of the outer outlines of its bodies (holes
  filled: concrete does not get inside a tube) over its whole z extent (min z0..max z1 of those bodies). Because it is
  the member's own outline, the hole follows the member when it is resized or moved;
- a shape `{"rect": [w, h], "at": Ref, "anchor": "center"}` (the box anchor of the rect that lands on `at`),
  `{"circle": d, "at": Ref}` (centered on `at`) or `{"points": [...]}` (§3.3; literal points relative to `at` when given),
  each with an optional `"z": [z0, z1]` (default: through the host).
After every component is placed (§8 step 3), each host's world prisms lose their voids: a prism is split along z at the
void ends inside its z range into slabs (consecutive slabs with the same voids merge), and each slab is its region minus
the voids that span it, one prism per remaining outer loop with its holes. A slab that loses nothing keeps its loops; a
void wholly inside the region adds its exact outline (arcs kept) as a hole, otherwise the region is flattened (0.005").
The split marks the caps between slabs as seams: iso and the mesh draw a seam cap's edges only where the touching slab of
the same member does not continue them (the void's end faces), so the outside of the host shows no split lines. A
`cmu_wall` keeps its running bond across slabs. Sections, iso, the mesh, landing, picking and every check see the cut
host; voids apply in every view (they are geometry, not view membership). The summary line of the host ends with
`minus <what>` (ids, shape kinds, `sleeve <id>`). `W_COVER` (§9) also measures to the void edges (classified by the
concrete's outward normal, which points into the void). Removing a component a `voids` entry, a `through` or an `except`
names is refused like any referenced component (§14).

**Sleeves.** Component type `sleeve` (default placement anchor `center`): `pipe` (pipe outside diameter, null = an empty
sleeve), `od` (sleeve outside diameter, default `pipe + 2·clearance + 2·wall`), `clearance` 0.5, `wall` 0.25,
`pipe_wall` 0.25, `fill` (packing material between pipe and sleeve, e.g. `joint_filler`; null = open), `material` and
`pipe_material` (`steel`), `run` `z` | `x` | `y`, `length` (required for run x/y; run z optional as lumber), `through`
(a host id or a list of them). Parts `sleeve`, `fill`, `pipe`. Run z: three rings seen end-on (box od × od); run x/y: the
sleeve cut along its axis: sleeve and pipe as rectangular tubes (top and bottom walls full depth, front and back walls
the wall thickness), packing as two strips, natural z thickness od (2.5D, like pipe seen lengthwise, §5.13). Each host in
`through` loses the sleeve's envelope as if it listed the sleeve in its `voids`; a sleeve inside concrete without it is
`W_OVERLAP`. A block-out without a sleeve is a `voids` shape.

**Diagnostics.**
- `E_REF_UNKNOWN` (path `components/<host>/voids/<k>`, `.../through/<k>`, `.../except/<k>`): no component with that id
  (nearest id as the fix), or no such part (lists the parts).
- `E_PARAM`: a host voiding itself, a fill named as a void (fills are clipped, never cut), an id with `@` or `#`, a
  component with no solid outline, a shape without `at` (rect, circle), with no area, a bad `z`, an unknown key or not
  exactly one of rect / circle / points (fix: an example list); a `through` naming a type that takes no voids; a sleeve whose
  pipe does not fit, whose wall is half the od or more, or run x/y without `length`.
- `W_VOID_MISS` (warning, id = the host, or the sleeve for `through`; path = the entry): a void that removes nothing from
  its host. The message gives both boxes and z ranges and whether the outlines miss or only the z ranges; the fix is the
  `update` op that drops the entry (or, for a sleeve, move it into the host or drop the host from `through`).
- `W_OVERLAP` between a concrete/cmu host and a non-masonry solid (or grout) prints, before the usual fix, the op that
  subtracts it: `{"op": "update", "path": "components/<host>", "value": {"voids": [..., "<id>"]}}`, or for a sleeve
  `{"op": "update", "path": "components/<sleeve>", "value": {"through": "<host>"}}`.

## 27. v0.2.0-beta.1 wood framing

### 27.1 beta.1: I-joists and truss profiles
From the MASS details dogfood run (issue #10): floor and roof framing in the S-40/60/70/80 families is mostly I-joists,
open-web floor trusses and trusses seen end-on. Agents built I-joists from three engineered `lumber` pieces (the web drew
as a block of lamination lines), floor trusses from rotated lumber (webs swept across the chords, `W_OVERLAP` acks) and
end-on trusses from loose rectangles; scissor, mono and top-chord-bearing trusses had no profile.

**`ijoist`** (catalog type after `truss`). Generic series table, no brand names: `size` is `"depth x flange width"` in
either order (`"11-7/8 x 2-5/16"`, `"2-5/16 x 11-7/8"`, `"11.875x2.3125"`).

| flange width | flange depth | depths |
|---|---|---|
| 1 3/4, 2 1/16 | 1 3/8 | 9 1/2, 11 7/8, 14, 16 |
| 2 5/16 | 1 3/8 | 9 1/2, 11 7/8, 14, 16, 18, 20, 22, 24 |
| 2 1/2 | 1 1/2 | 9 1/2, 11 7/8, 14, 16 |
| 3 1/2 | 1 3/8 | 11 7/8, 14, 16, 18, 20, 22, 24 |

Web 3/8 (7/16 for 3 1/2 flanges and depths over 16); `flange_depth` and `web` override. A size off the table is `E_PARAM`
listing the table and the nearest entry as `{"size": "..."}`; a size with a brand series name (`TJI 230`) says to give
that series' depth and flange width.

| param | default | notes |
|---|---|---|
| `run` | `"x"` | `x`: along x, seen from the side; `z`: cut end-on |
| `length` / `until` | — | run x: as lumber (§17); run z: optional `length` as lumber run z (§25.5), `until` is `E_PARAM` |
| `stiffeners` | `"none"` | `none` `left` `right` `both`: a web stiffener pair (part `stiffener`) at that end: one each side of the web, flush with the flange edges, 3 1/2 along the joist, on the bottom flange with 1/8 under the top flange |
| `squash_blocks` | `"none"` | the same ends: a 2x pair (part `squash_block`) beside the flanges, 1 1/2 thick, full joist depth, 3 1/2 along it |
| `blocking` | false | run z: the flanges get the single diagonal (I-joist blocking or rim cut end-on) |

Geometry: the cross-section in (u, v) with the box (0,0)–(flange width, depth). Run z draws it as it is: flanges
material `wood` with the X cut mark, web material `osb` (a thin unhatched strip), stiffeners and squash blocks cut beside
it at the joist's z centre (the first section view's cut when the joist spans the run). Run x lays every piece along x as
a slab at its true z (flanges `bf` deep in z, the web `t_w`, stiffeners and squash blocks at their offsets), natural z
thickness = flange width; the slabs are `lengthwise` (§26.4, as steel in elevation), so a section plane through the joist
draws it as an elevation: flange lines and the web outline, never hatch or grain. Iso, plan and the mesh see the real I.
Parts `top_flange`, `web`, `bottom_flange`, `stiffener`, `squash_block`. Anchors: the 9 box anchors of the joist alone;
run x: `top_flange`, `top_flange_bottom`, `web_center`, `bottom_flange_top`, `bottom_flange` at mid-length, each with
`_start` and `_end`, and `bearing_left` / `bearing_right` (underside at the ends); run z: `top_flange`, `bottom_flange`,
`web_center`, `web_left`, `web_right`, `top_flange_tip_left/right`, `bottom_flange_tip_left/right` (as steel W §5.13).
Traits: near-miss checks like framing lumber; a slope host (`slope: "@j"` on its top flange).

**`W_BEARING`** (per-view geometry check, §9). For an axis-aligned run x `ijoist` and each instance: the members whose
top is within 1/32 of the bottom flange's underside, overlapping it in x and z (not hardware, line-like members, fills,
embedded or dashed parts) are its supports; touching supports merge. A support reaching an end of the joist must give
1 3/4 of bearing, any other 3 1/2 (manufacturer minimums, `Traits::min_bearing`). The message gives the bearing and the
support ids; the fix at an end is the length to add and the x the end must reach, at an intermediate support to widen it
or add squash blocks. A support under both ends at once (a joist shorter than its bearing) is not reported.

**`truss` additions** (§5.8 stays the default: `profile: "pitched"`, `orient: "side"`, `bearing: "bottom_chord"`).
- `profile: "scissor"`: the bottom chord is flat over `bearing_width`, then rises at `bottom_pitch` (default half the
  pitch; one at or above `pitch`, or `bottom_pitch` on another profile, is `E_PARAM`); its vertical thickness is depth / cos(bottom pitch); the
  top chord's lower edge passes through the bottom chord's top at the bearing (raised: `heel_height` above it).
- `profile: "mono"`: the high end of a mono truss. `heel_height` (required) is the end height above the bottom chord at
  the bearing; an end vertical (part `end_vertical`, 3 1/2 wide) stands at the bearing and the top chord falls inward at
  `pitch`. `overhang` defaults to 0 (a tail continues the top chord up and out). If the top chord meets the bottom chord
  before `span_shown`, `E_PARAM` gives the largest `span_shown`.
- `profile: "parallel_chord"` (floor or flat truss): `depth` (required) from the bottom of the bottom chord to the top of
  the top chord; chords default to `"4x2"` (a 2x4 laid flat: 1 1/2 in-plane, 3 1/2 thick; any `"AxB"` whose swap is a
  sawn size reads flat); webs as wide in-plane as the flat chord (3 1/2 for upright chords). An end vertical at the bearing,
  then diagonals alternating down and up from the top chord at the end vertical, every `web_spacing` (default: the clear
  height, 45°), edges meeting the chords on horizontal cuts and touching at the panel points, while one fits before
  `span_shown`. `pitch`, `heel`, `heel_height`, `bottom_pitch` are `E_PARAM` here. Too little clear height (< 3) or too
  tight a `web_spacing` is `E_PARAM` with the minimum.
- `bearing: "top_chord"` (parallel_chord only, else `E_PARAM`): the top chord runs over the support (`bearing_outer` and
  `bearing_inner` are at its underside, `bearing_width` apart); the end vertical and the bottom chord start at
  `bearing_inner`.
- `bearing: "face"`: hung at a beam or ledger face: no overhang (a nonzero `overhang` is `E_PARAM`); pitched, scissor and
  mono need `heel_height` (the end height) and draw an `end_vertical` instead of the heel web.
- Anchors added: `heel_block_top` (top of the top chord above the bearing's outer edge: `until` target for full-height
  eave blocking between trusses, the standard heel included), `end_vertical`, `end_top`, `end_bottom` (end vertical's
  outer face middle, top, bottom: mono, face, parallel_chord), `bottom_chord_end` (parallel_chord).
- `slope: "@truss"` follows the top chord the builder reports (`Built::pitch`): the pitch, negative for mono, mirrored for
  `exterior: "right"`, zero for parallel chord and end-on.
- `orient: "end_on"`: the truss runs along z, cut across. `depth` (required) is its height at the cut; the chords are cut
  per ply (`plies` 1–4 side by side along x, ply lines, wood X), the top chord depth / cos(pitch) tall for a pitched
  profile; between them a web strip one web thick along z every `web_spacing` (default 24) along the truss, the nearest
  half a spacing behind the cut, so a section draws it beyond and iso/3D show the webs as verticals (2.5D: diagonals in
  the truss plane are not modelled). `length` as lumber run z (default: the doc run). Anchors `top_chord`,
  `top_chord_bottom`, `bottom_chord`, `bottom_chord_top`, `web_center` (across the plies' middle). `plies` and `length`
  on a side view are `E_PARAM`.

**Engineered lumber seen lengthwise.** A `lumber` member cut along its length (run x/y in a section, a plan cut through
any lumber) draws no material hatch: the `wood_engineered` lamination lines appear only where the member is cut end-on
(drafting practice; §5.1 already said "run x/y cut lengthwise => outline only").

**Compile.** A prism's `zhalf`/`zoff` now apply whether or not the component has a natural z thickness (relative to the
instance's z centre), so a run z member that spans the doc run can carry pieces at the section (stiffeners, end-on webs).

### 27.2 beta.1: framing hardware

From the MASS details dogfood run (issue #11): hangers, post caps, post bases and holdowns were hand-drawn `connector`
polylines (`side` flipping with point order) or stacks of `solid` plates (16 in one detail); straps on a member face
needed a hand-computed thin `z` and then floated (`W_FLOATING`) or vanished in iso (`W_NOTE_TARGET`); unknown models drew
an 18 ga default silently; leveling nuts and weld studs were `solid`s and `anchor_bolt embed: 0.01` hacks. Hardware is now
one component placed from the members it connects, a set of thin plates at their true place in 3D, so section, plan,
elevation, iso and the mesh agree.

**`connector` `kind`.** `strap` (default with `points` or `on`), `tie`, `angle`: the polyline of §5.7. `hanger`,
`post_cap`, `post_base`, `holdown`: sized from their members; they take no `at`, `rotate`, `slope`, `mirror`, `z`,
`points`, `on`, `lay` or `side` (E_PARAM: remove it), and keys of another kind are E_PARAM naming the kind's keys. Without
`kind`, `points` or `on`, the kind comes from the model's family (below). A member key (`carried`, `carrying`, `post`,
`beam`) takes an id, `id#k` (array instance k) or a list of ids drawn as one member (an I-joist's flanges and web, a
built-up post); these keys, `rod`, `on` and an anchor bolt's `plate` are placement dependencies. Every plate is steel,
gauge thick (`gauge`, default from the model, else 18; caps, bases and holdowns 12), and is framing hardware (below).
- **hanger** (`carried`, `carrying`, `mount` `face` | `top_flange`, `height`, `seat`, `width`): the carried member must
  end at a side face of the carrying one (gap -1/4" to 1/2" along x or z, overlapping in the other two axes; else E_PARAM
  with both extents and the fix). Seat under the carried member's end, `seat` long (2) plus the gap, as wide as the member
  (`width` overrides); sides on its two faces, `height` tall (0.85 x the member's depth at the face, down to 1/16"); face
  mount: two flanges 1" wide on the carrying face, outside the sides, three nail dots each; top flange: a back strap up the
  carrying face and a flange over its top, min(2 1/2", its depth) long. A member running along x (seen lengthwise in a
  section) gets sides that follow its bottom edge (sloped joists); one running along z is boxed. Models ending in `TF`
  and the top-flange families default to `top_flange`. Anchor `seat`.
- **post_cap** (`post`, `beam`, `height`): the beam must sit on the post (bottom within 1/4" of the post top, over it;
  else E_PARAM with the `at` fix). Seat plate at the post top; side plates up the beam's two faces across the beam
  (`height`, default half the beam depth, at most 7"), and down the post's other two faces (as tall, at most half the
  post), with nail dots on those seen face-on. A beam longer along z than its width runs along z. Anchor `seat`.
- **post_base** (`post`, `standoff` 1, `height` 3 1/2, `face`): base plate `standoff` below the post bottom, a seat under
  the post when the standoff exceeds two plate thicknesses, side plates from the base up the post (`face` left/right: the
  x faces, default; front/back: the z faces). Anchor `base_bottom`.
- **holdown** (`post`, `rod`, `face`, `height` 10, `width` 2 1/2, `seat` 3/8, `seat_height` 0): on the post face toward
  the rod (`face` overrides; a rod inside the post outline is E_PARAM): back plate on the face, seat `seat` thick from the
  face past the rod (rod radius + 3/8", at least 1/2"), two side plates tapering from the seat to the top of the back
  plate (a polygon when the face is x-normal, four slabs when it is z-normal: 2.5D), centered on the rod along the face,
  and screw ticks every 1 1/2" into the post (embedded). `rod` names an `anchor_bolt` or `bolt`. Anchor `seat_top` (the rod
  axis at the top of the seat).

**`on`: straps on a member face.** `"on": "<member>.face_left|face_right|face_top|face_bottom|face_front|face_back"`
places a strap flush outside that face: left/right/top/bottom are seen edge-on (`lay` edge, thickness outward, `width`
along z centered on the member), front/back face-on (`lay` face, gauge along z at the face). `points` snap onto the face
line (x for left/right, y for top/bottom; kept for front/back); without them the strap runs the whole face along the
member (top/bottom of a member running along z: the strap runs along z). A numeric `z` moves the strap's center. `side`
and the keys of other kinds are E_PARAM; a `lay` that contradicts the face is E_PARAM.

**Dashed when hidden.** Plates of these kinds and `on` straps are framing hardware. In a section, plan or elevation, the parts
of their outline a nearer (or cut) member hides draw in the `hidden` pen, without `hidden_lines`; in iso, the parts of
their edges other components hide (their own faces excepted) draw dashed. A note whose hardware target is hidden
everywhere lands on the hidden outline (no `W_NOTE_TARGET`). Plates seen face-on carry nail dots, drawn where the plate
is visible (section and elevation; radius 0.012 paper inch x scale, at least 0.04"). A `face_front` strap is a face-on
tie (§5.7: drawn over what it covers, with nail dots).

**Models.** `model` resolves against the table (exact, case-insensitive), then the families: a model starting with a
family prefix followed by a digit, `-`, `.`, `/` or nothing (longest prefix wins: `HDU5`, `HU212-2`, `ITS2.06/11.88`,
`CCQ44`). A family gives the kind, the hanger mount and a typical gauge (and width for straps); geometry comes from the
members, never from a product table, and the note carries the exact model. `kerf schema connector` lists the families,
`kerf catalog` their descriptions. A model in neither, with `gauge` (and for a strap `width`) not given, is
`W_UNKNOWN_MODEL` with the nearest known name; giving them silences it.

**Anchor bolts.** `plate: "<id>"`: the member the rod passes through; on the rod's axis its bottom and top set the nut:
washer and nut on its top, and `projection` (when not given) reaches through them plus d/4, rounded up to 1/16".
`leveling_nut: true` (needs `plate`, else E_PARAM) adds a washer (2.25d x 1/8) and nut (1.5d x 0.875d) under the plate
(parts `leveling_washer`, `leveling_nut`). `plate_washer: "WxWxT"` replaces the round washer. `hook: "adhesive"`: a
straight rod in a drilled hole (`hole`, default d + 1/8) filled for the full embed (part `adhesive`, material grout,
embedded). `projection: 0` is allowed with `nut_washer: false` (a rod cut flush); with a nut it is E_PARAM, as is a
projection that ends inside the plate, washer and nut.

**Weld and headed studs.** With `bolt rotate: 180` fixed (#15) a weld stud can be drawn as a bolt, but it then carries a
head at the flange and reads as a bolt in the summary and notes, so `bolt` gains `kind`: `bolt` (default), `weld_stud`
(no head: a weld collar at the placement point, the welded face, anchors `head_bearing` = `weld_face`; the shank up
through the `grip`, washer and nut on top, d/2 stick-out) and `headed_stud` (weld collar, shank, head 1.6d x 0.5d;
`grip` is the stud's length after welding). Studs point up (+y) and are side only (`orient: "end"` is E_PARAM: rotate the
component).

**Checks.**
- `W_UNKNOWN_MODEL` (path `components/<id>/model`): above.
- `W_HANGER_SIZE`: a hanger under 60% of the carried depth (fix: the `height` update op), taller than the member, a seat
  width more than 1/8" off the member's, or a model whose size reads off its name (`LUS210` = 2x10, `HU212-2` = two
  2x12, `ITS2.06/11.88`) and is for a member more than 1/2" shallower, 1" deeper or 1/4" narrower or wider.
- `W_HOLDOWN_ROD`: no `rod` (fix: the update op naming the nearest anchor bolt or bolt within 8"); a rod that ends below
  the seat top plus washer and 0.875d nut (fix: the anchor bolt's `projection` update op); a rod whose lower end lies in
  no concrete, masonry or steel member.
- `W_POST_BASE` (path `components/<id>/standoff`): the base plate bottom is not within 1/32" of the top of a member under
  it; the fix is the `standoff` update op that makes it bear on the member below (or moving the post).
- `W_FLOATING` of a face-on strap set with `z` that overlaps a member in plan and floats less than 1" off its front or
  back face says so; the fix is `{"op":"update","path":"components/<id>","value":{"on":"<member>.face_front","z":null}}`.
- `W_NEAR_MISS` is not reported between a hanger's carried and carrying members (the gap is the hanger's).

Limits (2.5D): hangers follow members that run along x or z; skewed (plan-angle) hangers are not modelled, and a
z-normal holdown's tapered sides are slabs. A post cap under a beam that ends on the post draws as a full cap.

### 27.3 beta.1: lumber cuts and two-point members

From the MASS details dogfood run (issue #12): lumber was a rotated box. A rafter rested on the corner of its plate, a
flush beam could not be notched for the plate, sloped chords ended square and their rotated box ran through the member
they met, and braces and truss webs were placed with angles and lengths computed in a script and trimmed by trial (with
`W_OVERLAP` acknowledged). Members seen lengthwise now take cuts, and braces run between two anchors.

**Frame.** A member seen lengthwise (lumber `run` x or y; `steel` `orient` elevation or plan) is a strip in its own
frame: u along the length (0 at the start, `length` at the end), v across (0 on the bottom face, the depth on the top
face). Run x: u = +x, v = +y; run y: u = +y, v = −x (v is always left of u). The cuts below change that strip's profile
in the builder's local coordinates before the member is placed, so section, plan, elevation, iso, the mesh, picking and
every check (`W_OVERLAP`, `W_FLOATING`, `W_NEAR_MISS`, landing) see the cut member. Lengthwise lumber grain and ply lines
follow the cut profile. The 9 box anchors stay those of the uncut `[0, length]` box. `cuts` on an end-on member (lumber
run z, steel section) or together with `mirror` is `E_PARAM`.

**Two-point members.** `lumber` and `steel` (elevation/plan) take `from` and `to`: points (a Ref, `{ref, offset}` or
`[x, y]`, §3.3). The member's length is |to − from| and its angle that of from→to (`rotate`, `slope`, `at`, `length`,
`until` and `mirror` with them are `E_PARAM` whose fix is the op that removes the key; lumber without `run` runs x; `run:
"z"` is `E_PARAM`; steel needs `orient` elevation or plan). `align`: `center` (default) | `top` | `bottom` picks the line
that runs from `from` to `to`: the center line, or the top (v = depth, left of from→to) or bottom face. Each end whose
point is a Ref to a component (`comp@anchor`, `comp#k@anchor`, `comp.part@anchor`) is trimmed to that component (a
`trim` cut, below) unless `cuts` gives that end its own cut; an end at a literal point is square. So a brace
`"from": "tie@top_center", "to": "chord@bottom_center"` bears flat on the tie and the chord, needs no computed number and
follows both when they move. `from`, `to` (and every Ref in `cuts`, except `notch.by`) are placement dependencies (§3.2);
`slope: "@brace"` follows its angle.

**End cuts.** `cuts` entries with `type` `square`, `plumb`, `level`, `bevel` or `trim` close an end (`end`: `start`, `end`
or `both`; default `both` for square, plumb, level and bevel with an angle; trim and bevel with a line need one end; two
cuts on one end are `E_PARAM`). Each is a line: square ⊥ the member, plumb vertical and level horizontal in world, bevel
`angle` degrees from square (CCW in the frame, |angle| < 80), all through the end of the reference line (the aligned line
of a two-point member, else the center line: u = 0 or `length`, v = depth/2); bevel `line: [p, q]` the line through two
points; trim `to: "<id>"` the face of that component the reference line meets: casting from the member's middle toward
that end, the first boundary edge of the target's outline (that instance, that part) it crosses, at most max(length, 48")
beyond the end. The member is stretched to reach both lines and clipped to the strip between them, so an end cut extends
or shortens the member as needed. A cut along the member (plumb on a vertical member, level on a level one) is `E_PARAM`;
end cuts that cross inside the member (too short for its cuts) or meet its faces absurdly far out are `E_PARAM`. Named
anchors `start_top`, `start_bottom`, `end_top`, `end_bottom` are the cut corners (sheathing on a plumb-cut rafter:
`"at": {"to": "rafter@start_top"}, "until": "rafter@end_top"`). A trim whose line misses its target is `W_CUT_MISS`
(the end stays square; the fix is the op that drops the cut, or for `from`/`to`, a face anchor of the target).

**Birdmouth.** `{"type": "birdmouth", "on": "<id>", "seat": s | "heel": h, "tail": t}` cuts a seat in a sloped member
(its bottom face must look down: within 85° of horizontal and at least 1° off level, else `E_PARAM`). The seat is level
(world), `seat` long (horizontal; default: the width of `on`'s box), and meets a plumb heel cut of height
`seat · tan(slope)` (`heel` sets that height instead; both is `E_PARAM`). The heel is on the low side of the member;
`tail` (default 0) is the horizontal distance from the heel's plumb line to the low end of the reference line (the eave
overhang). Named anchors: `seat_outer` (heel corner, where seat and plumb cut meet), `seat_inner` (the seat's inner end on
the bottom face), `heel_bottom` (the plumb cut's foot on the bottom face). Without `at`, a member with a birdmouth `on`
a component is placed with `seat_outer` on the outer top corner of `on` (top_left when the member rises toward +x, else
top_right), so the seat bears on it with no computed offset. The summary line gives seat, heel and HAP (the plumb height
above the seat). A seat that does not lie on the top of `on` (1/32" tolerance) is `W_CUT_MISS` with the `at` op that puts
it there. A seat cutting more than 1/3 of the member depth (perpendicular to the member) is `W_BIRDMOUTH_DEPTH`: common
framing practice and IRC R802.7.1 (notching of sawn lumber) keep the section at the bearing; the fix is the op with the
largest seat that keeps 2/3 (rounded down to 1/16"). A seat through the member, or one that falls outside it, is
`E_PARAM`. A `W_OVERLAP` between a member with a birdmouth and a component other than its `on` (the wall under a narrow
plate) prints as its fix the `heel` that lifts the member clear: the current heel minus the overlap height, at most the
depth of `on`.

**Notches.** `{"type": "notch", "at": "top_left" | "top_right" | "bottom_left" | "bottom_right", "w": w, "d": d}` takes a
w × d rectangle out of that corner (left = start; it runs past a cut end); `"at": "top" | "bottom"` with `x` (from the
start) a rectangle along that face. A notch deeper than the member or longer than it is `E_PARAM`.
`{"type": "notch", "by": "<id>"}` subtracts that component's outline after placement with the `voids` machinery (§26.4:
every instance, split along z where the z ranges differ, a part with `id.part`); it is not a placement dependency, so the
notching member may be placed from the notched one (a top plate at `beam@top_left`). One that removes nothing is
`W_CUT_MISS` (id = the notched member, path `components/<id>/cuts/<k>`) with the op that drops it. The summary line ends
with `minus notch <id>`.

**Validation.** Cut objects are checked strictly: an unknown `type` or key, a missing required field, or a bad length is
`E_PARAM` at `components/<id>/cuts/<k>[/<key>]` with an example in the fix; an unknown `on`, `to` or `by` id is
`E_REF_UNKNOWN`. `kerf schema cuts` documents the object; removing a component that a `from`, `to` or cut names is refused
(§14).

| code | level | rule |
|---|---|---|
| `W_BIRDMOUTH_DEPTH` | warning | a birdmouth seat cuts more than 1/3 of the member depth; fix: the op with the largest seat that keeps 2/3 |
| `W_CUT_MISS` | warning | a birdmouth seat off the top of its `on` member, a trim whose line misses its member, a `notch.by` that removes nothing; each fix is an op |

### 27.4 beta.1: nailing and fastener symbols
From the MASS details dogfood run (issue #13): nailing is the main graphic of shear wall, diaphragm, splice and blocking
details, and kerf could not draw it, so agents wrote the schedule into notes and left the drawing bare. Component type
`nailing` draws nails and screws, placed from the members they join.

**Params.**
| param | default | notes |
|---|---|---|
| `members` | required | component ids in driving order: the first holds the heads (face, toenail: the piece nailed; edge, field: the panel), the rest take the points. `id#k` names one array instance; otherwise every instance takes part. They are placement dependencies (§3.2) and cannot be removed while listed (§14) |
| `pattern` | `face` | `face` \| `toenail` \| `edge` \| `field` |
| `size` | required | nail: penny size `4d`..`60d` (common; `"16d box"`, `"16d sinker"`), or shank x length `"0.131x3"`; screw: `SDS25xyz` (1/4" SDS, length code 112 200 212 300 312 412 500 600 800), `"#10x3"`. Unknown: `E_PARAM` listing the forms |
| `fastener` | `nail` | `nail` \| `screw` (SDS and `#N` sizes are screws) |
| `count` | null | face, toenail: fasteners per joint, over all rows and faces (toenail default 2) |
| `spacing` | edge 6, field 12 | o.c. along a row; with `stagger` within each row, the rows offset by spacing / rows |
| `rows` | 1 | face: rows across the joint at (r+1)/(rows+1) of its width |
| `stagger` | true | rows > 1: nails alternate between rows (false: the rows line up, `count` is the total) |
| `along` | longer side | face: world axis (`x` `y` `z`) the rows run along; the drive axis is `W_PARAM` |
| `range` | null | face, toenail: `[from, to]` limits the rows along their axis: Refs or `[x, y]` points (projected on the row) or z values for a row along z |
| `edges` | all | edge: panel edges by direction `left right top bottom front back` (the outward normal's dominant world axis) |
| `faces` | in-plane sides | toenail: side faces of the first member the nails start on; default both side faces whose normal lies in the XY plane |
| `edge_distance` | edge, field 3/8; face 10D | edge, field: from the panel edge to the row; face: from each end of the joint to the first nail |
| `angle` | 30 | toenail: degrees between the nail and the member |
| `through` | false | the points may come out of the far face (plate splice face nails, clinched nails) |
| `text` | generated | note text form override |
`at`, `rotate`, `slope`, `mirror`, `array` and `z` are `E_PARAM` on a nailing (it is placed from its members).
Sizes: common 6d .113 x 2, 8d .131 x 2 1/2, 10d .148 x 3, 12d .148 x 3 1/4, 16d .162 x 3 1/2, 20d .192 x 4 (box and
sinker per the usual tables); screws #6..#14 .138..242, SDS 1/4.

**Placement.** Every member instance is a box: its builder box times its z range, in the frame of the first member's
instance (other members' boxes are the bounding boxes of their corners in that frame). Nails are stored in world
[x, y, z] (head on the face driven through, tip = head + length along the drive direction).
- `face`: for each instance pair (first, second) the joint is the face where the boxes touch (within 0.01") with the
  largest shared area. Heads go on the first member's opposite face, driven square to the joint. Rows run along `along`
  or the joint's longer side, inset `edge_distance` from its ends; `count` alone spreads the nails over that length,
  `spacing` alone fits floor(length / spacing) + 1 centered, both center `count` at `spacing`. Members after the second
  extend the stack when they continue it face to face (length check only).
- `toenail`: the joint as for `face`, direction t from the first member into the second; each nail starts on a side face
  (normal n, perpendicular to t) min(L/3, 3/4 of the member) back from the joint and runs at `angle` to the member:
  d = cos(angle)·t − sin(angle)·n. `count` nails go round-robin over `faces`, evenly spaced across the joint on each face
  ((q+1)/(c+1)); with `spacing`, positions at that spacing along the joint, faces alternating.
- `edge`: the panel's thin axis is the drive axis; the framing is every instance of members[1..] touching that face (the
  side of the first one found; instances on the other side are ignored). One row per edge at `edge_distance` inside it,
  from `edge_distance` to `edge_distance` from the corners at a step of at most `spacing` (both ends included, so corner
  nails coincide and are drawn once). A nail is kept where a framing box lies behind it.
- `field`: one row per framing member behind the panel along the member's longer side on its centerline, at most
  `spacing` apart, without the nails at the panel edge band; a member that carries an edge row, or whose centerline is
  within 3" of a panel edge (the second ply of a chord, a doubled stud at a joint), gets none.

**Drawing.** Pen `steel` (layer of the steel pen), drawn over everything like embedded members.
- Section, plan, elevation: view coordinates are (x, y, depth z), or in a plan (x, −z, depth y). A nail whose projected
  length is under 0.3 of its length is end-on: a filled dot of radius 0.016 paper inch at the head (screws: a circle of
  radius 0.03" with a cross). Otherwise it is its shank from just outside the head to the tip with a head bar across it
  (half width min(0.04", 0.4 × projected length)), a screw adds four thread ticks; toenails come out angled. Identical
  dots draw once and identical strokes are deduplicated.
- Which nails a section shows: a nail is **cut** when the plane passes through its depth range widened, for a row seen
  end-on (row direction within ~45° of the view axis), by half its row spacing (so a row of sill nails along a wall
  that a section crosses shows as one tick per row); it always draws. A nail wholly in front of the plane is dropped. A
  nail behind it draws unless a cut solid covers it (at its head when end-on, at its shank midpoint otherwise) or a solid
  that is not one of its members lies wholly in front of it. Its own members never hide it (splice nails show through
  the plates). Elevations cut nothing: only non-member solids in front hide nails. Nails show only in views that draw
  the nailing and its first member.
- Iso: a dot (radius 0.014 paper inch; screws a circle and cross) where the head is visible (the face it sits on is not
  behind another face); cutaway views drop heads in front of `cut_z`; a crop drops heads outside it.
- Notes: a note targeting a nailing lands on its drawn fasteners (the one nearest their middle first). A note without
  `text` targeting a nailing prints its text form: `text`, else face `(8) 16d NAILS` / `16d NAILS @ 12" O.C.` (+
  ` STAGGERED` for staggered rows, ` IN n ROWS` otherwise), toenail `(2) 16d TOENAILS` / `16d TOENAILS @ 10" O.C.`, edge
  `8d @ 6" O.C. EDGE NAILING`, field `8d @ 12" O.C. FIELD NAILING` (screws: `SCREWS`, `TOE SCREWS`, `EDGE SCREWS`,
  `FIELD SCREWS`). The same text is the nailing's coverage candidate (§25.3), so `meta.requested` items like
  `edge nailing`, `8d @ 6"` or `(2) 16d toenails` are covered by the component. The summary line reads
  `nailing <pattern> <text>: <n> nails (<dia> x <length>), a -> b -> ...` with the fasteners' extent.

**Diagnostics** (warnings, id = the nailing, path `components/<id>`, each with the exact fix; each distinct message once):
- `W_NAIL_MEMBERS`: no instance of the second member touches the first face to face (gap per axis, or overlap); the fix
  lists components that do touch the first member and the `members` update; also when no framing touches an edge/field
  panel, when `range` misses the joint, and when every framing member behind a field panel lies under an edge.
- `W_NAIL_EDGE`: `edge_distance` under 3/8"; outer rows of a face joint closer than max(3/8", 2.5D) to its edges (fix:
  fewer rows); a panel edge with positions that have no framing behind it (length unbacked and the first gap in world
  coordinates; fix: components not in `members` that lie behind the gap, blocking, or the `edges` update leaving it out).
- `W_NAIL_SPACING`: nails of one row closer than 10D (panels: max(2", 10D)); `count` at `spacing` longer than the joint
  (fix: the count that fits).
- `W_NAIL_LENGTH`: the nail stops before the last member; penetrates it less than 6D (fix: the shortest size of the same
  kind that reaches 6D); points come out of the far face of the stack without `through` (fix: the longest size that stays
  inside and reaches 6D, or `through: true`); a toenail whose point ends outside the second member (fix: a shorter size,
  or `angle: 45`).
- `W_NAIL_HIDDEN` (path `views/<id>`): a view that draws the nailing shows none of its fasteners although some lie in its
  crop: in a section or plan the fix puts the cut plane just in front of the members (`cut_z` / `cut_y`, or kind
  `elevation`), in an elevation it names the members in front; both offer `omit`.
- `W_PARAM`: `along` naming the drive axis, a `faces` entry that is the joint face. `E_PARAM`: bad size, fewer than two
  members, a member that is a nailing, an instance out of range, face nailing without `count` or `spacing`, `angle`
  outside 5..85, more than 5000 fasteners (`E_LIMIT`). `E_REF_UNKNOWN`: a member that is not a component (nearest id).
- A nailing has no solid: it takes no part in `W_OVERLAP`, `W_FLOATING` or any other geometry check, and the mesh has
  no nails (2.5D viewers show none).

## 28. v0.2.0-beta.2 round-2 fixes

### 28.1 beta.2: scenario-aware checks and point-list anchors
From round 2 of the MASS dogfood run (issues #20, #21).

**Every check between components runs per view set** (#20). §9's per-view rule (`W_OVERLAP` since §25.4) now covers
every check that compares one component with others: `W_BOLT_EDGE` of a bolt against the `steel` it passes through and
of an `anchor_bolt` against its concrete or masonry host, `W_BOLT_SPACING` between anchor bolts, `W_POST_BASE` (the
member under the plate), the anchorage half of `W_HOLDOWN_ROD` (the rod ends in concrete, masonry or steel) and
`W_BEARING` and `W_BAR_OUTSIDE` (already per view). Two components are compared only when some view draws both; a finding
that holds in some views only ends with ` (in view B)` / ` (in views A, C)`; a document without views checks every
component together. So rods, bolts and studs of scenarios that share one origin may coincide without
warnings, and a post base whose own scenario has nothing under it warns in that view although another scenario's pad
lies there. Checks of one component or one annotation stay document-wide: bolt group `spacing` (`W_BOLT_SPACING`),
`W_WELD_SIZE`, the builders' `W_HANGER_SIZE` / `W_HOLDOWN_ROD` / `W_UNKNOWN_MODEL`, the `W_NAIL_*` family (members are
named). Searches for a neighbour to name in a fix stay in the scenario too: the components a `W_NAIL_MEMBERS` /
`W_NAIL_EDGE` fix offers are drawn in a view with the member, the rod a `W_HOLDOWN_ROD` fix offers is drawn in a view
with the holdown.

**Anchors on point lists** (#21). A shape built from a point list (`concrete` polygon, `solid` `profile.points`, `fill`,
`insulation` / `flashing` with `points`, `membrane`, `connector` `points`, `rebar` path) is placed like a box shape:
- `at.anchor` omitted, or `"origin"`: the local origin (the point `[0, 0]` the literal points are measured from) lands
  on `at.to` plus `offset`, as before. Without `at` the literal points are absolute.
- any other `at.anchor`: one of the 9 box anchors of the shape's local box (the points' extent; a membrane's or bar's
  thickness included) or a named anchor (`vertex_k`, `edge_k_mid`, a bar's `start` / `bend_k`, ...) lands on `at.to`
  plus `offset`; `rotate` / `slope` turn the shape about it. `{"anchor":"top_left","to":"stem@top_right"}` puts the
  polygon's top left corner on the stem's top right corner wherever its points start.
- the anchor cannot apply when the geometry is fixed in place: a point given as a Ref (absolute), a membrane `until`
  (it runs to an absolute point), a strap `on` a member face. Then `E_PARAM` at `components/<id>/at/anchor`: `at.anchor
  "top_left" cannot apply to 'key' (concrete): point 0 of 'points' is the Ref "stem@top_right", an absolute point, so
  moving the shape to put its top_left on at.to would pull it off`; the fix is the op
  `{"op":"update","path":"components/<id>","value":{"at":{"anchor":null}}}` (or literal points only).
- an unknown anchor is `E_ANCHOR_UNKNOWN`, listing `origin`, the box anchors and the named anchors.
- `mirror: true` on a point-list shape, ignored before, is `E_PARAM` with the op that removes it (write the mirrored
  points).
Before beta.2 every point-list shape ignored `at.anchor` (silently). A document that wrote `"anchor": "bottom_left"`
next to points measured from `at.to` moves now unless the points' lower left corner is `[0, 0]`: remove the anchor (or
write `"origin"`).

### 28.2 beta.2: shared notes and layout sizing
From round 2 of the MASS dogfood (issue #22): authors copied the text and citation of a shared keyed note into every view
of a layout, a mismatch warning showed two identical texts (the cite differed), views were bent to fit a letter sheet they
never print on, sheets and scales were found by trial, iso views stayed NTS beside scaled sections, a "see other detail"
view still drew the model, schedule-view notes collided with keyed tags, and a 36x24 PNG needed `--px 3200` to read.

- **Key-only notes.** In a keyed or list view (and a schedule view), a note with a `key` and no `text` takes the text, and
  unless it has its own `cite` the cite, of the first note with that key and a text: in its own view, else in the
  document's `views` in order. It works the same in a single-view export, so a layout's shared entry is written once.
  No such note: `E_PARAM` (`... needs a string "text" (or a "key" shared with a note in any view that has one)`).
- **Shared-note mismatch** (`W_PARAM`, §26.2) names the field that differs: `its text differs` (both texts), `its cite
  differs` (both printed citations, or `no cite`; path `.../cite`), or `its text and cite differ` (both full entries).
  The fix: remove `text` and `cite` from the later note and keep its `key`, or give it its own key.
- **Schedule views in shared notes.** A schedule view's notes (always list notes, §26.1) join the layout's grouping and
  numbering like keyed notes: unkeyed ones join by text and cite or take the lowest free numbers, keyed ones their key.
  They no longer collide with the tags of keyed views.
- **Fit per layout.** `kerf check` raises a view's own `W_VIEW_FIT` (against the style's sheet) only for a view no layout
  names; a view in a layout is fit-checked as a cell of its layout (`W_VIEW_FIT` path `layouts/<id>`).
- **`sheet: "auto"`.** The layout takes one of the standard sheets 11x8.5, 17x11, 22x17, 24x18, 36x24 (smallest first):
  with a numeric layout `scale`, or when no view takes the common scale, the smallest that holds the arranged views and
  the list; with `common`, the smallest at which the common scale (§26.2, picked per sheet) reaches 1" = 1'-0", else the
  smallest that reaches the largest scale any of them fits. None holds it: 36x24 and `W_VIEW_FIT`. `I_LAYOUT_SHEET`
  (info, id = layout, path `layouts/<id>`, printed by `kerf check`) names the sheet, the scale and why. The sheet and PDF
  page are that size. Any other string is `E_PARAM` naming `auto` and the `WxH` form.
- **Iso views at the common scale.** An iso view without its own `scale` in a layout whose other views take a common or
  numeric layout scale is drawn at that scale factor over the layout's `iso_ratio` (number 0.1-4, default 1; else
  `E_PARAM`): 1 draws it at the sections' size, 0.5 at half. Its `SCALE:` line stays NTS. When no view takes a layout
  scale it stays NTS, fitted to its share of the sheet (§26.2). The title block reads one scale only when every cell
  prints the same scale line, else `AS NOTED`.
- **`W_VIEW_FIT` fixes** (layout): the larger sheet is the first of 17x11, 22x17, 24x18, 36x24 that holds it; the fix
  "view V has its own larger scale" names section, plan and elevation views only (not schedule or iso views).
- **"See other detail" views.** A view whose annotations are all `ref`s (at least one) and that has no `only` draws no
  model: its membership set is empty (like a schedule view, §25.4: no geometry checks), its `SCALE:` line prints NTS, and
  without `crop` its crop is the point at the centre of its bubbles (so it is as large as its refs, their text and title).
- **PNG size.** Without `px`, an export is `max(1600, 90 x the page width in paper inches)` pixels wide, at most 6000
  (a 36x24 sheet: 3240 px; a letter or 17x11 sheet: 1600). `px` still sets it.

### 28.3 beta.2: dimensions in plan and elevation

From the round-2 MASS dogfood run (issue #23): rod spacing in plan needed typed plan coordinates because the checks
measured plan dims in model x/y (`W_DIM_ZERO` on two rods that differ only in z), elevations were believed to take no
dims (an extra section carried a clear-span dim), dims sharing end points stacked instead of chaining, and vertical dim
text that did not fit was drawn on the axis of its own line.

**Every flat view kind.** `dim` works in section, plan and elevation views (not iso or schedule), measured in that view's
drawing plane: section and elevation [x, y] (model x, y), plan [x, −z] (§26.3). Refs and anchors resolve in that plane
(plan: box anchors on the plan footprint, named anchors at their x and the instance's z), literal points are in it, `h`
measures along its x and `v` along its y (in a plan: along model z). Everything that measures a dim does it in the view's
plane: the drawing, the default `dir` (dominant axis), `W_DIM_ZERO`, and the length coverage offers for a dim without
`text` (§21).

**Chains.** Two `h` (or two `v`) dims of one view whose `offset`s have the same sign are linked when their spans follow
each other end to end along the measured axis (the end coordinates agree to 1/64") and either the two meeting ends are the
same point (to 1/64") or their dimension lines at the written offsets are within 1/4" (paper) of each other. Linked dims
form a chain (transitively), except that a link that would put two overlapping spans in one chain is dropped, so an overall
dim over a chain stays its own dim and stacks outside it. Every member of a chain is drawn on the chain's outermost written
line: for a positive offset the largest of (higher point + offset) over its members, for a negative one the smallest of
(lower point + offset), the same for x with `v`. So `plate@top_left` → `rod_1@center` → `rod_2@center` →
`plate@top_right` with one `offset` draws E / EQ. / E on one line although the rods sit lower than the plate edge.
`aligned` dims do not chain.
- A chain stacks as one: it is placed by its overall span among the other dims (shortest first), it moves out in whole
  steps together, and a layout-repair push on any member moves the whole chain (the largest push wins). `pin` on any
  member pins the chain at its line. Chaining alone is not a move: `I_ANNOT_MOVED` compares the drawn offset with the
  chain line, and for a member whose written offset differs it names the chain and the written value; a member that only
  follows its chain says "together with its chain (...)".
- Text of a chain member that does not fit between its extension lines goes past a free end of the chain first. Placing it
  beside the line past an end shared with another member counts as a collision (it would read as that member's value);
  when both ends are shared it is centred on its own span one text row out, on the side away from the object, joined to
  the line by a short leader, before the raised places at a shared end are tried.

**Text never on its own line.** Text that does not fit between the extension lines and goes past an end sits beside the
dimension line, which is extended under it (to the text's end plus the text gap), on the same side and at the same gap as
text that fits (above an `h` line, left of a `v` line read from the right), never centred on the line's axis. The raised
places (a text row out at either end, on a short leader) are unchanged.
