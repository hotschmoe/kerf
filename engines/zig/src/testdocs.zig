//! Reference documents embedded for tests (provided by build.zig as anonymous imports).
pub const truss = @embedFile("doc_truss");
pub const slab = @embedFile("doc_slab");
pub const beam = @embedFile("doc_beam");
pub const all = [_][]const u8{ truss, slab, beam };
/// v0.1.2 catalog additions exercised together (tests/docs/palmer-sd1-like.kerf.json); not in `all` (no goldens).
pub const palmer = @embedFile("doc_palmer");
/// A field-test detail (flush PSL beam, 3/4" scale, dense left column + dim/label next to the jack studs).
pub const psl = @embedFile("doc_psl");
/// Every embedded document with all of its views checked for layout quality.
pub const layout_docs = [_][]const u8{ truss, slab, beam, psl, palmer };
