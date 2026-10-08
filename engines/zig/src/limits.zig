//! Global resource limits (REVIEW SAF-5b). Every dimension of a document used to have a local cap at best
//! (`array.count <= 500`, `place.count <= 200`) and no global one, so the caps multiplied: a 2.5 KB document could
//! cost 20 s and 1.8 GB. These are checked where the data enters (api input, compile, view annotations, builders)
//! and reported as `E_LIMIT` diagnostics that name the number found, the maximum and what to do about it.
//!
//! The constants are public so the server and tests can quote them; they are deliberately generous for real
//! construction details (the reference documents use < 20 components and < 20 annotations per view).

const std = @import("std");

/// Largest accepted `kerf.call` input (the document plus options), bytes.
pub const max_json_bytes: usize = 8 * 1024 * 1024;
/// Components in one document.
pub const max_components: usize = 2000;
/// Instances over all components (`array.count` and `place.count` multiply otherwise).
pub const max_instances_total: usize = 5000;
/// Annotations (notes, dims, labels) in one view.
pub const max_annotations_per_view: usize = 150;
/// Views in one document.
pub const max_views: usize = 64;
/// Vertices of one polygon / points of one path / cites of one note.
pub const max_points: usize = 5000;
/// Absolute value of any length or coordinate read from a document, inches (about 16 miles).
pub const max_coord_in: f64 = 1.0e6;
/// Characters in one note / label text.
pub const max_text_chars: usize = 2000;

/// The standard E_LIMIT sentence: what, how many, the maximum, and how to get under it.
pub fn message(a: std.mem.Allocator, what: []const u8, got: usize, max: usize, advice: []const u8) std.mem.Allocator.Error![]u8 {
    return a.print("{s}: {d} found, the maximum is {d}. {s}", .{ what, got, max, advice });
}
