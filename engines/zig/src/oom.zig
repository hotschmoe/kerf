//! Out-of-memory sensor (REVIEW SAF-6). Diagnostics code (`Diags.add`, `joinIds`, `ftin`, ~60 more sites) swallows
//! `error.OutOfMemory` with `catch return` / `catch "?"` so that a message can be built without a `try` chain. That is
//! convenient and, under memory pressure (wasm has no overcommit), wrong: the *error disappears* and `apply` can report
//! `ok: true` for a document that failed to compile. Instead of threading a flag through every site, `kerf.call` hands its
//! arena a child allocator that records whether any allocation was ever refused; after the call finished the failure is
//! reported as `error.OutOfMemory` (CLI `kerf: OutOfMemory`, wasm `E_OOM`, server 500) and the partial result is dropped.

const std = @import("std");
const Allocator = std.mem.Allocator;
const Alignment = std.mem.Alignment;

pub const Sensor = struct {
    child: Allocator,
    /// An `alloc` of the child returned null at least once.
    failed: bool = false,

    pub fn init(child: Allocator) Sensor {
        return .{ .child = child };
    }

    pub fn allocator(self: *Sensor) Allocator {
        return .{ .ptr = self, .vtable = &vtable };
    }

    const vtable = Allocator.VTable{ .alloc = alloc, .resize = resize, .remap = remap, .free = free };

    fn alloc(ctx: *anyopaque, len: usize, alignment: Alignment, ret_addr: usize) ?[*]u8 {
        const self: *Sensor = @ptrCast(@alignCast(ctx));
        const r = self.child.rawAlloc(len, alignment, ret_addr);
        if (r == null) self.failed = true;
        return r;
    }

    // A refused in-place resize or remap is normal (the caller allocates and copies), so only `alloc` is a failure.
    fn resize(ctx: *anyopaque, memory: []u8, alignment: Alignment, new_len: usize, ret_addr: usize) bool {
        const self: *Sensor = @ptrCast(@alignCast(ctx));
        return self.child.rawResize(memory, alignment, new_len, ret_addr);
    }

    fn remap(ctx: *anyopaque, memory: []u8, alignment: Alignment, new_len: usize, ret_addr: usize) ?[*]u8 {
        const self: *Sensor = @ptrCast(@alignCast(ctx));
        return self.child.rawRemap(memory, alignment, new_len, ret_addr);
    }

    fn free(ctx: *anyopaque, memory: []u8, alignment: Alignment, ret_addr: usize) void {
        const self: *Sensor = @ptrCast(@alignCast(ctx));
        self.child.rawFree(memory, alignment, ret_addr);
    }
};

test "sensor records a refused allocation and passes everything else through" {
    var failing = std.testing.FailingAllocator.init(std.testing.allocator, .{ .fail_index = 1 });
    var sensor = Sensor.init(failing.allocator());
    const a = sensor.allocator();
    const first = try a.alloc(u8, 16);
    defer a.free(first);
    try std.testing.expect(!sensor.failed);
    try std.testing.expectError(error.OutOfMemory, a.alloc(u8, 16));
    try std.testing.expect(sensor.failed);
}
