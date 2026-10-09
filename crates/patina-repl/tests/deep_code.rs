//! Code nested past the depth limit is refused, reported, and fails the run;
//! it never aborts the process (#617). Run the real binary, on the stack the
//! system gives its main thread, so that an overflow fails a test instead of
//! the test runner.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline_status};
use std::fs;
use tempfile::TempDir;

#[test]
fn code_nested_past_the_limit_is_reported_and_fails_the_run() {
    const DEPTH: usize = 100_000;
    let temp = TempDir::new().unwrap();
    let program = format!(
        "(import (scheme base) (scheme write))\n(write {}a{})\n",
        "(let ((a 1)) ".repeat(DEPTH),
        ")".repeat(DEPTH)
    );
    fs::write(temp.path().join("deep.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["--isolated-libraries", "deep.scm"]);
        let (stdout, stderr, status) = run_with_deadline_status(temp.path(), &args, None);
        assert_eq!(status.code(), Some(1), "{backend:?}: {status}\n{stderr}");
        assert!(
            stderr.contains("nested too deeply"),
            "{backend:?}: {stderr}"
        );
        assert_eq!(stdout, "", "{backend:?}: nothing runs");
    }
}
