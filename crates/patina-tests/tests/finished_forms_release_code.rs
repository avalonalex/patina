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
//! The slots in the store that held that code are given to later code (#352),
//! so the store grows with the code loaded at once rather than with the number
//! of forms run; two tests count the slots.
//!
//! Freeing code something can still run is the failure that matters, so most
//! of these hold on to a form's code in one of the ways it can be reached,
//! run many forms and a collection past it, and then run it.
//!
//! Collections are asked for with `(gc)`, which runs one at the next safe point
//! whatever the collector's mode — `PATINA_GC=0`, the differential lane's
//! no-collection reference, included — so what these test does not depend on
//! the environment the suite is run in.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

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

fn slots(interp: &Interpreter<VmBackend>) -> usize {
    interp.backend().code_store_slots()
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

/// The slot a form's code let go of is given to the next form's, so the store
/// grows with the code in use rather than with every form run (#352).
#[test]
fn forms_run_one_after_another_reuse_the_store_s_slots() {
    let interp = interpreter();
    let before = slots(&interp);
    for _ in 0..2_000 {
        interp.eval_program("(+ 1 2)").unwrap();
    }
    let after = slots(&interp);
    assert!(
        after <= before + 1,
        "{before} store slots before 2,000 forms and {after} after"
    );
}

/// A library's body form lets its code go once it has run, as a program's
/// form does, on either loader: a top-level `import`'s, and the nested one
/// `environment` runs. Each body form runs as a closure over the library's
/// environment (#779), which is made retired, so it names no code and holds
/// none: counted, it would keep every body form's code until a collection.
#[test]
fn a_library_body_form_lets_its_code_go_when_it_has_run() {
    const FORMS: usize = 300;
    let root = tempfile::TempDir::new().unwrap();
    let dir = root.path().join("release779");
    std::fs::create_dir(&dir).unwrap();
    let body: String = (0..FORMS).map(|i| format!("(define v{i} {i})\n")).collect();
    for name in ["imported", "nested"] {
        std::fs::write(
            dir.join(format!("{name}.sld")),
            format!(
                "(define-library (release779 {name}) (import (scheme base)) (export v0)
                   (begin {body}))"
            ),
        )
        .unwrap();
    }
    let interp = interpreter();
    interp
        .backend()
        .add_library_search_path(root.path().to_path_buf());
    interp.eval_program("(import (scheme eval))").unwrap();
    for load in [
        "(import (release779 imported))",
        "(environment '(release779 nested))",
    ] {
        let before = loaded(&interp);
        interp.eval_program(load).unwrap();
        let after = loaded(&interp);
        assert!(
            after <= before + 1,
            "{before} code objects before `{load}` ran {FORMS} body forms, and {after} after"
        );
    }
}

/// A host environment's form lets its code go once it has run, as a form of
/// the global environment does. It runs as a closure over the environment,
/// made retired (#779); the closure used to be counted, which kept the code
/// of every form until a collection freed the closure.
#[test]
fn a_host_environment_s_form_lets_its_code_go() {
    let interp = interpreter();
    let env = interp.new_environment();
    interp.eval_str_in(&env, "(+ 1 2)").unwrap();
    let before = loaded(&interp);
    for _ in 0..2_000 {
        interp.eval_str_in(&env, "(+ 1 2)").unwrap();
    }
    let after = loaded(&interp);
    assert!(
        after <= before + 1,
        "{before} code objects before 2,000 forms in a host environment, and {after} after"
    );
}

/// Code held until a collection gives its slots back then: rounds of forms
/// making closures, each ended by a collection, need the slots of one round.
#[test]
fn code_let_go_at_a_collection_gives_its_slots_to_later_forms() {
    const FORMS: usize = 200;
    let interp = interpreter();
    collect(&interp);
    let before = slots(&interp);
    for _ in 0..4 {
        for _ in 0..FORMS {
            interp
                .eval_program("(car (map (lambda (x) (+ x 1)) (list 1 2 3)))")
                .unwrap();
        }
        collect(&interp);
    }
    let after = slots(&interp);
    // Two code objects a form, so one round needs at most 2 * FORMS slots
    // and four without reuse would take 8 * FORMS.
    assert!(
        after <= before + 3 * FORMS,
        "{before} store slots before four rounds of {FORMS} forms making closures, and {after} after"
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
///
/// `keep` keeps every continuation, not only the last: one it let go of
/// could die, and its form's code go, at a collection the collector's mode
/// runs between the forms, which `PATINA_GC_STRESS` and `PATINA_GC_ZEAL` do.
#[test]
fn a_form_whose_continuation_has_died_lets_its_code_go_at_a_collection() {
    let interp = interpreter();
    interp
        .eval_program("(define saved '()) (define (keep k) (set! saved (cons k saved)))")
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
    interp.eval_program("(set! saved '())").unwrap();
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

/// A library whose procedures call themselves and each other, defined again
/// and imported again a hundred times: each replaced library's namespace,
/// its procedures and their code all go, so as many code objects are loaded
/// after a hundred redefinitions and a collection as after ten (4b's start
/// decision 3's gate, #836). A procedure's unit links the procedure's own
/// binding record, whose cell holds the procedure, so a unit's link table
/// traced from the code store would keep every replaced library's
/// procedures, their units and their namespace for good, and the count
/// would grow with the redefinitions: the VM traces a unit's table only
/// through its code id, which only something that can run the code reports.
/// The last answers come from the last library's procedures, which its
/// redefinitions' units, linked against the namespace each was compiled in,
/// do not reach. Under zeal in CI's zeal lane, which runs this file.
#[test]
fn a_redefined_library_s_recursive_procedures_let_their_units_go() {
    const LIBRARY: &str = "(define-library (gate4b9 lib)
          (export f even2? odd2?)
          (import (scheme base))
          (begin
            (define (f n) (if (= n 0) 0 (f (- n 1))))
            (define (even2? n) (if (= n 0) #t (odd2? (- n 1))))
            (define (odd2? n) (if (= n 0) #f (even2? (- n 1))))))
        (import (gate4b9 lib))";
    let interp = interpreter();
    let redefine = |times: usize| {
        for _ in 0..times {
            interp.eval_program(LIBRARY).unwrap();
        }
        collect(&interp);
        loaded(&interp)
    };
    let after_ten = redefine(10);
    let after_hundred = redefine(90);
    assert_eq!(
        after_hundred, after_ten,
        "code objects loaded after 10 redefinitions of a library, and after 100"
    );
    let answers = interp
        .eval_program("(list (f 10) (even2? 10) (odd2? 7))")
        .unwrap();
    assert_eq!(interp.display_tagged(answers), "(0 #t #t)");
}
