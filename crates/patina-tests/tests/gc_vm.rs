//! Garbage collection through real evaluation — **VM** backend.
//!
//! The backend-independent cases live in `common::gc_shared_tests!` and are
//! invoked here against `eval_program_vm`, with `gc_tree_walker.rs` doing the
//! same for the tree-walker. The shared cases take a single-backend evaluator
//! because several assert on `(gc-stats)` counters, which legitimately differ
//! between the backends.
//!
//! This file keeps only the tests that target VM machinery a heap scan cannot
//! reach: the continuation side tables, `CallFrame::closure`, boxed upvalues,
//! and the multi-value `value_buffer`. See `docs/GC_DESIGN.md` §5.2.

#[macro_use]
mod common;
use common::*;

gc_shared_tests!(
    eval_program_vm,
    eval_program_vm_gc_off,
    eval_program_vm_zeal
);

#[test]
fn closure_free_vars_survive_collection() {
    // A collection inside the closure's body, which then reads and writes
    // its free variable through its frame's `closure`.
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

#[test]
fn code_units_do_not_grow_across_forms_a_collection_lets_go() {
    // `code-units` (#836) counts the units the VM has loaded and not let go.
    // Fifty `eval`s of fresh forms that make no closure each let their unit
    // go as the form returns, so the count reads as it did before them.
    // Fifty that make a closure, which dies with the form, are each kept by
    // the closure's count until a collection frees it, and two `(gc)`s let
    // them all go: the count after them reads as the count before. A unit
    // nothing can run that a collection did not let go would leave the
    // count above where it started. Collecting only where `(gc)` asks, the
    // second batch is counted until then, fifty more; at every outermost
    // safe point each goes at the one after it, and the call that reads the
    // count is one; in the environment's mode, anywhere between.
    //
    // The last reading follows fifty more of them and a single `(gc)`,
    // which frees their closures and lets their units go before it returns,
    // so the count reads as it did before them in every mode: a write of
    // the count before the collection's release, which a second `(gc)`
    // would correct, leaves it fifty above where collection waits for
    // `(gc)`. One collection is enough here because no form's code quotes a
    // closure: a closure only a released unit's constants reach is marked
    // by the collection that releases that unit, and goes at the next.
    //
    // Each reading is taken in the form that did the work before it, with
    // no form loaded between: a load writes the count afresh, and would
    // hide a release that did not. Every reading form counts its own unit,
    // the same one each time. Both batches run once first, so that whatever
    // their first run keeps for good is kept before the first reading. The
    // count is positive, if only for the units the program's own procedures
    // keep; its value is held to the VM's table of units by
    // `the_heaps_count_of_code_units_is_the_tables_size` in `vm_state.rs`,
    // since these readings, compared with each other, would pass a count
    // off by a constant.
    let code = r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (units) (cdr (assq 'code-units (gc-stats))))
        (define (settled) (gc) (gc) (units))
        (define env (environment '(scheme base)))
        (define (plain i) (list '+ i 1))
        (define (closing i) `(car (map (lambda (x) (+ x ,i)) (list 1 2 3))))
        (define (run-forms make-form)
          (let loop ((i 0))
            (if (< i 50) (begin (eval (make-form i) env) (loop (+ i 1))))))
        (define u0 #f) (define u1 #f) (define u2 #f) (define u3 #f) (define u4 #f)
        (run-forms plain)
        (run-forms closing)
        (set! u0 (settled))
        (set! u1 (begin (run-forms plain) (units)))
        (set! u2 (begin (run-forms closing) (units)))
        (set! u3 (settled))
        (set! u4 (begin (run-forms closing) (gc) (units)))
        (list (> u0 0) (- u1 u0) (- u2 u0) (- u3 u0) (- u4 u0))
        "#;
    assert_eq!(eval_program_vm_gc_off(code), "(#t 0 50 0 0)");
    assert_eq!(eval_program_vm_zeal(code), "(#t 0 0 0 0)");
    let plain = eval_program_vm(code);
    let readings: Vec<&str> = plain
        .trim_matches(|c| c == '(' || c == ')')
        .split_whitespace()
        .collect();
    let [positive, after_plain, closing, after_gc, after_one_gc] = readings[..] else {
        panic!("expected five readings: {plain}")
    };
    assert_eq!(
        (positive, after_plain, after_gc, after_one_gc),
        ("#t", "0", "0", "0"),
        "readings {plain}"
    );
    assert!(
        closing.parse::<i64>().is_ok_and(|n| (0..=50).contains(&n)),
        "the forms that made closures read {closing} units above the start: {plain}"
    );
}

#[test]
fn code_units_count_a_unit_a_live_closure_keeps() {
    // A procedure `eval` compiled keeps its unit for as long as it lives:
    // `keep` holds it, and the count is one above the reading before it, as
    // it is after two `(gc)`s, which leave the unit loaded because the
    // closure is live. Once `keep` lets the closure go, one `(gc)` frees it
    // and lets the unit go before it returns, and the count reads as it did
    // before. Each reading is taken in the form that did the work before
    // it, as in `code_units_do_not_grow_across_forms_a_collection_lets_go`.
    // `eval` is warmed up first, its closure dropped and collected before
    // the first reading. Under zeal too, with a collection at every
    // outermost safe point. Each `(gc)` is written where it runs, once a
    // call, so that the zeal half's check that zeal collected beyond the
    // program's `(gc)`s counts every one of them.
    assert_gc_and_zeal_eval_to(
        r#"
        (import (scheme base) (scheme eval) (patina debug))
        (define (units) (cdr (assq 'code-units (gc-stats))))
        (define env (environment '(scheme base)))
        (define keep (eval '(lambda (x) (* x 3)) env))
        (define answer #f)
        (define u0 #f) (define u1 #f) (define u2 #f) (define u3 #f)
        (set! keep #f)
        (set! u0 (begin (gc) (gc) (units)))
        (set! u1 (begin (set! keep (eval '(lambda (x) (* x 3)) env)) (units)))
        (set! u2 (begin (gc) (gc) (units)))
        (set! answer (keep 14))
        (set! u3 (begin (set! keep #f) (gc) (units)))
        (list answer (> u0 0) (- u1 u0) (- u2 u0) (- u3 u0))
        "#,
        "(42 #t 1 1 0)",
    );
}

/// The control of `release_dead_code`'s check (#836): a check build asserts,
/// after every collection, that each unit the collection kept had its link
/// table traced, since whatever keeps a unit is something that can run its
/// code and reports the unit's code id. Here patina-vm's test-only switch
/// (`test_support::SkipCodeIds`) has the collection trace no table, as a
/// holder that reports nothing would leave it, and `g`'s closure keeps its
/// unit (as the closures the bootstrap's libraries made keep theirs): the
/// first collection with the switch on panics with the check's message,
/// wherever it lands. Declared after the interpreter, the switch is off
/// again before the interpreter drops in the unwind.
///
/// Run in every check build: every debug `cargo test`, and a release one
/// with this crate's `gc-check` feature, which CI's release GC lane runs
/// with `--include-ignored`. A build without the check reports it ignored.
/// No other test in this file may start with its name's prefix, which CI's
/// line filters on.
#[test]
#[cfg_attr(
    not(any(debug_assertions, feature = "gc-check")),
    ignore = "needs a check build"
)]
#[should_panic(expected = "did not trace its link table")]
fn a_check_build_refuses_a_kept_unit_whose_table_was_not_traced() {
    let interp = vm_interpreter();
    interp
        .eval_program_owned("(import (scheme base) (patina debug))")
        .expect("import");
    let _skip = patina_vm::test_support::SkipCodeIds::new();
    let _ = interp.eval_program_owned("(define (g) 1) (gc)");
}
