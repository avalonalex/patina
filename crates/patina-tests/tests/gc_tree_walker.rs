//! Garbage collection through real evaluation — **tree-walker** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against the feature-switched helper (tree-walker by default).
//! This file keeps only the tests that target tree-walker machinery.
//! See `docs/GC_DESIGN.md` §5.1.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(eval_program_tree_walker);

#[test]
fn closure_environment_survives_collection() {
    // The counter's captured environment is reachable only through the
    // closure on the heap — if the closure's env edge were untraced, the
    // captured binding would be swept.
    let code = r#"
        (import (patina debug))
        (define (make-counter)
          (let ((n 0))
            (lambda () (set! n (+ n 1)) n)))
        (define c (make-counter))
        (c)
        (gc)
        (c)
        (c)
    "#;
    assert_program_eval_to(code, "3");
}

#[test]
fn collection_inside_higher_order_primitive() {
    // `map` re-enters the evaluator through a nested trampoline; the safe
    // point there must defer rather than collect with the outer state
    // unrooted.
    let code = r#"
        (import (patina debug))
        (map (lambda (x) (gc) (* x x)) '(1 2 3 4))
    "#;
    assert_program_eval_to(code, "(1 4 9 16)");
}

#[test]
fn deeply_nested_continuations_collect_promptly() {
    // Regression guard: continuation environments are a persistent Rc list
    // whose nodes each capture the chain below them, so tracing without
    // dedup is exponential — this shape measured 6.8 s for a single
    // collection at depth 26 before `GcVisitor::visit_once` was introduced.
    // Nothing here should take a perceptible amount of time.
    let code = r#"
        (import (patina debug))
        (define (nest n)
          (if (= n 0)
              (begin (gc) 0)
              (+ 1 (nest (- n 1)))))
        (nest 30)
    "#;
    let start = std::time::Instant::now();
    assert_program_eval_to(code, "30");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "collection at continuation depth 30 took {elapsed:?} — tracing is likely exponential again"
    );
}
