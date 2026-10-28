//! The one stable sort of the engine: exactly Zig's `sort.stable`, proven with Verus (engines/rust/verus/VERUS.md).
#![allow(clippy::assign_op_pattern, clippy::collapsible_match, clippy::implicit_saturating_add, clippy::len_zero, clippy::manual_checked_ops, clippy::manual_clamp, clippy::manual_is_multiple_of, clippy::manual_range_contains, clippy::ptr_arg, clippy::question_mark, clippy::single_match, clippy::too_many_arguments)]
// Verified code is written in the forms Verus has specifications for, which these style lints would rewrite.

use alloc::vec::Vec;
#[cfg(verus_keep_ghost)]
pub use ord::*;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg(verus_keep_ghost)]
pub mod ord {
    #[cfg(verus_keep_ghost)]
    use vstd::prelude::*;

    #[cfg(verus_keep_ghost)]
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

    /// The order the index phase sorts by: `items[x]` against `items[y]`, out-of-range indices last.
    pub open spec fn ilt<E, F: Fn(&E, &E) -> bool>(less: F, items: Seq<E>) -> spec_fn(usize, usize) -> bool {
        |x: usize, y: usize|
            if (x as int) < items.len() {
                if (y as int) < items.len() {
                    lt_of(less)(items[x as int], items[y as int])
                } else {
                    true
                }
            } else {
                false
            }
    }

    pub broadcast proof fn lemma_ilt_swo<E, F: Fn(&E, &E) -> bool>(less: F, items: Seq<E>)
        ensures
            well_behaved(less) ==> swo(#[trigger] ilt(less, items)),
    {
        if well_behaved(less) {
            let lt = lt_of(less);
            let il = ilt(less, items);
            let n = items.len();
            assert forall|a: usize| !#[trigger] il(a, a) by {
                if (a as int) < n {
                    assert(!lt(items[a as int], items[a as int]));
                }
            }
            assert forall|a: usize, b: usize, c: usize| #[trigger] il(a, b) && #[trigger] il(b, c) implies il(a, c) by {
                if (c as int) < n {
                    assert(lt(items[a as int], items[b as int]) && lt(items[b as int], items[c as int]));
                }
            }
            assert forall|a: usize, b: usize, c: usize| #![trigger il(a, c), il(a, b)] #![trigger il(a, c), il(b, c)] il(a, c) implies il(a, b) || il(b, c) by {
                if (b as int) < n && (c as int) < n {
                    assert(lt(items[a as int], items[c as int]));
                    assert(lt(items[a as int], items[b as int]) || lt(items[b as int], items[c as int]));
                }
            }
        }
    }

    } // verus!
}

#[cfg(verus_keep_ghost)]
verus! {

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

} // verus!

/// Trusted: swaps two elements of a slice.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        i < old(s)@.len(),
        j < old(s)@.len(),
    ensures
        final(s)@ == old(s)@.update(i as int, old(s)@[j as int]).update(j as int, old(s)@[i as int]),
))]
fn swap<T>(s: &mut [T], i: usize, j: usize) {
    s.swap(i, j);
}

/// Binary insertion sort of `[a, b)`: element i goes right after the last element of `[a, i)` that
/// is not greater than it.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < b <= old(v)@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b as int),
))]
fn insertion(cx: &dyn IdxLess, v: &mut [usize], a: usize, b: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost mut g = ident(v@.len());
        let ghost mut inv = ident(v@.len());
    }
    let mut i = a + 1;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                a < i <= b <= v@.len(),
                v@.len() == v0.len() == g.len(),
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                forall|k: int| 0 <= k < a || i <= k < v@.len() ==> #[trigger] v@[k] == v0[k],
                forall|k: int| i <= k < v@.len() ==> #[trigger] g[k] == k,
                forall|k: int| i <= k < v@.len() ==> #[trigger] inv[k] == k,
                is_perm(g, inv, a as int, i as int),
                tracks(v@, v0, g, a as int, i as int),
                ok ==> sorted_by(lt, v@, g, a as int, i as int),
        decreases b - i,
    ))]
    while i < b {
        let x = v[i];
        let r = cx.lt(x, v[i - 1]);
        if !r {
            #[cfg(verus_keep_ghost)]
            proof! {
                assert(g[i as int] == i && inv[i as int] == i);
                assert(is_perm(g, inv, a as int, i + 1));
                if ok {
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
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                        a <= lo <= hi < i < v@.len(),
                        x == v@[i as int],
                        v@.len() == g.len(),
                        ok == cx.ok(),
                        ok ==> swo(lt),
                        lt == cx.ord(),
                        cx.callable(),
                cx.ok() ==> swo(cx.ord()),
                        ok ==> sorted_by(lt, v@, g, a as int, i as int),
                        ok ==> forall|k: int| a <= k < lo ==> !lt(x, #[trigger] v@[k]),
                        ok ==> forall|k: int| hi <= k < i ==> lt(x, #[trigger] v@[k]),
            decreases hi - lo,
        ))]
        while lo < hi {
            let h = lo + (hi - lo) / 2;
            let r = cx.lt(x, v[h]);
            if r {
                #[cfg(verus_keep_ghost)]
                proof! {
                    if ok {
                        assert forall|k: int| h <= k < i implies lt(x, #[trigger] v@[k]) by {
                            if k > h {
                                lemma_mono(lt, x, v@[h as int], g[h as int], v@[k], g[k]);
                            }
                        }
                    }
                }
                hi = h;
            } else {
                #[cfg(verus_keep_ghost)]
                proof! {
                    if ok {
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
        #[cfg(verus_keep_ghost)]
        proof! {
            if ok {
                }
        }
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost vb = v@; }
        let mut j = i;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                a <= lo <= j <= i < v@.len(),
                v@.len() == vb.len(),
                forall|k: int| 0 <= k < lo || i < k < v@.len() ==> #[trigger] v@[k] == vb[k],
                forall|k: int| lo <= k <= j ==> #[trigger] v@[k] == vb[k],
                forall|k: int| j < k <= i ==> #[trigger] v@[k] == vb[k - 1],
            decreases j - lo,
        ))]
        while j > lo {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[lo] = x;
        #[cfg(verus_keep_ghost)]
        proof! {
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
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(is_perm(g, inv, a as int, b as int));
    }
}

/// The same binary insertion sort, moving elements by swaps (any `T`).
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < b <= old(v)@.len(),
        forall|x: &T, y: &T| #[trigger] less.requires((x, y)),
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sorts(*less, old(v)@, final(v)@, a as int, b as int),
))]
fn insertion_swap<T, F: Fn(&T, &T) -> bool>(v: &mut [T], a: usize, b: usize, less: &F) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = well_behaved(*less);
        let ghost lt = lt_of(*less);
        let ghost mut g = ident(v@.len());
        let ghost mut inv = ident(v@.len());
    }
    let mut i = a + 1;
    #[cfg_attr(verus_keep_ghost, verus_spec(
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
    ))]
    while i < b {
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost x = v@[i as int]; }
        let r = less(&v[i], &v[i - 1]);
        if !r {
            #[cfg(verus_keep_ghost)]
            proof! {
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
        #[cfg_attr(verus_keep_ghost, verus_spec(
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
        ))]
        while lo < hi {
            let h = lo + (hi - lo) / 2;
            let r = less(&v[i], &v[h]);
            if r {
                #[cfg(verus_keep_ghost)]
                proof! {
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
                #[cfg(verus_keep_ghost)]
                proof! {
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
        #[cfg(verus_keep_ghost)]
        proof! {
            if ok {
                lemma_less(*less, x, v@[i - 1], r);
            }
        }
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost vb = v@; }
        let mut j = i;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                a <= lo <= j <= i < v@.len(),
                v@.len() == vb.len(),
                x == vb[i as int],
                forall|k: int| 0 <= k < lo || i < k < v@.len() ==> #[trigger] v@[k] == vb[k],
                forall|k: int| lo <= k < j ==> #[trigger] v@[k] == vb[k],
                v@[j as int] == x,
                forall|k: int| j < k <= i ==> #[trigger] v@[k] == vb[k - 1],
            decreases j - lo,
        ))]
        while j > lo {
            swap(v, j - 1, j);
            j -= 1;
        }
        #[cfg(verus_keep_ghost)]
        proof! {
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
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(is_perm(g, inv, a as int, b as int));
    }
}

/// Zig's buffered merge loop: left run `[a, m)` into `buf`, merged front to back with `[m, b)`; ties
/// keep the left element first.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < m < b <= old(v)@.len(),
        old(buf)@.len() >= m - a,
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b as int),
))]
fn merge_core(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a: usize, m: usize, b: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost id = ident(v@.len());
    }
    let nl = m - a;
    let mut t: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            t <= nl == m - a,
            a < m < b <= v@.len(),
            buf@.len() == old(buf)@.len() >= nl,
            v@ == v0,
            forall|k: int| 0 <= k < t ==> #[trigger] buf@[k] == v0[a + k],
        decreases nl - t,
    ))]
    while t < nl {
        buf[t] = v[a + t];
        t += 1;
    }
    let mut i: usize = 0;
    let mut j: usize = m;
    let mut k: usize = a;
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost mut g = id;
        let ghost mut inv = id;
    }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                nl == m - a,
                a < m < b <= v@.len() == v0.len() == g.len() == inv.len(),
                buf@.len() == old(buf)@.len() >= nl,
                i <= nl,
                m <= j <= b,
                k == a + i + (j - m),
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
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
    ))]
    while i < nl && j < b {
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost vk = v@;
            let ghost gk = g;
        }
        let r = cx.lt(v[j], buf[i]);
        if r {
            v[k] = v[j];
            #[cfg(verus_keep_ghost)]
            proof! {
                g = g.update(k as int, j as int);
                inv = inv.update(j as int, k as int);
                if ok {
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
            #[cfg(verus_keep_ghost)]
            proof! {
                g = g.update(k as int, a + i);
                inv = inv.update(a + i, k as int);
                if ok {
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
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            nl == m - a,
            a < m < b <= v@.len() == v0.len() == g.len() == inv.len(),
            buf@.len() == old(buf)@.len() >= nl,
            i <= nl,
            m <= j <= b,
            i < nl ==> j == b,
            k == a + i + (j - m),
            ok == cx.ok(),
            ok ==> swo(lt),
            lt == cx.ord(),
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
    ))]
    while i < nl {
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost vk = v@;
            let ghost gk = g;
        }
        v[k] = buf[i];
        #[cfg(verus_keep_ghost)]
        proof! {
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
    #[cfg(verus_keep_ghost)]
    proof! {
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

/// Zig's `mergeLow`: the left elements not greater than the first right one are already in place;
/// the rest of the left run goes through `buf`.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a0 < m < b <= old(v)@.len(),
        old(buf)@.len() >= m - a0,
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a0 as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a0 || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a0 as int, b as int),
))]
fn merge_low(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a0: usize, m: usize, b: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
    }
    let mut a = a0;
    let mut hi = m - 1;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                a0 <= a <= hi < m < b <= v@.len(),
                v@ == v0,
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                ok ==> sorted_by(lt, v0, ident(v0.len()), a0 as int, m as int),
                ok ==> forall|q: int| a0 <= q < a ==> !lt(v0[m as int], #[trigger] v0[q]),
        decreases hi - a,
    ))]
    while a < hi {
        let h = a + (hi - a) / 2;
        let r = cx.lt(v[m], v[h]);
        if r {
            hi = h;
        } else {
            #[cfg(verus_keep_ghost)]
            proof! {
                if ok {
                    assert forall|q: int| a0 <= q < h + 1 implies !lt(v0[m as int], #[trigger] v0[q]) by {
                        if q < h && lt(v0[m as int], v0[q]) {
                            lemma_mono(lt, v0[m as int], v0[q], q, v0[h as int], h as int);
                        }
                    }
                }
            }
            a = h + 1;
        }
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost id = ident(v0.len()); }
    #[cfg(verus_keep_ghost)]
    proof! {
        if ok {
            assert(sorted_by(lt, v0, id, a as int, m as int));
        }
    }
    merge_core(cx, v, buf, a, m, b);
    #[cfg(verus_keep_ghost)]
    proof! {
        let v1 = v@;
        let (gm, im) = choose|g: Seq<int>, inv: Seq<int>|
            {
                &&& #[trigger] is_perm(g, inv, a as int, b as int)
                &&& tracks(v1, v0, g, a as int, b as int)
                &&& cx.ok() ==> sorted_by(cx.ord(), v1, g, a as int, b as int)
            };
        let g = Seq::new(v0.len(), |k: int| if a <= k < b { gm[k] } else { k });
        let inv = Seq::new(v0.len(), |k: int| if a <= k < b { im[k] } else { k });
        assert(is_perm(g, inv, a0 as int, b as int));
        assert(tracks(v1, v0, g, a0 as int, b as int));
        if ok {
            assert forall|k: int, l: int| a0 <= k < l < b implies before(lt, #[trigger] v1[k], g[k], #[trigger] v1[l], g[l]) by {
                if k < a && l >= a {
                    let p = g[l];
                    if p < m {
                        assert(before(lt, v0[k], k, v0[p], p));
                    } else {
                        assert(!lt(v0[m as int], v0[k]));
                        if p > m {
                            assert(before(lt, v0[m as int], m as int, v0[p], p));
                        }
                        if lt(v0[p], v0[k]) {
                            assert(lt(v0[p], v0[m as int]) || lt(v0[m as int], v0[k]));
                        }
                    }
                } else if k < a && l < a {
                    assert(before(lt, v0[k], k, v0[l], l));
                }
            }
        }
        assert(sortsc(cx.ok(), cx.ord(), v0, v1, a0 as int, b as int));
    }
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_mono_le<T>(lt: spec_fn(T, T) -> bool, y: T, gy: int, z: T, gz: int, x: T)
    requires
        swo(lt),
        before(lt, y, gy, z, gz),
        lt(z, x),
    ensures
        lt(y, x),
{
    if !lt(y, z) {
        assert(lt(y, x) || lt(x, z));
        if lt(x, z) {
            assert(lt(z, z));
        }
    } else {
        assert(lt(y, x));
    }
}

} // verus!

/// Zig's `mergeHigh`: the right elements not less than the last left one are already in place; the
/// rest of the right run goes through `buf`, merged back to front.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < m < b0 <= old(v)@.len(),
        old(buf)@.len() >= b0 - m,
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b0 as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b0 <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b0 as int),
))]
fn merge_high(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], a: usize, m: usize, b0: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost id = ident(v0.len());
    }
    let mut lo = m + 1;
    let mut b = b0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                a < m < lo <= b <= b0 <= v@.len(),
                v@ == v0,
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                ok ==> sorted_by(lt, v0, id, m as int, b0 as int),
                ok ==> forall|q: int| b <= q < b0 ==> !lt(#[trigger] v0[q], v0[m - 1]),
        decreases b - lo,
    ))]
    while lo < b {
        let h = lo + (b - lo) / 2;
        let r = cx.lt(v[h], v[m - 1]);
        if r {
            lo = h + 1;
        } else {
            #[cfg(verus_keep_ghost)]
            proof! {
                if ok {
                    assert forall|q: int| h <= q < b0 implies !lt(#[trigger] v0[q], v0[m - 1]) by {
                        if q > h && q < b && lt(v0[q], v0[m - 1]) {
                            lemma_mono_le(lt, v0[h as int], h as int, v0[q], q, v0[m - 1]);
                        }
                    }
                }
            }
            b = h;
        }
    }
    let nr = b - m;
    let mut t: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            t <= nr == b - m,
            m < b <= b0 <= v@.len(),
            buf@.len() == old(buf)@.len() >= b0 - m,
            v@ == v0,
            forall|q: int| 0 <= q < t ==> #[trigger] buf@[q] == v0[m + q],
        decreases nr - t,
    ))]
    while t < nr {
        buf[t] = v[m + t];
        t += 1;
    }
    let mut i: usize = m;
    let mut j: usize = nr;
    let mut k: usize = b;
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost mut g = id;
        let ghost mut inv = id;
    }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                nr == b - m,
                a < m < b <= b0 <= v@.len() == v0.len() == g.len() == inv.len(),
                buf@.len() == old(buf)@.len() >= b0 - m,
                a <= i <= m,
                j <= nr,
                k == i + j,
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                forall|q: int| 0 <= q < nr ==> #[trigger] buf@[q] == v0[m + q],
                forall|q: int| 0 <= q < i || b <= q < v@.len() ==> #[trigger] v@[q] == v0[q],
                forall|q: int| (a <= q < i || b <= q < b0) ==> #[trigger] g[q] == q,
                forall|q: int| k <= q < b ==> (i <= #[trigger] g[q] < m || m + j <= g[q] < b) && inv[g[q]] == q,
                forall|p: int| (i <= p < m || m + j <= p < b) ==> k <= #[trigger] inv[p] < b && g[inv[p]] == p,
                forall|p: int| (a <= p < i || m <= p < m + j || b <= p < b0) ==> #[trigger] inv[p] == p,
                tracks(v@, v0, g, k as int, b as int),
                ok ==> sorted_by(lt, v0, id, a as int, m as int),
                ok ==> sorted_by(lt, v0, id, m as int, b0 as int),
                ok ==> sorted_by(lt, v@, g, k as int, b as int),
                ok ==> i > a ==> forall|q: int| k <= q < b ==> before(lt, v0[i - 1], i - 1, #[trigger] v@[q], g[q]),
                ok ==> j > 0 ==> forall|q: int| k <= q < b ==> before(lt, v0[m + j - 1], m + j - 1, #[trigger] v@[q], g[q]),
        decreases i + j,
    ))]
    while i > a && j > 0 {
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost vk = v@;
            let ghost gk = g;
        }
        k -= 1;
        let r = cx.lt(buf[j - 1], v[i - 1]);
        if r {
            i -= 1;
            v[k] = v[i];
            #[cfg(verus_keep_ghost)]
            proof! {
                g = g.update(k as int, i as int);
                inv = inv.update(i as int, k as int);
                if ok {
                    let e = v0[i as int];
                    assert forall|q: int| k < q < b implies before(lt, e, i as int, #[trigger] v@[q], g[q]) by {}
                    if i > a {
                        assert(before(lt, v0[i - 1], i - 1, e, i as int));
                        assert forall|q: int| k <= q < b implies before(lt, v0[i - 1], i - 1, #[trigger] v@[q], g[q]) by {
                            if q > k {
                                lemma_before_trans(lt, v0[i - 1], i - 1, e, i as int, vk[q], gk[q]);
                            }
                        }
                    }
                    if j > 0 {
                        assert forall|q: int| k <= q < b implies before(lt, v0[m + j - 1], m + j - 1, #[trigger] v@[q], g[q]) by {}
                    }
                    assert(sorted_by(lt, v@, g, k as int, b as int));
                }
            }
        } else {
            j -= 1;
            v[k] = buf[j];
            #[cfg(verus_keep_ghost)]
            proof! {
                g = g.update(k as int, m + j);
                inv = inv.update(m + j, k as int);
                if ok {
                    let e = v0[m + j];
                    assert forall|q: int| k < q < b implies before(lt, e, m + j, #[trigger] v@[q], g[q]) by {}
                    assert(before(lt, v0[i - 1], i - 1, e, m + j));
                    assert forall|q: int| k <= q < b implies before(lt, v0[i - 1], i - 1, #[trigger] v@[q], g[q]) by {
                        if q > k {
                            lemma_before_trans(lt, v0[i - 1], i - 1, e, m + j, vk[q], gk[q]);
                        }
                    }
                    if j > 0 {
                        assert(before(lt, v0[m + j - 1], m + j - 1, e, m + j));
                        assert forall|q: int| k <= q < b implies before(lt, v0[m + j - 1], m + j - 1, #[trigger] v@[q], g[q]) by {
                            if q > k {
                                lemma_before_trans(lt, v0[m + j - 1], m + j - 1, e, m + j, vk[q], gk[q]);
                            }
                        }
                    }
                    assert(sorted_by(lt, v@, g, k as int, b as int));
                }
            }
        }
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost vm = v@; }
    let mut t: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            nr == b - m,
            a < m < b <= b0 <= v@.len() == v0.len() == g.len() == inv.len(),
            buf@.len() == old(buf)@.len() >= b0 - m,
            i == a || j == 0,
            j <= nr,
            k == i + j,
            t <= j,
            forall|q: int| 0 <= q < nr ==> #[trigger] buf@[q] == v0[m + q],
            forall|q: int| 0 <= q < a || a + t <= q < v@.len() ==> #[trigger] v@[q] == vm[q],
            forall|q: int| a <= q < a + t ==> #[trigger] v@[q] == v0[m + q - a],
        decreases j - t,
    ))]
    while t < j {
        v[a + t] = buf[t];
        t += 1;
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        let gf = Seq::new(v0.len(), |q: int| if a <= q < a + j { m + q - a } else { g[q] });
        let invf = Seq::new(v0.len(), |p: int| if m <= p < m + j { a + p - m } else { inv[p] });
        assert(is_perm(gf, invf, a as int, b0 as int));
        assert(tracks(v@, v0, gf, a as int, b0 as int));
        if ok {
            assert forall|q: int, r: int| a <= q < r < b0 implies before(lt, #[trigger] v@[q], gf[q], #[trigger] v@[r], gf[r]) by {
                let (pq, pr) = (gf[q], gf[r]);
                if r >= b {
                    assert(!lt(v0[r], v0[m - 1]));
                    if q >= b {
                        assert(before(lt, v0[q], q, v0[r], r));
                    } else if pq >= m {
                        assert(before(lt, v0[pq], pq, v0[r], r));
                    } else {
                        if pq < m - 1 {
                            assert(before(lt, v0[pq], pq, v0[m - 1], m - 1));
                        }
                        if lt(v0[r], v0[pq]) {
                            assert(lt(v0[r], v0[m - 1]) || lt(v0[m - 1], v0[pq]));
                        }
                    }
                } else if q < k && r >= k {
                    if j > 0 {
                        let top = (m + j - 1) as int;
                        if pq < top {
                            assert(before(lt, v0[pq], pq, v0[top], top));
                            lemma_before_trans(lt, v0[pq], pq, v0[top], top, v@[r], pr);
                        }
                    } else if i > a {
                        let top = (i - 1) as int;
                        if pq < top {
                            assert(before(lt, v0[pq], pq, v0[top], top));
                            lemma_before_trans(lt, v0[pq], pq, v0[top], top, v@[r], pr);
                        }
                    }
                } else if q < k && r < k {
                    if j > 0 {
                        assert(before(lt, v0[pq], pq, v0[pr], pr));
                    } else {
                        assert(before(lt, v0[q], q, v0[r], r));
                    }
                }
            }
        }
        assert(sortsc(cx.ok(), cx.ord(), v0, v@, a as int, b0 as int));
    }
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_sorted_id(ok: bool, lt: spec_fn(usize, usize) -> bool, v: Seq<usize>, lo: int, hi: int)
    requires
        0 <= lo <= hi <= v.len(),
        ok ==> sorted_by(lt, v, ident(v.len()), lo, hi),
    ensures
        sortsc(ok, lt, v, v, lo, hi),
{
    let id = ident(v.len());
    assert(is_perm(id, id, lo, hi));
    assert(tracks(v, v, id, lo, hi));
}

} // verus!

/// Reverses `[lo, hi)`.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        lo <= hi <= old(v)@.len(),
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < lo || hi <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        forall|k: int| lo <= k < hi ==> #[trigger] final(v)@[k] == old(v)@[lo + hi - 1 - k],
))]
fn reverse<T>(v: &mut [T], lo: usize, hi: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v0 = v@; }
    let mut i = lo;
    let mut j = hi;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            lo <= i <= j <= hi <= v@.len() == v0.len(),
            i - lo == hi - j,
            forall|k: int| 0 <= k < lo || hi <= k < v@.len() ==> #[trigger] v@[k] == v0[k],
            forall|k: int| i <= k < j ==> #[trigger] v@[k] == v0[k],
            forall|k: int| (lo <= k < i || j <= k < hi) ==> #[trigger] v@[k] == v0[lo + hi - 1 - k],
        decreases j - i,
    ))]
    while i < j && j - i >= 2 {
        swap(v, i, j - 1);
        i += 1;
        j -= 1;
    }
}

/// `[s, m) [m, e)` -> `[m, e) [s, m)`. No comparisons, so any correct rotation matches Zig's block
/// swaps.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        s <= m <= e <= old(v)@.len(),
    ensures
        final(v)@.len() == old(v)@.len(),
        forall|k: int| 0 <= k < s || e <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        forall|k: int| s <= k < s + (e - m) ==> #[trigger] final(v)@[k] == old(v)@[k + (m - s)],
        forall|k: int| s + (e - m) <= k < e ==> #[trigger] final(v)@[k] == old(v)@[k - (e - m)],
))]
fn rotate<T>(v: &mut [T], s: usize, m: usize, e: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v0 = v@; }
    reverse(v, s, m);
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v1 = v@; }
    reverse(v, m, e);
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v2 = v@; }
    reverse(v, s, e);
    #[cfg(verus_keep_ghost)]
    proof! {
        assert forall|k: int| s <= k < s + (e - m) implies #[trigger] v@[k] == v0[k + (m - s)] by {
            let j = s + e - 1 - k;
            assert(v@[k] == v2[j]);
            assert(v2[j] == v1[m + e - 1 - j]);
        }
        assert forall|k: int| s + (e - m) <= k < e implies #[trigger] v@[k] == v0[k - (e - m)] by {
            let j = s + e - 1 - k;
            assert(v@[k] == v2[j]);
            assert(v2[j] == v1[j]);
        }
    }
}

/// Zig's `merge` for one pair of runs, skipped when either side is empty.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a <= m <= b <= old(v)@.len(),
        old(buf)@.len() >= cap || old(buf)@.len() >= old(v)@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b as int),
    decreases b - a, 3nat,
))]
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
        #[cfg(verus_keep_ghost)]
        proof! {
            if cx.ok() {
                let lt = cx.ord();
                assert(sorted_by(lt, v@, ident(v@.len()), a as int, b as int));
            }
            lemma_sorted_id(cx.ok(), cx.ord(), v@, a as int, b as int);
        }
    }
}

/// SymMerge's split: the first `c` in the searched range whose mirror `mid + m - 1 - c` in the right
/// run is less than it (Zig's binary search, comparison for comparison).
#[cfg_attr(verus_keep_ghost, verus_spec(start =>
    requires
        a < m < b <= v@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), v@, ident(v@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), v@, ident(v@.len()), m as int, b as int),
    ensures
        ({
            let mid = a + (b - a) / 2;
            let start0 = if m > mid { m - (b - mid) } else { a as int };
            let hi0 = if m > mid { mid } else { m as int };
            &&& start0 <= start <= hi0
            &&& cx.ok() ==> forall|c: int| start0 <= c < start ==> !cx.ord()(#[trigger] v@[mid + m - 1 - c], v@[c])
            &&& cx.ok() ==> forall|c: int| start <= c < hi0 ==> cx.ord()(#[trigger] v@[mid + m - 1 - c], v@[c])
        }),
))]
fn sym_split(cx: &dyn IdxLess, v: &[usize], a: usize, m: usize, b: usize) -> usize {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost id = ident(v0.len());
    }
    let mid = a + (b - a) / 2;
    let start0 = if m > mid { m - (b - mid) } else { a };
    let hi0 = if m > mid { mid } else { m };
    let mut start = start0;
    let mut hi = hi0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                a < m < b <= v@.len(),
                mid == a + (b - a) / 2,
                start0 == if m > mid { m - (b - mid) } else { a as int },
                hi0 == if m > mid { mid } else { m },
                a <= start0 <= start <= hi <= hi0 <= m,
                v@ == v0,
                ok == cx.ok(),
                ok ==> swo(lt),
                lt == cx.ord(),
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                ok ==> sorted_by(lt, v0, id, a as int, m as int),
                ok ==> sorted_by(lt, v0, id, m as int, b as int),
                ok ==> forall|c: int| start0 <= c < start ==> !lt(#[trigger] v0[mid + m - 1 - c], v0[c]),
                ok ==> forall|c: int| hi <= c < hi0 ==> lt(#[trigger] v0[mid + m - 1 - c], v0[c]),
        decreases hi - start,
    ))]
    while start < hi {
        let c = start + (hi - start) / 2;
        let pc = mid + (m - 1 - c);
        let r = cx.lt(v[pc], v[c]);
        if !r {
            #[cfg(verus_keep_ghost)]
            proof! {
                if ok {
                    assert forall|d: int| start0 <= d < c + 1 implies !lt(#[trigger] v0[mid + m - 1 - d], v0[d]) by {
                        if d < c {
                            let pd = mid + m - 1 - d;
                            assert(before(lt, v0[d], d, v0[c as int], c as int));
                            assert(before(lt, v0[pc as int], pc as int, v0[pd], pd));
                            if lt(v0[pd], v0[d]) {
                                assert(lt(v0[pd], v0[pc as int]) || lt(v0[pc as int], v0[d]));
                                assert(lt(v0[pc as int], v0[c as int]) || lt(v0[c as int], v0[d]));
                            }
                        }
                    }
                }
            }
            start = c + 1;
        } else {
            #[cfg(verus_keep_ghost)]
            proof! {
                if ok {
                    assert forall|d: int| c <= d < hi0 implies lt(#[trigger] v0[mid + m - 1 - d], v0[d]) by {
                        if d > c {
                            let pd = mid + m - 1 - d;
                            assert(before(lt, v0[c as int], c as int, v0[d], d));
                            assert(before(lt, v0[pd], pd, v0[pc as int], pc as int));
                            assert(lt(v0[pc as int], v0[d]) || lt(v0[d], v0[c as int]));
                            assert(lt(v0[pc as int], v0[pd]) || lt(v0[pd], v0[d]));
                        }
                    }
                }
            }
            hi = c;
        }
    }
    start
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_rotated_runs(lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v1: Seq<usize>, a: int, m: int, b: int, mid: int, s: int, e: int)
    requires
        0 <= a <= s <= m <= e <= b <= v0.len() == v1.len(),
        s <= mid <= e,
        e == mid + (m - s),
        forall|k: int| 0 <= k < v0.len() ==> #[trigger] v1[k] == v0[rho(k, s, m, e)],
        sorted_by(lt, v0, ident(v0.len()), a, m),
        sorted_by(lt, v0, ident(v0.len()), m, b),
    ensures
        sorted_by(lt, v1, ident(v1.len()), a, s),
        sorted_by(lt, v1, ident(v1.len()), s, mid),
        sorted_by(lt, v1, ident(v1.len()), mid, e),
        sorted_by(lt, v1, ident(v1.len()), e, b),
{
    let id = ident(v0.len());
    assert forall|k: int, l: int| a <= k < l < s implies before(lt, #[trigger] v1[k], id[k], #[trigger] v1[l], id[l]) by {
        assert(before(lt, v0[k], id[k], v0[l], id[l]));
    }
    assert forall|k: int, l: int| s <= k < l < mid implies before(lt, #[trigger] v1[k], id[k], #[trigger] v1[l], id[l]) by {
        let (p, q) = (k + (m - s), l + (m - s));
        assert(before(lt, v0[p], id[p], v0[q], id[q]));
    }
    assert forall|k: int, l: int| mid <= k < l < e implies before(lt, #[trigger] v1[k], id[k], #[trigger] v1[l], id[l]) by {
        let (p, q) = (k - (e - m), l - (e - m));
        assert(before(lt, v0[p], id[p], v0[q], id[q]));
    }
    assert forall|k: int, l: int| e <= k < l < b implies before(lt, #[trigger] v1[k], id[k], #[trigger] v1[l], id[l]) by {
        assert(before(lt, v0[k], id[k], v0[l], id[l]));
    }
}

proof fn lemma_sorted_frame(lt: spec_fn(usize, usize) -> bool, v1: Seq<usize>, v2: Seq<usize>, lo: int, hi: int)
    requires
        0 <= lo <= hi <= v1.len() == v2.len(),
        forall|k: int| lo <= k < hi ==> #[trigger] v2[k] == v1[k],
        sorted_by(lt, v1, ident(v1.len()), lo, hi),
    ensures
        sorted_by(lt, v2, ident(v2.len()), lo, hi),
{
    let id = ident(v1.len());
    assert forall|k: int, l: int| lo <= k < l < hi implies before(lt, #[trigger] v2[k], id[k], #[trigger] v2[l], id[l]) by {
        assert(before(lt, v1[k], id[k], v1[l], id[l]));
    }
}

} // verus!

/// The rest of SymMerge after the split: rotate, then merge both halves.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < m < b <= old(v)@.len(),
        old(buf)@.len() >= cap || old(buf)@.len() >= old(v)@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b as int),
        ({
            let mid = a + (b - a) / 2;
            let start0 = if m > mid { m - (b - mid) } else { a as int };
            let hi0 = if m > mid { mid } else { m as int };
            &&& start0 <= start <= hi0
            &&& cx.ok() ==> forall|c: int| start0 <= c < start ==> !cx.ord()(#[trigger] old(v)@[mid + m - 1 - c], old(v)@[c])
            &&& cx.ok() ==> forall|c: int| start <= c < hi0 ==> cx.ord()(#[trigger] old(v)@[mid + m - 1 - c], old(v)@[c])
        }),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b as int),
    decreases b - a, 1nat,
))]
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
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost id = ident(v0.len());
    }
    let mid = a + (b - a) / 2;
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost start0 = if m > mid { m - (b - mid) } else { a as int };
        let ghost hi0 = if m > mid { mid as int } else { m as int };
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(m <= mid + (m - start) <= b) by {
            if m > mid {
                assert(start >= m - (b - mid));
            } else {
                assert(m - a <= b - mid);
            }
        }
    }
    let end = mid + (m - start);
    if start < m && m < end {
        rotate(v, start, m, end);
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v1 = v@; }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert forall|k: int| 0 <= k < v0.len() implies #[trigger] v1[k] == v0[rho(k, start as int, m as int, end as int)] by {}
        if ok {
            lemma_rotated_runs(lt, v0, v1, a as int, m as int, b as int, mid as int, start as int, end as int);
        }
    }
    merge_opt(cx, v, buf, a, start, mid, cap);
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v2 = v@; }
    #[cfg(verus_keep_ghost)]
    proof! {
        if ok {
            lemma_sorted_frame(lt, v1, v2, mid as int, end as int);
            lemma_sorted_frame(lt, v1, v2, end as int, b as int);
        }
    }
    merge_opt(cx, v, buf, mid, end, b, cap);
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v3 = v@; }
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_symmerge_compose(cx.ok(), cx.ord(), v0, v1, v2, v3, a as int, m as int, b as int, mid as int, start as int, end as int, start0 as int, hi0 as int);
    }
}

/// Zig's `merge`: in order already, or the shorter-than-buffer side through the buffer, or a
/// SymMerge split (Kim and Kutzner) with a rotation and two smaller merges.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        a < m < b <= old(v)@.len(),
        old(buf)@.len() >= cap || old(buf)@.len() >= old(v)@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), a as int, m as int),
        cx.ok() ==> sorted_by(cx.ord(), old(v)@, ident(old(v)@.len()), m as int, b as int),
    ensures
        final(v)@.len() == old(v)@.len(),
        final(buf)@.len() == old(buf)@.len(),
        forall|k: int| 0 <= k < a || b <= k < final(v)@.len() ==> #[trigger] final(v)@[k] == old(v)@[k],
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, a as int, b as int),
    decreases b - a, 2nat,
))]
fn merge(
    cx: &dyn IdxLess,
    v: &mut [usize],
    buf: &mut [usize],
    a: usize,
    m: usize,
    b: usize,
    cap: usize,
) {
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost v0 = v@;
        let ghost ok = cx.ok();
        let ghost lt = cx.ord();
        let ghost id = ident(v0.len());
    }
    let r = cx.lt(v[m], v[m - 1]);
    if !r {
        #[cfg(verus_keep_ghost)]
        proof! {
            if ok {
                assert forall|k: int, l: int| a <= k < l < b implies before(lt, #[trigger] v0[k], id[k], #[trigger] v0[l], id[l]) by {
                    if k < m && l >= m {
                        let km = m - 1;
                        assert(before(lt, v0[km], km, v0[m as int], m as int));
                        if k < km {
                            lemma_before_trans(lt, v0[k], k, v0[km], km, v0[m as int], m as int);
                        }
                        if l > m {
                            lemma_before_trans(lt, v0[k], k, v0[m as int], m as int, v0[l], l);
                        }
                    }
                }
            }
            lemma_sorted_id(cx.ok(), cx.ord(), v0, a as int, b as int);
        }
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

#[cfg(verus_keep_ghost)]
verus! {

/// Where position `k` of the rotated array came from (rotation of `[s, m) [m, e)`).
pub open spec fn rho(k: int, s: int, m: int, e: int) -> int {
    if s <= k < s + (e - m) {
        k + (m - s)
    } else if s + (e - m) <= k < e {
        k - (e - m)
    } else {
        k
    }
}

pub open spec fn rho_inv(o: int, s: int, m: int, e: int) -> int {
    if m <= o < e {
        o - (m - s)
    } else if s <= o < m {
        o + (e - m)
    } else {
        o
    }
}

proof fn lemma_symmerge_compose(
    ok: bool, lt: spec_fn(usize, usize) -> bool,
    v0: Seq<usize>,
    v1: Seq<usize>,
    v2: Seq<usize>,
    v3: Seq<usize>,
    a: int,
    m: int,
    b: int,
    mid: int,
    s: int,
    e: int,
    start0: int,
    hi0: int,
)
    requires
        ok ==> swo(lt),
        0 <= a < m < b <= v0.len(),
        v1.len() == v0.len() && v2.len() == v0.len() && v3.len() == v0.len(),
        mid == a + (b - a) / 2,
        start0 == if m > mid { m - (b - mid) } else { a },
        hi0 == if m > mid { mid } else { m },
        a <= start0 <= s <= hi0 <= m,
        e == mid + (m - s),
        m <= e <= b,
        s <= mid <= e,
        forall|k: int| 0 <= k < v0.len() ==> #[trigger] v1[k] == v0[rho(k, s, m, e)],
        forall|k: int| 0 <= k < a || mid <= k < v1.len() ==> #[trigger] v2[k] == v1[k],
        forall|k: int| 0 <= k < mid || b <= k < v2.len() ==> #[trigger] v3[k] == v2[k],
        sortsc(ok, lt, v1, v2, a, mid),
        sortsc(ok, lt, v2, v3, mid, b),
        ok ==> sorted_by(lt, v0, ident(v0.len()), a, m),
        ok ==> sorted_by(lt, v0, ident(v0.len()), m, b),
        ok ==> forall|c: int| start0 <= c < s ==> !lt(#[trigger] v0[mid + m - 1 - c], v0[c]),
        ok ==> forall|c: int| s <= c < hi0 ==> lt(#[trigger] v0[mid + m - 1 - c], v0[c]),
    ensures
        sortsc(ok, lt, v0, v3, a, b),
{
    let (g1, i1) = choose|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, a, mid)
            &&& tracks(v2, v1, g, a, mid)
            &&& ok ==> sorted_by(lt, v2, g, a, mid)
        };
    let (g2, i2) = choose|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, mid, b)
            &&& tracks(v3, v2, g, mid, b)
            &&& ok ==> sorted_by(lt, v3, g, mid, b)
        };
    let gg = Seq::new(v0.len(), |q: int| if a <= q < mid { rho(g1[q], s, m, e) } else if mid <= q < b { rho(g2[q], s, m, e) } else { q });
    let ii = Seq::new(v0.len(), |o: int| if a <= o < b { if rho_inv(o, s, m, e) < mid { i1[rho_inv(o, s, m, e)] } else { i2[rho_inv(o, s, m, e)] } } else { o });
    assert forall|q: int| a <= q < b implies a <= #[trigger] gg[q] < b && ii[gg[q]] == q by {
        if q < mid {
            assert(a <= g1[q] < mid);
        } else {
            assert(mid <= g2[q] < b);
        }
    }
    assert forall|o: int| a <= o < b implies a <= #[trigger] ii[o] < b && gg[ii[o]] == o by {
        let r = rho_inv(o, s, m, e);
        assert(a <= r < b);
        if r < mid {
            assert(a <= i1[r] < mid && g1[i1[r]] == r);
        } else {
            assert(mid <= i2[r] < b && g2[i2[r]] == r);
        }
    }
    assert(is_perm(gg, ii, a, b));
    assert forall|q: int| a <= q < b implies #[trigger] v3[q] == v0[gg[q]] by {
        if q < mid {
            assert(v3[q] == v2[q]);
            assert(v2[q] == v1[g1[q]]);
        } else {
            assert(v3[q] == v2[g2[q]]);
            assert(v2[g2[q]] == v1[g2[q]]);
        }
    }
    assert(tracks(v3, v0, gg, a, b));
    if ok {
        assert forall|q: int, r: int| a <= q < r < b implies before(lt, #[trigger] v3[q], gg[q], #[trigger] v3[r], gg[r]) by {
            let (x, y) = (v3[q], v3[r]);
            let (gq, gr) = (gg[q], gg[r]);
            if r < mid {
                assert(before(lt, v2[q], g1[q], v2[r], g1[r]));
            } else if q >= mid {
                assert(before(lt, v3[q], g2[q], v3[r], g2[r]));
            } else {
                assert(a <= g1[q] < mid && mid <= g2[r] < b);
                if gq < s && gr < m {
                    assert(before(lt, v0[gq], gq, v0[gr], gr));
                } else if gq >= m && gr >= e {
                    assert(before(lt, v0[gq], gq, v0[gr], gr));
                } else if gq < s && gr >= e {
                    let c = s - 1;
                    assert(start0 <= c);
                    assert(!lt(v0[mid + m - 1 - c], v0[c]));
                    assert(mid + m - 1 - c == e);
                    if gq < c {
                        assert(before(lt, v0[gq], gq, v0[c], c));
                    }
                    if gr > e {
                        assert(before(lt, v0[e], e, v0[gr], gr));
                    }
                    if lt(y, x) {
                        assert(lt(y, v0[e]) || lt(v0[e], x));
                        assert(lt(v0[e], v0[c]) || lt(v0[c], x));
                    }
                } else {
                    let c = s;
                    assert(c < hi0);
                    assert(lt(v0[mid + m - 1 - c], v0[c]));
                    assert(mid + m - 1 - c == e - 1);
                    if gq < e - 1 {
                        assert(before(lt, v0[gq], gq, v0[e - 1], e - 1));
                    }
                    if gr > c {
                        assert(before(lt, v0[c], c, v0[gr], gr));
                    }
                    assert(lt(v0[e - 1], y) || lt(y, v0[c]));
                    assert(lt(v0[e - 1], x) || lt(x, y));
                }
            }
        }
    }
}

pub open spec fn min_int(x: int, y: int) -> int {
    if x < y { x } else { y }
}

/// `[lo, hi)` is one sorted run: a permutation of its own range, in stable order.
pub open spec fn run_ok(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, lo: int, hi: int) -> bool {
    &&& is_perm(g, inv, lo, hi)
    &&& tracks(v, v0, g, lo, hi)
    &&& ok ==> sorted_by(lt, v, g, lo, hi)
}

/// The runs of width `w` starting at `f`, `f + w`, ... below `t` (each ending at `n` at the latest).
pub open spec fn runs(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, f: int, t: int, w: int, n: int) -> bool
    decreases if f < t { t - f } else { 0 },
{
    if w <= 0 {
        false
    } else if f >= t {
        true
    } else {
        run_ok(ok, lt, v0, v, g, inv, f, min_int(f + w, n)) && runs(ok, lt, v0, v, g, inv, f + w, t, w, n)
    }
}

proof fn lemma_run_frame(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, v2: Seq<usize>, g2: Seq<int>, inv2: Seq<int>, lo: int, hi: int)
    requires
        run_ok(ok, lt, v0, v, g, inv, lo, hi),
        g2.len() == g.len() && inv2.len() == inv.len(),
        forall|k: int| lo <= k < hi ==> #[trigger] v2[k] == v[k],
        forall|k: int| lo <= k < hi ==> #[trigger] g2[k] == g[k],
        forall|k: int| lo <= k < hi ==> #[trigger] inv2[k] == inv[k],
    ensures
        run_ok(ok, lt, v0, v2, g2, inv2, lo, hi),
{
    if ok {
        assert forall|k: int, l: int| lo <= k < l < hi implies before(lt, #[trigger] v2[k], g2[k], #[trigger] v2[l], g2[l]) by {
            assert(before(lt, v[k], g[k], v[l], g[l]));
        }
    }
}

/// Runs inside `[lo, hi)` survive changes outside it. `t == n` or `t` is reached exactly (`t == f + r * w`).
proof fn lemma_runs_frame(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, v2: Seq<usize>, g2: Seq<int>, inv2: Seq<int>, f: int, t: int, w: int, n: int, r: nat, lo: int, hi: int)
    requires
        runs(ok, lt, v0, v, g, inv, f, t, w, n),
        w > 0,
        lo <= f,
        t <= n <= hi || (t == f + r * w && t <= hi),
        g2.len() == g.len() && inv2.len() == inv.len(),
        forall|k: int| lo <= k < hi ==> #[trigger] v2[k] == v[k],
        forall|k: int| lo <= k < hi ==> #[trigger] g2[k] == g[k],
        forall|k: int| lo <= k < hi ==> #[trigger] inv2[k] == inv[k],
    ensures
        runs(ok, lt, v0, v2, g2, inv2, f, t, w, n),
    decreases if f < t { t - f } else { 0 },
{
    if f < t {
        let e = min_int(f + w, n);
        if !(t <= n <= hi) {
            assert(r >= 1) by {
                if r == 0 {
                    assert(t == f);
                }
            }
            assert(t == (f + w) + (r - 1) * w) by (nonlinear_arith)
                requires
                    t == f + r * w,
                    r >= 1,
            ;
            assert(f + w <= t) by (nonlinear_arith)
                requires
                    t == f + r * w,
                    r >= 1,
                    w > 0,
            ;
        }
        lemma_run_frame(ok, lt, v0, v, g, inv, v2, g2, inv2, f, e);
        lemma_runs_frame(ok, lt, v0, v, g, inv, v2, g2, inv2, f + w, t, w, n, (if r > 0 { r - 1 } else { 0 }) as nat, lo, hi);
    }
}

/// Appending the run `[a, min(a + w, n))` to the runs `[f, a)` (with `a == f + r * w`).
proof fn lemma_runs_append(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, f: int, a: int, w: int, n: int, r: nat)
    requires
        w > 0,
        a == f + r * w,
        f <= a < n,
        runs(ok, lt, v0, v, g, inv, f, a, w, n),
        run_ok(ok, lt, v0, v, g, inv, a, min_int(a + w, n)),
    ensures
        runs(ok, lt, v0, v, g, inv, f, min_int(a + w, n), w, n),
    decreases r,
{
    if r == 0 {
        assert(a == f) by (nonlinear_arith)
            requires
                a == f + r * w,
                r == 0,
        ;
        assert(runs(ok, lt, v0, v, g, inv, f + w, min_int(a + w, n), w, n));
    } else {
        assert(a == (f + w) + (r - 1) * w) by (nonlinear_arith)
            requires
                a == f + r * w,
                r >= 1,
        ;
        assert(f + w <= a) by (nonlinear_arith)
            requires
                a == f + r * w,
                r >= 1,
                w > 0,
        ;
        lemma_runs_append(ok, lt, v0, v, g, inv, f + w, a, w, n, (r - 1) as nat);
    }
}

/// Merging two adjacent sorted runs `[a, m)`, `[m, b)` of the global state gives one sorted run.
proof fn lemma_merge_runs(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, v2: Seq<usize>, a: int, m: int, b: int) -> (r: (Seq<int>, Seq<int>))
    requires
        ok ==> swo(lt),
        0 <= a < m < b <= v.len(),
        v.len() == v2.len() == g.len() == inv.len(),
        run_ok(ok, lt, v0, v, g, inv, a, m),
        run_ok(ok, lt, v0, v, g, inv, m, b),
        sortsc(ok, lt, v, v2, a, b),
        forall|k: int| 0 <= k < a || b <= k < v.len() ==> #[trigger] v2[k] == v[k],
    ensures
        r.0.len() == g.len() && r.1.len() == inv.len(),
        run_ok(ok, lt, v0, v2, r.0, r.1, a, b),
        forall|k: int| 0 <= k < a || b <= k < v.len() ==> #[trigger] r.0[k] == g[k],
        forall|k: int| 0 <= k < a || b <= k < v.len() ==> #[trigger] r.1[k] == inv[k],
{
    let (gm, im) = choose|gg: Seq<int>, ii: Seq<int>|
        {
            &&& #[trigger] is_perm(gg, ii, a, b)
            &&& tracks(v2, v, gg, a, b)
            &&& ok ==> sorted_by(lt, v2, gg, a, b)
        };
    let g2 = Seq::new(g.len(), |k: int| if a <= k < b { g[gm[k]] } else { g[k] });
    let inv2 = Seq::new(inv.len(), |o: int| if a <= o < b { im[inv[o]] } else { inv[o] });
    assert forall|k: int| a <= k < b implies a <= #[trigger] g2[k] < b && inv2[g2[k]] == k by {
        let p = gm[k];
        if p < m {
            assert(a <= g[p] < m);
        } else {
            assert(m <= g[p] < b);
        }
    }
    assert forall|o: int| a <= o < b implies a <= #[trigger] inv2[o] < b && g2[inv2[o]] == o by {
        if o < m {
            assert(a <= inv[o] < m);
        } else {
            assert(m <= inv[o] < b);
        }
    }
    assert(is_perm(g2, inv2, a, b));
    assert forall|k: int| a <= k < b implies #[trigger] v2[k] == v0[g2[k]] by {
        assert(v2[k] == v[gm[k]]);
    }
    if ok {
        assert forall|k: int, l: int| a <= k < l < b implies before(lt, #[trigger] v2[k], g2[k], #[trigger] v2[l], g2[l]) by {
            assert(before(lt, v2[k], gm[k], v2[l], gm[l]));
            if !lt(v2[k], v2[l]) {
                let (p, q) = (gm[k], gm[l]);
                if p < m && q < m {
                    assert(before(lt, v[p], g[p], v[q], g[q]));
                } else if p >= m && q >= m {
                    assert(before(lt, v[p], g[p], v[q], g[q]));
                } else if p < m {
                    assert(g[p] < m <= g[q]);
                }
            }
        }
    }
    (g2, inv2)
}

proof fn lemma_run_ident(ok: bool, lt: spec_fn(usize, usize) -> bool, v0: Seq<usize>, v: Seq<usize>, g: Seq<int>, inv: Seq<int>, lo: int, hi: int)
    requires
        run_ok(ok, lt, v0, v, g, inv, lo, hi),
        hi <= v.len(),
    ensures
        ok ==> sorted_by(lt, v, ident(v.len()), lo, hi),
{
    if ok {
        assert forall|k: int, l: int| lo <= k < l < hi implies before(lt, #[trigger] v[k], ident(v.len())[k], #[trigger] v[l], ident(v.len())[l]) by {
            assert(before(lt, v[k], g[k], v[l], g[l]));
        }
    }
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_verify)]
pub const BLOCK: usize = 20;

/// Zig's `sortBytes`: binary insertion on blocks of 20, then bottom-up merging; `cap` is how many
/// elements Zig's 4 KiB merge buffer holds.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        old(buf)@.len() >= cap || old(buf)@.len() >= old(v)@.len(),
        cx.callable(),
        cx.ok() ==> swo(cx.ord()),
    ensures
        final(v)@.len() == old(v)@.len(),
        sortsc(cx.ok(), cx.ord(), old(v)@, final(v)@, 0, old(v)@.len() as int),
))]
fn sort_runs(cx: &dyn IdxLess, v: &mut [usize], buf: &mut [usize], cap: usize) {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v0 = v@; }
    let n = v.len();
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost mut g = ident(n as nat);
        let ghost mut inv = ident(n as nat);
    }
    let mut a: usize = 0;
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost mut r: nat = 0; }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                n == v@.len() == v0.len() == g.len() == inv.len(),
                buf@.len() == old(buf)@.len(),
                a <= n,
                a == r * BLOCK || a == n,
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                forall|k: int| a <= k < n ==> #[trigger] v@[k] == v0[k],
                forall|k: int| a <= k < n ==> #[trigger] g[k] == k,
                forall|k: int| a <= k < n ==> #[trigger] inv[k] == k,
                runs(cx.ok(), cx.ord(), v0, v@, g, inv, 0, a as int, BLOCK as int, n as int),
        decreases n - a,
    ))]
    while a < n {
        let b = if n - a > BLOCK { a + BLOCK } else { n };
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost vb = v@;
            let ghost (gb, ib) = (g, inv);
        }
        insertion(cx, v, a, b);
        #[cfg(verus_keep_ghost)]
        proof! {
            let (gm, im) = choose|gg: Seq<int>, ii: Seq<int>|
                {
                    &&& #[trigger] is_perm(gg, ii, a as int, b as int)
                    &&& tracks(v@, vb, gg, a as int, b as int)
                    &&& cx.ok() ==> sorted_by(cx.ord(), v@, gg, a as int, b as int)
                };
            g = Seq::new(n as nat, |k: int| if a <= k < b { gm[k] } else { gb[k] });
            inv = Seq::new(n as nat, |k: int| if a <= k < b { im[k] } else { ib[k] });
            assert(run_ok(cx.ok(), cx.ord(), v0, v@, g, inv, a as int, b as int)) by {
                if cx.ok() {
                    let lt = cx.ord();
                    assert forall|k: int, l: int| a <= k < l < b implies before(lt, #[trigger] v@[k], g[k], #[trigger] v@[l], g[l]) by {
                        assert(before(lt, v@[k], gm[k], v@[l], gm[l]));
                    }
                }
            }
            lemma_runs_frame(cx.ok(), cx.ord(), v0, vb, gb, ib, v@, g, inv, 0, a as int, BLOCK as int, n as int, r, 0, a as int);
            assert(min_int(a + BLOCK, n as int) == b);
            lemma_runs_append(cx.ok(), cx.ord(), v0, v@, g, inv, 0, a as int, BLOCK as int, n as int, r);
            if b < n {
                assert(b == (r + 1) * BLOCK) by (nonlinear_arith)
                    requires
                        a == r * BLOCK,
                        b == a + BLOCK,
                ;
            }
            r = r + 1;
        }
        a = b;
    }
    let mut width: usize = BLOCK;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
                n == v@.len() == v0.len() == g.len() == inv.len(),
                buf@.len() == old(buf)@.len(),
                buf@.len() >= cap || buf@.len() >= n,
                width >= 1,
                cx.callable(),
            cx.ok() ==> swo(cx.ord()),
                runs(cx.ok(), cx.ord(), v0, v@, g, inv, 0, n as int, width as int, n as int),
        decreases n - width,
    ))]
    while width < n {
        let mut a: usize = 0;
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost mut r: nat = 0; }
        #[cfg(verus_keep_ghost)]
        proof! { assert(runs(cx.ok(), cx.ord(), v0, v@, g, inv, 0, 0, 2 * width as int, n as int)); }
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                        n == v@.len() == v0.len() == g.len() == inv.len(),
                        buf@.len() == old(buf)@.len(),
                        buf@.len() >= cap || buf@.len() >= n,
                        1 <= width < n,
                        a <= n,
                        a == r * (2 * width) || a == n,
                        cx.callable(),
                cx.ok() ==> swo(cx.ord()),
                        runs(cx.ok(), cx.ord(), v0, v@, g, inv, 0, a as int, 2 * width as int, n as int),
                        runs(cx.ok(), cx.ord(), v0, v@, g, inv, a as int, n as int, width as int, n as int),
            decreases n - a,
        ))]
        while a < n && n - a > width {
            let m = a + width;
            let b = if n - m > width { m + width } else { n };
            #[cfg(verus_keep_ghost)]
            proof_decl! {
                let ghost vb = v@;
                let ghost (gb, ib) = (g, inv);
            }
            #[cfg(verus_keep_ghost)]
            proof! {
                assert(min_int(a + width, n as int) == m);
                assert(run_ok(cx.ok(), cx.ord(), v0, vb, gb, ib, a as int, m as int));
                assert(runs(cx.ok(), cx.ord(), v0, vb, gb, ib, m as int, n as int, width as int, n as int));
                assert(min_int(m + width, n as int) == b);
                assert(run_ok(cx.ok(), cx.ord(), v0, vb, gb, ib, m as int, b as int));
                assert(runs(cx.ok(), cx.ord(), v0, vb, gb, ib, m + width, n as int, width as int, n as int));
                lemma_run_ident(cx.ok(), cx.ord(), v0, vb, gb, ib, a as int, m as int);
                lemma_run_ident(cx.ok(), cx.ord(), v0, vb, gb, ib, m as int, b as int);
            }
            merge(cx, v, buf, a, m, b, cap);
            #[cfg(verus_keep_ghost)]
            proof! {
                let (g2, i2) = lemma_merge_runs(cx.ok(), cx.ord(), v0, vb, gb, ib, v@, a as int, m as int, b as int);
                g = g2;
                inv = i2;
                lemma_runs_frame(cx.ok(), cx.ord(), v0, vb, gb, ib, v@, g, inv, 0, a as int, 2 * width as int, n as int, r, 0, a as int);
                assert(min_int(a + 2 * width, n as int) == b);
                lemma_runs_append(cx.ok(), cx.ord(), v0, v@, g, inv, 0, a as int, 2 * width as int, n as int, r);
                if m + width < n {
                    lemma_runs_frame(cx.ok(), cx.ord(), v0, vb, gb, ib, v@, g, inv, m + width, n as int, width as int, n as int, 0, b as int, n as int);
                    assert(b == (r + 1) * (2 * width)) by (nonlinear_arith)
                        requires
                            a == r * (2 * width),
                            b == a + 2 * width,
                    ;
                } else {
                    assert(b == n);
                }
                r = r + 1;
            }
            a = b;
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            if a < n {
                assert(run_ok(cx.ok(), cx.ord(), v0, v@, g, inv, a as int, min_int(a + width, n as int)));
                assert(min_int(a + width, n as int) == n);
                assert(min_int(a + 2 * width, n as int) == n);
                lemma_runs_append(cx.ok(), cx.ord(), v0, v@, g, inv, 0, a as int, 2 * width as int, n as int, r);
            }
        }
        if width >= n - width {
            #[cfg(verus_keep_ghost)]
            proof! {
                assert(runs(cx.ok(), cx.ord(), v0, v@, g, inv, 0, n as int, 2 * width as int, n as int));
                assert(min_int(2 * width, n as int) == n);
                assert(run_ok(cx.ok(), cx.ord(), v0, v@, g, inv, 0, n as int));
            }
            width = n;
            #[cfg(verus_keep_ghost)]
            proof! {
                assert(runs(cx.ok(), cx.ord(), v0, v@, g, inv, width as int, n as int, width as int, n as int));
            }
        } else {
            width = width * 2;
        }
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        if n > 0 {
            assert(min_int(width as int, n as int) == n);
            assert(run_ok(cx.ok(), cx.ord(), v0, v@, g, inv, 0, n as int));
        } else {
            assert(is_perm(g, inv, 0, 0));
        }
        assert(sortsc(cx.ok(), cx.ord(), v0, v@, 0, n as int));
    }
}

#[cfg(verus_keep_ghost)]
verus! {

pub open spec fn sortsc(ok: bool, lt: spec_fn(usize, usize) -> bool, old: Seq<usize>, new: Seq<usize>, a: int, b: int) -> bool {
    exists|g: Seq<int>, inv: Seq<int>|
        {
            &&& #[trigger] is_perm(g, inv, a, b)
            &&& tracks(new, old, g, a, b)
            &&& ok ==> sorted_by(lt, new, g, a, b)
        }
}

} // verus!

/// `less(items[x], items[y])`, out-of-range indices last (they never occur: every index is a
/// position of `items`).
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        forall|a: &E, b: &E| #[trigger] less.requires((a, b)),
    ensures
        well_behaved(*less) ==> r == ilt(*less, items@)(x, y),
))]
fn cmp<E, F: Fn(&E, &E) -> bool>(items: &[E], less: &F, x: usize, y: usize) -> bool {
    if x < items.len() {
        if y < items.len() {
            let r = less(&items[x], &items[y]);
            #[cfg(verus_keep_ghost)]
            proof! {
                if well_behaved(*less) {
                    lemma_less(*less, items@[x as int], items@[y as int], r);
                }
            }
            r
        } else {
            true
        }
    } else {
        false
    }
}

#[cfg(verus_keep_ghost)]
verus! {

/// The index phase's comparator, behind `dyn` so that phase is compiled once for all element types
/// (Zig's `sortBytes` takes a function pointer for the same reason).
trait IdxLess {
    spec fn callable(&self) -> bool;

    spec fn ok(&self) -> bool;

    spec fn ord(&self) -> spec_fn(usize, usize) -> bool;

    fn lt(&self, x: usize, y: usize) -> (r: bool)
        requires
            self.callable(),
        ensures
            self.ok() ==> r == self.ord()(x, y),
    ;
}

} // verus!

/// The plain-build view of `IdxLess` (the Verus build above adds its specification).
#[cfg(not(verus_keep_ghost))]
trait IdxLess {
    fn lt(&self, x: usize, y: usize) -> bool;
}

#[cfg_attr(verus_keep_ghost, verus_verify)]
struct IdxCmp<'a, E, F: Fn(&E, &E) -> bool> {
    items: &'a [E],
    less: &'a F,
}

#[cfg(verus_keep_ghost)]
verus! {

impl<'a, E, F: Fn(&E, &E) -> bool> IdxLess for IdxCmp<'a, E, F> {
    spec fn callable(&self) -> bool {
        forall|a: &E, b: &E| #[trigger] self.less.requires((a, b))
    }

    spec fn ok(&self) -> bool {
        well_behaved(*self.less)
    }

    spec fn ord(&self) -> spec_fn(usize, usize) -> bool {
        ilt(*self.less, self.items@)
    }

    fn lt(&self, x: usize, y: usize) -> (r: bool) {
        cmp(self.items, self.less, x, y)
    }
}

} // verus!

#[cfg(not(verus_keep_ghost))]
impl<'a, E, F: Fn(&E, &E) -> bool> IdxLess for IdxCmp<'a, E, F> {
    fn lt(&self, x: usize, y: usize) -> bool {
        cmp(self.items, self.less, x, y)
    }
}

/// `items[k] = old items[idx[k]]` for a permutation `idx`, by swaps (any `T`).
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        idx@.len() == old(items)@.len(),
        forall|k: int| 0 <= k < idx@.len() ==> (#[trigger] idx@[k] as int) < idx@.len(),
        forall|k: int, l: int| 0 <= k < l < idx@.len() ==> #[trigger] idx@[k] != #[trigger] idx@[l],
    ensures
        final(items)@.len() == old(items)@.len(),
        forall|k: int| 0 <= k < idx@.len() ==> #[trigger] final(items)@[k] == old(items)@[idx@[k] as int],
))]
fn apply_perm<T>(items: &mut [T], idx: &[usize]) {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v0 = items@; }
    let n = items.len();
    let mut at_v: Vec<usize> = alloc::vec![0usize; n];
    let mut pos_v: Vec<usize> = alloc::vec![0usize; n];
    let at = at_v.as_mut_slice();
    let pos = pos_v.as_mut_slice();
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i <= n == items@.len() == at@.len() == pos@.len(),
            items@ == v0,
            forall|q: int| 0 <= q < i ==> #[trigger] at@[q] == q,
            forall|q: int| 0 <= q < i ==> #[trigger] pos@[q] == q,
        decreases n - i,
    ))]
    while i < n {
        at[i] = i;
        pos[i] = i;
        i += 1;
    }
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= n == items@.len() == v0.len() == idx@.len() == at@.len() == pos@.len(),
            forall|q: int| 0 <= q < n ==> (#[trigger] idx@[q] as int) < n,
            forall|q: int, l: int| 0 <= q < l < n ==> #[trigger] idx@[q] != #[trigger] idx@[l],
            forall|q: int| 0 <= q < n ==> (#[trigger] at@[q] as int) < n && pos@[at@[q] as int] == q,
            forall|q: int| 0 <= q < n ==> (#[trigger] pos@[q] as int) < n && at@[pos@[q] as int] == q,
            forall|q: int| 0 <= q < n ==> #[trigger] items@[q] == v0[at@[q] as int],
            forall|q: int| 0 <= q < k ==> #[trigger] at@[q] == idx@[q],
        decreases n - k,
    ))]
    while k < n {
        let e = idx[k];
        let p = pos[e];
        #[cfg(verus_keep_ghost)]
        proof! {
            if (p as int) < k {
                assert(at@[p as int] == idx@[p as int]);
                assert(at@[p as int] == e);
            }
        }
        if p != k {
            #[cfg(verus_keep_ghost)]
            proof_decl! { let ghost (at0, pos0, it0) = (at@, pos@, items@); }
            swap(items, k, p);
            let f = at[k];
            at[p] = f;
            pos[f] = p;
            at[k] = e;
            pos[e] = k;
            #[cfg(verus_keep_ghost)]
            proof! {
                assert forall|q: int| 0 <= q < n implies (#[trigger] at@[q] as int) < n && pos@[at@[q] as int] == q by {
                    if q != k && q != p {
                        assert(at@[q] == at0[q]);
                        assert(at0[q] != f && at0[q] != e);
                    }
                }
                assert forall|q: int| 0 <= q < n implies (#[trigger] pos@[q] as int) < n && at@[pos@[q] as int] == q by {
                    if q != f as int && q != e as int {
                        assert(pos@[q] == pos0[q]);
                        assert(pos0[q] != k && pos0[q] != p);
                    }
                }
                assert forall|q: int| 0 <= q < n implies #[trigger] items@[q] == v0[at@[q] as int] by {
                    if q != k && q != p {
                        assert(items@[q] == it0[q]);
                    }
                }
            }
        }
        k += 1;
    }
}

/// The one stable sort of the engine: exactly Zig's `sort.stable` (binary insertion on blocks of 20,
/// bottom-up merging through a 4 KiB buffer, SymMerge with rotations when both runs are longer),
/// comparison for comparison, so its result equals Zig's for every comparator, not only strict weak
/// orders. Up to 20 elements sort in place; longer slices sort an index array and then move the
/// elements once. Proven: always a permutation, never panics, terminates; sorted and stable when
/// `less` is a strict weak ordering.
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        forall|a: &T, b: &T| #[trigger] less.requires((a, b)),
    ensures
        final(items)@.len() == old(items)@.len(),
        sorts(less, old(items)@, final(items)@, 0, old(items)@.len() as int),
))]
pub fn stable<T, F: Fn(&T, &T) -> bool>(items: &mut [T], less: F) {
    let n = items.len();
    if n <= BLOCK {
        if n < 2 {
            #[cfg(verus_keep_ghost)]
            proof! {
                let g = ident(n as nat);
                assert(is_perm(g, g, 0, n as int));
                assert(tracks(items@, items@, g, 0, n as int));
            }
            return;
        }
        insertion_swap(items, 0, n, &less);
        return;
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost v0 = items@; }
    let size = core::mem::size_of::<T>();
    let cap = if size == 0 { 4096 } else { 4096 / size };
    let mut idx: Vec<usize> = alloc::vec![0usize; n];
    {
        let ix = idx.as_mut_slice();
        let mut i: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                i <= n == ix@.len(),
                forall|q: int| 0 <= q < i ==> #[trigger] ix@[q] == q,
            decreases n - i,
        ))]
        while i < n {
            ix[i] = i;
            i += 1;
        }
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost idx0 = idx@; }
    let blen = if cap < n { cap } else { n };
    let mut buf: Vec<usize> = alloc::vec![0usize; blen];
    let ic = IdxCmp {
        items: &*items,
        less: &less,
    };
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_ilt_swo(less, v0);
    }
    sort_runs(&ic, idx.as_mut_slice(), buf.as_mut_slice(), cap);
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost idxf = idx@; }
    #[cfg(verus_keep_ghost)]
    proof! {
        let (g, inv) = choose|g: Seq<int>, inv: Seq<int>|
            {
                &&& #[trigger] is_perm(g, inv, 0, n as int)
                &&& tracks(idxf, idx0, g, 0, n as int)
                &&& well_behaved(less) ==> sorted_by(ilt(less, v0), idxf, g, 0, n as int)
            };
        assert forall|k: int| 0 <= k < n implies (#[trigger] idxf[k] as int) < n by {
            assert(idxf[k] == idx0[g[k]]);
        }
        assert forall|k: int, l: int| 0 <= k < l < n implies #[trigger] idxf[k] != #[trigger] idxf[l] by {
            assert(idxf[k] == g[k] && idxf[l] == g[l]);
            assert(inv[g[k]] == k && inv[g[l]] == l);
        }
    }
    apply_perm(items, idx.as_slice());
    #[cfg(verus_keep_ghost)]
    proof! {
        let (g, inv) = choose|g: Seq<int>, inv: Seq<int>|
            {
                &&& #[trigger] is_perm(g, inv, 0, n as int)
                &&& tracks(idxf, idx0, g, 0, n as int)
                &&& well_behaved(less) ==> sorted_by(ilt(less, v0), idxf, g, 0, n as int)
            };
        assert forall|k: int| 0 <= k < n implies #[trigger] items@[k] == v0[g[k]] by {
            assert(idxf[k] == idx0[g[k]]);
        }
        if well_behaved(less) {
            let lt = lt_of(less);
            assert forall|k: int, l: int| 0 <= k < l < n implies before(lt, #[trigger] items@[k], g[k], #[trigger] items@[l], g[l]) by {
                assert(before(ilt(less, v0), idxf[k], g[k], idxf[l], g[l]));
                assert(idxf[k] == g[k] && idxf[l] == g[l]);
            }
        }
        assert(sorts(less, v0, items@, 0, n as int));
    }
}
