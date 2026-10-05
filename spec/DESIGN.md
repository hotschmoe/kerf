# KERF — Design Language

**Brief:** a 1970s engineering office. Imagine a drafting room, an IBM 3270 terminal on a steel desk,
a Selectric-typed transmittal form, and vellum with non-photo-blue grid. Serious and quiet, built
from typography, rules, and grids. **No** gradients, glassmorphism, rounded "SaaS" cards, emoji, or drop shadows
(one exception: a 2px hard offset shadow on floating panels, like a paper sheet on a desk).
All four apps (web, egui, teak) implement the same look so the bake-off compares fidelity
fairly. Code style is unaffected; this is only about what the user sees.

## 1. Tokens

| token | value | use |
|---|---|---|
| `paper` | `#F2EFE6` | app background (form paper) |
| `paper-2` | `#E9E5D8` | panel fill, alternate rows |
| `vellum` | `#FBFAF5` | drawing sheet background in the 2D view |
| `ink` | `#1A1A1A` | text, rules, linework |
| `ink-2` | `#55524B` | secondary text, field labels |
| `grid` | `#A9C1DD` | non-photo-blue grid (viewport, sheet) |
| `grid-2` | `#D3E0EE` | minor grid |
| `blue` | `#1D4E9E` | primary accent: selection outline, active tab bar, links |
| `red` | `#C8102E` | errors, destructive, "UNVERIFIED" stamp |
| `amber` | `#D98E04` | warnings |
| `green` | `#2E7D32` | verified, ok |
| `manila` | `#E9D9A6` | LLM message cards (punch-card/manila), cut caps in 3D |
| `term-bg` | `#0E120E` | console strip background (3270) |
| `term-fg` | `#5CF27A` | console text (phosphor green) |

Dark mode: none. It's a paper office.

**Type:** IBM Plex Mono (OFL, `spec/fonts/`) for everything in the chrome. Regular 400 for body,
Medium 500 for labels and buttons, Bold 700 for the wordmark and headings. Sizes: 11px field
labels (UPPERCASE, letter-spacing 0.08em), 13px body, 15px panel headings, 22px wordmark.
Line height 1.45. Drawings use the Kerf stroke font (Hershey Simplex), which looks like a pen plotter.

**Rules & grid:** 1px `ink` rules separate regions; 2px `ink` rule under the header bar. Spacing
on an 8px grid. Corners are square (radius 0) everywhere, except detail bubbles, which are circles.

## 2. Layout (desktop ≥ 1200px)

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ KERF ▮▮▮  DETAIL WORKSTATION     DOC: TRUSS-BEARING-CMU  REV 14   STYLE: KERF-STANDARD    │  header 40px
╞════════════════════════╤═════════════════════════════════════════════╤═════════════════════╡
│ OPERATOR CONSOLE       │ [SECTION A] [ISO B] [3D] [SHEET]   ─ + FIT │ INSPECTOR           │
│────────────────────────│                                              │ [PARTS][NOTES][DIFF]│
│ ┌ DESIGNER ─────────┐  │                                              │─────────────────────│
│ │ need a detail of  │  │          viewport (vellum + blue grid)       │ NO  ID         TYPE │
│ │ prefab truss on…  │  │                                              │ 01  cmu        CMU  │
│ └───────────────────┘  │                                              │ 02  sill_plate LUMB │
│ ┌ KERF/CLAUDE ──────┐  │                                              │ …                   │
│ │ manila card       │  │                                              │─────────────────────│
│ │ ▸ APPLY 6 OPS  ✓  │  │                                              │ FIELD   VALUE       │
│ └───────────────────┘  │                                              │ TEXT    [........]  │
│                        │                                              │ CITE    IRC R403.1.6│
│ ┌────────────────────┐ │                                              │ STATUS  ○SUGG ●VERIF│
│ │ > _                │ │                                              │                     │
│ └────────────────────┘ │                                              │ [EXPORT DXF][PDF][SVG]
╞════════════════════════╧═════════════════════════════════════════════╧═════════════════════╡
│ READY ▮ 14 COMPONENTS ▮ 0 ERR 1 WARN ▮ VIEW A 1-1/2"=1'-0" ▮ X 2'-3 1/2" Y 4'-0" ▮ CLAUDE OK │  status 24px (term colors)
└──────────────────────────────────────────────────────────────────────────────────────────┘
```
- Left: **Operator Console**, 360px. Right: **Inspector**, 320px. Center: the viewport, which flexes.
- The status line at the bottom is a 3270 OIA strip: `term-bg` background, `term-fg` text,
  segments separated by `▮`. It shows the state, counts, the cursor in ft-in, and the LLM state (`CLAUDE OK`,
  `CLAUDE BUSY ◐`, `NO KEY`).
- Narrow screens (< 900px): the console and inspector become tabs above the viewport.

## 3. Components

- **Wordmark:** `KERF` in Plex Mono Bold 22px with letter-spacing 0.2em, followed by three 6×14px `ink`
  bars (`▮▮▮`), like the saw kerf. Don't imitate any company logo.
- **Buttons:** rectangular, 1px `ink` border, `paper` fill, Medium 12px UPPERCASE, 28px tall.
  Hover inverts (ink fill, paper text). Primary buttons use a `blue` fill with paper text. On press the button moves down 1px.
- **Tabs:** text in brackets, `[SECTION A]`. The active tab gets a `blue` 3px underline bar and bold text.
- **Fields:** a typed-form look. A tiny UPPERCASE `ink-2` label sits above a value cell with a 1px bottom
  rule only. When focused, the bottom rule becomes 2px `blue`.
- **Tables:** monospace columns, 1px rules between rows, a header row in `paper-2` with UPPERCASE
  labels, zero-padded row numbers (`01`, `02`).
- **Chat messages:**
  - Designer messages are plain `paper` blocks with a 1px border and a header `DESIGNER  15:42`.
  - LLM messages are **manila cards** with a header `KERF/CLAUDE  15:42`.
  - Tool activity shows inside the card as monospace lines: `▸ APPLY  6 OPS   ✓ 0 ERR 1 WARN`,
    `▸ RENDER VIEW A  ✓`. These are collapsible and show the op JSON when expanded.
  - Attached screenshots show as thumbnails with a 1px border.
- **Stamps:** `UNVERIFIED` is a red outlined rubber-stamp label (rotated −3°, 1.5px border, red text)
  shown beside unverified citations in the inspector. `VERIFIED` is a green outlined stamp.
- **Diagnostics:** `E` red / `W` amber / `I` ink-2 single-letter tags in a 16px square, followed by the code
  and message. Clicking one selects the component.

## 4. Viewports

- **2D (section/iso/sheet):** `vellum` background with a non-photo-blue grid of 1" major and 1/4"
  minor in model space, which fades out when it gets too dense. Linework draws as ink at true pen
  weights scaled by zoom (clamped to a minimum of 1 device px). Hovering a component outlines it in `blue`, and selecting it fills
  its cut region with a 15% `blue` tint. Notes are draggable: dragging sets `place` and is recorded
  as an op.
- **3D:** `paper` background with a ground grid in `grid-2`. Faces are flat shaded with the style's
  `color3d` (muted, slightly desaturated), feature edges are ink lines (1.25 px), and cut caps are
  `manila`. Orbit/pan/zoom, with view-cube buttons `[FRONT] [ISO] [TOP] [RIGHT]` as bracketed text.
  No PBR or environment maps. It should look like a 1970s plotter's iso rendered in color.
- **Sheet:** the PDF page preview, exactly what will be exported.

## 5. Sheet & title block (drawn by the engine — DXF optional, PDF/SVG always)

Letter landscape 11×8.5, 3/8" margin, 0.7mm frame. The title block is a 3/4" strip across the bottom
with boxed cells. Each cell has a tiny label at its top-left (label height 5/64") and a value:

```
┌──────────────────────────────┬───────────────┬──────────┬─────────┬────────┬───────────┐
│ DETAIL                       │ PROJECT       │ SCALE    │ DRAWN   │ DATE   │ DETAIL NO │
│ PREFAB TRUSS BEARING AT CMU  │               │ 1½"=1'-0"│ KERF    │        │   1/S-501 │
└──────────────────────────────┴───────────────┴──────────┴─────────┴────────┴───────────┘
  KERF ▮▮▮  CODE BASIS: IRC 2021   * CODE REFERENCE NOT VERIFIED BY DESIGNER (if any)
```
Values come from the document `meta` and style `sheet`. DATE comes from `meta.date` and is never the wall clock.

## 6. Voice

- UI copy is terse, UPPERCASE for labels and buttons, sentence case for messages.
  Examples: "NO API KEY — ENTER KEY TO ENABLE CLAUDE.", "EXPORTED TRUSS-BEARING-CMU-A.DXF (48 KB)".
- Empty viewport: "NO DETAIL LOADED. DESCRIBE ONE IN THE CONSOLE, OR OPEN A .KERF.JSON."

## 7. Must-have interactions (all apps)

1. Enter the API key, which is stored locally, with a model picker (default `claude-opus-5-5`; `claude-sonnet-5-5` available).
2. Chat: text plus an image attachment (paste or drag a screenshot); the tool loop runs; the
   doc updates live after each `kerf_apply`; the console shows tool activity.
3. Views: switch section / iso / 3D / sheet; zoom, pan, fit; 3D orbit.
4. Select a component in the viewport or inspector and see its params and anchors (read-only is OK for MVP).
5. Notes: edit text, edit citations, toggle VERIFIED (designer only), drag the note position. Every edit is an op.
6. Export DXF / PDF / SVG, and save/open `.kerf.json`.
7. Diff tab: the op log (who: DESIGNER or CLAUDE, ops, timestamp), plus undo of the last op group.
