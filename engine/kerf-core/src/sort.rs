use alloc::vec::Vec;

const BLOCK: usize = 20;
const MERGE_BUFFER_BYTES: usize = 4096;

type Less<'a> = &'a dyn Fn(usize, usize) -> bool;

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
        for j in (lo..i).rev() {
            v.swap(j, j + 1);
        }
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

// SymMerge is the in-place merge of Kim and Kutzner.
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
