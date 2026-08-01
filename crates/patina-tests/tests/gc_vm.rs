//! Garbage collection through real evaluation on the **VM** backend.
//!
//! Mirrors `gc_tree_walker.rs`, but drives `VmBackend` explicitly so the
//! coverage holds regardless of which backend the feature-switched helpers
//! select. The VM's root set has two members a heap scan cannot reach — the
//! continuation side tables and `CallFrame::closure` — so the continuation
//! and closure tests here are the ones that would catch a regression in
//! `impl GcRoots for VmState`. See `docs/GC_DESIGN.md` §5.2.

mod common;
use common::*;

fn assert_vm_eval_to(code: &str, expected: &str) {
    let result = eval_program_vm(code);
    assert_eq!(
        result, expected,
        "\nProgram:\n{code}\nExpected: {expected}\nGot: {result}"
    );
}

/// Pull one `(gc-stats)` field out of the alist the primitive returns.
fn stat(code_before: &str, field: &str) -> i64 {
    let code = format!(
        r#"(import (patina debug))
           {code_before}
           (cdr (assq '{field} (gc-stats)))"#
    );
    eval_program_vm(&code)
        .parse()
        .unwrap_or_else(|_| panic!("expected a number for {field}"))
}

#[test]
fn gc_runs_and_reclaims_unreachable_pairs() {
    let freed = stat(
        r#"(define (churn n acc) (if (= n 0) acc (churn (- n 1) (cons n '()))))
           (churn 5000 '())
           (gc)"#,
        "free-pairs",
    );
    assert!(
        freed > 0,
        "expected the collector to reclaim pairs, free-pairs = {freed}"
    );
}

#[test]
fn collection_is_recorded_in_stats() {
    let collections = stat("(gc)", "collections");
    assert!(
        collections > 0,
        "no collection was recorded after (gc): {collections}"
    );
}

#[test]
fn live_data_survives_collection() {
    let code = r#"
        (import (patina debug))
        (define keep (list 1 2 3 4 5))
        (gc)
        (apply + keep)
    "#;
    assert_vm_eval_to(code, "15");
}

#[test]
fn deep_live_structure_survives_collection() {
    let code = r#"
        (import (patina debug))
        (define (build n) (if (= n 0) '() (cons n (build (- n 1)))))
        (define keep (build 2000))
        (gc)
        (length keep)
    "#;
    assert_vm_eval_to(code, "2000");
}

#[test]
fn unreachable_cycles_are_reclaimed() {
    let code = r#"
        (import (patina debug))
        (define (make-cycles n)
          (if (> n 0)
              (let ((x (cons n '())))
                (set-cdr! x x)
                (make-cycles (- n 1)))))
        (make-cycles 1000)
        (gc)
        (>= (cdr (assq 'last-swept (gc-stats))) 1000)
    "#;
    assert_vm_eval_to(code, "#t");
}

#[test]
fn closure_free_vars_survive_collection() {
    // `CallFrame::closure` is a bare HeapIndex, not a TaggedValue — if it
    // were left untraced, the running closure itself would be swept.
    let code = r#"
        (import (patina debug))
        (define (make-counter)
          (let ((n 0))
            (lambda () (gc) (set! n (+ n 1)) n)))
        (define c (make-counter))
        (c)
        (c)
        (c)
    "#;
    assert_vm_eval_to(code, "3");
}

#[test]
fn mutable_boxed_upvalue_survives_collection() {
    // `set!` on a captured variable boxes it as a MutableCell reached only
    // through the closure's free_vars.
    let code = r#"
        (import (patina debug))
        (define (make-acc)
          (let ((total '()))
            (lambda (x) (set! total (cons x total)) (gc) total)))
        (define a (make-acc))
        (a 1) (a 2)
        (a 3)
    "#;
    assert_vm_eval_to(code, "(3 2 1)");
}

#[test]
fn captured_continuation_survives_collection() {
    // The captured registers and frames live in VmState's continuation side
    // table, reachable from the heap only via an opaque VmContinuationRef —
    // nothing but `impl GcRoots for VmState` reaches them.
    let code = r#"
        (import (patina debug))
        (define (find-first pred lst)
          (call/cc
            (lambda (return)
              (for-each (lambda (x) (gc) (if (pred x) (return x))) lst)
              #f)))
        (find-first even? '(1 3 5 6 7))
    "#;
    assert_vm_eval_to(code, "6");
}

#[test]
fn continuation_invoked_after_collection() {
    let code = r#"
        (import (patina debug))
        (let ((k (call/cc (lambda (c) c))))
          (gc)
          (if (procedure? k) (k 42) k))
    "#;
    assert_vm_eval_to(code, "42");
}

#[test]
fn collection_during_dynamic_wind_preserves_thunks() {
    let code = r#"
        (import (patina debug))
        (define trace '())
        (dynamic-wind
          (lambda () (set! trace (cons 'before trace)))
          (lambda () (gc) (set! trace (cons 'during trace)))
          (lambda () (set! trace (cons 'after trace))))
        (reverse trace)
    "#;
    assert_vm_eval_to(code, "(before during after)");
}

#[test]
fn records_and_exceptions_survive_collection() {
    let code = r#"
        (import (patina debug))
        (define-record-type point (make-point x y) point? (x point-x) (y point-y))
        (define p (make-point 3 4))
        (gc)
        (guard (e (#t (+ (point-x p) (point-y p))))
          (raise 'boom))
    "#;
    assert_vm_eval_to(code, "7");
}

#[test]
fn strings_and_vectors_survive_collection() {
    let code = r#"
        (import (patina debug))
        (define s (string-append "hello" " " "world"))
        (define v (vector 1 2 3))
        (gc)
        (string-append s (number->string (vector-ref v 2)))
    "#;
    assert_vm_eval_to(code, "\"hello world3\"");
}

#[test]
fn multiple_values_survive_collection() {
    // `value_buffer` is the multi-value side channel — a root that is easy
    // to miss because it is empty most of the time.
    let code = r#"
        (import (patina debug))
        (call-with-values
          (lambda () (gc) (values 1 2 3))
          (lambda (a b c) (+ a b c)))
    "#;
    assert_vm_eval_to(code, "6");
}

#[test]
fn collecting_keeps_the_arena_smaller_than_not_collecting() {
    let workload = |collect: &str| {
        format!(
            r#"(define (round n)
                 (if (> n 0)
                     (begin
                       (let loop ((i 0) (acc '()))
                         (if (< i 200) (loop (+ i 1) (cons i acc)) acc))
                       {collect}
                       (round (- n 1)))))
               (round 10)"#
        )
    };
    let with_gc = stat(&workload("(gc)"), "pairs");
    let without_gc = stat(&workload(""), "pairs");
    assert!(
        with_gc < without_gc,
        "collecting did not shrink the arena: {with_gc} with gc vs {without_gc} without"
    );
}
