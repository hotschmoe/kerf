use alloc::vec::Vec;
use crate::dec::*;
use vstd::prelude::*;
pub use lit::*;

pub mod lit {
    use vstd::prelude::*;

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

/// One 20-byte xref entry: `{off:0>10} 00000 n \n`.
pub open spec fn xref_entry(off: nat) -> Seq<u8> {
    dec_fixed(off, 10) + s_inuse()
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

/// A PDF writer: the file so far and the offset of each object's `N 0 obj` header.
pub struct PdfWriter {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl PdfWriter {
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

    /// `%PDF-1.7` and the binary-comment line.
    pub fn new() -> (w: Self)
        ensures
            w.wf(),
            w.body() == pdf_header(),
            w.offsets().len() == 0,
    {
        reveal(pdf_header);
        let mut out = Vec::new();
        push_bytes(&mut out, &[37u8, 80, 68, 70, 45, 49, 46, 55, 10, 37, 0xE2, 0xE3, 0xCF, 0xD3, 10]);
        let w = PdfWriter { out, offsets: Vec::new() };
        assert(w.out@ =~= pdf_header());
        assert(w.out@.subrange(0, pdf_header().len() as int) =~= pdf_header());
        w
    }

    /// Starts the next object: records its offset and writes `N 0 obj\n`. Returns N.
    pub fn begin_object(&mut self) -> (n: usize)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            n == old(self).offsets().len() + 1,
            final(self).offsets() == old(self).offsets().push(old(self).body().len() as usize),
            final(self).body() == old(self).body() + obj_header(n as nat),
    {
        proof {
            self.lemma_count_bound();
        }
        let off = self.out.len();
        let n = self.offsets.len() + 1;
        let ghost out0 = self.out@;
        let ghost offs0 = self.offsets@;
        push_dec(&mut self.out, n);
        reveal(s_obj);
        push_bytes(&mut self.out, &[32u8, 48, 32, 111, 98, 106, 10]);
        self.offsets.push(off);
        proof {
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

    /// Appends raw bytes (dictionary text, stream data, `endobj`).
    pub fn push(&mut self, bytes: &[u8])
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).offsets() == old(self).offsets(),
            final(self).body() == old(self).body() + bytes@,
    {
        let ghost out0 = self.out@;
        push_bytes(&mut self.out, bytes);
        proof {
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
    pub fn push_usize(&mut self, n: usize)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).offsets() == old(self).offsets(),
            final(self).body() == old(self).body() + dec(n as nat),
    {
        let ghost out0 = self.out@;
        push_dec(&mut self.out, n);
        proof {
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

    /// Writes the cross-reference table and trailer. `None` only if an object starts at or beyond
    /// byte 10^10, which a 10-digit xref field cannot hold.
    pub fn finish(self, root: usize, info: usize) -> (r: Option<Vec<u8>>)
        requires
            self.wf(),
        ensures
            r.is_none() ==> self.offsets().len() > 0 && self.offsets().last() >= 10_000_000_000,
            r.is_some() ==> r.unwrap()@ == pdf_file(self.body(), self.offsets(), root as nat, info as nat)
                && xref_valid(r.unwrap()@, self.body().len(), self.offsets()),
    {
        proof {
            self.lemma_count_bound();
        }
        let ghost w = self;
        let mut out = self.out;
        let offsets = self.offsets;
        let k = offsets.len();
        if k > 0 && offsets[k - 1] as u64 >= 10_000_000_000u64 {
            return None;
        }
        let ghost body = out@;
        let ghost offs = offsets@;
        proof {
            assert forall|j: int| 0 <= j < k implies (offs[j] as nat) < 10_000_000_000 by {
                assert(offs[j] <= offs.last());
            }
        }
        let x = out.len();
        let count = k + 1;
        push_xref_head(&mut out, count);
        let ghost out1 = out@;
        let mut i: usize = 0;
        assert(offs.subrange(0, 0) =~= Seq::<usize>::empty());
        assert(out1 + xref_entries(offs.subrange(0, 0)) =~= out1);
        while i < k
            invariant
                k == offsets@.len() == offs.len(),
                offsets@ == offs,
                i <= k,
                out@ == out1 + xref_entries(offs.subrange(0, i as int)),
            decreases k - i,
        {
            let ghost before = out@;
            push_entry(&mut out, offsets[i]);
            proof {
                let s1 = offs.subrange(0, i + 1);
                assert(s1.drop_last() =~= offs.subrange(0, i as int));
                assert(s1.last() == offs[i as int]);
                assert(xref_entries(s1) == xref_entries(offs.subrange(0, i as int)) + xref_entry(offs[i as int] as nat));
                assert(out@ == before + xref_entry(offs[i as int] as nat));
                assert(out@ =~= out1 + xref_entries(s1));
            }
            i += 1;
        }
        assert(offs.subrange(0, k as int) =~= offs);
        push_trailer(&mut out, count, root, info, x);
        proof {
            lemma_pdf_file(body, offs, root as nat, info as nat);
        }
        Some(out)
    }
}

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

fn push_xref_head(out: &mut Vec<u8>, count: usize)
    ensures
        final(out)@ == old(out)@ + xref_head(count as nat),
{
    reveal(s_xref);
    reveal(s_free);
    push_bytes(out, &[120u8, 114, 101, 102, 10, 48, 32]);
    push_dec(out, count);
    push_bytes(
        out,
        &[10u8, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 32, 54, 53, 53, 51, 53, 32, 102, 32, 10],
    );
    assert(final(out)@ =~= old(out)@ + xref_head(count as nat));
}

fn push_trailer(out: &mut Vec<u8>, count: usize, root: usize, info: usize, x: usize)
    ensures
        final(out)@ == old(out)@ + trailer(count as nat, root as nat, info as nat, x as nat),
{
    reveal(s_trailer);
    reveal(s_root);
    reveal(s_info);
    reveal(s_startxref);
    reveal(s_eof);
    push_bytes(out, &[116u8, 114, 97, 105, 108, 101, 114, 10, 60, 60, 32, 47, 83, 105, 122, 101, 32]);
    push_dec(out, count);
    push_bytes(out, &[32u8, 47, 82, 111, 111, 116, 32]);
    push_dec(out, root);
    push_bytes(out, &[32u8, 48, 32, 82, 32, 47, 73, 110, 102, 111, 32]);
    push_dec(out, info);
    push_bytes(out, &[32u8, 48, 32, 82, 32, 62, 62, 10, 115, 116, 97, 114, 116, 120, 114, 101, 102, 10]);
    push_dec(out, x);
    push_bytes(out, &[10u8, 37, 37, 69, 79, 70, 10]);
    assert(final(out)@ =~= old(out)@ + trailer(count as nat, root as nat, info as nat, x as nat));
}

fn push_entry(out: &mut Vec<u8>, o: usize)
    ensures
        final(out)@ == old(out)@ + xref_entry(o as nat),
{
    reveal(s_inuse);
    push_dec_fixed(out, o, 10);
    push_bytes(out, &[32u8, 48, 48, 48, 48, 48, 32, 110, 32, 10]);
    assert(final(out)@ =~= old(out)@ + xref_entry(o as nat));
}

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
    ensures
        xref_entries(offs).len() == 20 * offs.len(),
    decreases offs.len(),
{
    if offs.len() > 0 {
        lemma_entries_len(offs.drop_last());
        lemma_dec_fixed(offs.last() as nat, 10);
    }
}

proof fn lemma_entries_at(offs: Seq<usize>, i: int)
    requires
        0 <= i < offs.len(),
    ensures
        xref_entries(offs).len() == 20 * offs.len(),
        xref_entries(offs).subrange(20 * i, 20 * i + 20) == xref_entry(offs[i] as nat),
    decreases offs.len(),
{
    lemma_entries_len(offs);
    let last = offs.len() - 1;
    lemma_entries_len(offs.drop_last());
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

pub fn push_bytes(out: &mut Vec<u8>, bytes: &[u8])
    ensures
        final(out)@ == old(out)@ + bytes@,
{
    let mut i: usize = 0;
    while i < bytes.len()
        invariant
            i <= bytes@.len(),
            out@ == old(out)@ + bytes@.subrange(0, i as int),
        decreases bytes.len() - i,
    {
        out.push(bytes[i]);
        i += 1;
        assert(out@ =~= old(out)@ + bytes@.subrange(0, i as int));
    }
    assert(bytes@.subrange(0, i as int) =~= bytes@);
}

} // verus!
