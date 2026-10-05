# Kerf Reference Details: Domain Content

Scope: US residential / light-commercial structural details. IRC and IBC only. Three reference details:
(1) prefab wood roof truss bearing on CMU wall, (2) monolithic turned-down footing + slab-on-grade with a sliding-door recess, (3) beam flush with top of double top plate with a steel strap restoring plate continuity.

## 0. How to read this file

- Default code edition: **2021 IRC / 2021 IBC**. 2018 and 2024 differences are called out where known. 2024 section renumbering was NOT checked; re-verify numbers before printing them on drawings for a 2024-adopting jurisdiction.
- Every citation carries `{code, edition, section, confidence}`. Confidence legend:
  - **high** = text or section number seen in a primary/near-primary source this session (ICC, up.codes, manufacturer catalog) or I am certain of it.
  - **medium** = topic and approximate section number agree across sources/memory but the exact text or subsection was not seen.
  - **low** = topic is right, number is a best guess. Marked "verify".
  - Where I was unsure I give a topic only ("verify") rather than a number.
- Manufacturer numbers (Simpson Strong-Tie) came from the C-2013 and C-C-2019 catalog PDFs plus retailer listings. Allowable loads change between catalog editions and by species/duration factor. **Use them only as order-of-magnitude placeholders**; the engine must treat loads as "per current catalog / ICC-ES report" and never print a load on a drawing unless the engineer supplied it.
- Units: inches unless stated. `'-"` architectural notation in text. "Actual" = real finished/surfaced size; "nominal" = trade name.
- Wood: SPF/DF/SYP spec'd per project; all lumber S4S dressed (surfaced dry, 19% max MC) sizes below.

---

## 1. Shared reference data

### 1.1 Actual vs nominal sizes (verified, standard NDS/PS 20 dressed sizes)

| Nominal | Actual (t x d) | Notes |
|---|---|---|
| 2x4 | 1-1/2 x 3-1/2 | studs, plates, truss chords (truss chords are laid flat in the truss plane: 3-1/2 in-plane depth, 1-1/2 out-of-plane) |
| 2x6 | 1-1/2 x 5-1/2 | studs (2x6 wall = 5-1/2 thick), plates, fascia/subfascia |
| 2x8 | 1-1/2 x 7-1/4 | PT sill on 8" CMU (fits within 7-5/8) |
| 2x10 | 1-1/2 x 9-1/4 | |
| 2x12 | 1-1/2 x 11-1/4 | |
| 1x4 / 1x6 / 1x8 | 3/4 x 3-1/2 / 5-1/2 / 7-1/4 | fascia/trim |
| 4x4 / 4x6 / 6x6 | 3-1/2 x 3-1/2 / 3-1/2 x 5-1/2 / 5-1/2 x 5-1/2 | posts |
| LVL (typ.) | ply 1-3/4; widths 1-3/4, 3-1/2, 5-1/4, 7; depths 5-1/2, 7-1/4, 9-1/4, 9-1/2, 11-1/4, 11-7/8, 14, 16, 18 | 1.9E typ. (Fb 2600 psi); a 3-1/2 x D LVL fills a 2x4 wall; 5-1/4 fills a 2x6 wall (5-1/2 with 1/4 shim or 2x ripped) |
| PSL (Parallam) | widths 1-3/4, 3-1/2, 5-1/4, 7; depths 9-1/4 ... 18+ | |
| Glulam (West. species) | widths 3-1/8, 3-1/2 (some), 5-1/8, 6-3/4, 8-3/4; depths in 1-1/2 (or 1-3/8 lam) steps, e.g. 9, 10-1/2, 12, 13-1/2 | 24F-V4 DF typ.; verify depth increments with supplier |
| OSB / plywood | 7/16 (24/16 span rating, Exp.1), 15/32 (32/16), 19/32 (40/20), 23/32 (48/24) | nominal 4x8 sheet = 48 x 96 actual (install with 1/8" edge gap) |
| Gypsum board | 1/2, 5/8 (Type X) | 4'-0" x 8/10/12'-0" |
| Precut stud lengths | 92-5/8 (8'-0" ceiling), 104-5/8 (9'-0"), 116-5/8 (10'-0") | with 1 bottom + 2 top plates (3 x 1-1/2 = 4-1/2): 97-1/8 / 109-1/8 / 121-1/8 overall |

Pitch geometry (rise : 12 run): 4:12 = 18.43 deg, slope length factor 1.054 (12.649/12); 6:12 = 26.57 deg, factor 1.118 (13.416/12). Perpendicular thickness t projects to vertical as t/cos(theta): 3-1/2 chord at 4:12 = 3.69" vertical; at 6:12 = 3.91". Plumb-cut tail drop for overhang O at pitch p:12 is O*p/12 (24" at 4:12 -> 8").

### 1.2 CMU (verified standard modular sizes)

| Item | Value |
|---|---|
| Nominal 8x8x16 | actual **7-5/8 wide x 7-5/8 high x 15-5/8 long**; with 3/8" mortar joint, module = 8" high x 16" long |
| Nominal 6" / 10" / 12" | actual 5-5/8 / 9-5/8 / 11-5/8 wide |
| Face shell (8" unit) | min 1-1/4" thick; web min 1" (ASTM C90) |
| Standard 8" units | 2-core; each cell approx 5-1/2" x 5-3/4" (verify vs. manufacturer), lightweight or normal weight |
| Bond-beam unit | U-shaped "lintel/bond beam block" or open-bottom block; web depressed/removed so horizontal bar sits continuous and is grouted solid |
| Grout | ASTM C476, f'c typ. 2000 psi min (f'm = 1500 psi min for HETA/META data, 2000 psi in HETAL notes); coarse grout for >= 2" spaces, fine grout narrower |
| Reinf. | ASTM A615 Gr 60; bond beam 2-#5 or 1-#5 + #4; vertical #5 @ 32" or 48" typical, in grouted cells |
| Ref. standards | TMS 402/602 (referenced by IBC Ch. 21), ASTM C90 (units), C270 (mortar, Type S/M), C476 (grout) |

---

## 2. General US detail-drafting conventions (what Kerf output should imitate)

### 2.1 Line weights (hierarchy; plotted widths are typical, not code)

| Class | Use | Typical plotted width |
|---|---|---|
| Heaviest / cut profile | material cut by the section plane: CMU outline, concrete outline, cut lumber outline, steel plate | 0.50-0.70 mm (0.020"-0.028") |
| Heavy-medium | rebar in section (solid dot) / long rebar line, fasteners, hardware outline | 0.35-0.50 mm (rebar lines at heavy-medium) |
| Medium | members beyond the cut, leaders, dimension "tick" marks | 0.25-0.35 mm |
| Light | hatch/pattern, dimension and extension lines, grid, joint lines in CMU, hidden thin | 0.13-0.18 mm |
| Hidden | dashed (medium-light): items behind/under the cut | 0.18-0.25 mm |
| Break lines | light; long break w/ zigzag "Z" for long members; short wavy for round/solid | 0.25 mm |
| Center/grid | long-short dash | 0.18 mm |
| Ground line (grade) | heavier than finish lines; often 0.7 mm | 0.5-0.7 mm |

Rules: nothing lighter than 0.13 mm for reproduction; hatches always the lightest class; cut = heaviest; beyond = one step lighter; hidden = dashed at beyond weight. (Source: NCS Uniform Drawing System practice, general; medium.)

### 2.2 Break lines and extents

- Use a break line wherever the member continues beyond the detail's area of interest (wall below, truss up-slope, slab out to the field, beam beyond the post).
- Long break (straight line with a single zigzag/"Z") for dimension lumber, beams, rebar-free slabs; "S" curved break for round/solid members and pipes; **cut line with hatch ends** for earth.
- Break lines are drawn outside the hatch fill; the hatch stops at the break (do not hatch through it).
- Section extents: show enough to see the full load path (roof tie -> plate -> wall; slab edge to ~12" in the field; beam end to the first jack stud + plate laps).

### 2.3 Annotation styles

- **Leader notes**: text with an arrow (closed filled or open arrow; dot on a surface, arrow on an edge), 1 leader per note, text left/right justified at leader shoulder; used in structural details (S-sheets) almost universally.
- **Keynotes**: numeric/alpha tag with a legend (CSI-based or sheet-local); more common on architectural (A-) sheets. Kerf should support both: `leader_note` (inline text) and `keynote` (tag + legend), and an ordering rule (top-to-bottom, left column then right).
- Uppercase text for notes; abbreviations: TYP., U.N.O., MIN., CONT., EA., O.C., E.W., NTS, SIM., CLR., PT, OSB, LVL, GLB, CMU, GWB, CONC., REINF., GRTD., TOS (top of slab), TOW (top of wall), T.O. PLATE, B.O. BEAM.
- Detail callout (title): circle/bubble with detail number (top) over sheet number (bottom), title text beside, scale under title: `1-1/2" = 1'-0"`.
- Cross-reference bubble on parent plans/sections: arrow + detail # / sheet #.

### 2.4 Dimensions

- Architectural style: feet-inches with fractions, e.g. `7-5/8"`, `1'-4"`, `3'-0 1/2"`; zero-feet shown as inches (`8"` not `0'-8"`) in details (structural convention); fractions to 1/16.
- Oblique 45 deg tick marks (architectural) or filled arrowheads (structural/engineering) on dimension lines; extension lines gap ~1/16" from object, overshoot ~1/8"; dimension line text above line, centered, reads from bottom/right.
- String dimensions chained, overall outermost; vertical dims in a detail often given to a datum (e.g. "TOS = 100'-0" or "T.O. WALL").

### 2.5 Text heights (plotted)

| Use | Height |
|---|---|
| Notes / dimensions | 3/32" (min.) to 1/8" |
| Detail titles | 1/8" to 3/16" (user spec: 1/8" titles) bold or all-caps |
| Sheet titles | 1/4"-3/8" |
| Scale text | 3/32"-1/8" |

At model-space scale S (e.g. 1-1/2"=1'-0" -> 1:8) text must be `plotted_height x scale_factor` (3/32" x 8 = 3/4" model units). Kerf should keep text in paper space or apply the scale factor.

### 2.6 Layers (NCS v5/v6 / AIA CAD Layer Guidelines; layer list seen in the AIA/NCS v5 PDF)

Format: `Discipline-Major-Minor1-Minor2-Status`. Verified existing codes include: `S-DETL`, `S-DETL-W2XS` (dimension lumber detail), `S-DETL-PLYW`, `S-FNDN`, `S-FNDN-FTNG`, `S-FNDN-RBAR`, `S-FNDN-RBAR-BOT1/TOP1`, `S-SLAB`, `S-SLAB-CONC`, `S-SLAB-EDGE`, `S-SLAB-OPNG` ("openings and depressions" - ideal for the sliding-door recess), `S-WALL-CMUW`, `S-WALL-WOOD`, `S-BEAM-WOOD`, `S-TRUS`, `S-FSTN` (fasteners/connections), `S-JNTS-CTLJ`/`-CNTJ` (control/construction joints), `S-COLS-WOOD`, `S-COLS-ABLT` (anchor bolts), `S-GRLN` (grade line). Annotation majors: `S-ANNO-TEXT`, `S-ANNO-DIMS`, `S-ANNO-NOTE`, `S-ANNO-KEYN`, `S-ANNO-PATT`, `S-ANNO-SYMB`, `S-ANNO-TTLB`.

Suggested Kerf layer map for these details:

| Content | Layer |
|---|---|
| detail linework (generic) | `S-DETL` (+ `-W2XS` for dimension lumber, `-PLYW` for sheathing) |
| CMU wall | `S-WALL-CMUW` |
| concrete footing/slab | `S-FNDN-FTNG`, `S-SLAB-CONC` |
| rebar | `S-FNDN-RBAR`, `S-FNDN-RBAR-BOT1`, `-TOP1` |
| slab recess | `S-SLAB-OPNG` |
| wood framing / beam | `S-WALL-WOOD`, `S-BEAM-WOOD`, `S-TRUS` |
| connectors/anchors/straps | `S-FSTN` (anchor bolts alt: `S-COLS-ABLT`) |
| hatch patterns | `S-ANNO-PATT` |
| text | `S-ANNO-TEXT`; dims `S-ANNO-DIMS`; leaders/notes `S-ANNO-NOTE`; keynotes `S-ANNO-KEYN`; grade line `S-GRLN` |
| architectural finish items (roofing, fascia, GWB) when drawn | `A-DETL-...` e.g. `A-ROOF`, `A-CLNG`, `A-DETL-GENF` (verify exact A- minors) |

(Confidence: S- codes high - read from the v5 list; `*-ANNO-KEYN`/`PATT` high; A-DETL-* minors low - verify.) Status field (`-N`, `-E`, `-D`) omit for new work.

### 2.7 Hatch / material graphic conventions (US section drawings)

| Material | Standard graphic in section | AutoCAD pattern (DXF HATCH name) | Notes |
|---|---|---|---|
| Concrete (cast-in-place) | random stipple + small irregular triangles/dots | `AR-CONC` (preferred), or `ANSI31` at wide spacing in older drawings | no outline hatch for tiny areas; show rebar as solid dots |
| CMU - ungrouted (hollow) | block outline with 45 deg diagonal hatch on the face shells/webs only; cells left empty; head-joint lines optional every 16" in elevation; bed joint lines every 8" | `ANSI31` (spacing ~ 1/8"-1/4" paper) on units; cells blank | at small scale, may hatch whole width with ANSI31 and show cells as dashed |
| CMU - grouted solid | block diagonal hatch + concrete stipple (AR-CONC) inside the grouted cells, OR single solid diagonal hatch with cell outline dashed | `ANSI31` + `AR-CONC` | bond beam courses: stipple the full course interior; show rebar dots/lines |
| Earth / undisturbed soil | short diagonal lines in groups, OR "ground" symbol; often with 2 line weights | `EARTH` (or `ANSI31` broken) | draw ground line heavy |
| Gravel / crushed rock base | random triangles & circles (irregular angular pieces) | `GRAVEL` | thickness drawn to scale (4") |
| Sand | fine random dots | `AR-SAND` | |
| Compacted fill | same as earth with a "compacted" label; or `EARTH` | | |
| Wood - continuous member (cut, e.g. blocking, plates, studs, joists, beams) | **box with "X"** (two diagonals corner to corner) | none (line pair); non-hatch X | "nominal-sized member, rough lumber / continuous framing" |
| Wood - finish/trim (fascia, casing, soffit trim) | outline with a few grain lines, no X | `WOOD`-like user pattern or none | |
| Plywood / OSB | thin rectangle with parallel lines (plywood plies alternate) or just 2 parallel lines with a centerline "X"-less diag chips for OSB; at small scale a single heavier outline | user-defined | thickness to scale (7/16" = 0.4375") |
| Wood - LVL/glulam | box with X plus lamination lines (optional) | | label LVL/GLB |
| Steel (structural shape) | solid black fill when thin (<= 1/8" plate, strap, anchor bolt, hardware); diagonal hatch for thick | `ANSI32` (steel) or SOLID | straps/ties: single heavy line or solid black at 16 ga |
| Rigid insulation | rectangle with fine parallel diagonal + crosshatch or "grid" | `INSUL`/`ANSI37` | batt = wavy/squiggle line (not rigid) |
| Gypsum board | thin rectangle (1/2" or 5/8" to scale) with light stipple/dashes inside or hatch of short crossing dashes | user pattern; often just 2 lines + light `ANSI` | label 1/2" GWB / 5/8" TYPE X |
| Roofing (shingles) | thin sawtooth/layered band or plain thin band | none/solid | |
| Underlayment / vapor retarder (poly) | heavy dashed or dotted line (single line, no fill), label thickness in note | `DASHED` linetype | 10 mil = 0.010" is a line only |
| Membrane/flashing | heavy single line with thicker weight | | |
| Reinforcing bar | **cut**: solid filled circle (to scale; #5 = 0.625 dia); **long**: heavy continuous line | `SOLID` | #4 = 0.500 dia, #3 = 0.375 dia, #5 = 0.625 dia |

---

# DETAIL 1: Prefab wood truss bearing on CMU exterior wall

## 1.A Typical definition

Exterior bearing wall of 8" grouted/reinforced CMU. Top course is a grouted bond beam with continuous horizontal reinforcing. Pre-engineered wood roof trusses at 24" o.c. (typ.; 16"/19.2" occur) bear on the bond beam, either on a PT wood plate with anchor bolts and hurricane ties, or directly on the masonry with embedded truss anchors (Simpson HETA/META/HHETA/HETAL or DETAL). Truss tails overhang with fascia and soffit.

### Typical drawing scale and extents

- Scale: **1-1/2" = 1'-0"** (1:8) is the default for a bearing wall/roof edge section. 1" = 1'-0" if the whole truss heel + 24" overhang + 3 courses must fit a small frame; 3" = 1'-0" for an enlarged heel/anchor view.
- Extents: top 3-4 courses of CMU (24"-32" of wall) with break line at bottom; truss shown from outer face of wall inward ~24"-36" (break line across the bottom chord and top chord); overhang tail/fascia fully shown (12"-24" overhang typ.); roof sheathing/roofing stopped at break line up-slope; interior ceiling GWB shown to the same inward break; ground not usually shown (if shown, a short stub of wall + finish grade line).

## 1.B Geometry and defaults the engine needs

| Parameter | Typical / default | Notes |
|---|---|---|
| Wall | 8" CMU, 7-5/8 actual | 6", 10", 12" variants |
| Courses shown | 3-4 | top course = bond beam |
| Bond beam height | 7-5/8 unit + 3/8 mortar = 8" | single course; some details use 2 courses |
| Plate (if used) | 2x8 PT flat (1-1/2 x 7-1/4) or 2x6 PT flat (1-1/2 x 5-1/2) centered; width <= wall thickness; bears on 1/2" sill seal/gasket | omit when embedded truss anchors seat the truss directly on the CMU (use TSS moisture barrier/membrane) |
| Pitch | 4:12 or 6:12 (also 3:12 - 8:12) | |
| Truss chords | 2x4 top and bottom (also 2x6); in section: member depth 3-1/2 (2x4) | truss thickness into page 1-1/2 (single ply) |
| Heel type | **standard heel** ~ 3-1/2"-4-1/2" vertical at outside face (2x4 chords, 4:12-6:12); **raised/energy heel** 5-1/2" to 12"+ vertical (full insulation depth over plate) | Heel height comes from the truss design drawing; engine treats as a user parameter (suggested default 5-1/2") |
| Overhang (horizontal, from outer wall face to tail plumb cut) | 12"-24" typ.; IRC R802.11 uplift table assumes <= 24" | |
| Tail cut | plumb (vertical) cut at tail end for fascia | |
| Fascia | 1x6 (3/4 x 5-1/2) over a 2x6 subfascia (1-1/2 x 5-1/2), or single 2x6; top of fascia ~1/2" below plane of top of sheathing | |
| Soffit | lookouts: 2x4 flat (1-1/2 x 3-1/2) @ 24" o.c. spanning wall to subfascia, or none (vented aluminum/vinyl soffit; 3/8" or 1/2" plywood soffit) | |
| Roof sheathing | 7/16" OSB (24/16, Exp.1) or 15/32" plywood; 4x8 panels with H-clips for 24" spans | span-rated; verify per truss spacing |
| Underlayment | #15 or #30 felt, or synthetic underlayment (ASTM D226 felt / D4869 / ICC-ES synthetic; verify) | line only |
| Roofing | asphalt shingles ~ 1/4"-3/8" thick | tile/metal alt. |
| Ceiling | 1/2" GWB (5/8" Type X where fire-rated, e.g. over garage), fastened to bottom chord, 24" o.c. trusses = 1/2" sag-resistant | |
| Vertical rebar | #5 @ 32" or 48" typ. in grouted cells | dowel to foundation, lap per TMS 402 (verify) |
| Horizontal bond beam rebar | 2-#5 (or 1-#5) continuous, cover per TMS 402 (verify cover value) | lapped; hooked/continuous at corners |
| Bond beam grout | full (all cells and the channel); f'c >= 2000 psi | |

## 1.C Component table

| Id | Material | Nominal | ACTUAL section (in) | Orientation / attached to | Extent in detail |
|---|---|---|---|---|---|
| `cmu_course_N` | CMU, normal/lightweight, ASTM C90 | 8x8x16 | 7-5/8 W x 7-5/8 H x 15-5/8 L | running bond, 3/8" bed joints, Type S mortar | 3-4 courses (24"-32") |
| `bond_beam` | bond-beam CMU unit + grout | 8x8x16 | 7-5/8 x 7-5/8 | top course; U-channel grouted solid | continuous along wall (section: 1 course) |
| `bb_rebar_h` | #5 Gr 60 (2 pcs) | #5 | dia 0.625 | continuous horizontal in bond beam channel (2 dots, vertically stacked or side by side with min spacing) | along wall |
| `vert_rebar` | #5 Gr 60 | #5 | dia 0.625 | vertical in grouted cell | full wall height; hook/terminate into bond beam |
| `grout` | grout ASTM C476 | | fills cells | in vertical-bar cells and bond beam | per detail |
| `sill_plate` | PT SYP/HF 2x, treated for ground/ exposure | 2x8 (flat) | 1-1/2 H x 7-1/4 W | on bond beam over sill seal; anchored by bolts | continuous |
| `anchor_bolt` | ASTM F1554 Gr 36 J/L-bolt, nut + 3x3x1/4 (0.229) plate washer | 1/2" (or 5/8") dia | dia 0.500 (0.625), 7" min embed in grout (see hardware) | through plate, in grouted cell; hooked end | @ typ. truss spacing or per engineer |
| `truss_bc` | SPF/DF/SYP No.2 / MSR chord | 2x4 | 1-1/2 x 3-1/2 (flat plane: depth 3-1/2) | bottom chord, bearing on plate/bond beam | extends beyond wall to heel; section: bears full 3"+ on masonry |
| `truss_tc` | same | 2x4 | 1-1/2 x 3-1/2 | top chord, slope 4:12/6:12; plumb tail cut | up-slope to break |
| `truss_heel_plate` | truss connector plate (gang-nail) | 20 ga | profile ~ 3-1/2 x 5" typ. (per truss mfr.) | at heel, both faces | shown optionally |
| `bird_block` | 2x4 or 2x6, flat | 2x4/2x6 | 1-1/2 x 3-1/2 or 5-1/2 | between trusses, atop wall, closes the 22-1/2" gap; **cut = X box** | at each truss bay |
| `vent_block` | same with holes | 2x4/2x6 | 1-1/2 x 3-1/2 | (3) 1-1/2" dia holes or cut-out for soffit-to-attic air; may be notched | optionally replace bird block |
| `baffle` | rigid/ plastic rafter vent | 1" airspace | approx 1-1/2" x 22" wide | between top chord and sheathing at eave | 2-3 ft up slope |
| `fascia` | 1x6 (or 2x6) | 1x6 | 3/4 x 5-1/2 | nailed to tails (via subfascia) | along eave |
| `subfascia` | 2x6 | 2x6 | 1-1/2 x 5-1/2 | nailed to truss tails | along eave |
| `lookout` | 2x4 | 2x4 | 1-1/2 x 3-1/2 | flat, soffit nailer, wall to subfascia | @ 24" o.c. |
| `soffit` | 3/8" plywood or vinyl vent | | 3/8 or 1/2 | under lookouts | wall to fascia |
| `roof_sheathing` | OSB/ply | 7/16" | 0.4375 (OSB) / 0.469 (15/32 ply) | on top chords | stops at break |
| `underlayment` | #30 felt/ synthetic | | ~ 0.03-0.06 | on sheathing; drip edge | line |
| `drip_edge` | galv./ aluminum, 3-1/2" typ. | | 26 ga approx | eave: under underlayment; rake: over | at eave |
| `roofing` | asphalt shingle | | ~ 1/4-3/8 | top | stops at break |
| `ceiling_gwb` | gypsum | 1/2 | 1/2 | on bottom chord, 24" o.c. | to inner break line |
| `insulation` | blown/batt | R-38 to R-60 | depth varies; show with baffle | on ceiling, tapers at heel | line |
| `truss_anchor` | see hardware | | | embedded in bond beam | |

Dependencies for the engine: plate level = top of bond beam + 0 (if no plate) or +1-1/2"; bottom chord bearing = top of plate or top of bond beam; heel top = bearing + heel height; top-chord line starts at heel outer corner and runs at pitch; fascia top = (top-of-sheathing plane at tail end) - ~1/2".

## 1.D Hardware options

Load path alternatives:

1. **Embedded truss anchors (no wood plate)**: strap cast into the grouted bond beam, then nailed/wrapped to the bottom chord. Truss bottom chord bears directly on CMU top (face-shell/grout); a moisture barrier/TSS seat under the chord is typical.
2. **PT plate + anchor bolts + hurricane ties**: plate bolted to bond beam, truss nailed to plate by H-ties (wood-to-wood).
3. **Strap or tie to wall below**: continuous strap/long bolts from truss to bond beam for high-wind regions (engineered).

| Model family | Purpose | Details (catalog data, verify current) | Typical fasteners |
|---|---|---|---|
| **META12/16/18/20/22/24/40** | embedded truss anchor, light | 18 ga; 4" embed in >= 6" concrete beam or 8" nom. grouted block; number = overall length; uplift ~1,450 lb (SP, 160% load dur.) per anchor at (7)-10dx1-1/2 (or (6)-16d); F1 340 / F2 725 lb lateral | 10dx1-1/2 (0.148 x 1-1/2) single-ply; 16d (0.162 x 3-1/2) multi-ply |
| **HETA12/16/20/24/40** | heavy embedded truss anchor | 16 ga; 4" embed in 8" grouted CMU or >= 6" conc. beam; HETA16 = 12" exposed, HETA20 = 16" exposed; uplift ~1,520 (HETA12) to ~1,810 (HETA16/20) lb at 7-9 10dx1-1/2 or 8 16d; lateral F1 340 / F2 725 lb. Min edge distance 1-1/2" (concrete), 2" (masonry). Min f'c 2500 psi / f'm 1500 psi | (7)-(9) 0.148x1-1/2; 16d (0.162x3-1/2) for 2-3 ply |
| **HHETA12/16/20/24/40** | heavier, 14 ga | uplift ~2,235 lb (HHETA16+) ; F1 340-435 / F2 815 | 10dx1-1/2 or 16d |
| **HETAL12/16/20** | HETA with truss seat (moisture barrier) | strap 16 ga, seat 18 ga; embed 5-1/16"; ~1,810 lb uplift single ply; 5 nails into truss seat; parallel-to-wall lateral ~1,975 lb | per table |
| **DETAL20** | high-capacity 2-anchor + seat | 16 ga + 18 ga barrier, centered/flush on top of 8" bond beam or tie beam; ~2,480 lb uplift; 6-10dx1-1/2 in seat + 6 each strap | |
| **TSS2 / TSS2-2 / TSS4** | truss seat moisture barrier for META | 22 ga, widths 1-3/4, 3-1/8, 3-5/8 | 6d commons/ preattached |
| **H2.5A** | single-sided hurricane tie, truss/rafter to wood plate | 18 ga, ~1-3/8" x 5-1/2/6" ; 5-8dx1-1/2 to plate + 5-8dx1-1/2 to rafter; ~ 635-730 lb uplift (SP) w/ 0.131 x 1-1/2 vs x 2-1/2 nails; lateral also | 8d (0.131 x 1-1/2) or SD9 screws |
| **H10A / H10S / H1 / H2A / H3 / H4** | stronger/ double-sided hurricane ties | verify models and loads | verify |
| **Anchor bolts (SB, SSTB, J-bolt)** | plate to bond beam | 1/2" dia min (IRC), 5/8" dia common w/ engineered uplift; min 7" embed in grout/concrete (IRC), 15" hooked used in many masonry details (verify source); plate washer 3x3x0.229 in SDC D | nut + washer |
| **Adhesive anchors (SET-3G / epoxy; Titen-HD in grout-filled CMU)** | post-installed plate anchors | evaluate per ICC-ES | |

Notes for the engine:
- H2.5A/H-ties attach wood to wood; they cannot attach to CMU. Hence the plate + bolt route.
- Embedded anchor strap **overlaps** (no nails where double anchors overlap at the heel).
- If the strap is mislocated >1/8" and <1-1/2" from truss face, shim (truss engineer) - catalog note.
- Provide **moisture barrier / sill seal** between wood and masonry. Truss chord to bear on masonry >= 3" (IRC R802.6 for rafters/ceiling joists on masonry; trusses per truss drawing; medium).

## 1.E Keynotes (typical engineer text) with candidate citations

| # | Text (uppercase) | Code citation {code, edition, section, confidence} |
|---|---|---|
| 1 | 8" CMU WALL (7-5/8" ACTUAL), f'm = 2000 PSI MIN, TYPE S MORTAR, GRADE 60 REINF. | {IBC, 2021, 2101.2 -> TMS 402/602, medium}; {IRC, 2021, R606 masonry, medium} |
| 2 | GROUT BOND BEAM COURSE SOLID, f'c = 2000 PSI MIN GROUT | {TMS 602-16 (grout), ASTM C476, medium}; verify subsection |
| 3 | (2)-#5 CONT. HORIZ. BOND BEAM REINF., LAP 48 BAR DIA. MIN (VERIFY), CLR. COVER PER TMS 402 | {TMS 402-16, Ch. 6 reinforcement details/splices, low - verify numbers} |
| 4 | #5 VERT. @ 32" O.C. IN GROUTED CELLS, DOWEL FROM FOUNDATION, TERMINATE IN BOND BEAM | {TMS 402, low}; {IRC 2021 R606.? seismic wall reinforcement, low - verify} |
| 5 | SIMPSON HETA20 TRUSS ANCHOR EMBEDDED 4" MIN. IN GROUTED BOND BEAM, (9)-10dx1-1/2" TO TRUSS, @ EA. TRUSS, PER MFR. (ICC-ES) | {IRC, 2021, R802.11.1 (truss uplift connection per truss drawings), high}; {IBC, 2021, Ch. 16 ASCE 7 wind uplift, medium} |
| 6 | MASONRY WALL ANCHORED TO ROOF: ROOF-TO-WALL ANCHORAGE PER DETAIL | {IRC, 2021, R606.11 Anchorage (Fig. R606.11(1)-(3) - bolts in hollow masonry: cells grouted solid), high}; {ASCE 7-16 12.11.2 wall anchorage (SDC C-F), medium} |
| 7 | 2x8 PT PLATE WITH SILL SEAL; 1/2" DIA A.B. @ 24" O.C., 3x3x1/4" PLATE WASHER, 7" MIN EMBED IN GROUT | {IRC, 2021, R403.1.6 (min 1/2" dia, 6' max, 7" embed), high}; {IRC, 2021, R317.1 treated wood, medium; IBC 2304.12 verify, low} |
| 8 | PRE-ENGINEERED WOOD TRUSS PER MFR. DWGS, 24" O.C.; MIN. 3" BEARING ON MASONRY | {IRC, 2021, R802.6 bearing 3" on masonry (rafters/joists), medium}; {IRC R802.10 trusses - verify, low} |
| 9 | 2x4 BIRD BLOCKING BETWEEN TRUSSES @ WALL; BLOCK W/ (3)-1-1/2" DIA VENT HOLES | {IRC, 2021, R802.8 lateral support, medium}; {IRC, 2021, R806 roof ventilation (NFA 1/150 or 1/300), medium} |
| 10 | 7/16" OSB ROOF SHTG. (24/16, EXP.1), 8d @ 6" O.C. EDGES / 12" O.C. FIELD | {IRC, 2021, Table R602.3(1) fastener schedule, medium}; {IRC 2021 R803.1 roof sheathing, medium - verify} |
| 11 | #30 UNDERLAYMENT, DRIP EDGE AT EAVES, ASPHALT SHINGLES PER MFR. | {IRC, 2021, R905.1.1 underlayment, medium}; {R905.2.8.5 drip edge, low-verify} |
| 12 | 1x6 FASCIA OVER 2x6 SUBFASCIA; VENTED SOFFIT; 1" MIN. AIR SPACE ABOVE INSULATION | {IRC, 2021, R806.3 vent and insulation clearance, medium} |
| 13 | 1/2" GWB CEILING (5/8" TYPE X AT GARAGE) | {IRC, 2021, R702.3 gypsum board, medium}; {R302.6 dwelling/garage separation, medium} |
| 14 | FINISH GRADE / MASONRY ABOVE GRADE: FOUNDATION WALL EXTENDS >= 6" ABOVE GRADE (4" W/ MASONRY VENEER) | {IRC, 2021, R404.1.6, high} (applies if grade shown) |

Typical engineer's callouts that are NOT code sections but conventional: "TYP. U.N.O.", "SEE TRUSS ENGINEERING FOR HEEL HEIGHT", "ANCHOR PER MFR'S INSTALLATION INSTRUCTIONS", "VERIFY ROOF TIE DOWNS WITH DESIGN WIND SPEED", "SEE PLAN FOR SPACING".

## 1.F Engine modeling and annotation notes

- Draw order: CMU courses -> bond beam -> grout fill -> rebar -> plate/sill seal -> anchor bolt -> truss chords/heel -> blocking -> sheathing/roofing -> fascia/soffit -> ceiling.
- Grouted vs ungrouted: cells carrying vertical bars are grouted; the top bond beam course is fully grouted; the rest of the face shell can be shown hollow.
- Embedded anchor: draw strap in grout (hidden/dashed or thin solid through the hatch) with note "4" EMBED"; strap wraps up face of bottom chord; exposed length = model number minus 4"/embed (e.g. HETA20: 16" exposed, 4" embed).
- Keep nails/bolts as small symbols (heavy dash for nails/bolt line) and dimension positions only for anchor embed, plate width, heel height, overhang.
- Add overall dims: wall thickness 7-5/8, heel height, overhang, roof pitch triangle (4/12), bond beam depth 7-5/8.

---

# DETAIL 2: Monolithic turned-down footing + slab-on-grade with depression at sliding door

## 2.A Typical definition

Monolithic (one-pour, "monopour"/"mono-slab") slab: slab and thickened edge (turndown) placed together. At an exterior sliding glass (patio) door a **depression (recess)** in the top of the slab holds the door track/sill so the finish floor is flush or near-flush with the exterior; the recess slopes to the exterior and is flashed per door manufacturer.

### Typical drawing scale and extents

- Scale: **3/4" = 1'-0"** or **1" = 1'-0"** for the slab edge; **1-1/2" = 1'-0"** for the enlarged recess; 3" = 1'-0" for the track/sill detail.
- Extents: from exterior finish grade ~12" outside the slab edge, across the turndown, interior ~18"-24" of slab (break line), with the wall/plate and door frame sill sketched above, ground hatch below, gravel base and vapor retarder lines drawn full length of the section.

## 2.B Dimensional defaults (typical; engineer chooses)

| Parameter | Typical | Rule/notes |
|---|---|---|
| Slab thickness | 4" actual (3-1/2" absolute IRC min) | R506.1; 5" for garages/heavy loads |
| Footing width (turndown) | 12" (min IRC) to 16"-18" | R403.1.1 min 12" width x 6" depth; Table R403.1(1) governs by soil bearing/stories |
| Footing depth (top of slab to bottom of footing) | 12"-24" typ. (e.g. 18" total, = slab 4" + turndown 14") | bottom must be >= 12" below undisturbed ground surface AND below frost line (R403.1.4/R403.1.4.1) |
| Frost depth | local (0" in warm climates to 48"+); note "BOTTOM OF FOOTING 12" MIN BELOW UNDISTURBED GRADE OR FROST DEPTH, WHICHEVER GREATER" | R403.1.4.1; R403.3 frost-protected shallow foundations alternative |
| Top of slab above exterior grade | 6" (4" w/ masonry veneer) typ.; recess then sits 1-1/2" below | R404.1.6 (foundation walls) / R317.1 6" wood-to-ground clearance |
| Compacted base | 4" clean graded sand/gravel/crushed stone passing 2" sieve | R506.2.2 |
| Vapor retarder | **10 mil** Class A (ASTM E1745), laps >= 6", under slab above base (2021 IRC); 6 mil in 2018 IRC and in IBC 1907 (verify 2021 IBC) | R506.2.3 |
| Slab reinforcement | none required by IRC for non-seismic; common: 6x6-W1.4xW1.4 WWF at mid-depth/upper third, or #3 @ 18" o.c. E.W., or #4 @ 24" o.c., or synthetic/steel fibers | engineer's choice; chairs/ supports |
| Footing rebar | non-seismic: plain allowed by IRC; typical (2)-#4 or (2)-#5 continuous bottom + (1)-#4 top; SDC D0-D2: 1-#4 top + 1-#4 bottom OR 1-#5 or 2-#4 in middle third of footing depth | R403.1.3 (turned-down monolithic: 1-#4 top and bottom, or 1-#5 / 2-#4 in middle third; hooks for non-monolithic dowels 12" below top of slab) |
| Cover | 3" min to earth (cast against and permanently in contact with ground); slab bars: 1-1/2"-2" from top or at mid-depth | ACI 318-19 20.5.1.3.1 (high); IRC refers to ACI 332 / ACI 318 |
| Recess (depression) | **1-1/2" deep** (some 2") x width = sliding-door frame sill depth + margin (e.g. door frame 4-9/16" for 2x4 wall, 6-9/16" for 2x6 wall + 1/2"-2" margin each side) typ. 6"-9" wide; slope 1/8" to 1/4" per ft to exterior | no IRC number; follows door mfr and flashing |
| Concrete cover to recess bottom | recess reduces slab thickness at that strip to 2-1/2"; locally thicken or place the recess over the turndown so full depth remains | important for the engine: the recess usually sits above the thickened edge or within the 4" slab only if >= 2-1/2" remains (engineer approval) |
| Concrete strength | 2500 psi min foundation; 3000 psi common for exterior flatwork/garage; 3500 severe weathering | IRC Table R402.2 (verify exact values/regions), medium |
| Slope at exterior | grade falls >= 6" within first 10' away from foundation | R401.3 (high-ish) |
| Control joints | saw cut 1/4 slab depth, spaced ~ 2-3x slab thickness in feet (e.g. 4" slab -> 10'-12'), within 6-18 h | ACI 302.1R / ACI 360 (practice; not IRC) |
| Fill | clean sand/gravel <= 24" fill; earth <= 8" fill unless approved | R506.2.1 (high-ish) |

Interior slab at door end: the recess is cut into the slab edge region between the interior flush finish line and the exterior face of the wall plate line. The wall bottom plate at a door is interrupted; the door sill sits in the recess. Bolt location: IRC requires an anchor bolt within 12" (and >= 7 bolt diameters, i.e., 3-1/2" for 1/2" bolts) of each plate end, so bolts flank the door opening.

## 2.C Component table

| Id | Material | Nominal | ACTUAL section (in) | Orientation / attached to | Extent in detail |
|---|---|---|---|---|---|
| `slab` | concrete, f'c 3000 psi | 4" | 4 thick (3-1/2 min) | monolithic with turndown; top at +6" above grade | width of section (break line in field) |
| `turndown` | concrete | 12" x 18" | 12 W x (14 + 4) = 18 total depth (typ.) | under exterior wall line; outside face plumb or 1:1 forming | full |
| `footing_rebar_bot` | #4 or #5 Gr 60 (2 pcs) | #4 | dia 0.500 (#4) / 0.625 (#5) | 3" clr. from bottom & sides | long: heavy line; cut: dots |
| `footing_rebar_top` | #4 (1 pc) | #4 | dia 0.500 | 3" clr. to side, ~2"-3" below top of slab | dots |
| `slab_wwf` | WWF 6x6-W1.4xW1.4 or #3 @ 18" | | W1.4 ~ 0.135 dia wire; #3 = 0.375 dia | mid-depth or upper third, on chairs | line through slab |
| `vapor_retarder` | polyethylene 10 mil (0.010) | | 0.010 | between base and slab; wrapped up the edge? (not at turndown) | full length; dashed heavy line |
| `base_course` | crushed stone/gravel | 4" | 4 thick | on compacted subgrade | full slab width; hatch GRAVEL |
| `subgrade` | compacted earth/fill | | | 95% Std. Proctor typ. (geotech) | below; hatch EARTH |
| `door_recess` | cut-out in slab | | 1-1/2 deep x ~6-9 wide, sloped | at exterior wall line | depression polygon; label "SLAB DEPRESSION" |
| `sill_plate` | PT 2x6 (1-1/2 x 5-1/2) on 2x6 wall; PT 2x4 (1-1/2 x 3-1/2) on 2x4 | 2x6 | 1-1/2 x 5-1/2 | on slab, sill seal; interrupted at door | wall segments either side of recess |
| `anchor_bolt` | 1/2" dia J/L-bolt or SSTB/SB | 1/2" | dia 0.500; embed 7" min into concrete | middle third of plate, 3x3x0.229 washer | @ <= 6'-0" o.c. |
| `sill_seal` | foam gasket/ caulk | | 1/4-3/8 | under sill plate | |
| `exterior_grade` | earth | | slopes away | | |
| `door_sill_track` | extruded aluminum/ vinyl | | typ. sill ~ 1-1/8" to 1-1/2" high, 4-9/16 or 6-9/16 deep | in recess; supported on shims/pan flashing | symbolic outline |
| `edge_insulation` (opt.) | rigid XPS 1-2" | | 1-2 | vertical at turndown face (climate zone) | |

## 2.D Hardware / accessories

| Item | Family | Purpose |
|---|---|---|
| Anchor bolt | SB 1/2x10 (J-bolt), SSTB16/20/24/28, or generic ASTM F1554 Gr 36 | sill plate to slab edge, min 7" embed (IRC) |
| Holddown | HDU/PA/STHD (embedded) | shear wall ends, engineered (SDC D) |
| Plate washer | 3x3x0.229 (SDC D, braced wall) | R602.11.1 (verify text for 2021: requirement for SDC C townhouses D0-D2) |
| Rebar chairs/bolsters | plastic/ steel | place WWF/rebar |
| Control joint / isolation | 1/2" fiber at column pads/walls | slab cracking |
| Slab-edge flashing/ pan | self-adhered membrane, sill pan, backer rod/sealant | door sill drainage (R703.4 flashing, medium) |

## 2.E Keynotes with candidate citations

| # | Text (uppercase) | Citation {code, edition, section, confidence} |
|---|---|---|
| 1 | 4" CONC. SLAB-ON-GRADE, f'c = 3000 PSI, ON 10 MIL VAPOR RETARDER, LAPS 6" MIN., ON 4" COMPACTED GRAVEL BASE | {IRC, 2021, R506.1/R506.2.2/R506.2.3, medium-high}; 2018: 6 mil; {Table R402.2 strengths, medium} |
| 2 | THICKENED EDGE (TURNDOWN) FOOTING 12" W x 18" DEEP BELOW TOP OF SLAB; BOTTOM OF FTG. MIN. 12" BELOW UNDISTURBED GROUND AND BELOW FROST DEPTH | {IRC, 2021, R403.1.1 (12" x 6" min size), high}; {R403.1.4 & R403.1.4.1, high} |
| 3 | (2)-#5 CONT. BOTTOM (1)-#4 CONT. TOP, 3" CLR. TO EARTH | {IRC, 2021, R403.1.3 (SDC D0-D2 turned-down: 1-#4 top+bottom or 1-#5/2-#4 mid), high}; {ACI 318-19, 20.5.1.3.1 (3" cast against ground), high}; subsection number for turned-down in 2021 verify (R403.1.3.3 in 2015/2018) |
| 4 | 6x6-W1.4xW1.4 WWF AT MID-DEPTH OF SLAB (OR #3 @ 18" O.C. E.W.) ON CHAIRS | engineer's practice; {ACI 360R/302.1R, low-verify} |
| 5 | SLAB DEPRESSION 1-1/2" DEEP x 8" WIDE FOR SLIDING DOOR SILL, SLOPE 1/4" PER FOOT TO EXTERIOR; SEE DOOR MFR. FOR SILL DEPTH | {IRC, 2021, R703.4 flashing, medium}; {IRC 2021 R311.3.2 floor elevation at exterior doors (landing <= 7-3/4" below threshold), medium}; install per ASTM E2112 / AAMA 2400-type guidance, low |
| 6 | PROVIDE SILL PAN FLASHING, SEAL TRACK TO CONC. | {IRC, 2021, R703.4, medium} |
| 7 | TOP OF SLAB 6" MIN. ABOVE FINISH GRADE (4" W/ MASONRY VENEER) | {IRC, 2021, R404.1.6 (foundation walls), high for walls; slab application is practice, low}; {R317.1 6" wood siding clearance to ground, medium} |
| 8 | FINISH GRADE SLOPES AWAY 6" MIN. IN FIRST 10'-0" | {IRC, 2021, R401.3, medium-high} |
| 9 | 1/2" DIA. ANCHOR BOLT @ 6'-0" O.C. MAX, 7" MIN. EMBED, WITHIN 12" OF PLATE ENDS (NOT LESS THAN 7 BOLT DIA.), 3x3x0.229 PLATE WASHER IN SDC D | {IRC, 2021, R403.1.6 / R403.1.6.1 / R602.11.1, high} |
| 10 | PRESSURE-TREATED SILL PLATE OVER SILL SEAL | {IRC, 2021, R317.1 (sills on slab in contact with earth), high-ish}; {IRC R317.1.2 ground contact, high} |
| 11 | SOIL TERMITE TREATMENT / BARRIER AS REQ'D BY LOCAL JURISDICTION | {IRC, 2021, R318.1 (methods: chemical, bait, PT wood, physical barriers), high} |
| 12 | COMPACTED FILL PER GEOTECH; FILL DEPTH <= 24" SAND/GRAVEL, <= 8" EARTH | {IRC, 2021, R506.2.1, medium-high} |
| 13 | CONTROL JOINTS: SAW CUT 1/4 SLAB DEPTH @ 10'-0" O.C. MAX. WITHIN 12 HRS | {ACI 302.1R-15 / ACI 360R, low; not IRC} |
| 14 | ADD (1)-#4 x 4'-0" DIAGONAL BAR AT RE-ENTRANT CORNERS OF RECESS | engineering practice; no code section, N/A |

## 2.F Notes for the engine

- Datum: top of slab = 0 (finished interior); exterior grade = -6" (or as parameter); recess bottom = -1-1/2" at the door line, sloping to exterior at 1/4"/ft over its width (draw slope).
- Draw the vapor retarder as a continuous thick-dashed line from the interior across the base course to the **inner face of the turndown** and under the footing (some details lap up the inside face of the turndown; either is acceptable, note it).
- If frost depth is provided, auto-set turndown depth = max(12" below grade, frost depth) and show a "FROST DEPTH" dimension; stop the drawing at a break line.
- The recess should auto-check remaining slab thickness (>= 2-1/2") and warn/switch to deeper local thickening or a full-depth turndown beneath.
- Ground hatching on the exterior side: heavy ground line from outside face down to bottom of footing level (frost).
- Exterior landing/step beyond the door (not drawn) - include a note "SEE SITE PLAN FOR PATIO".

---

# DETAIL 3: Beam flush with top of double top plate; strap restores plate continuity

## 3.A Typical definition

An engineered beam (LVL/PSL/glulam) or built-up 2x header is set in a stud wall so its **top is flush with the top of the double top plate**. Below it, king and jack studs (or a post) carry it; above it, roof trusses/rafters/joists bear directly on the beam and plates. Because the beam replaces the double top plate over its span, the plates are interrupted at each beam end. A galvanized strap lapped across the plate-to-beam joint (and nailed to both) maintains tension continuity (plate chord/drag/strut, wall-to-wall tie).

### Typical drawing scale and extents

- Scale: **1-1/2" = 1'-0"** (elevation of the beam end and strap) and **3" = 1'-0"** for a nailing enlargement; 3/4" = 1'-0" if the whole opening elevation is shown.
- Extents: wall elevation (looking at the wall face, sheathing removed) showing one beam end: top plates both sides with breaks 24"+ beyond the strap, king + trimmer studs, beam end, strap cut through (transparent) with a nailing pattern; a companion section through the wall (plan or section) shows beam width vs. wall width, plates, trusses bearing.

## 3.B Geometry the engine needs

- Wall: 2x4 (3-1/2 thick) or 2x6 (5-1/2 thick), studs @ 16"/24" o.c., stud length typically 92-5/8" (8'-0" ceiling) with 3 plates -> overall 97-1/8" (see table 1.1).
- Beam top = top of upper top plate = wall datum H (e.g. 97-1/8" above subfloor for 8'-0" ceiling).
- Beam bottom = H - D. e.g. 9-1/4" LVL -> 87-7/8" above subfloor; jack stud length = 87-7/8 - 1-1/2 (bottom plate) = **86-3/8"**; 11-7/8" LVL -> bottom at 85-1/4", jack 83-3/4".
- Beam width should equal wall thickness (flush faces for sheathing/ drywall): 3-1/2" LVL in 2x4 wall; 5-1/4" LVL (+ 1/4" shim) or 2-ply 1-3/4" + 1x ripped? in 2x6 wall (5-1/2"). For 2-ply 1-3/4" LVL = 3-1/2".
- Top plates: lower plate and upper plate each end at the beam end (butt joints in the same location at both plates on the beam end -> no lap). To obey the 24" offset rule, stagger plate butt joints where the plates occur away from the beam (beyond the strap).
- Wall height flush: the roof framing bears on the beam top (uplift/ties attach to beam: H2.5A can nail into the beam top plate/edge only with proper fasteners; many LVL beams have limits on face-nail edge distances - mfr. spec).

## 3.C Component table

| Id | Material | Nominal | ACTUAL section (in) | Orientation / attached to | Extent in detail |
|---|---|---|---|---|---|
| `beam` | LVL 1.9E (or PSL/GLB/ built-up 2x) | (2) 1-3/4 x 9-1/4 LVL | 3-1/2 W x 9-1/4 D | flush top; bears on jack studs; plies nailed per mfr. (e.g., 3 rows 16d @ 12") | entire opening span; shown half/one end |
| `plate_lower` | 2x4/2x6 SPF | 2x4/2x6 | 1-1/2 x 3-1/2 / 5-1/2 | wall top plate, lower; ends at beam | wall length to break |
| `plate_upper` | 2x4/2x6 | 2x4/2x6 | 1-1/2 x 3-1/2 / 5-1/2 | upper top plate, flush w/ beam top | wall length to break |
| `king_stud` | 2x4/2x6 | | 1-1/2 x 3-1/2 / 5-1/2 | full-height; end-nailed to beam with 4-16d (IRC) | each end of beam: 1 per <= 8' span, 2 per > 8' |
| `jack_stud` (trimmer) | 2x4/2x6 | | 1-1/2 x 3-1/2 / 5-1/2 | beam bearing; length = beam bottom - bottom plate | per Table R602.7(1)/(2) (number depends on span; engineered per beam reaction) |
| `bottom_plate` | 2x PT/ non-PT | | 1-1/2 x 3-1/2 / 5-1/2 | | |
| `cripple` | 2x | | | not applicable above flush beam (no cripples) | N/A |
| `post` (opt.) | 4x4, 4x6, 6x6 or built-up | 4x4 | 3-1/2 x 3-1/2 | under beam end where reaction too high for jack studs | |
| `post_cap` | steel | see hardware | | beam to post | |
| `strap` | galv. steel strap | CS16 / CMST / MSTA / LSTA / MSTC | CS16: 16 ga (0.0598) x 1-1/4 W; CMST14: 14 ga x 3 W; MSTC: 16 ga (0.054) x 3 W (coined slots) | over plate-to-beam joint (top face or both side faces) | cut length = 2 x end length + clear span (0 at a butt joint) |
| `wall_sheathing` | 7/16" OSB | | 7/16 | shown beyond or removed | |
| `truss/rafter` | roof members | | | bear on top of upper plate/beam | optional |

## 3.D Hardware

| Model family | Purpose | Data (catalog, verify) | Fasteners |
|---|---|---|---|
| **CS16** (coiled strap) | light strap, cut to length | 1-1/4" wide (width verify), 16 ga, 150' coil; end length **11"** each side with (20)-0.148x2-1/2 (10d x 2-1/2) nails -> allowable tension **1,705 lb** (DF/SP); (22)-0.131x2-1/2 -> 13"; alt. nailing reduces load proportionally | 10d common / 10dx2-1/2 (0.148 x 2-1/2) or 0.131 x 2-1/2; may use SD9 screws |
| **CS14** | heavier coil | 14 ga, 100' coil; end length 15" w/ (26) 0.148x2-1/2 -> 2,490 lb | same |
| **CS20** | light | 20 ga, 250' coil; (12) 0.148 x 2-1/2, end 6" -> 1,030 lb | |
| **CMST14** | 3" strap | 14 ga, 52-1/2' coil; 3" wide; end length 26" w/ (56) 0.162x2-1/2 -> 6,475 lb; 30"/(66) for SPF | 16d sinker (0.148 x 2-1/2/3-1/4) or 0.162 x 2-1/2; every other hole if wood splits |
| **CMST12** | 3" strap | 12 ga, 40' coil; 33" end w/ (74) 0.162x2-1/2 -> 9,215 lb | |
| **CMSTC16** | 3" strap, coined slots | 16 ga, 54' coil; 20" end w/ (50) 0.148x3-1/4 -> 4,690 lb (cut to length); **listed by Simpson as meeting IRC R602.6.1 (reinforce cut top plate) and IBC 2308.9.8 (2018 numbering)** | 0.148 x 3-1/4 sinker |
| **MSTA9...MSTA49** | edge-nail strap, 1-1/4" wide | e.g. MSTA24: 24" long, (18) 0.148 x 2-1/2 -> ~1,640 lb DF/SP; MSTA36: 36", (26) -> ~2,050 lb; verify gauge (LSTA lighter) | 0.148 x 2-1/2 |
| **LSTA9...LSTA36** | light edge-nail strap | e.g. LSTA24: 24", (18) 0.148x2-1/2 -> ~1,235 lb; LSTA36 -> ~1,640 lb | same |
| **MST37/48/60/72** | high-cap strap | 3" wide x 37-1/2" to 72"; MST48 (32)-0.162x2-1/2 -> 3,950 lb (floor-to-floor) | 16d sinker/ 0.162 x 2-1/2 |
| **Post caps/ connectors** | beam-to-post | BC/BC4/BC6 (post cap), CCQ/ECCQ (column cap), LCE (light cap), PC; verify sizes/ loads | per mfr. |
| **LVL screws** | beam ply connection | SDS 1/4x3-1/2 or 3-1/2" Strong-Drive SDWS timber | per beam mfr. |
| **H2.5A / H10** | truss-to-plate/beam tie | where truss bears on beam: nails into beam edge/top plate; verify LVL fastener edge distance | |

Notes:
- Nail length must be 2-1/2" for 0.148 nails so the points penetrate the lower plate (plates are 1-1/2 + 1-1/2 = 3" stack); on the beam, 2-1/2" nails go into 1-3/4" ply + 3/4" next ply (verify min penetration with LVL mfr).
- Sheathing can interfere with side-face straps; when a strap goes over wood structural panel sheathing, use >= 2-1/2" nails (catalog note: "when nailing the strap over wood structural panel sheathing use 2-1/2" long nail minimum").
- Wood shrinkage after strap install across horizontal wood members can buckle the strap outward (catalog note) - consider placing on top (cover) or accept; engineered lumber doesn't shrink much.
- Nail every hole in the specified count each side of the joint (half the nails in each member); reduced nails scale the allowable proportionally.

### Typical strap selection by use

1. **Light/ conventional framing (IRC)**: CS16 x 22" (11" each side of a butt joint, (20)/side at max load; frequently (10)-10d each side at code-equivalent load) or MSTA24/LSTA24 (24" total, 9 nails each side).
2. **Equivalent to IRC Table R602.3.2 plate**: 3" x 12" x 0.036" galv. plate with 12-8d box nails at butt splice in SDC A-C; 3x16x0.036 w/ 18 nails in higher SDC (verify).
3. **Engineered chord/collector**: CMST14/CMST12/MST48 with nail count to demand; clear span across the beam if only tension chord is required (nails not required in clear span).

## 3.E Code content (verified/semi-verified)

| Topic | Citation |
|---|---|
| **IRC top plate rule (verified text)**: "Wood stud walls shall be capped with a double top plate installed to provide overlapping at corners and at intersections with other partitions. End joints in double top plates shall be offset not less than 24 inches (610 mm)." Plates >= 2" nominal and >= stud width. | {IRC, 2021, R602.3.2, **high**} |
| **Single top plate exception (verified)**: single plate allowed if tied at corners, intersecting walls and in-line splices per Table R602.3.2; rafters/joists centered over studs within 1"; **omission of the top plate over headers is permitted where the headers are adequately tied to adjacent wall sections in accordance with Table R602.3.2** | {IRC, 2021, R602.3.2 Exception, **high**} |
| **Table R602.3.2 (single top plate splice connection details)**: SDC A-C & lower D: corners/ intersections 3"x6"x0.036" galv. plate with 6-8d box (2-1/2"x0.113") nails; butt joints 3"x12"x0.036" plate with 12-8d box nails. Higher SDC D: 3x8x0.036 w/ 9 nails (corners) and 3x16x0.036 w/ 18 nails (butt). **Exact per-side count and SDC split verify.** | {IRC, 2021, Table R602.3.2, medium} |
| **IBC counterpart**: double top plate **end joints offset >= 48"** (not 24"); single plate alt. with 3x6x0.036 plate, six 8d box nails each wall segment. (IBC vs IRC difference - verified) | {IBC, 2021, 2308.5.3.2, **high**} (2018 numbering 2308.9.3.2, medium) |
| Top-plate fastening: double top plates face-nail 16d @ 16" o.c.; lapped splice: min 24" offset, 8-16d face nails in the lap; studs end-nail 2-16d | {IRC, 2021, Table R602.3(1), medium-high} |
| Cut/ drilled top plate > 50% width: galvanized tie >= 0.054" (16 ga) x 1-1/2" wide, extend >= 6" beyond opening each side, with >= eight 10d (0.148 x 1-1/2) nails each side (some code editions say 16d - verify); exception when entire side is covered by WSP | {IRC, 2021, R602.6.1, **high** (text via up.codes TX 2024/2021)} - **closest code analog for "strap across interrupted top plate"** |
| Headers: single headers framed with a flat 2x member top and bottom, face-nailed (10d @ 12"), supported at each end by jack studs or approved framing anchors per Table R602.7(1)/(2); king/full-height stud adjacent end-nailed to header with four 16d | {IRC, 2021, R602.7.1, R602.7.5, medium-high} |
| Engineered beam/header sized per manufacturer, ICC-ES evaluation; engineered design | {IRC, 2021, R301.1.3 / R602.7, medium} |
| Diaphragm chord/collector: top plate continuity where beam breaks plates (the actual reason for the strap) | {ASCE 7-16, 12.10.2/12.10.2.1 (collectors), medium}; {AWC SDPWS 2021, Ch. 4 (diaphragm chords/collectors), medium}; no explicit IRC text for "flush beam" - topic only |
| Roof uplift tie at beam/plate: connections per Table R802.11 or truss drawings | {IRC, 2021, R802.11, R802.11.1, high} |
| Bearing/ lateral stability of beam | {NDS 2018 bearing/ 3.4? , low-verify}; manufacturer's LVL guide, no code number |

## 3.F Keynotes with candidate citations

| # | Text (uppercase) | Citation |
|---|---|---|
| 1 | (2) 1-3/4" x 9-1/4" LVL 1.9E BEAM, FLUSH WITH T.O. DBL. TOP PLATE; NAIL PLIES W/ 3 ROWS 16d @ 12" O.C. (VERIFY MFR.) | {IRC, 2021, R602.7 / R301.1.3 engineered, medium}; mfr. ESR |
| 2 | (2) KING STUDS + (2) JACK STUDS EACH END, FULL HT.; END NAIL KING TO BEAM W/ (4)-16d | {IRC, 2021, R602.7.5, medium-high} |
| 3 | DBL. TOP PLATE INTERRUPTED AT BEAM; STRAP: SIMPSON CS16 x 22" (11" EACH SIDE OF JOINT) AT TOP OF PLATE/BEAM, (20)-10d x 2-1/2" EACH SIDE (TOTAL 40) -- or LSTA24 / MSTA24 | {IRC, 2021, R602.3.2 Exception + Table R602.3.2, medium}; {R602.6.1 analog, medium}; Simpson catalog C-C-2019 |
| 4 | STRAP EACH FACE: (2) CMSTC16 x 24", (14)-0.148x3-1/4 EA. SIDE (ALT. when strap on face) | {R602.6.1, medium} |
| 5 | TOP PLATE END JOINTS OFFSET 24" MIN. (48" MIN. IBC) | {IRC, 2021, R602.3.2, high}; {IBC, 2021, 2308.5.3.2, high} |
| 6 | FACE-NAIL DBL. TOP PLATE: 16d @ 16" O.C.; 8-16d IN LAP | {IRC, 2021, Table R602.3(1), medium} |
| 7 | BEAM END BEARING: 3-1/2" MIN. ON JACK STUDS (BEARING STRESS CHECKED, Fc-perp) | {NDS, 2018, 3.10 bearing, low-verify}; no IRC number |
| 8 | POST CAP: SIMPSON BC4 (OR CCQ) FOR 4x4 POST, (VERIFY) | mfr. |
| 9 | TRUSS/RAFTER BEARS ON TOP OF BEAM & PLATES; HURRICANE TIE @ EA. TRUSS (H2.5A) NAIL INTO BEAM PER LVL MFR. EDGE DIST. | {IRC, 2021, R802.11.1, high} |
| 10 | EXTERIOR SHEATHING 7/16" OSB OVER BEAM, 8d @ 6" O.C. EDGES | {IRC, 2021, Table R602.3(1), medium} |
| 11 | DO NOT CUT, NOTCH OR DRILL LVL EXCEPT PER MFR. | mfr. guide; {IRC 2021 R502.8 sawn-lumber notching only, low} |
| 12 | LATERAL SUPPORT: TOP OF BEAM RESTRAINED BY ROOF FRAMING/ SHEATHING; BLOCK AT 8' O.C. IF NOT | {NDS 2018 3.3.3 beam stability, medium-low} |

## 3.G Engine notes

- The beam and the plates share the same top elevation: only draw the plates beyond the beam ends; at the joint draw the strap lying above them as a thin solid-black (or heavy) line with the nail pattern ticks (small crosses) at the specified count.
- Straps: draw end length = specified lap each side; label "(20)-10d x 2-1/2" EA. SIDE".
- If strap goes on the side faces, place in the section as a dashed line (hidden) on the elevation, solid in section.
- Use stud/plate spacing consistent with the 24" offset: place the next top-plate butt >= 24" away from the beam end (>= 48" in IBC details).
- Beam bearing lengths: IRC does not cap jack stud count by reaction; engineer provides. Offer typical: 2 jack + 2 king studs at end of beams up to ~6'-8' spans; Table R602.7(1) governs.

---

# 4. Open items: facts NOT verified in this pass (all flagged inline too)

1. **2024 IRC/IBC section numbers** (all citations are 2021/2018 based): not re-checked.
2. **Table R602.3.2** exact nail count per side and SDC split (text from a summarizer; the plate sizes 3x6/3x12 are consistent with IBC 2308.5.3.2).
3. **IRC R506.2.3 vapor retarder**: two sources say 2021 IRC = 10 mil ASTM E1745 Class A; 2018 = 6 mil; IBC 2021 1907.x still 6 mil per search - verify IBC 2021 wording and subsection number.
4. **IRC R403.1.3.3** (turned-down footing rebar) number is from 2015/2018; the 2021 numbering for the subsection was not seen (R403.1.3 verified).
5. **IRC R602.6.1 nail type**: 8 x 10d (0.148 x 1-1/2) in the TX 2024/2021 text; some older/other editions say 16d - verify.
6. **IRC R606.11 figures**: bolt size/embedment/spacing for roof anchorage (e.g. 1/2" x 15" @ 4'-0" o.c.) NOT verified; only the section title and "cells receiving bolts grouted solid" were seen.
7. **IRC R802.6 bearing** 3" on masonry (memory); trusses follow truss drawings.
8. **Simpson loads**: 2013/2019 catalog extracts via PDF text; currents differ. Allowable HETA16/20 ~1,810 lb; META ~1,450; HHETA ~2,235; HETAL ~1,810; DETAL20 2,480 (double). CS16 1,705 (verified C-C-2019). MSTA/LSTA loads from older catalog (digits garbled by extraction; verify). Gauge for LSTA/MSTA not extracted; CS14/CS20 widths not extracted (CS16 1-1/4" via retailer).
9. **Post cap models** (BC, CCQ, ECCQ, LCE, PC): family names from memory - verify exact names/loads.
10. **TMS 402 details** (bond beam rebar cover value, lap length, minimum vertical bar spacing, cell dimensions) - not verified; given as "verify".
11. **Concrete strength Table R402.2** values (2500/3000/3500 psi by weathering) - from memory.
12. **IRC R311.3.2** (exterior door landing max 7-3/4" below threshold) from memory (medium).
13. **R905.1.1/R905.2.8.5 (underlayment/drip edge), R806.2/R806.3 (vent area/clearance), R702.3 (gypsum), R302.6 (garage separation)** - section topics medium; numbers verify in the adopted edition.
14. **IBC 2308.5.x** numbering of cut-top-plate/ header provisions in 2021 (2018 was 2308.9.x).
15. **Heel heights** for standard/energy heel are truss-manufacturer data, not code; the numbers here are typical ranges.
16. **A-DETL minor layer codes** (architectural) not extracted - only S- and ANNO- codes from the NCS v5 list; v6 differs in a few codes.
17. Line-weight values and hatch pattern name mapping: typical practice; not read from a standard. NCS UDS reference text heights (3/32" min.) from the user spec and general practice.

# 5. Sources consulted this session (links)

- ICC IRC 2021 (R602.3.2): https://codes.iccsafe.org/s/IRC2021P3/chapter-6-wall-construction/IRC2021P3-Pt03-Ch06-SecR602.3.2
- up.codes top plate / R602.6.1: https://up.codes/s/top-plate ; https://up.codes/s/drilling-and-notching-of-top-plate
- IBC 2021 2308.5.3.2 Top plates: https://codes.iccsafe.org/s/IBC2021P1/chapter-23-wood/IBC2021P1-Ch23-Sec2308.5.3.2
- IRC R403.1.4 / frost, R403.1.1, R403.1.3, R403.1.6: via ICC / up.codes pages (see search)
- IRC R506 base/vapor retarder: up.codes concrete floors and Stego summary of 2021 R506.2.3
- IRC R317/R318/R404.1.6/R401.3: ICC and up.codes
- IRC R606.11 (NY 2020/IRC 2015): https://codes.iccsafe.org/s/IRC2015NY/chapter-6-wall-construction/IRC2015-Pt03-Ch06-SecR606.11
- IRC R802.11: https://up.codes/s/roof-tie-uplift-resistance
- Simpson Strong-Tie catalog C-2013 p. 166-167 (META/HETA/HHETA/HETAL/DETAL/TSS): https://images.thdstatic.com/catalog/pdfImages/e9/e9be2b81-e6ad-4a8b-929b-677ae0006391.pdf
- Simpson Strong-Tie C-C-2019 p. 267 (CS/CMST/CMSTC): https://assets.unilogcorp.com/187/ITEM/DOC/241730_Datasheet_1.pdf
- Simpson Strong-Tie straps catalog (MSTA/LSTA/MST/MSTC): https://assets.unilogcorp.com/187/ITEM/DOC/Simpson_Strong-Tie_1442555_Catalog.pdf
- AIA/NCS CAD Layer Guidelines v5: https://facilities.duke.edu/sites/default/files/AIA%20CAD%20Layer%20Guidelines.pdf
- ACI 318-19 20.5.1.3.1 cover (summary): https://www.concrete.org/frequentlyaskedquestions.aspx?faqid=903
