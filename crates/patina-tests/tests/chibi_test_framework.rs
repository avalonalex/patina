//! Tests for (chibi test) framework
//!
//! Note: These tests are temporarily commented out as they require the module
//! system (import) which is not yet implemented.

use patina_interpreter::TreeWalkInterpreter;

#[test]
#[ignore = "Requires module system (import)"]
fn test_chibi_test_framework_loads() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Test that we can import (chibi test) - now a Rust library
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
#[ignore = "Requires module system (import)"]
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
    assert_eq!(result.unwrap().to_string(), "#t");
}
