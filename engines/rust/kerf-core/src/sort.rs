//! The one stable sort of the engine.

use core::cmp::Ordering;

/// Sorts `items` in place, keeping equal elements in input order. `less` must be a strict weak ordering.
pub fn stable<T>(items: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    items.sort_by(|a, b| {
        if less(a, b) {
            Ordering::Less
        } else if less(b, a) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
}
