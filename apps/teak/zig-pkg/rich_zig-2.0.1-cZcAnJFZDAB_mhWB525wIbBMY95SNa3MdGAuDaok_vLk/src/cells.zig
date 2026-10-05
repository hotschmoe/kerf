const std = @import("std");

/// Get the display width of a Unicode codepoint.
/// Returns 0 for zero-width/combining characters, 2 for wide characters (CJK, emoji), 1 otherwise.
pub fn getCharacterCellSize(codepoint: u21) u2 {
    // Zero-width characters
    if (codepoint == 0x200B or // Zero-width space
        codepoint == 0x200C or // Zero-width non-joiner
        codepoint == 0x200D or // Zero-width joiner
        codepoint == 0xFEFF or // BOM / Zero-width no-break space
        codepoint == 0x00AD or // Soft hyphen
        codepoint == 0x034F or // Combining grapheme joiner
        codepoint == 0x061C or // Arabic letter mark
        codepoint == 0x2060 or // Word joiner
        codepoint == 0x2061 or // Function application
        codepoint == 0x2062 or // Invisible times
        codepoint == 0x2063 or // Invisible separator
        codepoint == 0x2064 or // Invisible plus
        codepoint == 0x180E) // Mongolian vowel separator
    {
        return 0;
    }

    // Combining characters (diacritical marks)
    if (isCombining(codepoint)) return 0;

    // Control characters
    if (codepoint < 32 or (codepoint >= 0x7F and codepoint < 0xA0)) {
        return 0;
    }

    // Variation selectors
    if (codepoint >= 0xFE00 and codepoint <= 0xFE0F) return 0;
    if (codepoint >= 0xE0100 and codepoint <= 0xE01EF) return 0;

    // Wide characters
    if (isWide(codepoint)) return 2;

    return 1;
}

fn isCombining(cp: u21) bool {
    return (cp >= 0x0300 and cp <= 0x036F) or // Combining Diacritical Marks
        (cp >= 0x0483 and cp <= 0x0489) or // Cyrillic combining marks
        (cp >= 0x0591 and cp <= 0x05BD) or // Hebrew combining marks
        (cp >= 0x05BF and cp <= 0x05BF) or
        (cp >= 0x05C1 and cp <= 0x05C2) or
        (cp >= 0x05C4 and cp <= 0x05C5) or
        (cp >= 0x05C7 and cp <= 0x05C7) or
        (cp >= 0x0610 and cp <= 0x061A) or // Arabic combining marks
        (cp >= 0x064B and cp <= 0x065F) or
        (cp >= 0x0670 and cp <= 0x0670) or
        (cp >= 0x06D6 and cp <= 0x06DC) or
        (cp >= 0x06DF and cp <= 0x06E4) or
        (cp >= 0x06E7 and cp <= 0x06E8) or
        (cp >= 0x06EA and cp <= 0x06ED) or
        (cp >= 0x0711 and cp <= 0x0711) or // Syriac
        (cp >= 0x0730 and cp <= 0x074A) or
        (cp >= 0x07A6 and cp <= 0x07B0) or // Thaana
        (cp >= 0x07EB and cp <= 0x07F3) or // NKo
        (cp >= 0x0816 and cp <= 0x0819) or // Samaritan
        (cp >= 0x081B and cp <= 0x0823) or
        (cp >= 0x0825 and cp <= 0x0827) or
        (cp >= 0x0829 and cp <= 0x082D) or
        (cp >= 0x0859 and cp <= 0x085B) or // Mandaic
        (cp >= 0x08D4 and cp <= 0x08E1) or // Arabic extended
        (cp >= 0x08E3 and cp <= 0x0902) or
        (cp >= 0x093A and cp <= 0x093A) or // Devanagari
        (cp >= 0x093C and cp <= 0x093C) or
        (cp >= 0x0941 and cp <= 0x0948) or
        (cp >= 0x094D and cp <= 0x094D) or
        (cp >= 0x0951 and cp <= 0x0957) or
        (cp >= 0x0962 and cp <= 0x0963) or
        (cp >= 0x0981 and cp <= 0x0981) or // Bengali
        (cp >= 0x09BC and cp <= 0x09BC) or
        (cp >= 0x09C1 and cp <= 0x09C4) or
        (cp >= 0x09CD and cp <= 0x09CD) or
        (cp >= 0x09E2 and cp <= 0x09E3) or
        (cp >= 0x0A01 and cp <= 0x0A02) or // Gurmukhi
        (cp >= 0x0A3C and cp <= 0x0A3C) or
        (cp >= 0x0A41 and cp <= 0x0A42) or
        (cp >= 0x0A47 and cp <= 0x0A48) or
        (cp >= 0x0A4B and cp <= 0x0A4D) or
        (cp >= 0x0A51 and cp <= 0x0A51) or
        (cp >= 0x0A70 and cp <= 0x0A71) or
        (cp >= 0x0A75 and cp <= 0x0A75) or
        (cp >= 0x0A81 and cp <= 0x0A82) or // Gujarati
        (cp >= 0x0ABC and cp <= 0x0ABC) or
        (cp >= 0x0AC1 and cp <= 0x0AC5) or
        (cp >= 0x0AC7 and cp <= 0x0AC8) or
        (cp >= 0x0ACD and cp <= 0x0ACD) or
        (cp >= 0x0AE2 and cp <= 0x0AE3) or
        (cp >= 0x1AB0 and cp <= 0x1AFF) or // Combining Diacritical Marks Extended
        (cp >= 0x1DC0 and cp <= 0x1DFF) or // Combining Diacritical Marks Supplement
        (cp >= 0x20D0 and cp <= 0x20FF) or // Combining Diacritical Marks for Symbols
        (cp >= 0xFE20 and cp <= 0xFE2F); // Combining Half Marks
}

fn isWide(cp: u21) bool {
    // Hangul Jamo
    if (cp >= 0x1100 and cp <= 0x115F) return true;
    if (cp >= 0x2329 and cp <= 0x232A) return true; // Angle brackets

    // CJK Radicals Supplement through Enclosed CJK Letters
    if (cp >= 0x2E80 and cp <= 0x303E) return true;

    // Hiragana, Katakana
    if (cp >= 0x3041 and cp <= 0x3096) return true;
    if (cp >= 0x30A1 and cp <= 0x30FA) return true;

    // CJK Unified Ideographs and related
    if (cp >= 0x3400 and cp <= 0x4DBF) return true; // CJK Extension A
    if (cp >= 0x4E00 and cp <= 0x9FFF) return true; // CJK Unified Ideographs

    // Hangul Syllables
    if (cp >= 0xAC00 and cp <= 0xD7A3) return true;

    // CJK Compatibility Ideographs
    if (cp >= 0xF900 and cp <= 0xFAFF) return true;

    // Vertical Forms
    if (cp >= 0xFE10 and cp <= 0xFE1F) return true;

    // CJK Compatibility Forms
    if (cp >= 0xFE30 and cp <= 0xFE6F) return true;

    // Fullwidth Forms
    if (cp >= 0xFF00 and cp <= 0xFF60) return true;
    if (cp >= 0xFFE0 and cp <= 0xFFE6) return true;

    // CJK Extension B and beyond
    if (cp >= 0x20000 and cp <= 0x2FFFD) return true;
    if (cp >= 0x30000 and cp <= 0x3FFFD) return true;

    // Emoji - wide ranges
    if (cp >= 0x1F300 and cp <= 0x1F9FF) return true; // Misc Symbols and Pictographs through Supplemental Symbols
    if (cp >= 0x1FA00 and cp <= 0x1FAFF) return true; // Extended-A
    if (cp >= 0x231A and cp <= 0x231B) return true; // Watch, Hourglass
    if (cp >= 0x23E9 and cp <= 0x23F3) return true; // Various symbols
    if (cp >= 0x23F8 and cp <= 0x23FA) return true;
    if (cp >= 0x25AA and cp <= 0x25AB) return true; // Small squares
    if (cp >= 0x25B6 and cp <= 0x25B6) return true; // Play button
    if (cp >= 0x25C0 and cp <= 0x25C0) return true; // Reverse play
    if (cp >= 0x25FB and cp <= 0x25FE) return true; // Medium squares
    if (cp >= 0x2600 and cp <= 0x2604) return true; // Weather
    if (cp >= 0x260E and cp <= 0x260E) return true; // Phone
    if (cp >= 0x2611 and cp <= 0x2611) return true; // Checkbox
    if (cp >= 0x2614 and cp <= 0x2615) return true; // Umbrella, coffee
    if (cp >= 0x2618 and cp <= 0x2618) return true; // Clover
    if (cp >= 0x261D and cp <= 0x261D) return true; // Pointing up
    if (cp >= 0x2620 and cp <= 0x2620) return true; // Skull
    if (cp >= 0x2622 and cp <= 0x2623) return true; // Radioactive, biohazard
    if (cp >= 0x2626 and cp <= 0x2626) return true; // Orthodox cross
    if (cp >= 0x262A and cp <= 0x262A) return true; // Star and crescent
    if (cp >= 0x262E and cp <= 0x262F) return true; // Peace, yin yang
    if (cp >= 0x2638 and cp <= 0x263A) return true; // Wheel, smiley
    if (cp >= 0x2640 and cp <= 0x2640) return true; // Female
    if (cp >= 0x2642 and cp <= 0x2642) return true; // Male
    if (cp >= 0x2648 and cp <= 0x2653) return true; // Zodiac
    if (cp >= 0x265F and cp <= 0x2660) return true; // Chess
    if (cp >= 0x2663 and cp <= 0x2663) return true; // Club
    if (cp >= 0x2665 and cp <= 0x2666) return true; // Heart, diamond
    if (cp >= 0x2668 and cp <= 0x2668) return true; // Hot springs
    if (cp >= 0x267B and cp <= 0x267B) return true; // Recycle
    if (cp >= 0x267E and cp <= 0x267F) return true; // Infinity, wheelchair
    if (cp >= 0x2692 and cp <= 0x2697) return true; // Tools, alembic
    if (cp >= 0x2699 and cp <= 0x2699) return true; // Gear
    if (cp >= 0x269B and cp <= 0x269C) return true; // Atom, fleur-de-lis
    if (cp >= 0x26A0 and cp <= 0x26A1) return true; // Warning, high voltage
    if (cp >= 0x26AA and cp <= 0x26AB) return true; // Circles
    if (cp >= 0x26B0 and cp <= 0x26B1) return true; // Coffin, urn
    if (cp >= 0x26BD and cp <= 0x26BE) return true; // Soccer, baseball
    if (cp >= 0x26C4 and cp <= 0x26C5) return true; // Snowman, sun
    if (cp >= 0x26CE and cp <= 0x26CE) return true; // Ophiuchus
    if (cp >= 0x26D4 and cp <= 0x26D4) return true; // No entry
    if (cp >= 0x26EA and cp <= 0x26EA) return true; // Church
    if (cp >= 0x26F2 and cp <= 0x26F3) return true; // Fountain, golf
    if (cp >= 0x26F5 and cp <= 0x26F5) return true; // Sailboat
    if (cp >= 0x26FA and cp <= 0x26FA) return true; // Tent
    if (cp >= 0x26FD and cp <= 0x26FD) return true; // Fuel pump
    if (cp >= 0x2702 and cp <= 0x2702) return true; // Scissors
    if (cp >= 0x2705 and cp <= 0x2705) return true; // Check mark
    if (cp >= 0x2708 and cp <= 0x270D) return true; // Plane through writing hand
    if (cp >= 0x270F and cp <= 0x270F) return true; // Pencil
    if (cp >= 0x2712 and cp <= 0x2712) return true; // Black nib
    if (cp >= 0x2714 and cp <= 0x2714) return true; // Heavy check
    if (cp >= 0x2716 and cp <= 0x2716) return true; // Heavy X
    if (cp >= 0x271D and cp <= 0x271D) return true; // Latin cross
    if (cp >= 0x2721 and cp <= 0x2721) return true; // Star of David
    if (cp >= 0x2728 and cp <= 0x2728) return true; // Sparkles
    if (cp >= 0x2733 and cp <= 0x2734) return true; // Eight spoked asterisk
    if (cp >= 0x2744 and cp <= 0x2744) return true; // Snowflake
    if (cp >= 0x2747 and cp <= 0x2747) return true; // Sparkle
    if (cp >= 0x274C and cp <= 0x274C) return true; // Cross mark
    if (cp >= 0x274E and cp <= 0x274E) return true; // Cross mark in square
    if (cp >= 0x2753 and cp <= 0x2755) return true; // Question marks
    if (cp >= 0x2757 and cp <= 0x2757) return true; // Exclamation
    if (cp >= 0x2763 and cp <= 0x2764) return true; // Heart exclamation, heart
    if (cp >= 0x2795 and cp <= 0x2797) return true; // Plus, minus, divide
    if (cp >= 0x27A1 and cp <= 0x27A1) return true; // Right arrow
    if (cp >= 0x27B0 and cp <= 0x27B0) return true; // Curly loop
    if (cp >= 0x27BF and cp <= 0x27BF) return true; // Double curly loop
    if (cp >= 0x2934 and cp <= 0x2935) return true; // Arrows
    if (cp >= 0x2B05 and cp <= 0x2B07) return true; // Arrows
    if (cp >= 0x2B1B and cp <= 0x2B1C) return true; // Large squares
    if (cp >= 0x2B50 and cp <= 0x2B50) return true; // Star
    if (cp >= 0x2B55 and cp <= 0x2B55) return true; // Heavy circle
    if (cp >= 0x3030 and cp <= 0x3030) return true; // Wavy dash
    if (cp >= 0x303D and cp <= 0x303D) return true; // Part alternation mark
    if (cp >= 0x3297 and cp <= 0x3297) return true; // Circled ideograph congratulation
    if (cp >= 0x3299 and cp <= 0x3299) return true; // Circled ideograph secret

    return false;
}

/// Calculate the cell width of a UTF-8 string
pub fn cellLen(text: []const u8) usize {
    var width: usize = 0;
    var i: usize = 0;

    while (i < text.len) {
        const byte = text[i];
        const cp_len = std.unicode.utf8ByteSequenceLength(byte) catch {
            i += 1;
            continue;
        };

        if (i + cp_len > text.len) break;

        const cp = std.unicode.utf8Decode(text[i..][0..cp_len]) catch {
            i += 1;
            continue;
        };

        width += getCharacterCellSize(cp);
        i += cp_len;
    }

    return width;
}

/// Find the byte index for a given cell position
pub fn cellToByteIndex(text: []const u8, cell_pos: usize) usize {
    var current_cell: usize = 0;
    var i: usize = 0;

    while (i < text.len and current_cell < cell_pos) {
        const byte = text[i];
        const cp_len = std.unicode.utf8ByteSequenceLength(byte) catch {
            i += 1;
            continue;
        };

        if (i + cp_len > text.len) break;

        const cp = std.unicode.utf8Decode(text[i..][0..cp_len]) catch {
            i += 1;
            continue;
        };

        current_cell += getCharacterCellSize(cp);
        i += cp_len;
    }

    return i;
}

/// Truncate text to fit within max_width cells, optionally adding ellipsis
pub fn truncate(text: []const u8, max_width: usize, ellipsis: []const u8) []const u8 {
    const text_width = cellLen(text);
    if (text_width <= max_width) return text;

    const ellipsis_width = cellLen(ellipsis);
    if (max_width <= ellipsis_width) {
        // Not enough room for ellipsis, just truncate
        return text[0..cellToByteIndex(text, max_width)];
    }

    const target_width = max_width - ellipsis_width;
    return text[0..cellToByteIndex(text, target_width)];
}

/// Set the width of text by padding or truncating
pub fn setLen(text: []const u8, width: usize, pad_char: u8, allocator: std.mem.Allocator) ![]u8 {
    const current_width = cellLen(text);

    if (current_width == width) {
        const result = try allocator.alloc(u8, text.len);
        @memcpy(result, text);
        return result;
    }

    if (current_width > width) {
        const truncated = truncate(text, width, "");
        const result = try allocator.alloc(u8, truncated.len);
        @memcpy(result, truncated);
        return result;
    }

    // Pad
    const padding = width - current_width;
    const result = try allocator.alloc(u8, text.len + padding);
    @memcpy(result[0..text.len], text);
    @memset(result[text.len..], pad_char);
    return result;
}

// Tests
test "getCharacterCellSize ASCII" {
    try std.testing.expectEqual(@as(u2, 1), getCharacterCellSize('A'));
    try std.testing.expectEqual(@as(u2, 1), getCharacterCellSize('z'));
    try std.testing.expectEqual(@as(u2, 1), getCharacterCellSize('0'));
    try std.testing.expectEqual(@as(u2, 1), getCharacterCellSize(' '));
    try std.testing.expectEqual(@as(u2, 1), getCharacterCellSize('!'));
}

test "getCharacterCellSize control characters" {
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0));
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize('\n'));
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize('\r'));
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize('\t'));
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x7F)); // DEL
}

test "getCharacterCellSize zero-width" {
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x200B)); // Zero-width space
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x200C)); // ZWNJ
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x200D)); // ZWJ
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0xFEFF)); // BOM
}

test "getCharacterCellSize combining marks" {
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x0300)); // Combining grave
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x0301)); // Combining acute
    try std.testing.expectEqual(@as(u2, 0), getCharacterCellSize(0x0308)); // Combining diaeresis
}

test "getCharacterCellSize CJK wide" {
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x4E00)); // CJK unified ideograph
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x3042)); // Hiragana 'a'
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x30A2)); // Katakana 'a'
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0xAC00)); // Hangul syllable
}

test "getCharacterCellSize emoji wide" {
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x1F600)); // Grinning face
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x1F4A9)); // Pile of poo
    try std.testing.expectEqual(@as(u2, 2), getCharacterCellSize(0x2764)); // Heart
}

test "cellLen ASCII string" {
    try std.testing.expectEqual(@as(usize, 5), cellLen("Hello"));
    try std.testing.expectEqual(@as(usize, 11), cellLen("Hello World"));
    try std.testing.expectEqual(@as(usize, 0), cellLen(""));
}

test "cellLen CJK string" {
    try std.testing.expectEqual(@as(usize, 6), cellLen("\u{4E2D}\u{6587}\u{5B57}")); // 3 CJK chars * 2 width
}

test "cellLen mixed string" {
    // A=1, B=1, CJK=2, C=1, D=1 = 6
    try std.testing.expectEqual(@as(usize, 6), cellLen("AB\u{4E2D}CD"));
}

test "cellLen with combining marks" {
    // 'e' + combining acute = displays as 1 cell
    try std.testing.expectEqual(@as(usize, 1), cellLen("e\u{0301}"));
    // 'a' + combining diaeresis = displays as 1 cell
    try std.testing.expectEqual(@as(usize, 1), cellLen("a\u{0308}"));
}

test "truncate basic" {
    const text = "Hello World";
    try std.testing.expectEqualStrings("Hello", truncate(text, 5, ""));
    try std.testing.expectEqualStrings("Hello World", truncate(text, 20, ""));
    try std.testing.expectEqualStrings("Hello World", truncate(text, 11, ""));
}

test "truncate with ellipsis" {
    const text = "Hello World";
    const truncated = truncate(text, 8, "...");
    // 8 - 3 (ellipsis) = 5 cells, "Hello"
    try std.testing.expectEqualStrings("Hello", truncated);
}

test "cellToByteIndex" {
    try std.testing.expectEqual(@as(usize, 0), cellToByteIndex("Hello", 0));
    try std.testing.expectEqual(@as(usize, 3), cellToByteIndex("Hello", 3));
    try std.testing.expectEqual(@as(usize, 5), cellToByteIndex("Hello", 5));

    // CJK: each char is 3 bytes but 2 cells
    try std.testing.expectEqual(@as(usize, 3), cellToByteIndex("\u{4E2D}\u{6587}", 2)); // First CJK = 2 cells
    try std.testing.expectEqual(@as(usize, 6), cellToByteIndex("\u{4E2D}\u{6587}", 4)); // Both CJK = 4 cells
}

test "setLen padding" {
    const allocator = std.testing.allocator;

    const padded = try setLen("Hi", 5, ' ', allocator);
    defer allocator.free(padded);
    try std.testing.expectEqualStrings("Hi   ", padded);
}

test "setLen truncating" {
    const allocator = std.testing.allocator;

    const truncated = try setLen("Hello World", 5, ' ', allocator);
    defer allocator.free(truncated);
    try std.testing.expectEqualStrings("Hello", truncated);
}

test "setLen exact" {
    const allocator = std.testing.allocator;

    const exact = try setLen("Hello", 5, ' ', allocator);
    defer allocator.free(exact);
    try std.testing.expectEqualStrings("Hello", exact);
}
