//! A form's literals live in its compilation's literal pool, which roots
//! them from the form's expansion until the code that holds them is rooted
//! (#705, #869; M2 in `PRD/GC_PRD.md` §17): on the VM until its unit is
//! loaded, before it runs; on the tree-walker until its run returns, and for
//! an `eval`'d datum's tree until the start of the driver's next
//! collection, the next `eval` or a run's return. So after every evaluation, on every path a form
//! is compiled by, the heap's root set is as it was: no pool outlives the
//! evaluation that compiled it, and nothing it held stays live through it.

mod common;
use common::{eval_program_tree_walker, eval_program_vm, tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter};

/// The forms, each compiled by a different path or with a different kind
/// of literal.
const FORMS: &[&str] = &[
    // A quoted list, and a string and a symbol.
    "'(a b c)",
    "(list \"a string\" 'a-symbol)",
    // A quasiquote, whose constructors are literals the backend adds.
    "(define x 1) `(1 ,x \"s\" #(v ,x))",
    // An import, and a library's body forms.
    "(import (scheme write))",
    "(define-library (literal pool test) (export f) (import (scheme base))
       (begin (define (f) '(x y))))
     (import (literal pool test))
     (f)",
    // `eval`'s expansion, in a mutable environment and an immutable one.
    "(import (scheme eval)) (eval '(list 'a \"b\") (interaction-environment))",
    "(import (scheme eval)) (eval '(list 'c \"d\") (environment '(scheme base)))",
    // A form that fails as it runs, and one that fails to expand.
    "(car '(1 . 2)) (car '())",
    "(if)",
];

fn check<B: Backend>(backend: &str, interp: Interpreter<B>) {
    let registered = || interp.backend().heap().borrow().root_set().registered();
    let before = registered();
    for form in FORMS {
        let _ = interp.eval_program_owned(form);
        assert_eq!(
            registered(),
            before,
            "{backend}: a literal pool outlived its compilation of {form}"
        );
    }
}

#[test]
fn no_literal_pool_outlives_its_compilation() {
    check("vm", vm_interpreter());
    check("tree-walker", tree_walker_interpreter());
}

/// An `eval`'d datum's quoted pair, once its form is done and only an
/// ephemeron's key names it, is let go of by the next collection, inside
/// the run that evaluated it as well as after: nothing the datum's pool held
/// stays live through it once its form is done (M2, #869). On the
/// tree-walker the pool of a `Step::Eval` is retired until the driver's
/// next collection, which must release it before it marks: released after,
/// it kept the pair through that collection, a liveness the VM and the
/// parent of #869 never had. The control is the same pair made with no
/// `eval`.
#[test]
fn an_evald_datum_is_let_go_of_by_the_next_collection() {
    const PROGRAM: &str = "
        (import (scheme base) (scheme eval) (scheme ephemeron) (patina debug))
        (define env (environment '(scheme base)))
        (define (broken-after-one-gc make)
          (let ((e (make-ephemeron (make) 'v)))
            (gc)
            (ephemeron-broken? e)))
        (list (broken-after-one-gc (lambda () (eval (list 'quote (list 1 2)) env)))
              (broken-after-one-gc (lambda () (list 1 2))))";
    assert_eq!(eval_program_vm(PROGRAM), "(#t #t)", "vm");
    assert_eq!(eval_program_tree_walker(PROGRAM), "(#t #t)", "tree-walker");
}

/// `an_evald_datum_is_let_go_of_by_the_next_collection`, with the
/// collection the driver's own at a safe point, which the run reaches by
/// allocating, rather than `(gc)`'s at its call: the tree-walker releases a
/// `Step::Eval`'s retired pool before either marks, at separate sites
/// (`maybe_collect` and `collect_at_call`, #869). Each element says first
/// that a collection ran, so a run that never collected cannot pass, then
/// that the ephemeron's key went.
#[test]
fn an_evald_datum_is_let_go_of_by_the_next_safe_point_collection() {
    const PROGRAM: &str = "
        (import (scheme base) (scheme eval) (scheme ephemeron) (patina debug))
        (define env (environment '(scheme base)))
        (define (collections) (cdr (assq 'collections (gc-stats))))
        (define (broken-after-a-safe-point-collection make)
          (let ((e (make-ephemeron (make) 'v)) (n (collections)))
            (let loop ((i 0))
              (when (and (= (collections) n) (< i 1000000))
                (make-vector 64 i)
                (loop (+ i 1))))
            (list (> (collections) n) (ephemeron-broken? e))))
        (list (broken-after-a-safe-point-collection
               (lambda () (eval (list 'quote (list 1 2)) env)))
              (broken-after-a-safe-point-collection (lambda () (list 1 2))))";
    assert_eq!(eval_program_vm(PROGRAM), "((#t #t) (#t #t))", "vm");
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "((#t #t) (#t #t))",
        "tree-walker"
    );
}
