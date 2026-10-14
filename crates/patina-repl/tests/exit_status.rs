//! A program's exit status says whether it failed, whatever its path is
//! called and whatever its comments say (#335).
//!
//! The CLI chose how to run a program by sniffing: a path containing `test`,
//! or `test-begin` anywhere in the text, selected a mode that carried on past
//! an evaluation error and also reported success whatever happened. So a
//! failing program under a directory named `latest/` exited 0, and so did one
//! whose comment mentioned `test-begin`.
//!
//! These run the binary because the exit status is the claim, and they assert
//! only on the status and on the error being reported: whether a program
//! stops at its first error or carries on is a separate question from whether
//! it admits to failing. Every case runs on both backends.
//!
//! `-k` asks for the carrying-on explicitly, and never changes the verdict.

mod common;

use common::{
    BOTH_BACKENDS, expect_failure_on_both_backends, run_both_backends, run_patina,
    run_with_deadline,
};
use std::fs;
use std::path::Path;

/// A program whose second form raises an evaluation error.
const FAILS: &str = "(import (scheme base))\n(car 5)\n";

fn write(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("a parent directory")).unwrap();
    fs::write(path, body).unwrap();
}

/// Paths are passed relative to the working directory, so the name the binary
/// sees is exactly the one written here and the temporary directory's own
/// randomly named path cannot match anything by accident.
#[test]
fn a_failing_program_fails_whatever_its_directory_is_called() {
    let dir = tempfile::tempdir().unwrap();
    for sub in ["plain", "latest", "contest", "tests"] {
        let relative = format!("{sub}/boom.scm");
        write(dir.path(), &relative, FAILS);
        expect_failure_on_both_backends(dir.path(), &[&relative], |stderr| {
            assert!(stderr.contains("Error"), "{relative}: {stderr}");
        });
    }
}

#[test]
fn a_failing_program_fails_whatever_its_file_is_named() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["boom.scm", "boom_test.scm", "test.scm", "r7rs-tests.scm"] {
        write(dir.path(), name, FAILS);
        expect_failure_on_both_backends(dir.path(), &[name], |stderr| {
            assert!(stderr.contains("Error"), "{name}: {stderr}");
        });
    }
}

/// The text half of the sniff matched raw source, so a word in a comment or a
/// string literal was enough to forfeit the status.
#[test]
fn a_failing_program_fails_whatever_its_comments_and_strings_say() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        (
            "comment.scm",
            "(import (scheme base))\n;; TODO: port to test-begin one day\n(car 5)\n",
        ),
        (
            "string.scm",
            "(import (scheme base))\n(define label \"test-begin\")\n(car 5)\n",
        ),
    ] {
        write(dir.path(), name, body);
        expect_failure_on_both_backends(dir.path(), &[name], |stderr| {
            assert!(stderr.contains("Error"), "{name}: {stderr}");
        });
    }
}

/// The other direction has to hold as well: a program that reports no error
/// still succeeds, however it is named. Failing every test-named file would be
/// a different wrong answer, not a fix.
#[test]
fn a_program_that_reports_no_error_succeeds_however_it_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let body = "(import (scheme base))\n;; test-begin\n(define x 1)\n";
    for relative in ["latest/ok_test.scm", "plain/ok.scm"] {
        write(dir.path(), relative, body);
        run_both_backends(dir.path(), &[relative], "");
    }
}

/// `-k` changes how much of a program runs, never whether it failed.
#[test]
fn keep_going_still_fails_a_failing_program_and_passes_a_clean_one() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "boom.scm",
        "(import (scheme base) (scheme write))\n(car 5)\n(display \"after\")\n(car 6)\n",
    );
    write(
        dir.path(),
        "clean.scm",
        "(import (scheme base) (scheme write))\n(display \"fine\")\n",
    );
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["-k", "boom.scm"]);
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "{args:?}");
        assert_eq!(stdout.trim(), "after", "{args:?}");
        let reported = stderr.lines().filter(|l| l.starts_with("Error")).count();
        assert_eq!(reported, 2, "{args:?}: {stderr}");

        let mut args = backend.to_vec();
        args.extend_from_slice(&["-k", "clean.scm"]);
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(ok, "a clean program under -k succeeds: {args:?}: {stderr}");
        assert_eq!(stdout.trim(), "fine", "{args:?}");
    }
}

/// A program that reports an error and then calls `(exit 0)` has still failed.
/// `exit` ends the process before the runner that counted the errors can choose
/// a status, so without a process-wide record `-k` would let a failing program
/// report success by asking for it.
#[test]
fn an_exit_after_a_reported_error_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    let base = "(import (scheme base) (scheme process-context))\n";
    write(
        dir.path(),
        "exit0.scm",
        &format!("{base}(car 5)\n(exit 0)\n"),
    );
    write(
        dir.path(),
        "exit3.scm",
        &format!("{base}(car 5)\n(exit 3)\n"),
    );
    write(dir.path(), "clean.scm", &format!("{base}(exit 0)\n"));
    for backend in BOTH_BACKENDS {
        let status = |name: &str| {
            let mut args = backend.to_vec();
            args.extend_from_slice(&["-k", name]);
            common::patina_command(dir.path(), &args, &[])
                .output()
                .expect("failed to spawn patina binary")
                .status
                .code()
        };
        assert_eq!(
            status("exit0.scm"),
            Some(1),
            "{backend:?}: success is withheld"
        );
        assert_eq!(
            status("exit3.scm"),
            Some(3),
            "{backend:?}: a failure is kept"
        );
        assert_eq!(status("clean.scm"), Some(0), "{backend:?}: nothing failed");
    }
}

/// The suite shape the record exists for: an error escapes to top level, `-k`
/// carries on, every assertion that runs passes, and SRFI 64's `(test-exit)`
/// asks for 0 because the escaped error never reached the runner's count.
#[test]
fn a_suite_that_escaped_an_error_fails_even_through_test_exit() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "suite.scm",
        "(import (scheme base) (srfi 64))\n(test-begin \"s\")\n(test-equal 1 1)\n(car 5)\n\
         (test-equal 2 2)\n(test-end \"s\")\n(test-exit)\n",
    );
    expect_failure_on_both_backends(dir.path(), &["-k", "suite.scm"], |stderr| {
        assert!(stderr.contains("Error"), "{stderr}");
    });
}

/// The boundary that stops the fix over-firing: a failure a test framework
/// catches is the program handling it, so the run still succeeds unless the
/// suite asks otherwise with `(test-exit)`.
#[test]
fn a_failure_a_test_framework_catches_leaves_the_status_to_the_suite() {
    let dir = tempfile::tempdir().unwrap();
    let suite = "(import (scheme base) (srfi 64))\n(test-begin \"c\")\n(test-equal 1 2)\n(test-end \"c\")\n";
    write(dir.path(), "caught.scm", suite);
    write(
        dir.path(),
        "caught-exit.scm",
        &format!("{suite}(test-exit)\n"),
    );
    for backend in BOTH_BACKENDS {
        for keep_going in [&[][..], &["-k"][..]] {
            let mut args = backend.to_vec();
            args.extend_from_slice(keep_going);
            args.push("caught.scm");
            let (_, stderr, ok) = run_patina(dir.path(), &args);
            assert!(ok, "{args:?}: {stderr}");

            let mut args = backend.to_vec();
            args.extend_from_slice(keep_going);
            args.push("caught-exit.scm");
            let (_, _, ok) = run_patina(dir.path(), &args);
            assert!(!ok, "(test-exit) reports the failed assertion: {args:?}");
        }
    }
}

/// `-k` decides how much of a program runs, so it is refused where no program
/// runs, and where carrying on is already what happens.
#[test]
fn keep_going_is_refused_where_it_cannot_change_anything() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "p.scm", "(import (scheme base))\n");
    for args in [
        &["-k", "-p", "(+ 1 2)"][..],
        &["-k", "--dump", "p.scm"][..],
        &["-k", "-i"][..],
    ] {
        let (_, stderr, ok) = run_patina(dir.path(), args);
        assert!(!ok, "{args:?} must be refused");
        assert!(stderr.contains("-k"), "{args:?}: {stderr}");
    }
}

/// A program on standard input is judged as a file is, so the text half of the
/// old sniff reached it too: a piped failing program that mentioned
/// `test-begin` exited 0.
#[test]
fn a_failing_program_on_standard_input_fails_whatever_it_says() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let (_, stderr, ok) = run_with_deadline(
            dir.path(),
            backend,
            Some("(import (scheme base))\n;; test-begin\n(car 5)\n"),
        );
        assert!(!ok, "{backend:?}: {stderr}");
    }
}

/// A program that writes a marker file once it runs, leaves output unflushed,
/// then loops until it is interrupted.
const LOOPS: &str = "(import (scheme base) (scheme write) (scheme file))
(display \"written\")
(call-with-output-file \"started\" (lambda (port) (write 'started port)))
(let loop () (loop))
";

/// Run patina with `args` in `dir`, `program` on its standard input if given,
/// and send it SIGINT once it has made the marker file: its (stdout,
/// stderr, status).
fn interrupted(
    dir: &Path,
    args: &[&str],
    program: Option<&str>,
) -> (String, String, std::process::ExitStatus) {
    let marker = dir.join("started");
    let _ = fs::remove_file(&marker);
    let mut patina = common::spawn_patina(dir, args);
    if let Some(program) = program {
        patina.write(program);
    }
    let started = std::time::Instant::now();
    while !marker.exists() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(marker.exists(), "patina {args:?} never started its loop");
    patina.interrupt();
    patina.finish_with_status()
}

/// A program Ctrl-C interrupts ends with status 130, 128 + SIGINT, once the
/// output it left unflushed is written (#701; decision 3 on #697): a script,
/// with `-k` or without, a program on standard input, and `-p`'s
/// expressions, on both backends. The interrupt is reported as an error.
#[test]
fn an_interrupted_program_exits_130_with_its_output_written() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "loops.scm", LOOPS);
    for backend in BOTH_BACKENDS {
        for (mode, extra, program) in [
            ("script", &["loops.scm"][..], None),
            ("-k script", &["-k", "loops.scm"][..], None),
            ("stdin", &[][..], Some(LOOPS)),
            ("-p", &["-p", LOOPS][..], None),
        ] {
            let args: Vec<&str> = backend.iter().chain(extra).copied().collect();
            let (stdout, stderr, status) = interrupted(dir.path(), &args, program);
            assert_eq!(
                status.code(),
                Some(130),
                "{backend:?} {mode}: {status}\nstderr: {stderr}"
            );
            assert_eq!(stdout, "written", "{backend:?} {mode}: {stderr}");
            assert!(
                stderr.contains("Error: interrupted"),
                "{backend:?} {mode}: {stderr}"
            );
        }
    }
}

/// A program that catches the interrupt with `(patina interrupt)`'s
/// `interrupt?` runs on, and ends as it would have.
#[test]
fn a_program_that_catches_the_interrupt_runs_on() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "catches.scm",
        "(import (scheme base) (scheme write) (scheme file) (patina interrupt))
(display (guard (e ((interrupt? e) 'caught))
  (call-with-output-file \"started\" (lambda (port) (write 'started port)))
  (let loop () (loop))))
(newline)
",
    );
    for backend in BOTH_BACKENDS {
        let args: Vec<&str> = backend.iter().copied().chain(["catches.scm"]).collect();
        let (stdout, stderr, status) = interrupted(dir.path(), &args, None);
        assert!(status.success(), "{backend:?}: {status}\nstderr: {stderr}");
        assert_eq!(stdout, "caught\n", "{backend:?}: {stderr}");
    }
}
