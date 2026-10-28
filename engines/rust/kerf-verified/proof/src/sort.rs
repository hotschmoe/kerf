use alloc::vec::Vec;
use vstd::multiset::*;
use vstd::prelude::*;
use vstd::seq_lib::*;

verus! {

broadcast use {vstd::seq_lib::group_to_multiset_ensures, vstd::multiset::group_multiset_axioms};

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

/// `x` (original position `gx`) comes before `y` (original position `gy`) in a stable sort.
pub open spec fn before<T>(lt: spec_fn(T, T) -> bool, x: T, gx: int, y: T, gy: int) -> bool {
    lt(x, y) || (!lt(y, x) && gx < gy)
}

pub open spec fn sorted_run<T>(lt: spec_fn(T, T) -> bool, s: Seq<T>, g: Seq<int>, a: int, b: int) -> bool {
    forall|k: int, l: int| a <= k < l < b ==> before(lt, #[trigger] s[k], g[k], #[trigger] s[l], g[l])
}

pub open spec fn range(a: int, b: int) -> Seq<int> {
    Seq::new((b - a) as nat, |i: int| a + i)
}

/// `g[a..b)` is a permutation of the positions `a..b`.
pub open spec fn perm_of_range(g: Seq<int>, a: int, b: int) -> bool {
    g.subrange(a, b).to_multiset() == range(a, b).to_multiset()
}

/// Positions `[a, b)` of `s` hold the original elements `orig[g[k]]`.
pub open spec fn tracks<T>(s: Seq<T>, orig: Seq<T>, g: Seq<int>, a: int, b: int) -> bool {
    &&& g.len() == s.len()
    &&& forall|k: int| a <= k < b ==> 0 <= #[trigger] g[k] < orig.len() && s[k] == orig[g[k]]
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
    if lt(y, z) {
        assert(lt(x, z));
    } else {
        assert(lt(x, z) || lt(z, y));
    }
}

proof fn lemma_range_bound(g: Seq<int>, a: int, b: int, k: int)
    requires
        0 <= a <= k < b <= g.len(),
        perm_of_range(g, a, b),
    ensures
        a <= g[k] < b,
{
    let s = g.subrange(a, b);
    assert(s[k - a] == g[k]);
    assert(s.contains(g[k]));
    to_multiset_contains(s, g[k]);
    to_multiset_contains(range(a, b), g[k]);
    let r = range(a, b);
    assert(r.contains(g[k]));
    let w = choose|w: int| 0 <= w < r.len() && r[w] == g[k];
    assert(r[w] == a + w);
}

proof fn lemma_range_split(a: int, m: int, b: int)
    requires
        a <= m <= b,
    ensures
        range(a, b).to_multiset() == range(a, m).to_multiset().add(range(m, b).to_multiset()),
{
    assert(range(a, b) =~= range(a, m) + range(m, b));
    lemma_multiset_commutative(range(a, m), range(m, b));
}

proof fn lemma_identity_perm(g: Seq<int>, a: int, b: int)
    requires
        0 <= a <= b <= g.len(),
        forall|k: int| a <= k < b ==> g[k] == k,
    ensures
        perm_of_range(g, a, b),
{
    assert(g.subrange(a, b) =~= range(a, b));
}

/// Stable sort of `v` by `less`, as the Zig engine's `sort.stable` (whose output it reproduces for
/// every strict weak ordering, since a stable sort's output is unique). Never panics and always
/// permutes, whatever `less` does; sorted and stable whenever `less` is a strict weak ordering.
pub fn stable_sort_by<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], less: F)
    requires
        forall|a: &T, b: &T| #[trigger] less.requires((a, b)),
    ensures
        exists|g: Seq<int>|
            #![trigger g.to_multiset()]
            {
                &&& g.len() == old(v)@.len()
                &&& g.to_multiset() == range(0, g.len() as int).to_multiset()
                &&& tracks(final(v)@, old(v)@, g, 0, g.len() as int)
                &&& deterministic(less) && swo(lt_of(less)) ==> sorted_run(
                    lt_of(less),
                    final(v)@,
                    g,
                    0,
                    g.len() as int,
                )
            },
{
    let n = v.len();
    let ghost orig = v@;
    let ghost g0 = range(0, n as int);
    if n < 2 {
        proof {
            assert(g0.len() == n);
            assert(g0.to_multiset() == range(0, g0.len() as int).to_multiset());
            assert(tracks(v@, orig, g0, 0, n as int));
            assert(sorted_run(lt_of(less), v@, g0, 0, n as int));
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
    let ghost p = deterministic(less) && swo(lt_of(less));
    let g = sort_range(v, &mut buf, 0, n, &less, Ghost(orig), Ghost(lt_of(less)), Ghost(p));
    proof {
        assert(g@.subrange(0, n as int) =~= g@);
    }
}

fn sort_range<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    buf: &mut Vec<T>,
    a: usize,
    b: usize,
    less: &F,
    Ghost(orig): Ghost<Seq<T>>,
    Ghost(lt): Ghost<spec_fn(T, T) -> bool>,
    Ghost(p): Ghost<bool>,
) -> (g: Ghost<Seq<int>>)
    requires
        a < b <= old(v)@.len(),
        old(v)@.len() == orig.len(),
        old(buf)@.len() >= old(v)@.len() / 2,
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
        p ==> swo(lt),
        p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
        forall|k: int| a <= k < b ==> #[trigger] old(v)@[k] == orig[k],
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> final(v)@[k] == old(v)@[k],
        tracks(final(v)@, orig, g@, a as int, b as int),
        perm_of_range(g@, a as int, b as int),
        p ==> sorted_run(lt, final(v)@, g@, a as int, b as int),
    decreases b - a,
{
    if b - a <= 20 {
        return insertion(v, a, b, less, Ghost(orig), Ghost(lt), Ghost(p));
    }
    let m = a + (b - a) / 2;
    let gl = sort_range(v, buf, a, m, less, Ghost(orig), Ghost(lt), Ghost(p));
    let gr = sort_range(v, buf, m, b, less, Ghost(orig), Ghost(lt), Ghost(p));
    let ghost g = Seq::new(v@.len(), |k: int| if k < m { gl@[k] } else { gr@[k] });
    proof {
        assert(g.subrange(a as int, m as int) =~= gl@.subrange(a as int, m as int));
        assert(g.subrange(m as int, b as int) =~= gr@.subrange(m as int, b as int));
        if p {
            assert(sorted_run(lt, v@, g, a as int, m as int));
            assert(sorted_run(lt, v@, g, m as int, b as int));
        }
    }
    merge(v, buf, a, m, b, less, Ghost(orig), Ghost(lt), Ghost(p), Ghost(g))
}

/// Binary insertion sort of `[a, b)`: element i goes right after the last element of `[a, i)` that
/// is not greater than it.
fn insertion<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    a: usize,
    b: usize,
    less: &F,
    Ghost(orig): Ghost<Seq<T>>,
    Ghost(lt): Ghost<spec_fn(T, T) -> bool>,
    Ghost(p): Ghost<bool>,
) -> (g: Ghost<Seq<int>>)
    requires
        a < b <= old(v)@.len(),
        old(v)@.len() == orig.len(),
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
        p ==> swo(lt),
        p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
        forall|k: int| a <= k < b ==> #[trigger] old(v)@[k] == orig[k],
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> final(v)@[k] == old(v)@[k],
        tracks(final(v)@, orig, g@, a as int, b as int),
        perm_of_range(g@, a as int, b as int),
        p ==> sorted_run(lt, final(v)@, g@, a as int, b as int),
{
    let ghost v0 = v@;
    let ghost mut g = Seq::new(v@.len(), |k: int| k);
    let mut i = a + 1;
    proof {
        lemma_identity_perm(g, a as int, i as int);
    }
    while i < b
        invariant
            a < i <= b <= v@.len(),
            v@.len() == v0.len() == orig.len(),
            forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
            p ==> swo(lt),
            p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
            forall|k: int| 0 <= k < a || i <= k < v@.len() ==> #[trigger] v@[k] == v0[k],
            forall|k: int| a <= k < b ==> v0[k] == orig[k],
            forall|k: int| i <= k < v@.len() ==> g[k] == k,
            tracks(v@, orig, g, a as int, i as int),
            perm_of_range(g, a as int, i as int),
            p ==> sorted_run(lt, v@, g, a as int, i as int),
        decreases b - i,
    {
        let x = v[i];
        if !less(&x, &v[i - 1]) {
            proof {
                if p {
                    assert forall|k: int, l: int| a <= k < l < i + 1 implies before(
                        lt,
                        #[trigger] v@[k],
                        g[k],
                        #[trigger] v@[l],
                        g[l],
                    ) by {
                        if l == i {
                            lemma_range_bound(g, a as int, i as int, k);
                            lemma_range_bound(g, a as int, i as int, i - 1);
                            if k < i - 1 {
                                lemma_before_trans(lt, v@[k], g[k], v@[i - 1], g[i - 1], v@[l], g[l]);
                            }
                        }
                    }
                }
                assert(g.subrange(a as int, i + 1) =~= g.subrange(a as int, i as int).push(i as int));
                assert(range(a as int, i + 1) =~= range(a as int, i as int).push(i as int));
                to_multiset_build(g.subrange(a as int, i as int), i as int);
                to_multiset_build(range(a as int, i as int), i as int);
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
                forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
                p ==> swo(lt),
                p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
                p ==> sorted_run(lt, v@, g, a as int, i as int),
                g.len() == v@.len(),
                p ==> forall|k: int| a <= k < lo ==> !lt(x, #[trigger] v@[k]),
                p ==> forall|k: int| hi <= k < i ==> lt(x, #[trigger] v@[k]),
            decreases hi - lo,
        {
            let h = lo + (hi - lo) / 2;
            if less(&x, &v[h]) {
                proof {
                    if p {
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
                    if p {
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
            assert(g1.subrange(a as int, i + 1) =~= g.subrange(a as int, i as int).insert(
                lo - a,
                i as int,
            ));
            to_multiset_insert(g.subrange(a as int, i as int), lo - a, i as int);
            assert(range(a as int, i + 1) =~= range(a as int, i as int).push(i as int));
            to_multiset_build(range(a as int, i as int), i as int);
            if p {
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
                        lemma_range_bound(g, a as int, i as int, k);
                        assert(!lt(x, vb[k]));
                    } else {
                        let k0 = if k < lo { k } else { k - 1 };
                        let l0 = if l < lo { l } else { l - 1 };
                        assert(before(lt, vb[k0], g[k0], vb[l0], g[l0]));
                    }
                }
            }
            g = g1;
        }
        i += 1;
    }
    Ghost(g)
}

/// Merges the sorted runs `[a, m)` and `[m, b)` in place, left run through `buf`; ties keep the left
/// element first.
fn merge<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    buf: &mut Vec<T>,
    a: usize,
    m: usize,
    b: usize,
    less: &F,
    Ghost(orig): Ghost<Seq<T>>,
    Ghost(lt): Ghost<spec_fn(T, T) -> bool>,
    Ghost(p): Ghost<bool>,
    Ghost(g): Ghost<Seq<int>>,
) -> (g2: Ghost<Seq<int>>)
    requires
        a < m < b <= old(v)@.len(),
        old(v)@.len() == orig.len(),
        old(buf)@.len() >= m - a,
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
        p ==> swo(lt),
        p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
        tracks(old(v)@, orig, g, a as int, b as int),
        perm_of_range(g, a as int, m as int),
        perm_of_range(g, m as int, b as int),
        p ==> sorted_run(lt, old(v)@, g, a as int, m as int),
        p ==> sorted_run(lt, old(v)@, g, m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> final(v)@[k] == old(v)@[k],
        tracks(final(v)@, orig, g2@, a as int, b as int),
        perm_of_range(g2@, a as int, b as int),
        p ==> sorted_run(lt, final(v)@, g2@, a as int, b as int),
{
    let ghost v0 = v@;
    proof {
        lemma_range_split(a as int, m as int, b as int);
        assert(g.subrange(a as int, b as int) =~= g.subrange(a as int, m as int) + g.subrange(
            m as int,
            b as int,
        ));
        lemma_multiset_commutative(g.subrange(a as int, m as int), g.subrange(m as int, b as int));
    }
    if !less(&v[m], &v[m - 1]) {
        proof {
            if p {
                lemma_range_bound(g, a as int, m as int, m - 1);
                lemma_range_bound(g, m as int, b as int, m as int);
                assert forall|k: int, l: int| a <= k < l < b implies before(
                    lt,
                    #[trigger] v@[k],
                    g[k],
                    #[trigger] v@[l],
                    g[l],
                ) by {
                    if k < m && l >= m {
                        let km = (m - 1) as int;
                        if k < km {
                            lemma_before_trans(lt, v@[k], g[k], v@[km], g[km], v@[m as int], g[m as int]);
                        }
                        if l > m {
                            lemma_before_trans(lt, v@[k], g[k], v@[m as int], g[m as int], v@[l], g[l]);
                        }
                    }
                }
            }
        }
        return Ghost(g);
    }
    let nl = m - a;
    let mut t: usize = 0;
    while t < nl
        invariant
            t <= nl == m - a,
            a < m < b <= v@.len(),
            buf@.len() == old(buf)@.len() >= nl,
            v@ == v0,
            forall|k: int| 0 <= k < t ==> buf@[k] == v0[a + k],
        decreases nl - t,
    {
        buf.set(t, v[a + t]);
        t += 1;
    }
    let mut i: usize = 0;
    let mut j: usize = m;
    let mut k: usize = a;
    let ghost mut g2 = g;
    assert(g.subrange(a as int, a as int) =~= Seq::<int>::empty());
    assert(g.subrange(m as int, m as int) =~= Seq::<int>::empty());
    assert(Seq::<int>::empty().to_multiset() =~= Multiset::<int>::empty());
    while i < nl && j < b
        invariant
            nl == m - a,
            a < m < b <= v@.len() == v0.len() == orig.len(),
            buf@.len() == old(buf)@.len() >= nl,
            i <= nl,
            m <= j <= b,
            k == a + i + (j - m),
            forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
            p ==> swo(lt),
            p ==> forall|x: T, y: T, r: bool| #[trigger] less.ensures((&x, &y), r) ==> r == lt(x, y),
            forall|q: int| 0 <= q < nl ==> buf@[q] == v0[a + q],
            forall|q: int| 0 <= q < a || j <= q < v@.len() ==> #[trigger] v@[q] == v0[q],
            g2.len() == g.len(),
            forall|q: int| j <= q < b ==> g2[q] == g[q],
            tracks(v0, orig, g, a as int, b as int),
            tracks(v@, orig, g2, a as int, k as int),
            perm_of_range(g, a as int, m as int),
            perm_of_range(g, m as int, b as int),
            g2.subrange(a as int, k as int).to_multiset() == g.subrange(a as int, a + i).to_multiset().add(
                g.subrange(m as int, j as int).to_multiset(),
            ),
            p ==> sorted_run(lt, v0, g, a as int, m as int),
            p ==> sorted_run(lt, v0, g, m as int, b as int),
            p ==> sorted_run(lt, v@, g2, a as int, k as int),
            p ==> i < nl ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g2[q], v0[a + i], g[a + i]),
            p ==> j < b ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g2[q], v0[j as int], g[j as int]),
        decreases nl - i + b - j,
    {
        let ghost vk = v@;
        let ghost g2k = g2;
        if less(&v[j], &buf[i]) {
            v[k] = v[j];
            proof {
                g2 = g2.update(k as int, g[j as int]);
                assert(g2.subrange(a as int, k + 1) =~= g2k.subrange(a as int, k as int).push(g[j as int]));
                assert(g.subrange(m as int, j + 1) =~= g.subrange(m as int, j as int).push(g[j as int]));
                assert(g2.subrange(a as int, k + 1).to_multiset() =~= g.subrange(a as int, a + i).to_multiset().add(
                    g.subrange(m as int, j + 1).to_multiset(),
                ));
                if p {
                    let e = v0[j as int];
                    assert(lt(e, v0[a + i]));
                    assert forall|q: int| a <= q < k + 1 implies before(
                        lt,
                        #[trigger] v@[q],
                        g2[q],
                        v0[a + i],
                        g[a + i],
                    ) by {
                        if q < k {
                            assert(before(lt, vk[q], g2k[q], e, g[j as int]));
                            lemma_before_trans(lt, vk[q], g2k[q], e, g[j as int], v0[a + i], g[a + i]);
                        }
                    }
                    if j + 1 < b {
                        assert(before(lt, e, g[j as int], v0[j + 1], g[j + 1]));
                        assert forall|q: int| a <= q < k + 1 implies before(
                            lt,
                            #[trigger] v@[q],
                            g2[q],
                            v0[j + 1],
                            g[j + 1],
                        ) by {
                            if q < k {
                                lemma_before_trans(lt, vk[q], g2k[q], e, g[j as int], v0[j + 1], g[j + 1]);
                            }
                        }
                    }
                    assert(sorted_run(lt, v@, g2, a as int, k + 1));
                }
            }
            j += 1;
        } else {
            v[k] = buf[i];
            proof {
                g2 = g2.update(k as int, g[a + i]);
                assert(g2.subrange(a as int, k + 1) =~= g2k.subrange(a as int, k as int).push(g[a + i]));
                assert(g.subrange(a as int, a + i + 1) =~= g.subrange(a as int, a + i).push(g[a + i]));
                assert(g2.subrange(a as int, k + 1).to_multiset() =~= g.subrange(a as int, a + i + 1).to_multiset().add(
                    g.subrange(m as int, j as int).to_multiset(),
                ));
                if p {
                    let e = v0[a + i];
                    lemma_range_bound(g, a as int, m as int, a + i);
                    lemma_range_bound(g, m as int, b as int, j as int);
                    assert(before(lt, e, g[a + i], v0[j as int], g[j as int]));
                    assert forall|q: int| a <= q < k + 1 implies before(
                        lt,
                        #[trigger] v@[q],
                        g2[q],
                        v0[j as int],
                        g[j as int],
                    ) by {
                        if q < k {
                            lemma_before_trans(lt, vk[q], g2k[q], e, g[a + i], v0[j as int], g[j as int]);
                        }
                    }
                    if i + 1 < nl {
                        assert(before(lt, e, g[a + i], v0[a + i + 1], g[a + i + 1]));
                        assert forall|q: int| a <= q < k + 1 implies before(
                            lt,
                            #[trigger] v@[q],
                            g2[q],
                            v0[a + i + 1],
                            g[a + i + 1],
                        ) by {
                            if q < k {
                                lemma_before_trans(lt, vk[q], g2k[q], e, g[a + i], v0[a + i + 1], g[a + i + 1]);
                            }
                        }
                    }
                    assert(sorted_run(lt, v@, g2, a as int, k + 1));
                }
            }
            i += 1;
        }
        k += 1;
    }
    while i < nl
        invariant
            nl == m - a,
            a < m < b <= v@.len() == v0.len() == orig.len(),
            buf@.len() == old(buf)@.len() >= nl,
            i <= nl,
            m <= j <= b,
            i < nl ==> j == b,
            k == a + i + (j - m),
            p ==> swo(lt),
            forall|q: int| 0 <= q < nl ==> buf@[q] == v0[a + q],
            forall|q: int| 0 <= q < a || j <= q < v@.len() ==> #[trigger] v@[q] == v0[q],
            g2.len() == g.len(),
            forall|q: int| j <= q < b ==> g2[q] == g[q],
            tracks(v0, orig, g, a as int, b as int),
            tracks(v@, orig, g2, a as int, k as int),
            g2.subrange(a as int, k as int).to_multiset() == g.subrange(a as int, a + i).to_multiset().add(
                g.subrange(m as int, j as int).to_multiset(),
            ),
            p ==> sorted_run(lt, v0, g, a as int, m as int),
            p ==> sorted_run(lt, v@, g2, a as int, k as int),
            p ==> sorted_run(lt, v0, g, m as int, b as int),
            p ==> i < nl ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g2[q], v0[a + i], g[a + i]),
            p ==> j < b ==> forall|q: int|
                a <= q < k ==> before(lt, #[trigger] v@[q], g2[q], v0[j as int], g[j as int]),
        decreases nl - i,
    {
        let ghost vk = v@;
        let ghost g2k = g2;
        v[k] = buf[i];
        proof {
            g2 = g2.update(k as int, g[a + i]);
            assert(g2.subrange(a as int, k + 1) =~= g2k.subrange(a as int, k as int).push(g[a + i]));
            assert(g.subrange(a as int, a + i + 1) =~= g.subrange(a as int, a + i).push(g[a + i]));
            assert(g2.subrange(a as int, k + 1).to_multiset() =~= g.subrange(a as int, a + i + 1).to_multiset().add(
                g.subrange(m as int, j as int).to_multiset(),
            ));
            if p {
                let e = v0[a + i];
                if i + 1 < nl {
                    assert(before(lt, e, g[a + i], v0[a + i + 1], g[a + i + 1]));
                    assert forall|q: int| a <= q < k + 1 implies before(
                        lt,
                        #[trigger] v@[q],
                        g2[q],
                        v0[a + i + 1],
                        g[a + i + 1],
                    ) by {
                        if q < k {
                            lemma_before_trans(lt, vk[q], g2k[q], e, g[a + i], v0[a + i + 1], g[a + i + 1]);
                        }
                    }
                }
                assert(sorted_run(lt, v@, g2, a as int, k + 1));
            }
        }
        i += 1;
        k += 1;
    }
    proof {
        assert(g.subrange(a as int, a + nl) =~= g.subrange(a as int, m as int));
        if j < b {
            assert(i == nl);
        }
        if i < nl {
            assert(j == b);
        }
        assert(g2.subrange(a as int, b as int) =~= g2.subrange(a as int, k as int) + g2.subrange(
            k as int,
            b as int,
        ));
        assert(g2.subrange(k as int, b as int) =~= g.subrange(j as int, b as int));
        assert(g.subrange(m as int, b as int) =~= g.subrange(m as int, j as int) + g.subrange(
            j as int,
            b as int,
        ));
        lemma_multiset_commutative(g2.subrange(a as int, k as int), g2.subrange(k as int, b as int));
        lemma_multiset_commutative(g.subrange(m as int, j as int), g.subrange(j as int, b as int));
        let ma = g.subrange(a as int, m as int).to_multiset();
        let mb = g.subrange(m as int, j as int).to_multiset();
        let mc = g.subrange(j as int, b as int).to_multiset();
        assert(i == nl && k == j);
        assert(g2.subrange(a as int, b as int).to_multiset() =~= ma.add(mb).add(mc));
        assert(g.subrange(m as int, b as int).to_multiset() =~= mb.add(mc));
        lemma_range_split(a as int, m as int, b as int);
        assert(ma.add(mb).add(mc) =~= range(a as int, b as int).to_multiset());
        if p {
            assert forall|q: int, r: int| a <= q < r < b implies before(
                lt,
                #[trigger] v@[q],
                g2[q],
                #[trigger] v@[r],
                g2[r],
            ) by {
                if q < k && r >= k {
                    if r > j {
                        lemma_before_trans(lt, v@[q], g2[q], v0[j as int], g[j as int], v@[r], g2[r]);
                    }
                }
            }
        }
    }
    Ghost(g2)
}

} // verus!
