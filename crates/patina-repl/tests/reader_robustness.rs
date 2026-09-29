//! Reader inputs whose failure mode is a process abort (#355). Run the real
//! binary so an overflowing reader fails a test instead of the test runner.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline_status};
use std::fs;
use tempfile::TempDir;

#[test]
fn million_element_lists_work_through_read_and_quoted_programs() {
    const LENGTH: usize = 1_000_000;
    let temp = TempDir::new().unwrap();
    let elements = "1 ".repeat(LENGTH);
    let list = format!("({elements})");
    let imports = "(import (scheme base) (scheme read) (scheme write) (scheme file))\n";
    fs::write(temp.path().join("list.dat"), &list).unwrap();
    fs::write(
        temp.path().join("file.scm"),
        format!("{imports}(define x (call-with-input-file \"list.dat\" read)) (write (length x))"),
    )
    .unwrap();
    fs::write(
        temp.path().join("string.scm"),
        format!("{imports}(define x (read (open-input-string \"{list}\"))) (write (length x))"),
    )
    .unwrap();
    fs::write(
        temp.path().join("quoted.scm"),
        format!("{imports}(define x '{list}) (write (length x))"),
    )
    .unwrap();

    // These force label patching to walk the long spine too: skipping only
    // label-free datums would leave the original stack overflow here.
    fs::write(
        temp.path().join("cycle.dat"),
        format!("#0=({elements}. #0#)"),
    )
    .unwrap();
    fs::write(
        temp.path().join("cycle.scm"),
        format!(
            "{imports}(define x (call-with-input-file \"cycle.dat\" read)) \
             (write (eq? x (list-tail x {LENGTH})))"
        ),
    )
    .unwrap();
    fs::write(temp.path().join("shared.dat"), format!("#(#0# #0={list})")).unwrap();
    fs::write(
        temp.path().join("shared.scm"),
        format!(
            "{imports}(define x (call-with-input-file \"shared.dat\" read)) \
             (write (and (= (length (vector-ref x 0)) {LENGTH}) \
                         (eq? (vector-ref x 0) (vector-ref x 1))))"
        ),
    )
    .unwrap();

    // Sequential to keep the memory cost to one million-element child at a
    // time. Each runs with the normal stack and GC settings on both backends.
    for (script, expected) in [
        ("file.scm", "1000000"),
        ("string.scm", "1000000"),
        ("quoted.scm", "1000000"),
        ("cycle.scm", "#t"),
        ("shared.scm", "#t"),
    ] {
        for backend in BOTH_BACKENDS {
            let mut args = backend.to_vec();
            args.extend_from_slice(&["--isolated-libraries", script]);
            let (stdout, stderr, status) = run_with_deadline_status(temp.path(), &args, None);
            assert!(status.success(), "{script} {backend:?}: {status}\n{stderr}");
            assert_eq!(stdout.trim(), expected, "{script} {backend:?}: {stderr}");
        }
    }
}

#[test]
fn million_level_datums_work_through_read_and_quoted_programs() {
    const DEPTH: usize = 1_000_000;
    let temp = TempDir::new().unwrap();
    let imports = "(import (scheme base) (scheme read) (scheme write) (scheme file))\n";
    for (name, opener, closer, step) in [
        ("list", "(", ")", "(car x)"),
        ("vector", "#(", ")", "(vector-ref x 0)"),
        ("quote", "'", "", "(car (cdr x))"),
    ] {
        let datum = format!("{}1{}", opener.repeat(DEPTH), closer.repeat(DEPTH));
        fs::write(temp.path().join("datum.dat"), &datum).unwrap();
        for (route, expression) in [
            (
                "file",
                "(call-with-input-file \"datum.dat\" read)".to_string(),
            ),
            ("string", format!("(read (open-input-string \"{datum}\"))")),
            ("program", format!("'{datum}")),
        ] {
            // Inspect the entire depth without asking the printer to traverse
            // it. Merely checking that reading returned would miss truncation.
            let source = format!(
                "{imports}(define x {expression})\n\
                 (write (let loop ((n {DEPTH}) (x x))\n\
                   (if (= n 0) x (loop (- n 1) {step}))))"
            );
            check_deep_input(&temp, &source, "1", &format!("{name}/{route}"));
            if route == "program" {
                // Standard input uses Reader's token replay and source-map
                // recording, whereas a script file is parsed from text.
                for backend in BOTH_BACKENDS {
                    let mut args = backend.to_vec();
                    args.push("--isolated-libraries");
                    let (stdout, stderr, status) =
                        run_with_deadline_status(temp.path(), &args, Some(&source));
                    assert!(
                        status.success(),
                        "{name}/stdin {backend:?}: {status}\n{stderr}"
                    );
                    assert_eq!(stdout.trim(), "1", "{name}/stdin {backend:?}: {stderr}");
                }
            }
        }
    }

    // Nested comments consume one datum apiece. With too few datums the
    // reader must raise a read error, even a million prefixes into the input.
    let comments = "#;".repeat(DEPTH);
    let complete = format!("{comments}{} 7", " 1".repeat(DEPTH));
    let incomplete = format!("{comments} 1 2");
    for (name, datum, expected) in [
        ("comments", complete, "7"),
        ("incomplete comments", incomplete, "read-error"),
    ] {
        fs::write(temp.path().join("datum.dat"), &datum).unwrap();
        for (route, expression) in [
            (
                "file",
                "(call-with-input-file \"datum.dat\" read)".to_string(),
            ),
            ("string", format!("(read (open-input-string \"{datum}\"))")),
        ] {
            let source = format!(
                "{imports}(guard (e ((read-error? e) (display \"read-error\")))\n\
                 (write {expression}))"
            );
            check_deep_input(&temp, &source, expected, &format!("{name}/{route}"));
        }
        if name == "comments" {
            check_deep_input(&temp, &format!("{imports}(write {datum})"), "7", name);
        } else {
            fs::write(temp.path().join("case.scm"), &datum).unwrap();
            for backend in BOTH_BACKENDS {
                let mut args = backend.to_vec();
                args.extend_from_slice(&["--isolated-libraries", "case.scm"]);
                let (_, stderr, status) = run_with_deadline_status(temp.path(), &args, None);
                assert_eq!(status.code(), Some(1), "{name} {backend:?}: {stderr}");
                assert!(
                    stderr.contains("Unexpected end of input inside the datum"),
                    "{stderr}"
                );
            }
        }
    }

    for separator in [" ", "\n"] {
        let datum = format!("{}MiXeD", format!("#!fold-case{separator}").repeat(DEPTH));
        fs::write(temp.path().join("datum.dat"), &datum).unwrap();
        for expression in [
            "(call-with-input-file \"datum.dat\" read)".to_string(),
            format!("(read (open-input-string \"{datum}\"))"),
            format!("'{datum}"),
        ] {
            check_deep_input(
                &temp,
                &format!("{imports}(write {expression})"),
                "mixed",
                "directives",
            );
        }
    }
}

fn check_deep_input(temp: &TempDir, source: &str, expected: &str, case: &str) {
    fs::write(temp.path().join("case.scm"), source).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["--isolated-libraries", "case.scm"]);
        let (stdout, stderr, status) = run_with_deadline_status(temp.path(), &args, None);
        assert!(status.success(), "{case} {backend:?}: {status}\n{stderr}");
        assert_eq!(stdout.trim(), expected, "{case} {backend:?}: {stderr}");
    }
}
