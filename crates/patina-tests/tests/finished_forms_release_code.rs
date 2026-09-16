//! The VM lets go of a top-level form's compiled code once nothing can run it
//! again, and keeps it for as long as something can (#338).
//!
//! It used to keep the code and constants of every form it had run, about 600
//! bytes each: 200,000 `(display 1)` forms held 142 MB where the tree-walker
//! held 28. A frame or a captured continuation holds its code itself, while a
//! closure names its code by id, so the VM counts the closures naming each code
//! object and lets a form's code go when no frame, continuation or closure
//! needs any of it.
//!
//! Freeing code something can still run is the failure that matters, so most
//! of these hold on to a form's code in one of the ways it can be reached,
//! run many forms and a collection past it, and then run it.
//!
//! Collections are asked for with `(gc)`, which runs one at the next safe point
//! whatever the collector's mode — `PATINA_GC=0`, the differential lane's
//! no-collection reference, included — so what these test does not depend on
//! the environment the suite is run in.

mod common;

use common::vm_interpreter;
use patina_interpreter::Interpreter;
use patina_vm::VmBackend;

/// A VM interpreter with `(scheme base)`, and `(patina debug)` for `(gc)`.
fn interpreter() -> Interpreter<VmBackend> {
    let interp = vm_interpreter();
    interp
        .eval_program("(import (scheme base) (patina debug))")
        .unwrap();
    interp
}

/// Evaluate `program` and return the fixnum it produces.
fn fixnum(interp: &Interpreter<VmBackend>, program: &str) -> i64 {
    let value = interp
        .eval_program(program)
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    value
        .as_fixnum()
        .unwrap_or_else(|| panic!("{program}: not a fixnum"))
}

/// Run a collection, at a safe point inside the form that asks for it.
fn collect(interp: &Interpreter<VmBackend>) {
    interp.eval_program("(begin (gc) (cons 1 2))").unwrap();
}

/// Run many forms that leave nothing behind, then a collection, so whatever
/// code can go has had its chance to.
fn run_forms_and_collect(interp: &Interpreter<VmBackend>) {
    for _ in 0..500 {
        interp.eval_program("(+ 1 2)").unwrap();
    }
    collect(interp);
}

fn loaded(interp: &Interpreter<VmBackend>) -> usize {
    interp.backend().loaded_code_objects()
}

#[test]
fn a_form_that_leaves_nothing_behind_lets_its_code_go() {
    let interp = interpreter();
    let before = loaded(&interp);
    for _ in 0..2_000 {
        interp.eval_program("(+ 1 2)").unwrap();
    }
    let after = loaded(&interp);
    assert!(
        after <= before + 1,
        "{before} code objects before 2,000 forms and {after} after"
    );
}

/// A closure that has become garbage stops counting once the collector frees
/// it, and its form's code goes then.
#[test]
fn a_form_whose_closures_have_died_lets_its_code_go_at_a_collection() {
    let interp = interpreter();
    collect(&interp);
    let before = loaded(&interp);
    for _ in 0..500 {
        interp
            .eval_program("(car (map (lambda (x) (+ x 1)) (list 1 2 3)))")
            .unwrap();
    }
    collect(&interp);
    let after = loaded(&interp);
    assert!(
        after <= before + 2,
        "{before} code objects before 500 forms making closures, and {after} after a collection"
    );
}

/// A continuation that has died stops holding its form's code, which goes at
/// the next collection even though no closure died with it — `keep` is made
/// once, so these forms free none.
#[test]
fn a_form_whose_continuation_has_died_lets_its_code_go_at_a_collection() {
    let interp = interpreter();
    interp
        .eval_program("(define saved #f) (define (keep k) (set! saved k))")
        .unwrap();
    collect(&interp);
    let before = loaded(&interp);
    for _ in 0..200 {
        interp.eval_program("(call/cc keep)").unwrap();
    }
    assert!(
        loaded(&interp) >= before + 199,
        "each form's code is held by the continuation it captured, while that lives"
    );
    interp.eval_program("(set! saved #f)").unwrap();
    collect(&interp);
    let after = loaded(&interp);
    assert!(
        after <= before + 2,
        "{before} code objects before 200 continuations, and {after} after they died and a collection ran"
    );
}

#[test]
fn a_closure_keeps_the_code_it_runs() {
    let interp = interpreter();
    interp.eval_program("(define (double x) (* x 2))").unwrap();
    run_forms_and_collect(&interp);
    assert_eq!(fixnum(&interp, "(double 21)"), 42);
}

/// A lambda nested in code that has finished running can still be made into a
/// closure later, by a closure of the code around it — so a form's code is kept
/// or let go whole.
#[test]
fn a_lambda_can_be_made_after_the_form_around_it_has_finished() {
    let interp = interpreter();
    interp
        .eval_program("(define (adder n) (lambda (x) (+ x n)))")
        .unwrap();
    interp
        .eval_program("(define (maker) (lambda () (lambda () 42)))")
        .unwrap();
    run_forms_and_collect(&interp);
    assert_eq!(fixnum(&interp, "((adder 1) 41)"), 42);
    assert_eq!(fixnum(&interp, "(((maker)))"), 42);
}

/// A continuation captured while a form ran holds that form's frames, and so
/// its code, after the form has returned.
#[test]
fn a_captured_continuation_keeps_its_form_s_code() {
    let interp = interpreter();
    interp
        .eval_program("(define k #f) (define runs 0)")
        .unwrap();
    interp
        .eval_program("(begin (call/cc (lambda (c) (set! k c))) (set! runs (+ runs 1)))")
        .unwrap();
    run_forms_and_collect(&interp);
    interp.eval_program("(if (< runs 2) (k #f))").unwrap();
    assert_eq!(fixnum(&interp, "runs"), 2);
}

/// Code compiled by `eval` at run time is a form's code like any other, kept by
/// the closure `eval` returned.
#[test]
fn a_closure_eval_returned_keeps_its_code() {
    let interp = interpreter();
    interp
        .eval_program(
            "(import (scheme eval)) \
             (define triple (eval '(lambda (x) (* x 3)) (environment '(scheme base))))",
        )
        .unwrap();
    run_forms_and_collect(&interp);
    assert_eq!(fixnum(&interp, "(triple 14)"), 42);
}
