//! Garbage collection through real evaluation (tree-walker backend).
//!
//! `(gc)` records a request that the next trampoline safe point services, so
//! these tests exercise the whole stage 2 path: root providers, safe point,
//! defer guards, mark, and sweep. See `docs/GC_DESIGN.md`.
//!
//! Gated to the tree-walker: the VM has no safe point until stage 3, so under
//! `--features vm-backend` these would exercise an interpreter that never
//! collects. Remove the gate when stage 3 lands.
#![cfg(not(feature = "vm-backend"))]

mod common;
use common::*;

/// Pull one `(gc-stats)` field out of the alist the primitive returns.
fn stat(code_before: &str, field: &str) -> i64 {
    let code = format!(
        r#"(import (patina debug))
           {code_before}
           (cdr (assq '{field} (gc-stats)))"#
    );
    eval_program(&code)
        .parse()
        .unwrap_or_else(|_| panic!("expected a number for {field}"))
}

#[test]
fn gc_runs_and_reclaims_unreachable_pairs() {
    // Allocate a large amount of garbage, drop the only reference, collect.
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
fn live_data_survives_collection() {
    // The list is still bound when the collection runs; every element must
    // survive and stay readable.
    let code = r#"
        (import (patina debug))
        (define keep (list 1 2 3 4 5))
        (gc)
        (apply + keep)
    "#;
    assert_program_eval_to(code, "15");
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
    assert_program_eval_to(code, "2000");
}

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
fn unreachable_cycles_are_reclaimed() {
    // Build a thousand self-referential pairs, each garbage the moment the
    // next iteration starts. Reference counting alone could never reclaim
    // any of them, so a sweep that frees at least that many slots is only
    // possible if cycles are being collected.
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
    assert_program_eval_to(code, "#t");
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
    assert_program_eval_to(code, "\"hello world3\"");
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
    assert_program_eval_to(code, "(before during after)");
}

#[test]
fn captured_continuation_survives_and_escapes() {
    // The continuation is reified into a heap object, survives a collection,
    // and is then invoked to escape — exercising both the `Continuation`
    // trace rule and the escape path's `PENDING_ESCAPE` root.
    let code = r#"
        (import (patina debug))
        (define (find-first pred lst)
          (call/cc
            (lambda (return)
              (for-each (lambda (x) (gc) (if (pred x) (return x))) lst)
              #f)))
        (find-first even? '(1 3 5 6 7))
    "#;
    assert_program_eval_to(code, "6");
}

#[test]
fn continuation_invoked_after_collection() {
    let code = r#"
        (import (patina debug))
        (let ((k (call/cc (lambda (c) c))))
          (gc)
          (if (procedure? k) (k 42) k))
    "#;
    assert_program_eval_to(code, "42");
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
fn records_and_exceptions_survive_collection() {
    // The record's fields live behind an `Rc<RefCell<Vec<TaggedValue>>>`, and
    // the guard clause closes over `p` — both must survive the collection.
    let code = r#"
        (import (patina debug))
        (define-record-type point (make-point x y) point? (x point-x) (y point-y))
        (define p (make-point 3 4))
        (gc)
        (guard (e (#t (+ (point-x p) (point-y p))))
          (raise 'boom))
    "#;
    assert_program_eval_to(code, "7");
}

#[test]
fn collecting_keeps_the_arena_smaller_than_not_collecting() {
    // Allocate and drop repeatedly. The assertion compares against the same
    // workload with the collections removed, so it can only pass if slots
    // are actually being reclaimed and reused — a fixed bound would pass
    // vacuously with zero collections.
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

#[test]
fn collection_is_recorded_in_stats() {
    let collections = stat("(gc)", "collections");
    assert!(
        collections > 0,
        "no collection was recorded after (gc): {collections}"
    );
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
