//! Tests for import special form

use patina_interpreter::Interpreter;

#[test]
fn test_import_scheme_base() {
    let interp = Interpreter::new();

    let result = interp.eval_program(
        r#"
        (import (scheme base))
        (+ 1 2 3)
    "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "6");
}

#[test]
fn test_import_only() {
    let interp = Interpreter::new();

    let result = interp.eval_program(
        r#"
        (import (only (scheme base) + - *))
        (* 2 3 4)
    "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "24");
}

#[test]
fn test_import_prefix() {
    let interp = Interpreter::new();

    let result = interp.eval_program(
        r#"
        (import (prefix (scheme base) s:))
        (s:+ 10 20)
    "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "30");
}
