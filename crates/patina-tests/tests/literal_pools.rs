//! A form's literals live in its compilation's literal pool, which roots
//! them from the form's expansion until its code holds them, and is gone
//! before the form runs (#705; M2 in `PRD/GC_PRD.md` §17). So after every
//! evaluation, on every path a form is compiled by, the heap's root set is
//! as it was: no pool outlives its compilation, and nothing it held stays
//! live through it.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
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
    let registered = || interp.heap().borrow().root_set().registered();
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
