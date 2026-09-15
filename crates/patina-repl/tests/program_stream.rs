//! A program on standard input runs as it arrives and shares that input with
//! its own reads, and a session whose input ends inside a form reports that
//! form rather than dropping it (#333).
//!
//! These run the binary: the claims are about a pipe, a process's exit status,
//! and when a program's side effects happen. Every case runs on both backends.

mod common;

use common::{BOTH_BACKENDS, run_patina, run_with_deadline, spawn_patina};
use std::path::Path;
use std::time::{Duration, Instant};

/// The start of the parser's message for input that ends inside a form.
const DIAGNOSTIC: &str = "Unexpected end of input inside the datum beginning at";

/// Whether `path` exists within ten seconds.
fn appears(path: &Path) -> bool {
    let waiting = Instant::now();
    while !path.exists() && waiting.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}

/// A form runs before the input after it has been written. Each stage ends a
/// form that creates a file, and the writer waits for the file with the pipe
/// still open, which never ends against a runner that reads to the end of its
/// input first, or that holds a finished form back.
#[test]
fn a_program_on_standard_input_runs_each_form_as_it_arrives() {
    let create = |name: &str| {
        format!("(call-with-output-file \"{name}\" (lambda (port) (write-char #\\x port)))")
    };
    let stages = [
        // A form on a line of its own.
        (format!("{}\n", create("one")), "one"),
        // A finished form sharing its line with the start of one still open.
        (format!("{} (define later\n", create("two")), "two"),
        // A form whose last line is shorter than the lines before it, and
        // holds a `|symbol|` right after `,@`, which the lexer ends without
        // a delimiter.
        (
            format!(
                "  1)\n(begin ;{}\n  {}\n  '(,@|a |))\n",
                "x".repeat(120),
                create("three")
            ),
            "three",
        ),
    ];
    for backend in BOTH_BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let mut patina = spawn_patina(dir.path(), backend);
        patina.write("(import (scheme base) (scheme file))\n");
        let mut arrived = Vec::new();
        for (stage, name) in &stages {
            patina.write(stage);
            arrived.push((name, appears(&dir.path().join(name))));
        }
        let (_, stderr, ok) = patina.finish();
        for (name, arrived) in arrived {
            assert!(
                arrived,
                "{backend:?}: the form creating {name:?} had not run 10 s after it was written\n{stderr}"
            );
        }
        assert!(ok, "{backend:?}: {stderr}");
    }
}

/// Positions are the program's own: an error on a later line still names that
/// line and quotes it.
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

/// An error located in a form read before the one that fails is reported as a
/// file reports it: placed, quoted, and with its macro expansion.
#[test]
fn an_error_in_a_form_read_earlier_is_reported_as_a_file_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme base))\n\
                   (define-syntax my-car (syntax-rules () ((_ x) (car x))))\n\
                   (define (f) (my-car 5))\n\
                   (f)\n";
    std::fs::write(dir.path().join("p.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("p.scm");
        let (_, from_file, _) = run_patina(dir.path(), &args);
        let (_, from_stdin, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(!ok, "{backend:?}");
        assert_eq!(
            from_stdin,
            from_file.replace("p.scm", "<stdin>"),
            "{backend:?}"
        );
    }
    let (_, stderr, _) = run_with_deadline(dir.path(), &[], Some(program));
    assert!(
        stderr.contains("3 | (define (f) (my-car 5))")
            && stderr.contains("macro expansion: my-car"),
        "{stderr}"
    );
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

/// The program's text and its own input are one stream, as in chibi and
/// Gauche: a read from standard input continues right after the form being
/// run, and the program carries on after whatever the read took.
#[test]
fn a_read_inside_the_program_continues_right_after_its_form() {
    let dir = tempfile::tempdir().unwrap();
    for (program, expected) in [
        (
            "(import (scheme base) (scheme write))\n(display (read-line)) and the rest\n(newline)\n",
            " and the rest\n",
        ),
        (
            "(import (scheme base) (scheme write))\n(write (read-char))x\n(newline)\n",
            "#\\x\n",
        ),
        // A datum read from the next line leaves the rest of that line, which
        // is program text again.
        (
            "(import (scheme base) (scheme read) (scheme write))\n(write (read))\n(1 2) (display \"after\")\n",
            "(1 2)after",
        ),
    ] {
        for backend in BOTH_BACKENDS {
            let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
            assert!(ok, "{backend:?} {program:?}: {stderr}");
            assert_eq!(stdout, expected, "{backend:?} {program:?}");
        }
    }
}

/// A bad token after a finished form is reported after the form has run, from
/// a file as from standard input, as chibi and Gauche report it.
#[test]
fn a_bad_token_after_a_form_does_not_keep_the_form_from_running() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme base) (scheme write))\n(display 0)\n(display 1)\n#\\bogus\n";
    std::fs::write(dir.path().join("p.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("p.scm");
        for (stdout, stderr, ok) in [
            run_patina(dir.path(), &args),
            run_with_deadline(dir.path(), backend, Some(program)),
        ] {
            assert!(!ok, "{backend:?}");
            assert_eq!(stdout, "01", "{backend:?}: {stderr}");
            assert!(
                stderr.contains("Invalid character literal"),
                "{backend:?}: {stderr}"
            );
        }
    }
}

/// A byte order mark is dropped at the start of a program only; after the
/// first line it is a character, from standard input as in a file.
#[test]
fn a_byte_order_mark_after_the_first_line_is_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme base) (scheme write))\n(define \u{feff}#\\ 1)\n\
                   \u{feff}#\\(\ndisplay \"after\")\n(display \"end\")\n";
    std::fs::write(dir.path().join("p.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("p.scm");
        for (stdout, stderr, ok) in [
            run_patina(dir.path(), &args),
            run_with_deadline(dir.path(), backend, Some(program)),
        ] {
            assert!(ok, "{backend:?}: {stderr}");
            assert_eq!(stdout, "afterend", "{backend:?}");
        }
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

/// The VM session's `(vm-compile …)` shortcut takes only finished input, so a
/// session cut off inside one is reported like any unfinished form.
#[test]
fn a_session_cut_off_inside_vm_compile_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    for input in ["(vm-compile 42", "(vm-compile (+ 1 2)"] {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), &["-i"], Some(input));
        assert!(!ok, "{input:?}: {stdout}");
        assert!(stderr.contains(DIAGNOSTIC), "{input:?}: {stderr}");
        assert!(stderr.contains("<repl>:1:1"), "{input:?}: {stderr}");
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

/// `#!fold-case` holds for the rest of a program, not only for the form it was
/// read with, from standard input as through a file.
#[test]
fn fold_case_holds_for_the_rest_of_a_program_on_standard_input() {
    let dir = tempfile::tempdir().unwrap();
    let program = "#!fold-case\n(IMPORT (SCHEME BASE) (SCHEME WRITE))\n\
                   (DEFINE GREETING \"hi\")\n(DISPLAY GREETING)\n";
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, "hi", "{backend:?}");
    }
}

/// A form spanning many lines is read in time proportional to its size.
/// Asking the parser whether it is finished at every new line takes time
/// proportional to its square, which would not finish within the deadline:
/// the lines here are inside a string, or follow a prefix still waiting for
/// its datum.
#[test]
fn one_large_form_on_standard_input_is_read_in_linear_time() {
    let dir = tempfile::tempdir().unwrap();
    let lines = 40_000;
    let import = "(import (scheme base) (scheme write))\n";
    let string = format!(
        "{import}(define text \"\n{}\")\n(display (string-length text))\n",
        "x\n".repeat(lines)
    );
    let commented = format!(
        "{import}#;\n{}(display \"dropped\")\n(display \"kept\")\n",
        "; nothing\n".repeat(lines)
    );
    let quoted = format!("{import}'\n{}x\n(display \"done\")\n", "\n".repeat(lines));
    for (program, expected) in [
        (&string, (2 * lines + 1).to_string()),
        (&commented, "kept".to_string()),
        (&quoted, "done".to_string()),
    ] {
        for backend in BOTH_BACKENDS {
            let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
            assert!(ok, "{backend:?}: {stderr}");
            assert_eq!(stdout, expected, "{backend:?}");
        }
    }
}
