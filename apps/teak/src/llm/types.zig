//! Shared plain data types of the harness: configuration, HTTP effect description, tool call and
//! tool result shapes, usage, error kinds. No behaviour except small helpers.

const std = @import("std");
const jsonw = @import("jsonw.zig");

pub const system_md = @embedFile("spec_system_md");
pub const tools_json_raw = @embedFile("spec_tools_json");

pub const api_url = "https://api.anthropic.com/v1/messages";
pub const api_version = "2023-06-01";
pub const beta_fallbacks = "server-side-fallback-2026-07-01";
pub const max_tool_rounds: u32 = 25;
/// Designer images must have their long side at or below this (HARNESS.md).
pub const max_image_long_side: u32 = 1568;

pub const Effort = enum {
    low,
    medium,
    high,
    xhigh,
    max,

    pub fn name(self: Effort) []const u8 {
        return @tagName(self);
    }
};

/// The two models in the picker.
pub const Model = enum {
    opus_5_5,
    sonnet_5_5,

    pub fn id(self: Model) []const u8 {
        return switch (self) {
            .opus_5_5 => "claude-opus-5-5",
            .sonnet_5_5 => "claude-sonnet-5-5",
        };
    }

    pub fn label(self: Model) []const u8 {
        return switch (self) {
            .opus_5_5 => "OPUS 5.5",
            .sonnet_5_5 => "SONNET 5.5",
        };
    }
};

pub const Config = struct {
    /// Designer's own key (BYO). Empty => `Session` refuses to send (`.error{.no_key}`).
    api_key: []const u8 = "",
    /// Model id string (see `Model.id`).
    model: []const u8 = "claude-opus-5-5",
    max_tokens: u32 = 32000,
    effort: Effort = .high,
    /// FULL system text. Build with `buildSystemText` (system.md + component catalog).
    system: []const u8 = system_md,
    /// tools JSON array text; default is the embedded spec/llm/tools.json (minified at request time).
    tools_json: []const u8 = tools_json_raw,
    /// Ask for `fallbacks: "default"` + beta header. Session turns this off for good (per session)
    /// if the API rejects it.
    fallbacks: bool = true,
    /// Send `anthropic-dangerous-direct-browser-access: true` (HARNESS: required for browser calls;
    /// harmless natively).
    browser_header: bool = true,
    url: []const u8 = api_url,
    max_rounds: u32 = max_tool_rounds,
};

/// system.md + "\n\n# Component catalog\n" + catalog markdown (engine `catalog --markdown`).
pub fn buildSystemText(a: std.mem.Allocator, catalog_markdown: []const u8) std.mem.Allocator.Error![]u8 {
    return std.fmt.allocPrint(a, "{s}\n\n# Component catalog\n{s}", .{ std.mem.trimEnd(u8, system_md, "\n"), catalog_markdown });
}

pub const MediaType = enum {
    png,
    jpeg,

    pub fn mime(self: MediaType) []const u8 {
        return switch (self) {
            .png => "image/png",
            .jpeg => "image/jpeg",
        };
    }
};

/// A designer image. `data` is raw file bytes, or already-base64 text if `is_base64`.
/// If the app downscaled it, set `orig_width/orig_height` so Claude is told (a text note is added).
pub const Image = struct {
    media_type: MediaType = .png,
    data: []const u8,
    is_base64: bool = false,
    width: u32 = 0,
    height: u32 = 0,
    orig_width: u32 = 0,
    orig_height: u32 = 0,
};

pub const Header = struct { name: []const u8, value: []const u8 };

/// A POST the host must perform. Memory is owned by the Session and valid until the next call on
/// it; a TEA host should copy it into its effect. Suggested client timeouts: connect 15 s,
/// total 300 s (non-streaming responses with thinking can take minutes). Do not follow redirects.
pub const HttpRequestSpec = struct {
    url: []const u8,
    headers: []const Header,
    body_json: []const u8,
};

/// What the host reports back. `status` 0 + `err` set means a transport failure (DNS, offline,
/// CORS, timeout): no HTTP response was received.
pub const HttpResult = struct {
    status: u16 = 0,
    body: []const u8 = "",
    err: ?[]const u8 = null,
};

pub const ToolUse = struct {
    id: []const u8,
    name: []const u8,
    /// Raw JSON object text of the tool input, exactly as sent by the API.
    input_json: []const u8,
};

pub const ContentBlock = union(enum) {
    text: []const u8,
    /// Base64 text of a PNG (not raw bytes).
    image_png_b64: []const u8,
};

pub const ToolResult = struct {
    /// tool_use id this answers.
    id: []const u8,
    is_error: bool = false,
    content: []const ContentBlock,
    /// Diagnostics counts for the console activity line (kerf_apply).
    n_err: u32 = 0,
    n_warn: u32 = 0,
};

pub const Usage = struct {
    input_tokens: u64 = 0,
    output_tokens: u64 = 0,
    cache_read_input_tokens: u64 = 0,
    cache_creation_input_tokens: u64 = 0,

    pub fn add(self: *Usage, o: Usage) void {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_read_input_tokens += o.cache_read_input_tokens;
        self.cache_creation_input_tokens += o.cache_creation_input_tokens;
    }
};

pub const ErrorKind = enum {
    /// No API key configured.
    no_key,
    /// HTTP 401.
    invalid_api_key,
    /// 429 after the 2 s/4 s/8 s retries were exhausted.
    rate_limited,
    /// 529 after retries were exhausted.
    overloaded,
    /// Any other 4xx/5xx; message is the API's error message verbatim.
    api_error,
    /// Transport failure (HttpResult.err).
    network,
    /// 200 but the body was not a usable message.
    bad_response,
    /// 25 tool rounds reached in one designer turn.
    round_limit,
    /// stop_reason max_tokens (no tools were run) or other abnormal stop.
    truncated,
    /// A call arrived in a state that cannot accept it (e.g. userSubmit while busy).
    busy,
    /// Empty submit.
    empty_input,
};

pub const ErrInfo = struct {
    kind: ErrorKind,
    message: []const u8,
    /// HTTP status when relevant, else 0.
    status: u16 = 0,
};

/// Backoff schedule for 429/529: attempt index -> delay. `null` once exhausted.
pub fn backoffMs(attempt: u32) ?u32 {
    return switch (attempt) {
        0 => 2000,
        1 => 4000,
        2 => 8000,
        else => null,
    };
}

test "system text" {
    const a = std.testing.allocator;
    const s = try buildSystemText(a, "## cmu_wall\n...");
    defer a.free(s);
    try std.testing.expect(std.mem.indexOf(u8, s, "# Component catalog\n## cmu_wall") != null);
    try std.testing.expect(std.mem.startsWith(u8, s, "You are the drafting engine"));
}
