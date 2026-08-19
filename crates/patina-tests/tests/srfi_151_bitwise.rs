//! SRFI 151 bitwise operations, plus the legacy spellings the ecosystem
//! actually imports.
//!
//! Bundled because it is the largest measured gap in the vendored corpus:
//! `(srfi 60)` has an in-degree of 31 and `(srfi 33)` 19, against SRFI 151's
//! standard-track status; `(srfi 142)` is 151's own withdrawn predecessor.
//! All four are the same operations under different sets of names, so 60, 33
//! and 142 are renames over 151 rather than separate ports.
//!
//! The core operators are Rust primitives. Two's-complement semantics over
//! unbounded integers is the part worth testing: negative operands behave as if
//! sign-extended infinitely to the left, and bignums must behave like fixnums.

mod common;
use common::eval_program as eval;

fn srfi151(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 151)) {expr}"))
}

fn srfi60(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 60)) {expr}"))
}

fn srfi33(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 33)) {expr}"))
}

fn srfi142(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 142)) {expr}"))
}

// ─── Core operators ──────────────────────────────────────────────────────────

#[test]
fn test_basic_logical_operators() {
    assert_eq!(srfi151("(bitwise-and 12 10)"), "8");
    assert_eq!(srfi151("(bitwise-ior 12 10)"), "14");
    assert_eq!(srfi151("(bitwise-xor 12 10)"), "6");
    assert_eq!(srfi151("(bitwise-not 12)"), "-13");
}

/// The operators are n-ary, and the empty case returns the identity SRFI 151
/// specifies -- which differs per operator and is easy to get backwards.
#[test]
fn test_nary_and_identities() {
    assert_eq!(srfi151("(bitwise-and 255 63 15)"), "15");
    assert_eq!(srfi151("(bitwise-ior 1 2 4 8)"), "15");
    assert_eq!(srfi151("(bitwise-xor 1 2 4)"), "7");
    assert_eq!(srfi151("(bitwise-and)"), "-1");
    assert_eq!(srfi151("(bitwise-ior)"), "0");
    assert_eq!(srfi151("(bitwise-xor)"), "0");
    assert_eq!(srfi151("(bitwise-and 42)"), "42");
}

/// Negative operands are sign-extended, so a right shift is arithmetic and
/// `bit-count` counts *zero* bits to stay finite.
#[test]
fn test_two_complement_semantics() {
    assert_eq!(srfi151("(arithmetic-shift -8 -2)"), "-2");
    assert_eq!(srfi151("(arithmetic-shift -1 4)"), "-16");
    assert_eq!(srfi151("(bitwise-and -1 12)"), "12");
    assert_eq!(srfi151("(bit-count -1)"), "0");
    assert_eq!(srfi151("(bit-count -2)"), "1");
    assert_eq!(srfi151("(integer-length -1)"), "0");
    assert_eq!(srfi151("(integer-length 0)"), "0");
}

/// Must behave identically past the fixnum boundary. The primitives take an
/// allocation-free i64 path when every operand is a fixnum and promote to
/// bignums otherwise, so these pin the slow path and the seam between them.
#[test]
fn test_bignum_operands() {
    assert_eq!(
        srfi151("(bitwise-and (expt 2 100) (- (expt 2 101) 1))"),
        "1267650600228229401496703205376"
    );
    assert_eq!(srfi151("(integer-length (expt 2 100))"), "101");
    assert_eq!(srfi151("(bit-count (- (expt 2 64) 1))"), "64");
    assert_eq!(
        srfi151("(arithmetic-shift 1 100)"),
        "1267650600228229401496703205376"
    );
    // Round-trips back below the boundary.
    assert_eq!(srfi151("(arithmetic-shift (expt 2 100) -100)"), "1");
}

#[test]
fn test_bit_tests_and_fields() {
    assert_eq!(srfi151("(bit-set? 2 12)"), "#t");
    assert_eq!(srfi151("(bit-set? 0 12)"), "#f");
    assert_eq!(srfi151("(first-set-bit 8)"), "3");
    assert_eq!(srfi151("(first-set-bit 0)"), "-1");
    assert_eq!(srfi151("(bit-field #b1101101010 0 4)"), "10");
    assert_eq!(srfi151("(any-bit-set? 12 10)"), "#t");
    assert_eq!(srfi151("(every-bit-set? 4 6)"), "#t");
    assert_eq!(srfi151("(copy-bit 0 0 #t)"), "1");
}

#[test]
fn test_derived_logical_operators() {
    assert_eq!(srfi151("(bitwise-nand 12 10)"), "-9");
    assert_eq!(srfi151("(bitwise-nor 12 10)"), "-15");
    assert_eq!(srfi151("(bitwise-andc1 12 10)"), "2");
    assert_eq!(srfi151("(bitwise-andc2 12 10)"), "4");
    assert_eq!(srfi151("(bitwise-if 3 1 8)"), "9");
}

/// N-ary eqv must fold pairwise. Complementing a seeded xor once at the end
/// coincidentally agrees for odd argument counts and is wrong for every even
/// one, so even arities and the zero-argument identity are the cases that bite.
#[test]
fn test_bitwise_eqv_folds_pairwise() {
    assert_eq!(srfi151("(bitwise-eqv 37 12)"), "-42"); // the SRFI 151 example
    assert_eq!(srfi151("(bitwise-eqv 12 10)"), "-7");
    assert_eq!(srfi151("(bitwise-eqv)"), "-1");
    assert_eq!(srfi151("(bitwise-eqv 42)"), "42");
    assert_eq!(srfi151("(bitwise-eqv 1 2 4)"), "7");
    assert_eq!(srfi151("(bitwise-eqv 1 2 4 8)"), "-16"); // eqv(7, 8)
}

#[test]
fn test_conversion_round_trip() {
    assert_eq!(srfi151("(bits->list 6)"), "(#f #t #t)");
    assert_eq!(srfi151("(list->bits '(#f #t #t))"), "6");
    assert_eq!(srfi151("(list->bits (bits->list 12345))"), "12345");
    assert_eq!(srfi151("(bits #f #t #t)"), "6");
}

#[test]
fn test_fold_and_unfold() {
    assert_eq!(
        srfi151("(bitwise-fold (lambda (b acc) (if b (+ acc 1) acc)) 0 #b1011)"),
        "3"
    );
}

// ─── The names the corpus actually imports ───────────────────────────────────

/// `(srfi 60)` is the single biggest gap measured over the vendored corpus
/// (in-degree 31). It spells things differently from 151, so importing 151 is
/// not a substitute.
#[test]
fn test_srfi_60_spellings() {
    assert_eq!(srfi60("(logand 12 10)"), "8");
    assert_eq!(srfi60("(logior 12 10)"), "14");
    assert_eq!(srfi60("(logxor 12 10)"), "6");
    assert_eq!(srfi60("(lognot 12)"), "-13");
    assert_eq!(srfi60("(ash 1 10)"), "1024");
    assert_eq!(srfi60("(logcount 12)"), "2");
    assert_eq!(srfi60("(logbit? 2 12)"), "#t");
    assert_eq!(srfi60("(log2-binary-factors 8)"), "3");
}

/// SRFI 60's list conversions are MSB-first — the opposite of SRFI 151's
/// bits->list family, which is why 151 renamed them. `(integer->list 6)` is
/// `(#t #t #f)` here and `(bits->list 6)` is `(#f #t #t)`.
#[test]
fn test_srfi_60_conversions_are_msb_first() {
    assert_eq!(srfi60("(integer->list 6)"), "(#t #t #f)");
    assert_eq!(srfi60("(integer->list 6 5)"), "(#f #f #t #t #f)");
    assert_eq!(srfi60("(list->integer '(#t #t #f))"), "6");
    assert_eq!(srfi60("(list->integer (integer->list 12345))"), "12345");
    assert_eq!(srfi60("(booleans->integer #t #f #t #f)"), "10");
}

/// SRFI 142 (withdrawn, superseded by 151) is a rename over the same
/// primitives, with one semantic twist: its `bitwise-if` takes bits from
/// the *third* argument where the mask bit is 1 — the opposite of 151's —
/// so the shim swaps the trailing arguments rather than aliasing.
/// mask 5 (101), i 3 (011), j 0: 142 keeps i's mask-0 bits → 2, where 151
/// keeps i's mask-1 bits → 1.
#[test]
fn test_srfi_142_bitwise_if_is_mask_zero_from_second() {
    assert_eq!(srfi142("(bitwise-if 5 3 0)"), "2");
    assert_eq!(srfi151("(bitwise-if 5 3 0)"), "1");
}

/// Unlike SRFI 60's MSB-first `integer->list`, SRFI 142's is LSB-first —
/// the very family 151 renamed to `bits->list` without changing the order,
/// so these are plain renames.
#[test]
fn test_srfi_142_conversions_are_lsb_first() {
    assert_eq!(srfi142("(integer->list 6)"), "(#f #t #t)");
    assert_eq!(srfi142("(list->integer '(#f #t #t))"), "6");
    assert_eq!(srfi142("(vector->integer #(#f #t #t))"), "6");
    assert_eq!(srfi142("(list->integer (integer->list 12345))"), "12345");
}

/// Headline operators reached through the 142 name — including the three
/// jkode-sassy, the corpus package that imports it, actually calls.
#[test]
fn test_srfi_142_headline_operators() {
    assert_eq!(srfi142("(bitwise-and 12 10)"), "8");
    assert_eq!(srfi142("(bitwise-ior 12 10)"), "14");
    assert_eq!(srfi142("(arithmetic-shift 1 8)"), "256");
    assert_eq!(srfi142("(bit-count 7)"), "3");
    assert_eq!(srfi142("(first-set-bit 12)"), "2");
}

/// SRFI 60 exports both spellings of eight operators -- `logand` *and*
/// `bitwise-and`, `ash` *and* `arithmetic-shift`. Easy to lose one half.
#[test]
fn test_srfi_60_exports_both_spellings() {
    assert_eq!(srfi60("(bitwise-and 12 10)"), "8");
    assert_eq!(srfi60("(arithmetic-shift 1 4)"), "16");
    assert_eq!(srfi60("(bit-count 12)"), "2");
    assert_eq!(srfi60("(list (logand 12 10) (bitwise-and 12 10))"), "(8 8)");
}

#[test]
fn test_srfi_33_spellings() {
    assert_eq!(srfi33("(bitwise-merge 3 1 8)"), "9");
    assert_eq!(srfi33("(any-bits-set? 12 10)"), "#t");
    assert_eq!(srfi33("(all-bits-set? 4 6)"), "#t");
}

/// All five SRFI 33 field operations, with chibi's `(srfi 33)` as the ground
/// truth for signatures and semantics — that is the implementation the corpus
/// packages were written against. In particular `copy-bit-field` takes
/// `(size position from to)` like its siblings, not SRFI 60's
/// `(to from start end)`.
#[test]
fn test_srfi_33_field_operations() {
    assert_eq!(srfi33("(extract-bit-field 4 0 255)"), "15");
    assert_eq!(srfi33("(extract-bit-field 4 8 #xA55A)"), "5");
    assert_eq!(srfi33("(replace-bit-field 4 0 5 255)"), "245");
    assert_eq!(srfi33("(copy-bit-field 4 0 255 0)"), "240");
    // test-bit-field? / clear-bit-field are renames of SRFI 151's
    // bit-field-any? / bit-field-clear, so they take (n start end) — chibi's
    // (srfi 33) makes the same choice.
    assert_eq!(srfi33("(test-bit-field? 10 1 2)"), "#t");
    assert_eq!(srfi33("(test-bit-field? 10 2 3)"), "#f");
    assert_eq!(srfi33("(clear-bit-field 15 0 2)"), "12");
}

/// R7RS-large names this `(scheme bitwise)`.
#[test]
fn test_r7rs_large_alias() {
    let s = |e: &str| eval(&format!("(import (scheme base) (scheme bitwise)) {e}"));
    assert_eq!(s("(bitwise-and 12 10)"), "8");
    assert_eq!(s("(arithmetic-shift 1 10)"), "1024");
}

/// All three names denote the same binding, not three implementations.
#[test]
fn test_all_three_libraries_agree() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 151) (srfi 60) (scheme bitwise)) \
             (list (bitwise-and 12 10) (logand 12 10))"
        ),
        "(8 8)"
    );
}

/// Audit item A6 (PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md): an absurd left-shift count must
/// raise a catchable Scheme error. num-bigint allocates the result up front, so
/// unguarded this was a ~137 GB allocation — a process abort rather than an
/// error, and inconsistent with the bignum-count rejection beside it.
#[test]
fn test_arithmetic_shift_rejects_absurd_counts() {
    // A fixnum count past the cap is a catchable error, not an abort.
    assert_eq!(
        srfi151("(guard (e (#t 'caught)) (arithmetic-shift 1 (expt 2 40)))"),
        "caught"
    );
    // A bignum count is rejected just as catchably.
    assert_eq!(
        srfi151("(guard (e (#t 'caught)) (arithmetic-shift 1 (expt 2 80)))"),
        "caught"
    );
    // Generous-but-sane counts still work, and huge right shifts are harmless.
    assert_eq!(
        srfi151("(integer-length (arithmetic-shift 1 100000))"),
        "100001"
    );
    assert_eq!(srfi151("(arithmetic-shift 123 (- (expt 2 40)))"), "0");
}

/// Pin the seam between the fixnum fast path and the bignum path (±2^60).
///
/// The fast path is sound because and/ior/xor/not are closed over the
/// sign-extended fixnum range and shifts re-check narrowing — but nothing
/// else in this suite crosses the boundary, so an edit to either half (or to
/// `fits_fixnum`) would otherwise pass every test. Values straddle the seam
/// deliberately: fixnum operands, boundary operands, and results that must
/// promote.
#[test]
fn test_fixnum_bignum_seam() {
    // fx-greatest = 2^60 - 1, fx-least = -2^60 (61-bit fixnums).
    let greatest = "1152921504606846975";
    let least = "-1152921504606846976";
    // Closure of the fast path at the extremes.
    assert_eq!(srfi151(&format!("(bitwise-and {least} -1)")), least);
    assert_eq!(srfi151(&format!("(bitwise-not {least})")), greatest);
    assert_eq!(srfi151(&format!("(bitwise-not {greatest})")), least);
    assert_eq!(srfi151(&format!("(bitwise-ior {greatest} {least})")), "-1");
    assert_eq!(srfi151(&format!("(bitwise-xor {greatest} {least})")), "-1");
    // Left shifts: the last one that stays a fixnum, and the first that must
    // promote — the narrowing bail inside the fast path.
    assert_eq!(srfi151("(arithmetic-shift 1 59)"), "576460752303423488");
    assert_eq!(srfi151("(arithmetic-shift 1 60)"), "1152921504606846976");
    assert_eq!(srfi151("(arithmetic-shift -1 60)"), least);
    assert_eq!(srfi151("(arithmetic-shift -1 61)"), "-2305843009213693952");
    // Right shifts stay on the fast path even past the width: positive
    // operands drain to 0, negative ones sign-fill to -1.
    assert_eq!(srfi151(&format!("(arithmetic-shift {greatest} -100)")), "0");
    assert_eq!(srfi151(&format!("(arithmetic-shift {least} -100)")), "-1");
    // Mixed fixnum/bignum operands must fall back, not misclassify.
    assert_eq!(
        srfi151(&format!("(bitwise-and (expt 2 61) {greatest})")),
        "0"
    );
    // bit-set? at and past the sign boundary.
    assert_eq!(srfi151("(bit-set? 60 -1)"), "#t");
    assert_eq!(srfi151(&format!("(bit-set? 60 {greatest})")), "#f");
    assert_eq!(srfi151(&format!("(bit-set? 200 {least})")), "#t");
    // bit-count / integer-length at the extremes. For a negative argument
    // bit-count counts zero bits: (bitwise-not least) = greatest = 60 ones.
    assert_eq!(srfi151(&format!("(bit-count {greatest})")), "60");
    assert_eq!(srfi151(&format!("(bit-count {least})")), "60");
    assert_eq!(srfi151(&format!("(integer-length {greatest})")), "60");
    assert_eq!(srfi151(&format!("(integer-length {least})")), "60");
}
