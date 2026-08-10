//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.
//!
//! # Every helper runs on both backends
//!
//! Patina's central architectural claim is that the tree-walker and the VM
//! implement the same language. These helpers make that a *checked* claim
//! rather than a stated one: each one evaluates the program on both backends
//! and holds both to the same expectation, so a divergence fails a test
//! instead of waiting to be found by hand. The failure message always names
//! which backend produced what, because the culprit is usually the backend
//! you were not thinking about.
//!
//! A known divergence is opted out explicitly with the `_on` variants:
//!
//!     assert_program_eval_to_on(On::Vm, code, expected);  // tree-walker bug: see PRD/bugs/...
//!
//! Those call sites are the inventory of known divergences — keep each one
//! commented with the reason and a pointer, and delete it when the bug is
//! fixed. `rg 'On::(Vm|TreeWalker)' crates/patina-tests` lists them all.

#![allow(dead_code)]
// `gc_shared_tests!` is used only by the GC test binaries; every other test
// file that includes this module compiles it unused.
#![allow(unused_macros)]

use patina_core::tagged_value::TaggedValue;
use patina_interpreter::{Interpreter, TreeWalkInterpreter};
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use patina_runtime::Backend;
use patina_vm::VmBackend;
use std::cell::RefCell;

// ─── Backend selection ───────────────────────────────────────────────────────

/// Which backends a helper exercises. `Both` is the default and the only
/// value that should appear in new tests; the single-backend variants exist
/// to quarantine a *known* divergence until it is fixed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    /// Both backends must agree. The default for every plain helper.
    Both,
    /// Tree-walker only — the VM diverges. Requires a comment saying why.
    TreeWalker,
    /// VM only — the tree-walker diverges. Requires a comment saying why.
    Vm,
}

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

/// What one backend did with a program: the `write`-formatted value, or the
/// error rendered as a string.
type Outcome = Result<String, String>;

/// Whether the code is a whole program (`eval_program`) or a single
/// expression (`eval_str`). The two entry points differ in how they treat
/// multiple top-level forms, so tests must keep using the one they chose.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Expr,
    Program,
}

/// Run `code` on one backend, capturing either its value or its error.
///
/// Generic over `B: Backend` — `global_env()` is on the trait and
/// `display_tagged` is the same writer for both backends, so one body serves
/// the tree-walker and the VM.
fn run_on<B: Backend>(interp: Interpreter<B>, code: &str, mode: Mode) -> Outcome {
    let result = match mode {
        Mode::Expr => interp.eval_str(code),
        Mode::Program => interp.eval_program(code),
    };
    match result {
        Ok(tv) => Ok(display_tagged(tv, interp.backend().global_env().heap())),
        Err(e) => Err(e.to_string()),
    }
}

/// Run `code` on each backend `on` selects, labelled for failure messages.
fn outcomes(on: On, code: &str, mode: Mode) -> Vec<(&'static str, Outcome)> {
    let mut out = Vec::with_capacity(2);
    if on != On::Vm {
        out.push((
            "tree-walker",
            run_on(TreeWalkInterpreter::new_tree_walker(), code, mode),
        ));
    }
    if on != On::TreeWalker {
        out.push(("vm", run_on(Interpreter::new(VmBackend::new()), code, mode)));
    }
    out
}

/// Assert every selected backend evaluates `code` to `expected`.
fn expect_value(on: On, code: &str, expected: &str, mode: Mode) {
    for (backend, outcome) in outcomes(on, code, mode) {
        match outcome {
            Ok(actual) => assert_eq!(
                actual, expected,
                "\n[{backend}] wrong result\nProgram:\n{code}\nExpected: {expected}\nGot: {actual}"
            ),
            Err(e) => panic!("\n[{backend}] failed to evaluate\nProgram:\n{code}\nError: {e}"),
        }
    }
}

/// Assert every selected backend rejects `code`.
///
/// One backend erroring where the other succeeds is itself a divergence — the
/// exact shape of the control-operator cluster in Track Q §1.2 — so this is a
/// stronger check than the single-backend version it replaces.
fn expect_error(on: On, code: &str, mode: Mode) {
    for (backend, outcome) in outcomes(on, code, mode) {
        if let Ok(value) = outcome {
            panic!("\n[{backend}] expected an error\nProgram:\n{code}\nGot: {value}");
        }
    }
}

// ─── Public helpers ──────────────────────────────────────────────────────────

/// Assert that evaluating a Scheme expression produces the expected result
/// on both backends.
pub fn assert_eval_to(expr: &str, expected: &str) {
    expect_value(On::Both, expr, expected, Mode::Expr);
}

/// [`assert_eval_to`] restricted to one backend — for a known divergence only.
pub fn assert_eval_to_on(on: On, expr: &str, expected: &str) {
    expect_value(on, expr, expected, Mode::Expr);
}

/// Assert that evaluating a Scheme expression produces an error on both backends
pub fn assert_eval_error(expr: &str) {
    expect_error(On::Both, expr, Mode::Expr);
}

/// [`assert_eval_error`] restricted to one backend — for a known divergence only.
pub fn assert_eval_error_on(on: On, expr: &str) {
    expect_error(on, expr, Mode::Expr);
}

/// Evaluate multiple expressions in sequence on both backends, assert the two
/// agree, and return the shared result.
///
/// The agreement check is what makes callers that compare the return value
/// against their own expectation differential for free.
pub fn eval_program(code: &str) -> String {
    let tw = run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program);
    let vm = run_on(Interpreter::new(VmBackend::new()), code, Mode::Program);

    match (tw, vm) {
        (Ok(a), Ok(b)) => {
            assert_eq!(
                a, b,
                "\nbackends disagree\nProgram:\n{code}\ntree-walker: {a}\nvm: {b}"
            );
            a
        }
        (Err(a), Err(_)) => panic!("\nFailed to evaluate program:\n{code}\nError: {a}"),
        (Ok(a), Err(b)) => {
            panic!("\nbackends disagree\nProgram:\n{code}\ntree-walker: {a}\nvm errored: {b}")
        }
        (Err(a), Ok(b)) => {
            panic!("\nbackends disagree\nProgram:\n{code}\ntree-walker errored: {a}\nvm: {b}")
        }
    }
}

/// Evaluate a program on the tree-walker only and `write` the result. For
/// tests that target tree-walker-specific machinery.
pub fn eval_program_tree_walker(code: &str) -> String {
    match run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {e}\n{code}"),
    }
}

/// Evaluate a program on the VM only and `write` the result. For tests that
/// exercise VM-only machinery (CallPrimitive deopt, inline opcodes).
pub fn eval_program_vm(code: &str) -> String {
    match run_on(Interpreter::new(VmBackend::new()), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {e}\n{code}"),
    }
}

/// Assert that a multi-expression program produces expected result on both backends
pub fn assert_program_eval_to(code: &str, expected: &str) {
    expect_value(On::Both, code, expected, Mode::Program);
}

/// [`assert_program_eval_to`] restricted to one backend — known divergence only.
pub fn assert_program_eval_to_on(on: On, code: &str, expected: &str) {
    expect_value(on, code, expected, Mode::Program);
}

/// Assert that evaluating a multi-expression program produces an error on both backends
pub fn assert_program_eval_error(code: &str) {
    expect_error(On::Both, code, Mode::Program);
}

/// [`assert_program_eval_error`] restricted to one backend — known divergence only.
pub fn assert_program_eval_error_on(on: On, code: &str) {
    expect_error(on, code, Mode::Program);
}

/// Assert that evaluating an expression with (scheme char) imported produces
/// expected result on both backends
pub fn assert_eval_with_scheme_char(expr: &str, expected: &str) {
    let code = format!("(import (scheme char)) {}", expr);
    expect_value(On::Both, &code, expected, Mode::Program);
}

/// Assert that a multi-expression program produces expected result
/// This function now always uses CPS evaluation (the default mode).
/// The `use_cps` parameter is kept for backward compatibility but is ignored.
#[allow(unused_variables)]
pub fn assert_program_eval_to_with_cps(code: &str, expected: &str, use_cps: bool) {
    expect_value(On::Both, code, expected, Mode::Program);
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

        #[test]
        fn liveness_stress_large_list_survives_collection() {
            // The stage-4b liveness stress (design §10): a 100k-element list
            // stays fully intact across a collection that runs while it is
            // the only thing keeping its cells alive.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc))))
                (define big (build 100000 '()))
                (gc)
                (define (sum lst acc) (if (null? lst) acc (sum (cdr lst) (+ acc (car lst)))))
                (sum big 0)
                "#,
                "5000050000",
            );
        }

        #[test]
        fn arena_plateaus_under_repeated_churn_with_gc() {
            // The stage-4b arena-reuse proof (design §10): across 20 churn
            // rounds with a collection each, the pairs arena must reuse freed
            // slots rather than grow — the delta after 100k churned conses
            // stays under one round's worth.
            let delta: i64 = $eval(
                r#"(import (patina debug))
                   (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
                   (define (rounds k)
                     (if (> k 0) (begin (churn 5000) (gc) (rounds (- k 1)))))
                   (churn 5000)
                   (gc)
                   (define baseline (cdr (assq 'pairs (gc-stats))))
                   (rounds 20)
                   (- (cdr (assq 'pairs (gc-stats))) baseline)"#,
            )
            .parse()
            .unwrap_or_else(|_| panic!("expected a numeric arena delta"));
            assert!(
                delta < 5000,
                "pairs arena grew by {delta} slots across 20 collected churn rounds"
            );
        }
    };
}
