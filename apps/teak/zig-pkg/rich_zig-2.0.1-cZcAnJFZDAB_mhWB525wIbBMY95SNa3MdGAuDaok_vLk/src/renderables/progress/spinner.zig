const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;

pub const Spinner = struct {
    frames: []const []const u8 = &DEFAULT_FRAMES,
    current_frame: usize = 0,
    style: Style = Style.empty,

    const DEFAULT_FRAMES = [_][]const u8{
        "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}",
        "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}",
        "\u{2807}", "\u{280F}",
    };

    const DOTS_FRAMES = [_][]const u8{
        "\u{28F7}", "\u{28EF}", "\u{28DF}", "\u{287F}",
        "\u{28BF}", "\u{28FB}", "\u{28FD}", "\u{28FE}",
    };

    const LINE_FRAMES = [_][]const u8{ "-", "\\", "|", "/" };

    pub fn init() Spinner {
        return .{};
    }

    pub fn dots() Spinner {
        return .{ .frames = &DOTS_FRAMES };
    }

    pub fn line() Spinner {
        return .{ .frames = &LINE_FRAMES };
    }

    pub fn withStyle(self: Spinner, s: Style) Spinner {
        var sp = self;
        sp.style = s;
        return sp;
    }

    pub fn advance(self: *Spinner) void {
        self.current_frame = (self.current_frame + 1) % self.frames.len;
    }

    pub fn render(self: Spinner, allocator: std.mem.Allocator) ![]Segment {
        const result = try allocator.alloc(Segment, 1);
        result[0] = Segment.styled(self.frames[self.current_frame], self.style);
        return result;
    }

    pub fn renderWidth(self: Spinner, _: usize, allocator: std.mem.Allocator) ![]Segment {
        return self.render(allocator);
    }
};

test "Spinner.init" {
    const spinner = Spinner.init();
    try std.testing.expectEqual(@as(usize, 0), spinner.current_frame);
}

test "Spinner.advance" {
    var spinner = Spinner.init();
    spinner.advance();
    try std.testing.expectEqual(@as(usize, 1), spinner.current_frame);
}

test "Spinner.render" {
    const allocator = std.testing.allocator;
    const spinner = Spinner.init();

    const segments = try spinner.render(allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 1), segments.len);
}
