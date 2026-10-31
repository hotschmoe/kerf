//! Field tables of the non-component objects (SPEC 19): doc, view, note, dim, label, cite, ops, at, array, acknowledge.
//! Pure data plus lookups; `schema` renders them (`kerf schema`, the guide), `lint` checks documents against them
//! (W_UNKNOWN_KEY) and `canon` takes the canonical key order from them.

#[derive(Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    /// Short type text, e.g. `string`, `[x, y]`, `right|left|both`.
    pub ty: &'static str,
    /// Empty = optional with no stated default.
    pub def: &'static str,
    pub required: bool,
    pub desc: &'static str,
}

#[derive(Clone, Copy)]
pub struct Object {
    pub name: &'static str,
    /// Few words for the topic list.
    pub brief: &'static str,
    pub summary: &'static str,
    pub fields: &'static [Field],
    /// Extra text printed after the fields (path tables, rules).
    pub notes: &'static str,
    /// One compact JSON example ("" = none; the doc example is rendered from `example_doc`).
    pub example: &'static str,
}

pub static DOC: Object = Object {
    name: "doc",
    brief: "the whole document",
    summary: "The whole document (one `<id>.kerf.json` file).",
    fields: &[
        Field { name: "kerf", ty: "string", required: true, desc: "schema version, always \"0.1\"", ..Field::NONE },
        Field {
            name: "id",
            ty: "string",
            required: true,
            desc: "slug of the detail, e.g. \"truss-bearing-cmu\"",
            ..Field::NONE
        },
        Field { name: "title", ty: "string", def: "\"\"", desc: "sheet title in UPPERCASE", ..Field::NONE },
        Field {
            name: "meta",
            ty: "object",
            desc: "free-form, known keys: requested[] (the designer's asks; see below), references[] (the reference images the detail is redrawn from; see below), author, discipline, classification {uniformat, masterformat[]}, jurisdiction {code, edition} (default citation basis, e.g. {\"code\":\"IRC\",\"edition\":2021}), tags[], sheet, date, forked_from",
            ..Field::NONE
        },
        Field {
            name: "run",
            ty: "[z0, z1]",
            def: "[-24, 24]",
            desc: "default z extent (inches) of members that span the depth; members with a natural z thickness are centered on the section cut (or the middle of run)",
            ..Field::NONE
        },
        Field {
            name: "scale",
            ty: "string",
            desc: "default scale of every view without its own `scale` (iso views stay NTS), e.g. \"3/4\\\"=1'-0\\\"\", so scenarios print at one scale",
            ..Field::NONE
        },
        Field {
            name: "components",
            ty: "[component]",
            def: "[]",
            desc: "the construction; order = draw order (see `kerf schema <type>` and `kerf catalog`)",
            ..Field::NONE
        },
        Field {
            name: "views",
            ty: "[view]",
            def: "[]",
            desc: "drawings of the components (see `kerf schema view`)",
            ..Field::NONE
        },
        Field {
            name: "layouts",
            ty: "[layout]",
            def: "[]",
            desc: "sheets of several views with one shared DETAIL NOTES list (see `kerf schema layout`)",
            ..Field::NONE
        },
    ],
    notes: concat!(
        "First build: one op {\"op\":\"set\",\"path\":\"doc\",\"value\":<whole document>}. Then small add/update/remove ops (`kerf schema ops`).\n",
        "meta.requested: [\"cmu wall\", \"bond beam\", \"H2.5A ties\"]: the designer's asks, one short phrase each, written on the first build. `kerf check` and `kerf apply` print a COVERAGE block ",
        "(`ok <component ids>` or `MISSING` with the closest candidate per item) and W_REQUESTED_MISSING for each missing item. An item is covered when all its words appear in one component's ",
        "id/type/label/model/size or in one annotation: note or label text (with its cites), dim text (or the length it measures), a weld (`3/16 fillet weld all around`, `cjp`), a view title. ",
        "Words match ignoring case, punctuation and a plural s, through the house abbreviations both ways (`each` = `EA.`, `clear` = `CLR.`, `on center` = `O.C.`) and as lengths (`15 ft` = `15'-0\"`), ",
        "so write the designer's own words and draw or note them in house style. Finish only at full coverage; remove an item only if the designer dropped it.\n",
        "meta.references: [{\"path\": \"refs/0123_exterior_wall_at_stem.png\", \"caption\": \"EXTERIOR WALL AT STEM\"}]: the 1-3 snips of real drawings the detail is redrawn from, shown next to the drawing in `kerf serve`. ",
        "path* is relative to the document's folder (forward slashes, no `..`, no absolute paths or URLs) and names a png, jpg, jpeg, webp, gif, svg or pdf; caption is optional text. ",
        "`kerf check` warns W_REFERENCE_FILE when a path is not usable or the file does not exist."
    ),
    ..Object::NONE
};

pub static VIEW: Object = Object {
    name: "view",
    brief: "a drawing view (crop, scale, cut_z, notes_side, annotations)",
    summary: "One drawing view (goes in `views[]`; export with `--view <id>`).",
    fields: &[
        Field {
            name: "id",
            ty: "string",
            required: true,
            desc: "view id, e.g. \"A\"; used by `kerf export --view A` and op paths `views/A/...`",
            ..Field::NONE
        },
        Field {
            name: "kind",
            ty: "section|iso|schedule",
            def: "section",
            desc: "section = 2D cut looking toward -Z; iso = hidden-line 3D; schedule = tables, notes and labels only (no components, no crop or scale, paper inches from [0, 0], caption SCALE: NTS; \"sheet\" is accepted for it)",
            ..Field::NONE
        },
        Field {
            name: "number",
            ty: "string",
            def: "order in views: \"1\", \"2\"",
            desc: "detail number in the title bubble",
            ..Field::NONE
        },
        Field {
            name: "title",
            ty: "string",
            def: "\"\"",
            desc: "title under the view, UPPERCASE, e.g. \"TRUSS BEARING AT CMU WALL\"",
            ..Field::NONE
        },
        Field {
            name: "scale",
            ty: "string",
            desc: "schedule: none (\"NTS\" accepted). section: 3\"=1'-0\" | 1-1/2\"=1'-0\" | 1\"=1'-0\" | 3/4\"=1'-0\" | 1/2\"=1'-0\" | 3/8\"=1'-0\" | 1/4\"=1'-0\" | 3/16\"=1'-0\" | 1/8\"=1'-0\" (any <paper>\"=1'-0\") | \"1:N\"; iso: \"NTS\". When omitted the engine picks the largest of 3\"..1/4\"=1'-0\" at which the view + notes + title fit the frame",
            ..Field::NONE
        },
        Field {
            name: "cut_z",
            ty: "length",
            def: "0",
            desc: "section: z of the cut plane (the viewer is at +z looking -z); members whose z range contains it are cut (hatched/marked), members behind it (z1 <= cut_z) draw as beyond outlines, members wholly in front of it (z0 >= cut_z) are not drawn. A section used as an elevation therefore needs cut_z in front of (greater than) every member to show. Also the default z of in-plane members and anchor bolts",
            ..Field::NONE
        },
        Field {
            name: "crop",
            ty: "{\"x\":[x0,x1],\"y\":[y0,y1]}",
            desc: "model-inch window (x right, y up). When omitted it auto-fits all non-fill components plus 6\" (fills are clipped to that box). Solids cut by the window get break lines. After an edit that leaves a member mostly outside an explicit crop, W_CROP_STALE says so (remove crop and scale to re-fit)",
            ..Field::NONE
        },
        Field {
            name: "from",
            ty: "front_right|front_left|back_right|back_left",
            def: "front_right",
            desc: "iso only: viewing corner",
            ..Field::NONE
        },
        Field {
            name: "cutaway",
            ty: "boolean",
            def: "false",
            desc: "iso only: clip solids to z <= cut_z and hatch the cut face",
            ..Field::NONE
        },
        Field {
            name: "notes_side",
            ty: "right|left|both|above",
            def: "both",
            desc: "leader notes: which side the note column goes; both = each note goes to the side nearer its landing point. keyed/list: where the DETAIL NOTES list goes (both = above)",
            ..Field::NONE
        },
        Field {
            name: "notes_style",
            ty: "leader|keyed|list",
            def: "leader",
            desc: "leader = text at the leader end; keyed = a number tag at the leader end and the texts in a numbered DETAIL NOTES list (notes sharing a `key` share one entry); list = the DETAIL NOTES list only, no tags or leaders (typical notes; `target` not needed). In a keyed view a note with neither `target` nor `at` is a list-only entry (a number, no tag). Schedule views are always list",
            ..Field::NONE
        },
        Field {
            name: "notes_title",
            ty: "string",
            desc: "heading of the DETAIL NOTES list in this view, e.g. \"HEADER SCHEDULE NOTES\" (1-60 characters; default: the style's notes.list_title, DETAIL NOTES)",
            ..Field::NONE
        },
        Field {
            name: "omit",
            ty: "[id|glob]",
            def: "[]",
            desc: "components not drawn (or checked) in this view; ids or `*` globs (\"b_*\"). An entry that matches no component warns W_REF_UNKNOWN",
            ..Field::NONE
        },
        Field {
            name: "only",
            ty: "[id|glob]",
            def: "",
            desc: "the view draws only the components matching one of these ids or `*` globs ([\"shared_*\", \"a_*\"]); `omit` and component `views` still apply",
            ..Field::NONE
        },
        Field {
            name: "annotations",
            ty: "[note|dim|label|weld|table]",
            def: "[]",
            desc: "notes, dimensions, labels, weld symbols and tables (schedules) in drawing order (`kerf schema note|dim|label|weld|table`)",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"A\",\"kind\":\"section\",\"number\":\"1\",\"title\":\"WALL AT SILL\",\"scale\":\"1\\\"=1'-0\\\"\",\"cut_z\":0,\"crop\":{\"x\":[-12,18],\"y\":[-8,30]},\"notes_side\":\"both\",\"annotations\":[]}",
    ..Object::NONE
};

pub static NOTE: Object = Object {
    name: "note",
    brief: "leader or keyed note with optional citations",
    summary: "Leader note (annotation of type note): UPPERCASE text, an arrow landing inside the target, optional code citations.",
    fields: &[
        Field {
            name: "id",
            ty: "string",
            required: true,
            desc: "unique within the view, [a-z][a-z0-9_]*",
            ..Field::NONE
        },
        Field { name: "type", ty: "\"note\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "text",
            ty: "string",
            required: true,
            desc: "keyed views: optional on a note whose `key` another note already has. House format: UPPERCASE, no trailing period, fractions `1 1/2\"`, ` X ` between sizes, abbreviations W/ O.C. EA. CONT. TYP. MIN. DIA. BOT. CONC. GYP. BD. REINF. PT (always O.C. MIN. MAX. DIA. GYP. BD.; other words may be spelled out); penny sizes `16d` and ACI symbols `db` `ld` `ldh` `f'c` `fy` keep their case, a house abbreviation may end the text (`PER ARCH.`) (W_NOTE_STYLE lints this, and also commentary such as `?`, `NOTE:`, `WE`, `SHOULD BE`, a text that ends on `W/` or `AND`, over 130 characters, a repeat of another note)",
            ..Field::NONE
        },
        Field {
            name: "target",
            ty: "string",
            desc: "component id or `comp.part` (e.g. `slab.footing`); the arrow lands inside its visible region. Required unless `at` is given (or the view is keyed, list or schedule: a keyed note with neither is listed with no tag)",
            ..Field::NONE
        },
        Field {
            name: "at",
            ty: "point",
            desc: "exact arrow landing: \"comp@anchor\", {\"ref\":\"comp@anchor\",\"offset\":[dx,dy]} or [x, y]. Use it when the target is hidden or the label point is bad. Iso: the point is on the face of its component (else of `target`) nearest the viewer. W_NOTE_LANDING: outside the crop (section) or off every face (iso)",
            ..Field::NONE
        },
        Field {
            name: "place",
            ty: "[x, y]",
            desc: "model-inch position of the text (designer override; the note is then fixed). Not `side`/`pos`: the column side is the VIEW's `notes_side`",
            ..Field::NONE
        },
        Field {
            name: "column",
            ty: "left|right",
            desc: "which notes column this note's text goes in (per-note version of the view's `notes_side`; the layout never moves it to the other column). A note with `place` right-aligns its text when it sits left of its arrow",
            ..Field::NONE
        },
        Field {
            name: "key",
            ty: "integer",
            desc: "keyed views: the note's number in the DETAIL NOTES list (1-999; keeps numbers stable across edits). Notes with the same key share one entry and each draws its tag; a repeat may omit `text` (it then also takes the first one's `cite`). Unkeyed notes take the lowest free numbers in note order",
            ..Field::NONE
        },
        Field {
            name: "cite",
            ty: "[cite]",
            desc: "code citations printed after the text, e.g. (IRC R403.1.6)* (`kerf schema cite`). The key is `cite`, not citations",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"n_sill\",\"type\":\"note\",\"text\":\"2X6 PT SILL PLATE\",\"target\":\"sill\",\"cite\":[{\"code\":\"IRC\",\"edition\":2021,\"section\":\"R317.1\",\"title\":\"Location required\",\"status\":\"suggested\"}]}",
    ..Object::NONE
};

pub static DIM: Object = Object {
    name: "dim",
    brief: "linear dimension",
    summary: "Linear dimension between two points (annotation of type dim). Section views only.",
    fields: &[
        Field { name: "id", ty: "string", required: true, desc: "unique within the view", ..Field::NONE },
        Field { name: "type", ty: "\"dim\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "from",
            ty: "point",
            required: true,
            desc: "\"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]",
            ..Field::NONE
        },
        Field { name: "to", ty: "point", required: true, desc: "same forms as `from`", ..Field::NONE },
        Field {
            name: "dir",
            ty: "h|v|aligned",
            def: "dominant axis",
            desc: "h measures |dx|, v measures |dy|, aligned the true distance. When omitted: h if |dx| >= |dy|, else v. W_DIM_ZERO fires when the dimension measures under 1/16\"",
            ..Field::NONE
        },
        Field {
            name: "offset",
            ty: "length",
            def: "0",
            desc: "distance of the dimension line from the points, model inches. h: > 0 puts it above the higher point, < 0 below the lower; v: > 0 right of the rightmost point, < 0 left of the leftmost; aligned: perpendicular, sign = side",
            ..Field::NONE
        },
        Field { name: "text", ty: "string", desc: "replaces the measured text (e.g. \"VERIFY\")", ..Field::NONE },
        Field {
            name: "pin",
            ty: "bool",
            def: "false",
            desc: "true: drawn exactly at `offset`, never stacked or pushed off a leader (overlaps then warn W_LABEL_OVERLAP). Without it a colliding dimension moves out and I_ANNOT_MOVED says where",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"d_sill\",\"type\":\"dim\",\"from\":\"sill@bottom_left\",\"to\":\"sill@bottom_right\",\"dir\":\"h\",\"offset\":-3}",
    ..Object::NONE
};

pub static LABEL: Object = Object {
    name: "label",
    brief: "free text without a leader",
    summary: "Free text without a leader (annotation of type label), such as EXTERIOR, INTERIOR or GRADE.",
    fields: &[
        Field { name: "id", ty: "string", required: true, desc: "unique within the view", ..Field::NONE },
        Field { name: "type", ty: "\"label\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "text",
            ty: "string",
            required: true,
            desc: "UPPERCASE text; a newline (\"\\n\" in JSON) starts a new line",
            ..Field::NONE
        },
        Field {
            name: "at",
            ty: "point",
            required: true,
            desc: "\"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]. The text block is centered vertically on it",
            ..Field::NONE
        },
        Field { name: "offset", ty: "[dx, dy]", def: "[0, 0]", desc: "model inches added to `at`", ..Field::NONE },
        Field {
            name: "pin",
            ty: "bool",
            def: "false",
            desc: "true: never moved off a note leader (a leader through it then warns W_LEADER_HIT). Without it a label on a leader moves up to 6 text heights and I_ANNOT_MOVED says where",
            ..Field::NONE
        },
        Field {
            name: "align",
            ty: "left|center|right",
            def: "center",
            desc: "which part of each line sits on `at`: its start (left), middle or end (right)",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"l_ext\",\"type\":\"label\",\"text\":\"EXTERIOR\",\"at\":\"sill@top_left\",\"offset\":[-6,6]}",
    ..Object::NONE
};

pub static WELD: Object = Object {
    name: "weld",
    brief: "AWS weld symbol at a joint",
    summary: "AWS weld symbol (annotation of type weld): arrow to the joint, reference line, fillet / CJP / PJP symbol with its size (below the line = arrow side, above = other side), all-around circle, field-weld flag, tail text. Placed exactly where you put it (not moved by the note layout). Section views only.",
    fields: &[
        Field { name: "id", ty: "string", required: true, desc: "unique within the view", ..Field::NONE },
        Field { name: "type", ty: "\"weld\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "at",
            ty: "point",
            required: true,
            desc: "the joint the arrow touches: \"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y], e.g. the corner where a column meets its base plate",
            ..Field::NONE
        },
        Field {
            name: "offset",
            ty: "[dx, dy]",
            def: "1/2\" paper up and right",
            desc: "model inches from `at` to the elbow where the reference line starts; the reference line runs away from the arrow (dx < 0 puts it on the left)",
            ..Field::NONE
        },
        Field {
            name: "weld",
            ty: "fillet|cjp|pjp",
            def: "fillet",
            desc: "fillet: triangle; cjp / pjp: bevel groove with CJP / PJP in the tail",
            ..Field::NONE
        },
        Field {
            name: "size",
            ty: "length",
            desc: "fillet leg or PJP groove depth, e.g. \"3/16\" (required for fillet)",
            ..Field::NONE
        },
        Field {
            name: "length",
            ty: "length",
            desc: "weld length, printed right of the symbol (AWS A2.4), e.g. 2 for \"3/16 x 2\\\" EA. END\" (the EA. END goes in `tail`). Omitted: continuous",
            ..Field::NONE
        },
        Field {
            name: "pitch",
            ty: "length",
            desc: "intermittent weld: center-to-center spacing of the segments, printed as length-pitch (e.g. length 2, pitch 12 prints 2-12); needs `length`",
            ..Field::NONE
        },
        Field {
            name: "sides",
            ty: "arrow|other|both",
            def: "arrow",
            desc: "arrow side (symbol below the line), other side (above) or both",
            ..Field::NONE
        },
        Field {
            name: "all_around",
            ty: "bool",
            def: "false",
            desc: "circle at the elbow: weld all around the joint",
            ..Field::NONE
        },
        Field { name: "field", ty: "bool", def: "false", desc: "field-weld flag at the elbow", ..Field::NONE },
        Field {
            name: "tail",
            ty: "string",
            desc: "tail text, e.g. \"TYP.\", \"3 SIDES\" (cjp/pjp default to CJP/PJP)",
            ..Field::NONE
        },
        Field {
            name: "members",
            ty: "[id, id]",
            desc: "the two steel components joined; W_WELD_SIZE then checks a fillet against the thinner part (AISC 360 J2.2b: at most t, or t - 1/16 when t >= 1/4; at least Table J2.4). An entry that is not a steel component warns (W_PARAM)",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"w_base\",\"type\":\"weld\",\"at\":\"col@bottom_right\",\"offset\":[6,6],\"weld\":\"fillet\",\"size\":\"3/16\",\"all_around\":true,\"members\":[\"col\",\"base_plate\"]}",
    ..Object::NONE
};

pub static TABLE: Object = Object {
    name: "table",
    brief: "a schedule: ruled grid of cells with headers",
    summary: "A schedule table (annotation of type table): a ruled grid sized in paper inches (the view scale does not change it), with header rows (colspan/rowspan for grouped headers), body rows, an optional title above and numbered notes below. The way to draw any schedule (lap lengths, anchor embedment, holdowns, headers, hangers), never cells built from solids and labels. Put it in a view of kind \"schedule\" (no components) or under a section's drawing.",
    fields: &[
        Field { name: "id", ty: "string", required: true, desc: "unique within the view", ..Field::NONE },
        Field { name: "type", ty: "\"table\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "columns",
            ty: "[string|column]",
            required: true,
            desc: "one entry per column (1-40): a header string, or {\"header\", \"unit\", \"width\", \"align\"}: unit prints as (UNIT) under the header, width is the printed width in paper inches (0.1-30; default: fit the widest cell, long text wraps at about notes.wrap_chars), align left|center|right of the body cells (default center)",
            ..Field::NONE
        },
        Field {
            name: "header",
            ty: "[[cell]]",
            desc: "header rows for grouped headers (then leave `header` out of `columns`); without it the column headers make one header row. Header cells are centred",
            ..Field::NONE
        },
        Field {
            name: "rows",
            ty: "[[cell]]",
            required: true,
            desc: "body rows, top to bottom. A cell is a string, a number, null (empty) or {\"text\", \"colspan\", \"rowspan\", \"align\"}. As in HTML a cell takes the next column not covered by a rowspan from above; every row must cover every column exactly (E_PARAM names the row and the gap). \"\\n\" in a text starts a new line",
            ..Field::NONE
        },
        Field {
            name: "title",
            ty: "string",
            desc: "printed centred and underlined above the table, e.g. \"CLASS B TENSION LAP SPLICE (IN.)\"",
            ..Field::NONE
        },
        Field {
            name: "notes",
            ty: "[string]",
            desc: "printed under the table as NOTES: 1. 2. ...; a note starting with * is a footnote (no number)",
            ..Field::NONE
        },
        Field {
            name: "rules",
            ty: "all|header|outer|none",
            def: "all",
            desc: "all = heavy border, header rule, every row and column rule; header = no rules between body rows; outer = border and header rule; none = text only",
            ..Field::NONE
        },
        Field {
            name: "at",
            ty: "point",
            desc: "top-left corner of the border: a Ref or [x, y] (model inches; in a schedule view paper inches, y up). Default: a schedule view stacks its tables from [0, 0] down in order; other views put them under the drawing and its notes, left-aligned. A table placed with `at` over notes, dims, labels, lines or another table warns W_TABLE_OVERLAP",
            ..Field::NONE
        },
    ],
    notes: "Text takes the house case transform and W_NOTE_STYLE (no repeat check), and counts for meta.requested coverage (the title with the headers is one candidate, each body row with them another). Text height and line pitch are the style's note text. A cell wider than its fixed `width` warns W_TABLE_CELL with the width that fits. In a schedule view the DETAIL NOTES list sits above the tables (notes_side left/right puts it beside them; the view's notes_title renames it).",
    example: "{\"id\":\"t_lap\",\"type\":\"table\",\"title\":\"CLASS B TENSION LAP SPLICE (IN.)\",\"columns\":[{\"width\":0.7},{},{},{},{}],\"header\":[[{\"text\":\"BAR SIZE\",\"rowspan\":2},{\"text\":\"f'c = 3,000 PSI\",\"colspan\":2},{\"text\":\"f'c = 4,000 PSI\",\"colspan\":2}],[\"OTHER\",\"TOP\",\"OTHER\",\"TOP\"]],\"rows\":[[\"#3\",24,28,24,25],[\"#4\",29,38,25,33]],\"notes\":[\"VERT. BARS: USE OTHER VALUES\"]}",
};

/// The keys of a `columns` entry object (lint only).
pub static TABLE_COLUMN: Object = Object {
    name: "table",
    fields: &[
        Field { name: "header", ty: "string", ..Field::NONE },
        Field { name: "unit", ty: "string", ..Field::NONE },
        Field { name: "width", ty: "length", ..Field::NONE },
        Field { name: "align", ty: "left|center|right", ..Field::NONE },
    ],
    ..Object::NONE
};

/// The keys of a cell object (lint only).
pub static TABLE_CELL: Object = Object {
    name: "table",
    fields: &[
        Field { name: "text", ty: "string", ..Field::NONE },
        Field { name: "colspan", ty: "integer", ..Field::NONE },
        Field { name: "rowspan", ty: "integer", ..Field::NONE },
        Field { name: "align", ty: "left|center|right", ..Field::NONE },
    ],
    ..Object::NONE
};

pub static CITE: Object = Object {
    name: "cite",
    brief: "one code citation",
    summary: "One code citation; goes in a note's `cite` array.",
    fields: &[
        Field { name: "code", ty: "string", required: true, desc: "IRC, IBC, ACI 318, TMS 402, ...", ..Field::NONE },
        Field { name: "edition", ty: "number", def: "meta.jurisdiction.edition", desc: "e.g. 2021", ..Field::NONE },
        Field {
            name: "section",
            ty: "string",
            required: true,
            desc: "e.g. \"R403.1.6\". Only cite sections you are sure exist",
            ..Field::NONE
        },
        Field { name: "title", ty: "string", desc: "short section title", ..Field::NONE },
        Field {
            name: "status",
            ty: "suggested|verified",
            def: "suggested",
            desc: "agents may only write suggested; only the designer verifies (an agent's verified is reset). Unverified citations print with a trailing *",
            ..Field::NONE
        },
    ],
    example: "{\"code\":\"IRC\",\"edition\":2021,\"section\":\"R403.1.6\",\"title\":\"Foundation anchorage\",\"status\":\"suggested\"}",
    ..Object::NONE
};

pub static OPS: Object = Object {
    name: "ops",
    brief: "the edit ops `kerf apply` takes",
    summary: "The edit list `kerf apply` takes: a JSON array of op objects, applied in order and atomically (any error: nothing is written).",
    fields: &[
        Field { name: "op", ty: "add|update|remove|set", required: true, desc: "what to do", ..Field::NONE },
        Field { name: "path", ty: "string", required: true, desc: "where (table below)", ..Field::NONE },
        Field {
            name: "value",
            ty: "json",
            desc: "the object to add / the merge patch / the replacement; required except for remove",
            ..Field::NONE
        },
        Field {
            name: "before",
            ty: "component id",
            desc: "add to `components` only: insert before this component instead of appending",
            ..Field::NONE
        },
    ],
    notes: "Paths:\n  add    components                       value = a component object (unique id)\n  add    views                            value = a view object\n  add    views/<view>/annotations         value = a note | dim | label | weld | table\n  update components/<id>                  value = JSON merge patch: given keys replace (arrays and objects whole), null deletes a key; `id` cannot change\n  update views/<view>                     merge patch of view fields (not `annotations`: use the annotation paths)\n  update views/<view>/annotations/<id>    merge patch (changing `text` or `cite` resets citations to suggested)\n  update meta                             merge patch\n  remove components/<id> | views/<view> | views/<view>/annotations/<id>\n          (a component still referenced by an `at`, `until`, `slope`, note target or dim is refused: the error names the referrers)\n  set    doc                              value = the whole document (first build)\n  set    components/<id>                  replace the whole component\nInput forms: `kerf apply <doc> ops.json`, `... -` (stdin), or `--ops '<json>'`; the ops may be an array, a single op object, or {\"ops\":[...],\"why\":\"...\"} (the why is used when --why is absent). Add `-w --why \"...\"` to write and log.",
    example: "[{\"op\":\"add\",\"path\":\"components\",\"value\":{\"id\":\"sill\",\"type\":\"lumber\",\"size\":\"2x6\",\"orient\":\"flat\",\"treated\":true}},{\"op\":\"update\",\"path\":\"views/A/annotations/n_sill\",\"value\":{\"text\":\"2X6 PT SILL PLATE W/ SEALER\"}},{\"op\":\"remove\",\"path\":\"components/old\"}]",
};

pub static AT: Object = Object {
    name: "at",
    brief: "component placement",
    summary: "Component placement (`at`): translate the member so its own anchor lands on a point.",
    fields: &[
        Field {
            name: "anchor",
            ty: "anchor name",
            def: "bottom_left (anchor_bolt: top_of_concrete)",
            desc: "which anchor of THIS component lands on `to`: the 9 box anchors or a builder's named anchor",
            ..Field::NONE
        },
        Field {
            name: "to",
            ty: "point",
            def: "[0, 0]",
            desc: "\"comp@anchor\", \"comp.part@anchor\", \"@origin\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]",
            ..Field::NONE
        },
        Field { name: "offset", ty: "[dx, dy]", def: "[0, 0]", desc: "model inches added to `to`", ..Field::NONE },
    ],
    example: "{\"anchor\":\"bottom_left\",\"to\":\"sill@top_left\",\"offset\":[0,0]}",
    ..Object::NONE
};

pub static ARRAY: Object = Object {
    name: "array",
    brief: "repeat a component",
    summary: "Repeat a component (`array`): instance k is translated by k*spacing from instance 0, which sits where `at` and `z` put it (a numeric `z` places instance 0, it does not center the group). Instances are id#0..id#n-1; a ref to `id` means instance 0, `id#k@anchor` addresses instance k.",
    fields: &[
        Field {
            name: "axis",
            ty: "x|y|z",
            def: "x",
            desc: "x/y translate in the section plane; z steps in depth (trusses, anchor bolts, straps along the wall: drawn in iso/3D, one cut instance in section)",
            ..Field::NONE
        },
        Field { name: "count", ty: "integer 1..500", def: "1", desc: "number of instances", ..Field::NONE },
        Field {
            name: "spacing",
            ty: "length",
            required: true,
            desc: "inches between instances (may be negative)",
            ..Field::NONE
        },
    ],
    example: "{\"axis\":\"z\",\"count\":3,\"spacing\":32}",
    ..Object::NONE
};

pub static ACK: Object = Object {
    name: "acknowledge",
    brief: "suppress a warning with a logged reason",
    summary: "`acknowledge` on a component: suppress a warning for that component with a reason that is logged and stays visible (counted in the summary).",
    fields: &[
        Field {
            name: "code",
            ty: "string",
            required: true,
            desc: "the warning code, e.g. W_UNTREATED_CONTACT, W_OVERLAP, W_NEAR_MISS, W_SHORT_SLOPE, W_COVER",
            ..Field::NONE
        },
        Field {
            name: "reason",
            ty: "string",
            required: true,
            desc: "why it is fine; listed in the summary's ACKNOWLEDGED block (by code, with the components) and logged by `kerf apply -w`",
            ..Field::NONE
        },
    ],
    notes: "Usage: \"acknowledge\": [{\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"truss seat moisture barrier by mfr.\"}] on the component the warning names. Errors (E_*) cannot be acknowledged; an I_* info code is accepted but suppresses nothing (W_ACK_UNUSED). The DOC line counts acknowledged warnings (`0 errors  0 warnings  3 acknowledged`), `kerf check --strict` reports them as warnings (exit 1), `kerf export --review` marks them on the drawing, and an entry that suppresses nothing in any view is W_ACK_UNUSED (its fix is the update op that removes it). Prefer fixing the cause; for wood on masonry use `\"barrier\": \"sill_seal\"` (lumber) instead.",
    example: "[{\"code\":\"W_UNTREATED_CONTACT\",\"reason\":\"truss seat moisture barrier by mfr.\"}]",
};

impl Field {
    const NONE: Field = Field { name: "", ty: "", def: "", required: false, desc: "" };
}

pub static LAYOUT: Object = Object {
    name: "layout",
    brief: "several views on one sheet with one shared DETAIL NOTES list",
    summary: "A sheet of several views (goes in the document's `layouts[]`; export with `--layout <id>`). Each view keeps its bubble, title and scale line; keyed notes of all its views share one DETAIL NOTES list.",
    fields: &[
        Field {
            name: "id",
            ty: "string",
            required: true,
            desc: "layout id, e.g. \"S1\"; used by `kerf export --layout S1` and op paths `layouts/S1`",
            ..Field::NONE
        },
        Field {
            name: "views",
            ty: "[view id | [view id]]",
            required: true,
            desc: "the views in reading order; a nested list stacks views across the arrangement (a column inside a row or grid, a row inside a column), e.g. a main section beside two small plan insets: [\"A\", [\"P1\", \"P2\"]]",
            ..Field::NONE
        },
        Field {
            name: "arrange",
            ty: "row|column|grid",
            def: "row (grid when columns is set)",
            desc: "row: side by side, titles on one line; column: one above the other; grid: rows of `columns` entries",
            ..Field::NONE
        },
        Field { name: "columns", ty: "number", def: "2", desc: "entries per grid row (1-12)", ..Field::NONE },
        Field {
            name: "notes",
            ty: "shared|per_view",
            def: "shared",
            desc: "shared: one DETAIL NOTES list for the keyed notes of every view, numbered 1..N without gaps over the layout (same `key` = one entry; an unkeyed note with the same text and cite as an earlier one joins it); per_view: each keyed view keeps its own list",
            ..Field::NONE
        },
        Field {
            name: "notes_side",
            ty: "right|left|below|above",
            def: "right",
            desc: "where the shared list goes, beside or under the arranged views",
            ..Field::NONE
        },
        Field {
            name: "scale",
            ty: "string",
            def: "common",
            desc: "common: the largest standard scale (3\" ... 1/4\" = 1'-0\") at which the whole sheet fits, for every view without its own scale (or the document `scale`); or a scale for those views. A view's own `scale` wins: a plan inset at 1/2\"=1'-0\" beside a 1\"=1'-0\" section",
            ..Field::NONE
        },
        Field { name: "gap", ty: "number", def: "0.5", desc: "paper inches between views (0-4)", ..Field::NONE },
        Field {
            name: "sheet",
            ty: "\"WxH\"",
            def: "the style's sheet",
            desc: "paper size in inches, e.g. \"17x11\" (ANSI B) or \"36x24\": four scenarios with a list rarely fit a letter sheet at a useful scale",
            ..Field::NONE
        },
        Field { name: "title", ty: "string", def: "doc title", desc: "title block DETAIL text", ..Field::NONE },
        Field { name: "number", ty: "string", def: "\"\"", desc: "title block DETAIL NO", ..Field::NONE },
    ],
    notes: concat!(
        "Ops: {\"op\":\"add\",\"path\":\"layouts\",\"value\":{...}}, {\"op\":\"update\",\"path\":\"layouts/S1\",\"value\":{...}}, {\"op\":\"remove\",\"path\":\"layouts/S1\"}; the document default scale: {\"op\":\"set\",\"path\":\"scale\",\"value\":\"3/4\\\"=1'-0\\\"\"}.\n",
        "A layout is always drawn as one sheet; single-view export (`--view`) is unchanged. `kerf check` builds every layout: W_VIEW_FIT (path layouts/<id>) when the sheet cannot hold it, with each view's size and the fixes that fit (scale, arrangement, list side); I_KEY_RENUMBERED when the shared numbers differ from the notes' keys."
    ),
    example: "{\"id\":\"S1\",\"views\":[\"A\",\"B\",\"C\",\"D\"],\"arrange\":\"grid\",\"columns\":2,\"notes\":\"shared\",\"scale\":\"common\"}",
};

pub static REF: Object = Object {
    name: "ref",
    brief: "reference bubble: SEE detail/sheet",
    summary: "A reference bubble (annotation of type ref): the detail number over the sheet number in a split circle, with an optional leader and text beside it. Placed exactly (not moved by the note layout). Points at another detail (`SEE 3/S-50.01`); a view whose only content is a ref is a \"see other detail\" scenario.",
    fields: &[
        Field { name: "id", ty: "string", required: true, desc: "unique within the view", ..Field::NONE },
        Field { name: "type", ty: "\"ref\"", required: true, desc: "annotation type", ..Field::NONE },
        Field {
            name: "detail",
            ty: "string",
            required: true,
            desc: "detail number, top half of the bubble",
            ..Field::NONE
        },
        Field {
            name: "sheet",
            ty: "string",
            desc: "sheet number, bottom half (omitted: a plain bubble)",
            ..Field::NONE
        },
        Field {
            name: "at",
            ty: "point",
            desc: "where the leader arrow points: \"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y] (omitted: no leader)",
            ..Field::NONE
        },
        Field {
            name: "place",
            ty: "[x, y]",
            def: "at + 3/4\" paper up and right",
            desc: "model position of the bubble centre (needed without `at`)",
            ..Field::NONE
        },
        Field {
            name: "text",
            ty: "string",
            desc: "text beside the bubble, e.g. \"SEE\", \"SIM.\" or \"NON-BEARING PARTITION\"",
            ..Field::NONE
        },
    ],
    example: "{\"id\":\"r_see\",\"type\":\"ref\",\"detail\":\"3\",\"sheet\":\"S-50.01\",\"at\":\"beam@top_right\",\"text\":\"SIM.\"}",
    ..Object::NONE
};

impl Object {
    const NONE: Object = Object { name: "", brief: "", summary: "", fields: &[], notes: "", example: "" };
}

/// Every non-component topic, in listing order.
pub static OBJECTS: [&Object; 14] =
    [&DOC, &VIEW, &LAYOUT, &NOTE, &DIM, &LABEL, &WELD, &TABLE, &REF, &CITE, &OPS, &AT, &ARRAY, &ACK];

pub fn find_object(name: &str) -> Option<&'static Object> {
    if let Some(o) = OBJECTS.iter().find(|o| o.name == name) {
        return Some(o);
    }
    match name {
        "op" => Some(&OPS),
        "ack" => Some(&ACK),
        _ => None,
    }
}

pub fn has_field(o: &Object, key: &str) -> bool {
    o.fields.iter().any(|f| f.name == key)
}
