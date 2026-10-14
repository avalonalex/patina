//! Ctrl-C stops a tight loop (#701; `PRD/GC_PRD.md` §12, DESIGN Appendix F).
//!
//! A host posts `SIGNAL` through its interpreter's interrupt handle, from
//! another thread, as the REPL's SIGINT handler does from a signal handler.
//! The running evaluation raises a non-continuable `&interrupt` at its next
//! poll. The condition is not an error object (owner decision on #701,
//! 2026-10-13): `(patina interrupt)`'s `interrupt?` answers `#t` for it and
//! `error-object?` `#f`, so a guard that catches errors lets it through.
//! Unhandled, the evaluation answers an error whose diagnostic kind is
//! `interrupt`, which a script runner turns into exit status 130.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter, event};
use patina_runtime::{DiagnosticKind, HasDiagnostic};
use std::time::Duration;

/// How an evaluation ended.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Its value, written.
    Value(String),
    /// The interrupt, which nothing handled.
    Interrupted,
    /// Any other error.
    Error(String),
}

use Outcome::{Interrupted, Value};

fn outcome<E: HasDiagnostic + std::fmt::Display>(result: Result<String, E>) -> Outcome {
    match result {
        Ok(value) => Value(value),
        Err(e) if e.diagnostic().kind == DiagnosticKind::Interrupt => Interrupted,
        Err(e) => Outcome::Error(e.to_string()),
    }
}

/// Run `program` on `interp` while another thread posts `SIGNAL` to it
/// soon after it starts.
fn run_interrupted<B: Backend>(interp: &Interpreter<B>, program: &str) -> Outcome
where
    B::Error: HasDiagnostic,
{
    let handle = interp.interrupt_handle();
    let (finished, done) = std::sync::mpsc::channel::<()>();
    let poster = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        assert!(handle.post(event::SIGNAL), "the interpreter is alive");
        // A loop the interrupt does not stop never returns: end the run
        // rather than hang it.
        if done.recv_timeout(Duration::from_secs(60)).is_err() {
            eprintln!("the interrupt did not stop the program within 60 s");
            std::process::abort();
        }
    });
    let result = interp
        .eval_program_owned(program)
        .map(|value| interp.display_tagged(value));
    finished.send(()).expect("the poster waits");
    poster.join().expect("the poster");
    outcome(result)
}

/// `program`, interrupted, on each backend.
fn on_both(program: &str) -> [(&'static str, Outcome); 2] {
    [
        ("vm", run_interrupted(&vm_interpreter(), program)),
        (
            "tree-walker",
            run_interrupted(&tree_walker_interpreter(), program),
        ),
    ]
}

/// Assert that `program`, interrupted, ends as `expected` on both backends.
fn assert_both(program: &str, expected: Outcome) {
    for (backend, outcome) in on_both(program) {
        assert_eq!(outcome, expected, "{backend}: {program}");
    }
}

/// DESIGN Appendix F's test: a self-tail loop that allocates nothing stops
/// with the condition. It passes a poll at every iteration, the loop's own
/// entry.
#[test]
fn a_tight_loop_stops_with_the_interrupt() {
    assert_both("(let loop () (loop))", Interrupted);
}

/// Loops of other shapes stop as well: one that calls primitives, one
/// through a global's self tail call, one that writes the heap, and one
/// beneath a thousand frames of non-tail recursion, which the interrupt
/// abandons. (Unbounded recursion abandoned that way overflows the
/// tree-walker's Rust stack whatever abandons it, #725.)
#[test]
fn other_loops_stop_too() {
    for program in [
        "(let loop ((i 0)) (loop (+ i 1)))",
        "(define (spin n) (if (< n 0) n (spin (+ n 1)))) (spin 0)",
        "(define v (make-vector 1 0))
         (let loop () (vector-set! v 0 (+ 1 (vector-ref v 0))) (loop))",
        "(define (deep n) (if (= n 1000) (let loop () (loop)) (+ 1 (deep (+ n 1)))))
         (deep 0)",
    ] {
        assert_both(program, Interrupted);
    }
}

/// `interrupt?` catches it, and the guard's body is abandoned.
#[test]
fn interrupt_p_catches_it() {
    assert_both(
        "(import (patina interrupt))
         (guard (e ((interrupt? e) 'stopped))
           (let loop () (loop)))",
        Value("stopped".into()),
    );
}

/// It is not an error object: a guard on `error-object?` lets it through to
/// the top level, as Racket's `exn:break` passes an `exn:fail?` handler.
#[test]
fn a_guard_on_errors_lets_it_through() {
    assert_both(
        "(guard (e ((error-object? e) 'swallowed))
           (let loop () (loop)))",
        Interrupted,
    );
    assert_both(
        "(import (patina interrupt))
         (guard (e (#t (list (error-object? e) (interrupt? e)
                             (file-error? e) (read-error? e))))
           (let loop () (loop)))",
        Value("(#f #t #f #f)".into()),
    );
}

/// It is raised non-continuably: a handler that returns raises a secondary
/// exception, as for any `raise`.
#[test]
fn it_is_not_continuable() {
    assert_both(
        "(import (patina interrupt))
         (guard (e ((interrupt? e) 'interrupt) (#t 'secondary))
           (with-exception-handler
             (lambda (e) 'returned)
             (lambda () (let loop () (loop)))))",
        Value("secondary".into()),
    );
}

/// The guard's escape unwinds the loop's dynamic extent: the after thunk
/// runs.
#[test]
fn the_after_thunks_run_on_the_way_out() {
    assert_both(
        "(import (patina interrupt))
         (define cleaned #f)
         (guard (e ((interrupt? e) cleaned))
           (dynamic-wind
             (lambda () #f)
             (lambda () (let loop () (loop)))
             (lambda () (set! cleaned #t))))",
        Value("#t".into()),
    );
}

/// A loop a primitive runs, beneath a `NoGcScope`, where collections stay
/// deferred, still takes the interrupt: here `(patina internal lists)`'s
/// `member`, which calls its comparator from Rust on a nested loop.
#[test]
fn a_nested_loop_takes_it_too() {
    assert_both(
        "(import (prefix (patina internal lists) internal:))
         (internal:member 1 '(2) (lambda (a b) (let loop () (loop))))",
        Interrupted,
    );
    assert_both(
        "(import (patina interrupt) (prefix (patina internal lists) internal:))
         (guard (e ((interrupt? e) 'stopped))
           (internal:member 1 '(2) (lambda (a b) (let loop () (loop)))))",
        Value("stopped".into()),
    );
}

/// A post while nothing runs stops the next evaluation at its start, and
/// only that one; the interpreter runs on after an interrupt, as after any
/// error that reaches the top level.
#[test]
fn a_post_between_evaluations_stops_the_next_only() {
    fn check<B: Backend>(backend: &str, interp: Interpreter<B>)
    where
        B::Error: HasDiagnostic,
    {
        let run = |program: &str| {
            outcome(
                interp
                    .eval_program_owned(program)
                    .map(|value| interp.display_tagged(value)),
            )
        };
        assert!(interp.interrupt_handle().post(event::SIGNAL));
        assert_eq!(run("(define x 2) (+ x 1)"), Interrupted, "{backend}");
        assert_eq!(run("(define y 2) (+ y 1)"), Value("3".into()), "{backend}");
        assert_eq!(
            run_interrupted(&interp, "(let loop () (loop))"),
            Interrupted,
            "{backend}"
        );
        assert_eq!(run("(+ y 2)"), Value("4".into()), "{backend}");
    }
    check("vm", vm_interpreter());
    check("tree-walker", tree_walker_interpreter());
}
