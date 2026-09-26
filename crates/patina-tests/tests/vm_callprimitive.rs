//! Semantic tests for `CallPrimitive` dispatch on the VM backend (Track P P2).
//!
//! These run against `VmBackend` explicitly rather than the both-backends
//! common helpers: the deopt behavior under test only exists in the VM.

mod common;
use common::eval_program_vm as eval;

#[test]
fn fast_path_basics() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(car '(a b))"), "a");
    assert_eq!(eval("(cons 1 2)"), "(1 . 2)");
    assert_eq!(eval("(vector-ref #(10 20 30) 1)"), "20");
    assert_eq!(eval("(define (f x y) (* (+ x 1) y)) (f 4 2)"), "10");
}

#[test]
fn import_rebind_deoptimizes() {
    // PRD P8.1: rebinding a primitive name via `import` (not define/set!)
    // must deoptimize already-compiled CallPrimitive/inline sites exactly
    // like a top-level redefinition does. Rebinds car to scheme.base's cdr;
    // the tree-walker (name lookup per call) returns (2) here.
    assert_eq!(
        eval(
            "(define (f p) (car p)) \
             (import (rename (only (scheme base) cdr) (cdr car))) \
             (f '(1 2))"
        ),
        "(2)"
    );
}

#[test]
fn tail_deopt_returns_correct_result() {
    // PRD P8.2: a rebound primitive at a tail-position site must dispatch
    // with tail-call semantics. `(car p)` is `count`'s entire body (tail
    // position → compiled to `Car; Return`); after the rebind every call
    // deopts to the closure through the tail path.
    assert_eq!(
        eval(
            "(define (count p) (car p)) \
             (define car (lambda (p) (if (null? (cdr p)) 0 (+ 1 (count (cdr p)))))) \
             (count (list 1 2 3 4))"
        ),
        "3"
    );
}

#[test]
fn tail_deopt_runs_deep_mutual_recursion() {
    // PRD P8.2: 100k-deep mutual tail recursion through a rebound
    // tail-position primitive site. With frame reuse this runs in constant
    // space; the flat-RSS property is verified out-of-band (PRD P8.2
    // acceptance), this test pins the semantics at depth.
    assert_eq!(
        eval(
            "(define (loop n) (car n)) \
             (define car (lambda (n) (if (= n 0) 'done (loop (- n 1))))) \
             (loop 100000)"
        ),
        "done"
    );
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

/// The sequence a head-position `call-with-values` or `dynamic-wind` compiles
/// to (#442) is guarded like a `CallPrimitive` site: compiled while the name
/// is the form's procedure, it calls what the name holds once that changes.
/// Both positions, because the sequence ends differently in each: in tail
/// position the ordinary call it falls back to is a `TailCall`.
#[test]
fn control_forms_define_after_use_deoptimize() {
    assert_eq!(
        eval(
            "(define (f) (call-with-values (lambda () (values 1 2)) list)) \
             (define (f-inner) (car (list (call-with-values (lambda () (values 1 2)) list)))) \
             (define (g) (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))) \
             (define (g-inner) (car (list (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))))) \
             (define before (list (f) (f-inner) (g) (g-inner))) \
             (define (call-with-values p c) 'mine) \
             (define (dynamic-wind a b c) 'mine) \
             (list before (f) (f-inner) (g) (g-inner))"
        ),
        "(((1 2) (1 2) body body) mine mine mine mine)"
    );
}

/// The same through `set!`, and back: restoring the procedure restores the
/// answer, though not the sequence — the shadow mark stays set, and the site
/// calls the procedure through its value form from then on.
#[test]
fn control_forms_set_after_use_deoptimize() {
    assert_eq!(
        eval(
            "(define saved-cwv call-with-values) \
             (define saved-dw dynamic-wind) \
             (define (f) (call-with-values (lambda () (values 1 2)) list)) \
             (define (g) (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))) \
             (define before (list (f) (g))) \
             (set! call-with-values (lambda (p c) 'assigned)) \
             (set! dynamic-wind (lambda (a b c) 'assigned)) \
             (define during (list (f) (g))) \
             (set! call-with-values saved-cwv) \
             (set! dynamic-wind saved-dw) \
             (list before during (f) (g))"
        ),
        "(((1 2) body) (assigned assigned) (1 2) body)"
    );
}

/// Re-pointing an alias does not write the binding it previously reached,
/// so no shadow mark is set and a fast path must never trust that alias.
/// Found in review of #442 for `call-with-values` and ordinary primitives.
/// The original Scheme fixture used bare-name access to an introduced
/// global, removed by #427. Install and re-point aliases directly to keep
/// testing the compiler's alias guard independently of that old policy.
#[test]
fn a_re_pointed_alias_is_never_a_fast_path() {
    use patina_runtime::Backend;

    let interp = common::vm_interpreter();
    let env = interp.backend().global_env();
    env.define_alias("helper", env.clone(), "call-with-values".into());
    env.define_alias("h2", env.clone(), "car".into());
    interp
        .eval_program(
            "(define (f) (helper (lambda () 1) (lambda (x) (list 'cwv x)))) \
             (define (g) (h2 '(1 2))) \
             (define (second p c) 'second)",
        )
        .unwrap();
    env.define_alias("helper", env.clone(), "second".into());
    env.define_alias("h2", env.clone(), "cdr".into());
    assert_eq!(
        interp
            .eval_program("(equal? (list (f) (g)) '(second (2)))")
            .unwrap(),
        patina_core::TaggedValue::TRUE
    );
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
    // member's predicate form re-enters the VM from a CallPrimitive dispatch.
    // (vector-map used to be the example here, but it is an ordinary Scheme
    // closure now — the continuation-broken Rust version was deleted.) So is
    // `(scheme base)`'s `member` since #471, so this calls the primitive under
    // it, from `(patina internal lists)`.
    assert_eq!(
        eval(
            "(import (rename (only (patina internal lists) member) (member prim-member))) \
             (prim-member 2.0 '(1 2 3) =)"
        ),
        "(2 3)"
    );
    // `force` rode the same path until #476. It is a VM control primitive
    // now, excluded from `CallPrimitive` (`primitive_calls::is_excluded`),
    // so this row only pins that the exclusion leaves the call working.
    assert_eq!(
        eval("(import (scheme lazy)) (force (delay (+ 20 22)))"),
        "42"
    );
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
    let interp = patina_interpreter::Interpreter::new(patina_vm::VmBackend::new());
    let err = interp
        .eval_program("(car '(1) '(2))")
        .expect_err("arity error expected")
        .to_string();
    assert!(err.contains("car"), "unexpected message: {err}");
}
