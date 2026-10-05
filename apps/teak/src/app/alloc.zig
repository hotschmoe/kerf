//! The app-wide allocator. wasm32-freestanding has no libc: use the page-backed
//! wasm allocator there; native links libc (X11 host), so use malloc.

const std = @import("std");
const builtin = @import("builtin");

pub const gpa: std.mem.Allocator = if (builtin.cpu.arch.isWasm())
    std.heap.wasm_allocator
else if (builtin.is_test)
    std.heap.page_allocator
else
    std.heap.c_allocator;
