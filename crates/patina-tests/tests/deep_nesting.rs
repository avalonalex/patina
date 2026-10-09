//! Code nested deeply runs, or is refused with an error a program can catch,
//! on both backends. It never aborts the process (#617).
//!
//! Every stage between the reader and a backend walked code by recursing on
//! it, a native frame or more per level and no check, so code nested about
//! 1,000 deep overflowed the main thread's 8 MiB and aborted the process, and
//! on a 2 MiB thread nested `let` aborted from 243 levels. The walks now move
//! to a new stack segment when theirs runs low
//! (`patina_core::walk::ensure_sufficient_stack`), and the desugarer refuses a
//! form nested past `patina_core::walk::MAX_FORM_DEPTH`.
//!
//! The programs are generated, not checked in. Each runs on a thread with a
//! 2 MiB stack, the size `std::thread::spawn` gives a thread, set here rather
//! than left to the harness: an overflow aborts the whole test binary instead
//! of failing one test, which is also why these tests have a binary of their
//! own.
//!
//! The shapes no macro expands run 1,000 deep. `let`, `cond` and `and` run
//! 400 deep, past the 243 that aborted: expanding a macro copies what it
//! substitutes, so their cost grows with the square of their depth, and 1,000
//! nested `let`s take seconds even in a release build.

mod common;
use common::{try_eval_program_tree_walker, try_eval_program_vm};

/// The stack each test runs on.
const STACK: usize = 2 * 1024 * 1024;

/// A shape of nesting: what opens a level, what closes it, and the innermost
/// expression.
struct Shape {
    name: &'static str,
    open: &'static str,
    close: &'static str,
    innermost: &'static str,
}

const LET: Shape = Shape {
    name: "let",
    open: "(let ((a 1)) ",
    close: ")",
    innermost: "a",
};
const COND: Shape = Shape {
    name: "cond",
    open: "(cond (#t ",
    close: "))",
    innermost: "1",
};
const AND: Shape = Shape {
    name: "and",
    open: "(and #t ",
    close: ")",
    innermost: "1",
};
const THUNK: Shape = Shape {
    name: "thunk",
    open: "((lambda () ",
    close: "))",
    innermost: "1",
};
const LAMBDA: Shape = Shape {
    name: "lambda",
    open: "((lambda (a) ",
    close: ") 1)",
    innermost: "a",
};
const BEGIN: Shape = Shape {
    name: "begin",
    open: "(begin ",
    close: ")",
    innermost: "1",
};
const PLUS: Shape = Shape {
    name: "plus",
    open: "(- 2 ",
    close: ")",
    innermost: "1",
};
const IF: Shape = Shape {
    name: "if",
    open: "(if #t ",
    close: " 0)",
    innermost: "1",
};

/// The shapes a macro expands, at the depth they run here.
const EXPANDED: [&Shape; 3] = [&LET, &COND, &AND];

/// The shapes nothing expands but the desugarer's core forms.
const CORE: [&Shape; 5] = [&THUNK, &LAMBDA, &BEGIN, &PLUS, &IF];

/// A program whose one expression is `shape` nested `depth` deep. Inside a
/// call, so that a `begin` is an expression's and not spliced into the top
/// level, where nesting it is no nesting at all.
fn nested(shape: &Shape, depth: usize) -> String {
    format!(
        "(import (scheme base))\n(values {}{}{})",
        shape.open.repeat(depth),
        shape.innermost,
        shape.close.repeat(depth)
    )
}

/// A program that builds a datum nested `depth` deep with `quotation` and
/// measures it, without asking `write` to traverse it.
fn measured(quotation: &str, innermost: &str, depth: usize) -> String {
    format!(
        "(import (scheme base))\n\
         (let loop ((x {quotation}{}{innermost}{}) (n 0))\n\
           (if (pair? x) (loop (car x) (+ n 1)) (list n x)))",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

/// Run `test` on a thread with a [`STACK`]-sized stack.
fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(test)
        .expect("spawn");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

/// `program`'s value on each backend, or its error.
fn on_both_backends(program: &str) -> [(&'static str, Result<String, String>); 2] {
    [
        ("vm", try_eval_program_vm(program)),
        ("tree-walker", try_eval_program_tree_walker(program)),
    ]
}

fn assert_runs(program: &str, expected: &str, case: &str) {
    for (backend, outcome) in on_both_backends(program) {
        assert_eq!(outcome.as_deref(), Ok(expected), "{case} on the {backend}");
    }
}

#[test]
fn code_that_nested_past_the_old_stack_limit_runs_on_a_small_stack() {
    on_small_stack(|| {
        for (shapes, depth) in [(&EXPANDED[..], 400), (&CORE[..], 1_000)] {
            for shape in shapes {
                let case = format!("{} nested {depth} deep", shape.name);
                assert_runs(&nested(shape, depth), "1", &case);
            }
        }
    });
}

#[test]
fn code_nested_just_short_of_the_limit_compiles_runs_and_is_dropped() {
    // Every tree a stage builds is as deep as this, and dropping one recursed
    // a level at a time with nothing to grow the stack: an unoptimized build
    // overflowed 2 MiB here until each tree's `Drop` checked.
    on_small_stack(|| {
        let depth = patina_core::walk::MAX_FORM_DEPTH - 10;
        assert_runs(&nested(&IF, depth), "1", &format!("if nested {depth} deep"));
    });
}

#[test]
fn a_quasiquote_template_nested_deeply_builds_its_datum() {
    on_small_stack(|| {
        assert_runs(&measured("`", "a", 5_000), "(5000 a)", "a template");
        assert_runs(
            &measured("`", ",(+ 1 1)", 5_000),
            "(5000 2)",
            "a template with an unquote",
        );
    });
}

#[test]
fn quoted_data_is_not_held_to_the_depth_of_code() {
    on_small_stack(|| {
        assert_runs(&measured("'", "a", 100_000), "(100000 a)", "a quoted datum");
    });
}

#[test]
fn a_cond_of_many_clauses_runs_on_a_small_stack() {
    // The clauses nest only once `cond` has expanded into `if`s, one inside
    // the next's alternative.
    on_small_stack(|| {
        let clauses: String = (1..=500).map(|i| format!("((= x {i}) {i}) ")).collect();
        let program = format!("(import (scheme base))\n(define x 500)\n(cond {clauses}(else 0))");
        assert_runs(&program, "500", "a cond of 500 clauses");
    });
}

/// `(guard … (eval <form> …))`: what a program sees of `form`, a `let` nested
/// `depth` deep that it builds at run time.
fn evaluated(depth: usize) -> String {
    format!(
        "(import (scheme base) (scheme eval))\n\
         (define (nest n)\n\
           (if (= n 0) 'a (list 'let '((a 1)) (nest (- n 1)))))\n\
         (guard (e ((error-object? e) (list 'refused (error-object-message e))))\n\
           (eval (nest {depth}) (environment '(scheme base))))"
    )
}

#[test]
fn code_nested_past_the_limit_is_refused_with_an_error_a_program_catches() {
    on_small_stack(|| {
        for depth in [10_000, 100_000] {
            for (backend, outcome) in on_both_backends(&evaluated(depth)) {
                let value = outcome.unwrap_or_else(|error| {
                    panic!("{depth} on the {backend}: the refusal was not caught: {error}")
                });
                assert!(
                    value.starts_with("(refused ") && value.contains("nested too deeply"),
                    "{depth} on the {backend}: {value}"
                );
            }
        }
    });
}

#[test]
fn a_program_nested_past_the_limit_is_refused_before_it_runs() {
    // The macro shapes are refused when what they substitute nests past the
    // limit, at the first expansion; the core shapes, by the desugarer when
    // it gets there. Not the applied `lambda`: the desugarer walks a binding
    // form's whole body at each level, so getting there takes it seconds, and
    // the cost grows with the square of the depth.
    on_small_stack(|| {
        for shape in [&LET, &COND, &AND, &THUNK, &BEGIN, &PLUS, &IF] {
            for depth in [10_000, 100_000] {
                for (backend, outcome) in on_both_backends(&nested(shape, depth)) {
                    let error = outcome.expect_err(&format!(
                        "{} nested {depth} deep on the {backend} was not refused",
                        shape.name
                    ));
                    assert!(
                        error.contains("nested too deeply"),
                        "{} nested {depth} deep on the {backend}: {error}",
                        shape.name
                    );
                }
            }
        }
        // A quasiquote template is held to the limit on its own, where no
        // macro use holds it.
        let program = format!(
            "(import (scheme base))\n(define x `{}a{})",
            "(".repeat(100_000),
            ")".repeat(100_000)
        );
        for (backend, outcome) in on_both_backends(&program) {
            let error = outcome.expect_err(&format!("a template on the {backend}"));
            assert!(error.contains("nested too deeply"), "{backend}: {error}");
        }
    });
}
