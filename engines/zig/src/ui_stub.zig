//! Default `ui_assets` module: no embedded web UI. `zig build -Dui=<dist dir>` replaces it with a
//! generated module that @embedFile's every file of the dist directory.
pub const File = struct { path: []const u8, data: []const u8 };
pub const files: []const File = &.{};
