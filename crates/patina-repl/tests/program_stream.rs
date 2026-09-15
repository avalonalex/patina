//! A program on standard input runs as it arrives, and a session whose input
//! ends inside a form reports that form rather than dropping it (#333).
//!
//! These run the binary: the claims are about a pipe, a process's exit status,
//! and when a program's side effects happen. Every case runs on both backends.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline};
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The start of the parser's message for input that ends inside a form.
const DIAGNOSTIC: &str = "Unexpected end of input inside the datum beginning at";

/// A form runs before the input after it has been written. The first form
/// creates a file, and the writer waits for it with the pipe still open, which
/// never ends against a runner that reads to the end of its input first.
#[test]
fn a_program_on_standard_input_runs_each_form_as_it_arrives() {
    for backend in BOTH_BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let mut child = common::patina_command(dir.path(), backend, &[])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn patina binary");
        let mut stdin = child.stdin.take().expect("stdin pipe");
        stdin
            .write_all(
                b"(import (scheme base) (scheme file))\n\
                  (call-with-output-file \"marker\" (lambda (port) (write-char #\\x port)))\n",
            )
            .unwrap();
        stdin.flush().unwrap();

        let marker = dir.path().join("marker");
        let waiting = Instant::now();
        while !marker.exists() && waiting.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let arrived = marker.exists();

        // Finish the program either way, so a failure does not leave it waiting.
        let _ = stdin.write_all(b"(define done #t)\n");
        drop(stdin);
        let finishing = Instant::now();
        while child.try_wait().expect("wait on patina").is_none() {
            if finishing.elapsed() > Duration::from_secs(10) {
                child.kill().ok();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().expect("collect patina output");
        assert!(
            arrived,
            "{backend:?}: the first form had not run 10 s after it was written"
        );
        assert!(
            output.status.success(),
            "{backend:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Positions are the program's, not the piece's: an error on a later line still
/// names that line and quotes it.
#[test]
fn an_error_later_in_the_stream_is_placed_and_quoted() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let (_, stderr, ok) = run_with_deadline(
            dir.path(),
            backend,
            Some("(import (scheme base))\n(define x 1)\n\n(no-such-procedure x)\n"),
        );
        assert!(!ok, "{backend:?}");
        assert!(stderr.contains("<stdin>:4:1"), "{backend:?}: {stderr}");
        assert!(
            stderr.contains("4 | (no-such-procedure x)"),
            "{backend:?}: {stderr}"
        );
    }
}

#[test]
fn forms_that_span_lines_on_standard_input_run() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme base) (scheme write))\n(display\n  \"one\ntwo\")\n\
                   #| a block\ncomment |#\n(newline)\n";
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, "one\ntwo\n", "{backend:?}");
    }
}

#[test]
fn keep_going_on_standard_input_runs_past_an_error_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("-k");
        let (stdout, _, ok) = run_with_deadline(
            dir.path(),
            &args,
            Some(
                "(import (scheme base) (scheme write))\n(no-such-procedure)\n(display \"after\")\n",
            ),
        );
        assert!(!ok, "{args:?}");
        assert_eq!(stdout.trim(), "after", "{args:?}");
    }
}

/// A read from standard input inside the program takes the line after the one
/// holding the form: the program is read a line at a time, so a line it has not
/// reached is still there to read.
#[test]
fn a_program_on_standard_input_reads_the_lines_after_its_form() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(
            dir.path(),
            backend,
            Some(
                "(import (scheme base) (scheme write))\n(display (read-line))\nhello\n(newline)\n",
            ),
        );
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, "hello\n", "{backend:?}");
    }
}

/// A session whose input ends inside a form reports it the way a file does,
/// after running what came before, and fails. The editor throws the half-read
/// form away at the end of its input; the validator had kept it.
#[test]
fn a_session_cut_off_inside_a_form_reports_it_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("-i");
        let (stdout, stderr, ok) =
            run_with_deadline(dir.path(), &args, Some("(+ 40 2)\n(define y\n  (+ 1\n"));
        assert!(!ok, "{args:?}: {stdout}");
        assert!(stdout.contains("42"), "{args:?}: {stdout}");
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("(define y"),
            "{args:?}: {stderr}"
        );
        assert!(!stdout.contains("Goodbye!"), "{args:?}: {stdout}");
    }
}

#[test]
fn a_session_whose_input_ends_between_forms_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("-i");
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), &args, Some("(+ 40 2)\n"));
        assert!(ok, "{args:?}: {stderr}");
        assert!(
            stdout.contains("42") && stdout.contains("Goodbye!"),
            "{args:?}: {stdout}"
        );
    }
}

/// `#!fold-case` holds for the rest of a program, not only for the piece it was
/// read in. A program on standard input is read a piece at a time, so the
/// reader's case folding has to carry from one piece to the next, as it does
/// through a file.
#[test]
fn fold_case_holds_across_the_pieces_of_a_program_on_standard_input() {
    let dir = tempfile::tempdir().unwrap();
    let program = "#!fold-case\n(IMPORT (SCHEME BASE) (SCHEME WRITE))\n\
                   (DEFINE GREETING \"hi\")\n(DISPLAY GREETING)\n";
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, "hi", "{backend:?}");
    }
}

/// One form spanning many lines is read in time proportional to its size.
/// Asking the reader whether the whole growing form is finished at every new
/// line takes time proportional to its square, and this one would not finish
/// within the deadline.
#[test]
fn one_large_form_on_standard_input_is_read_in_linear_time() {
    let dir = tempfile::tempdir().unwrap();
    let lines = 40_000;
    let mut program = String::from("(import (scheme base) (scheme write))\n(define text \"\n");
    for _ in 0..lines {
        program.push_str("x\n");
    }
    program.push_str("\")\n(display (string-length text))\n");
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(&program));
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, (2 * lines + 1).to_string(), "{backend:?}");
    }
}
