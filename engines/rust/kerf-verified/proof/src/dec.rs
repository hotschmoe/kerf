use alloc::vec::Vec;
use vstd::arithmetic::div_mod::*;
use vstd::arithmetic::mul::*;
use vstd::prelude::*;

verus! {

pub open spec fn is_digit(c: u8) -> bool {
    48 <= c <= 57
}

pub open spec fn all_digits(s: Seq<u8>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> is_digit(#[trigger] s[i])
}

/// The value a reader assigns to a run of ASCII digits.
pub open spec fn digits_val(s: Seq<u8>) -> nat
    decreases s.len(),
{
    if s.len() == 0 {
        0
    } else {
        digits_val(s.drop_last()) * 10 + (s.last() - 48) as nat
    }
}

pub open spec fn digit(d: nat) -> u8 {
    (48 + d % 10) as u8
}

/// The last `w` decimal digits of `n`, zero padded.
pub open spec fn dec_fixed(n: nat, w: nat) -> Seq<u8>
    decreases w,
{
    if w == 0 {
        Seq::empty()
    } else {
        dec_fixed(n / 10, (w - 1) as nat).push(digit(n))
    }
}

pub open spec fn pow10(k: nat) -> nat
    decreases k,
{
    if k == 0 {
        1
    } else {
        10 * pow10((k - 1) as nat)
    }
}

/// Shortest decimal form of `n`, as `{d}` prints it.
pub open spec fn dec(n: nat) -> Seq<u8>
    decreases n,
{
    if n < 10 {
        seq![digit(n)]
    } else {
        dec(n / 10).push(digit(n))
    }
}

pub proof fn lemma_dec_fixed(n: nat, w: nat)
    ensures
        dec_fixed(n, w).len() == w,
        all_digits(dec_fixed(n, w)),
        digits_val(dec_fixed(n, w)) == n % pow10(w),
    decreases w,
{
    if w > 0 {
        let s = dec_fixed(n, w);
        lemma_dec_fixed(n / 10, (w - 1) as nat);
        assert(s.drop_last() =~= dec_fixed(n / 10, (w - 1) as nat));
        lemma_pow10_pos((w - 1) as nat);
        let p = pow10((w - 1) as nat);
        lemma_mod_breakdown(n as int, 10, p as int);
        lemma_mul_is_commutative((n / 10 % (p as nat)) as int, 10);
    }
}

pub proof fn lemma_dec(n: nat)
    ensures
        dec(n).len() >= 1,
        all_digits(dec(n)),
        digits_val(dec(n)) == n,
    decreases n,
{
    if n >= 10 {
        lemma_dec(n / 10);
        let s = dec(n);
        assert(s.drop_last() =~= dec(n / 10));
        lemma_fundamental_div_mod(n as int, 10);
        lemma_mul_is_commutative(10, (n / 10) as int);
        assert(s.last() == digit(n));
    } else {
        assert(dec(n).drop_last() =~= Seq::<u8>::empty());
        reveal_with_fuel(digits_val, 2);
    }
}

pub proof fn lemma_pow10_pos(k: nat)
    ensures
        pow10(k) >= 1,
    decreases k,
{
    if k > 0 {
        lemma_pow10_pos((k - 1) as nat);
    }
}

pub fn push_dec(out: &mut Vec<u8>, n: usize)
    ensures
        final(out)@ == old(out)@ + dec(n as nat),
    decreases n,
{
    if n >= 10 {
        push_dec(out, n / 10);
    }
    out.push(b'0' + (n % 10) as u8);
    proof {
        if n < 10 {
            assert(old(out)@ + dec(n as nat) =~= old(out)@.push(digit(n as nat)));
        } else {
            assert(old(out)@ + dec(n as nat) =~= (old(out)@ + dec((n / 10) as nat)).push(digit(n as nat)));
        }
    }
}

pub fn push_dec_fixed(out: &mut Vec<u8>, n: usize, w: usize)
    ensures
        final(out)@ == old(out)@ + dec_fixed(n as nat, w as nat),
    decreases w,
{
    if w > 0 {
        push_dec_fixed(out, n / 10, w - 1);
        out.push(b'0' + (n % 10) as u8);
        assert(old(out)@ + dec_fixed(n as nat, w as nat) =~= (old(out)@ + dec_fixed((n / 10) as nat, (w - 1) as nat)).push(digit(n as nat)));
    } else {
        assert(old(out)@ + dec_fixed(n as nat, 0) =~= old(out)@);
    }
}

} // verus!
