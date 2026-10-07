//! File-system helpers that work around defects of the Zig 0.16 standard library on Windows.

const std = @import("std");
const builtin = @import("builtin");
const Io = std.Io;

/// Open `sub_path` (relative to `dir`) read-only WITHOUT following a symlink / reparse point at the last component, and
/// return a handle that can be read with `File.reader`.
///
/// Why this exists: on Windows `Dir.openFile(.{ .follow_symlinks = false })` opens the handle for ASYNCHRONOUS I/O
/// (`FILE_OPEN_REPARSE_POINT` without `FILE_SYNCHRONOUS_IO_NONALERT`) but returns a `File` that claims to be blocking.
/// The first `NtReadFile` that does not complete at once returns STATUS_PENDING, which std treats as
/// `unreachable // wrong File nonblocking flag`: the server died with "reached unreachable code" on the first document read
/// (v0.1.0-alpha.6 and .7; alpha.5 read documents with the default, following open). Here the handle is synchronous.
/// A symlink then shows up as `stat().kind == .sym_link` and the callers refuse it, as on POSIX (O_NOFOLLOW).
pub fn openFileNoFollow(io: Io, dir: Io.Dir, sub_path: []const u8) Io.File.OpenError!Io.File {
    if (builtin.os.tag != .windows) return dir.openFile(io, sub_path, .{ .follow_symlinks = false });
    const w = std.os.windows;
    const path_w = try Io.Threaded.sliceToPrefixedFileW(dir.handle, sub_path, .{});
    const name_w = path_w.span();
    const root: ?w.HANDLE = if (Io.Dir.path.isAbsoluteWindowsWtf16(name_w)) null else dir.handle;
    var attempt: u5 = 0;
    while (true) {
        try io.checkCancel();
        var handle: w.HANDLE = undefined;
        var iosb: w.IO_STATUS_BLOCK = undefined;
        const status = w.ntdll.NtCreateFile(
            &handle,
            .{ .STANDARD = .{ .SYNCHRONIZE = true }, .GENERIC = .{ .READ = true } },
            &.{ .RootDirectory = root, .ObjectName = @constCast(&w.UNICODE_STRING.init(name_w)) },
            &iosb,
            null,
            .{ .NORMAL = true },
            .VALID_FLAGS,
            .OPEN,
            .{ .IO = .SYNCHRONOUS_NONALERT, .NON_DIRECTORY_FILE = true, .OPEN_REPARSE_POINT = true },
            null,
            0,
        );
        switch (status) {
            .SUCCESS => return .{ .handle = handle, .flags = .{ .nonblocking = false } },
            .OBJECT_NAME_INVALID, .OBJECT_PATH_SYNTAX_BAD => return error.BadPathName,
            .OBJECT_NAME_NOT_FOUND, .OBJECT_PATH_NOT_FOUND => return error.FileNotFound,
            .BAD_NETWORK_PATH, .BAD_NETWORK_NAME => return error.NetworkNotFound,
            .NO_MEDIA_IN_DEVICE, .PIPE_NOT_AVAILABLE => return error.NoDevice,
            .ACCESS_DENIED, .USER_MAPPED_FILE => return error.AccessDenied,
            .PIPE_BUSY => return error.PipeBusy,
            .FILE_IS_A_DIRECTORY => return error.IsDir,
            .NOT_A_DIRECTORY => return error.NotDir,
            .VIRUS_INFECTED, .VIRUS_DELETED => return error.AntivirusInterference,
            .SHARING_VIOLATION, .DELETE_PENDING => { // an editor or antivirus has it for a moment: retry briefly
                if (attempt >= 8) return error.FileBusy;
                io.sleep(Io.Duration.fromMilliseconds((@as(u32, 1) << attempt) >> 1), .awake) catch return error.Canceled;
                attempt += 1;
            },
            else => return w.unexpectedStatus(status),
        }
    }
}
