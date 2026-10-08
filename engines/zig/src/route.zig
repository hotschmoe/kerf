//! Note routing (SPEC 6.3, 16, 18): column placement, leader de-crossing, landing-point nudging and
//! leader-hit detection.
//!
//! Pure geometry; no document types. The caller (annot.zig) supplies the notes' text-block sizes,
//! candidate landing points (index 0 is the SPEC 6.3 label point), the dimension-text / label boxes
//! that leaders must stay away from, and the dimension line segments (soft obstacles).
//!
//! Algorithm (deterministic; no randomness, no hash order):
//!  1. Baseline = SPEC 6.3 (sort by landing y, stack top-down) + the SPEC 16 adjacent-swap de-crossing.
//!     A clean baseline is returned untouched.
//!  2. If any leader crosses or comes within one text height of another leader, a note box, a dimension
//!     text or a label, each column is re-solved exactly by dynamic programming: for notes in their
//!     column order, pick a landing candidate and a vertical position (a grid of half-pitch steps around
//!     the landing y) minimizing the sum of per-leader costs (obstacle hits, dimension-line crossings,
//!     steepness, length, distance from the preferred landing) plus the leader/leader cost of adjacent
//!     notes, subject to the text blocks not overlapping. Both columns are solved in turn (each sees the
//!     other's leaders as fixed). If hits remain, discrete changes are tried one at a time and re-solved:
//!     a note moves to the other column (`notes_side` "both"), swaps with a neighbour, or is re-inserted
//!     at another position of its column; the first change that lowers (hits, cost) is kept.
//!  3. Whatever hits remain are returned with concrete fix proposals (`at` / `place`).

const std = @import("std");
const sort = @import("sort.zig");
const geom = @import("geom.zig");
const V2 = geom.V2;
const Box = geom.Box;
const Allocator = std.mem.Allocator;

pub const Geo = struct { h: f64, pitch: f64, gap: f64, shoulder: f64, pad: f64 };

pub const Side = enum { left, right, both };

pub const NoteIn = struct {
    /// Text block size (model units).
    w: f64,
    hgt: f64,
    /// Candidate landing points; `cands[0]` is the preferred (SPEC 6.3) one.
    cands: []const V2,
    /// False when the author gave `at` (the landing is exact and never nudged).
    movable: bool,
    /// Designer override of the text position (top-left of the block).
    place: ?V2,
    /// Per-note column hint (`column` on the note): the note never leaves that column.
    column: ?Side = null,
};

pub const ObstKind = enum { dim, label };
pub const Obst = struct { kind: ObstKind, poly: [4]V2 };

/// Layout effort counter (REVIEW LAY-1): one unit per leader/leader, leader/box or leader/obstacle penalty evaluation. The search
/// stops improving once `used >= limit` and the layout falls back to what it has (SPEC 6.3 stacking plus de-crossing, or the best
/// state so far); the work is shared by every pass and every trial of one view build, so no document can cost more than a bounded
/// amount of time however many notes it has.
pub const Work = struct {
    used: u64 = 0,
    limit: u64 = default_work_limit,

    pub fn spent(self: *const Work) bool {
        return self.used >= self.limit;
    }
};

/// 40 million units is about 1.5 s of ReleaseFast: 50 notes + 10 dims + 5 labels reach it, the largest reference view uses 68 thousand.
pub const default_work_limit: u64 = 40_000_000;

/// `Layout.hits` keeps every hit, but concrete fix proposals are only computed for the first `max_fix_hits` of them: the caller
/// reports at most that many (`annot.reportHits`), and a proposal costs a full layout plus ~60 hit scans.
pub const max_fix_hits: usize = 8;

pub const Params = struct {
    geo: Geo,
    crop: Box,
    xl: f64,
    xr: f64,
    gutter: f64,
    side: Side,
    /// Re-routes inside the caller's repair loop: one search attempt with a small budget.
    light: bool = false,
    /// Shared effort counter; null = a fresh default budget for this call.
    work: ?*Work = null,
};

pub const HitKind = enum { leader, note, dim, label };

pub const Hit = struct {
    /// The note whose leader is involved.
    note: usize,
    kind: HitKind,
    /// Note index (leader, note) or obstacle index (dim, label).
    other: usize,
    /// Smallest distance between the leader and the other item (0 = they cross), model units.
    dist: f64,
    /// Proposed fixes for `note` (null when none was found).
    fix_at: ?V2 = null,
    fix_place: ?V2 = null,
    /// Same, for the other note when kind is `.leader`.
    other_fix_at: ?V2 = null,
    other_fix_place: ?V2 = null,
};

pub const Layout = struct {
    top: []f64,
    x: []f64,
    left: []bool,
    landing: []V2,
    leaders: [][3]V2,
    hits: []Hit,
    /// Number of crossings + near hits before the search (diagnostics for tests).
    searched: bool,
    /// The work budget ran out: the notes are placed by the basic rule (plus whatever the search had found) and hits may remain.
    budget_exhausted: bool = false,
};

const W_CROSS: f64 = 1000;
const W_NEAR_BASE: f64 = 100;
const W_NEAR: f64 = 200;
const W_SOFT_CROSS: f64 = 25;

// ---- geometry helpers -----------------------------------------------------------------------------------------

fn cross3(o: V2, a: V2, b: V2) f64 {
    return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

/// Plain floating point proper-or-touching intersection test (leaders are drawn, not constructed:
/// no exact arithmetic needed, and this is the hot loop of the search).
fn segsCross(a0: V2, a1: V2, b0: V2, b1: V2) bool {
    const d1 = cross3(b0, b1, a0);
    const d2 = cross3(b0, b1, a1);
    const d3 = cross3(a0, a1, b0);
    const d4 = cross3(a0, a1, b1);
    if (((d1 > 0 and d2 < 0) or (d1 < 0 and d2 > 0)) and ((d3 > 0 and d4 < 0) or (d3 < 0 and d4 > 0))) return true;
    const eps = 1e-12;
    if (@abs(d1) <= eps and onSeg(b0, b1, a0)) return true;
    if (@abs(d2) <= eps and onSeg(b0, b1, a1)) return true;
    if (@abs(d3) <= eps and onSeg(a0, a1, b0)) return true;
    if (@abs(d4) <= eps and onSeg(a0, a1, b1)) return true;
    return false;
}

fn onSeg(a: V2, b: V2, p: V2) bool {
    return p.x >= @min(a.x, b.x) - 1e-12 and p.x <= @max(a.x, b.x) + 1e-12 and p.y >= @min(a.y, b.y) - 1e-12 and p.y <= @max(a.y, b.y) + 1e-12;
}

fn ptSegDist(p: V2, a: V2, b: V2) f64 {
    const abx = b.x - a.x;
    const aby = b.y - a.y;
    const l2 = abx * abx + aby * aby;
    var t: f64 = 0;
    if (l2 > 0) t = std.math.clamp(((p.x - a.x) * abx + (p.y - a.y) * aby) / l2, 0, 1);
    const dx = p.x - (a.x + t * abx);
    const dy = p.y - (a.y + t * aby);
    return @sqrt(dx * dx + dy * dy);
}

pub fn segSegDist(a0: V2, a1: V2, b0: V2, b1: V2) f64 {
    if (segsCross(a0, a1, b0, b1)) return 0;
    return @min(@min(ptSegDist(a0, b0, b1), ptSegDist(a1, b0, b1)), @min(ptSegDist(b0, a0, a1), ptSegDist(b1, a0, a1)));
}

pub fn polyPolyDist(p: [3]V2, q: [3]V2) f64 {
    var d = std.math.inf(f64);
    for (0..2) |i| for (0..2) |j| {
        d = @min(d, segSegDist(p[i], p[i + 1], q[j], q[j + 1]));
    };
    return d;
}

pub fn polyBoxDist(p: [3]V2, b: *const [4]V2) f64 {
    if (geom.pointInLoopEO(p[0], b) or geom.pointInLoopEO(p[1], b) or geom.pointInLoopEO(p[2], b)) return 0;
    var d = std.math.inf(f64);
    for (0..2) |i| for (0..4) |j| {
        d = @min(d, segSegDist(p[i], p[i + 1], b[j], b[(j + 1) % 4]));
    };
    return d;
}

pub fn leaderOf(left_side: bool, x: f64, top: f64, w: f64, hgt: f64, landing: V2, g: Geo) [3]V2 {
    const ymid = top - hgt * 0.5;
    const edge_x: f64 = if (left_side) x + w + g.pad else x - g.pad;
    const dir: f64 = if (left_side) 1 else -1;
    return .{ V2.init(edge_x, ymid), V2.init(edge_x + dir * g.shoulder, ymid), landing };
}

fn boxPoly(x: f64, top: f64, w: f64, hgt: f64) [4]V2 {
    return .{ V2.init(x, top - hgt), V2.init(x + w, top - hgt), V2.init(x + w, top), V2.init(x, top) };
}

// ---- layout context -------------------------------------------------------------------------------------------

/// `hard` counts the hits between notes (leader/leader, leader/text block); `hits` all of them, dimension text and
/// labels included. Dimension text and labels can be moved by the caller afterwards, so the search ranks `hard` first.
const Cost = struct { total: f64, hits: usize, hard: usize = 0 };

const BB = struct {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,

    fn ofLeader(l: [3]V2) BB {
        return .{
            .x0 = @min(l[0].x, @min(l[1].x, l[2].x)),
            .y0 = @min(l[0].y, @min(l[1].y, l[2].y)),
            .x1 = @max(l[0].x, @max(l[1].x, l[2].x)),
            .y1 = @max(l[0].y, @max(l[1].y, l[2].y)),
        };
    }

    fn ofBox(b: [4]V2) BB {
        return .{
            .x0 = @min(@min(b[0].x, b[1].x), @min(b[2].x, b[3].x)),
            .y0 = @min(@min(b[0].y, b[1].y), @min(b[2].y, b[3].y)),
            .x1 = @max(@max(b[0].x, b[1].x), @max(b[2].x, b[3].x)),
            .y1 = @max(@max(b[0].y, b[1].y), @max(b[2].y, b[3].y)),
        };
    }

    fn apart(a: BB, b: BB, d: f64) bool {
        return a.x0 > b.x1 + d or b.x0 > a.x1 + d or a.y0 > b.y1 + d or b.y0 > a.y1 + d;
    }
};

const Slot = struct { ci: u32, y: f64, lead: [3]V2, bb: BB, u: f64 };

const dp_cands: usize = 6;
const dp_j: i32 = 12;
const dp_slots: usize = dp_cands * (2 * @as(usize, @intCast(dp_j)) + 1);

const Ctx = struct {
    a: Allocator,
    p: Params,
    notes: []const NoteIn,
    obst: []const Obst,
    obst_bb: []const BB,
    soft: []const [2]V2,
    left: []bool,
    ci: []usize,
    key: []f64,
    /// Explicit text-block top chosen by the column solver (NaN = use the stacking rule).
    ov: []f64,
    top: []f64,
    x: []f64,
    leaders: [][3]V2,
    boxes: [][4]V2,
    order: []usize,
    // DP scratch
    slots: []Slot,
    dpc: []f64,
    back: []i32,
    cnt: []usize,
    ord2: []usize,
    adj: []bool,
    /// Remaining column solves (bounds the work on dense views).
    budget: usize = 60,
    /// Shared effort counter (`Work`).
    work: *Work,

    fn landing(self: *const Ctx, i: usize) V2 {
        return self.notes[i].cands[self.ci[i]];
    }

    fn fixed(self: *const Ctx, i: usize) bool {
        return self.notes[i].place != null;
    }

    fn sortOrder(self: *Ctx, col_left: bool) []usize {
        var n: usize = 0;
        for (self.notes, 0..) |_, i| {
            if (self.fixed(i) or self.left[i] != col_left) continue;
            self.order[n] = i;
            n += 1;
        }
        sort.stable(usize, self.order[0..n], self, struct {
            fn lt(c: *Ctx, x: usize, y: usize) bool {
                if (c.key[x] != c.key[y]) return c.key[x] < c.key[y];
                return x < y;
            }
        }.lt);
        return self.order[0..n];
    }

    fn shiftColumn(self: *Ctx, ord: []const usize) void {
        if (ord.len == 0) return;
        const last = ord[ord.len - 1];
        const bottom = self.top[last] - self.notes[last].hgt;
        if (bottom < self.p.crop.y0) {
            const d = self.p.crop.y0 - bottom;
            for (ord) |i| self.top[i] += d;
        }
        const t = self.top[ord[0]];
        if (t > self.p.crop.y1) {
            const d = t - self.p.crop.y1;
            for (ord) |i| self.top[i] -= d;
        }
    }

    /// SPEC 6.3 step 4: initial y = landing y, resolve overlaps top-down, then shift the column into the crop.
    fn stackTopDown(self: *Ctx, ord: []const usize) void {
        var prev_bottom: f64 = std.math.inf(f64);
        for (ord) |i| {
            var t = self.landing(i).y + self.notes[i].hgt * 0.5;
            if (t > prev_bottom - self.p.geo.gap) t = prev_bottom - self.p.geo.gap;
            self.top[i] = t;
            prev_bottom = t - self.notes[i].hgt;
        }
        self.shiftColumn(ord);
    }

    fn layout(self: *Ctx) void {
        for ([2]bool{ false, true }) |col_left| {
            const ord = self.sortOrder(col_left);
            for (ord) |i| {
                self.x[i] = if (col_left) self.p.xl - self.p.gutter - self.notes[i].w else self.p.xr + self.p.gutter;
            }
            self.stackTopDown(ord);
            for (ord) |i| if (!std.math.isNan(self.ov[i])) {
                self.top[i] = self.ov[i];
            };
        }
        for (self.notes, 0..) |nt, i| {
            if (nt.place) |pl| {
                self.x[i] = pl.x;
                self.top[i] = pl.y;
                self.left[i] = pl.x + nt.w * 0.5 < self.landing(i).x;
            }
            self.leaders[i] = leaderOf(self.left[i], self.x[i], self.top[i], nt.w, nt.hgt, self.landing(i), self.p.geo);
            self.boxes[i] = boxPoly(self.x[i], self.top[i], nt.w, nt.hgt);
        }
    }

    /// Penalty for a leader/box or leader/leader distance (null = clear).
    fn near(self: *const Ctx, d: f64) ?f64 {
        if (d <= 1e-9) return W_CROSS;
        if (d < self.p.geo.h) return W_NEAR_BASE + W_NEAR * (1.0 - d / self.p.geo.h);
        return null;
    }

    fn pairPen(self: *const Ctx, a: [3]V2, ab: BB, b: [3]V2, bb: BB) f64 {
        self.work.used += 1;
        if (ab.apart(bb, self.p.geo.h)) return 0;
        return self.near(polyPolyDist(a, b)) orelse 0;
    }

    fn boxPen(self: *const Ctx, a: [3]V2, ab: BB, b: *const [4]V2, bb: BB) f64 {
        self.work.used += 1;
        if (ab.apart(bb, self.p.geo.h)) return 0;
        return self.near(polyBoxDist(a, b)) orelse 0;
    }

    fn softPen(self: *const Ctx, l: [3]V2, lb: BB) f64 {
        const h = self.p.geo.h;
        var c: f64 = 0;
        for (self.soft) |s| {
            const sb = BB{ .x0 = @min(s[0].x, s[1].x), .y0 = @min(s[0].y, s[1].y), .x1 = @max(s[0].x, s[1].x), .y1 = @max(s[0].y, s[1].y) };
            if (lb.apart(sb, h * 0.5)) continue;
            var dmin = segSegDist(l[0], l[1], s[0], s[1]);
            dmin = @min(dmin, segSegDist(l[1], l[2], s[0], s[1]));
            if (dmin <= 1e-9) c += W_SOFT_CROSS else if (dmin < h * 0.5) c += 5;
        }
        return c;
    }

    /// Leader shape: prefer short, shallow leaders and landings near the preferred point.
    fn shapePen(self: *const Ctx, l: [3]V2, land: V2, pref: V2) f64 {
        const h = self.p.geo.h;
        const dy = @abs(l[2].y - l[1].y);
        const dx = @abs(l[2].x - l[1].x);
        var c = 0.4 * dy / h + 0.02 * l[1].dist(l[2]) / h + 0.5 * land.dist(pref) / h;
        if (dy > 1.5 * dx) c += 3;
        return c;
    }

    fn cost(self: *const Ctx) Cost {
        var c: f64 = 0;
        var hits: usize = 0;
        var hard: usize = 0;
        const n = self.notes.len;
        for (0..n) |i| {
            const li = self.leaders[i];
            const lb = BB.ofLeader(li);
            for (i + 1..n) |j| {
                const w = self.pairPen(li, lb, self.leaders[j], BB.ofLeader(self.leaders[j]));
                if (w > 0) {
                    c += w;
                    hits += 1;
                    hard += 1;
                }
            }
            for (0..n) |j| {
                if (j == i) continue;
                const w = self.boxPen(li, lb, &self.boxes[j], BB.ofBox(self.boxes[j]));
                if (w > 0) {
                    c += w;
                    hits += 1;
                    hard += 1;
                }
            }
            for (self.obst, 0..) |o, k| {
                const w = self.boxPen(li, lb, &o.poly, self.obst_bb[k]);
                if (w > 0) {
                    c += w;
                    hits += 1;
                }
            }
            c += self.softPen(li, lb);
            c += self.shapePen(li, self.landing(i), self.notes[i].cands[0]);
        }
        return .{ .total = c, .hits = hits, .hard = hard };
    }

    fn eval(self: *Ctx) Cost {
        self.layout();
        return self.cost();
    }

    /// Hits involving note i's leader only.
    fn hitsOf(self: *const Ctx, i: usize) usize {
        var k: usize = 0;
        const li = self.leaders[i];
        const lb = BB.ofLeader(li);
        for (0..self.notes.len) |j| {
            if (j == i) continue;
            if (self.pairPen(li, lb, self.leaders[j], BB.ofLeader(self.leaders[j])) > 0) k += 1;
            if (self.boxPen(li, lb, &self.boxes[j], BB.ofBox(self.boxes[j])) > 0) k += 1;
        }
        for (self.obst, 0..) |o, m| if (self.boxPen(li, lb, &o.poly, self.obst_bb[m]) > 0) {
            k += 1;
        };
        return k;
    }

    const Snap = struct { left: []bool, ci: []usize, key: []f64, ov: []f64, hits: usize, hard: usize, total: f64 };

    /// A copy of the search state (REVIEW LAY-3: a refused copy is an error; aliasing the live buffers instead would make
    /// `restore` a no-op and the search would silently keep a worse layout).
    fn snapshot(self: *Ctx) Allocator.Error!Snap {
        const cst = self.eval();
        return .{
            .left = try self.a.dupe(bool, self.left),
            .ci = try self.a.dupe(usize, self.ci),
            .key = try self.a.dupe(f64, self.key),
            .ov = try self.a.dupe(f64, self.ov),
            .hits = cst.hits,
            .hard = cst.hard,
            .total = cst.total,
        };
    }

    fn restore(self: *Ctx, s: Snap) void {
        @memcpy(self.left, s.left);
        @memcpy(self.ci, s.ci);
        @memcpy(self.key, s.key);
        @memcpy(self.ov, s.ov);
    }

    // ---- exact column solver --------------------------------------------------------------------------------------

    /// Re-solve one column by dynamic programming (see the module doc). Returns false (state untouched) when
    /// no feasible assignment exists.
    fn solveColumn(self: *Ctx, col_left: bool) bool {
        const g = self.p.geo;
        const ord_in = self.sortOrder(col_left);
        const n = ord_in.len;
        if (n == 0) return true;
        if (self.budget == 0 or self.work.spent()) return false;
        self.budget -= 1;
        const ord = self.ord2[0..n];
        @memcpy(ord, ord_in);
        var total_h: f64 = 0;
        for (ord) |i| total_h += self.notes[i].hgt;
        total_h += g.gap * @as(f64, @floatFromInt(n - 1));
        const span = @max(total_h, self.p.crop.y1 - self.p.crop.y0);
        const mid = (self.p.crop.y0 + self.p.crop.y1) * 0.5;
        const lo = mid - span * 0.5 - 2 * g.pitch;
        const hi = mid + span * 0.5 + 2 * g.pitch;
        const step = g.pitch * 0.5;
        const nn = self.notes.len;
        const adjacent = self.adj;
        // states
        for (ord, 0..) |i, k| {
            const nt = self.notes[i];
            const base = k * dp_slots;
            var cnt: usize = 0;
            const ncand: usize = if (nt.movable) @min(dp_cands, nt.cands.len) else 1;
            const xi = if (col_left) self.p.xl - self.p.gutter - nt.w else self.p.xr + self.p.gutter;
            @memset(adjacent, false);
            if (k > 0) adjacent[ord[k - 1]] = true;
            if (k + 1 < n) adjacent[ord[k + 1]] = true;
            for (0..ncand) |ci| {
                const land = nt.cands[ci];
                var jj: i32 = 0;
                while (jj <= dp_j) : (jj += 1) {
                    var sgn: i32 = 1;
                    while (sgn >= -1) : (sgn -= 2) {
                        if (jj == 0 and sgn == -1) continue;
                        const y = land.y + @as(f64, @floatFromInt(jj * sgn)) * step;
                        const tp = y + nt.hgt * 0.5;
                        if (tp > hi or y - nt.hgt * 0.5 < lo) continue;
                        const lead = leaderOf(col_left, xi, tp, nt.w, nt.hgt, land, g);
                        const lb = BB.ofLeader(lead);
                        var u = self.shapePen(lead, land, nt.cands[0]) + self.softPen(lead, lb);
                        for (self.obst, 0..) |o, m| u += self.boxPen(lead, lb, &o.poly, self.obst_bb[m]);
                        for (0..nn) |j| {
                            if (j == i) continue;
                            const in_col = !self.fixed(j) and self.left[j] == col_left;
                            if (in_col) {
                                // same column: neighbours are handled by the pair term; the others count
                                // as fixed at their current place (coordinate descent over passes)
                                if (adjacent[j] or j == i) continue;
                                u += self.pairPen(lead, lb, self.leaders[j], BB.ofLeader(self.leaders[j]));
                                continue;
                            }
                            u += self.pairPen(lead, lb, self.leaders[j], BB.ofLeader(self.leaders[j]));
                            u += self.boxPen(lead, lb, &self.boxes[j], BB.ofBox(self.boxes[j]));
                        }
                        self.slots[base + cnt] = .{ .ci = @intCast(ci), .y = y, .lead = lead, .bb = lb, .u = u };
                        cnt += 1;
                    }
                }
            }
            self.cnt[k] = cnt;
            if (cnt == 0) return false;
        }
        // forward pass
        const inf = std.math.inf(f64);
        for (0..self.cnt[0]) |s| {
            self.dpc[s] = self.slots[s].u;
            self.back[s] = -1;
        }
        for (1..n) |k| {
            const hk = self.notes[ord[k]].hgt;
            const hp = self.notes[ord[k - 1]].hgt;
            const base = k * dp_slots;
            const pbase = (k - 1) * dp_slots;
            for (0..self.cnt[k]) |s| {
                const sl = self.slots[base + s];
                var best: f64 = inf;
                var bi: i32 = -1;
                for (0..self.cnt[k - 1]) |q| {
                    const pc = self.dpc[pbase + q];
                    if (pc >= best) continue;
                    const ps = self.slots[pbase + q];
                    // text blocks must not overlap: previous bottom - gap >= this top
                    if (ps.y - hp * 0.5 - g.gap < sl.y + hk * 0.5 - 1e-9) continue;
                    const v = pc + self.pairPen(ps.lead, ps.bb, sl.lead, sl.bb);
                    if (v < best) {
                        best = v;
                        bi = @intCast(q);
                    }
                }
                self.dpc[base + s] = if (bi < 0) inf else best + sl.u;
                self.back[base + s] = bi;
            }
        }
        // best end state
        var best: f64 = inf;
        var bs: usize = 0;
        const lbase = (n - 1) * dp_slots;
        for (0..self.cnt[n - 1]) |s| {
            if (self.dpc[lbase + s] < best) {
                best = self.dpc[lbase + s];
                bs = s;
            }
        }
        if (best == inf) return false;
        var k: usize = n;
        var s: i32 = @intCast(bs);
        while (k > 0) {
            k -= 1;
            const sl = self.slots[k * dp_slots + @as(usize, @intCast(s))];
            const i = ord[k];
            self.ci[i] = sl.ci;
            self.ov[i] = sl.y + self.notes[i].hgt * 0.5;
            s = self.back[k * dp_slots + @as(usize, @intCast(s))];
        }
        return true;
    }

    fn solveAll(self: *Ctx) void {
        for (0..2) |_| {
            _ = self.solveColumn(false);
            self.layout();
            _ = self.solveColumn(true);
            self.layout();
        }
    }
};

fn better(a: Cost, b: Cost) bool {
    return betterMode(a, b, true);
}

/// Lexicographic: hits between notes (if `hard_first`), all hits, cost.
fn betterMode(a: Cost, b: Cost, hard_first: bool) bool {
    if (hard_first and a.hard != b.hard) return a.hard < b.hard;
    return a.hits < b.hits or (a.hits == b.hits and a.total < b.total - 1e-9);
}

/// Hit-driven improvement: DP re-solve, then single discrete changes (column flip, neighbour swap,
/// re-insertion) re-solved one at a time; the first change that lowers (hits, cost) is kept.
fn improve(c: *Ctx) Allocator.Error!void {
    const n = c.notes.len;
    c.solveAll();
    var cur = c.eval();
    var rounds: usize = 0;
    while (cur.hits > 0 and rounds < 6 and c.budget > 0 and !c.work.spent()) : (rounds += 1) {
        var changed = false;
        var i: usize = 0;
        while (i < n and !changed and c.budget > 0 and !c.work.spent()) : (i += 1) {
            if (c.fixed(i) or c.hitsOf(i) == 0) continue;
            const start = try c.snapshot();
            // (a) other column
            if (c.p.side == .both and c.notes[i].column == null) {
                c.left[i] = !c.left[i];
                c.key[i] = -c.landing(i).y;
                c.solveAll();
                const r = c.eval();
                if (better(r, cur)) {
                    cur = r;
                    changed = true;
                    continue;
                }
                c.restore(start);
                _ = c.eval();
            }
            // (b) re-insert at every other position of its column
            var cnt: usize = 0;
            var pos: usize = 0;
            for (0..n) |j| {
                if (c.fixed(j) or c.left[j] != c.left[i]) continue;
                if (j != i and (c.key[j] < c.key[i] or (c.key[j] == c.key[i] and j < i))) pos += 1;
                cnt += 1;
            }
            var v: usize = 0;
            while (v < cnt and !changed and c.budget > 0 and !c.work.spent()) : (v += 1) {
                if (v == pos) continue;
                rekeyAt(c.key, c.sortOrder(c.left[i]), i, v);
                c.solveAll();
                const r = c.eval();
                if (better(r, cur)) {
                    cur = r;
                    changed = true;
                    break;
                }
                c.restore(start);
                _ = c.eval();
            }
        }
        if (!changed) break;
    }
}

/// Renumber the keys of a column (`ord`: its notes in key order, `i` among them) to 0, 1, ... with note `i` moved to position
/// `v` and the others keeping their order. Every note of the column gets a fresh key, however many there are (REVIEW LAY-3:
/// a 64-entry scratch array used to leave the notes past the 64th with stale keys that collided with the renumbered ones).
fn rekeyAt(key: []f64, ord: []const usize, i: usize, v: usize) void {
    var k: usize = 0;
    for (ord) |o| {
        if (o == i) continue;
        if (k == v) {
            key[i] = @floatFromInt(k);
            k += 1;
        }
        key[o] = @floatFromInt(k);
        k += 1;
    }
    if (k == v) key[i] = @floatFromInt(k);
}

/// SPEC 16: swap adjacent column notes whose leaders cross (bounded), then renumber the keys by rank so later
/// changes are position changes.
fn decross(c: *Ctx) void {
    const n = c.notes.len;
    var iter: usize = 0;
    while (iter < n * 4 + 8) : (iter += 1) {
        var swapped = false;
        for ([2]bool{ false, true }) |col_left| {
            const ord = c.sortOrder(col_left);
            var k: usize = 0;
            while (k + 1 < ord.len) : (k += 1) {
                if (polyPolyDist(c.leaders[ord[k]], c.leaders[ord[k + 1]]) <= 1e-9) {
                    std.mem.swap(f64, &c.key[ord[k]], &c.key[ord[k + 1]]);
                    swapped = true;
                    break;
                }
            }
            if (swapped) break;
        }
        if (!swapped) break;
        c.layout();
    }
    for ([2]bool{ false, true }) |col_left| {
        const ord = c.sortOrder(col_left);
        for (ord, 0..) |i, k| c.key[i] = @floatFromInt(k);
    }
}

// ---- entry point ---------------------------------------------------------------------------------------------------

pub fn route(a: Allocator, p: Params, notes: []const NoteIn, obst: []const Obst, soft: []const [2]V2) Allocator.Error!Layout {
    const n = notes.len;
    const obb = try a.alloc(BB, obst.len);
    for (obst, 0..) |o, i| obb[i] = BB.ofBox(o.poly);
    var local_work = Work{};
    var c = Ctx{
        .work = p.work orelse &local_work,
        .a = a,
        .p = p,
        .notes = notes,
        .obst = obst,
        .obst_bb = obb,
        .soft = soft,
        .left = try a.alloc(bool, n),
        .ci = try a.alloc(usize, n),
        .key = try a.alloc(f64, n),
        .ov = try a.alloc(f64, n),
        .top = try a.alloc(f64, n),
        .x = try a.alloc(f64, n),
        .leaders = try a.alloc([3]V2, n),
        .boxes = try a.alloc([4]V2, n),
        .order = try a.alloc(usize, n),
        .slots = try a.alloc(Slot, n * dp_slots),
        .dpc = try a.alloc(f64, n * dp_slots),
        .back = try a.alloc(i32, n * dp_slots),
        .cnt = try a.alloc(usize, n),
        .ord2 = try a.alloc(usize, n),
        .adj = try a.alloc(bool, n),
    };
    for (notes, 0..) |nt, i| {
        c.ci[i] = 0;
        c.ov[i] = std.math.nan(f64);
        const l = nt.cands[0];
        c.left[i] = if (nt.column) |cs| cs == .left else switch (p.side) {
            .left => true,
            .right => false,
            .both => @abs(l.x - p.crop.x0) < @abs(p.crop.x1 - l.x),
        };
        c.key[i] = -l.y;
    }
    c.layout();
    decross(&c);
    var searched = false;
    if (c.eval().hits > 0) {
        searched = true;
        const start = try c.snapshot();
        var best = start;
        // one bounded search (a few column solves per hit note); hits on dimension text and labels are the
        // caller's to repair (annot.zig), so the search ranks hits between notes first
        c.budget = if (p.light) 20 else 60;
        _ = c.eval();
        try improve(&c);
        const r = try c.snapshot();
        if (betterMode(.{ .total = r.total, .hits = r.hits, .hard = r.hard }, .{ .total = best.total, .hits = best.hits, .hard = best.hard }, true)) best = r;
        c.restore(best);
        _ = c.eval();
    }

    // remaining hits with fix proposals
    var hits: std.ArrayList(Hit) = .empty;
    for (0..n) |i| {
        for (i + 1..n) |j| {
            const d = polyPolyDist(c.leaders[i], c.leaders[j]);
            if (c.near(d) != null) try hits.append(a, .{ .note = i, .kind = .leader, .other = j, .dist = d });
        }
        for (0..n) |j| {
            if (j == i) continue;
            const d = polyBoxDist(c.leaders[i], &c.boxes[j]);
            if (c.near(d) != null) try hits.append(a, .{ .note = i, .kind = .note, .other = j, .dist = d });
        }
        for (obst, 0..) |o, j| {
            const d = polyBoxDist(c.leaders[i], &o.poly);
            if (c.near(d) != null) try hits.append(a, .{ .note = i, .kind = if (o.kind == .dim) .dim else .label, .other = j, .dist = d });
        }
    }
    for (hits.items, 0..) |*ht, hi| {
        if (hi >= max_fix_hits) break;
        var fa: ?V2 = null;
        var fp: ?V2 = null;
        try proposeFix(&c, ht.note, &fa, &fp);
        ht.fix_at = fa;
        ht.fix_place = fp;
        if (ht.kind == .leader) {
            var oa: ?V2 = null;
            var op: ?V2 = null;
            try proposeFix(&c, ht.other, &oa, &op);
            ht.other_fix_at = oa;
            ht.other_fix_place = op;
        }
    }
    c.layout();
    const landing = try a.alloc(V2, n);
    for (0..n) |i| landing[i] = c.landing(i);
    return .{ .top = c.top, .x = c.x, .left = c.left, .landing = landing, .leaders = c.leaders, .hits = hits.items, .searched = searched, .budget_exhausted = c.work.spent() };
}

/// A concrete way to clear note i's hits: an alternative landing point (`at`) or a new text position
/// (`place`), found by trying the alternatives with every other note where it is now.
fn proposeFix(c: *Ctx, i: usize, fix_at: *?V2, fix_place: *?V2) Allocator.Error!void {
    c.layout();
    const base = c.hitsOf(i);
    if (base == 0) return;
    const nt = c.notes[i];
    if (nt.movable and !c.fixed(i)) {
        const save = c.ci[i];
        for (0..nt.cands.len) |v| {
            if (v == save) continue;
            c.ci[i] = v;
            const old = c.leaders[i];
            c.leaders[i] = leaderOf(c.left[i], c.x[i], c.top[i], nt.w, nt.hgt, c.landing(i), c.p.geo);
            const k = c.hitsOf(i);
            c.leaders[i] = old;
            if (k == 0) {
                fix_at.* = nt.cands[v];
                break;
            }
        }
        c.ci[i] = save;
    }
    // place: slide the text block up/down in half-pitch steps until the leader clears everything
    const step = c.p.geo.pitch * 0.5;
    const top0 = c.top[i];
    const old_l = c.leaders[i];
    const old_b = c.boxes[i];
    var k: usize = 1;
    while (k <= 24 and fix_place.* == null) : (k += 1) {
        for ([2]f64{ 1, -1 }) |sg| {
            const t = top0 + sg * @as(f64, @floatFromInt(k)) * step;
            c.leaders[i] = leaderOf(c.left[i], c.x[i], t, nt.w, nt.hgt, c.landing(i), c.p.geo);
            c.boxes[i] = boxPoly(c.x[i], t, nt.w, nt.hgt);
            var ok = c.hitsOf(i) == 0;
            if (ok) for (0..c.notes.len) |j| {
                if (j == i) continue;
                // keep text blocks apart
                const bj = c.boxes[j];
                if (c.x[j] < c.x[i] + nt.w and c.x[i] < c.x[j] + c.notes[j].w and bj[0].y < t + c.p.geo.gap and t - nt.hgt - c.p.geo.gap < bj[2].y) ok = false;
            };
            if (ok) {
                fix_place.* = V2.init(c.x[i], t);
                break;
            }
        }
    }
    c.leaders[i] = old_l;
    c.boxes[i] = old_b;
}

// ---- tests -----------------------------------------------------------------------------------------------------------

fn testNotes(a: Allocator, landings: []const V2) ![]NoteIn {
    const out = try a.alloc(NoteIn, landings.len);
    for (landings, 0..) |l, i| {
        const cs = try a.alloc(V2, 1);
        cs[0] = l;
        out[i] = .{ .w = 20, .hgt = 1.0, .cands = cs, .movable = true, .place = null };
    }
    return out;
}

test "route: parallel leaders in a column do not cross and stay apart" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const g = Geo{ .h = 1.0, .pitch = 1.6, .gap = 0.6, .shoulder = 1.3, .pad = 0.4 };
    const p = Params{ .geo = g, .crop = .{ .x0 = 0, .y0 = 0, .x1 = 40, .y1 = 30 }, .xl = 0, .xr = 40, .gutter = 4, .side = .left };
    const notes = try testNotes(a, &.{ V2.init(10, 20), V2.init(10, 19), V2.init(10, 18.5), V2.init(30, 5), V2.init(12, 4) });
    const r = try route(a, p, notes, &.{}, &.{});
    for (0..notes.len) |i| for (i + 1..notes.len) |j| {
        try std.testing.expect(polyPolyDist(r.leaders[i], r.leaders[j]) > 0);
    };
}

test "route: deterministic" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const g = Geo{ .h = 1.0, .pitch = 1.6, .gap = 0.6, .shoulder = 1.3, .pad = 0.4 };
    const p = Params{ .geo = g, .crop = .{ .x0 = 0, .y0 = 0, .x1 = 40, .y1 = 30 }, .xl = 0, .xr = 40, .gutter = 4, .side = .both };
    const notes = try testNotes(a, &.{ V2.init(10, 20), V2.init(30, 19), V2.init(12, 18.5), V2.init(30, 5), V2.init(12, 4) });
    const r1 = try route(a, p, notes, &.{}, &.{});
    const r2 = try route(a, p, notes, &.{}, &.{});
    for (0..notes.len) |i| {
        try std.testing.expectEqual(r1.top[i], r2.top[i]);
        try std.testing.expectEqual(r1.x[i], r2.x[i]);
    }
}

test "route: 150 crowded notes stay inside the work budget (REVIEW LAY-1)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const g = Geo{ .h = 1.0, .pitch = 1.6, .gap = 0.6, .shoulder = 1.3, .pad = 0.4 };
    const p0 = Params{ .geo = g, .crop = .{ .x0 = 0, .y0 = 0, .x1 = 40, .y1 = 30 }, .xl = 0, .xr = 40, .gutter = 4, .side = .both };
    // 150 landings packed into a 20 x 20 patch so almost every leader meets another
    var lands: std.ArrayList(V2) = .empty;
    for (0..150) |i| try lands.append(a, V2.init(10 + @as(f64, @floatFromInt(i % 15)) * 1.3, 5 + @as(f64, @floatFromInt(i / 15)) * 2.1));
    const notes = try testNotes(a, lands.items);
    var work = Work{ .limit = 1_000_000 };
    var p = p0;
    p.work = &work;
    const r = try route(a, p, notes, &.{}, &.{});
    // the search stops at the limit; the rest is the O(n^2) hit scan and the baseline layout (a few hundred thousand units)
    try std.testing.expect(work.used < 3 * work.limit);
    try std.testing.expect(r.budget_exhausted);
    try std.testing.expectEqual(@as(usize, 150), r.top.len);
    // only the first max_fix_hits hits carry proposals
    var proposals: usize = 0;
    for (r.hits) |h| if (h.fix_at != null or h.fix_place != null) {
        proposals += 1;
    };
    try std.testing.expect(proposals <= max_fix_hits);
}

test "route: re-inserting a note renumbers every note of a column with more than 64 notes (REVIEW LAY-3)" {
    // 100 notes in one column (limits.zig allows 150 annotations per view); the old 64-entry scratch array left the notes past
    // the 64th with their old keys and never placed `i` at a position past 64
    const n = 100;
    var ord: [n]usize = undefined;
    for (&ord, 0..) |*o, k| o.* = (k * 37 + 11) % n; // a permutation: the column order is not the index order
    for ([_]usize{ 0, 1, 50, 64, 65, 90, n - 1 }) |v| {
        for ([_]usize{ 0, 20, 70, n - 1 }) |pos| {
            const i = ord[pos];
            var key: [n]f64 = undefined;
            @memset(&key, -1);
            rekeyAt(&key, &ord, i, v);
            try std.testing.expectEqual(@as(f64, @floatFromInt(v)), key[i]);
            // keys are exactly 0..n-1 (no stale, no duplicate) and the other notes keep their order
            var seen: [n]bool = @splat(false);
            for (key) |kv| {
                const k: usize = @intFromFloat(kv);
                try std.testing.expect(kv >= 0 and !seen[k]);
                seen[k] = true;
            }
            var prev: f64 = -1;
            for (ord) |o| if (o != i) {
                try std.testing.expect(key[o] > prev);
                prev = key[o];
            };
        }
    }
}

test "route: a refused allocation is an error, never a silently different layout (REVIEW LAY-3)" {
    const oom = @import("oom.zig");
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const g = Geo{ .h = 1.0, .pitch = 1.6, .gap = 0.6, .shoulder = 1.3, .pad = 0.4 };
    const p = Params{ .geo = g, .crop = .{ .x0 = 0, .y0 = 0, .x1 = 40, .y1 = 30 }, .xl = 0, .xr = 40, .gutter = 4, .side = .both };
    // a crowded patch: the baseline has hits, so the search (and its state snapshots) runs
    var lands: std.ArrayList(V2) = .empty;
    for (0..12) |i| try lands.append(a, V2.init(10 + @as(f64, @floatFromInt(i % 4)) * 1.3, 10 + @as(f64, @floatFromInt(i / 4)) * 1.1));
    const notes = try testNotes(a, lands.items);
    const base = try route(a, p, notes, &.{}, &.{});
    try std.testing.expect(base.searched);
    var idx: usize = 0;
    while (true) : (idx += 1) {
        var one = oom.OneShotFail.init(a, idx);
        const r = route(one.allocator(), p, notes, &.{}, &.{}) catch |e| {
            try std.testing.expectEqual(error.OutOfMemory, e);
            try std.testing.expect(one.failed);
            continue;
        };
        // before: a refused snapshot copy aliased the live state, `restore` did nothing and route returned normally
        try std.testing.expect(!one.failed);
        for (0..notes.len) |i| {
            try std.testing.expectEqual(base.top[i], r.top[i]);
            try std.testing.expectEqual(base.x[i], r.x[i]);
            try std.testing.expectEqual(base.left[i], r.left[i]);
        }
        break; // idx is past the last allocation of the call
    }
    try std.testing.expect(idx > 20);
}
