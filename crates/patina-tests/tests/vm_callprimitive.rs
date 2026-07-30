//! Semantic tests for `CallPrimitive` dispatch on the VM backend (Track P P2).
//!
//! These run against `VmBackend` explicitly (not the feature-switched common
//! helpers): the deopt behavior under test only exists in the VM.

use patina_interpreter::{Backend, Interpreter};
use patina_vm::VmBackend;

fn eval(code: &str) -> String {
    let interp = Interpreter::new(VmBackend::new());
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {e}\n{code}"));
    let heap = interp.backend().global_env().heap();
    patina_primitives::primitives::io::datum_writer::format_write_tagged(result, heap)
}

#[test]
fn fast_path_basics() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(car '(a b))"), "a");
    assert_eq!(eval("(cons 1 2)"), "(1 . 2)");
    assert_eq!(eval("(vector-ref #(10 20 30) 1)"), "20");
    assert_eq!(eval("(define (f x y) (* (+ x 1) y)) (f 4 2)"), "10");
}

#[test]
fn define_after_use_deoptimizes() {
    // f is compiled while car is the primitive; rebinding car afterwards must
    // change what f calls (R7RS top-level redefinition semantics).
    assert_eq!(
        eval("(define (f p) (car p)) (define car (lambda (p) 'shadowed)) (f '(1 2))"),
        "shadowed"
    );
}

#[test]
fn set_after_use_deoptimizes() {
    assert_eq!(eval("(define (g) (+ 40 2)) (set! + -) (g)"), "38");
}

#[test]
fn deopt_is_per_primitive() {
    // Rebinding car must not disturb cdr's fast path.
    assert_eq!(
        eval("(define (g p) (cdr p)) (define car (lambda (p) 'x)) (g '(1 2))"),
        "(2)"
    );
}

#[test]
fn lexical_shadowing_never_uses_fast_path() {
    assert_eq!(
        eval("(let ((cdr (lambda (x) 'lexical))) (cdr '(1 2)))"),
        "lexical"
    );
}

#[test]
fn control_primitives_still_intercepted() {
    assert_eq!(
        eval("(call-with-current-continuation (lambda (k) (k 'ok)))"),
        "ok"
    );
    assert_eq!(eval("(call-with-values (lambda () (values 1 2)) +)"), "3");
    assert_eq!(
        eval("(dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))"),
        "body"
    );
}

#[test]
fn higher_order_primitive_through_fast_path() {
    // vector-map re-enters the VM from a CallPrimitive dispatch.
    assert_eq!(eval("(vector-map + #(1 2) #(30 40))"), "#(31 42)");
}

#[test]
fn errors_still_catchable() {
    assert_eq!(eval("(guard (e (#t 'caught)) (car 5))"), "caught");
    assert_eq!(
        eval("(guard (e (#t 'caught)) (vector-ref (vector 1) 99))"),
        "caught"
    );
}

#[test]
fn arity_error_message_parity() {
    let interp = Interpreter::new(VmBackend::new());
    let err = interp
        .eval_program("(car '(1) '(2))")
        .expect_err("arity error expected")
        .to_string();
    assert!(err.contains("car"), "unexpected message: {err}");
}
