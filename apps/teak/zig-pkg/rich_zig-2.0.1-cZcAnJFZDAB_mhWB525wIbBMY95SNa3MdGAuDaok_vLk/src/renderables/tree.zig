const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const Style = @import("../style.zig").Style;

pub const LabelContent = union(enum) {
    text: []const u8,
    segments: []const Segment,

    pub fn getText(self: LabelContent) []const u8 {
        return switch (self) {
            .text => |t| t,
            .segments => |segs| if (segs.len > 0) segs[0].text else "",
        };
    }
};

pub const KV = struct {
    key: []const u8,
    value: KVValue,

    pub const KVValue = union(enum) {
        string: []const u8,
        nested: []const KV,
    };

    pub fn string(key: []const u8, val: []const u8) KV {
        return .{ .key = key, .value = .{ .string = val } };
    }

    pub fn nested(key: []const u8, children: []const KV) KV {
        return .{ .key = key, .value = .{ .nested = children } };
    }
};

pub const TreeGuide = struct {
    vertical: []const u8 = "\u{2502}",
    horizontal: []const u8 = "\u{2500}\u{2500}",
    corner: []const u8 = "\u{2514}",
    tee: []const u8 = "\u{251C}",
    space: []const u8 = "   ",
};

pub const TreeNode = struct {
    label: []const u8,
    label_content: ?LabelContent = null,
    children: std.ArrayList(TreeNode),
    style: Style = Style.empty,
    guide_style: Style = Style.empty,
    expanded: bool = true,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, label: []const u8) TreeNode {
        return .{
            .label = label,
            .children = std.ArrayList(TreeNode).empty,
            .allocator = allocator,
        };
    }

    pub fn initWithSegments(allocator: std.mem.Allocator, segments: []const Segment) TreeNode {
        return .{
            .label = if (segments.len > 0) segments[0].text else "",
            .label_content = .{ .segments = segments },
            .children = std.ArrayList(TreeNode).empty,
            .allocator = allocator,
        };
    }

    pub fn getLabelContent(self: TreeNode) LabelContent {
        if (self.label_content) |content| {
            return content;
        }
        return .{ .text = self.label };
    }

    pub fn deinit(self: *TreeNode) void {
        for (self.children.items) |*child| {
            child.deinit();
        }
        self.children.deinit(self.allocator);
    }

    pub fn addChild(self: *TreeNode, child: TreeNode) !void {
        try self.children.append(self.allocator, child);
    }

    pub fn addChildLabel(self: *TreeNode, label: []const u8) !*TreeNode {
        try self.children.append(self.allocator, TreeNode.init(self.allocator, label));
        return &self.children.items[self.children.items.len - 1];
    }

    pub fn withStyle(self: TreeNode, s: Style) TreeNode {
        var n = self;
        n.style = s;
        return n;
    }

    pub fn withGuideStyle(self: TreeNode, s: Style) TreeNode {
        var n = self;
        n.guide_style = s;
        return n;
    }

    pub fn collapsed(self: TreeNode) TreeNode {
        var n = self;
        n.expanded = false;
        return n;
    }

    pub fn addFromKV(self: *TreeNode, items: []const KV) !void {
        for (items) |item| {
            var child = TreeNode.init(self.allocator, item.key);
            switch (item.value) {
                .string => |val| _ = try child.addChildLabel(val),
                .nested => |nested_items| try child.addFromKV(nested_items),
            }
            try self.children.append(self.allocator, child);
        }
    }
};

pub const Tree = struct {
    root: TreeNode,
    guide: TreeGuide = .{},
    hide_root: bool = false,

    pub fn init(root: TreeNode) Tree {
        return .{ .root = root };
    }

    pub fn withGuide(self: Tree, guide: TreeGuide) Tree {
        var t = self;
        t.guide = guide;
        return t;
    }

    pub fn hideRoot(self: Tree) Tree {
        var t = self;
        t.hide_root = true;
        return t;
    }

    pub fn render(self: Tree, _: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (!self.hide_root) {
            try renderLabel(&segments, allocator, &self.root);
            try segments.append(allocator, Segment.line());
        }

        // Track ancestor levels: true = has more siblings (show vertical), false = last child (show space)
        var levels: std.ArrayList(bool) = .empty;
        defer levels.deinit(allocator);

        try self.renderChildren(&segments, allocator, &self.root, &levels);

        return segments.toOwnedSlice(allocator);
    }

    fn renderLabel(segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, node: *const TreeNode) !void {
        const content = node.getLabelContent();
        switch (content) {
            .text => |t| {
                try segments.append(allocator, Segment.styled(t, node.style));
            },
            .segments => |segs| {
                for (segs) |seg| {
                    const combined_style = if (seg.style) |s| node.style.combine(s) else node.style;
                    try segments.append(allocator, Segment.styledOptional(seg.text, if (combined_style.isEmpty()) null else combined_style));
                }
            },
        }
    }

    fn renderChildren(self: Tree, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, node: *const TreeNode, levels: *std.ArrayList(bool)) !void {
        if (!node.expanded) return;

        for (node.children.items, 0..) |*child, i| {
            const is_last = (i == node.children.items.len - 1);

            // Render prefix based on ancestor levels (uses static string refs, no lifetime issues)
            for (levels.items) |has_more_siblings| {
                if (has_more_siblings) {
                    try segments.append(allocator, Segment.styled(self.guide.vertical, node.guide_style));
                    try segments.append(allocator, Segment.plain("  "));
                } else {
                    try segments.append(allocator, Segment.plain(self.guide.space));
                }
            }

            const guide_char = if (is_last) self.guide.corner else self.guide.tee;
            try segments.append(allocator, Segment.styled(guide_char, node.guide_style));
            try segments.append(allocator, Segment.styled(self.guide.horizontal, node.guide_style));
            try segments.append(allocator, Segment.plain(" "));

            try renderLabel(segments, allocator, child);
            try segments.append(allocator, Segment.line());

            // Recurse
            if (child.children.items.len > 0 and child.expanded) {
                try levels.append(allocator, !is_last);
                try self.renderChildren(segments, allocator, child, levels);
                _ = levels.pop();
            }
        }
    }
};

// Tests
test "TreeNode.init" {
    const allocator = std.testing.allocator;
    var node = TreeNode.init(allocator, "root");
    defer node.deinit();

    try std.testing.expectEqualStrings("root", node.label);
    try std.testing.expectEqual(@as(usize, 0), node.children.items.len);
}

test "TreeNode.addChildLabel" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "root");
    defer root.deinit();

    _ = try root.addChildLabel("child1");
    _ = try root.addChildLabel("child2");

    try std.testing.expectEqual(@as(usize, 2), root.children.items.len);
    try std.testing.expectEqualStrings("child1", root.children.items[0].label);
}

test "Tree.init" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "root");
    defer root.deinit();

    const tree = Tree.init(root);
    try std.testing.expect(!tree.hide_root);
}

test "Tree.render basic" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "root");
    defer root.deinit();

    _ = try root.addChildLabel("child1");
    _ = try root.addChildLabel("child2");

    const tree = Tree.init(root);
    const segments = try tree.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    // Verify root label is present
    var found_root = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "root")) {
            found_root = true;
            break;
        }
    }
    try std.testing.expect(found_root);
}

test "Tree.hideRoot" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "root");
    defer root.deinit();

    _ = try root.addChildLabel("child");

    const tree = Tree.init(root).hideRoot();
    const segments = try tree.render(80, allocator);
    defer allocator.free(segments);

    // Root label should not be present
    for (segments) |seg| {
        try std.testing.expect(!std.mem.eql(u8, seg.text, "root"));
    }
}

test "LabelContent.text" {
    const content = LabelContent{ .text = "hello" };
    try std.testing.expectEqualStrings("hello", content.getText());
}

test "LabelContent.segments" {
    const segs = [_]Segment{Segment.plain("world")};
    const content = LabelContent{ .segments = &segs };
    try std.testing.expectEqualStrings("world", content.getText());
}

test "TreeNode.initWithSegments" {
    const allocator = std.testing.allocator;
    const segs = [_]Segment{
        Segment.styled("styled", Style.empty.bold()),
        Segment.plain(" text"),
    };
    var node = TreeNode.initWithSegments(allocator, &segs);
    defer node.deinit();

    try std.testing.expect(node.label_content != null);
    try std.testing.expectEqualStrings("styled", node.label);
}

test "TreeNode.getLabelContent" {
    const allocator = std.testing.allocator;
    var node = TreeNode.init(allocator, "simple");
    defer node.deinit();

    const content = node.getLabelContent();
    try std.testing.expectEqualStrings("simple", content.text);
}

test "Tree.render with segments" {
    const allocator = std.testing.allocator;
    const segs = [_]Segment{
        Segment.styled("bold", Style.empty.bold()),
        Segment.plain(" root"),
    };
    var root = TreeNode.initWithSegments(allocator, &segs);
    defer root.deinit();

    _ = try root.addChildLabel("child");

    const tree = Tree.init(root);
    const segments = try tree.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "KV.string" {
    const kv = KV.string("name", "Alice");
    try std.testing.expectEqualStrings("name", kv.key);
    try std.testing.expectEqualStrings("Alice", kv.value.string);
}

test "KV.nested" {
    const children = [_]KV{
        KV.string("a", "1"),
        KV.string("b", "2"),
    };
    const kv = KV.nested("parent", &children);
    try std.testing.expectEqualStrings("parent", kv.key);
    try std.testing.expectEqual(@as(usize, 2), kv.value.nested.len);
}

test "TreeNode.addFromKV simple" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "config");
    defer root.deinit();

    const kvs = [_]KV{
        KV.string("host", "localhost"),
        KV.string("port", "8080"),
    };
    try root.addFromKV(&kvs);

    try std.testing.expectEqual(@as(usize, 2), root.children.items.len);
    try std.testing.expectEqualStrings("host", root.children.items[0].label);
    try std.testing.expectEqual(@as(usize, 1), root.children.items[0].children.items.len);
    try std.testing.expectEqualStrings("localhost", root.children.items[0].children.items[0].label);
}

test "TreeNode.addFromKV nested" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "app");
    defer root.deinit();

    const db_config = [_]KV{
        KV.string("host", "db.example.com"),
        KV.string("port", "5432"),
    };
    const kvs = [_]KV{
        KV.string("name", "myapp"),
        KV.nested("database", &db_config),
    };
    try root.addFromKV(&kvs);

    try std.testing.expectEqual(@as(usize, 2), root.children.items.len);
    try std.testing.expectEqualStrings("database", root.children.items[1].label);
    try std.testing.expectEqual(@as(usize, 2), root.children.items[1].children.items.len);
}

test "Tree.render with KV" {
    const allocator = std.testing.allocator;
    var root = TreeNode.init(allocator, "settings");
    defer root.deinit();

    const kvs = [_]KV{
        KV.string("theme", "dark"),
        KV.string("lang", "en"),
    };
    try root.addFromKV(&kvs);

    const tree = Tree.init(root);
    const segments = try tree.render(80, allocator);
    defer allocator.free(segments);

    var found_theme = false;
    var found_dark = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "theme")) found_theme = true;
        if (std.mem.eql(u8, seg.text, "dark")) found_dark = true;
    }
    try std.testing.expect(found_theme);
    try std.testing.expect(found_dark);
}
