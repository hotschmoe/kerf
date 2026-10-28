//! PDF object writer with a classic cross-reference table, proven with Verus (engines/rust/verus/VERUS.md):
//! every xref entry is the offset of its object's `N 0 obj` header, entries are 20 bytes and in order, and
//! `startxref` points at `xref`. Self-contained: byte slices in, byte vector out.

use alloc::vec::Vec;
#[cfg(verus_keep_ghost)]
pub use lit::*;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::div_mod::*;
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::mul::*;

#[cfg(verus_keep_ghost)]
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

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + dec(n as nat),
    decreases n,
))]
pub fn push_dec(out: &mut Vec<u8>, n: usize) {
    if n >= 10 {
        push_dec(out, n / 10);
    }
    out.push(b'0' + (n % 10) as u8);
    #[cfg(verus_keep_ghost)]
    proof! {
        if n < 10 {
            assert(old(out)@ + dec(n as nat) =~= old(out)@.push(digit(n as nat)));
        } else {
            assert(old(out)@ + dec(n as nat) =~= (old(out)@ + dec((n / 10) as nat)).push(digit(n as nat)));
        }
    }
}

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + dec_fixed(n as nat, w as nat),
    decreases w,
))]
pub fn push_dec_fixed(out: &mut Vec<u8>, n: usize, w: usize) {
    if w > 0 {
        push_dec_fixed(out, n / 10, w - 1);
        out.push(b'0' + (n % 10) as u8);
        #[cfg(verus_keep_ghost)]
        proof! { assert(old(out)@ + dec_fixed(n as nat, w as nat) =~= (old(out)@ + dec_fixed((n / 10) as nat, (w - 1) as nat)).push(digit(n as nat))); }
    } else {
        #[cfg(verus_keep_ghost)]
        proof! { assert(old(out)@ + dec_fixed(n as nat, 0) =~= old(out)@); }
    }
}


#[cfg(verus_keep_ghost)]
pub mod lit {
    #[cfg(verus_keep_ghost)]
    use vstd::prelude::*;

    #[cfg(verus_keep_ghost)]
    verus! {

    #[verifier::opaque]
    pub open spec fn pdf_header() -> Seq<u8> {
        seq![37u8, 80, 68, 70, 45, 49, 46, 55, 10, 37, 0xE2, 0xE3, 0xCF, 0xD3, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_obj() -> Seq<u8> {
        seq![32u8, 48, 32, 111, 98, 106, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_xref() -> Seq<u8> {
        seq![120u8, 114, 101, 102, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_free() -> Seq<u8> {
        seq![10u8, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 32, 54, 53, 53, 51, 53, 32, 102, 32, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_inuse() -> Seq<u8> {
        seq![32u8, 48, 48, 48, 48, 48, 32, 110, 32, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_trailer() -> Seq<u8> {
        seq![116u8, 114, 97, 105, 108, 101, 114, 10, 60, 60, 32, 47, 83, 105, 122, 101, 32]
    }

    #[verifier::opaque]
    pub open spec fn s_root() -> Seq<u8> {
        seq![32u8, 47, 82, 111, 111, 116, 32]
    }

    #[verifier::opaque]
    pub open spec fn s_info() -> Seq<u8> {
        seq![32u8, 48, 32, 82, 32, 47, 73, 110, 102, 111, 32]
    }

    #[verifier::opaque]
    pub open spec fn s_startxref() -> Seq<u8> {
        seq![32u8, 48, 32, 82, 32, 62, 62, 10, 115, 116, 97, 114, 116, 120, 114, 101, 102, 10]
    }

    #[verifier::opaque]
    pub open spec fn s_eof() -> Seq<u8> {
        seq![10u8, 37, 37, 69, 79, 70, 10]
    }

    pub broadcast proof fn lemma_lit_lens()
        ensures
            #[trigger] pdf_header().len() == 15,
            #[trigger] s_obj().len() == 7,
            #[trigger] s_xref().len() == 5,
            #[trigger] s_free().len() == 21,
            #[trigger] s_inuse().len() == 10,
            #[trigger] s_trailer().len() == 17,
            #[trigger] s_root().len() == 7,
            #[trigger] s_info().len() == 11,
            #[trigger] s_startxref().len() == 18,
            #[trigger] s_eof().len() == 7,
    {
        reveal(pdf_header);
        reveal(s_obj);
        reveal(s_xref);
        reveal(s_free);
        reveal(s_inuse);
        reveal(s_trailer);
        reveal(s_root);
        reveal(s_info);
        reveal(s_startxref);
        reveal(s_eof);
    }

    } // verus!
}

#[cfg(verus_keep_ghost)]
verus! {

broadcast use lemma_lit_lens;

/// `N 0 obj\n`
pub open spec fn obj_header(n: nat) -> Seq<u8> {
    dec(n) + s_obj()
}

/// `xref\n0 {count}\n0000000000 65535 f \n`
pub open spec fn xref_head(count: nat) -> Seq<u8> {
    s_xref() + seq![48u8, 32] + dec(count) + s_free()
}

/// `{off:0>10}`: at least 10 digits, zero padded.
pub open spec fn pad10(n: nat) -> Seq<u8> {
    if n < 10_000_000_000 { dec_fixed(n, 10) } else { dec(n) }
}

/// One xref entry, `{off:0>10} 00000 n \n` (20 bytes below offset 10^10).
pub open spec fn xref_entry(off: nat) -> Seq<u8> {
    pad10(off) + s_inuse()
}

/// The entries fit the 10-digit field.
pub open spec fn offsets_fit(offs: Seq<usize>) -> bool {
    forall|j: int| 0 <= j < offs.len() ==> (#[trigger] offs[j] as nat) < 10_000_000_000
}

pub open spec fn xref_entries(offs: Seq<usize>) -> Seq<u8>
    decreases offs.len(),
{
    if offs.len() == 0 {
        Seq::empty()
    } else {
        xref_entries(offs.drop_last()) + xref_entry(offs.last() as nat)
    }
}

/// `trailer\n<< /Size {count} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n`
pub open spec fn trailer(count: nat, root: nat, info: nat, xref: nat) -> Seq<u8> {
    s_trailer() + dec(count) + s_root() + dec(root) + s_info() + dec(info) + s_startxref() + dec(xref)
        + s_eof()
}

/// The PDF file the writer's state finishes to.
pub open spec fn pdf_file(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat) -> Seq<u8> {
    body + xref_head((offs.len() + 1) as nat) + xref_entries(offs) + trailer(
        (offs.len() + 1) as nat,
        root,
        info,
        body.len(),
    )
}

/// Byte offset of the xref entry for object `i + 1` in a file whose `xref` keyword is at `x`.
pub open spec fn entry_pos(x: nat, count: nat, i: nat) -> nat {
    x + xref_head(count).len() + 20 * i
}

} // verus!

/// A PDF writer: the file so far and the offset of each object's `N 0 obj` header.
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

#[cfg(verus_keep_ghost)]
verus! {

impl Writer {
pub closed spec fn body(&self) -> Seq<u8> {
        self.out@
    }

pub closed spec fn offsets(&self) -> Seq<usize> {
        self.offsets@
    }

/// Every recorded offset is where that object's header was written, in increasing order.
    pub closed spec fn wf(&self) -> bool {
        &&& self.out@.len() >= pdf_header().len()
        &&& self.out@.subrange(0, pdf_header().len() as int) == pdf_header()
        &&& forall|i: int|
            #![trigger self.offsets@[i]]
            0 <= i < self.offsets@.len() ==> {
                &&& pdf_header().len() <= self.offsets@[i]
                &&& self.offsets@[i] + obj_header((i + 1) as nat).len() <= self.out@.len()
                &&& self.out@.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ) == obj_header((i + 1) as nat)
            }
        &&& forall|i: int, j: int|
            #![trigger self.offsets@[i], self.offsets@[j]]
            0 <= i < j < self.offsets@.len() ==> self.offsets@[i] + obj_header((i + 1) as nat).len()
                <= self.offsets@[j]
    }

proof fn lemma_count_bound(&self)
        requires
            self.wf(),
        ensures
            self.offsets@.len() < self.out@.len(),
            self.offsets@.len() > 0 ==> self.offsets@.last() < self.out@.len(),
            forall|i: int| 0 <= i < self.offsets@.len() ==> self.offsets@[i] <= #[trigger] self.offsets@.last(),
    {
        let n = self.offsets@.len();
        if n > 0 {
            assert forall|i: int| 0 <= i < n implies #[trigger] self.offsets@[i] >= pdf_header().len() + i by {
                lemma_offsets_grow(self.offsets@, i);
            }
            assert(self.offsets@[n - 1] + obj_header(n as nat).len() <= self.out@.len());
            assert forall|i: int| 0 <= i < n implies self.offsets@[i] <= #[trigger] self.offsets@.last() by {
                if i < n - 1 {
                    assert(self.offsets@[i] + obj_header((i + 1) as nat).len() <= self.offsets@[n - 1]);
                }
            }
        }
    }
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_verify)]
impl Writer {
    /// `%PDF-1.7` and the binary-comment line.
    #[cfg_attr(verus_keep_ghost, verus_spec(w =>
        ensures
            w.wf(),
            w.body() == pdf_header(),
            w.offsets().len() == 0,
    ))]
    pub fn new() -> Self {
        #[cfg(verus_keep_ghost)]
        proof! { reveal(pdf_header); }
        let mut out = Vec::new();
        push_bytes(
            &mut out,
            &[
                37u8, 80, 68, 70, 45, 49, 46, 55, 10, 37, 0xE2, 0xE3, 0xCF, 0xD3, 10,
            ],
        );
        let w = Writer {
            out,
            offsets: Vec::new(),
        };
        #[cfg(verus_keep_ghost)]
        proof! { assert(w.out@ =~= pdf_header()); }
        #[cfg(verus_keep_ghost)]
        proof! { assert(w.out@.subrange(0, pdf_header().len() as int) =~= pdf_header()); }
        w
    }

    /// Starts the next object: records its offset and writes `N 0 obj\n`. Returns N.
    #[cfg_attr(verus_keep_ghost, verus_spec(n =>
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            n == old(self).offsets().len() + 1,
            final(self).offsets() == old(self).offsets().push(old(self).body().len() as usize),
            final(self).body() == old(self).body() + obj_header(n as nat),
    ))]
    pub fn begin_object(&mut self) -> usize {
        #[cfg(verus_keep_ghost)]
        proof! {
            self.lemma_count_bound();
        }
        let off = self.out.len();
        let n = self.offsets.len() + 1;
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost out0 = self.out@;
            let ghost offs0 = self.offsets@;
        }
        push_dec(&mut self.out, n);
        #[cfg(verus_keep_ghost)]
        proof! { reveal(s_obj); }
        push_bytes(&mut self.out, &[32u8, 48, 32, 111, 98, 106, 10]);
        self.offsets.push(off);
        #[cfg(verus_keep_ghost)]
        proof! {
            assert(self.out@ =~= out0 + obj_header(n as nat));
            assert(self.out@.subrange(0, pdf_header().len() as int) =~= out0.subrange(
                0,
                pdf_header().len() as int,
            ));
            assert forall|i: int|
                #![trigger self.offsets@[i]]
                0 <= i < self.offsets@.len() implies {
                &&& pdf_header().len() <= self.offsets@[i]
                &&& self.offsets@[i] + obj_header((i + 1) as nat).len() <= self.out@.len()
                &&& self.out@.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ) == obj_header((i + 1) as nat)
            } by {
                if i < offs0.len() {
                    assert(self.offsets@[i] == offs0[i]);
                    assert(self.out@.subrange(
                        self.offsets@[i] as int,
                        self.offsets@[i] + obj_header((i + 1) as nat).len(),
                    ) =~= out0.subrange(
                        offs0[i] as int,
                        offs0[i] + obj_header((i + 1) as nat).len(),
                    ));
                } else {
                    assert(self.out@.subrange(off as int, off + obj_header(n as nat).len()) =~= obj_header(
                        n as nat,
                    ));
                }
            }
        }
        n
    }

    /// Appends raw bytes (dictionary text, stream data).
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).offsets() == old(self).offsets(),
            final(self).body() == old(self).body() + bytes@,
    ))]
    pub fn put(&mut self, bytes: &[u8]) {
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost out0 = self.out@; }
        push_bytes(&mut self.out, bytes);
        #[cfg(verus_keep_ghost)]
        proof! {
            assert(self.out@.subrange(0, pdf_header().len() as int) =~= out0.subrange(
                0,
                pdf_header().len() as int,
            ));
            assert forall|i: int|
                #![trigger self.offsets@[i]]
                0 <= i < self.offsets@.len() implies self.out@.subrange(
                self.offsets@[i] as int,
                self.offsets@[i] + obj_header((i + 1) as nat).len(),
            ) == obj_header((i + 1) as nat) by {
                assert(self.out@.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ) =~= out0.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ));
            }
        }
    }

    /// Appends the decimal form of `n` (`{d}`).
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).offsets() == old(self).offsets(),
            final(self).body() == old(self).body() + dec(n as nat),
    ))]
    pub fn push_usize(&mut self, n: usize) {
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost out0 = self.out@; }
        push_dec(&mut self.out, n);
        #[cfg(verus_keep_ghost)]
        proof! {
            assert(self.out@.subrange(0, pdf_header().len() as int) =~= out0.subrange(
                0,
                pdf_header().len() as int,
            ));
            assert forall|i: int|
                #![trigger self.offsets@[i]]
                0 <= i < self.offsets@.len() implies self.out@.subrange(
                self.offsets@[i] as int,
                self.offsets@[i] + obj_header((i + 1) as nat).len(),
            ) == obj_header((i + 1) as nat) by {
                assert(self.out@.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ) =~= out0.subrange(
                    self.offsets@[i] as int,
                    self.offsets@[i] + obj_header((i + 1) as nat).len(),
                ));
            }
        }
    }

    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).offsets() == old(self).offsets(),
            final(self).body() == old(self).body() + seq![101u8, 110, 100, 111, 98, 106, 10],
    ))]
    pub fn end_object(&mut self) {
        self.put(&[101u8, 110, 100, 111, 98, 106, 10]);
    }

    /// A whole object whose body is `body` (a dictionary, say), then a newline and `endobj`.
    #[cfg_attr(verus_keep_ghost, verus_spec(n =>
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            n == old(self).offsets().len() + 1,
            final(self).offsets() == old(self).offsets().push(old(self).body().len() as usize),
            final(self).body() == old(self).body() + obj_header(n as nat) + body@ + seq![10u8]
                + seq![101u8, 110, 100, 111, 98, 106, 10],
    ))]
    pub fn object(&mut self, body: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(body);
        self.put(&[10u8]);
        self.end_object();
        n
    }

    /// A stream object with only a /Length entry.
    #[cfg_attr(verus_keep_ghost, verus_spec(n =>
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            n == old(self).offsets().len() + 1,
            final(self).offsets() == old(self).offsets().push(old(self).body().len() as usize),
    ))]
    pub fn stream_object(&mut self, data: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(&[60u8, 60, 32, 47, 76, 101, 110, 103, 116, 104, 32]);
        self.push_usize(data.len());
        self.put(&[32u8, 62, 62, 10, 115, 116, 114, 101, 97, 109, 10]);
        self.put(data);
        self.put(&[101u8, 110, 100, 115, 116, 114, 101, 97, 109, 10]);
        self.end_object();
        n
    }

    /// Appends the xref table and trailer; `root` and `info` are object numbers. Entries are padded
    /// to at least 10 digits (`{d:0>10}`), so an offset of 10^10 or more widens its entry, as in Zig.
    #[cfg_attr(verus_keep_ghost, verus_spec(r =>
        requires
            self.wf(),
        ensures
            r@ == pdf_file(self.body(), self.offsets(), root as nat, info as nat),
            offsets_fit(self.offsets()) ==> xref_valid(r@, self.body().len(), self.offsets()),
    ))]
    pub fn finish(self, root: usize, info: usize) -> Vec<u8> {
        #[cfg(verus_keep_ghost)]
        proof! {
            self.lemma_count_bound();
        }
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost w = self; }
        let mut out = self.out;
        let offsets = self.offsets;
        let k = offsets.len();
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost body = out@;
            let ghost offs = offsets@;
        }
        let x = out.len();
        let count = k + 1;
        push_xref_head(&mut out, count);
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost out1 = out@; }
        let mut i: usize = 0;
        #[cfg(verus_keep_ghost)]
        proof! { assert(offs.subrange(0, 0) =~= Seq::<usize>::empty()); }
        #[cfg(verus_keep_ghost)]
        proof! { assert(out1 + xref_entries(offs.subrange(0, 0)) =~= out1); }
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                k == offsets@.len() == offs.len(),
                offsets@ == offs,
                i <= k,
                out@ == out1 + xref_entries(offs.subrange(0, i as int)),
            decreases k - i,
        ))]
        while i < k {
            #[cfg(verus_keep_ghost)]
            proof_decl! { let ghost before = out@; }
            push_entry(&mut out, offsets[i]);
            #[cfg(verus_keep_ghost)]
            proof! {
                let s1 = offs.subrange(0, i + 1);
                assert(s1.drop_last() =~= offs.subrange(0, i as int));
                assert(s1.last() == offs[i as int]);
                assert(xref_entries(s1) == xref_entries(offs.subrange(0, i as int)) + xref_entry(offs[i as int] as nat));
                assert(out@ == before + xref_entry(offs[i as int] as nat));
                assert(out@ =~= out1 + xref_entries(s1));
            }
            i += 1;
        }
        #[cfg(verus_keep_ghost)]
        proof! { assert(offs.subrange(0, k as int) =~= offs); }
        push_trailer(&mut out, count, root, info, x);
        #[cfg(verus_keep_ghost)]
        proof! {
            if offsets_fit(offs) {
                lemma_pdf_file(body, offs, root as nat, info as nat);
            }
        }
        out
    }
}

#[cfg(verus_keep_ghost)]
verus! {

/// What a PDF reader relies on: every xref entry is 20 bytes, in object order, and holds the
/// offset of that object's `N 0 obj` header; `startxref` is followed by the decimal offset of the
/// `xref` keyword, then `%%EOF`.
pub open spec fn xref_valid(f: Seq<u8>, x: nat, offs: Seq<usize>) -> bool {
    let t = f.len() - s_eof().len();
    &&& x + 5 <= f.len()
    &&& f.subrange(x as int, x as int + 5) == s_xref()
    &&& entries_valid(f, x, offs)
    &&& f.subrange(t, f.len() as int) == s_eof()
    &&& f.subrange(t - dec(x).len() - s_startxref().len(), t - dec(x).len()) == s_startxref()
    &&& all_digits(f.subrange(t - dec(x).len(), t))
    &&& digits_val(f.subrange(t - dec(x).len(), t)) == x
}

pub open spec fn entries_valid(f: Seq<u8>, x: nat, offs: Seq<usize>) -> bool {
    forall|i: int| #![trigger offs[i]] 0 <= i < offs.len() ==> entry_valid(f, x, offs, i)
}

pub open spec fn entry_valid(f: Seq<u8>, x: nat, offs: Seq<usize>, i: int) -> bool {
    let e = entry_pos(x, (offs.len() + 1) as nat, i as nat) as int;
    let o = offs[i] as int;
    &&& e + 20 <= f.len()
    &&& f.subrange(e, e + 20) == xref_entry(o as nat)
    &&& all_digits(f.subrange(e, e + 10))
    &&& digits_val(f.subrange(e, e + 10)) == o
    &&& o + obj_header((i + 1) as nat).len() <= x
    &&& f.subrange(o, o + obj_header((i + 1) as nat).len()) == obj_header((i + 1) as nat)
}

/// `pdf_file` of a well-formed writer state satisfies `xref_valid`.
pub proof fn lemma_pdf_file(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat)
    requires
        forall|i: int|
            #![trigger offs[i]]
            0 <= i < offs.len() ==> {
                &&& (offs[i] as nat) < 10_000_000_000
                &&& offs[i] + obj_header((i + 1) as nat).len() <= body.len()
                &&& body.subrange(offs[i] as int, offs[i] + obj_header((i + 1) as nat).len())
                    == obj_header((i + 1) as nat)
            },
    ensures
        xref_valid(pdf_file(body, offs, root, info), body.len(), offs),
{
    lemma_xref_keyword(body, offs, root, info);
    lemma_tail(body, offs, root, info);
    lemma_entries_in_file(body, offs, root, info);
}

proof fn lemma_xref_keyword(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat)
    ensures
        ({
            let f = pdf_file(body, offs, root, info);
            let x = body.len();
            &&& x + 5 <= f.len()
            &&& f.subrange(x as int, x as int + 5) == s_xref()
        }),
{
    let f = pdf_file(body, offs, root, info);
    let x = body.len();
    let count = (offs.len() + 1) as nat;
    let xh = xref_head(count);
    let en = xref_entries(offs);
    let tr = trailer(count, root, info, x);
    let h = ((body + xh) + en).len() as int;
    lemma_concat((body + xh) + en, tr);
    lemma_concat(body + xh, en);
    lemma_concat(body, xh);
    assert(s_xref() =~= xh.subrange(0, 5));
    lemma_sub_sub(f, 0, h, x as int, x as int + 5);
    lemma_sub_sub((body + xh) + en, 0, (body + xh).len() as int, x as int, x as int + 5);
}

proof fn lemma_tail(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat)
    ensures
        ({
            let f = pdf_file(body, offs, root, info);
            let x = body.len();
            let t = f.len() - s_eof().len();
            &&& f.subrange(t, f.len() as int) == s_eof()
            &&& f.subrange(t - dec(x).len() - s_startxref().len(), t - dec(x).len()) == s_startxref()
            &&& all_digits(f.subrange(t - dec(x).len(), t))
            &&& digits_val(f.subrange(t - dec(x).len(), t)) == x
        }),
{
    let f = pdf_file(body, offs, root, info);
    let x = body.len();
    let count = (offs.len() + 1) as nat;
    let tr = trailer(count, root, info, x);
    let pre = s_trailer() + dec(count) + s_root() + dec(root) + s_info() + dec(info);
    let a = pre + s_startxref();
    let b = a + dec(x);
    let dl = dec(x).len() as int;
    let sl = s_startxref().len() as int;
    let h = (body + xref_head(count) + xref_entries(offs)).len() as int;
    assert(tr == b + s_eof());
    assert(f == (body + xref_head(count) + xref_entries(offs)) + tr);
    lemma_concat(body + xref_head(count) + xref_entries(offs), tr);
    lemma_concat(b, s_eof());
    lemma_concat(a, dec(x));
    lemma_concat(pre, s_startxref());
    let t = f.len() - s_eof().len();
    let tt = b.len() as int;
    assert(t == h + tt);
    let fb = f.subrange(h, t);
    lemma_sub_sub(f, h, f.len() as int, 0, tt);
    assert(fb == tr.subrange(0, tt));
    assert(fb == b);
    lemma_sub_sub(f, h, f.len() as int, tt, tr.len() as int);
    assert(f.subrange(t, f.len() as int) == s_eof());
    lemma_sub_sub(f, h, t, tt - dl, tt);
    assert(f.subrange(t - dl, t) == b.subrange(tt - dl, tt));
    assert(b.subrange(tt - dl, tt) == dec(x));
    lemma_sub_sub(f, h, t, tt - dl - sl, tt - dl);
    lemma_sub_sub(b, 0, tt - dl, tt - dl - sl, tt - dl);
    assert(f.subrange(t - dl - sl, t - dl) == a.subrange(tt - dl - sl, tt - dl));
    assert(a.subrange(tt - dl - sl, tt - dl) == s_startxref());
    lemma_dec(x);
}

proof fn lemma_entries_in_file(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat)
    requires
        forall|i: int|
            #![trigger offs[i]]
            0 <= i < offs.len() ==> {
                &&& (offs[i] as nat) < 10_000_000_000
                &&& offs[i] + obj_header((i + 1) as nat).len() <= body.len()
                &&& body.subrange(offs[i] as int, offs[i] + obj_header((i + 1) as nat).len())
                    == obj_header((i + 1) as nat)
            },
    ensures
        entries_valid(pdf_file(body, offs, root, info), body.len(), offs),
{
    assert(offsets_fit(offs));
    assert forall|i: int| #![trigger offs[i]] 0 <= i < offs.len() implies entry_valid(
        pdf_file(body, offs, root, info),
        body.len(),
        offs,
        i,
    ) by {
        lemma_entry_in_file(body, offs, root, info, i);
    }
}

proof fn lemma_entry_in_file(body: Seq<u8>, offs: Seq<usize>, root: nat, info: nat, i: int)
    requires
        0 <= i < offs.len(),
        offsets_fit(offs),
        (offs[i] as nat) < 10_000_000_000,
        offs[i] + obj_header((i + 1) as nat).len() <= body.len(),
        body.subrange(offs[i] as int, offs[i] + obj_header((i + 1) as nat).len()) == obj_header(
            (i + 1) as nat,
        ),
    ensures
        entry_valid(pdf_file(body, offs, root, info), body.len(), offs, i),
{
    let f = pdf_file(body, offs, root, info);
    let x = body.len();
    let count = (offs.len() + 1) as nat;
    let xh = xref_head(count);
    let en = xref_entries(offs);
    let tr = trailer(count, root, info, x);
    let e = entry_pos(x, count, i as nat) as int;
    let o = offs[i] as int;
    let hl = obj_header((i + 1) as nat).len() as int;
    let h = ((body + xh) + en).len() as int;
    lemma_entries_at(offs, i);
    lemma_dec_fixed(o as nat, 10);
    assert(pow10(10) == 10_000_000_000) by (compute);
    vstd::arithmetic::div_mod::lemma_small_mod(o as nat, 10_000_000_000);
    lemma_concat((body + xh) + en, tr);
    lemma_concat(body + xh, en);
    lemma_concat(body, xh);
    let b2 = (body + xh).len() as int;
    lemma_sub_sub(f, 0, h, e, e + 20);
    lemma_sub_sub((body + xh) + en, b2, h, 20 * i, 20 * i + 20);
    lemma_sub_sub(f, 0, h, 0, b2);
    lemma_sub_sub(f, 0, h, 0, x as int);
    lemma_sub_sub(f, 0, x as int, o, o + hl);
    lemma_sub_sub((body + xh) + en, 0, b2, 0, x as int);
    lemma_sub_sub(f, e, e + 20, 0, 10);
    lemma_concat(dec_fixed(o as nat, 10), s_inuse());
}

proof fn lemma_concat(a: Seq<u8>, b: Seq<u8>)
    ensures
        (a + b).len() == a.len() + b.len(),
        (a + b).subrange(0, a.len() as int) == a,
        (a + b).subrange(a.len() as int, (a + b).len() as int) == b,
{
    assert((a + b).subrange(0, a.len() as int) =~= a);
    assert((a + b).subrange(a.len() as int, (a + b).len() as int) =~= b);
}

proof fn lemma_sub_sub(s: Seq<u8>, i: int, j: int, k: int, l: int)
    requires
        0 <= i <= j <= s.len(),
        0 <= k <= l <= j - i,
    ensures
        s.subrange(i, j).subrange(k, l) == s.subrange(i + k, i + l),
{
    assert(s.subrange(i, j).subrange(k, l) =~= s.subrange(i + k, i + l));
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + xref_head(count as nat),
))]
fn push_xref_head(out: &mut Vec<u8>, count: usize) {
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_xref); }
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_free); }
    push_bytes(out, &[120u8, 114, 101, 102, 10, 48, 32]);
    push_dec(out, count);
    push_bytes(
        out,
        &[
            10u8, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 32, 54, 53, 53, 51, 53, 32, 102, 32, 10,
        ],
    );
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + xref_head(count as nat)); }
}

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + trailer(count as nat, root as nat, info as nat, x as nat),
))]
fn push_trailer(out: &mut Vec<u8>, count: usize, root: usize, info: usize, x: usize) {
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_trailer); }
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_root); }
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_info); }
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_startxref); }
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_eof); }
    push_bytes(
        out,
        &[
            116u8, 114, 97, 105, 108, 101, 114, 10, 60, 60, 32, 47, 83, 105, 122, 101, 32,
        ],
    );
    push_dec(out, count);
    push_bytes(out, &[32u8, 47, 82, 111, 111, 116, 32]);
    push_dec(out, root);
    push_bytes(out, &[32u8, 48, 32, 82, 32, 47, 73, 110, 102, 111, 32]);
    push_dec(out, info);
    push_bytes(
        out,
        &[
            32u8, 48, 32, 82, 32, 62, 62, 10, 115, 116, 97, 114, 116, 120, 114, 101, 102, 10,
        ],
    );
    push_dec(out, x);
    push_bytes(out, &[10u8, 37, 37, 69, 79, 70, 10]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + trailer(count as nat, root as nat, info as nat, x as nat)); }
}

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + xref_entry(o as nat),
))]
fn push_entry(out: &mut Vec<u8>, o: usize) {
    #[cfg(verus_keep_ghost)]
    proof! { reveal(s_inuse); }
    if (o as u64) < 10_000_000_000u64 {
        push_dec_fixed(out, o, 10);
        #[cfg(verus_keep_ghost)]
        proof! { assert(pad10(o as nat) == dec_fixed(o as nat, 10)); }
    } else {
        push_dec(out, o);
        #[cfg(verus_keep_ghost)]
        proof! { assert(pad10(o as nat) == dec(o as nat)); }
    }
    push_bytes(out, &[32u8, 48, 48, 48, 48, 48, 32, 110, 32, 10]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + xref_entry(o as nat)); }
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_offsets_grow(offs: Seq<usize>, i: int)
    requires
        0 <= i < offs.len(),
        forall|j: int| #![trigger offs[j]] 0 <= j < offs.len() ==> pdf_header().len() <= offs[j],
        forall|a: int, b: int|
            #![trigger offs[a], offs[b]]
            0 <= a < b < offs.len() ==> offs[a] + obj_header((a + 1) as nat).len() <= offs[b],
    ensures
        offs[i] >= pdf_header().len() + i,
    decreases i,
{
    if i > 0 {
        lemma_offsets_grow(offs, i - 1);
        lemma_dec((i) as nat);
    }
}

proof fn lemma_entries_len(offs: Seq<usize>)
    requires
        offsets_fit(offs),
    ensures
        xref_entries(offs).len() == 20 * offs.len(),
    decreases offs.len(),
{
    if offs.len() > 0 {
        assert(offsets_fit(offs.drop_last()));
        lemma_entries_len(offs.drop_last());
        assert((offs.last() as nat) < 10_000_000_000);
        lemma_dec_fixed(offs.last() as nat, 10);
    }
}

proof fn lemma_entries_at(offs: Seq<usize>, i: int)
    requires
        0 <= i < offs.len(),
        offsets_fit(offs),
    ensures
        xref_entries(offs).len() == 20 * offs.len(),
        xref_entries(offs).subrange(20 * i, 20 * i + 20) == xref_entry(offs[i] as nat),
    decreases offs.len(),
{
    lemma_entries_len(offs);
    let last = offs.len() - 1;
    assert(offsets_fit(offs.drop_last()));
    lemma_entries_len(offs.drop_last());
    assert((offs.last() as nat) < 10_000_000_000);
    assert((offs[i] as nat) < 10_000_000_000);
    lemma_dec_fixed(offs.last() as nat, 10);
    if i < last {
        lemma_entries_at(offs.drop_last(), i);
        assert(xref_entries(offs).subrange(20 * i, 20 * i + 20) =~= xref_entries(
            offs.drop_last(),
        ).subrange(20 * i, 20 * i + 20));
    } else {
        assert(xref_entries(offs).subrange(20 * i, 20 * i + 20) =~= xref_entry(offs[i] as nat));
    }
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + bytes@,
))]
pub fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + bytes@); }
}

#[cfg_attr(verus_keep_ghost, verifier::external)]
impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
