//! Garbage collection through real evaluation — **VM** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against `eval_program_vm`, with `gc_tree_walker.rs` doing the
//! same for the tree-walker. The shared cases take a single-backend evaluator
//! because several assert on `(gc-stats)` counters, which legitimately differ
//! between the backends.
//!
//! This file keeps only the tests that target VM machinery a heap scan cannot
//! reach: the continuation side tables, `CallFrame::closure` (a bare
//! `ObjectIndex`), boxed upvalues, and the multi-value `value_buffer`. See
//! `docs/GC_DESIGN.md` §5.2.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(eval_program_vm);

#[test]
fn closure_free_vars_survive_collection() {
    // `CallFrame::closure` is a bare ObjectIndex, not a TaggedValue — if it
    // were left untraced, the running closure itself would be swept.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-counter)
          (let ((n 0))
            (lambda () (gc) (set! n (+ n 1)) n)))
        (define c (make-counter))
        (c)
        (c)
        (c)
        "#,
        "3",
    );
}

#[test]
fn mutable_boxed_upvalue_survives_collection() {
    // `set!` on a captured variable boxes it as a MutableCell reached only
    // through the closure's free_vars.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (make-acc)
          (let ((total '()))
            (lambda (x) (set! total (cons x total)) (gc) total)))
        (define a (make-acc))
        (a 1) (a 2)
        (a 3)
        "#,
        "(3 2 1)",
    );
}

#[test]
fn continuation_in_heap_data_survives_collections_and_invokes() {
    // The side tables are weak (design §9.5): a continuation must stay
    // invocable as long as its ref object is reachable — here only through
    // a heap vector, across two collections, re-entering top level twice.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define saved (vector #f))
        (define count 0)
        (define result
          (+ 1 (call/cc (lambda (k) (vector-set! saved 0 k) 1))))
        (set! count (+ count 1))
        (gc)
        (if (< count 3) ((vector-ref saved 0) 41))
        result
        "#,
        "42",
    );
}

#[test]
fn dead_captures_are_pruned_from_side_table() {
    // The §9.5 blowup: dropped `call/cc` captures used to pin their ref
    // objects and snapshots forever through the side table. 20 000 dead
    // captures must leave no more than noise behind after a collection —
    // with strong tables the delta is the full 20 000.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (define (live-objects)
          (let ((s (gc-stats)))
            (- (cdr (assq 'objects s)) (cdr (assq 'free-objects s)))))
        (define (churn n)
          (if (> n 0) (begin (call/cc (lambda (k) 1)) (churn (- n 1)))))
        (gc)
        (define before (live-objects))
        (churn 20000)
        (gc)
        (< (- (live-objects) before) 200)
        "#,
        "#t",
    );
}

#[test]
fn multiple_values_survive_collection() {
    // `value_buffer` is the multi-value side channel — a root that is easy
    // to miss because it is empty most of the time.
    assert_gc_eval_to(
        r#"
        (import (patina debug))
        (call-with-values
          (lambda () (gc) (values 1 2 3))
          (lambda (a b c) (+ a b c)))
        "#,
        "6",
    );
}

// This pins VM register retirement, not a guarantee about exactly when the
// tree-walker releases discarded CPS results.
#[test]
fn discarded_results_and_both_branch_tests_release_the_key() {
    for condition in ["#t", "#f"] {
        assert_gc_eval_to(
            &format!(
                "(import (scheme base) (scheme ephemeron) (patina debug))
                 (define key (list 'key))
                 (define e (make-ephemeron key 'datum))
                 (define (drop!)
                   key
                   (if (begin key {condition}) key key)
                   (reference-barrier key)
                   (set! key #f)
                   (gc)
                   (ephemeron-broken? e))
                 (drop!)"
            ),
            "#t",
        );
    }
}

#[test]
fn replacing_a_captured_local_releases_its_previous_value() {
    assert_gc_eval_to(
        "(import (scheme base) (scheme ephemeron) (patina debug))
         (define (drop!)
           (define keys (list (list 'dead) (list 'live)))
           (define current (lambda () keys))
           (define e (make-ephemeron (car keys) 'datum))
           (set! keys (reverse (reverse (cdr keys))))
           (gc)
           (list (ephemeron-broken? e) (current)))
         (drop!)",
        "(#t ((live)))",
    );
}

#[test]
fn wide_pending_operands_survive_collection() {
    let operands = (0..80).map(|n| format!("(list {n}) ")).collect::<String>();
    assert_gc_eval_to(
        &format!(
            "(import (patina debug))
             (map car (list {operands} (begin (gc) (list 80))))"
        ),
        &format!(
            "({})",
            (0..81).map(|n| n.to_string()).collect::<Vec<_>>().join(" ")
        ),
    );
}

#[test]
fn rebound_control_operator_keeps_operands_after_a_larger_expression() {
    assert_gc_eval_to(
        "(import (patina debug))
         (define (run)
           (list (list 1 2 3 4 5) (list 6 7 8 9 10))
           (call-with-values (lambda () (list 'alive)) (lambda (x) x)))
         (set! call-with-values (lambda (producer consumer)
                                  (gc) (consumer (producer))))
         (run)",
        "(alive)",
    );
}
