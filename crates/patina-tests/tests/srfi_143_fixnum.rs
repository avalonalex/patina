//! SRFI 143 fixnums.
//!
//! Almost all of it is renames — the bitwise operators from SRFI 151, the
//! arithmetic from `(scheme base)` — because on a fixnum argument those already
//! do the right thing and SRFI 143 leaves non-fixnum arguments unspecified.
//!
//! The part that can actually be wrong is `fx-width` / `fx-greatest` /
//! `fx-least`: they are claims about Patina's representation, and a library
//! that overstates the range is worse than no library. They are derived by
//! probing `fixnum?` rather than hardcoded, so the tests below check the
//! boundary rather than trusting a constant.

mod common;
use common::eval_program as eval;

fn fx(expr: &str) -> String {
    eval(&format!("(import (scheme base) (srfi 143)) {expr}"))
}

/// The boundary claims must match reality, whatever the tagging happens to be.
/// Asserting the invariant rather than the number means retagging Patina moves
/// these automatically instead of leaving a library that lies.
#[test]
fn test_fixnum_range_matches_the_representation() {
    assert_eq!(fx("(fixnum? fx-greatest)"), "#t");
    assert_eq!(fx("(fixnum? (+ fx-greatest 1))"), "#f");
    assert_eq!(fx("(fixnum? fx-least)"), "#t");
    assert_eq!(fx("(fixnum? (- fx-least 1))"), "#f");
    // fx-width counts the sign bit, so the range is +-2^(w-1).
    assert_eq!(fx("(= fx-greatest (- (expt 2 (- fx-width 1)) 1))"), "#t");
    assert_eq!(fx("(= fx-least (- (expt 2 (- fx-width 1))))"), "#t");
}

/// Today's values, recorded so a change to the tagging shows up as a failing
/// test to review rather than passing silently.
#[test]
fn test_current_fixnum_width_is_61_bit() {
    assert_eq!(fx("fx-width"), "61");
    assert_eq!(fx("fx-greatest"), "1152921504606846975");
    assert_eq!(fx("fx-least"), "-1152921504606846976");
}

#[test]
fn test_arithmetic_and_predicates() {
    assert_eq!(fx("(fx+ 2 3)"), "5");
    assert_eq!(fx("(fx- 10 3)"), "7");
    assert_eq!(fx("(fx* 6 7)"), "42");
    assert_eq!(fx("(fxneg 5)"), "-5");
    assert_eq!(fx("(fxabs -5)"), "5");
    assert_eq!(fx("(fxsquare 5)"), "25");
    assert_eq!(fx("(fxquotient 17 5)"), "3");
    assert_eq!(fx("(fxremainder 17 5)"), "2");
    assert_eq!(
        fx("(list (fxzero? 0) (fxpositive? 1) (fxnegative? -1))"),
        "(#t #t #t)"
    );
    assert_eq!(fx("(list (fxodd? 3) (fxeven? 4))"), "(#t #t)");
    assert_eq!(fx("(list (fxmax 1 5 3) (fxmin 1 5 3))"), "(5 1)");
    assert_eq!(fx("(list (fx=? 1 1) (fx<? 1 2) (fx>=? 2 2))"), "(#t #t #t)");
}

#[test]
fn test_bitwise_renames_reach_srfi_151() {
    assert_eq!(fx("(fxand 12 10)"), "8");
    assert_eq!(fx("(fxior 12 10)"), "14");
    assert_eq!(fx("(fxxor 12 10)"), "6");
    assert_eq!(fx("(fxnot 12)"), "-13");
    assert_eq!(fx("(fxbit-count 12)"), "2");
    assert_eq!(fx("(fxlength 12)"), "4");
    assert_eq!(fx("(fxbit-set? 2 12)"), "#t");
    assert_eq!(fx("(fxfirst-set-bit 8)"), "3");
    assert_eq!(fx("(fxbit-field #b1101101010 0 4)"), "10");
    assert_eq!(fx("(fxif 3 1 8)"), "9");
}

/// SRFI 143 has both a signed shift and directional ones; the right shift is
/// the easy one to get backwards.
#[test]
fn test_shifts_in_both_directions() {
    assert_eq!(fx("(fxarithmetic-shift 1 10)"), "1024");
    assert_eq!(fx("(fxarithmetic-shift 1024 -4)"), "64");
    assert_eq!(fx("(fxarithmetic-shift-left 1 10)"), "1024");
    assert_eq!(fx("(fxarithmetic-shift-right 1024 4)"), "64");
    // Right shift is arithmetic, so the sign propagates.
    assert_eq!(fx("(fxarithmetic-shift-right -8 2)"), "-2");
}

/// The carry operators split a result too wide for a fixnum into a fixnum plus
/// a carry, using balanced division so the remainder lands in [-2^w/2, 2^w/2).
#[test]
fn test_carry_operators() {
    assert_eq!(
        fx("(call-with-values (lambda () (fx+/carry 1 2 3)) list)"),
        "(6 0)"
    );
    assert_eq!(
        fx("(call-with-values (lambda () (fx-/carry 10 3 2)) list)"),
        "(5 0)"
    );
    assert_eq!(
        fx("(call-with-values (lambda () (fx*/carry 6 7 0)) list)"),
        "(42 0)"
    );
    // A sum that overflows the fixnum range carries out rather than silently
    // producing a bignum.
    assert_eq!(
        fx(
            "(call-with-values (lambda () (fx+/carry fx-greatest fx-greatest 0)) \
            (lambda (r c) (list (fixnum? r) c)))"
        ),
        "(#t 1)"
    );
}

/// The exact half-point: a wide result of precisely 2^(fx-width-1) must land on
/// fx-least with a carry of 1. A round-based balanced division puts it on the
/// excluded endpoint +2^(fx-width-1) instead (round is half-to-even), returning
/// a non-fixnum from the operators that exist to handle this boundary.
#[test]
fn test_carry_operators_at_the_half_point() {
    assert_eq!(
        fx("(call-with-values (lambda () (fx+/carry fx-greatest 1 0)) \
            (lambda (r c) (list (fixnum? r) (eqv? r fx-least) c)))"),
        "(#t #t 1)"
    );
    assert_eq!(
        fx("(call-with-values (lambda () (fx-/carry fx-least 1 0)) \
            (lambda (r c) (list (fixnum? r) (eqv? r fx-greatest) c)))"),
        "(#t #t -1)"
    );
    // The multiplicative half-point: fx-least * 1 + 0 stays put with no carry.
    assert_eq!(
        fx("(call-with-values (lambda () (fx*/carry fx-least 1 0)) \
            (lambda (r c) (list (eqv? r fx-least) c)))"),
        "(#t 0)"
    );
}

#[test]
fn test_fxsqrt_returns_root_and_remainder() {
    assert_eq!(
        fx("(call-with-values (lambda () (fxsqrt 17)) list)"),
        "(4 1)"
    );
}

/// R7RS-large names this `(scheme fixnum)`.
#[test]
fn test_r7rs_large_alias() {
    let s = |e: &str| eval(&format!("(import (scheme base) (scheme fixnum)) {e}"));
    assert_eq!(s("(fx+ 2 3)"), "5");
    assert_eq!(s("(fxand 12 10)"), "8");
    assert_eq!(s("fx-width"), "61");
}

/// The fx bitwise operators are SRFI 151's, not a second implementation.
#[test]
fn test_shares_bindings_with_srfi_151() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 143) (srfi 151)) \
             (list (fxand 12 10) (bitwise-and 12 10))"
        ),
        "(8 8)"
    );
}

/// The fx ops at the representation's own edges — the fast-path seam.
///
/// This file's method is to check the boundary rather than trust a constant
/// (see the module doc); these extend that to the bitwise aliases, whose
/// shared primitives take an i64 fast path exactly when every operand is a
/// fixnum.
#[test]
fn test_bitwise_aliases_at_the_boundary() {
    assert_eq!(fx("(fxand fx-least -1)"), "-1152921504606846976");
    assert_eq!(fx("(fxnot fx-least)"), "1152921504606846975");
    assert_eq!(fx("(fxnot fx-greatest)"), "-1152921504606846976");
    assert_eq!(fx("(fxxor fx-greatest fx-least)"), "-1");
    assert_eq!(fx("(fxarithmetic-shift 1 59)"), "576460752303423488");
    assert_eq!(fx("(fxarithmetic-shift fx-least -60)"), "-1");
    assert_eq!(fx("(fxbit-count fx-greatest)"), "60");
    assert_eq!(fx("(fxlength fx-greatest)"), "60");
    assert_eq!(fx("(fxbit-set? 60 -1)"), "#t");
    assert_eq!(fx("(fxbit-set? 60 fx-greatest)"), "#f");
}
