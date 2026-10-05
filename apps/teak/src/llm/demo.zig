//! Demo mode: a scripted multi-round conversation so the whole UI loop runs without an API key.
//!
//! `Demo.next(request_body) HttpResult` inspects the request's `messages` and answers the stage
//! the real model would be at (stateless, so retries are safe). For the first designer message:
//!   0. thinking + text + `kerf_apply {"op":"set","path":"doc","value":<truss-bearing-cmu doc>}`
//!   1. text + `kerf_render {"view":"A"}`
//!   2. text + `kerf_inspect {"q":"summary"}`
//!   3. final short report (`end_turn`)
//! Later designer messages get one canned `end_turn` reply explaining demo mode.
//! The doc comes from the embedded spec/details/truss-bearing-cmu.kerf.json.
//! Use `Demo.transport()` with `driver.run`, or call `Demo.next` from your own effect runner.

const std = @import("std");
const types = @import("types.zig");
const jsonw = @import("jsonw.zig");
const js = @import("jsonspan.zig");
const driver = @import("driver.zig");

const doc_min = jsonw.minifyComptime(@embedFile("sample_truss_json"));

fn q(comptime s: []const u8) []const u8 {
    comptime {
        var out: []const u8 = "\"";
        for (s) |c| {
            out = out ++ switch (c) {
                '"' => "\\\"",
                '\\' => "\\\\",
                '\n' => "\\n",
                else => &[_]u8{c},
            };
        }
        return out ++ "\"";
    }
}

fn head(comptime n: []const u8) []const u8 {
    return "{\"id\":\"msg_demo_" ++ n ++ "\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"kerf-demo\",\"content\":[";
}

const usage_tool = "],\"stop_reason\":\"tool_use\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":6200,\"output_tokens\":900,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":5800}}";
const usage_end = "],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":300,\"output_tokens\":210,\"cache_read_input_tokens\":12000,\"cache_creation_input_tokens\":0}}";

fn textBlock(comptime s: []const u8) []const u8 {
    return "{\"type\":\"text\",\"text\":" ++ q(s) ++ "}";
}

const stage0 = head("1") ++
    "{\"type\":\"thinking\",\"thinking\":" ++ q("Prefab truss on an 8-inch CMU wall. Build the whole document in one set op, then render and verify.") ++ ",\"signature\":\"RGVtbyBzaWduYXR1cmUgKG5vdCByZWFsKQ==\"}," ++
    textBlock("Building a prefab truss bearing on an 8\" reinforced CMU wall (exterior on the left). Assumptions: 4:12 pitch, 2x4 chords at 24\" o.c., 2x8 PT sill with a 5/8\" J-bolt, one grouted bond beam at the top course, vertical #5 at 32\" o.c.") ++
    ",{\"type\":\"tool_use\",\"id\":\"toolu_demo_apply\",\"name\":\"kerf_apply\",\"input\":{\"ops\":[{\"op\":\"set\",\"path\":\"doc\",\"value\":" ++ doc_min ++ "}],\"why\":" ++ q("Build truss bearing at CMU wall detail") ++ "}}" ++
    usage_tool;

const stage1 = head("2") ++
    textBlock("Document applied. Rendering section A to check proportions and note placement.") ++
    ",{\"type\":\"tool_use\",\"id\":\"toolu_demo_render\",\"name\":\"kerf_render\",\"input\":{\"view\":\"A\"}}" ++
    usage_tool;

const stage2 = head("3") ++
    textBlock("Section A reads correctly: truss heel bears on the sill, J-bolt embeds in the grouted bond beam, hurricane tie and bird block present. Checking the component table.") ++
    ",{\"type\":\"tool_use\",\"id\":\"toolu_demo_inspect\",\"name\":\"kerf_inspect\",\"input\":{\"q\":\"summary\"}}" ++
    usage_tool;

const stage3 = head("4") ++
    textBlock("Built PREFAB TRUSS BEARING AT CMU WALL: section A and an iso view B, 14 components, no errors.\n\nAssumptions: 4:12 pitch, 18\" overhang with plumb tail, trusses @ 24\" O.C., 8\" CMU with one grouted bond beam, #5 vertical @ 32\" O.C.\n\nOpen question: is the uplift connector sized by the truss manufacturer? Hardware capacities come from the manufacturer and the engineer of record.\n\nCitations to verify: IRC R802.10 (wood trusses) and R905.1.1 (underlayment). All are marked suggested.") ++
    usage_end;

const canned = head("x") ++
    textBlock("DEMO MODE: replies are scripted, so I cannot act on new requests. Enter an API key to talk to Claude.") ++
    usage_end;

const responses = [_][]const u8{ stage0, stage1, stage2, stage3 };

pub const Demo = struct {
    calls: u32 = 0,

    /// Scripted reply for this request body. Pure and stateless.
    pub fn next(request_body: []const u8) types.HttpResult {
        const st = stageOf(request_body);
        if (st.real_users > 1) {
            return .{ .status = 200, .body = canned };
        }
        if (st.assistants_after >= responses.len) return .{ .status = 200, .body = canned };
        return .{ .status = 200, .body = responses[st.assistants_after] };
    }

    pub fn transport(self: *Demo) driver.Transport {
        return .{ .ctx = self, .perform_fn = struct {
            fn f(ctx: *anyopaque, spec: types.HttpRequestSpec) types.HttpResult {
                const d: *Demo = @ptrCast(@alignCast(ctx));
                d.calls += 1;
                return next(spec.body_json);
            }
        }.f };
    }
};

const Stage = struct { real_users: u32, assistants_after: u32 };

fn stageOf(body: []const u8) Stage {
    var out: Stage = .{ .real_users = 0, .assistants_after = 0 };
    var fba_buf: [256]u8 = undefined;
    var fba = std.heap.FixedBufferAllocator.init(&fba_buf);
    const messages = (js.get(body, "messages") catch return out) orelse return out;
    var it = js.ArrIter.init(messages) catch return out;
    while (it.next() catch null) |m| {
        fba.reset();
        const role = (js.getString(fba.allocator(), m, "role") catch null) orelse continue;
        if (std.mem.eql(u8, role, "assistant")) {
            out.assistants_after += 1;
        } else if (isRealUser(m)) {
            out.real_users += 1;
            out.assistants_after = 0;
        }
    }
    return out;
}

/// A designer message has a text or image block; tool_result messages do not.
fn isRealUser(msg: []const u8) bool {
    const content = (js.get(msg, "content") catch return false) orelse return false;
    var it = js.ArrIter.init(content) catch return false;
    var fba_buf: [64]u8 = undefined;
    var fba = std.heap.FixedBufferAllocator.init(&fba_buf);
    while (it.next() catch null) |blk| {
        fba.reset();
        const t = (js.getString(fba.allocator(), blk, "type") catch null) orelse continue;
        if (std.mem.eql(u8, t, "text") or std.mem.eql(u8, t, "image")) return true;
    }
    return false;
}
