//! The one stable sort of the engine: binary insertion sort on runs of 20, then bottom-up SymMerge
//! (Kim and Kutzner, as Go's `sort.Stable`). In place, no allocation, any element type, O(n log n)
//! comparisons. Stable, so for any strict weak ordering the result is the unique stable order.

const BLOCK: usize = 20;

/// Sorts `items` in place, keeping equal elements in input order. `less` must be a strict weak ordering.
pub fn stable<T>(items: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    let n = items.len();
    let mut a = 0;
    while a < n {
        insertion(items, a, (a + BLOCK).min(n), &mut less);
        a += BLOCK;
    }
    let mut width = BLOCK;
    while width < n {
        let mut a = 0;
        while a + width < n {
            sym_merge(items, a, a + width, (a + 2 * width).min(n), &mut less);
            a += 2 * width;
        }
        width *= 2;
    }
}

fn insertion<T>(items: &mut [T], a: usize, b: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    for i in a + 1..b {
        if !less(&items[i], &items[i - 1]) {
            continue;
        }
        let mut lo = a;
        let mut hi = i - 1;
        while lo < hi {
            let h = lo + (hi - lo) / 2;
            if less(&items[i], &items[h]) {
                hi = h;
            } else {
                lo = h + 1;
            }
        }
        items[lo..=i].rotate_right(1);
    }
}

fn sym_merge<T>(items: &mut [T], a: usize, m: usize, b: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    if !less(&items[m], &items[m - 1]) {
        return;
    }
    if m - a == 1 {
        let mut i = m;
        let mut j = b;
        while i < j {
            let h = i + (j - i) / 2;
            if less(&items[h], &items[a]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        items[a..i].rotate_left(1);
        return;
    }
    if b - m == 1 {
        let mut i = a;
        let mut j = m;
        while i < j {
            let h = i + (j - i) / 2;
            if !less(&items[m], &items[h]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        items[i..=m].rotate_right(1);
        return;
    }
    let mid = a + (b - a) / 2;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = start + (r - start) / 2;
        if !less(&items[p - c], &items[c]) {
            start = c + 1;
        } else {
            r = c;
        }
    }
    let end = n - start;
    if start < m && m < end {
        items[start..end].rotate_left(m - start);
    }
    if a < start && start < mid {
        sym_merge(items, a, start, mid, less);
    }
    if mid < end && end < b {
        sym_merge(items, mid, end, b, less);
    }
}
