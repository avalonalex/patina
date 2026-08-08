//! Semantic tests for the inline primitive opcodes (Track P P3) and the
//! procedures P9 moved from Scheme definitions into the registry.
//!
//! Every opcode is exercised on both paths: the inline fast path (fixnums,
//! native pairs, in-bounds vectors) and the fallback to the registry handler
//! (floats, bignums, overflow, type errors, rebound names). Results and
//! error behavior must be indistinguishable from the generic call path.
//! Basic fixnum-value coverage lives in the compliance tests; here we assert
//! only what the move could have changed (slow paths, error behavior, deopt).

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
    // Only #f is falsy (basic cases are in compliance/predicates.rs); the
    // novel cases here: values with no literal syntax, and the handler via
    // the higher-order (non-callee) position.
    assert_eq!(eval("(not \"\")"), "#f");
    assert_eq!(eval("(not not)"), "#f");
    assert_eq!(eval("(map not '(#f 1 #f))"), "(#t #f #t)");
}

// ── Moved-to-registry procedures (P9): cxr compositions, numeric predicates ──

#[test]
fn every_cxr_composition_matches_its_car_cdr_chain() {
    // Generate all 28 compositions (two- through four-deep) and compare each
    // against its hand-inlined car/cdr chain on a path-labeled binary tree.
    // This cross-checks the handler step order, the registration table, and
    // the library exports for every name — a dropped or misordered entry
    // fails here rather than silently at run time.
    let mut program = String::from(
        "(import (scheme base) (scheme cxr)) \
         (define (build d p) \
           (if (= d 0) p (cons (build (- d 1) (* 2 p)) (build (- d 1) (+ (* 2 p) 1))))) \
         (define t (build 5 1)) \
         (list ",
    );
    for depth in 2..=4usize {
        for bits in 0..(1u32 << depth) {
            // Letters outermost-first: bit set → 'd' (cdr), clear → 'a' (car).
            let letters: String = (0..depth)
                .map(|i| {
                    if bits & (1 << (depth - 1 - i)) != 0 {
                        'd'
                    } else {
                        'a'
                    }
                })
                .collect();
            // Chain applies letters right-to-left: caddr x = (car (cdr (cdr x))).
            let mut chain = String::from("t");
            for letter in letters.chars().rev() {
                let op = if letter == 'a' { "car" } else { "cdr" };
                chain = format!("({op} {chain})");
            }
            program.push_str(&format!("(equal? (c{letters}r t) {chain}) "));
        }
    }
    program.push(')');
    let result = eval(&program);
    assert!(
        !result.contains("#f"),
        "some composition diverged from its chain: {result}"
    );
}

#[test]
fn cxr_error_behavior_chains_through_car_cdr() {
    // Same catchable errors as the old Scheme bodies.
    assert_eq!(eval("(guard (e (#t 'caught)) (cadr '(1)))"), "caught");
    assert_eq!(eval("(guard (e (#t 'caught)) (caar 'x))"), "caught");
    assert_eq!(eval("(guard (e (#t 'caught)) (cddddr '(1 2 3)))"), "caught");
}

#[test]
fn numeric_predicates_slow_paths_and_errors() {
    // Fixnum basics live in compliance/numbers.rs; these are the non-fixnum
    // slow paths and error behavior the registry move could have changed.
    assert_eq!(eval("(zero? 0.0)"), "#t");
    assert_eq!(eval("(zero? 1/2)"), "#f");
    assert_eq!(eval("(negative? -1/2)"), "#t");
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
fn rebinding_deoptimizes_not() {
    assert_eq!(
        eval("(define (f x) (not x)) (define not (lambda (x) 'mine)) (f #f)"),
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
