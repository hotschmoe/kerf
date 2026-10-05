//! Reference documents embedded for tests (provided by build.zig as anonymous imports).
pub const truss = @embedFile("doc_truss");
pub const slab = @embedFile("doc_slab");
pub const beam = @embedFile("doc_beam");
pub const all = [_][]const u8{ truss, slab, beam };
/// v0.1.2 catalog additions exercised together (tests/docs/palmer-sd1-like.kerf.json); not in `all` (no goldens).
pub const palmer = @embedFile("doc_palmer");
