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
            "an expression that fails to read prints nothing: {args:?}"
        );
        assert!(
            stderr.contains(DIAGNOSTIC) && stderr.contains("line 1, column 4"),
            "{args:?}: {stderr}"
        );
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

/// Run the binary and collect its output, killing it if it is still running
/// after ten seconds: a runner looping on a parse error would otherwise hang
/// the suite. Output must fit a pipe buffer, since it is read after exit.
fn run_with_deadline(cwd: &Path, args: &[&str]) -> (String, String) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_patina"))
        .args(args)
        .env_remove("PATINA_ALLOW_R6RS")
        .env_remove("PATINA_LIBRARY_PATH")
        .env_remove("PATINA_HOME")
        .env_remove("PATINA_ISOLATED_LIBRARIES")
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn patina binary");
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
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .ok();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .ok();
    assert!(
        exited,
        "patina {args:?} was still running after 10 s; stderr began:\n{}",
        stderr.lines().take(3).collect::<Vec<_>>().join("\n")
    );
    (stdout, stderr)
}
