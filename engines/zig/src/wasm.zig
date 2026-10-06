//! wasm32-freestanding entry points: the raw ABI of SPEC 13.1 (no imports, no wasm-bindgen).
//!
//!   memory
//!   kerf_alloc(len) -> ptr        kerf_free(ptr, len)
//!   kerf_call(fn_ptr, fn_len, in_ptr, in_len) -> 0 ok | 1 error (output is error JSON)
//!   kerf_out_ptr() / kerf_out_len()   (valid until the next call)

const std = @import("std");
const kerf = @import("kerf");

/// `no_panic` is `@trap()` on every safety failure and builds in every optimize mode; `simple_panic` pulls in the stderr
/// machinery (`std.Io.Threaded`) that does not compile for wasm32-freestanding once safety checks are on (REVIEW SAF-2).
pub const panic = std.debug.no_panic;

const gpa = std.heap.wasm_allocator;

var out_ptr: [*]u8 = undefined;
var out_len: usize = 0;
var out_live: bool = false;

fn setOut(bytes: []u8) void {
    releaseOut();
    out_ptr = bytes.ptr;
    out_len = bytes.len;
    out_live = true;
}

fn releaseOut() void {
    if (out_live) {
        gpa.free(out_ptr[0..out_len]);
        out_live = false;
        out_len = 0;
    }
}

pub export fn kerf_alloc(len: u32) u32 {
    const mem = gpa.alloc(u8, @max(len, 1)) catch return 0;
    return @intCast(@intFromPtr(mem.ptr));
}

pub export fn kerf_free(ptr: u32, len: u32) void {
    if (ptr == 0) return;
    const p: [*]u8 = @ptrFromInt(ptr);
    gpa.free(p[0..@max(len, 1)]);
}

pub export fn kerf_call(fn_ptr: u32, fn_len: u32, in_ptr: u32, in_len: u32) i32 {
    const name_p: [*]const u8 = @ptrFromInt(fn_ptr);
    const in_p: [*]const u8 = @ptrFromInt(in_ptr);
    const name = name_p[0..fn_len];
    const input = in_p[0..in_len];
    const r = kerf.call(gpa, name, input) catch {
        const msg = gpa.dupe(u8, "{\"error\":{\"code\":\"E_OOM\",\"message\":\"out of memory\"}}") catch return 1;
        setOut(msg);
        return 1;
    };
    setOut(r.bytes);
    return if (r.ok) 0 else 1;
}

pub export fn kerf_out_ptr() u32 {
    return @intCast(@intFromPtr(out_ptr));
}

pub export fn kerf_out_len() u32 {
    return @intCast(out_len);
}
