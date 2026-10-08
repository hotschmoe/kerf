//! File-system helpers for what the Zig standard library does not do the way Kerf needs on Windows.
//!
//! (Zig 0.16's `Dir.openFile(.{ .follow_symlinks = false })` returned an asynchronous handle on Windows and crashed the server
//! on the first document read; 0.17 opens it synchronously, so the former `openFileNoFollow` workaround is gone. The
//! end-to-end regression test is `tests/windows_serve.mjs`, run on a Windows runner by `.github/workflows/windows.yml`.)

const std = @import("std");
const builtin = @import("builtin");
const Io = std.Io;

/// Far past any real file size: a byte-range lock there excludes other lockers but never blocks anybody's reads or writes
/// of the file's content (Windows range locks are mandatory, and std's own `File.lock` locks byte 0, which would make a
/// concurrent read of the first line of a log fail).
const windows_lock_off: std.os.windows.LARGE_INTEGER = 1 << 62;
const windows_lock_len: std.os.windows.LARGE_INTEGER = 1;

/// Exclusive advisory lock on `f`, held until `f` is closed; blocks until it is granted. `flock` on POSIX (a file system
/// without locks is tolerated), `LockFileEx`-style range lock on Windows (same tolerance).
pub fn lockExclusive(io: Io, f: Io.File) !void {
    if (builtin.os.tag != .windows) {
        f.lock(io, .exclusive) catch |e| switch (e) {
            error.FileLocksUnsupported => {},
            else => return e,
        };
        return;
    }
    const w = std.os.windows;
    while (true) {
        try io.checkCancel();
        var iosb: w.IO_STATUS_BLOCK = undefined;
        switch (w.ntdll.NtLockFile(f.handle, null, null, null, &iosb, &windows_lock_off, &windows_lock_len, null, .FALSE, .TRUE)) {
            .SUCCESS => return,
            .CANCELLED => continue,
            .INSUFFICIENT_RESOURCES => return error.SystemResources,
            else => return, // a file system that cannot lock (network share, FAT): serialised by the server's own mutex only
        }
    }
}
