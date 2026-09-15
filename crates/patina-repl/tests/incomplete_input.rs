//! Input that ends inside a datum is a diagnosed failure, not a quiet success
//! (#329). A script cut short by a bad copy or an editor crash used to run
//! every complete form before the cut, drop the unfinished one, and exit 0;
//! `-p '42 (+ 1'` printed 42.
//!
//! These run the binary because the exit status is part of the claim, and
//! because `-p` and the two script paths have their own read loops in
//! `patina-repl/src/main.rs`. Every case runs on both backends.

mod common;

use common::{expect_failure_on_both_backends, run_both_backends, run_patina};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The start of the parser's message; the position that follows it is what
/// a reader of a long file needs, since the end of the file is where the
/// reader stopped, not where the problem is.
const DIAGNOSTIC: &str = "Unexpected end of input inside the datum beginning at";

const BOTH_BACKENDS: [&[&str]; 2] = [&[], &["--tree-walker"]];

#[test]
fn p_rejects_an_expression_cut_short_inside_a_datum() {
    let dir = tempfile::tempdir().unwrap();
    for expr in ["(+ 1", "'", "#(1 2", "#u8(1", "(1 .", "1 2 #;"] {
        expect_failure_on_both_backends(dir.path(), &["-p", expr], |stderr| {
            assert!(stderr.contains(DIAGNOSTIC), "-p {expr:?}: {stderr}");
        });
    }
}

#[test]
fn p_rejects_a_malformed_suffix_after_a_complete_form() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["-p", "42 (+ 1"]);
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "{args:?} succeeded: {stdout}");
        assert_eq!(
            stdout, "",
            "a `-p` that fails to read prints no value: {args:?}"
        );
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("line 1, column 4"),
            "{args:?}: {stderr}"
        );
    }
}

/// What the forms before the cut did is kept, not rolled back: they ran, so
/// what they wrote stands. Only the value of the failed `-p` is withheld.
#[test]
fn output_written_before_the_cut_survives_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&[
            "-p",
            r#"(import (scheme base) (scheme write)) (display "hi") (+ 1"#,
        ]);
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "{args:?} succeeded");
        assert_eq!(stdout.trim(), "hi", "{args:?}");
        assert!(stderr.contains(DIAGNOSTIC), "{args:?}: {stderr}");
    }
}

/// Resilient mode (a script path containing `test`) reports evaluation
/// errors and carries on, so those leave the status alone — but a file it
/// could not read to the end never ran in full, and must not report success.
#[test]
fn a_test_script_fails_on_a_read_error_and_not_on_an_evaluation_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cut_test.scm"),
        "(import (scheme base) (scheme write))\n(display \"ran\")\n(define y (+ 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("eval_test.scm"),
        "(import (scheme base) (scheme write))\n(no-such-procedure)\n(display \"after\")\n",
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("cut_test.scm");
        let (_, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "a truncated suite must not report success: {args:?}");
        assert!(stderr.contains(DIAGNOSTIC), "{args:?}: {stderr}");

        let mut args = backend.to_vec();
        args.push("eval_test.scm");
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(ok, "{args:?} must still succeed: {stderr}");
        assert_eq!(stdout.trim(), "after", "{args:?}");
    }
}

#[test]
fn p_ends_cleanly_after_trailing_comments() {
    let dir = tempfile::tempdir().unwrap();
    run_both_backends(dir.path(), &["-p", "42 ; done"], "42");
    run_both_backends(dir.path(), &["-p", "42 #;(dropped) #| block |#"], "42");
}

#[test]
fn a_script_cut_short_runs_the_forms_before_the_cut_and_then_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cut.scm"),
        "(import (scheme base) (scheme write))\n(display \"ran\")\n(newline)\n(define y\n  (+ 1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("only-open.scm"), "(").unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("cut.scm");
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "{args:?} succeeded: {stdout}");
        assert_eq!(stdout.trim(), "ran", "{args:?}");
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("line 4, column 1"),
            "{args:?}: {stderr}"
        );

        let mut args = backend.to_vec();
        args.push("only-open.scm");
        let (stdout, stderr, ok) = run_patina(dir.path(), &args);
        assert!(!ok, "{args:?} succeeded: {stdout}");
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("line 1, column 1"),
            "{args:?}: {stderr}"
        );
    }
}

/// Script paths containing `test` run in the resilient mode, which reports
/// each error and goes on, and exits 0 by design. A cut must still be
/// reported there, and a stray paren reported once: the VM's runner used to
/// print the same parse error without end, because the parser leaves the
/// offending token in place.
#[test]
fn a_test_script_cut_short_or_with_a_stray_paren_reports_it_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cut_test.scm"),
        "(import (scheme base) (scheme write))\n(display \"ran\")\n(define y (+ 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("stray_test.scm"),
        "(import (scheme base) (scheme write))\n(display \"ran\"))\n(display \"after\")\n",
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("cut_test.scm");
        let (stdout, stderr) = run_with_deadline(dir.path(), &args);
        assert_eq!(stdout.trim(), "ran", "{args:?}");
        assert_eq!(stderr.matches(DIAGNOSTIC).count(), 1, "{args:?}: {stderr}");

        let mut args = backend.to_vec();
        args.push("stray_test.scm");
        let (stdout, stderr) = run_with_deadline(dir.path(), &args);
        assert_eq!(stdout.trim(), "ran", "{args:?}");
        assert_eq!(
            stderr.matches("Unexpected token").count(),
            1,
            "{args:?}: {stderr}"
        );
    }
}

/// `patina < program.scm` means what `patina program.scm` means.
///
/// A line editor reading a pipe cannot report what it never gets to keep: at
/// end of input it drops a partly-read form, so a cut-short program printed
/// its prefix, said "Goodbye!", and exited 0. chibi, Gauche and Chez all
/// report this; being the only one that stays quiet is the defect, and a
/// silent success is the part a shell script cannot see.
#[test]
fn a_program_on_standard_input_is_diagnosed_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cut =
        "(import (scheme base) (scheme write))\n(display \"ran\")\n(newline)\n(define y (+ 1\n";
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_patina_stdin(dir.path(), backend, cut);
        assert!(
            !ok,
            "{backend:?} succeeded on a cut-short program: {stdout}"
        );
        assert_eq!(stdout.trim(), "ran", "{backend:?}");
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("<stdin>:4:1"),
            "{backend:?}: {stderr}"
        );
        assert!(
            !stdout.contains("Goodbye!") && !stderr.contains("Goodbye!"),
            "a program is not a session: {backend:?}"
        );
    }
}

/// The same path must still run what is not broken, and must keep reporting
/// evaluation failures through the status.
#[test]
fn a_whole_program_on_standard_input_runs_and_reports_its_outcome() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_patina_stdin(
            dir.path(),
            backend,
            "(import (scheme base) (scheme write))\n(display (+ 40 2))\n",
        );
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout.trim(), "42", "{backend:?}");

        let (_, _, ok) = run_patina_stdin(
            dir.path(),
            backend,
            "(import (scheme base))\n(no-such-procedure)\n",
        );
        assert!(!ok, "an unbound variable must fail: {backend:?}");
    }
}

/// Run the binary with `input` on standard input, as a shell redirect does.
fn run_patina_stdin(cwd: &Path, args: &[&str], input: &str) -> (String, String, bool) {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_patina"))
        .args(args)
        .env_remove("PATINA_ALLOW_R6RS")
        .env_remove("PATINA_LIBRARY_PATH")
        .env_remove("PATINA_HOME")
        .env_remove("PATINA_ISOLATED_LIBRARIES")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn patina binary");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.as_bytes())
        .expect("write program to stdin");
    let out = child.wait_with_output().expect("wait on patina");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

/// Run the binary and collect its output, killing it if it is still running
/// after ten seconds: a runner looping on a parse error would otherwise hang
/// the suite. Both pipes are drained on their own threads while the child
/// runs, so a flood of output fails on what it printed rather than filling a
/// pipe buffer and stalling until the deadline.
fn run_with_deadline(cwd: &Path, args: &[&str]) -> (String, String) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_patina"))
        .args(args)
        .env_remove("PATINA_ALLOW_R6RS")
        .env_remove("PATINA_LIBRARY_PATH")
        .env_remove("PATINA_HOME")
        .env_remove("PATINA_ISOLATED_LIBRARIES")
        .current_dir(cwd)
        // As `common::run_patina` does through `output()`: a helper that ever
        // reaches the REPL must not take the test runner's terminal.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn patina binary");

    let mut out = child.stdout.take().expect("stdout pipe");
    let mut err = child.stderr.take().expect("stderr pipe");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    let exited = loop {
        if child.try_wait().expect("wait on patina").is_some() {
            break true;
        }
        if started.elapsed() > Duration::from_secs(10) {
            child.kill().ok();
            child.wait().ok();
            break false;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let stdout = String::from_utf8_lossy(&out_reader.join().expect("stdout reader")).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().expect("stderr reader")).into_owned();
    assert!(
        exited,
        "patina {args:?} was still running after 10 s; stderr began:\n{}",
        stderr.lines().take(3).collect::<Vec<_>>().join("\n")
    );
    (stdout, stderr)
}
