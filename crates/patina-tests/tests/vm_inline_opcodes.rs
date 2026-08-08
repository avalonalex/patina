//! Semantic tests for the inline primitive opcodes (Track P P3).
//!
//! Every opcode is exercised on both paths: the inline fast path (fixnums,
//! native pairs, in-bounds vectors) and the fallback to the registry handler
//! (floats, bignums, overflow, type errors, rebound names). Results and
//! error behavior must be indistinguishable from the generic call path.

use patina_interpreter::Interpreter;
use patina_vm::VmBackend;

mod common;
use common::eval_program_vm as eval;

fn eval_err(code: &str) -> String {
    Interpreter::new(VmBackend::new())
        .eval_program(code)
        .expect_err("expected error")
        .to_string()
}

// ── Arithmetic: fast path, promotion, inexact fallback ───────────────────────

#[test]
fn arithmetic_fixnum_fast_path() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(- 10 4)"), "6");
    assert_eq!(eval("(* 6 7)"), "42");
    assert_eq!(eval("(+ -3 3)"), "0");
}

#[test]
fn arithmetic_overflow_promotes_to_bignum() {
    // Same expectations as the variadic handler: overflow leaves fixnum range.
    assert_eq!(eval("(* 4611686018427387903 2)"), "9223372036854775806");
    assert_eq!(
        eval("(* 4611686018427387903 4611686018427387903)"),
        "21267647932558653957237540927630737409"
    );
    assert_eq!(
        eval("(- (- 4611686018427387904) 1)"),
        "-4611686018427387905"
    );
}

#[test]
fn arithmetic_inexact_fallback() {
    assert_eq!(eval("(+ 1.5 2)"), "3.5");
    assert_eq!(eval("(* 2 0.5)"), "1.0");
    assert_eq!(eval("(- 1/2 1/4)"), "1/4");
}

#[test]
fn arithmetic_type_error_matches_generic_path() {
    // The inline opcode's error must be the handler's error, verbatim —
    // compare against the 3-arg shape, which stays on CallPrimitive.
    let inline_err = eval_err("(+ 'a 2)");
    let generic_err = eval_err("(+ 'a 2 3)");
    assert_eq!(inline_err, generic_err, "inline vs generic error text");
}

// ── Comparisons ──────────────────────────────────────────────────────────────

#[test]
fn comparisons() {
    assert_eq!(eval("(< 1 2)"), "#t");
    assert_eq!(eval("(< 2 1)"), "#f");
    assert_eq!(eval("(< 1.5 2)"), "#t");
    assert_eq!(eval("(= 2 2)"), "#t");
    assert_eq!(eval("(= 2.0 2)"), "#t");
    assert_eq!(eval("(= 2 3)"), "#f");
}

// ── Pairs ────────────────────────────────────────────────────────────────────

#[test]
fn pairs() {
    assert_eq!(eval("(car '(a b))"), "a");
    assert_eq!(eval("(cdr '(a b))"), "(b)");
    assert_eq!(eval("(cons 1 2)"), "(1 . 2)");
    assert_eq!(eval("(car (cons 'x 'y))"), "x");
}

#[test]
fn pair_type_errors_catchable() {
    assert_eq!(eval("(guard (e (#t 'caught)) (car 5))"), "caught");
    assert_eq!(eval("(guard (e (#t 'caught)) (cdr \"s\"))"), "caught");
}

// ── Predicates and eq? ───────────────────────────────────────────────────────

#[test]
fn predicates() {
    assert_eq!(eval("(null? '())"), "#t");
    assert_eq!(eval("(null? '(1))"), "#f");
    assert_eq!(eval("(pair? '(1))"), "#t");
    assert_eq!(eval("(pair? 5)"), "#f");
    assert_eq!(eval("(vector? #(1))"), "#t");
    assert_eq!(eval("(vector? '(1))"), "#f");
}

#[test]
fn not_is_total_on_every_value() {
    // Only #f is falsy; everything else — including 0, '(), and "" — negates
    // to #f. The inline fast path is total, so also exercise the handler via
    // the higher-order (non-callee) position.
    assert_eq!(eval("(not #f)"), "#t");
    assert_eq!(eval("(not #t)"), "#f");
    assert_eq!(eval("(not 0)"), "#f");
    assert_eq!(eval("(not '())"), "#f");
    assert_eq!(eval("(not \"\")"), "#f");
    assert_eq!(eval("(not not)"), "#f");
    assert_eq!(eval("(map not '(#f 1 #f))"), "(#t #f #t)");
}

#[test]
fn rebinding_deoptimizes_not() {
    assert_eq!(
        eval("(define (f x) (not x)) (define not (lambda (x) 'mine)) (f #f)"),
        "mine"
    );
}

// ── Moved-to-registry procedures: cxr compositions, numeric predicates ──────

#[test]
fn cxr_compositions() {
    assert_eq!(eval("(cadr '(1 2 3))"), "2");
    assert_eq!(eval("(cddr '(1 2 3))"), "(3)");
    assert_eq!(eval("(caar '((1 2) 3))"), "1");
    assert_eq!(eval("(caddr '(1 2 3))"), "3");
    assert_eq!(eval("(cdadr '(1 (2 3) 4))"), "(3)");
    // Error behavior chains through car/cdr, same as the old Scheme bodies.
    assert_eq!(eval("(guard (e (#t 'caught)) (cadr '(1)))"), "caught");
    assert_eq!(eval("(guard (e (#t 'caught)) (caar 'x))"), "caught");
}

#[test]
fn numeric_predicates() {
    assert_eq!(eval("(zero? 0)"), "#t");
    assert_eq!(eval("(zero? 0.0)"), "#t");
    assert_eq!(eval("(zero? 1/2)"), "#f");
    assert_eq!(eval("(positive? 3)"), "#t");
    assert_eq!(eval("(positive? -3)"), "#f");
    assert_eq!(eval("(negative? -1/2)"), "#t");
    assert_eq!(eval("(odd? 7)"), "#t");
    assert_eq!(eval("(odd? -3)"), "#t");
    assert_eq!(eval("(even? -4)"), "#t");
    assert_eq!(eval("(even? 2.0)"), "#t");
    // Bignums take the slow path.
    assert_eq!(eval("(odd? 100000000000000000000001)"), "#t");
    assert_eq!(
        eval("(zero? (- 100000000000000000000000 100000000000000000000000))"),
        "#t"
    );
    // Domain errors are catchable, as with the old Scheme definitions.
    assert_eq!(eval("(guard (e (#t 'caught)) (zero? 'a))"), "caught");
    assert_eq!(eval("(guard (e (#t 'caught)) (odd? 1.5))"), "caught");
}

#[test]
fn eq_p() {
    assert_eq!(eval("(eq? 'a 'a)"), "#t");
    assert_eq!(eval("(eq? 'a 'b)"), "#f");
    assert_eq!(eval("(eq? 1 1)"), "#t");
    assert_eq!(eval("(let ((x '(1))) (eq? x x))"), "#t");
}

// ── Vectors ──────────────────────────────────────────────────────────────────

#[test]
fn vector_ref_and_set() {
    assert_eq!(eval("(vector-ref #(10 20 30) 1)"), "20");
    assert_eq!(
        eval("(let ((v (vector 1 2 3))) (vector-set! v 0 99) v)"),
        "#(99 2 3)"
    );
}

#[test]
fn vector_bounds_error_matches_generic_path() {
    // Out-of-bounds goes through the handler on both shapes; messages match.
    let inline_err = eval_err("(vector-ref (vector 1) 99)");
    assert!(
        inline_err.contains("out of bounds"),
        "unexpected: {inline_err}"
    );
    assert_eq!(
        eval("(guard (e (#t 'caught)) (vector-set! (vector 1) 99 0))"),
        "caught"
    );
}

// ── Redefinition deopt through inline opcodes ────────────────────────────────

#[test]
fn rebinding_deoptimizes_inline_arithmetic() {
    assert_eq!(eval("(define (g a b) (+ a b)) (set! + -) (g 10 3)"), "7");
    assert_eq!(
        eval("(define (h a b) (* a b)) (define * (lambda (a b) 'shadowed)) (h 2 3)"),
        "shadowed"
    );
}

#[test]
fn rebinding_deoptimizes_inline_pair_ops() {
    assert_eq!(
        eval("(define (f p) (car p)) (define car (lambda (p) 'mine)) (f '(1 2))"),
        "mine"
    );
    assert_eq!(
        eval("(define (f a b) (cons a b)) (define cons (lambda (a b) 'mine)) (f 1 2)"),
        "mine"
    );
}

#[test]
fn rebinding_is_per_primitive() {
    // Rebinding + must not disturb -'s inline fast path.
    assert_eq!(eval("(define (f a b) (- a b)) (set! + *) (f 10 3)"), "7");
}

// ── Interaction with higher-order and tail positions ─────────────────────────

#[test]
fn inline_ops_in_tail_position() {
    assert_eq!(
        eval("(define (last-add a b) (+ a b)) (last-add 20 22)"),
        "42"
    );
    assert_eq!(
        eval("(define (nth v i) (vector-ref v i)) (nth #(1 2 3) 2)"),
        "3"
    );
}

#[test]
fn deep_recursion_through_inline_ops() {
    // fib-shaped recursion: every op is an inline opcode.
    assert_eq!(
        eval("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 20)"),
        "6765"
    );
}
