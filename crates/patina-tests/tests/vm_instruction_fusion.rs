//! Semantic tests for the P5 fused/immediate instruction forms
//! (`NotJumpUnless`, `AddImm`/`SubImm`/`LtImm`/`NumEqImm`) and return
//! threading. These run against `VmBackend` explicitly — the fusions only
//! exist in the VM — and pin the R7RS redefinition semantics the deopt
//! paths must preserve.

mod common;
use common::eval_program_vm as eval;

#[test]
fn fused_not_branch_takes_both_arms() {
    assert_eq!(
        eval("(define (f a b) (if (not (< a b)) 'ge 'lt)) (list (f 2 1) (f 1 2))"),
        "(ge lt)"
    );
}

#[test]
fn fused_not_branch_deoptimizes_on_rebind() {
    // After `not` is rebound to the identity function, the already-compiled
    // fused site must call the new binding (a closure — the deopt path
    // re-enters the VM) and branch on its result: (f #f) now tests #f
    // itself (falsy → else), (f 17) tests 17 (truthy → then).
    assert_eq!(
        eval(
            "(define (f x) (if (not x) 'a 'b)) \
             (define r1 (f #f)) \
             (define not (lambda (v) v)) \
             (list r1 (f #f) (f 17))"
        ),
        "(a b a)"
    );
}

#[test]
fn imm_forms_compute_and_fall_back() {
    // Fixnum fast paths, plus the non-fixnum operand falling back to the
    // registry handler (float contagion preserved).
    assert_eq!(
        eval(
            "(define (dec x) (- x 1)) \
             (define (inc x) (+ x 1)) \
             (define (small? x) (< x 5)) \
             (define (zero-like? x) (= x 0)) \
             (list (dec 10) (inc 1.5) (small? 4) (small? 5) (zero-like? 0) (zero-like? 1))"
        ),
        "(9 2.5 #t #f #t #f)"
    );
}

#[test]
fn imm_overflow_promotes_to_bignum() {
    // Fixnum overflow on the imm fast path must promote exactly like the
    // register form: fall back to the registry handler.
    assert_eq!(
        eval("(define (inc x) (+ x 1)) (inc 4611686018427387903)"),
        "4611686018427387904"
    );
}

#[test]
fn imm_deopt_preserves_operand_order() {
    // The regression that shaped the design: after `set! - +` the deopt
    // callee is not commutative-compatible with any operand shuffling —
    // `(- x 1)` must reach the rebound procedure as (x, 1) exactly.
    assert_eq!(eval("(define (dec x) (- x 1)) (set! - +) (dec 10)"), "11");
    assert_eq!(eval("(define (g) (+ 40 2)) (set! + -) (g)"), "38");
}

#[test]
fn threaded_return_branches_produce_correct_values() {
    // Both arms of a tail `if` now return directly (Move+Jump threaded to
    // Return); deep recursion through the threaded base case stays correct.
    assert_eq!(
        eval(
            "(define (count n) (if (= n 0) 'done (count (- n 1)))) \
             (count 100000)"
        ),
        "done"
    );
}
