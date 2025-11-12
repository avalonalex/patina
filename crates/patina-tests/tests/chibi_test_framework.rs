//! Tests for (chibi test) framework

use patina_interpreter::Interpreter;

#[test]
fn test_chibi_test_framework_loads() {
    let interp = Interpreter::new();

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
fn test_chibi_test_basic_functionality() {
    let interp = Interpreter::new();

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
