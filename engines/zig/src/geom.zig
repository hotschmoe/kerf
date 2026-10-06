//! 2D geometry primitives: vectors, bulge arcs, loops, transforms, robust orientation, and
//! segment intersection. Loops are closed polylines of `Pt` (x, y, bulge); the bulge on vertex i
//! describes the segment i -> i+1 with DXF LWPOLYLINE semantics (bulge = tan(theta/4), CCW positive).

const std = @import("std");
const cast = @import("num.zig");
const Allocator = std.mem.Allocator;

pub const V2 = struct {
    x: f64,
    y: f64,

    pub fn init(x: f64, y: f64) V2 {
        return .{ .x = x, .y = y };
    }
    pub fn add(a: V2, b: V2) V2 {
        return .{ .x = a.x + b.x, .y = a.y + b.y };
    }
    pub fn sub(a: V2, b: V2) V2 {
        return .{ .x = a.x - b.x, .y = a.y - b.y };
    }
    pub fn scale(a: V2, s: f64) V2 {
        return .{ .x = a.x * s, .y = a.y * s };
    }
    pub fn dot(a: V2, b: V2) f64 {
        return a.x * b.x + a.y * b.y;
    }
    pub fn cross(a: V2, b: V2) f64 {
        return a.x * b.y - a.y * b.x;
    }
    pub fn len(a: V2) f64 {
        return @sqrt(a.x * a.x + a.y * a.y);
    }
    pub fn dist(a: V2, b: V2) f64 {
        return a.sub(b).len();
    }
    pub fn norm(a: V2) V2 {
        const l = a.len();
        if (l == 0) return a;
        return .{ .x = a.x / l, .y = a.y / l };
    }
    /// Left normal (-y, x).
    pub fn perp(a: V2) V2 {
        return .{ .x = -a.y, .y = a.x };
    }
    pub fn lerp(a: V2, b: V2, t: f64) V2 {
        return .{ .x = a.x + (b.x - a.x) * t, .y = a.y + (b.y - a.y) * t };
    }
    pub fn mid(a: V2, b: V2) V2 {
        return lerp(a, b, 0.5);
    }
    pub fn eql(a: V2, b: V2, tol: f64) bool {
        return @abs(a.x - b.x) <= tol and @abs(a.y - b.y) <= tol;
    }
};

pub const Pt = struct {
    x: f64,
    y: f64,
    b: f64 = 0,

    pub fn v(p: Pt) V2 {
        return .{ .x = p.x, .y = p.y };
    }
    pub fn at(p: V2, b: f64) Pt {
        return .{ .x = p.x, .y = p.y, .b = b };
    }
};

pub const Loop = []const Pt;

pub const Box = struct {
    x0: f64 = std.math.inf(f64),
    y0: f64 = std.math.inf(f64),
    x1: f64 = -std.math.inf(f64),
    y1: f64 = -std.math.inf(f64),

    pub fn isEmpty(b: Box) bool {
        return b.x0 > b.x1 or b.y0 > b.y1;
    }
    pub fn addPoint(b: *Box, x: f64, y: f64) void {
        b.x0 = @min(b.x0, x);
        b.y0 = @min(b.y0, y);
        b.x1 = @max(b.x1, x);
        b.y1 = @max(b.y1, y);
    }
    pub fn addBox(b: *Box, o: Box) void {
        if (o.isEmpty()) return;
        b.addPoint(o.x0, o.y0);
        b.addPoint(o.x1, o.y1);
    }
    pub fn width(b: Box) f64 {
        return b.x1 - b.x0;
    }
    pub fn height(b: Box) f64 {
        return b.y1 - b.y0;
    }
    pub fn overlaps(a: Box, o: Box, tol: f64) bool {
        return a.x0 <= o.x1 + tol and o.x0 <= a.x1 + tol and a.y0 <= o.y1 + tol and o.y0 <= a.y1 + tol;
    }
    pub fn contains(b: Box, p: V2) bool {
        return p.x >= b.x0 and p.x <= b.x1 and p.y >= b.y0 and p.y <= b.y1;
    }
    pub fn center(b: Box) V2 {
        return .{ .x = (b.x0 + b.x1) / 2, .y = (b.y0 + b.y1) / 2 };
    }
    pub fn expand(b: Box, d: f64) Box {
        return .{ .x0 = b.x0 - d, .y0 = b.y0 - d, .x1 = b.x1 + d, .y1 = b.y1 + d };
    }
};

// ---- affine transforms -----------------------------------------------------------------------------

pub const Xf = struct {
    a: f64 = 1,
    b: f64 = 0,
    c: f64 = 0,
    d: f64 = 1,
    tx: f64 = 0,
    ty: f64 = 0,

    pub const identity = Xf{};

    pub fn apply(m: Xf, p: V2) V2 {
        return .{ .x = m.a * p.x + m.c * p.y + m.tx, .y = m.b * p.x + m.d * p.y + m.ty };
    }
    pub fn applyDir(m: Xf, p: V2) V2 {
        return .{ .x = m.a * p.x + m.c * p.y, .y = m.b * p.x + m.d * p.y };
    }
    pub fn det(m: Xf) f64 {
        return m.a * m.d - m.b * m.c;
    }
    /// `m` applied after `n`: (m * n)(p) = m(n(p)).
    pub fn mul(m: Xf, n: Xf) Xf {
        return .{
            .a = m.a * n.a + m.c * n.b,
            .b = m.b * n.a + m.d * n.b,
            .c = m.a * n.c + m.c * n.d,
            .d = m.b * n.c + m.d * n.d,
            .tx = m.a * n.tx + m.c * n.ty + m.tx,
            .ty = m.b * n.tx + m.d * n.ty + m.ty,
        };
    }
    pub fn translate(x: f64, y: f64) Xf {
        return .{ .tx = x, .ty = y };
    }
    pub fn rotate(rad: f64) Xf {
        const c = @cos(rad);
        const s = @sin(rad);
        return .{ .a = c, .b = s, .c = -s, .d = c };
    }
    /// Rotate by `rad` about point `p`.
    pub fn rotateAbout(rad: f64, p: V2) Xf {
        return translate(p.x, p.y).mul(rotate(rad)).mul(translate(-p.x, -p.y));
    }
    pub fn scaling(sx: f64, sy: f64) Xf {
        return .{ .a = sx, .d = sy };
    }
    pub fn isIdentity(m: Xf) bool {
        return m.a == 1 and m.b == 0 and m.c == 0 and m.d == 1 and m.tx == 0 and m.ty == 0;
    }

    pub fn applyPt(m: Xf, p: Pt) Pt {
        const q = m.apply(p.v());
        return .{ .x = q.x, .y = q.y, .b = if (m.det() < 0) -p.b else p.b };
    }
    pub fn applyLoop(m: Xf, a: Allocator, loop: Loop) Allocator.Error![]Pt {
        const out = try a.alloc(Pt, loop.len);
        for (loop, 0..) |p, i| out[i] = m.applyPt(p);
        return out;
    }
};

// ---- arcs ------------------------------------------------------------------------------------------

pub const Arc = struct {
    c: V2,
    r: f64,
    a0: f64,
    /// Signed sweep in radians (CCW positive).
    sweep: f64,

    pub fn at(self: Arc, t: f64) V2 {
        const a = self.a0 + self.sweep * t;
        return .{ .x = self.c.x + self.r * @cos(a), .y = self.c.y + self.r * @sin(a) };
    }
};

/// Arc through p0 -> p1 with the given non-zero bulge.
pub fn arcOf(p0: V2, p1: V2, bulge: f64) Arc {
    const chord = p1.sub(p0);
    const cl = chord.len();
    const sweep = 4.0 * std.math.atan(bulge);
    const r = cl * (1.0 + bulge * bulge) / (4.0 * @abs(bulge));
    const d = (cl / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    const m = V2.mid(p0, p1);
    const n = if (cl > 0) chord.perp().scale(1.0 / cl) else V2.init(0, 0);
    const c = m.add(n.scale(d));
    return .{ .c = c, .r = r, .a0 = std.math.atan2(p0.y - c.y, p0.x - c.x), .sweep = sweep };
}

pub fn bulgeFromSweep(sweep: f64) f64 {
    return @tan(sweep / 4.0);
}

pub const Seg = struct { a: V2, b: V2, bulge: f64 = 0 };

/// Sub-segment of (p0, p1, bulge) between parameters t0 < t1 (parameter = fraction of the sweep or chord).
pub fn subSeg(p0: V2, p1: V2, bulge: f64, t0: f64, t1: f64) Seg {
    if (bulge == 0) return .{ .a = V2.lerp(p0, p1, t0), .b = V2.lerp(p0, p1, t1) };
    const arc = arcOf(p0, p1, bulge);
    const a = if (t0 <= 0) p0 else arc.at(t0);
    const b = if (t1 >= 1) p1 else arc.at(t1);
    return .{ .a = a, .b = b, .bulge = bulgeFromSweep(arc.sweep * (t1 - t0)) };
}

/// Point on the segment at parameter t.
pub fn segPoint(p0: V2, p1: V2, bulge: f64, t: f64) V2 {
    if (bulge == 0) return V2.lerp(p0, p1, t);
    if (t <= 0) return p0;
    if (t >= 1) return p1;
    return arcOf(p0, p1, bulge).at(t);
}

/// Append the flattened points of segment p0 -> p1 (excluding p0, including p1). Chord sagitta <= tol.
pub fn flattenSegInto(list: *std.ArrayList(V2), a: Allocator, p0: V2, p1: V2, bulge: f64, tol: f64) Allocator.Error!void {
    if (bulge == 0) {
        try list.append(a, p1);
        return;
    }
    const arc = arcOf(p0, p1, bulge);
    const n = arcSteps(arc.r, arc.sweep, tol);
    var i: usize = 1;
    while (i < n) : (i += 1) {
        try list.append(a, arc.at(@as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(n))));
    }
    try list.append(a, p1);
}

/// Upper bound on the segments of one flattened arc (a full circle at 0.004" tolerance needs about 200 up to a 10 ft radius).
pub const max_arc_steps: usize = 4096;

/// Segments for an arc of `sweep` radians when one segment may span at most `step` radians; clamped to [2, max_arc_steps]
/// (NaN and a zero `step` can not reach the integer conversion).
pub fn stepsForSweep(sweep: f64, step: f64) usize {
    return cast.toIntClamped(usize, @ceil(@abs(sweep) / step), 2, max_arc_steps);
}

pub fn arcSteps(r: f64, sweep: f64, tol: f64) usize {
    if (!(r > tol)) return 2;
    const step = 2.0 * std.math.acos(1.0 - tol / r);
    // a radius so large that tol/r underflows 1 - x: the arc is straight to within `tol`
    if (!(step > 0)) return 2;
    return stepsForSweep(sweep, step);
}

pub fn flattenLoop(a: Allocator, loop: Loop, tol: f64) Allocator.Error![]V2 {
    var out: std.ArrayList(V2) = .empty;
    if (loop.len == 0) return out.items;
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % loop.len];
        try out.append(a, p.v());
        if (p.b != 0) {
            // intermediate points only; the endpoint is appended by the next iteration
            var tmp: std.ArrayList(V2) = .empty;
            try flattenSegInto(&tmp, a, p.v(), q.v(), p.b, tol);
            if (tmp.items.len > 0) {
                for (tmp.items[0 .. tmp.items.len - 1]) |m| try out.append(a, m);
            }
        }
    }
    return out.items;
}

/// Polyline flatten for open or closed pts (closed adds the wrap segment). Returns V2 list.
pub fn flattenPolyline(a: Allocator, pts: []const Pt, closed: bool, tol: f64) Allocator.Error![]V2 {
    var out: std.ArrayList(V2) = .empty;
    if (pts.len == 0) return out.items;
    try out.append(a, pts[0].v());
    const n = if (closed) pts.len else pts.len - 1;
    for (0..n) |i| {
        const q = pts[(i + 1) % pts.len];
        try flattenSegInto(&out, a, pts[i].v(), q.v(), pts[i].b, tol);
    }
    return out.items;
}

pub fn segLength(p0: V2, p1: V2, bulge: f64) f64 {
    if (bulge == 0) return p0.dist(p1);
    const arc = arcOf(p0, p1, bulge);
    return arc.r * @abs(arc.sweep);
}

/// Signed area of a closed bulge loop (CCW positive), exact for arcs.
pub fn signedArea(loop: Loop) f64 {
    var s: f64 = 0;
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % loop.len];
        s += p.x * q.y - q.x * p.y;
        if (p.b != 0) {
            // circular segment area between the chord and the arc: r^2/2 (theta - sin theta), signed by bulge
            const arc = arcOf(p.v(), q.v(), p.b);
            const th = arc.sweep;
            s += arc.r * arc.r * (th - @sin(th));
        }
    }
    return s / 2.0;
}

pub fn signedAreaV(pts: []const V2) f64 {
    var s: f64 = 0;
    for (pts, 0..) |p, i| {
        const q = pts[(i + 1) % pts.len];
        s += p.x * q.y - q.x * p.y;
    }
    return s / 2.0;
}

pub fn loopBox(loop: Loop) Box {
    var b = Box{};
    for (loop, 0..) |p, i| {
        b.addPoint(p.x, p.y);
        if (p.b != 0) {
            const q = loop[(i + 1) % loop.len];
            segBoxInto(&b, p.v(), q.v(), p.b);
        }
    }
    return b;
}

/// Extend `b` by the extremes of an arc segment (endpoints are added by the caller).
pub fn segBoxInto(b: *Box, p0: V2, p1: V2, bulge: f64) void {
    if (bulge == 0) {
        b.addPoint(p0.x, p0.y);
        b.addPoint(p1.x, p1.y);
        return;
    }
    const arc = arcOf(p0, p1, bulge);
    b.addPoint(p0.x, p0.y);
    b.addPoint(p1.x, p1.y);
    // cardinal extremes inside the sweep
    var k: i32 = 0;
    while (k < 4) : (k += 1) {
        const ang = @as(f64, @floatFromInt(k)) * std.math.pi / 2.0;
        if (angleInSweep(ang, arc.a0, arc.sweep)) {
            b.addPoint(arc.c.x + arc.r * @cos(ang), arc.c.y + arc.r * @sin(ang));
        }
    }
}

pub fn angleInSweep(ang: f64, a0: f64, sweep: f64) bool {
    const two_pi = 2.0 * std.math.pi;
    var d = ang - a0;
    if (sweep >= 0) {
        d = @mod(d, two_pi);
        return d <= sweep + 1e-12;
    } else {
        d = @mod(-d, two_pi);
        return d <= -sweep + 1e-12;
    }
}

pub fn pointsBox(pts: []const Pt) Box {
    var b = Box{};
    for (pts) |p| b.addPoint(p.x, p.y);
    return b;
}

pub fn reverseLoop(a: Allocator, loop: Loop) Allocator.Error![]Pt {
    // Reversing a bulge loop: vertex order reverses and segment i (v_i -> v_{i+1}, bulge b_i)
    // becomes segment v_{i+1} -> v_i with bulge -b_i.
    const n = loop.len;
    const out = try a.alloc(Pt, n);
    for (0..n) |i| {
        const src = n - 1 - i; // new vertex i is old vertex src
        const prev = (src + n - 1) % n; // old segment prev (v_prev -> v_src) becomes src -> prev
        out[i] = .{ .x = loop[src].x, .y = loop[src].y, .b = -loop[prev].b };
    }
    return out;
}

// ---- robust orientation ------------------------------------------------------------------------------

fn twoSum(a: f64, b: f64) [2]f64 {
    const s = a + b;
    const bb = s - a;
    const e = (a - (s - bb)) + (b - bb);
    return .{ s, e };
}

fn twoProd(a: f64, b: f64) [2]f64 {
    const p = a * b;
    const e = @mulAdd(f64, a, b, -p);
    return .{ p, e };
}

const Exp = struct {
    v: [24]f64 = undefined,
    n: usize = 0,

    fn grow(self: *Exp, b: f64) void {
        var q = b;
        var k: usize = 0;
        var i: usize = 0;
        while (i < self.n) : (i += 1) {
            const s = twoSum(q, self.v[i]);
            q = s[0];
            if (s[1] != 0) {
                self.v[k] = s[1];
                k += 1;
            }
        }
        if (q != 0 or k == 0) {
            self.v[k] = q;
            k += 1;
        }
        self.n = k;
    }
    fn sign(self: Exp) i32 {
        var i = self.n;
        while (i > 0) {
            i -= 1;
            if (self.v[i] > 0) return 1;
            if (self.v[i] < 0) return -1;
        }
        return 0;
    }
};

/// Sign of the orientation determinant: +1 if c is left of a->b, -1 right, 0 collinear (exact).
pub fn orient2d(a: V2, b: V2, c: V2) i32 {
    const detleft = (a.x - c.x) * (b.y - c.y);
    const detright = (a.y - c.y) * (b.x - c.x);
    const det = detleft - detright;
    const bound = 3.3306690738754716e-16 * (@abs(detleft) + @abs(detright));
    if (det > bound) return 1;
    if (det < -bound) return -1;
    // Exact fallback with expansions: (ax-cx)(by-cy) - (ay-cy)(bx-cx).
    const dx1 = twoSum(a.x, -c.x);
    const dy1 = twoSum(b.y, -c.y);
    const dy2 = twoSum(a.y, -c.y);
    const dx2 = twoSum(b.x, -c.x);
    var e = Exp{};
    const xs = [_][2]f64{ dx1, dy1, dy2, dx2 };
    // product 1: (dx1[0]+dx1[1])*(dy1[0]+dy1[1])
    inline for (.{ 0, 1 }) |i| inline for (.{ 0, 1 }) |j| {
        const p = twoProd(xs[0][i], xs[1][j]);
        e.grow(p[1]);
        e.grow(p[0]);
        const q = twoProd(xs[2][i], xs[3][j]);
        e.grow(-q[1]);
        e.grow(-q[0]);
    };
    return e.sign();
}

// ---- point location ---------------------------------------------------------------------------------

pub fn distPointSeg(p: V2, a: V2, b: V2) f64 {
    const ab = b.sub(a);
    const l2 = ab.dot(ab);
    if (l2 == 0) return p.dist(a);
    const t = std.math.clamp(p.sub(a).dot(ab) / l2, 0, 1);
    return p.dist(V2.lerp(a, b, t));
}

pub const Loc = enum { outside, inside, boundary };

/// Winding-number point location against flattened loops. Loops use the CCW-filled / CW-hole
/// convention (or any consistent set for which a non-zero winding means inside). `tol` is the
/// boundary band.
pub fn locate(p: V2, loops: []const []const V2, tol: f64) Loc {
    var wn: i32 = 0;
    for (loops) |lp| {
        for (lp, 0..) |a, i| {
            const b = lp[(i + 1) % lp.len];
            if (distPointSeg(p, a, b) <= tol) return .boundary;
            if (a.y <= p.y) {
                if (b.y > p.y and orient2d(a, b, p) > 0) wn += 1;
            } else {
                if (b.y <= p.y and orient2d(a, b, p) < 0) wn -= 1;
            }
        }
    }
    return if (wn != 0) .inside else .outside;
}

/// Even-odd variant for sets whose orientation is unspecified.
pub fn locateEvenOdd(p: V2, loops: []const []const V2, tol: f64) Loc {
    var inside = false;
    for (loops) |lp| {
        for (lp, 0..) |a, i| {
            const b = lp[(i + 1) % lp.len];
            if (distPointSeg(p, a, b) <= tol) return .boundary;
            if ((a.y > p.y) != (b.y > p.y)) {
                const xint = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if (p.x < xint) inside = !inside;
            }
        }
    }
    return if (inside) .inside else .outside;
}

pub fn pointInLoopEO(p: V2, lp: []const V2) bool {
    var inside = false;
    for (lp, 0..) |a, i| {
        const b = lp[(i + 1) % lp.len];
        if ((a.y > p.y) != (b.y > p.y)) {
            const xint = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if (p.x < xint) inside = !inside;
        }
    }
    return inside;
}

pub fn centroidV(pts: []const V2) V2 {
    var a: f64 = 0;
    var cx: f64 = 0;
    var cy: f64 = 0;
    for (pts, 0..) |p, i| {
        const q = pts[(i + 1) % pts.len];
        const w = p.x * q.y - q.x * p.y;
        a += w;
        cx += (p.x + q.x) * w;
        cy += (p.y + q.y) * w;
    }
    if (@abs(a) < 1e-12) {
        var sx: f64 = 0;
        var sy: f64 = 0;
        for (pts) |p| {
            sx += p.x;
            sy += p.y;
        }
        const n: f64 = @floatFromInt(@max(pts.len, 1));
        return .{ .x = sx / n, .y = sy / n };
    }
    return .{ .x = cx / (3.0 * a), .y = cy / (3.0 * a) };
}

// ---- segment intersection ----------------------------------------------------------------------------

/// Intersection parameters of two straight segments. Fills `ta`/`tb` (positions along a and b in
/// [0,1]) and returns the number of intersection points (0, 1, or 2 for collinear overlap, where
/// the points are the overlap's endpoints).
pub fn segSeg(a0: V2, a1: V2, b0: V2, b1: V2, ta: *[2]f64, tb: *[2]f64) usize {
    const d1 = orient2d(b0, b1, a0);
    const d2 = orient2d(b0, b1, a1);
    const d3 = orient2d(a0, a1, b0);
    const d4 = orient2d(a0, a1, b1);
    if (d1 == 0 and d2 == 0 and d3 == 0 and d4 == 0) {
        // collinear: project onto a's direction
        const ab = a1.sub(a0);
        const l2 = ab.dot(ab);
        if (l2 == 0) return 0;
        const tb0 = b0.sub(a0).dot(ab) / l2;
        const tb1 = b1.sub(a0).dot(ab) / l2;
        const lo = @max(0.0, @min(tb0, tb1));
        const hi = @min(1.0, @max(tb0, tb1));
        if (lo > hi) return 0;
        const bb = b1.sub(b0);
        const bl2 = bb.dot(bb);
        ta[0] = lo;
        ta[1] = hi;
        if (bl2 == 0) {
            tb[0] = 0;
            tb[1] = 0;
        } else {
            tb[0] = std.math.clamp(a0.add(ab.scale(lo)).sub(b0).dot(bb) / bl2, 0, 1);
            tb[1] = std.math.clamp(a0.add(ab.scale(hi)).sub(b0).dot(bb) / bl2, 0, 1);
        }
        return if (hi - lo < 1e-15) 1 else 2;
    }
    if (d1 * d2 > 0 or d3 * d4 > 0) return 0;
    // At least a touch or a proper crossing.
    const r = a1.sub(a0);
    const s = b1.sub(b0);
    const denom = r.cross(s);
    if (denom == 0) return 0;
    const w = b0.sub(a0);
    var t = w.cross(s) / denom;
    var u = w.cross(r) / denom;
    // Endpoint touches resolve exactly.
    if (d1 == 0 and d2 != 0) t = 0;
    if (d2 == 0 and d1 != 0) t = 1;
    if (d3 == 0 and d4 != 0) u = 0;
    if (d4 == 0 and d3 != 0) u = 1;
    ta[0] = std.math.clamp(t, 0, 1);
    tb[0] = std.math.clamp(u, 0, 1);
    return 1;
}

/// Parameters t (fraction of the sweep) at which the arc (p0,p1,bulge) meets the straight segment
/// b0->b1, with the matching parameter u on the straight segment. Up to 2 hits.
pub fn arcSeg(p0: V2, p1: V2, bulge: f64, b0: V2, b1: V2, ta: *[2]f64, tb: *[2]f64) usize {
    const arc = arcOf(p0, p1, bulge);
    const d = b1.sub(b0);
    const f = b0.sub(arc.c);
    const aa = d.dot(d);
    if (aa == 0) return 0;
    const bq = 2.0 * f.dot(d);
    const cq = f.dot(f) - arc.r * arc.r;
    const disc = bq * bq - 4.0 * aa * cq;
    if (disc < 0) {
        // tangent within rounding?
        if (disc > -1e-18 * aa * arc.r * arc.r) {
            // treat as tangent: single root
        } else return 0;
    }
    const sq = @sqrt(@max(disc, 0));
    var n: usize = 0;
    const roots = [2]f64{ (-bq - sq) / (2.0 * aa), (-bq + sq) / (2.0 * aa) };
    for (roots, 0..) |u, k| {
        if (k == 1 and sq == 0) break;
        if (u < -1e-12 or u > 1 + 1e-12) continue;
        const uc = std.math.clamp(u, 0, 1);
        const pt = V2.lerp(b0, b1, uc);
        const ang = std.math.atan2(pt.y - arc.c.y, pt.x - arc.c.x);
        // fraction along the sweep
        var da = ang - arc.a0;
        const two_pi = 2.0 * std.math.pi;
        if (arc.sweep >= 0) {
            da = @mod(da, two_pi);
            if (da > arc.sweep + 1e-9) {
                // allow the end point within tolerance
                if (da - two_pi < -1e-9) continue;
                da -= two_pi;
            }
        } else {
            da = -@mod(-da, two_pi);
            if (da < arc.sweep - 1e-9) {
                if (da + two_pi > 1e-9) continue;
                da += two_pi;
            }
        }
        const t = std.math.clamp(da / arc.sweep, 0, 1);
        ta[n] = t;
        tb[n] = uc;
        n += 1;
    }
    return n;
}

// ---- tests ---------------------------------------------------------------------------------------------

test "bulge arc round trip" {
    // quarter circle radius 1 from (1,0) to (0,1), CCW => bulge tan(pi/8)
    const b = @tan(std.math.pi / 8.0);
    const arc = arcOf(.{ .x = 1, .y = 0 }, .{ .x = 0, .y = 1 }, b);
    try std.testing.expectApproxEqAbs(0.0, arc.c.x, 1e-12);
    try std.testing.expectApproxEqAbs(0.0, arc.c.y, 1e-12);
    try std.testing.expectApproxEqAbs(1.0, arc.r, 1e-12);
    try std.testing.expectApproxEqAbs(std.math.pi / 2.0, arc.sweep, 1e-12);
    // negative bulge: CW, centre on the other side
    const arc2 = arcOf(.{ .x = 1, .y = 0 }, .{ .x = 0, .y = 1 }, -b);
    try std.testing.expectApproxEqAbs(1.0, arc2.c.x, 1e-12);
    try std.testing.expectApproxEqAbs(1.0, arc2.c.y, 1e-12);
}

test "signed area of a circle made of two arcs" {
    const loop = [_]Pt{ .{ .x = 1, .y = 0, .b = 1 }, .{ .x = -1, .y = 0, .b = 1 } };
    try std.testing.expectApproxEqAbs(std.math.pi, signedArea(&loop), 1e-12);
}

test "orient2d exact on nearly collinear" {
    const a = V2.init(0, 0);
    const b = V2.init(1e17, 1e17);
    try std.testing.expectEqual(@as(i32, 0), orient2d(a, b, V2.init(0.5e17, 0.5e17)));
    try std.testing.expectEqual(@as(i32, 1), orient2d(V2.init(0, 0), V2.init(1, 0), V2.init(0.5, 1e-300)));
    try std.testing.expectEqual(@as(i32, -1), orient2d(V2.init(0, 0), V2.init(1, 0), V2.init(0.5, -1e-300)));
}

test "segSeg cases" {
    var ta: [2]f64 = undefined;
    var tb: [2]f64 = undefined;
    // proper cross
    try std.testing.expectEqual(@as(usize, 1), segSeg(V2.init(0, 0), V2.init(2, 2), V2.init(0, 2), V2.init(2, 0), &ta, &tb));
    try std.testing.expectApproxEqAbs(0.5, ta[0], 1e-12);
    // T junction
    try std.testing.expectEqual(@as(usize, 1), segSeg(V2.init(0, 0), V2.init(2, 0), V2.init(1, 0), V2.init(1, 5), &ta, &tb));
    try std.testing.expectApproxEqAbs(0.5, ta[0], 1e-12);
    try std.testing.expectApproxEqAbs(0.0, tb[0], 1e-12);
    // collinear overlap
    try std.testing.expectEqual(@as(usize, 2), segSeg(V2.init(0, 0), V2.init(4, 0), V2.init(1, 0), V2.init(6, 0), &ta, &tb));
    try std.testing.expectApproxEqAbs(0.25, ta[0], 1e-12);
    try std.testing.expectApproxEqAbs(1.0, ta[1], 1e-12);
    // disjoint collinear
    try std.testing.expectEqual(@as(usize, 0), segSeg(V2.init(0, 0), V2.init(1, 0), V2.init(2, 0), V2.init(3, 0), &ta, &tb));
    // parallel apart
    try std.testing.expectEqual(@as(usize, 0), segSeg(V2.init(0, 0), V2.init(1, 0), V2.init(0, 1), V2.init(1, 1), &ta, &tb));
}

test "arc vs segment" {
    var ta: [2]f64 = undefined;
    var tb: [2]f64 = undefined;
    // upper semicircle of the unit circle from (1,0) to (-1,0), CCW => bulge 1
    const n = arcSeg(V2.init(1, 0), V2.init(-1, 0), 1, V2.init(-2, 0.5), V2.init(2, 0.5), &ta, &tb);
    try std.testing.expectEqual(@as(usize, 2), n);
    try std.testing.expectApproxEqAbs(1.0 / 6.0, @min(ta[0], ta[1]), 1e-9);
    try std.testing.expectApproxEqAbs(5.0 / 6.0, @max(ta[0], ta[1]), 1e-9);
    // below the arc: no hit
    try std.testing.expectEqual(@as(usize, 0), arcSeg(V2.init(1, 0), V2.init(-1, 0), 1, V2.init(-2, -0.5), V2.init(2, -0.5), &ta, &tb));
}

test "reverse loop keeps shape" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const loop = [_]Pt{ .{ .x = 0, .y = 0, .b = 0.5 }, .{ .x = 2, .y = 0 }, .{ .x = 2, .y = 2 }, .{ .x = 0, .y = 2 } };
    const rev = try reverseLoop(arena.allocator(), &loop);
    try std.testing.expectApproxEqAbs(-signedArea(&loop), signedArea(rev), 1e-12);
}
