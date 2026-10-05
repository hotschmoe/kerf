//! Root of the UI-layer module: the TEA app plus the pure-logic layers it uses.
pub const app = @import("app/mod.zig");

test {
    _ = app;
}
