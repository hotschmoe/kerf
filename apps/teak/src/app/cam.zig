//! Cameras: 2D pan/zoom (model inches <-> screen px) and a 3D orbit camera
//! producing a column-major view-projection matrix.

const std = @import("std");

pub const Cam2D = struct {
    /// Screen px per model inch.
    scale: f32 = 8,
    /// Screen position (canvas-local px) of model (0,0). Screen y grows down;
    /// model y grows up.
    ox: f32 = 0,
    oy: f32 = 0,

    pub fn toScreen(self: Cam2D, x: f64, y: f64) [2]f32 {
        return .{ self.ox + @as(f32, @floatCast(x)) * self.scale, self.oy - @as(f32, @floatCast(y)) * self.scale };
    }

    pub fn toModel(self: Cam2D, sx: f32, sy: f32) [2]f64 {
        return .{ (sx - self.ox) / self.scale, (self.oy - sy) / self.scale };
    }

    /// Zoom by `factor` keeping the model point under screen (sx, sy) fixed.
    pub fn zoomAt(self: *Cam2D, sx: f32, sy: f32, factor: f32) void {
        const clamped = std.math.clamp(self.scale * factor, 0.5, 400);
        const f = clamped / self.scale;
        self.ox = sx - (sx - self.ox) * f;
        self.oy = sy - (sy - self.oy) * f;
        self.scale = clamped;
    }

    pub fn pan(self: *Cam2D, dx: f32, dy: f32) void {
        self.ox += dx;
        self.oy += dy;
    }

    /// Fit model bounds [x0,y0,x1,y1] into a w x h canvas with a margin (px).
    pub fn fit(bounds: [4]f64, w: f32, h: f32, margin: f32) Cam2D {
        const bw: f32 = @floatCast(@max(bounds[2] - bounds[0], 1e-3));
        const bh: f32 = @floatCast(@max(bounds[3] - bounds[1], 1e-3));
        const aw = @max(w - 2 * margin, 8);
        const ah = @max(h - 2 * margin, 8);
        const s = @min(aw / bw, ah / bh);
        const cx: f32 = @floatCast((bounds[0] + bounds[2]) * 0.5);
        const cy: f32 = @floatCast((bounds[1] + bounds[3]) * 0.5);
        return .{ .scale = s, .ox = w * 0.5 - cx * s, .oy = h * 0.5 + cy * s };
    }
};

pub const Mat4 = [16]f32;

pub const Orbit = struct {
    yaw: f32 = -0.6, // radians around +Y (model Y is up)
    pitch: f32 = 0.45,
    dist: f32 = 100,
    target: [3]f32 = .{ 0, 0, 0 },

    pub const Preset = enum { front, iso, top, right };

    pub fn setPreset(self: *Orbit, p: Preset) void {
        switch (p) {
            .front => {
                self.yaw = 0;
                self.pitch = 0;
            },
            .iso => {
                self.yaw = -0.62;
                self.pitch = 0.5;
            },
            .top => {
                self.yaw = 0;
                self.pitch = 1.5607;
            },
            .right => {
                self.yaw = -1.5707963;
                self.pitch = 0;
            },
        }
    }

    pub fn rotate(self: *Orbit, dx_px: f32, dy_px: f32) void {
        self.yaw -= dx_px * 0.008;
        self.pitch = std.math.clamp(self.pitch + dy_px * 0.008, -1.5607, 1.5607);
    }

    pub fn zoom(self: *Orbit, factor: f32) void {
        self.dist = std.math.clamp(self.dist * factor, 2, 5000);
    }

    /// Pan in the view plane by screen px (dist-scaled).
    pub fn panPx(self: *Orbit, dx_px: f32, dy_px: f32, canvas_h: f32) void {
        const k = self.dist * 0.9 / @max(canvas_h, 1);
        const e = self.eye();
        const f = norm(sub3(self.target, e));
        const right = norm(cross(f, .{ 0, 1, 0 }));
        const up = cross(right, f);
        for (0..3) |i| self.target[i] -= right[i] * dx_px * k - up[i] * dy_px * k;
    }

    pub fn eye(self: Orbit) [3]f32 {
        const cp = @cos(self.pitch);
        return .{
            self.target[0] + self.dist * cp * @sin(self.yaw),
            self.target[1] + self.dist * @sin(self.pitch),
            self.target[2] + self.dist * cp * @cos(self.yaw),
        };
    }

    /// Frame a bounding box (min, max).
    pub fn frame(self: *Orbit, lo: [3]f32, hi: [3]f32) void {
        for (0..3) |i| self.target[i] = (lo[i] + hi[i]) * 0.5;
        const dx = hi[0] - lo[0];
        const dy = hi[1] - lo[1];
        const dz = hi[2] - lo[2];
        const r = 0.5 * @sqrt(dx * dx + dy * dy + dz * dz);
        self.dist = @max(r * 2.6, 6);
    }

    /// Column-major view-projection (right-handed, depth 0..1 as WebGPU).
    pub fn viewProj(self: Orbit, aspect: f32) Mat4 {
        const e = self.eye();
        const v = lookAt(e, self.target, .{ 0, 1, 0 });
        const near = @max(self.dist * 0.02, 0.1);
        const far = self.dist * 20;
        const p = perspective(0.6, aspect, near, far);
        return mul(p, v);
    }
};

fn sub3(a: [3]f32, b: [3]f32) [3]f32 {
    return .{ a[0] - b[0], a[1] - b[1], a[2] - b[2] };
}
fn cross(a: [3]f32, b: [3]f32) [3]f32 {
    return .{ a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0] };
}
fn dot(a: [3]f32, b: [3]f32) f32 {
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}
fn norm(a: [3]f32) [3]f32 {
    const l = @sqrt(dot(a, a));
    if (l < 1e-9) return .{ 0, 0, 1 };
    return .{ a[0] / l, a[1] / l, a[2] / l };
}

pub fn lookAt(eye: [3]f32, target: [3]f32, up: [3]f32) Mat4 {
    const f = norm(sub3(target, eye));
    var s = cross(f, up);
    if (dot(s, s) < 1e-12) s = .{ 1, 0, 0 };
    s = norm(s);
    const u = cross(s, f);
    return .{
        s[0],  u[0],  -f[0], 0,
        s[1],  u[1],  -f[1], 0,
        s[2],  u[2],  -f[2], 0,
        -dot(s, eye), -dot(u, eye), dot(f, eye), 1,
    };
}

pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) Mat4 {
    const f = 1.0 / @tan(fovy * 0.5);
    return .{
        f / aspect, 0, 0,                         0,
        0,          f, 0,                         0,
        0,          0, far / (near - far),        -1,
        0,          0, (far * near) / (near - far), 0,
    };
}

/// C = A * B, column-major.
pub fn mul(a: Mat4, b: Mat4) Mat4 {
    var c: Mat4 = undefined;
    for (0..4) |col| for (0..4) |row| {
        var s: f32 = 0;
        for (0..4) |k| s += a[k * 4 + row] * b[col * 4 + k];
        c[col * 4 + row] = s;
    };
    return c;
}

pub fn transformPoint(m: Mat4, p: [3]f32) [4]f32 {
    var r: [4]f32 = undefined;
    for (0..4) |row| r[row] = m[0 * 4 + row] * p[0] + m[1 * 4 + row] * p[1] + m[2 * 4 + row] * p[2] + m[3 * 4 + row];
    return r;
}

test "Cam2D zoomAt keeps the anchor fixed" {
    var c: Cam2D = .{ .scale = 10, .ox = 100, .oy = 200 };
    const before = c.toModel(150, 120);
    c.zoomAt(150, 120, 2);
    const after = c.toModel(150, 120);
    try std.testing.expectApproxEqAbs(before[0], after[0], 1e-4);
    try std.testing.expectApproxEqAbs(before[1], after[1], 1e-4);
}

test "Cam2D fit centers bounds" {
    const c = Cam2D.fit(.{ 0, 0, 100, 50 }, 800, 400, 20);
    const p = c.toScreen(50, 25);
    try std.testing.expectApproxEqAbs(@as(f32, 400), p[0], 0.01);
    try std.testing.expectApproxEqAbs(@as(f32, 200), p[1], 0.01);
}

test "orbit projects target to the center" {
    var o: Orbit = .{};
    o.target = .{ 5, 6, 7 };
    const m = o.viewProj(1.5);
    const p = transformPoint(m, o.target);
    try std.testing.expectApproxEqAbs(@as(f32, 0), p[0] / p[3], 1e-4);
    try std.testing.expectApproxEqAbs(@as(f32, 0), p[1] / p[3], 1e-4);
    try std.testing.expect(p[2] / p[3] > 0 and p[2] / p[3] < 1);
}
