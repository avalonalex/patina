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
