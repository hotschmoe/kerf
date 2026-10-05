const std = @import("std");
pub fn cliMain(gpa: std.mem.Allocator, io: std.Io, args: []const []const u8, err: *std.Io.Writer, environ: *const std.process.Environ.Map) !u8 {
    _ = gpa; _ = io; _ = args; _ = environ;
    try err.writeAll("kerf serve: not yet\n");
    return 2;
}
