//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.
//!
//! # Every helper here runs on both backends
//!
//! Patina's central architectural claim is that the tree-walker and the VM
//! implement the same language. These helpers make that a *checked* claim
//! rather than a stated one: each one evaluates the program on both backends
//! and holds both to the same expectation, so a divergence fails a test
//! instead of waiting to be found by hand. The failure message always names
//! which backend produced what, because the culprit is usually the backend
//! you were not thinking about.
//!
//! Note the scope: this covers the tests that *use these helpers*. Several
//! test files still construct an interpreter directly and remain
//! tree-walker-only — porting them is tracked as Q1 in
//! `PRD/TRACK_Q_QUALITY_PRD.md`. Prefer these helpers in new tests.
//!
//! A known divergence is declared with [`assert_divergence`], which pins the
//! working backend's answer *and* requires the other to still fail:
//!
//!     assert_divergence(code, On::Vm, "(1 2)", ErrorClass::AtRuntime, "PRD/bugs/SOME_BUG.md");
//!
//! Fixing the bug makes that test fail, which is the point — the quarantine
//! retires itself instead of outliving the defect. `rg assert_divergence
//! crates/patina-tests` is the complete inventory.

#![allow(dead_code)]
// `gc_shared_tests!` is used only by the GC test binaries; every other test
// file that includes this module compiles it unused.
#![allow(unused_macros)]

use patina_core::tagged_value::TaggedValue;
use patina_interpreter::{Interpreter, InterpreterError, TreeWalkInterpreter};
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use patina_runtime::Backend;
use patina_vm::VmBackend;
use std::cell::RefCell;

// ─── Backend selection ───────────────────────────────────────────────────────

/// The backend a quarantined divergence currently *works* on. The other one is
/// asserted to still fail, so fixing it retires the quarantine automatically.
///
/// This is deliberately two-valued: "both backends" is not expressible, because
/// a quarantine that quarantines nothing is just [`assert_program_eval_to`].
#[derive(Clone, Copy)]
pub enum On {
    TreeWalker,
    Vm,
}

/// Which backends one run covers. Private — tests never select backends
/// directly, they either use a plain both-backends helper or declare a
/// divergence with [`assert_divergence`].
#[derive(Clone, Copy)]
enum Which {
    Both,
    TreeWalker,
    Vm,
}

impl On {
    /// The backend that behaves correctly today.
    fn working(self) -> Which {
        match self {
            On::TreeWalker => Which::TreeWalker,
            On::Vm => Which::Vm,
        }
    }

    /// The backend that is still broken.
    fn broken(self) -> Which {
        match self {
            On::TreeWalker => Which::Vm,
            On::Vm => Which::TreeWalker,
        }
    }
}

/// The workspace root, for tests that read repo files (bundled libraries,
/// upstream suites). Encodes the crates/patina-tests → root distance once.
pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Every file under `root`, recursively, in unspecified order. Callers filter
/// by extension. Panics on an unreadable directory — in a test, an IO failure
/// should be loud, not an empty listing that lets an assertion pass vacuously.
pub fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
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

// ─── Error classes ───────────────────────────────────────────────────────────

/// The coarse class of a failure, compared between backends (audit D3).
///
/// Error *text* is deliberately not compared: the backends legitimately word
/// the same failure differently, so pinning prose would turn every message
/// improvement into a test break. The *stage* is compared — one backend
/// rejecting a program before it runs while the other fails it at run time is
/// a real divergence, not a wording difference.
///
/// Two buckets is all the VM's `Backend` boundary preserves today — see
/// `VmBackendError`, whose `Runtime` variant carries only a rendered string.
/// Refining this split means carrying a class across that boundary first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorClass {
    /// The pipeline rejected the program before it ran: lex, parse, desugar,
    /// or VM compile.
    BeforeRun,
    /// The program was accepted and failed during evaluation.
    AtRuntime,
}

/// How a backend's error maps onto [`ErrorClass`]. Implemented per concrete
/// error type because the class is no longer recoverable from the rendered
/// message.
trait ClassifyError {
    fn class(&self) -> ErrorClass;
}

impl ClassifyError for patina_runtime::EvalError {
    // Desugar failures arrive here rather than via `InterpreterError::Desugar`
    // — see `EvalError::DesugarError`. Matched exhaustively so adding a
    // variant forces a classification decision instead of silently defaulting
    // to AtRuntime.
    fn class(&self) -> ErrorClass {
        use patina_runtime::EvalError::*;
        match self {
            WithLocation { error, .. } => error.class(),
            DesugarError(_) => ErrorClass::BeforeRun,
            UndefinedVariable(_)
            | NotAProcedure(_)
            | WrongArity { .. }
            | InvalidSyntax(_)
            | TypeError(_)
            | DivisionByZero
            | IndexOutOfBounds(_)
            | IOError(_)
            | InternalError(_)
            | ContinuationEscape
            | SchemeException { .. } => ErrorClass::AtRuntime,
        }
    }
}

impl ClassifyError for patina_vm::VmBackendError {
    fn class(&self) -> ErrorClass {
        match self {
            patina_vm::VmBackendError::Compile(_) | patina_vm::VmBackendError::Desugar(_) => {
                ErrorClass::BeforeRun
            }
            patina_vm::VmBackendError::Runtime { .. } => ErrorClass::AtRuntime,
        }
    }
}

/// A failure with its coarse class kept next to the rendered message. The
/// class is asserted; the message is only ever shown.
#[derive(Debug)]
struct TestError {
    class: ErrorClass,
    message: String,
}

/// What one backend did with a program: the `write`-formatted value, or the
/// classified error.
type Outcome = Result<String, TestError>;

/// Whether the code is a whole program (`eval_program`) or a single
/// expression (`eval_str`). The two entry points differ in how they treat
/// multiple top-level forms, so tests must keep using the one they chose.
#[derive(Clone, Copy)]
enum Mode {
    Expr,
    Program,
}

/// Run `code` on one backend, capturing either its value or its error.
///
/// Generic over `B: Backend` — `global_env()` is on the trait and
/// `display_tagged` is the same writer for both backends, so one body serves
/// the tree-walker and the VM.
fn run_on<B: Backend>(interp: Interpreter<B>, code: &str, mode: Mode) -> Outcome
where
    B::Error: ClassifyError,
{
    let result = match mode {
        Mode::Expr => interp.eval_str(code),
        Mode::Program => interp.eval_program(code),
    };
    match result {
        Ok(tv) => Ok(display_tagged(tv, interp.backend().global_env().heap())),
        Err(e) => {
            let class = match &e {
                InterpreterError::Lex(_)
                | InterpreterError::Parse(_)
                | InterpreterError::Desugar(_) => ErrorClass::BeforeRun,
                InterpreterError::Backend(backend_error) => backend_error.class(),
            };
            Err(TestError {
                class,
                message: e.to_string(),
            })
        }
    }
}

/// Run `code` on each backend `which` selects, labelled for failure messages.
///
/// Matched exhaustively rather than tested with `!=`, so adding a backend is a
/// compile error here instead of silently defaulting to running both.
fn outcomes(which: Which, code: &str, mode: Mode) -> Vec<(&'static str, Outcome)> {
    let tree_walker = || {
        (
            "tree-walker",
            run_on(TreeWalkInterpreter::new_tree_walker(), code, mode),
        )
    };
    let vm = || ("vm", run_on(Interpreter::new(VmBackend::new()), code, mode));
    match which {
        Which::Both => vec![tree_walker(), vm()],
        Which::TreeWalker => vec![tree_walker()],
        Which::Vm => vec![vm()],
    }
}

/// Assert every selected backend evaluates `code` to `expected`.
fn expect_value(which: Which, code: &str, expected: &str, mode: Mode) {
    for (backend, outcome) in outcomes(which, code, mode) {
        match outcome {
            Ok(actual) => assert_eq!(
                actual, expected,
                "\n[{backend}] wrong result\nProgram:\n{code}\nExpected: {expected}\nGot: {actual}"
            ),
            Err(e) => panic!(
                "\n[{backend}] failed to evaluate\nProgram:\n{code}\nError: {}",
                e.message
            ),
        }
    }
}

/// Assert every selected backend rejects `code` — at the same [`ErrorClass`].
///
/// One backend erroring where the other succeeds is itself a divergence — the
/// exact shape of the control-operator cluster in Track Q §1.2 — so this is a
/// stronger check than the single-backend version it replaces.
fn expect_error(which: Which, code: &str, mode: Mode) {
    let mut failures = Vec::new();
    for (backend, outcome) in outcomes(which, code, mode) {
        match outcome {
            Ok(value) => panic!("\n[{backend}] expected an error\nProgram:\n{code}\nGot: {value}"),
            Err(e) => failures.push((backend, e)),
        }
    }
    // Every backend must reject at the same stage as the first — written over
    // the rest of the list, not a two-element pattern, so a third backend
    // joins the check instead of silently disabling it.
    if let Some(((b1, e1), rest)) = failures.split_first() {
        for (b2, e2) in rest {
            assert_eq!(
                e1.class, e2.class,
                "\nbackends both fail, but at different stages\nProgram:\n{code}\n\
                 [{b1}] {:?}: {}\n[{b2}] {:?}: {}",
                e1.class, e1.message, e2.class, e2.message
            );
        }
    }
}

// ─── Public helpers ──────────────────────────────────────────────────────────

/// Assert that evaluating a Scheme expression produces the expected result
/// on both backends.
pub fn assert_eval_to(expr: &str, expected: &str) {
    expect_value(Which::Both, expr, expected, Mode::Expr);
}

/// Assert that evaluating a Scheme expression produces an error on both backends
pub fn assert_eval_error(expr: &str) {
    expect_error(Which::Both, expr, Mode::Expr);
}

/// Pin a **known divergence** between the backends: `works_on` must produce
/// `expected`, and the other backend must still fail **at the recorded
/// stage** (`broken_fails`). `tracking` names the document that records the
/// bug — a mandatory argument, so a quarantine cannot be written without
/// saying where it is tracked.
///
/// This is the only way to opt a test out of both-backends coverage, and it is
/// designed to **fail when the bug is fixed**: repairing the broken backend
/// trips the second assertion, whose message tells the fixer to replace this
/// call with a plain [`assert_program_eval_to`]. Asserting only the working
/// side would let a quarantine outlive its bug forever, which is how an
/// exception list becomes a permanent excuse.
///
/// The pinned [`ErrorClass`] closes the other escape (audit D3): without it,
/// *any* failure satisfied the quarantine, so a new, unrelated bug could
/// silently replace the recorded one and hide behind it.
pub fn assert_divergence(
    code: &str,
    works_on: On,
    expected: &str,
    broken_fails: ErrorClass,
    tracking: &str,
) {
    expect_value(works_on.working(), code, expected, Mode::Program);

    for (backend, outcome) in outcomes(works_on.broken(), code, Mode::Program) {
        match outcome {
            Ok(value) => panic!(
                "\n[{backend}] NO LONGER DIVERGES — it now returns {value}.\n\
                 \n\
                 This quarantine has done its job. Replace the assert_divergence \
                 call with\n    \
                 assert_program_eval_to(code, {expected:?});\n\
                 so both backends are held to the same expectation, and update \
                 {tracking}.\n\
                 \nProgram:\n{code}"
            ),
            Err(e) => assert_eq!(
                e.class, broken_fails,
                "\n[{backend}] still fails, but at a different stage \
                 ({broken_fails:?} recorded, now {:?}): {}\n\
                 The quarantined failure changed mode — check whether the \
                 original bug (see {tracking}) was replaced by a new one, and \
                 re-record the class if the change is understood.\nProgram:\n{code}",
                e.class, e.message
            ),
        }
    }
}

/// Evaluate multiple expressions in sequence on both backends, assert the two
/// agree, and return the shared result.
///
/// The agreement check is what makes callers that compare the return value
/// against their own expectation differential for free.
pub fn eval_program(code: &str) -> String {
    let tw = run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program);
    let vm = run_on(Interpreter::new(VmBackend::new()), code, Mode::Program);

    // Both failing is a shared bug in the program under test, not a
    // divergence — report both errors, since the two backends can fail for
    // different reasons and showing only one hides half the evidence.
    if let (Err(a), Err(b)) = (&tw, &vm) {
        panic!(
            "\nFailed to evaluate program:\n{code}\ntree-walker: {}\nvm: {}",
            a.message, b.message
        );
    }

    // Agreement is on *values*: error text is deliberately not compared (see
    // `ErrorClass`), and the class needs no check here because both-failing
    // already panicked above.
    let show = |o: &Outcome| match o {
        Ok(v) => v.clone(),
        Err(e) => format!("errored: {}", e.message),
    };
    assert!(
        tw.as_ref().ok() == vm.as_ref().ok(),
        "\nbackends disagree\nProgram:\n{code}\ntree-walker: {}\nvm: {}",
        show(&tw),
        show(&vm)
    );

    tw.expect("checked non-error above")
}

/// Evaluate a program on the tree-walker only and `write` the result. For
/// tests that target tree-walker-specific machinery.
pub fn eval_program_tree_walker(code: &str) -> String {
    match run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// Evaluate a program on the VM only and `write` the result. For tests that
/// exercise VM-only machinery (CallPrimitive deopt, inline opcodes).
pub fn eval_program_vm(code: &str) -> String {
    match run_on(Interpreter::new(VmBackend::new()), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// Assert that a multi-expression program produces expected result on both backends
pub fn assert_program_eval_to(code: &str, expected: &str) {
    expect_value(Which::Both, code, expected, Mode::Program);
}

/// Assert that evaluating a multi-expression program produces an error on both backends
pub fn assert_program_eval_error(code: &str) {
    expect_error(Which::Both, code, Mode::Program);
}

/// Assert that evaluating an expression with (scheme char) imported produces
/// expected result on both backends
pub fn assert_eval_with_scheme_char(expr: &str, expected: &str) {
    let code = format!("(import (scheme char)) {}", expr);
    expect_value(Which::Both, &code, expected, Mode::Program);
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
/// result: `eval_program_tree_walker` or `eval_program_vm`.
///
/// These take a *single-backend* evaluator on purpose. Several cases read
/// `(gc-stats)` counters, which legitimately differ between the backends —
/// the same workload frees 12295 pairs on the VM and 12294 on the
/// tree-walker — so routing them through the agreement-asserting
/// `eval_program` would fail on a difference that is not a divergence.
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
