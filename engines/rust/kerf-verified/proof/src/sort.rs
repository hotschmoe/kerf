use alloc::vec::Vec;
use vstd::prelude::*;

verus! {

/// `lt` is a strict weak ordering: irreflexive, transitive, and incomparability is transitive
/// (stated as negative transitivity). `<` on floats without NaN is one; `<` with NaN is not.
pub open spec fn swo<T>(lt: spec_fn(T, T) -> bool) -> bool {
    &&& forall|a: T| !#[trigger] lt(a, a)
    &&& forall|a: T, b: T, c: T| #[trigger] lt(a, b) && #[trigger] lt(b, c) ==> lt(a, c)
    &&& forall|a: T, b: T, c: T|
        #![trigger lt(a, c), lt(a, b)]
        #![trigger lt(a, c), lt(b, c)]
        lt(a, c) ==> lt(a, b) || lt(b, c)
}

/// What the comparator closure computes, read off its postcondition.
pub open spec fn lt_of<T, F: Fn(&T, &T) -> bool>(less: F) -> spec_fn(T, T) -> bool {
    |a: T, b: T| less.ensures((&a, &b), true)
}

/// The closure is a function of its arguments (no hidden state changes its answer).
pub open spec fn deterministic<T, F: Fn(&T, &T) -> bool>(less: F) -> bool {
    forall|a: T, b: T, r: bool| #[trigger] less.ensures((&a, &b), r) ==> r == less.ensures((&a, &b), true)
}

/// The comparator is one a stable sort is defined for.
pub open spec fn well_behaved<T, F: Fn(&T, &T) -> bool>(less: F) -> bool {
    deterministic(less) && swo(lt_of(less))
}

/// `x` (original position `gx`) comes before `y` (original position `gy`) in a stable sort.
pub open spec fn before<T>(lt: spec_fn(T, T) -> bool, x: T, gx: int, y: T, gy: int) -> bool {
    lt(x, y) || (!lt(y, x) && gx < gy)
}

/// Positions `[a, b)` of `s`, whose elements came from original positions `g`, are in stable
/// sorted order: for k < l, `s[k]` is less than `s[l]`, or equivalent and originally earlier.
pub open spec fn sorted_by<T>(lt: spec_fn(T, T) -> bool, s: Seq<T>, g: Seq<int>, a: int, b: int) -> bool {
    forall|k: int, l: int| a <= k < l < b ==> before(lt, #[trigger] s[k], g[k], #[trigger] s[l], g[l])
}

/// `g` restricted to `[a, b)` is a permutation of `[a, b)`, with inverse `inv`.
pub open spec fn is_perm(g: Seq<int>, inv: Seq<int>, a: int, b: int) -> bool {
    &&& 0 <= a <= b <= g.len() == inv.len()
    &&& forall|k: int| a <= k < b ==> a <= #[trigger] g[k] < b && inv[g[k]] == k
    &&& forall|j: int| a <= j < b ==> a <= #[trigger] inv[j] < b && g[inv[j]] == j
}

/// Positions `[a, b)` of `s` hold `orig[g[k]]`.
pub open spec fn tracks<T>(s: Seq<T>, orig: Seq<T>, g: Seq<int>, a: int, b: int) -> bool {
    forall|k: int| a <= k < b ==> #[trigger] s[k] == orig[g[k]]
}

pub open spec fn ident(n: nat) -> Seq<int> {
    Seq::new(n, |k: int| k)
}

/// The result of sorting `[a, b)` of `old` into `new` by `less`.
pub open spec fn sorts<T, F: Fn(&T, &T) -> bool>(less: F, old: Seq<T>, new: Seq<T>, a: int, b: int) -> bool {
    exists|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, a, b)
            &&& tracks(new, old, g, a, b)
            &&& well_behaved(less) ==> sorted_by(lt_of(less), new, g, a, b)
        }
}

proof fn lemma_before_trans<T>(lt: spec_fn(T, T) -> bool, x: T, gx: int, y: T, gy: int, z: T, gz: int)
    requires
        swo(lt),
        before(lt, x, gx, y, gy),
        before(lt, y, gy, z, gz),
    ensures
        before(lt, x, gx, z, gz),
{
    if lt(x, y) && lt(y, z) {
        assert(lt(x, z));
    } else if lt(x, y) {
        assert(lt(x, z) || lt(z, y));
    } else if lt(y, z) {
        assert(lt(y, x) || lt(x, z));
    } else {
        if lt(z, x) {
            assert(lt(z, y) || lt(y, x));
        }
    }
}

proof fn lemma_mono<T>(lt: spec_fn(T, T) -> bool, x: T, y: T, gy: int, z: T, gz: int)
    requires
        swo(lt),
        before(lt, y, gy, z, gz),
        lt(x, y),
    ensures
        lt(x, z),
{
    if !lt(y, z) {
        assert(lt(x, z) || lt(z, y));
    }
}

proof fn lemma_less<T, F: Fn(&T, &T) -> bool>(less: F, x: T, y: T, r: bool)
    requires
        well_behaved(less),
        less.ensures((&x, &y), r),
    ensures
        r == lt_of(less)(x, y),
{
}

/// Stable sort of `v` by `less`, as the Zig engine's `sort.stable` (whose output it reproduces for
/// every strict weak ordering, since a stable sort's output is unique). Never panics and always
/// permutes, whatever `less` does; sorted and stable whenever `less` is a strict weak ordering.
pub fn stable_sort_by<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], less: F)
    requires
        forall|a: &T, b: &T| #[trigger] less.requires((a, b)),
    ensures
        final(v)@.len() == old(v)@.len(),
        sorts(less, old(v)@, final(v)@, 0, old(v)@.len() as int),
{
    let n = v.len();
    if n < 2 {
        proof {
            let g = ident(n as nat);
            assert(is_perm(g, g, 0, n as int));
        }
        return ;
    }
    let mut buf: Vec<T> = Vec::with_capacity(n / 2);
    let mut t: usize = 0;
    while t < n / 2
        invariant
            t <= n / 2,
            n == v.len(),
            buf@.len() == t,
        decreases n / 2 - t,
    {
        buf.push(v[t]);
        t += 1;
    }
    sort_range(v, &mut buf, 0, n, &less);
}

fn sort_range<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], buf: &mut Vec<T>, a: usize, b: usize, less: &F)
    requires
        a < b <= old(v)@.len(),
        old(buf)@.len() >= old(v)@.len() / 2,
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sorts(*less, old(v)@, final(v)@, a as int, b as int),
    decreases b - a,
{
    if b - a <= 20 {
        insertion(v, a, b, less);
        return ;
    }
    let ghost v0 = v@;
    let m = a + (b - a) / 2;
    sort_range(v, buf, a, m, less);
    let ghost v1 = v@;
    sort_range(v, buf, m, b, less);
    let ghost v2 = v@;
    let ghost (gl, il) = choose|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, a as int, m as int)
            &&& tracks(v1, v0, g, a as int, m as int)
            &&& well_behaved(*less) ==> sorted_by(lt_of(*less), v1, g, a as int, m as int)
        };
    let ghost (gr, ir) = choose|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, m as int, b as int)
            &&& tracks(v2, v1, g, m as int, b as int)
            &&& well_behaved(*less) ==> sorted_by(lt_of(*less), v2, g, m as int, b as int)
        };
    let ghost gc = Seq::new(v0.len(), |k: int| if k < m { gl[k] } else { gr[k] });
    let ghost ic = Seq::new(v0.len(), |k: int| if k < m { il[k] } else { ir[k] });
    proof {
        assert(is_perm(gc, ic, a as int, b as int));
        assert(tracks(v2, v0, gc, a as int, b as int));
        if well_behaved(*less) {
            let lt = lt_of(*less);
            assert forall|k: int, l: int| a <= k < l < m implies before(
                lt,
                #[trigger] v2[k],
                k,
                #[trigger] v2[l],
                l,
            ) by {
                assert(before(lt, v1[k], gl[k], v1[l], gl[l]));
            }
            assert forall|k: int, l: int| m <= k < l < b implies before(
                lt,
                #[trigger] v2[k],
                k,
                #[trigger] v2[l],
                l,
            ) by {
                assert(before(lt, v2[k], gr[k], v2[l], gr[l]));
            }
        }
    }
    merge(v, buf, a, m, b, less);
    let ghost v3 = v@;
    proof {
        let (gm, im) = choose|g: Seq<int>, inv: Seq<int>|
            {
                &&& #[trigger] is_perm(g, inv, a as int, b as int)
                &&& tracks(v3, v2, g, a as int, b as int)
                &&& well_behaved(*less) ==> sorted_by(lt_of(*less), v3, g, a as int, b as int)
            };
        let g = Seq::new(v0.len(), |k: int| if a <= k < b { gc[gm[k]] } else { k });
        let inv = Seq::new(v0.len(), |j: int| if a <= j < b { im[ic[j]] } else { j });
        assert(is_perm(g, inv, a as int, b as int));
        assert(tracks(v3, v0, g, a as int, b as int));
        if well_behaved(*less) {
            let lt = lt_of(*less);
            assert forall|k: int, l: int| a <= k < l < b implies before(
                lt,
                #[trigger] v3[k],
                g[k],
                #[trigger] v3[l],
                g[l],
            ) by {
                assert(before(lt, v3[k], gm[k], v3[l], gm[l]));
                if !lt(v3[k], v3[l]) {
                    let (p, q) = (gm[k], gm[l]);
                    if p < m && q < m {
                        assert(before(lt, v1[p], gl[p], v1[q], gl[q]));
                    } else if p >= m && q >= m {
                        assert(before(lt, v2[p], gr[p], v2[q], gr[q]));
                    }
                }
            }
        }
        assert(sorts(*less, v0, v3, a as int, b as int));
    }
}

/// Binary insertion sort of `[a, b)`: element i goes right after the last element of `[a, i)` that
/// is not greater than it.
fn insertion<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], a: usize, b: usize, less: &F)
    requires
        a < b <= old(v)@.len(),
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sorts(*less, old(v)@, final(v)@, a as int, b as int),
{
    let ghost v0 = v@;
    let ghost ok = well_behaved(*less);
    let ghost lt = lt_of(*less);
    let ghost mut g = ident(v@.len());
    let ghost mut inv = ident(v@.len());
    let mut i = a + 1;
    while i < b
        invariant
            a < i <= b <= v@.len(),
            v@.len() == v0.len() == g.len(),
            ok == well_behaved(*less),
            lt == lt_of(*less),
            forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
            forall|k: int| 0 <= k < a || i <= k < v@.len() ==> #[trigger] v@[k] == v0[k],
            forall|k: int| i <= k < v@.len() ==> #[trigger] g[k] == k,
            forall|k: int| i <= k < v@.len() ==> #[trigger] inv[k] == k,
            is_perm(g, inv, a as int, i as int),
            tracks(v@, v0, g, a as int, i as int),
            ok ==> sorted_by(lt, v@, g, a as int, i as int),
        decreases b - i,
    {
        let x = v[i];
        let r = less(&x, &v[i - 1]);
        if !r {
            proof {
                assert(g[i as int] == i && inv[i as int] == i);
                assert(is_perm(g, inv, a as int, i + 1));
                if ok {
                    lemma_less(*less, x, v@[i - 1], r);
                    assert forall|k: int, l: int| a <= k < l < i + 1 implies before(
                        lt,
                        #[trigger] v@[k],
                        g[k],
                        #[trigger] v@[l],
                        g[l],
                    ) by {
                        if l == i && k < i - 1 {
                            lemma_before_trans(lt, v@[k], g[k], v@[i - 1], g[i - 1], v@[l], g[l]);
                        }
                    }
                }
            }
            i += 1;
            continue;
        }
        let mut lo = a;
        let mut hi = i - 1;
        while lo < hi
            invariant
                a <= lo <= hi < i < v@.len(),
                x == v@[i as int],
                v@.len() == g.len(),
                ok == well_behaved(*less),
                lt == lt_of(*less),
                forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
                ok ==> sorted_by(lt, v@, g, a as int, i as int),
                ok ==> forall|k: int| a <= k < lo ==> !lt(x, #[trigger] v@[k]),
                ok ==> forall|k: int| hi <= k < i ==> lt(x, #[trigger] v@[k]),
            decreases hi - lo,
        {
            let h = lo + (hi - lo) / 2;
            let r = less(&x, &v[h]);
            if r {
                proof {
                    if ok {
                        lemma_less(*less, x, v@[h as int], r);
                        assert forall|k: int| h <= k < i implies lt(x, #[trigger] v@[k]) by {
                            if k > h {
                                lemma_mono(lt, x, v@[h as int], g[h as int], v@[k], g[k]);
                            }
                        }
                    }
                }
                hi = h;
            } else {
                proof {
                    if ok {
                        lemma_less(*less, x, v@[h as int], r);
                        assert forall|k: int| a <= k < h + 1 implies !lt(x, #[trigger] v@[k]) by {
                            if k < h && lt(x, v@[k]) {
                                lemma_mono(lt, x, v@[k], g[k], v@[h as int], g[h as int]);
                            }
                        }
                    }
                }
                lo = h + 1;
            }
        }
        proof {
            if ok {
                lemma_less(*less, x, v@[i - 1], r);
            }
        }
        let ghost vb = v@;
        let mut j = i;
        while j > lo
            invariant
                a <= lo <= j <= i < v@.len(),
                v@.len() == vb.len(),
                forall|k: int| 0 <= k < lo || i < k < v@.len() ==> #[trigger] v@[k] == vb[k],
                forall|k: int| lo <= k <= j ==> #[trigger] v@[k] == vb[k],
                forall|k: int| j < k <= i ==> #[trigger] v@[k] == vb[k - 1],
            decreases j - lo,
        {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[lo] = x;
        proof {
            let g1 = Seq::new(
                g.len(),
                |k: int|
                    if k < lo || k > i {
                        g[k]
                    } else if k == lo {
                        i as int
                    } else {
                        g[k - 1]
                    },
            );
            let inv1 = Seq::new(
                inv.len(),
                |t: int|
                    if t < a || t > i {
                        inv[t]
                    } else if t == i {
                        lo as int
                    } else if inv[t] < lo {
                        inv[t]
                    } else {
                        inv[t] + 1
                    },
            );
            assert(is_perm(g1, inv1, a as int, i + 1));
            assert(tracks(v@, v0, g1, a as int, i + 1));
            if ok {
                assert forall|k: int, l: int| a <= k < l < i + 1 implies before(
                    lt,
                    #[trigger] v@[k],
                    g1[k],
                    #[trigger] v@[l],
                    g1[l],
                ) by {
                    if k == lo {
                        assert(lt(x, vb[l - 1]));
                    } else if l == lo {
                        assert(!lt(x, vb[k]));
                        assert(g[k] < i);
                    } else {
                        let k0 = if k < lo { k } else { k - 1 };
                        let l0 = if l < lo { l } else { l - 1 };
                        assert(before(lt, vb[k0], g[k0], vb[l0], g[l0]));
                    }
                }
            }
            g = g1;
            inv = inv1;
        }
        i += 1;
    }
    proof {
        assert(is_perm(g, inv, a as int, b as int));
    }
}

/// Merges the runs `[a, m)` and `[m, b)`, each in stable order, in place; the left run goes through
/// `buf`, and ties keep the left element first.
fn merge<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], buf: &mut Vec<T>, a: usize, m: usize, b: usize, less: &F)
    requires
        a < m < b <= old(v)@.len(),
        old(buf)@.len() >= m - a,
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
        well_behaved(*less) ==> sorted_by(lt_of(*less), old(v)@, ident(old(v)@.len()), a as int, m as int),
        well_behaved(*less) ==> sorted_by(lt_of(*less), old(v)@, ident(old(v)@.len()), m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sorts(*less, old(v)@, final(v)@, a as int, b as int),
{
    let ghost v0 = v@;
    let ghost ok = well_behaved(*less);
    let ghost lt = lt_of(*less);
    let ghost id = ident(v@.len());
    let r = less(&v[m], &v[m - 1]);
    if !r {
        proof {
            assert(is_perm(id, id, a as int, b as int));
            if ok {
                lemma_less(*less, v@[m as int], v@[m - 1], r);
                assert forall|k: int, l: int| a <= k < l < b implies before(
                    lt,
                    #[trigger] v@[k],
                    id[k],
                    #[trigger] v@[l],
                    id[l],
                ) by {
                    if k < m && l >= m {
                        let km = m - 1;
                        assert(before(lt, v@[km], km, v@[m as int], m as int));
                        if k < km {
                            lemma_before_trans(lt, v@[k], k, v@[km], km, v@[m as int], m as int);
                        }
                        if l > m {
                            lemma_before_trans(lt, v@[k], k, v@[m as int], m as int, v@[l], l);
                        }
                    }
                }
            }
        }
        return ;
    }
    let nl = m - a;
    let mut t: usize = 0;
    while t < nl
        invariant
            t <= nl == m - a,
            a < m < b <= v@.len(),
            buf@.len() == old(buf)@.len() >= nl,
            v@ == v0,
            forall|k: int| 0 <= k < t ==> #[trigger] buf@[k] == v0[a + k],
        decreases nl - t,
    {
        buf[t] = v[a + t];
        t += 1;
    }
    let mut i: usize = 0;
    let mut j: usize = m;
    let mut k: usize = a;
    let ghost mut g = id;
    let ghost mut inv = id;
    while i < nl && j < b
        invariant
            nl == m - a,
            a < m < b <= v@.len() == v0.len() == g.len() == inv.len(),
            buf@.len() == old(buf)@.len() >= nl,
            i <= nl,
            m <= j <= b,
            k == a + i + (j - m),
            ok == well_behaved(*less),
            lt == lt_of(*less),
            forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
            forall|q: int| 0 <= q < nl ==> #[trigger] buf@[q] == v0[a + q],
            forall|q: int| 0 <= q < a || j <= q < v@.len() ==> #[trigger] v@[q] == v0[q],
            forall|q: int| j <= q < b ==> #[trigger] g[q] == q,
            forall|q: int| a <= q < k ==> (a <= #[trigger] g[q] < a + i || m <= g[q] < j) && inv[g[q]] == q,
            forall|t: int|
                (a <= t < a + i || m <= t < j) ==> a <= #[trigger] inv[t] < k && g[inv[t]] == t,
            forall|t: int| (a + i <= t < m || j <= t < b) ==> #[trigger] inv[t] == t,
            tracks(v@, v0, g, a as int, k as int),
            ok ==> sorted_by(lt, v0, id, a as int, m as int),
            ok ==> sorted_by(lt, v0, id, m as int, b as int),
            ok ==> sorted_by(lt, v@, g, a as int, k as int),
            ok ==> i < nl ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g[q], v0[a + i], a + i),
            ok ==> j < b ==> forall|q: int| a <= q < k ==> before(lt, #[trigger] v@[q], g[q], v0[j as int], j as int),
        decreases nl - i + b - j,
    {
        let ghost vk = v@;
        let ghost gk = g;
        let r = less(&v[j], &buf[i]);
        if r {
            v[k] = v[j];
            proof {
                g = g.update(k as int, j as int);
                inv = inv.update(j as int, k as int);
                if ok {
                    lemma_less(*less, v0[j as int], v0[a + i], r);
                    let e = v0[j as int];
                    assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[a + i], a + i) by {
                        if q < k {
                            lemma_before_trans(lt, vk[q], gk[q], e, j as int, v0[a + i], a + i);
                        }
                    }
                    if j + 1 < b {
                        assert(before(lt, e, j as int, v0[j + 1], j + 1));
                        assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[j + 1], j + 1) by {
                            if q < k {
                                lemma_before_trans(lt, vk[q], gk[q], e, j as int, v0[j + 1], j + 1);
                            }
                        }
                    }
                    assert(sorted_by(lt, v@, g, a as int, k + 1));
                }
            }
            j += 1;
        } else {
            v[k] = buf[i];
            proof {
                g = g.update(k as int, a + i);
                inv = inv.update(a + i, k as int);
                if ok {
                    lemma_less(*less, v0[j as int], v0[a + i], r);
                    let e = v0[a + i];
                    assert(before(lt, e, a + i, v0[j as int], j as int));
                    assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[j as int], j as int) by {
                        if q < k {
                            lemma_before_trans(lt, vk[q], gk[q], e, a + i, v0[j as int], j as int);
                        }
                    }
                    if i + 1 < nl {
                        assert(before(lt, e, a + i, v0[a + i + 1], a + i + 1));
                        assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[a + i + 1], a + i + 1) by {
                            if q < k {
                                lemma_before_trans(lt, vk[q], gk[q], e, a + i, v0[a + i + 1], a + i + 1);
                            }
                        }
                    }
                    assert(sorted_by(lt, v@, g, a as int, k + 1));
                }
            }
            i += 1;
        }
        k += 1;
    }
    while i < nl
        invariant
            nl == m - a,
            a < m < b <= v@.len() == v0.len() == g.len() == inv.len(),
            buf@.len() == old(buf)@.len() >= nl,
            i <= nl,
            m <= j <= b,
            i < nl ==> j == b,
            k == a + i + (j - m),
            ok == well_behaved(*less),
            lt == lt_of(*less),
            forall|q: int| 0 <= q < nl ==> #[trigger] buf@[q] == v0[a + q],
            forall|q: int| 0 <= q < a || j <= q < v@.len() ==> #[trigger] v@[q] == v0[q],
            forall|q: int| j <= q < b ==> #[trigger] g[q] == q,
            forall|q: int| a <= q < k ==> (a <= #[trigger] g[q] < a + i || m <= g[q] < j) && inv[g[q]] == q,
            forall|t: int|
                (a <= t < a + i || m <= t < j) ==> a <= #[trigger] inv[t] < k && g[inv[t]] == t,
            forall|t: int| (a + i <= t < m || j <= t < b) ==> #[trigger] inv[t] == t,
            tracks(v@, v0, g, a as int, k as int),
            ok ==> swo(lt),
            ok ==> sorted_by(lt, v0, id, a as int, m as int),
            ok ==> sorted_by(lt, v0, id, m as int, b as int),
            ok ==> sorted_by(lt, v@, g, a as int, k as int),
            ok ==> i < nl ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g[q], v0[a + i], a + i),
            ok ==> j < b ==> forall|q: int| a <= q < k ==> before(lt, #[trigger] v@[q], g[q], v0[j as int], j as int),
        decreases nl - i,
    {
        let ghost vk = v@;
        let ghost gk = g;
        v[k] = buf[i];
        proof {
            g = g.update(k as int, a + i);
            inv = inv.update(a + i, k as int);
            if ok {
                let e = v0[a + i];
                if i + 1 < nl {
                    assert(before(lt, e, a + i, v0[a + i + 1], a + i + 1));
                    assert forall|q: int| a <= q < k + 1 implies before(lt, #[trigger] v@[q], g[q], v0[a + i + 1], a + i + 1) by {
                        if q < k {
                            lemma_before_trans(lt, vk[q], gk[q], e, a + i, v0[a + i + 1], a + i + 1);
                        }
                    }
                }
                assert(sorted_by(lt, v@, g, a as int, k + 1));
            }
        }
        i += 1;
        k += 1;
    }
    proof {
        assert(k == j);
        assert(is_perm(g, inv, a as int, b as int));
        assert(tracks(v@, v0, g, a as int, b as int));
        if ok {
            assert forall|q: int, r: int| a <= q < r < b implies before(
                lt,
                #[trigger] v@[q],
                g[q],
                #[trigger] v@[r],
                g[r],
            ) by {
                if q < k && r >= k && r > j {
                    lemma_before_trans(lt, v@[q], g[q], v0[j as int], j as int, v@[r], g[r]);
                }
            }
        }
    }
}

} // verus!
