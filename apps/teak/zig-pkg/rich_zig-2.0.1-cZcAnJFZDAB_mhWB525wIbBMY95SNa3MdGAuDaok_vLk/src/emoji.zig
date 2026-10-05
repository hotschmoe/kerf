const std = @import("std");

pub const emoji_map = std.StaticStringMap([]const u8).initComptime(.{
    // Smileys & People
    .{ "smile", "\u{1F604}" },
    .{ "grin", "\u{1F600}" },
    .{ "joy", "\u{1F602}" },
    .{ "rofl", "\u{1F923}" },
    .{ "wink", "\u{1F609}" },
    .{ "blush", "\u{1F60A}" },
    .{ "heart_eyes", "\u{1F60D}" },
    .{ "kissing", "\u{1F617}" },
    .{ "thinking", "\u{1F914}" },
    .{ "neutral", "\u{1F610}" },
    .{ "expressionless", "\u{1F611}" },
    .{ "unamused", "\u{1F612}" },
    .{ "sweat", "\u{1F613}" },
    .{ "pensive", "\u{1F614}" },
    .{ "confused", "\u{1F615}" },
    .{ "disappointed", "\u{1F61E}" },
    .{ "worried", "\u{1F61F}" },
    .{ "angry", "\u{1F620}" },
    .{ "rage", "\u{1F621}" },
    .{ "cry", "\u{1F622}" },
    .{ "sob", "\u{1F62D}" },
    .{ "scream", "\u{1F631}" },
    .{ "fearful", "\u{1F628}" },
    .{ "cold_sweat", "\u{1F630}" },
    .{ "sunglasses", "\u{1F60E}" },
    .{ "nerd", "\u{1F913}" },
    .{ "sleeping", "\u{1F634}" },
    .{ "drool", "\u{1F924}" },
    .{ "sick", "\u{1F912}" },
    .{ "mask", "\u{1F637}" },
    .{ "skull", "\u{1F480}" },
    .{ "alien", "\u{1F47D}" },
    .{ "robot", "\u{1F916}" },
    .{ "clap", "\u{1F44F}" },
    .{ "wave", "\u{1F44B}" },
    .{ "thumbsup", "\u{1F44D}" },
    .{ "thumbsdown", "\u{1F44E}" },
    .{ "punch", "\u{1F44A}" },
    .{ "fist", "\u{270A}" },
    .{ "ok_hand", "\u{1F44C}" },
    .{ "point_up", "\u{261D}" },
    .{ "point_down", "\u{1F447}" },
    .{ "point_left", "\u{1F448}" },
    .{ "point_right", "\u{1F449}" },
    .{ "raised_hand", "\u{270B}" },
    .{ "pray", "\u{1F64F}" },
    .{ "muscle", "\u{1F4AA}" },
    .{ "eyes", "\u{1F440}" },
    .{ "brain", "\u{1F9E0}" },

    // Hearts & Love
    .{ "heart", "\u{2764}" },
    .{ "red_heart", "\u{2764}" },
    .{ "orange_heart", "\u{1F9E1}" },
    .{ "yellow_heart", "\u{1F49B}" },
    .{ "green_heart", "\u{1F49A}" },
    .{ "blue_heart", "\u{1F499}" },
    .{ "purple_heart", "\u{1F49C}" },
    .{ "black_heart", "\u{1F5A4}" },
    .{ "white_heart", "\u{1F90D}" },
    .{ "broken_heart", "\u{1F494}" },
    .{ "sparkling_heart", "\u{1F496}" },

    // Status & Symbols
    .{ "check", "\u{2705}" },
    .{ "white_check_mark", "\u{2705}" },
    .{ "heavy_check_mark", "\u{2714}" },
    .{ "x", "\u{274C}" },
    .{ "cross_mark", "\u{274C}" },
    .{ "warning", "\u{26A0}" },
    .{ "no_entry", "\u{26D4}" },
    .{ "stop", "\u{1F6D1}" },
    .{ "question", "\u{2753}" },
    .{ "exclamation", "\u{2757}" },
    .{ "info", "\u{2139}" },
    .{ "sos", "\u{1F198}" },
    .{ "new", "\u{1F195}" },
    .{ "free", "\u{1F193}" },
    .{ "ok", "\u{1F197}" },
    .{ "cool", "\u{1F192}" },

    // Objects & Tools
    .{ "rocket", "\u{1F680}" },
    .{ "fire", "\u{1F525}" },
    .{ "sparkles", "\u{2728}" },
    .{ "star", "\u{2B50}" },
    .{ "star2", "\u{1F31F}" },
    .{ "zap", "\u{26A1}" },
    .{ "lightning", "\u{26A1}" },
    .{ "boom", "\u{1F4A5}" },
    .{ "bulb", "\u{1F4A1}" },
    .{ "key", "\u{1F511}" },
    .{ "lock", "\u{1F512}" },
    .{ "unlock", "\u{1F513}" },
    .{ "gear", "\u{2699}" },
    .{ "wrench", "\u{1F527}" },
    .{ "hammer", "\u{1F528}" },
    .{ "shield", "\u{1F6E1}" },
    .{ "sword", "\u{2694}" },
    .{ "bomb", "\u{1F4A3}" },
    .{ "trophy", "\u{1F3C6}" },
    .{ "medal", "\u{1F3C5}" },
    .{ "bell", "\u{1F514}" },
    .{ "gift", "\u{1F381}" },
    .{ "package", "\u{1F4E6}" },
    .{ "mail", "\u{2709}" },
    .{ "email", "\u{1F4E7}" },
    .{ "phone", "\u{260E}" },
    .{ "computer", "\u{1F4BB}" },
    .{ "laptop", "\u{1F4BB}" },
    .{ "keyboard", "\u{2328}" },
    .{ "printer", "\u{1F5A8}" },
    .{ "camera", "\u{1F4F7}" },
    .{ "video", "\u{1F4F9}" },
    .{ "movie", "\u{1F3AC}" },
    .{ "music", "\u{1F3B5}" },
    .{ "microphone", "\u{1F3A4}" },
    .{ "headphones", "\u{1F3A7}" },

    // Files & Documents
    .{ "file", "\u{1F4C4}" },
    .{ "folder", "\u{1F4C1}" },
    .{ "clipboard", "\u{1F4CB}" },
    .{ "memo", "\u{1F4DD}" },
    .{ "pencil", "\u{270F}" },
    .{ "pen", "\u{1F58A}" },
    .{ "book", "\u{1F4D6}" },
    .{ "bookmark", "\u{1F516}" },
    .{ "link", "\u{1F517}" },
    .{ "paperclip", "\u{1F4CE}" },
    .{ "scissors", "\u{2702}" },

    // Time & Calendar
    .{ "clock", "\u{1F550}" },
    .{ "hourglass", "\u{231B}" },
    .{ "timer", "\u{23F2}" },
    .{ "calendar", "\u{1F4C5}" },

    // Nature
    .{ "sun", "\u{2600}" },
    .{ "moon", "\u{1F319}" },
    .{ "cloud", "\u{2601}" },
    .{ "rain", "\u{1F327}" },
    .{ "snow", "\u{2744}" },
    .{ "rainbow", "\u{1F308}" },
    .{ "tree", "\u{1F333}" },
    .{ "flower", "\u{1F33C}" },
    .{ "rose", "\u{1F339}" },
    .{ "herb", "\u{1F33F}" },
    .{ "seedling", "\u{1F331}" },

    // Animals
    .{ "dog", "\u{1F415}" },
    .{ "cat", "\u{1F408}" },
    .{ "mouse", "\u{1F401}" },
    .{ "rabbit", "\u{1F407}" },
    .{ "bear", "\u{1F43B}" },
    .{ "fox", "\u{1F98A}" },
    .{ "wolf", "\u{1F43A}" },
    .{ "lion", "\u{1F981}" },
    .{ "tiger", "\u{1F42F}" },
    .{ "horse", "\u{1F40E}" },
    .{ "unicorn", "\u{1F984}" },
    .{ "cow", "\u{1F404}" },
    .{ "pig", "\u{1F416}" },
    .{ "chicken", "\u{1F414}" },
    .{ "bird", "\u{1F426}" },
    .{ "penguin", "\u{1F427}" },
    .{ "fish", "\u{1F41F}" },
    .{ "whale", "\u{1F40B}" },
    .{ "dolphin", "\u{1F42C}" },
    .{ "octopus", "\u{1F419}" },
    .{ "butterfly", "\u{1F98B}" },
    .{ "bee", "\u{1F41D}" },
    .{ "bug", "\u{1F41B}" },
    .{ "ant", "\u{1F41C}" },
    .{ "spider", "\u{1F577}" },
    .{ "snake", "\u{1F40D}" },
    .{ "turtle", "\u{1F422}" },
    .{ "crab", "\u{1F980}" },
    .{ "dragon", "\u{1F409}" },

    // Food & Drink
    .{ "apple", "\u{1F34E}" },
    .{ "banana", "\u{1F34C}" },
    .{ "orange", "\u{1F34A}" },
    .{ "lemon", "\u{1F34B}" },
    .{ "grapes", "\u{1F347}" },
    .{ "watermelon", "\u{1F349}" },
    .{ "strawberry", "\u{1F353}" },
    .{ "peach", "\u{1F351}" },
    .{ "pizza", "\u{1F355}" },
    .{ "burger", "\u{1F354}" },
    .{ "fries", "\u{1F35F}" },
    .{ "hotdog", "\u{1F32D}" },
    .{ "taco", "\u{1F32E}" },
    .{ "sushi", "\u{1F363}" },
    .{ "ramen", "\u{1F35C}" },
    .{ "cake", "\u{1F370}" },
    .{ "cookie", "\u{1F36A}" },
    .{ "chocolate", "\u{1F36B}" },
    .{ "candy", "\u{1F36C}" },
    .{ "ice_cream", "\u{1F368}" },
    .{ "coffee", "\u{2615}" },
    .{ "tea", "\u{1F375}" },
    .{ "beer", "\u{1F37A}" },
    .{ "wine", "\u{1F377}" },
    .{ "cocktail", "\u{1F378}" },

    // Travel & Places
    .{ "house", "\u{1F3E0}" },
    .{ "office", "\u{1F3E2}" },
    .{ "hospital", "\u{1F3E5}" },
    .{ "school", "\u{1F3EB}" },
    .{ "bank", "\u{1F3E6}" },
    .{ "hotel", "\u{1F3E8}" },
    .{ "car", "\u{1F697}" },
    .{ "bus", "\u{1F68C}" },
    .{ "train", "\u{1F686}" },
    .{ "plane", "\u{2708}" },
    .{ "ship", "\u{1F6A2}" },
    .{ "bike", "\u{1F6B2}" },
    .{ "world", "\u{1F30D}" },
    .{ "earth", "\u{1F30D}" },
    .{ "globe", "\u{1F30E}" },
    .{ "mountain", "\u{26F0}" },
    .{ "beach", "\u{1F3D6}" },
    .{ "camping", "\u{1F3D5}" },

    // Arrows & Directions
    .{ "arrow_up", "\u{2B06}" },
    .{ "arrow_down", "\u{2B07}" },
    .{ "arrow_left", "\u{2B05}" },
    .{ "arrow_right", "\u{27A1}" },
    .{ "arrow_upper_left", "\u{2196}" },
    .{ "arrow_upper_right", "\u{2197}" },
    .{ "arrow_lower_left", "\u{2199}" },
    .{ "arrow_lower_right", "\u{2198}" },
    .{ "arrows_clockwise", "\u{1F503}" },
    .{ "arrows_counterclockwise", "\u{1F504}" },
    .{ "back", "\u{1F519}" },
    .{ "end", "\u{1F51A}" },
    .{ "soon", "\u{1F51C}" },
    .{ "top", "\u{1F51D}" },

    // Numbers
    .{ "zero", "\u{0030}\u{FE0F}\u{20E3}" },
    .{ "one", "\u{0031}\u{FE0F}\u{20E3}" },
    .{ "two", "\u{0032}\u{FE0F}\u{20E3}" },
    .{ "three", "\u{0033}\u{FE0F}\u{20E3}" },
    .{ "four", "\u{0034}\u{FE0F}\u{20E3}" },
    .{ "five", "\u{0035}\u{FE0F}\u{20E3}" },
    .{ "six", "\u{0036}\u{FE0F}\u{20E3}" },
    .{ "seven", "\u{0037}\u{FE0F}\u{20E3}" },
    .{ "eight", "\u{0038}\u{FE0F}\u{20E3}" },
    .{ "nine", "\u{0039}\u{FE0F}\u{20E3}" },
    .{ "ten", "\u{1F51F}" },
    .{ "100", "\u{1F4AF}" },
    .{ "1234", "\u{1F522}" },

    // Misc
    .{ "plus", "\u{2795}" },
    .{ "minus", "\u{2796}" },
    .{ "multiply", "\u{2716}" },
    .{ "divide", "\u{2797}" },
    .{ "infinity", "\u{267E}" },
    .{ "hash", "\u{0023}\u{FE0F}\u{20E3}" },
    .{ "asterisk", "\u{002A}\u{FE0F}\u{20E3}" },
    .{ "copyright", "\u{00A9}" },
    .{ "registered", "\u{00AE}" },
    .{ "tm", "\u{2122}" },
});

pub fn get(name: []const u8) ?[]const u8 {
    return emoji_map.get(name);
}

pub fn replaceShortcodes(text: []const u8, allocator: std.mem.Allocator) ![]u8 {
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;

    while (i < text.len) {
        if (text[i] == ':') {
            if (findShortcodeEnd(text, i)) |end| {
                const shortcode = text[i + 1 .. end];
                if (get(shortcode)) |emoji| {
                    try result.appendSlice(allocator, emoji);
                    i = end + 1;
                    continue;
                }
            }
        }
        try result.append(allocator, text[i]);
        i += 1;
    }

    return result.toOwnedSlice(allocator);
}

fn findShortcodeEnd(text: []const u8, start: usize) ?usize {
    var end = start + 1;
    while (end < text.len) : (end += 1) {
        switch (text[end]) {
            ':', ' ', '\n' => break,
            else => {},
        }
    }
    if (end < text.len and text[end] == ':' and end > start + 1) {
        return end;
    }
    return null;
}

test "emoji.get known emoji" {
    const smile = get("smile");
    try std.testing.expect(smile != null);
    try std.testing.expectEqualStrings("\u{1F604}", smile.?);
}

test "emoji.get unknown emoji" {
    const unknown = get("not_an_emoji");
    try std.testing.expect(unknown == null);
}

test "emoji.get common emoji" {
    try std.testing.expect(get("check") != null);
    try std.testing.expect(get("x") != null);
    try std.testing.expect(get("rocket") != null);
    try std.testing.expect(get("fire") != null);
    try std.testing.expect(get("heart") != null);
    try std.testing.expect(get("star") != null);
    try std.testing.expect(get("thumbsup") != null);
    try std.testing.expect(get("warning") != null);
}

test "emoji.replaceShortcodes simple" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes("Hello :smile: world", allocator);
    defer allocator.free(result);

    try std.testing.expect(std.mem.indexOf(u8, result, "\u{1F604}") != null);
    try std.testing.expect(std.mem.indexOf(u8, result, "Hello ") != null);
    try std.testing.expect(std.mem.indexOf(u8, result, " world") != null);
}

test "emoji.replaceShortcodes multiple" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes(":rocket: Launch :fire:", allocator);
    defer allocator.free(result);

    try std.testing.expect(std.mem.indexOf(u8, result, "\u{1F680}") != null);
    try std.testing.expect(std.mem.indexOf(u8, result, "\u{1F525}") != null);
}

test "emoji.replaceShortcodes no replacement" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes("No emoji here", allocator);
    defer allocator.free(result);

    try std.testing.expectEqualStrings("No emoji here", result);
}

test "emoji.replaceShortcodes unknown shortcode" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes("Hello :unknown_emoji: world", allocator);
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Hello :unknown_emoji: world", result);
}

test "emoji.replaceShortcodes incomplete shortcode" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes("Hello :smile world", allocator);
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Hello :smile world", result);
}

test "emoji.replaceShortcodes empty shortcode" {
    const allocator = std.testing.allocator;
    const result = try replaceShortcodes("Hello :: world", allocator);
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Hello :: world", result);
}
