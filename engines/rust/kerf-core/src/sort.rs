//! The one stable sort of the engine: exactly Zig's `sort.stable`, comparison for comparison.

use alloc::vec::Vec;

/// Binary insertion sort of `[a, b)`: element i goes right after the last element of `[a, i)` that
/// is not greater than it.
fn insertion(cx: &dyn IdxLess, v: &mut [usize], a: usize, b: usize) {
    let mut i = a + 1;
    while i < b {
        let x = v[i];
        let r = cx.lt(x, v[i - 1]);
        if !r {
            i += 1;
            continue;
        }
        let mut lo = a;
        let mut hi = i - 1;
        while lo < hi {
            let h = lo + (hi - lo) / 2;
            let r = cx.lt(x, v[h]);
            if r {
                hi = h;
            } else {
                lo = h + 1;
            }
        }
        let mut j = i;
        while j > lo {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[lo] = x;
        i += 1;
    }
}

/// The same binary insertion sort, moving elements by swaps (any `T`).
fn insertion_swap<T, F: Fn(&T, &T) -> bool>(v: &mut [T], a: usize, b: usize, less: &F) {
    let mut i = a + 1;
    while i < b {
        let r = less(&v[i], &v[i - 1]);
        if !r {
            i += 1;
            continue;
        }
        let mut lo = a;
        let mut hi = i - 1;
        while lo < hi {
            let h = lo + (hi - lo) / 2;
            let r = less(&v[i], &v[h]);
            if r {
                hi = h;
            } else {
                lo = h + 1;
            }
        }
        let mut j = i;
        while j > lo {
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// Zig's buffered merge loop: left run `[a, m)` into `buf`, merged front to back with `[m, b)`; ties
/// keep the left element first.
fn merge_core(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a: usize, m: usize, b: usize) {
    let nl = m - a;
    let mut t: usize = 0;
    while t < nl {
        buf[t] = v[a + t];
        t += 1;
    }
    let mut i: usize = 0;
    let mut j: usize = m;
    let mut k: usize = a;
    while i < nl && j < b {
        let r = cx.lt(v[j], buf[i]);
        if r {
            v[k] = v[j];
            j += 1;
        } else {
            v[k] = buf[i];
            i += 1;
        }
        k += 1;
    }
    while i < nl {
        v[k] = buf[i];
        i += 1;
        k += 1;
    }
}

/// Zig's `mergeLow`: the left elements not greater than the first right one are already in place;
/// the rest of the left run goes through `buf`.
fn merge_low(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a0: usize, m: usize, b: usize) {
    let mut a = a0;
    let mut hi = m - 1;
    while a < hi {
        let h = a + (hi - a) / 2;
        let r = cx.lt(v[m], v[h]);
        if r {
            hi = h;
        } else {
            a = h + 1;
        }
    }
    merge_core(cx, v, buf, a, m, b);
}

/// Zig's `mergeHigh`: the right elements not less than the last left one are already in place; the
/// rest of the right run goes through `buf`, merged back to front.
fn merge_high(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a: usize, m: usize, b0: usize) {
    let mut lo = m + 1;
    let mut b = b0;
    while lo < b {
        let h = lo + (b - lo) / 2;
        let r = cx.lt(v[h], v[m - 1]);
        if r {
            lo = h + 1;
        } else {
            b = h;
        }
    }
    let nr = b - m;
    let mut t: usize = 0;
    while t < nr {
        buf[t] = v[m + t];
        t += 1;
    }
    let mut i: usize = m;
    let mut j: usize = nr;
    let mut k: usize = b;
    while i > a && j > 0 {
        k -= 1;
        let r = cx.lt(buf[j - 1], v[i - 1]);
        if r {
            i -= 1;
            v[k] = v[i];
        } else {
            j -= 1;
            v[k] = buf[j];
        }
    }
    let mut t: usize = 0;
    while t < j {
        v[a + t] = buf[t];
        t += 1;
    }
}

/// Reverses `[lo, hi)`.
fn reverse<T>(v: &mut [T], lo: usize, hi: usize) {
    let mut i = lo;
    let mut j = hi;
    while i < j && j - i >= 2 {
        v.swap(i, j - 1);
        i += 1;
        j -= 1;
    }
}

/// `[s, m) [m, e)` -> `[m, e) [s, m)`. No comparisons, so any correct rotation matches Zig's block
/// swaps.
fn rotate<T>(v: &mut [T], s: usize, m: usize, e: usize) {
    reverse(v, s, m);
    reverse(v, m, e);
    reverse(v, s, e);
}

/// Zig's `merge` for one pair of runs, skipped when either side is empty.
fn merge_opt(
    cx: &dyn IdxLess,
    v: &mut [usize],
    buf: &mut [usize],
    a: usize,
    m: usize,
    b: usize,
    cap: usize,
) {
    if a < m && m < b {
        merge(cx, v, buf, a, m, b, cap);
    } else {
    }
}

/// SymMerge's split: the first `c` in the searched range whose mirror `mid + m - 1 - c` in the right
/// run is less than it (Zig's binary search, comparison for comparison).
fn sym_split(cx: &dyn IdxLess, v: &[usize], a: usize, m: usize, b: usize) -> usize {
    let mid = a + (b - a) / 2;
    let start0 = if m > mid { m - (b - mid) } else { a };
    let hi0 = if m > mid { mid } else { m };
    let mut start = start0;
    let mut hi = hi0;
    while start < hi {
        let c = start + (hi - start) / 2;
        let pc = mid + (m - 1 - c);
        let r = cx.lt(v[pc], v[c]);
        if !r {
            start = c + 1;
        } else {
            hi = c;
        }
    }
    start
}

/// The rest of SymMerge after the split: rotate, then merge both halves.
fn sym_tail(
    cx: &dyn IdxLess,
    v: &mut [usize],
    buf: &mut [usize],
    a: usize,
    m: usize,
    b: usize,
    start: usize,
    cap: usize,
) {
    let mid = a + (b - a) / 2;
    let end = mid + (m - start);
    if start < m && m < end {
        rotate(v, start, m, end);
    }
    merge_opt(cx, v, buf, a, start, mid, cap);
    merge_opt(cx, v, buf, mid, end, b, cap);
}

/// Zig's `merge`: in order already, or the shorter-than-buffer side through the buffer, or a
/// SymMerge split (Kim and Kutzner) with a rotation and two smaller merges.
fn merge(
    cx: &dyn IdxLess,
    v: &mut [usize],
    buf: &mut [usize],
    a: usize,
    m: usize,
    b: usize,
    cap: usize,
) {
    let r = cx.lt(v[m], v[m - 1]);
    if !r {
        return;
    }
    if m - a <= cap {
        merge_low(cx, v, buf, a, m, b);
        return;
    }
    if b - m <= cap {
        merge_high(cx, v, buf, a, m, b);
        return;
    }
    let start = sym_split(cx, v, a, m, b);
    sym_tail(cx, v, buf, a, m, b, start, cap);
}

pub const BLOCK: usize = 20;

/// Zig's `sortBytes`: binary insertion on blocks of 20, then bottom-up merging; `cap` is how many
/// elements Zig's 4 KiB merge buffer holds.
fn sort_runs(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], cap: usize) {
    let n = v.len();
    let mut a: usize = 0;
    while a < n {
        let b = if n - a > BLOCK { a + BLOCK } else { n };
        insertion(cx, v, a, b);
        a = b;
    }
    let mut width: usize = BLOCK;
    while width < n {
        let mut a: usize = 0;
        while a < n && n - a > width {
            let m = a + width;
            let b = if n - m > width { m + width } else { n };
            merge(cx, v, buf, a, m, b, cap);
            a = b;
        }
        if width >= n - width {
            width = n;
        } else {
            width = width * 2;
        }
    }
}

/// `less(items[x], items[y])`, out-of-range indices last (they never occur: every index is a
/// position of `items`).
fn cmp<E, F: Fn(&E, &E) -> bool>(items: &[E], less: &F, x: usize, y: usize) -> bool {
    if x < items.len() {
        if y < items.len() {
            let r = less(&items[x], &items[y]);
            r
        } else {
            true
        }
    } else {
        false
    }
}

trait IdxLess {
    fn lt(&self, x: usize, y: usize) -> bool;
}

struct IdxCmp<'a, E, F: Fn(&E, &E) -> bool> {
    items: &'a [E],
    less: &'a F,
}

impl<'a, E, F: Fn(&E, &E) -> bool> IdxLess for IdxCmp<'a, E, F> {
    fn lt(&self, x: usize, y: usize) -> bool {
        cmp(self.items, self.less, x, y)
    }
}

/// `items[k] = old items[idx[k]]` for a permutation `idx`, by swaps (any `T`).
fn apply_perm<T>(items: &mut [T], idx: &[usize]) {
    let n = items.len();
    let mut at_v: Vec<usize> = alloc::vec![0usize; n];
    let mut pos_v: Vec<usize> = alloc::vec![0usize; n];
    let at = at_v.as_mut_slice();
    let pos = pos_v.as_mut_slice();
    let mut i: usize = 0;
    while i < n {
        at[i] = i;
        pos[i] = i;
        i += 1;
    }
    let mut k: usize = 0;
    while k < n {
        let e = idx[k];
        let p = pos[e];
        if p != k {
            items.swap(k, p);
            let f = at[k];
            at[p] = f;
            pos[f] = p;
            at[k] = e;
            pos[e] = k;
        }
        k += 1;
    }
}

/// The one stable sort of the engine: exactly Zig's `sort.stable` (binary insertion on blocks of 20,
/// bottom-up merging through a 4 KiB buffer, SymMerge with rotations when both runs are longer),
/// comparison for comparison, so its result equals Zig's for every comparator, not only strict weak
/// orders. Up to 20 elements sort in place; longer slices sort an index array and then move the
/// elements once. Always a permutation and never panics; sorted and stable when
/// `less` is a strict weak ordering.
pub fn stable<T, F: Fn(&T, &T) -> bool>(items: &mut [T], less: F) {
    let n = items.len();
    if n <= BLOCK {
        if n < 2 {
            return;
        }
        insertion_swap(items, 0, n, &less);
        return;
    }
    let size = core::mem::size_of::<T>();
    let cap = if size == 0 { 4096 } else { 4096 / size };
    let mut idx: Vec<usize> = alloc::vec![0usize; n];
    {
        let ix = idx.as_mut_slice();
        let mut i: usize = 0;
        while i < n {
            ix[i] = i;
            i += 1;
        }
    }
    let blen = if cap < n { cap } else { n };
    let mut buf: Vec<usize> = alloc::vec![0usize; blen];
    let ic = IdxCmp {
        items: &*items,
        less: &less,
    };
    sort_runs(&ic, idx.as_mut_slice(), buf.as_mut_slice(), cap);
    apply_perm(items, idx.as_slice());
}
