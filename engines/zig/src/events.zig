//! SSE hub: a bounded ring of pre-formatted frames. Publishers append, every SSE connection thread
//! waits on the condition and copies the frames it has not seen yet. No per-client queues, so a slow
//! client can only lose old frames, never block publishers.

const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const max_frames = 512;
pub const max_bytes: usize = 8 << 20;

const Frame = struct { seq: u64, bytes: []u8 };

pub const Hub = struct {
    gpa: Allocator,
    mu: Io.Mutex = .init,
    cond: Io.Condition = .init,
    /// Sequence number of the newest frame (0 = none yet).
    seq: u64 = 0,
    frames: std.ArrayList(Frame) = .empty,
    bytes: usize = 0,
    closed: bool = false,

    pub fn init(gpa: Allocator) Hub {
        return .{ .gpa = gpa };
    }

    pub fn deinit(h: *Hub) void {
        for (h.frames.items) |f| h.gpa.free(f.bytes);
        h.frames.deinit(h.gpa);
    }

    /// Publish `event: <name>\ndata: <data_json>\n\n`. `data_json` must be one line.
    pub fn publish(h: *Hub, io: Io, name: []const u8, data_json: []const u8) void {
        const frame = std.fmt.allocPrint(h.gpa, "event: {s}\ndata: {s}\n\n", .{ name, data_json }) catch return;
        h.mu.lockUncancelable(io);
        defer h.mu.unlock(io);
        h.seq += 1;
        h.frames.append(h.gpa, .{ .seq = h.seq, .bytes = frame }) catch {
            h.gpa.free(frame);
            return;
        };
        h.bytes += frame.len;
        while (h.frames.items.len > 1 and (h.frames.items.len > max_frames or h.bytes > max_bytes)) {
            const old = h.frames.orderedRemove(0);
            h.bytes -= old.bytes.len;
            h.gpa.free(old.bytes);
        }
        h.cond.broadcast(io);
    }

    pub fn current(h: *Hub, io: Io) u64 {
        h.mu.lockUncancelable(io);
        defer h.mu.unlock(io);
        return h.seq;
    }

    /// Block until frames newer than `*last` exist; append them to `out` and advance `*last`.
    /// Returns false when cancelled or the hub is closed.
    pub fn wait(h: *Hub, io: Io, last: *u64, out: *std.ArrayList(u8), a: Allocator) bool {
        h.mu.lockUncancelable(io);
        defer h.mu.unlock(io);
        while (h.seq == last.* and !h.closed) {
            h.cond.wait(io, &h.mu) catch return false;
        }
        if (h.closed and h.seq == last.*) return false;
        for (h.frames.items) |f| {
            if (f.seq > last.*) out.appendSlice(a, f.bytes) catch return false;
        }
        last.* = h.seq;
        return true;
    }

    pub fn close(h: *Hub, io: Io) void {
        h.mu.lockUncancelable(io);
        h.closed = true;
        h.cond.broadcast(io);
        h.mu.unlock(io);
    }
};

test "hub publish and read" {
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var hub = Hub.init(std.testing.allocator);
    defer hub.deinit();
    var last: u64 = hub.current(io);
    hub.publish(io, "ping", "{}");
    hub.publish(io, "doc_added", "{\"file\":\"a\"}");
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(std.testing.allocator);
    try std.testing.expect(hub.wait(io, &last, &out, std.testing.allocator));
    try std.testing.expectEqualStrings("event: ping\ndata: {}\n\nevent: doc_added\ndata: {\"file\":\"a\"}\n\n", out.items);
    try std.testing.expectEqual(@as(u64, 2), last);
}

test "hub ring is bounded" {
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var hub = Hub.init(std.testing.allocator);
    defer hub.deinit();
    for (0..max_frames + 50) |_| hub.publish(io, "ping", "{}");
    try std.testing.expectEqual(@as(usize, max_frames), hub.frames.items.len);
}
