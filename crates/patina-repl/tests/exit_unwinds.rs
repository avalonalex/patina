//! `exit` runs the after thunk of every outstanding `dynamic-wind` before it
//! ends the process (R7RS 6.14, #336), and `emergency-exit` does not.
//!
//! These run the binary: `exit` ends the process it runs in, so no in-process
//! driver can observe it, which is also why `control_flow_matrix.rs` has no row
//! for it. Every case runs on both backends.
//!
//! # The oracle
//!
//! Measured 2026-09-15 against chibi 0.12, Gauche 0.9.15 and Chez 10.4.1
//! (`petite --script`, which has no `emergency-exit`), with the thunks writing
//! to stderr: Gauche does not flush standard output on the way out of an
//! `exit` whose thunks raised or exited, which reads as running nothing. Each
//! test names the implementations its expectation rests on.
//!
//! | Case | chibi | Gauche | Chez |
//! |---|---|---|---|
//! | nested extents unwind innermost first, and the status is kept | ✓ | ✓ | ✓ |
//! | `exit` in a before thunk runs only the enclosing after thunks | ✓ | ✓ | ✓ |
//! | `exit` from a handler, from a `map` callback, under `parameterize` | ✓ | ✓ | ✓ |
//! | an after thunk runs under its own call's exception handlers | ✓ | prints nothing | ✓ |
//! | `exit` in an after thunk while exiting: the rest run, with its status | loops | skips the rest | ✓ |
//! | an after thunk that escapes abandons the exit | loops | ✓ | ✓ |
//! | `exit` in an after thunk that an escape is running | loops | ✓ | ✓ |
//! | `emergency-exit` runs no after thunk | ✓ | ✓ | n/a |
//! | a `guard` outside the exit catches an after thunk's error, and the program carries on | loops | exits 3 | ✓ |
//! | an unhandled error in an after thunk while exiting | reports it, exits 70 | ignores it, exits 3 | exits 255 |
//!
//! The last row has no agreement, and the choice made is this: the error is
//! reported as any other is, and the process ends with the status a program
//! that reported an error gets — the one `exit` asked for, or 1 if it asked
//! for success — even under `-k` or in a session, since the program asked to
//! exit. Every oracle ends the process there too.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline_status};

const PRELUDE: &str =
    "(import (scheme base) (scheme lazy) (scheme write) (scheme process-context))\n";

/// Run `program` after the prelude as a file, with `flags`, on `backend`, and
/// return its stdout, stderr and exit code.
fn run(backend: &[&str], flags: &[&str], program: &str) -> (String, String, i32) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.scm"), format!("{PRELUDE}{program}")).unwrap();
    let mut args = backend.to_vec();
    args.extend_from_slice(flags);
    args.push("p.scm");
    finished(run_with_deadline_status(dir.path(), &args, None))
}

/// Run `program` after the prelude from standard input, with `flags`.
fn run_stdin(backend: &[&str], flags: &[&str], program: &str) -> (String, String, i32) {
    let dir = tempfile::tempdir().unwrap();
    let mut args = backend.to_vec();
    args.extend_from_slice(flags);
    let input = format!("{PRELUDE}{program}");
    finished(run_with_deadline_status(dir.path(), &args, Some(&input)))
}

fn finished(
    (stdout, stderr, status): (String, String, std::process::ExitStatus),
) -> (String, String, i32) {
    let code = status
        .code()
        .unwrap_or_else(|| panic!("killed by a signal: {status}\n{stderr}"));
    (stdout, stderr, code)
}

/// A `dynamic-wind` around `body` whose thunks display `before` and `after`.
fn wind(before: &str, body: &str, after: &str) -> String {
    format!(
        "(dynamic-wind (lambda () (display \"{before}\")) (lambda () {body}) (lambda () (display \"{after}\")))"
    )
}

/// chibi, Gauche and Chez.
#[test]
fn after_thunks_run_innermost_first_and_the_status_is_kept() {
    let nested = wind("in1 ", &wind("in2 ", "(exit 3)", "out2 "), "out1");
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], &nested),
            ("in1 in2 out2 out1".into(), "".into(), 3),
            "{backend:?}"
        );
        for (exit, code) in [("(exit)", 0), ("(exit #t)", 0), ("(exit #f)", 1)] {
            let (stdout, stderr, status) = run(backend, &[], &wind("", exit, "after"));
            assert_eq!(
                (stdout.as_str(), status),
                ("after", code),
                "{backend:?} {exit}: {stderr}"
            );
        }
    }
}

/// chibi, Gauche and Chez.
#[test]
fn exit_in_a_before_thunk_runs_only_the_enclosing_after_thunks() {
    let program = wind(
        "",
        "(dynamic-wind (lambda () (exit 4)) (lambda () (display \"body\")) (lambda () (display \"inner-after \")))",
        "outer-after",
    );
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], &program),
            ("outer-after".into(), "".into(), 4),
            "{backend:?}"
        );
    }
}

/// chibi, Gauche and Chez.
#[test]
fn exit_unwinds_from_a_handler_and_from_a_primitive_s_callback() {
    for (body, code) in [
        (
            "(with-exception-handler (lambda (e) (exit 6)) (lambda () (+ 1 (raise-continuable 'oops))))",
            6,
        ),
        ("(map (lambda (x) (exit 0)) (list 1 2))", 0),
        ("(vector-map (lambda (x) (exit 5)) (vector 1 2))", 5),
        ("(force (delay (exit 7)))", 7),
    ] {
        for backend in BOTH_BACKENDS {
            let (stdout, stderr, status) = run(backend, &[], &wind("", body, "after"));
            assert_eq!(
                (stdout.as_str(), status),
                ("after", code),
                "{backend:?} {body}: {stderr}"
            );
        }
    }
}

/// An after thunk runs in the dynamic environment of its own `dynamic-wind`
/// call: a `parameterize` around it (chibi, Gauche, Chez) and the exception
/// handlers installed around it (chibi, Chez).
#[test]
fn after_thunks_run_in_their_own_dynamic_environment() {
    let parameter = "(define p (make-parameter 1))\n\
                     (parameterize ((p 2)) (dynamic-wind (lambda () #f) (lambda () (exit 0)) (lambda () (display (p)))))";
    let handler = "(with-exception-handler (lambda (e) (display \"outer-handler \") 42)\n\
                   (lambda () (dynamic-wind (lambda () #f) (lambda () (exit 3))\n\
                     (lambda () (display (+ 1 (raise-continuable 'from-after)))))))";
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], parameter),
            ("2".into(), "".into(), 0),
            "{backend:?}"
        );
        assert_eq!(
            run(backend, &[], handler),
            ("outer-handler 43".into(), "".into(), 3),
            "{backend:?}"
        );
    }
}

/// An `exit` inside an after thunk that an `exit` is running starts again from
/// where the first had got to: the thunk's own record is already gone, so the
/// rest run once, and the process ends with the second status. Chez; Gauche
/// skips the rest, and chibi loops.
#[test]
fn exit_in_an_after_thunk_while_exiting_runs_the_rest_with_its_status() {
    let program = "(dynamic-wind (lambda () #f)\n\
                     (lambda () (dynamic-wind (lambda () #f) (lambda () (exit 3)) (lambda () (display \"inner-after \") (exit 7))))\n\
                     (lambda () (display \"outer-after\")))";
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], program),
            ("inner-after outer-after".into(), "".into(), 7),
            "{backend:?}"
        );
    }
}

/// An after thunk that escapes abandons the exit, as it abandons any jump, and
/// the program carries on. Gauche and Chez; chibi loops.
#[test]
fn an_after_thunk_that_escapes_abandons_the_exit() {
    let program = "(define r (call-with-current-continuation (lambda (k)\n\
                     (dynamic-wind (lambda () #f) (lambda () (exit 3)) (lambda () (k 'escaped))))))\n\
                   (display r) (display \" still running\")";
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], program),
            ("escaped still running".into(), "".into(), 0),
            "{backend:?}"
        );
    }
}

/// An `exit` inside an after thunk that an escape is running still leaves the
/// extents outside it. Gauche and Chez; chibi loops.
#[test]
fn exit_in_an_after_thunk_an_escape_is_running_unwinds_the_rest() {
    let program = "(dynamic-wind (lambda () #f)\n\
                     (lambda () (call-with-current-continuation (lambda (k)\n\
                       (dynamic-wind (lambda () #f) (lambda () (k 1)) (lambda () (display \"inner-after \") (exit 5))))))\n\
                     (lambda () (display \"outer-after\")))";
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], program),
            ("inner-after outer-after".into(), "".into(), 5),
            "{backend:?}"
        );
    }
}

/// chibi and Gauche; Chez has no `emergency-exit`.
#[test]
fn emergency_exit_runs_no_after_thunk() {
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, &[], &wind("in ", "(emergency-exit 2)", "after")),
            ("in ".into(), "".into(), 2),
            "{backend:?}"
        );
    }
}

/// `exit` unwinds in a program read from standard input and in a session too,
/// and a program that already reported an error under `-k` still does not exit
/// with success.
#[test]
fn exit_unwinds_from_standard_input_in_a_session_and_after_an_error() {
    let program = wind("in ", "(exit 0)", "after");
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run_stdin(backend, &[], &program),
            ("in after".into(), "".into(), 0),
            "{backend:?}"
        );
        let (stdout, stderr, status) = run_stdin(backend, &["-i"], &format!("{program}\n"));
        assert!(stdout.contains("in after"), "{backend:?}: {stdout}{stderr}");
        assert!(!stdout.contains("Goodbye!"), "{backend:?}: {stdout}");
        assert_eq!(status, 0, "{backend:?}: {stderr}");

        let (stdout, stderr, status) = run(backend, &["-k"], &format!("(car 5)\n{program}"));
        assert_eq!(
            (stdout.as_str(), status),
            ("in after", 1),
            "{backend:?}: {stderr}"
        );
    }
}

/// An error that no handler takes, raised by an after thunk while `exit` is
/// unwinding, is reported and still ends the process — the extents outside it
/// are not left, the next form does not run, and the status is the one `exit`
/// asked for, or 1 for a success — from a file, under `-k`, from standard input
/// and in a session. The oracles all end the process here, and disagree about
/// the rest; see the module documentation.
#[test]
fn an_unhandled_error_in_an_after_thunk_while_exiting_is_reported_and_ends_the_process() {
    let failing = |status: &str| {
        format!(
            "{}\n(display \"next form\")\n",
            wind(
                "",
                &format!(
                    "(dynamic-wind (lambda () #f) (lambda () (exit {status})) (lambda () (display \"inner-after \") (error \"boom in after\")))"
                ),
                "outer-after"
            )
        )
    };
    for backend in BOTH_BACKENDS {
        for (label, (stdout, stderr, code)) in [
            ("file", run(backend, &[], &failing("3"))),
            ("-k", run(backend, &["-k"], &failing("3"))),
            ("stdin", run_stdin(backend, &[], &failing("3"))),
            ("stdin -k", run_stdin(backend, &["-k"], &failing("3"))),
        ] {
            assert_eq!(code, 3, "{backend:?} {label}: {stdout}{stderr}");
            assert_eq!(stdout, "inner-after ", "{backend:?} {label}: {stderr}");
            assert!(
                stderr.contains("boom in after"),
                "{backend:?} {label}: {stderr}"
            );
        }
        let (_, stderr, code) = run(backend, &["-k"], &failing("0"));
        assert_eq!(code, 1, "{backend:?}: {stderr}");

        let (stdout, stderr, code) = run_stdin(backend, &["-i"], &failing("3"));
        assert_eq!(code, 3, "{backend:?} session: {stdout}{stderr}");
        assert!(
            format!("{stdout}{stderr}").contains("boom in after"),
            "{backend:?} session: {stdout}{stderr}"
        );
        assert!(
            !stdout.contains("next form"),
            "{backend:?} session: {stdout}"
        );
    }
}

/// An error in an after thunk that something outside the `exit` catches is an
/// escape like any other: the exit is abandoned, and the program carries on
/// without being ended by the error it handled. Chez; Gauche ends the process
/// with the exit's status, and chibi loops.
#[test]
fn an_error_in_an_after_thunk_caught_outside_the_exit_abandons_it() {
    let program = "(display (guard (e (#t 'caught))\n\
                     (dynamic-wind (lambda () #f) (lambda () (exit 3)) (lambda () (error \"boom\")))))\n\
                   (display \" running\")\n\
                   (car 5)\n\
                   (display \" after the error\")";
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, code) = run(backend, &["-k"], program);
        assert_eq!(
            stdout, "caught running after the error",
            "{backend:?}: {stderr}"
        );
        assert_eq!(code, 1, "{backend:?}: {stderr}");
    }
}
