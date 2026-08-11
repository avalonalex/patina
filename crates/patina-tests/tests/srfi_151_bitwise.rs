//! SRFI 151 bitwise operations, plus the two legacy spellings the ecosystem
//! actually imports.
//!
//! Bundled because it is the largest measured gap in the vendored corpus:
//! `(srfi 60)` has an in-degree of 31 and `(srfi 33)` 19, against SRFI 151's
//! standard-track status. All three are the same operations under three sets of
//! names, so 60 and 33 are renames over 151 rather than separate ports.
//!
//! The core operators are Rust primitives. Two's-complement semantics over
//! unbounded integers is the part worth testing: negative operands behave as if
//! sign-extended infinitely to the left, and bignums must behave like fixnums.

mod common;
use common::eval_program as eval;

fn srfi151(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 151)) {expr}"))
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

/// Must behave identically past the fixnum boundary, since the primitives
/// promote to bignums rather than special-casing.
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
    let s = |e: &str| eval(&format!("(import (scheme base) (srfi 60)) {e}"));
    assert_eq!(s("(logand 12 10)"), "8");
    assert_eq!(s("(logior 12 10)"), "14");
    assert_eq!(s("(logxor 12 10)"), "6");
    assert_eq!(s("(lognot 12)"), "-13");
    assert_eq!(s("(ash 1 10)"), "1024");
    assert_eq!(s("(logcount 12)"), "2");
    assert_eq!(s("(logbit? 2 12)"), "#t");
    assert_eq!(s("(log2-binary-factors 8)"), "3");
    assert_eq!(s("(integer->list 6)"), "(#f #t #t)");
    assert_eq!(s("(booleans->integer #f #t #t)"), "6");
}

/// SRFI 60 exports both spellings of eight operators -- `logand` *and*
/// `bitwise-and`, `ash` *and* `arithmetic-shift`. Easy to lose one half.
#[test]
fn test_srfi_60_exports_both_spellings() {
    let s = |e: &str| eval(&format!("(import (scheme base) (srfi 60)) {e}"));
    assert_eq!(s("(bitwise-and 12 10)"), "8");
    assert_eq!(s("(arithmetic-shift 1 4)"), "16");
    assert_eq!(s("(bit-count 12)"), "2");
    assert_eq!(s("(list (logand 12 10) (bitwise-and 12 10))"), "(8 8)");
}

#[test]
fn test_srfi_33_spellings() {
    let s = |e: &str| eval(&format!("(import (scheme base) (srfi 33)) {e}"));
    assert_eq!(s("(bitwise-merge 3 1 8)"), "9");
    assert_eq!(s("(any-bits-set? 12 10)"), "#t");
    assert_eq!(s("(all-bits-set? 4 6)"), "#t");
    assert_eq!(s("(extract-bit-field 4 0 255)"), "15");
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

/// Audit item A6 (PRD/AUDIT_2026_08_10_PRD.md): an absurd left-shift count must
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
