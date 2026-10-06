//! Component builders (SPEC 5): turn a component's JSON params into a `Built` (prisms, anchors,
//! zones) in local coordinates. Builders validate their params and report E_PARAM diagnostics;
//! they return null when the component cannot be built.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const catalog = @import("catalog.zig");
const params_mod = @import("params.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const path_geom = @import("pathgeom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Params = model.Params;
const Box = geom.Box;
const common = @import("builders/common.zig");
pub const anchor_bolt = @import("builders/anchor_bolt.zig");
pub const rebar = @import("builders/rebar.zig");
pub const concrete = @import("builders/concrete.zig");
pub const cmu_wall = @import("builders/cmu_wall.zig");
pub const panel = @import("builders/panel.zig");
pub const lumber = @import("builders/lumber.zig");
pub const BuildError = common.BuildError;
pub const Ctx = common.Ctx;
pub const mirrorAboutCenter = common.mirrorAboutCenter;
pub const worldBox = common.worldBox;
const boxOfPrisms = common.boxOfPrisms;
const zoneRect = common.zoneRect;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const quadOf = common.quadOf;
const parseSawn = common.parseSawn;
const parseActual = common.parseActual;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;
const materialOk = common.materialOk;
const lengthOrUntil = common.lengthOrUntil;
const parseCover = common.parseCover;
const mirrorBuilt = common.mirrorBuilt;
const gaugeThickness = common.gaugeThickness;
const vsOf = common.vsOf;
const pathLen = common.pathLen;
const materialOr = common.materialOr;

// ---- helpers ---------------------------------------------------------------------------------------

// ---- lumber -------------------------------------------------------------------------------------------

// ---- panel -----------------------------------------------------------------------------------------------

// ---- cover parsing ----------------------------------------------------------------------------------------

// ---- cmu_wall -----------------------------------------------------------------------------------------------

// ---- concrete ---------------------------------------------------------------------------------------------------

// ---- rebar --------------------------------------------------------------------------------------------------------

// ---- anchor bolt ----------------------------------------------------------------------------------------------------

// ---- connector ------------------------------------------------------------------------------------------------------

const Hardware = catalog.Hardware;
const hardware = catalog.hardware;

pub const ConnectorParams = struct {
    model: []const u8 = "",
    points: ?json.Value = null,
    lay: enum { edge, face } = .edge,
    side: enum { left, right } = .left,
    gauge: ?f64 = null,
    width: ?f64 = null,
    fasteners: []const u8 = "",

    pub const spec = .{
        .model = .{ .def = "null", .desc = "e.g. MSTA36, H2.5A, HETA20, CS16, CS14: fills width/gauge from the hardware table" },
        .points = .{ .def = "required", .desc = "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
        .lay = .{ .desc = "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z" },
        .side = .{ .desc = "lay edge: left of the polyline direction (left of a left-to-right line = up) or right" },
        .gauge = .{ .def = "18", .desc = "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
        .width = .{ .len = .pos, .def = "1.25", .desc = "extent along Z (lay edge) or in-plane (lay face)" },
        .fasteners = .{ .def = "null", .desc = "text for notes, e.g. \"(10) 10d EA. END\"" },
    };
};

fn buildConnector(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(ConnectorParams);
    const model_name = cp.model;
    var hw: ?Hardware = null;
    if (model_name.len > 0) {
        for (hardware) |h| if (std.ascii.eqlIgnoreCase(h.model, model_name)) {
            hw = h;
            break;
        };
    }
    const gauge_default: f64 = if (hw) |h| @floatFromInt(h.gauge) else 18;
    const gauge_n = cp.gauge orelse gauge_default;
    const width: f64 = cp.width orelse if (hw) |h| h.width else 1.25;
    const pv = cp.points orelse {
        p.fail("points", "connector needs 'points': the strap polyline, e.g. [\"upper_plate@top_right\", \"beam@top_left\"] (Refs or [x, y])", .{});
        return null;
    };
    if (!p.ok) return null;
    const gauge_i: u32 = cast.toInt(u32, @round(gauge_n)) orelse 0;
    const thickness = gaugeThickness(gauge_i) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 10, 11, 12, 14, 16, 18, 20, 22, 24, 26 or 28", .{fmtNum(a, gauge_n)});
        return null;
    };
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a connector needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    const edge = cp.lay == .edge;
    const rib = if (edge)
        try path_geom.ribbon(a, clean, if (cp.side == .left) thickness else 0, if (cp.side == .right) thickness else 0)
    else
        try path_geom.ribbon(a, clean, width / 2, width / 2);
    const prism = Prism{ .material = "steel", .loops = try model.oneLoop(a, rib), .centerline = clean, .face_tie = !edge };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .nat_z = if (edge) width else thickness,
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "connector {s}{s}{s}{d} ga x {s} lay {s}", .{
            model_name,
            if (model_name.len > 0) " " else "",
            if (hw) |h| try std.fmt.allocPrint(a, "{s} ", .{h.kind}) else "",
            gauge_i,
            ftin(a, width),
            @tagName(cp.lay),
        }),
    };
}

// ---- truss -----------------------------------------------------------------------------------------------------------

pub const TrussParams = struct {
    exterior: enum { left, right } = .left,
    pitch: json.Value = .{ .string = "4:12" },
    top_chord: []const u8 = "2x4",
    bottom_chord: []const u8 = "2x4",
    heel: enum { standard, raised } = .standard,
    heel_height: ?f64 = null,
    bearing_width: f64 = 3.5,
    overhang: f64 = 12,
    tail: enum { plumb, square } = .plumb,
    span_shown: f64 = 48,
    plate: bool = true,

    pub const spec = .{
        .exterior = .{ .desc = "side of the heel/overhang (right mirrors)" },
        .pitch = .{ .def = "4:12", .desc = "rise:run" },
        .top_chord = .{ .desc = "sawn nominal size, depth in-plane" },
        .bottom_chord = .{ .desc = "sawn nominal size, depth in-plane" },
        .heel = .{ .desc = "standard | raised" },
        .heel_height = .{ .len = .pos, .desc = "raised heel: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
        .bearing_width = .{ .len = .pos, .desc = "width of the support under the heel" },
        .overhang = .{ .len = .any, .desc = "horizontal distance from outer face of bearing to the tail end" },
        .tail = .{ .desc = "plumb | square cut" },
        .span_shown = .{ .len = .pos, .desc = "how far into the building to draw (crop/break at the end)" },
        .plate = .{ .desc = "draw the heel truss plate outline (dashed hidden pen)" },
    };
};

fn buildTruss(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const tp = p.parse(TrussParams) orelse return null;
    const top = tp.top_chord;
    const bot = tp.bottom_chord;
    const theta = units.parseSlope(tp.pitch) orelse {
        p.fail("pitch", "param 'pitch' must be rise:run like \"4:12\" or degrees (got {s})", .{model.kindOrText(a, tp.pitch)});
        return null;
    };
    if (theta <= 0 or theta >= std.math.pi / 2.0) {
        p.fail("pitch", "pitch must slope up (between 0 and 90 degrees)", .{});
        return null;
    }
    const tsz = parseSawn(top) orelse {
        p.fail("top_chord", "top_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{top});
        return null;
    };
    const bsz = parseSawn(bot) orelse {
        p.fail("bottom_chord", "bottom_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{bot});
        return null;
    };
    const dt = tsz.d;
    const db = bsz.d;
    const s = @tan(theta);
    const c = @cos(theta);
    const raised = tp.heel == .raised;
    const v_thick = dt / c; // vertical thickness of the top chord
    var y_low0 = db; // lower edge of the top chord at x = 0
    if (raised) {
        const hh = tp.heel_height orelse {
            p.fail("heel_height", "heel 'raised' needs 'heel_height' (vertical height at the bearing outer edge from top of bottom chord to top of top chord)", .{});
            return null;
        };
        if (hh < v_thick) {
            p.fail("heel_height", "heel_height {s} is smaller than the top chord's vertical thickness {s}; the raised heel must be at least that tall", .{ fmtNum(a, hh), fmtNum(a, v_thick) });
            return null;
        }
        y_low0 = db + hh - v_thick;
    }
    const x_tail = -tp.overhang;
    const xe = tp.span_shown;
    const lower = struct {
        fn f(x: f64, y0: f64, sl: f64) f64 {
            return y0 + sl * x;
        }
    }.f;
    const tail_bottom = V2.init(x_tail, lower(x_tail, y_low0, s));
    var tail_top = V2.init(x_tail, tail_bottom.y + v_thick);
    var tail_bottom_pt = tail_bottom;
    if (tp.tail == .square) {
        // end cut perpendicular to the chord, hanging through the plumb-cut lower point
        tail_bottom_pt = tail_bottom;
        tail_top = tail_bottom.add(V2.init(-@sin(theta), @cos(theta)).scale(dt));
    }
    const top_loop = try a.dupe(Pt, &.{
        Pt.at(tail_bottom_pt, 0),
        .{ .x = xe, .y = lower(xe, y_low0, s) },
        .{ .x = xe, .y = lower(xe, y_low0, s) + v_thick },
        Pt.at(tail_top, 0),
    });
    const bot_loop = try model.rectLoop(a, 0, 0, xe, db);
    var prisms: std.ArrayList(Prism) = .empty;
    try prisms.append(a, .{ .part = "bottom_chord", .material = "wood", .loops = try model.oneLoop(a, bot_loop) });
    try prisms.append(a, .{ .part = "top_chord", .material = "wood", .loops = try model.oneLoop(a, top_loop) });
    var zones: std.ArrayList(model.Zone) = .empty;
    try zones.append(a, try zoneRect(a, "bottom_chord", 0, 0, xe, db));
    try zones.append(a, .{ .name = "top_chord", .loops = try model.oneLoop(a, top_loop), .box = geom.loopBox(top_loop) });
    if (raised) {
        const web_w = 1.5;
        const web = try a.dupe(Pt, &.{
            .{ .x = 0, .y = db },
            .{ .x = web_w, .y = db },
            .{ .x = web_w, .y = lower(web_w, y_low0, s) },
            .{ .x = 0, .y = y_low0 },
        });
        try prisms.append(a, .{ .part = "heel_web", .material = "wood", .loops = try model.oneLoop(a, web) });
        try zones.append(a, .{ .name = "heel_web", .loops = try model.oneLoop(a, web), .box = geom.loopBox(web) });
    }
    if (tp.plate) {
        const py1 = @max(db + 0.5, y_low0 + 0.5);
        const pl = try model.rectLoop(a, 0.25, 0.25, 5.25, py1);
        try prisms.append(a, .{ .part = "plate", .material = "steel", .loops = try model.oneLoop(a, pl), .kind = .ghost, .pen = .hidden, .embedded = true });
        try zones.append(a, try zoneRect(a, "plate", 0.25, 0.25, 5.25, py1));
    }
    // tail zone: the part of the top chord outside the bearing
    {
        var tb = Box{};
        tb.addPoint(tail_bottom_pt.x, tail_bottom_pt.y);
        tb.addPoint(tail_top.x, tail_top.y);
        tb.addPoint(0, lower(0, y_low0, s));
        tb.addPoint(0, lower(0, y_low0, s) + v_thick);
        try zones.append(a, .{ .name = "tail", .loops = try model.oneLoop(a, try model.rectLoop(a, tb.x0, tb.y0, tb.x1, tb.y1)), .box = tb });
    }
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "bearing_outer", .p = V2.init(0, 0) });
    try anchors.append(a, .{ .name = "bearing_inner", .p = V2.init(tp.bearing_width, 0) });
    try anchors.append(a, .{ .name = "tail_bottom", .p = tail_bottom_pt });
    try anchors.append(a, .{ .name = "tail_top", .p = tail_top });
    try anchors.append(a, .{ .name = "top_chord_at_bearing", .p = V2.init(0, lower(0, y_low0, s) + v_thick) });
    // SPEC 19: where ties and straps land on the heel without literal offsets
    try anchors.append(a, .{ .name = "heel_outer", .p = V2.init(0, (db + lower(0, y_low0, s) + v_thick) / 2) });
    try anchors.append(a, .{ .name = "top_chord_bottom_at_bearing", .p = V2.init(0, lower(0, y_low0, s)) });
    try anchors.append(a, .{ .name = "top_chord_end", .p = V2.init(xe, lower(xe, y_low0, s) + v_thick) });
    try anchors.append(a, .{ .name = "bottom_chord_top_inner", .p = V2.init(xe, db) });
    var box = Box{};
    for (prisms.items) |pr| if (pr.kind != .ghost) {
        for (pr.loops) |l| box.addBox(geom.loopBox(l));
    };
    var built = Built{
        .prisms = prisms.items,
        .anchors = anchors.items,
        .zones = zones.items,
        .box = box,
        .nat_z = tsz.t,
        .info = try std.fmt.allocPrint(a, "truss {s}:12 {s} heel, {s}+{s} chords, ovh {s}", .{ fmtNum(a, @tan(theta) * 12.0), @tagName(tp.heel), top, bot, ftin(a, tp.overhang) }),
    };
    if (tp.exterior == .right) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}

// ---- membrane -----------------------------------------------------------------------------------------------------------

fn membraneThickness(material: []const u8) f64 {
    const eq = std.mem.eql;
    if (eq(u8, material, "vapor_retarder")) return 0.04;
    if (eq(u8, material, "shingles")) return 0.25;
    if (eq(u8, material, "underlayment")) return 0.06;
    if (eq(u8, material, "wrb")) return 0.04;
    if (eq(u8, material, "flashing_membrane")) return 0.06;
    return 0.05;
}

pub const MembraneParams = struct {
    material: enum { membrane, underlayment, vapor_retarder, wrb, shingles, flashing_membrane } = .membrane,
    points: ?json.Value = null,
    thickness: ?f64 = null,
    side: enum { left, right } = .left,
    until: ?json.Value = null,

    pub const spec = .{
        .material = .{ .desc = "underlayment | vapor_retarder | wrb | shingles | flashing_membrane | membrane" },
        .points = .{ .def = "required", .desc = "polyline [x,y] or Refs" },
        .thickness = .{ .len = .pos, .def = "per material", .desc = "draw thickness (vapor retarder 0.04, shingles 0.25 typical)" },
        .side = .{ .desc = "which side of the polyline direction the thickness grows: left of dx,dy is (-dy,dx), right the opposite" },
        .until = .{ .desc = "a Ref (or {ref, offset}): the LAST segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction. With `slope`: \"@truss\" and points [[0,0],[12,0]] a roofing layer follows the roof and stops at e.g. \"truss@top_chord_end\"" },
    };
};

fn buildMembrane(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const mp = p.parseAll(MembraneParams);
    const material = @tagName(mp.material);
    const pv = mp.points orelse {
        p.fail("points", "membrane needs 'points': the layer polyline, e.g. [\"roof_sheathing@top_left\", \"roof_sheathing@top_right\"]", .{});
        return null;
    };
    if (!p.ok) return null;
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a membrane needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    var until_note: []const u8 = "";
    if (mp.until) |uv| {
        // SPEC 19: the last segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction
        const path = try std.fmt.allocPrint(a, "{s}/{s}/until", .{ p.base, p.id });
        const target = ctx.scene.point(uv, p.id, path) orelse {
            p.ok = false;
            return null;
        };
        const last = clean.len - 1;
        const seg = V2.init(clean[last].x - clean[last - 1].x, clean[last].y - clean[last - 1].y);
        if (seg.len() < 1e-9) {
            p.fail("until", "'until' needs a last segment with a direction (the last two points coincide)", .{});
            return null;
        }
        const u = seg.norm();
        // the polyline is local (world - origin, then rotated by the placement angle): bring the target into that frame
        const rel = target.sub(ctx.origin);
        const ca = @cos(-ctx.angle);
        const sa = @sin(-ctx.angle);
        const tl = V2.init(rel.x * ca - rel.y * sa, rel.x * sa + rel.y * ca);
        const new_len = (tl.x - clean[last - 1].x) * u.x + (tl.y - clean[last - 1].y) * u.y;
        if (new_len <= 1e-6) {
            p.fail("until", "'until' target lies at or behind the start of the last segment along its direction (length would be {s})", .{fmtNum(a, new_len)});
            return null;
        }
        clean[last].x = clean[last - 1].x + u.x * new_len;
        clean[last].y = clean[last - 1].y + u.y * new_len;
        const ref_txt: []const u8 = switch (uv) {
            .string => |t| t,
            .object => if (uv.get("ref")) |r| (r.str() orelse "ref") else "ref",
            else => "ref",
        };
        until_note = try std.fmt.allocPrint(a, " (until {s})", .{ref_txt});
    }
    const left = mp.side == .left;
    const t: f64 = mp.thickness orelse membraneThickness(material);
    const rib = try path_geom.ribbon(a, clean, if (left) t else 0, if (left) 0 else t);
    // The drawn line sits at mid-thickness; vapor retarders keep a minimum separation from the host
    // (0.03 paper inch is applied at draw time via `line_gap`, here only the mid-thickness).
    const line = try path_geom.offsetOpen(a, clean, if (left) t / 2 else -t / 2);
    const prism = Prism{
        .material = material,
        .loops = try model.oneLoop(a, rib),
        .kind = .line,
        .line_pts = line,
        .ticks = std.mem.eql(u8, material, "shingles"),
        .centerline = clean,
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "membrane {s} {s} thick L={s}{s}", .{ material, ftin(a, t), ftin(a, pathLen(try vsOf(a, clean))), until_note }),
    };
}

// ---- fill ------------------------------------------------------------------------------------------------------------------

pub const FillParams = struct {
    material: enum { earth, gravel, sand, compacted_fill } = .earth,
    points: ?json.Value = null,
    outline: enum { top, full, none } = .top,
    grade_label: []const u8 = "",

    pub const spec = .{
        .material = .{ .desc = "earth | gravel | sand | compacted_fill" },
        .points = .{ .def = "required", .desc = "polygon [x,y] or Refs" },
        .outline = .{ .desc = "top (stroke only edges with outward normal up: the grade line) | full | none" },
        .grade_label = .{ .def = "null", .desc = "optional text for annotations" },
    };
};

fn buildFill(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const fp = p.parseAll(FillParams);
    const pv = fp.points orelse {
        p.fail("points", "fill needs 'points': a polygon [[x, y], ...] or Refs", .{});
        return null;
    };
    if (!p.ok) return null;
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 3) {
        p.fail("points", "a fill polygon needs at least 3 distinct points (got {d})", .{clean.len});
        return null;
    }
    const loop = try orientedCcw(a, clean);
    const om: model.OutlineMode = switch (fp.outline) {
        .top => .top,
        .full => .full,
        .none => .none,
    };
    const prism = Prism{ .material = @tagName(fp.material), .loops = try model.oneLoop(a, loop), .outline = om };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(loop),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "fill {s}", .{@tagName(fp.material)}),
    };
}

// ---- insulation ----------------------------------------------------------------------------------------------------------------

pub const InsulationParams = struct {
    form: enum { rigid, batt } = .rigid,
    width: ?f64 = null,
    height: ?f64 = null,
    points: ?json.Value = null,

    pub const spec = .{
        .form = .{ .desc = "rigid | batt" },
        .width = .{ .len = .pos, .hint = "give width and height, or points", .also = &.{"height"}, .def = "rect: required unless points", .desc = "box size" },
        .height = .{ .len = .pos, .hint = "give width and height, or points", .row = false, .desc = "box size" },
        .points = .{ .desc = "polygon alternative to width/height" },
    };
};

fn buildInsulation(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const ip = p.parse(InsulationParams) orelse return null;
    var loop: []const Pt = undefined;
    var points_mode = false;
    if (ip.points) |pv| {
        const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("points", "insulation polygon needs at least 3 points", .{});
            return null;
        }
        loop = try orientedCcw(a, clean);
        points_mode = true;
    } else {
        const hint = "give width and height, or points";
        const w = ip.width orelse {
            p.missing("width", hint);
            return null;
        };
        const h = ip.height orelse {
            p.missing("height", hint);
            return null;
        };
        loop = try model.rectLoop(a, 0, 0, w, h);
    }
    const batt = ip.form == .batt;
    var prism = Prism{
        .material = if (batt) "insulation_batt" else "insulation_rigid",
        .loops = try model.oneLoop(a, loop),
    };
    const bx = geom.loopBox(loop);
    if (batt) {
        prism.kind = .batt;
        prism.line_pts = try battSymbol(a, bx);
    }
    return .{
        .prisms = try onePrism(a, prism),
        .box = bx,
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "insulation {s}", .{@tagName(ip.form)}),
    };
}

/// Sinusoidal loop line fitted to the box (batt insulation symbol).
fn battSymbol(a: Allocator, bx: Box) Allocator.Error![]const Pt {
    const horizontal = bx.width() >= bx.height();
    const long = if (horizontal) bx.width() else bx.height();
    const short = if (horizontal) bx.height() else bx.width();
    const loops: f64 = @min(@max(2, @round(long / (short * 0.9))), 2000);
    const pad = short * 0.2;
    const pitch = (long - 2 * pad) / loops;
    const loop_w = 1.7 * pitch / (2.0 * std.math.pi);
    const amp = 0.42 * short;
    const steps_per = 20;
    const total: usize = cast.toIntClamped(usize, loops, 2, 2000) * steps_per;
    var out: std.ArrayList(Pt) = .empty;
    var i: usize = 0;
    while (i <= total) : (i += 1) {
        const t = 2.0 * std.math.pi * loops * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(total));
        const u = pad + (long - 2 * pad) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(total)) - loop_w * @sin(t);
        const v = amp * @cos(t);
        const along = u;
        if (horizontal) {
            try out.append(a, .{ .x = bx.x0 + along, .y = (bx.y0 + bx.y1) / 2 + v });
        } else {
            try out.append(a, .{ .x = (bx.x0 + bx.x1) / 2 + v, .y = bx.y0 + along });
        }
    }
    return out.items;
}

// ---- solid ------------------------------------------------------------------------------------------------------------------------

pub const SolidParams = struct {
    profile: ?json.Value = null,
    material: []const u8,

    pub const spec = .{
        .profile = .{ .def = "required", .desc = "{rect:[w,h]} | {circle:d} | {points:[...]}" },
        .material = .{ .desc = "any style material (aluminum, steel, ...)" },
    };
};

fn buildSolid(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const sp = p.parseAll(SolidParams);
    const prof = sp.profile orelse {
        p.fail("profile", "solid needs 'profile': {{\"rect\": [w, h]}}, {{\"circle\": d}} or {{\"points\": [[x, y], ...]}}", .{});
        return null;
    };
    if (!p.ok) return null;
    if (!materialOk(ctx, "material", sp.material)) return null;
    var loop: []const Pt = undefined;
    var points_mode = false;
    if (prof.get("rect")) |rv| {
        const arr = rv.arr();
        if (arr == null or arr.?.len != 2) {
            p.fail("profile/rect", "profile.rect must be [width, height]", .{});
            return null;
        }
        const w = units.parseLength(arr.?[0]);
        const h = units.parseLength(arr.?[1]);
        if (w == null or h == null or w.? <= 0 or h.? <= 0) {
            p.fail("profile/rect", "profile.rect must be two positive lengths [width, height]", .{});
            return null;
        }
        loop = try model.rectLoop(a, 0, 0, w.?, h.?);
    } else if (prof.get("circle")) |cv| {
        const dd = units.parseLength(cv);
        if (dd == null or dd.? <= 0) {
            p.fail("profile/circle", "profile.circle must be a positive diameter", .{});
            return null;
        }
        loop = try model.circleLoop(a, dd.? / 2, dd.? / 2, dd.? / 2);
    } else if (prof.get("points")) |pv| {
        const pts = (try parsePointList(ctx, "profile/points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("profile/points", "profile.points needs at least 3 points", .{});
            return null;
        }
        loop = try orientedCcw(a, clean);
        points_mode = true;
    } else {
        p.fail("profile", "profile must have one of 'rect', 'circle' or 'points'", .{});
        return null;
    }
    const prism = Prism{ .material = sp.material, .loops = try model.oneLoop(a, loop) };
    const bx = geom.loopBox(loop);
    return .{
        .prisms = try onePrism(a, prism),
        .box = bx,
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "solid {s} (escape hatch)", .{sp.material}),
    };
}

// ---- flashing -----------------------------------------------------------------------------------------------------

pub const FlashingParams = struct {
    profile: enum { z, l, drip, weep_screed, points } = .z,
    flange: ?f64 = null,
    leg: ?f64 = null,
    drop: ?f64 = null,
    kick: f64 = 0.5,
    gauge: f64 = 26,
    exterior: enum { left, right } = .left,
    points: ?json.Value = null,

    pub const spec = .{
        .profile = .{ .desc = "z: back flange up the wall, horizontal leg out, drop at the nose; l: flange + horizontal leg; drip: flange on the deck, drop, outward kick; weep_screed: nailing flange up the wall, ledge, small drip drop; points: free centerline polyline" },
        .flange = .{ .len = .pos, .def = "2 (weep_screed 3.5)", .desc = "vertical back/nailing flange length (drip: horizontal flange on the deck)" },
        .leg = .{ .len = .pos, .def = "1 (l 2)", .desc = "horizontal leg length toward the exterior" },
        .drop = .{ .len = .pos, .def = "2 (drip 1.5, weep_screed 0.5)", .desc = "downturned leg at the nose" },
        .kick = .{ .len = .pos, .desc = "drip only: outward kick at the bottom of the drop" },
        .gauge = .{ .desc = "20 .0359, 22 .0299, 24 .0239, 26 .0179, 28 .0149" },
        .exterior = .{ .desc = "side the nose faces (right mirrors); presets only" },
        .points = .{ .def = "profile points: required", .desc = "centerline polyline [x,y] relative to the placement point, or Refs" },
    };
};

fn buildFlashing(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const fp = p.parse(FlashingParams) orelse return null;
    const pr = fp.profile;
    const flange: f64 = fp.flange orelse if (pr == .weep_screed) 3.5 else 2;
    const leg: f64 = fp.leg orelse if (pr == .l) 2 else 1;
    const drop: f64 = fp.drop orelse switch (pr) {
        .drip => 1.5,
        .weep_screed => 0.5,
        else => 2,
    };
    const kick = fp.kick;
    const gauge_i: u32 = cast.toInt(u32, @round(fp.gauge)) orelse 0;
    const thickness = gaugeThickness(gauge_i) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 20, 22, 24, 26 (default, 0.0179\") or 28", .{fmtNum(a, fp.gauge)});
        return null;
    };
    var pts: std.ArrayList(Pt) = .empty;
    var points_mode = false;
    // local frame: corner (the first bend) at (0,0); the wall surface is x = 0 and the exterior is -x
    if (pr == .z or pr == .weep_screed) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 }, .{ .x = -leg, .y = -drop } });
    } else if (pr == .l) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 } });
    } else if (pr == .drip) {
        try pts.appendSlice(a, &.{ .{ .x = flange, .y = 0 }, .{ .x = 0, .y = 0 }, .{ .x = 0, .y = -drop }, .{ .x = -kick, .y = -drop - 0.5 * kick } });
    } else {
        const pv = fp.points orelse {
            p.fail("points", "profile \"points\" needs 'points': the sheet-metal centerline polyline, e.g. [[0,2],[0,0],[-1,0],[-1,-2]] (or Refs)", .{});
            return null;
        };
        const pl = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pl);
        if (clean.len < 2) {
            p.fail("points", "flashing points need at least 2 distinct points (got {d})", .{clean.len});
            return null;
        }
        try pts.appendSlice(a, clean);
        points_mode = true;
    }
    const rib = try path_geom.ribbon(a, pts.items, thickness / 2, thickness / 2);
    const mat = materialOr(ctx, "steel", "generic");
    const prism = Prism{ .material = mat, .loops = try model.oneLoop(a, rib), .embedded = true, .centerline = pts.items };
    const anchors = try a.dupe(model.NamedAnchor, &.{
        .{ .name = "corner", .p = if (points_mode) pts.items[0].v() else V2.init(0, 0) },
        .{ .name = "start", .p = pts.items[0].v() },
        .{ .name = "end", .p = pts.items[pts.items.len - 1].v() },
    });
    var built = Built{
        .prisms = try onePrism(a, prism),
        .anchors = anchors,
        .box = geom.loopBox(rib),
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "flashing {s} {d} ga ({s}\" thick)", .{ @tagName(pr), gauge_i, fmtNum(a, thickness) }),
    };
    if (!points_mode and fp.exterior == .right) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}

// ---- joint --------------------------------------------------------------------------------------------------------

pub const JointParams = struct {
    kind: enum { expansion, control, tooled_edge, sealant },
    width: ?f64 = null,
    depth: ?f64 = null,
    in: ?json.Value = null,
    cap: f64 = 0,
    radius: f64 = 0.25,
    corner: enum { top_right, top_left, bottom_right, bottom_left } = .top_right,
    backer_rod: bool = true,

    pub const spec = .{
        .kind = .{ .desc = "expansion | control | tooled_edge | sealant" },
        .width = .{ .len = .pos, .def = "0.5 (control 0.25)", .desc = "expansion: filler thickness; control: notch width at the top; sealant: joint gap width" },
        .depth = .{ .len = .pos, .def = "expansion 4, control 1, sealant 0.25", .desc = "expansion: filler depth below the top (set to the slab thickness, or give `in`); control: notch depth (default 1/4 of the `in` zone height); sealant: bead depth" },
        .in = .{ .desc = "optional host zone \"comp[.part]\" whose height sets the default depth (expansion: full height; control: 1/4)" },
        .cap = .{ .len = .any, .desc = "expansion: depth of a sealant cap at the top of the filler (part `sealant`)" },
        .radius = .{ .len = .pos, .desc = "tooled_edge: radius of the rounded corner" },
        .corner = .{ .desc = "tooled_edge: which corner of the concrete the point is: top_right (concrete lies left and below), top_left, bottom_right, bottom_left" },
        .backer_rod = .{ .desc = "sealant: draw the backer rod circle (diameter 1.25*width) below the bead" },
    };
};

fn buildJoint(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const jp = p.parse(JointParams) orelse return null;
    const k = jp.kind;
    // optional host zone: default depth = slab thickness (expansion) or a quarter of it (control)
    var host_h: f64 = 0;
    if (jp.in) |iv| {
        const in_s = iv.str() orelse {
            p.fail("in", "param 'in' must be \"<component>[.<part>]\", the concrete zone the joint cuts (its height sets the default depth)", .{});
            return null;
        };
        var hid = in_s;
        var part: ?[]const u8 = null;
        if (std.mem.indexOfScalar(u8, in_s, '.')) |dot| {
            hid = in_s[0..dot];
            part = in_s[dot + 1 ..];
        }
        const host = ctx.scene.find(hid) orelse {
            const ids = try ctx.scene.compIds(a);
            p.fail("in", "no component '{s}'. Known ids: {s}", .{ hid, scene_mod.joinIds(a, ids) });
            return null;
        };
        if (host.state != .ok) {
            p.fail("in", "host '{s}' did not build; fix its errors first", .{hid});
            return null;
        }
        const bx = if (part) |pt| (scene_mod.Scene.partBox(host, pt) orelse {
            p.fail("in", "component '{s}' has no part '{s}'", .{ hid, pt });
            return null;
        }) else host.built.box;
        host_h = bx.height();
    }
    const width: f64 = jp.width orelse if (k == .control) 0.25 else 0.5;
    const depth_def: f64 = switch (k) {
        .control => if (host_h > 0) host_h / 4.0 else 1.0,
        .sealant => @max(0.25, 0.5 * width),
        else => if (host_h > 0) host_h else 4.0,
    };
    const depth: f64 = jp.depth orelse depth_def;
    const radius = jp.radius;
    const cap = jp.cap;
    const rod = jp.backer_rod;
    var prisms: std.ArrayList(Prism) = .empty;
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "joint_top", .p = V2.init(0, 0) });
    const void_mat = materialOr(ctx, "void", "generic");
    if (k == .expansion) {
        if (cap < 0 or cap >= depth) {
            p.fail("cap", "cap (sealant depth at the top of the joint) must be from 0 to less than depth {s} (got {s})", .{ fmtNum(a, depth), fmtNum(a, cap) });
            return null;
        }
        const hw = width / 2;
        try prisms.append(a, .{ .part = "filler", .material = materialOr(ctx, "joint_filler", "generic"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -depth, hw, -cap)) });
        if (cap > 0) try prisms.append(a, .{ .part = "sealant", .material = materialOr(ctx, "sealant", "steel"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -cap, hw, 0)) });
    } else if (k == .control) {
        const tri = [_]Pt{ .{ .x = -width / 2, .y = 0 }, .{ .x = 0, .y = -depth }, .{ .x = width / 2, .y = 0 } };
        try prisms.append(a, .{ .part = "notch", .material = void_mat, .loops = try model.oneLoop(a, try orientedCcw(a, &tri)), .embedded = true });
    } else if (k == .tooled_edge) {
        // the sliver between the sharp corner and the radius, drawn for the top-right corner then flipped into place
        const r = radius;
        const loop0 = [_]Pt{ .{ .x = 0, .y = 0 }, .{ .x = -r, .y = 0, .b = geom.bulgeFromSweep(-std.math.pi / 2.0) }, .{ .x = 0, .y = -r } };
        const c = jp.corner;
        const sx: f64 = if (c == .top_left or c == .bottom_left) -1 else 1;
        const sy: f64 = if (c == .bottom_left or c == .bottom_right) -1 else 1;
        const xf = geom.Xf.scaling(sx, sy);
        const loop = try orientedCcw(a, try xf.applyLoop(a, &loop0));
        try prisms.append(a, .{ .part = "radius", .material = void_mat, .loops = try model.oneLoop(a, loop), .embedded = true });
        try anchors.append(a, .{ .name = "corner", .p = V2.init(0, 0) });
    } else {
        // sealant bead over a backer rod, in a gap of `width`
        const hw = width / 2;
        try prisms.append(a, .{ .part = "bead", .material = materialOr(ctx, "sealant", "steel"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -depth, hw, 0)) });
        if (rod) {
            const rd = 1.25 * width;
            const loop = try model.circleLoop(a, 0, -depth - rd / 2, rd / 2);
            try prisms.append(a, .{ .part = "rod", .material = materialOr(ctx, "backer_rod", "generic"), .loops = try model.oneLoop(a, loop) });
        }
    }
    return .{
        .prisms = prisms.items,
        .anchors = anchors.items,
        .box = boxOfPrisms(prisms.items),
        .info = try std.fmt.allocPrint(a, "joint {s} {s} wide x {s} deep", .{ @tagName(k), ftin(a, width), ftin(a, depth) }),
    };
}

// ---- dispatch -----------------------------------------------------------------------------------------------------------------------

pub fn build(ctx: *Ctx) BuildError!?Built {
    return switch (ctx.comp.ty.type) {
        .lumber => lumber.build(ctx),
        .panel => panel.build(ctx),
        .cmu_wall => cmu_wall.build(ctx),
        .concrete => concrete.build(ctx),
        .rebar => rebar.build(ctx),
        .anchor_bolt => anchor_bolt.build(ctx),
        .connector => buildConnector(ctx),
        .truss => buildTruss(ctx),
        .membrane => buildMembrane(ctx),
        .fill => buildFill(ctx),
        .insulation => buildInsulation(ctx),
        .solid => buildSolid(ctx),
        .flashing => buildFlashing(ctx),
        .joint => buildJoint(ctx),
    };
}

/// The parameter struct of every component type, in `catalog.Type` order.
pub const param_structs = .{ lumber.Params, panel.Params, cmu_wall.Params, concrete.Params, rebar.Params, anchor_bolt.Params, ConnectorParams, TrussParams, MembraneParams, FillParams, InsulationParams, SolidParams, FlashingParams, JointParams };

comptime {
    if (param_structs.len != std.meta.tags(catalog.Type).len) @compileError("builders.param_structs must have one struct per catalog.Type tag");
}

test "every choice of an enum parameter is mentioned in its catalog row (docs cannot drift from the parser)" {
    inline for (param_structs) |S| {
        const rows = params_mod.rows(S);
        inline for (@typeInfo(S).@"struct".fields) |f| {
            const Base = switch (@typeInfo(f.type)) {
                .optional => |o| o.child,
                else => f.type,
            };
            if (@typeInfo(Base) == .@"enum") {
                var found_row = false;
                for (rows) |r| {
                    if (!std.mem.eql(u8, r.names[0], f.name)) continue;
                    found_row = true;
                    inline for (@typeInfo(Base).@"enum".fields) |ef| {
                        if (std.mem.indexOf(u8, r.desc, ef.name) == null and std.mem.indexOf(u8, r.def, ef.name) == null) {
                            std.debug.print("{s}.{s}: choice '{s}' is not in its catalog text\n", .{ @typeName(S), f.name, ef.name });
                            return error.TestUnexpectedResult;
                        }
                    }
                }
                try std.testing.expect(found_row);
            }
        }
    }
}
