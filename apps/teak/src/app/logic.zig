//! Pure app logic: everything below `teak` (engine boundary, document digest,
//! sessions). Tested without any UI framework.
test {
    _ = @import("engine.zig");
    _ = @import("docinfo.zig");
    _ = @import("session.zig");
    _ = @import("units.zig");
    _ = @import("cam.zig");
    _ = @import("editor.zig");
}
