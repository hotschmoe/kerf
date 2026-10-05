const Style = @import("../../style.zig").Style;
const JustifyMethod = @import("cell.zig").JustifyMethod;

pub const Overflow = enum {
    fold,
    ellipsis,
    crop,
};

pub const Column = struct {
    header: []const u8,
    justify: JustifyMethod = .left,
    style: Style = Style.empty,
    header_style: Style = Style.empty,
    width: ?usize = null,
    min_width: ?usize = null,
    max_width: ?usize = null,
    no_wrap: bool = false,
    ratio: ?u8 = null,
    overflow: Overflow = .fold,
    ellipsis: []const u8 = "...",

    pub fn init(header: []const u8) Column {
        return .{ .header = header };
    }

    pub fn withJustify(self: Column, j: JustifyMethod) Column {
        var c = self;
        c.justify = j;
        return c;
    }

    pub fn withWidth(self: Column, w: usize) Column {
        var c = self;
        c.width = w;
        return c;
    }

    pub fn withMinWidth(self: Column, w: usize) Column {
        var c = self;
        c.min_width = w;
        return c;
    }

    pub fn withMaxWidth(self: Column, w: usize) Column {
        var c = self;
        c.max_width = w;
        return c;
    }

    pub fn withStyle(self: Column, s: Style) Column {
        var c = self;
        c.style = s;
        return c;
    }

    pub fn withHeaderStyle(self: Column, s: Style) Column {
        var c = self;
        c.header_style = s;
        return c;
    }

    pub fn withRatio(self: Column, r: u8) Column {
        var c = self;
        c.ratio = r;
        return c;
    }

    pub fn withOverflow(self: Column, o: Overflow) Column {
        var c = self;
        c.overflow = o;
        return c;
    }

    pub fn withEllipsis(self: Column, e: []const u8) Column {
        var c = self;
        c.ellipsis = e;
        return c;
    }

    pub fn withNoWrap(self: Column, nw: bool) Column {
        var c = self;
        c.no_wrap = nw;
        return c;
    }
};
