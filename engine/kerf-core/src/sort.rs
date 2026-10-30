//! The engine's one stable sort. Comparators in geometry code are not always strict weak orders (tolerances), so the
//! algorithm is fixed and total: it never panics, always yields a permutation, and gives the same result for the same
//! comparisons on every target. Std's sort is not used because it may panic on an inconsistent comparator.
//!
//! Binary insertion sort on blocks of [`BLOCK`], then bottom-up merging. A run that fits the merge buffer (4 KiB of
//! elements) is merged through it; longer runs are merged in place with SymMerge (Kim and Kutzner): a split found by
//! binary search, a rotation, and two smaller merges. Slices longer than a block are sorted as an index permutation
//! through one non-generic code path (which keeps the wasm small) and moved into place once.

use alloc::vec::Vec;

/// Elements sorted by insertion before merging starts.
const BLOCK: usize = 20;
const MERGE_BUFFER_BYTES: usize = 4096;

/// `less(x, y)` on positions of the sorted slice.
type Less<'a> = &'a dyn Fn(usize, usize) -> bool;

/// Stable sort by the strict "less than" `less`. Sorted and stable whenever `less` is a strict weak ordering.
pub fn stable<T, F: Fn(&T, &T) -> bool>(items: &mut [T], less: F) {
    let n = items.len();
    if n <= BLOCK {
        insertion_sort(items, &less);
        return;
    }
    let mut order: Vec<usize> = (0..n).collect();
    let cap = MERGE_BUFFER_BYTES / core::mem::size_of::<T>().max(1);
    let mut buf = alloc::vec![0; cap.min(n)];
    {
        let items = &*items;
        sort_runs(&mut order, &mut buf, &|x, y| less(&items[x], &items[y]));
    }
    apply_permutation(items, &order);
}

/// Binary insertion sort: each element goes right after the last element before it that is not greater.
fn insertion_sort<T>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    for i in 1..v.len() {
        if !less(&v[i], &v[i - 1]) {
            continue;
        }
        let (mut lo, mut hi) = (0, i - 1);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if less(&v[i], &v[mid]) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        v[lo..=i].rotate_right(1);
    }
}

fn sort_runs(v: &mut [usize], buf: &mut [usize], less: Less) {
    for block in v.chunks_mut(BLOCK) {
        insertion_sort(block, &|&x, &y| less(x, y));
    }
    let mut width = BLOCK;
    while width < v.len() {
        for run in v.chunks_mut(2 * width) {
            if run.len() > width {
                merge(run, width, buf, less);
            }
        }
        width *= 2;
    }
}

/// Merges the sorted runs `v[..m]` and `v[m..]` (both non-empty).
fn merge(v: &mut [usize], m: usize, buf: &mut [usize], less: Less) {
    if !less(v[m], v[m - 1]) {
        return;
    }
    if m <= buf.len() {
        merge_low(v, m, buf, less);
    } else if v.len() - m <= buf.len() {
        merge_high(v, m, buf, less);
    } else {
        sym_merge(v, m, buf, less);
    }
}

/// The left elements not greater than the first right one are already in place; the rest of the left run goes through
/// `buf` and is merged front to back (ties keep the left element first).
fn merge_low(v: &mut [usize], m: usize, buf: &mut [usize], less: Less) {
    let (mut a, mut hi) = (0, m - 1);
    while a < hi {
        let h = a + (hi - a) / 2;
        if less(v[m], v[h]) {
            hi = h;
        } else {
            a = h + 1;
        }
    }
    let left = m - a;
    buf[..left].copy_from_slice(&v[a..m]);
    let (mut i, mut j, mut k) = (0, m, a);
    while i < left && j < v.len() {
        if less(v[j], buf[i]) {
            v[k] = v[j];
            j += 1;
        } else {
            v[k] = buf[i];
            i += 1;
        }
        k += 1;
    }
    v[k..k + left - i].copy_from_slice(&buf[i..left]);
}

/// The right elements not less than the last left one are already in place; the rest of the right run goes through
/// `buf` and is merged back to front.
fn merge_high(v: &mut [usize], m: usize, buf: &mut [usize], less: Less) {
    let (mut lo, mut end) = (m + 1, v.len());
    while lo < end {
        let h = lo + (end - lo) / 2;
        if less(v[h], v[m - 1]) {
            lo = h + 1;
        } else {
            end = h;
        }
    }
    let right = end - m;
    buf[..right].copy_from_slice(&v[m..end]);
    let (mut i, mut j, mut k) = (m, right, end);
    while i > 0 && j > 0 {
        k -= 1;
        if less(buf[j - 1], v[i - 1]) {
            i -= 1;
            v[k] = v[i];
        } else {
            j -= 1;
            v[k] = buf[j];
        }
    }
    v[..j].copy_from_slice(&buf[..j]);
}

/// SymMerge: split both runs at the point where the left tail and the right head swap places, rotate, and merge the two
/// halves.
fn sym_merge(v: &mut [usize], m: usize, buf: &mut [usize], less: Less) {
    let mid = v.len() / 2;
    let (mut start, mut hi) = if m > mid { (m - (v.len() - mid), mid) } else { (0, m) };
    while start < hi {
        let c = start + (hi - start) / 2;
        if less(v[mid + (m - 1 - c)], v[c]) {
            hi = c;
        } else {
            start = c + 1;
        }
    }
    let end = mid + (m - start);
    if start < m && m < end {
        v[start..end].rotate_left(m - start);
    }
    let (low, high) = v.split_at_mut(mid);
    if 0 < start && start < mid {
        merge(low, start, buf, less);
    }
    if 0 < end - mid && end < mid + high.len() {
        merge(high, end - mid, buf, less);
    }
}

/// `items[k] = old items[order[k]]` for a permutation `order`, by swaps (any `T`).
fn apply_permutation<T>(items: &mut [T], order: &[usize]) {
    let mut at: Vec<usize> = (0..items.len()).collect();
    let mut pos = at.clone();
    for (k, &want) in order.iter().enumerate() {
        let p = pos[want];
        if p != k {
            items.swap(k, p);
            let displaced = at[k];
            (at[p], pos[displaced], at[k], pos[want]) = (displaced, p, want, k);
        }
    }
}
