//! Hard caps on hostile input.

pub const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMPONENTS: usize = 2000;
pub const MAX_INSTANCES_TOTAL: usize = 5000;
pub const MAX_ANNOTATIONS_PER_VIEW: usize = 150;
pub const MAX_VIEWS: usize = 64;
pub const MAX_POINTS: usize = 5000;
pub const MAX_COORD_IN: f64 = 1.0e6;
pub const MAX_TEXT_CHARS: usize = 2000;

/// The standard E_LIMIT sentence: what, how many, the maximum, and how to get under it.
pub fn message(what: &str, got: usize, max: usize, advice: &str) -> alloc::string::String {
    alloc::format!("{what}: {got} found, the maximum is {max}. {advice}")
}
