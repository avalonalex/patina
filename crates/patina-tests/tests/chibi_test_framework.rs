//! Tests for the (chibi test) framework.
//!
//! These were `#[ignore]`d as "requires the module system", which stopped being
//! true long before the ignore was removed. They now exercise the real upstream
//! `(chibi test)` that Patina bundles, rather than the hand-written subset that
//! used to stand in for it.

use patina_interpreter::TreeWalkInterpreter;

#[test]
fn test_chibi_test_framework_loads() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (scheme base) (chibi test))
        (test-begin "test-suite")
        (test 3 (+ 1 2))
        (test-end)
    "#,
    );

    // Should succeed without errors
    assert!(result.is_ok(), "Failed to run chibi test: {:?}", result);
}

#[test]
fn test_chibi_test_basic_functionality() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (scheme base) (chibi test))

        (test-begin "arithmetic")
        (test 6 (+ 1 2 3))
        (test 10 (* 2 5))
        (test-end)

        #t
    "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap(), patina_core::TaggedValue::TRUE);
}
