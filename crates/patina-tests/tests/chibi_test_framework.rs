//! Tests for the (chibi test) framework.
//!
//! These were `#[ignore]`d as "requires the module system", which stopped being
//! true long before the ignore was removed. They now exercise the real upstream
//! `(chibi test)`, rather than the hand-written subset that used to stand in
//! for it.
//!
//! Patina supplies that library from `test-lib/` rather than bundling it, so
//! the interpreters come from `common`, which puts that root on the search
//! path — this lane's spelling of the `-A test-lib` the shell lanes pass. The
//! shared helpers also run both backends and require them to agree, which
//! matters here: loading this library is macro expansion plus library
//! resolution, which is where the two diverge.
//!
//! **Every program below ends in `(test-failure-count)`.** `test-end` prints a
//! tally and returns normally — only `test-exit` exits non-zero — so a test
//! that checks nothing but "the program ran and returned #t" stays green with
//! every assertion inside it failing. Asserting the count is what makes these
//! tests of the framework rather than of the loader.

mod common;
use common::assert_program_eval_to;

/// The framework loads, and its `test` form both runs and passes.
#[test]
fn test_chibi_test_framework_loads() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (chibi test))
        (test-begin "test-suite")
        (test 3 (+ 1 2))
        (test-end)
        (test-failure-count)
    "#,
        "0",
    );
}

#[test]
fn test_chibi_test_basic_functionality() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (chibi test))
        (test-begin "arithmetic")
        (test 6 (+ 1 2 3))
        (test 10 (* 2 5))
        (test-end)
        (test-failure-count)
    "#,
        "0",
    );
}

/// The counter the two tests above rely on is itself load-bearing: if it
/// stayed 0 through a real failure they would pass vacuously.
#[test]
fn test_a_failing_assertion_is_counted() {
    assert_program_eval_to(
        r#"
        (import (scheme base) (chibi test))
        (test-begin "deliberate")
        (test 1 2)
        (test 3 3)
        (test-end)
        (test-failure-count)
    "#,
        "1",
    );
}
