//! Garbage collection through real evaluation — **tree-walker** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against `eval_program_tree_walker`, with `gc_vm.rs` doing the
//! same for the VM. That split is deliberate: several cases assert on
//! `(gc-stats)` counters, which legitimately differ between the backends, so
//! the both-backends helpers cannot serve them. Everything in this file is
//! therefore tree-walker-only — use `eval_program_tree_walker`, not
//! `assert_program_eval_to`, or the timing guard below silently measures the
//! VM too. See `docs/GC_DESIGN.md` §5.1.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(eval_program_tree_walker, eval_program_tree_walker_gc_off);

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
    assert_eq!(eval_program_tree_walker(code), "3");
}

#[test]
fn collection_inside_higher_order_primitive() {
    // A Rust primitive that calls a procedure back runs it on a nested
    // trampoline (`ApplyContext::apply_proc`) and keeps its own state in Rust
    // locals meanwhile: here `(patina internal lists)`'s `member` walks, with
    // the program's comparator, a fresh list that no root provider sees once
    // the call's step has been taken apart. The safe point inside the
    // comparator must defer rather than collect, or the walk's next step
    // reads a freed pair, which panics in a check build.
    //
    // This was `map` until `map` became Scheme (`higher_order.scm`, #471): a
    // Scheme procedure's calls are steps of the trampoline it runs on, so the
    // test had stopped nesting one. It calls the internal library's `member`
    // directly (#624): `(scheme base)`'s `member` is Scheme for the same
    // reason, and hands a comparator to its own `%member-by`, never to this
    // primitive, whose comparator path a program reaches only through
    // `(patina internal lists)`. That path and `%parameterize-swap!` (the
    // `parameterize` test in `gc_shared_tests!`) are what the deferral tests
    // nest through; if the comparator path goes, repoint this test at
    // another primitive that calls back through `apply_proc`.
    let code = r#"
        (import (patina debug) (only (patina internal lists) member))
        (member 3 (list 1 2 3 4) (lambda (x y) (gc) (= x y)))
    "#;
    assert_eq!(eval_program_tree_walker(code), "(3 4)");
}

#[test]
fn collection_inside_higher_order_primitive_on_the_detached_context() {
    // The same walk, called from Rust through the evaluator's own
    // `ApplyContext` rather than from a program (#622). No loop runs above
    // it, so nothing deferred the comparator's trampoline: it was outermost,
    // collected the list that only `member`'s Rust frame held, and the walk's
    // next step read a freed pair. The detached context now defers its runs
    // as a holder (`GcDeferGuard::holding`), as a machine's nested loop does.
    use patina_core::TaggedValue;
    use patina_primitives::ApplyContext;
    let interp = tree_walker_interpreter();
    interp
        .eval_program(
            "(import (patina debug) (only (patina internal lists) member))
             (define compare (lambda (x y) (gc) (= x y)))",
        )
        .expect("setup");
    let member = interp.eval_str("member").expect("member");
    let compare = interp.eval_str("compare").expect("compare");
    // Made in Rust, which cannot collect, so it is reachable from nothing
    // but the call's arguments: not a global, and not the value of the last
    // evaluation either.
    let list = interp
        .global_env()
        .heap()
        .borrow_mut()
        .list_from_iter((1..=4).map(TaggedValue::fixnum).collect::<Vec<_>>());
    let found = interp
        .evaluator()
        .apply_proc(member, vec![TaggedValue::fixnum(3), list, compare])
        .expect("member");
    assert_eq!(interp.display_tagged(found), "(3 4)");
}

#[test]
fn collection_at_deep_call_depth_preserves_suspended_values() {
    // Larceny family 6: Local -> ContEnv -> Local was still traced
    // recursively despite the evaluator's trampoline. A collection while
    // thousands of non-tail calls were suspended overflowed the Rust stack.
    // Each frame retains a different heap pair used only after its recursive
    // call returns, so skipping the deep roots cannot make this test pass.
    let code = r#"
        (import (patina debug))
        (define before (cdr (assq 'collections (gc-stats))))
        (define (nest n)
          (if (= n 0)
              (begin (gc) 0)
              (let ((saved (cons n '())))
                (let ((result (nest (- n 1))))
                  (+ result (car saved))))))
        (define result (nest 50000))
        (list result (> (cdr (assq 'collections (gc-stats))) before))
    "#;
    assert_eq!(eval_program_tree_walker(code), "(1250025000 #t)");
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
    assert_eq!(eval_program_tree_walker(code), "30");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "collection at continuation depth 30 took {elapsed:?} — tracing is likely exponential again"
    );
}
