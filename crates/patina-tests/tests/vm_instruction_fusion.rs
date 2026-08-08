//! Semantic tests for the P5 fused/immediate instruction forms
//! (`TestJumpUnless`, `AddImm`/`SubImm`/`LtImm`/`NumEqImm`) and return
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
fn fused_predicates_take_both_arms() {
    // Every fusable predicate, both outcomes, through a fused branch.
    assert_eq!(
        eval(
            "(define (t-null x) (if (null? x) 'y 'n)) \
             (define (t-pair x) (if (pair? x) 'y 'n)) \
             (define (t-vec x) (if (vector? x) 'y 'n)) \
             (define (t-eq a b) (if (eq? a b) 'y 'n)) \
             (define (t-lt a b) (if (< a b) 'y 'n)) \
             (define (t-numeq a b) (if (= a b) 'y 'n)) \
             (list (t-null '()) (t-null 1) \
                   (t-pair '(1)) (t-pair 1) \
                   (t-vec #(1)) (t-vec 1) \
                   (t-eq 'a 'a) (t-eq 'a 'b) \
                   (t-lt 1 2) (t-lt 2 1) \
                   (t-numeq 1 1) (t-numeq 1 2))"
        ),
        "(y n y n y n y n y n y n)"
    );
}

#[test]
fn fused_comparison_falls_back_for_non_fixnums() {
    // The fixnum fast path can't judge these, so the fused branch falls
    // through to the registry handler and the kept JumpUnless — the result
    // must match the unfused opcode exactly, across the numeric tower.
    assert_eq!(
        eval(
            "(define (t-lt a b) (if (< a b) 'y 'n)) \
             (define (t-numeq a b) (if (= a b) 'y 'n)) \
             (list (t-lt 1.5 2) (t-lt 2 1.5) (t-lt 1/2 3/4) \
                   (t-numeq 1.0 1) (t-numeq 1/2 1/2) (t-numeq 2.5 1))"
        ),
        "(y n y y y n)"
    );
}

/// Evaluate on the VM and return the error text (panics if it succeeds).
fn eval_err(code: &str) -> String {
    let interp = patina_interpreter::Interpreter::new(patina_vm::VmBackend::new());
    match interp.eval_program(code) {
        Ok(v) => panic!("expected an error, got: {v:?}\n{code}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn fused_comparison_type_error_matches_unfused() {
    // The fused site's slow path calls the same registry handler as the
    // unfused opcode, so a type error must read identically. `(if (< a b) …)`
    // fuses; returning `(< a b)` does not.
    let fused = eval_err("(define (f a b) (if (< a b) 1 0)) (f 1 'x)");
    let unfused = eval_err("(define (g a b) (< a b)) (g 1 'x)");
    assert_eq!(fused, unfused, "fused site must report the same error");
}

#[test]
fn fused_predicate_deoptimizes_on_rebind() {
    // Rebinding a fused predicate must change what an already-compiled
    // branch site tests: null? becomes "is it the symbol none?".
    assert_eq!(
        eval(
            "(define (f x) (if (null? x) 'empty 'full)) \
             (define r1 (f '())) \
             (define null? (lambda (v) (eq? v 'none))) \
             (list r1 (f '()) (f 'none))"
        ),
        "(empty full empty)"
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
