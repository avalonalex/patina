//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.
//!
//! When the `vm-backend` feature is enabled, all helpers use VmBackend
//! instead of the tree-walker. This allows running the full test suite
//! against the VM with:
//!
//!     cargo test --package patina-tests --features vm-backend

#![allow(dead_code)]
// `gc_shared_tests!` is used only by the GC test binaries; every other test
// file that includes this module compiles it unused.
#![allow(unused_macros)]

use patina_core::tagged_value::TaggedValue;
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use std::cell::RefCell;

// ─── Backend selection ───────────────────────────────────────────────────────

#[cfg(not(feature = "vm-backend"))]
use patina_interpreter::TreeWalkInterpreter;

#[cfg(feature = "vm-backend")]
use patina_interpreter::Interpreter;
#[cfg(feature = "vm-backend")]
use patina_runtime::Backend;
#[cfg(feature = "vm-backend")]
use patina_vm::VmBackend;

/// Format a TaggedValue for display (backend-agnostic).
fn display_tagged(tv: TaggedValue, heap: &RefCell<patina_core::heap::Heap>) -> String {
    // Unpack multiple values (R7RS: each value displayed on its own line)
    let vals = heap.borrow().get_values(tv).map(|v| v.to_vec());
    if let Some(vals) = vals {
        return vals
            .iter()
            .map(|v| format_write_tagged(*v, heap))
            .collect::<Vec<_>>()
            .join("\n");
    }
    format_write_tagged(tv, heap)
}

// ─── Tree-walker helpers (default) ───────────────────────────────────────────

#[cfg(not(feature = "vm-backend"))]
fn make_interp() -> TreeWalkInterpreter {
    TreeWalkInterpreter::new_tree_walker()
}

#[cfg(not(feature = "vm-backend"))]
fn interp_display(interp: &TreeWalkInterpreter, tv: TaggedValue) -> String {
    interp.display_tagged(tv)
}

// ─── VM backend helpers ──────────────────────────────────────────────────────

#[cfg(feature = "vm-backend")]
fn make_interp() -> Interpreter<VmBackend> {
    Interpreter::new(VmBackend::new())
}

#[cfg(feature = "vm-backend")]
fn interp_display(interp: &Interpreter<VmBackend>, tv: TaggedValue) -> String {
    let heap = interp.backend().global_env().heap();
    display_tagged(tv, heap)
}

// ─── Public helpers (backend-agnostic) ───────────────────────────────────────

/// Assert that evaluating a Scheme expression produces the expected result
pub fn assert_eval_to(expr: &str, expected: &str) {
    let interp = make_interp();
    let result = interp
        .eval_str(expr)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    let result_str = interp_display(&interp, result);

    assert_eq!(
        result_str, expected,
        "\nExpression: {}\nExpected: {}\nGot: {}",
        expr, expected, result_str
    );
}

/// Assert that evaluating a Scheme expression produces an error
pub fn assert_eval_error(expr: &str) {
    let interp = make_interp();
    let result = interp.eval_str(expr);

    assert!(
        result.is_err(),
        "Expected error for '{}', but got: {:?}",
        expr,
        result.unwrap()
    );
}

/// Evaluate multiple expressions in sequence, return last result
pub fn eval_program(code: &str) -> String {
    let interp = make_interp();
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    interp_display(&interp, result)
}

/// Evaluate a program on the VM backend *explicitly* — not the
/// feature-switched default — and `write` the result. For tests that
/// exercise VM-only machinery (CallPrimitive deopt, inline opcodes).
pub fn eval_program_vm(code: &str) -> String {
    use patina_interpreter::Backend as _;
    let interp = patina_interpreter::Interpreter::new(patina_vm::VmBackend::new());
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {e}\n{code}"));
    let heap = interp.backend().global_env().heap();
    format_write_tagged(result, heap)
}

/// Assert that a multi-expression program produces expected result
pub fn assert_program_eval_to(code: &str, expected: &str) {
    let result = eval_program(code);
    assert_eq!(
        result, expected,
        "\nProgram:\n{}\nExpected: {}\nGot: {}",
        code, expected, result
    );
}

/// Assert that evaluating a multi-expression program produces an error
pub fn assert_program_eval_error(code: &str) {
    let interp = make_interp();
    let result = interp.eval_program(code);

    assert!(
        result.is_err(),
        "Expected error for program, but got: {:?}",
        result.unwrap()
    );
}

/// Assert that evaluating an expression with (scheme char) imported produces expected result
pub fn assert_eval_with_scheme_char(expr: &str, expected: &str) {
    let code = format!("(import (scheme char)) {}", expr);
    let interp = make_interp();
    let result = interp
        .eval_program(&code)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    let result_str = interp_display(&interp, result);

    assert_eq!(
        result_str, expected,
        "\nExpression: {}\nExpected: {}\nGot: {}",
        expr, expected, result_str
    );
}

/// Assert that a multi-expression program produces expected result
/// This function now always uses CPS evaluation (the default mode).
/// The `use_cps` parameter is kept for backward compatibility but is ignored.
#[allow(unused_variables)]
pub fn assert_program_eval_to_with_cps(code: &str, expected: &str, use_cps: bool) {
    let interp = make_interp();
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    let result_str = interp_display(&interp, result);
    assert_eq!(
        result_str, expected,
        "\nProgram:\n{}\nExpected: {}\nGot: {}",
        code, expected, result_str
    );
}

// ─── Shared GC test suite ────────────────────────────────────────────────────

/// Backend-independent garbage-collection tests.
///
/// `(gc)` records a request that the next safe point services, so these
/// exercise the whole path — root providers, safe point, defer guards, mark,
/// and sweep — for whichever backend `$eval` drives. Invoked once per backend
/// so both are covered in a single test lane; each backend's own file keeps
/// only the tests that target machinery unique to it.
///
/// `$eval` is a `fn(&str) -> String` that evaluates a program and `write`s the
/// result (`eval_program` or `eval_program_vm`).
macro_rules! gc_shared_tests {
    ($eval:path) => {
        /// Pull one `(gc-stats)` field out of the alist the primitive returns.
        fn stat(code_before: &str, field: &str) -> i64 {
            let code = format!(
                r#"(import (patina debug))
                   {code_before}
                   (cdr (assq '{field} (gc-stats)))"#
            );
            $eval(&code)
                .parse()
                .unwrap_or_else(|_| panic!("expected a number for {field}"))
        }

        fn assert_gc_eval_to(code: &str, expected: &str) {
            let result = $eval(code);
            assert_eq!(
                result, expected,
                "\nProgram:\n{code}\nExpected: {expected}\nGot: {result}"
            );
        }

        #[test]
        fn gc_runs_and_reclaims_unreachable_pairs() {
            // Allocate a large amount of garbage, drop the only reference,
            // collect.
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
            // The list is still bound when the collection runs; every element
            // must survive and stay readable.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define keep (list 1 2 3 4 5))
                (gc)
                (apply + keep)
                "#,
                "15",
            );
        }

        #[test]
        fn deep_live_structure_survives_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (build n) (if (= n 0) '() (cons n (build (- n 1)))))
                (define keep (build 2000))
                (gc)
                (length keep)
                "#,
                "2000",
            );
        }

        #[test]
        fn unreachable_cycles_are_reclaimed() {
            // A thousand self-referential pairs, each garbage the moment the
            // next iteration starts. Reference counting could never reclaim
            // any of them, so a sweep freeing that many slots is only possible
            // if cycles are collected.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (make-cycles n)
                  (if (> n 0)
                      (let ((x (cons n '())))
                        (set-cdr! x x)
                        (make-cycles (- n 1)))))
                (make-cycles 1000)
                (gc)
                (>= (cdr (assq 'last-swept (gc-stats))) 1000)
                "#,
                "#t",
            );
        }

        #[test]
        fn captured_continuation_survives_and_escapes() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (find-first pred lst)
                  (call/cc
                    (lambda (return)
                      (for-each (lambda (x) (gc) (if (pred x) (return x))) lst)
                      #f)))
                (find-first even? '(1 3 5 6 7))
                "#,
                "6",
            );
        }

        #[test]
        fn continuation_invoked_after_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (let ((k (call/cc (lambda (c) c))))
                  (gc)
                  (if (procedure? k) (k 42) k))
                "#,
                "42",
            );
        }

        #[test]
        fn collection_during_dynamic_wind_preserves_thunks() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define trace '())
                (dynamic-wind
                  (lambda () (set! trace (cons 'before trace)))
                  (lambda () (gc) (set! trace (cons 'during trace)))
                  (lambda () (set! trace (cons 'after trace))))
                (reverse trace)
                "#,
                "(before during after)",
            );
        }

        #[test]
        fn records_and_exceptions_survive_collection() {
            // The record's fields live behind an `Rc<RefCell<Vec<_>>>`, and
            // the guard clause closes over `p` — both must survive.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define-record-type point (make-point x y) point? (x point-x) (y point-y))
                (define p (make-point 3 4))
                (gc)
                (guard (e (#t (+ (point-x p) (point-y p))))
                  (raise 'boom))
                "#,
                "7",
            );
        }

        #[test]
        fn strings_and_vectors_survive_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define s (string-append "hello" " " "world"))
                (define v (vector 1 2 3))
                (gc)
                (string-append s (number->string (vector-ref v 2)))
                "#,
                "\"hello world3\"",
            );
        }

        #[test]
        fn collecting_keeps_the_arena_smaller_than_not_collecting() {
            // Compared against the same workload with the collections removed,
            // so it can only pass if slots are actually reclaimed and reused —
            // a fixed bound would pass vacuously with zero collections.
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
    };
}
