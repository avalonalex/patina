//! Tests for the high-level Interpreter API
//!
//! These tests verify the public API provided by the main `patina` crate

use patina_interpreter::{Interpreter, Value};

#[test]
fn test_interpreter_basic_arithmetic() {
    let interp = Interpreter::new();
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    assert!(matches!(result, Value::Integer(6)));
}

#[test]
fn test_interpreter_define_and_use() {
    let interp = Interpreter::new();
    interp.eval_str("(define x 42)").unwrap();
    let result = interp.eval_str("x").unwrap();
    assert!(matches!(result, Value::Integer(42)));
}

#[test]
fn test_eval_program() {
    let interp = Interpreter::new();
    let result = interp
        .eval_program(
            r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#,
        )
        .unwrap();
    assert!(matches!(result, Value::Integer(30)));
}

#[test]
fn test_macro_when() {
    let interp = Interpreter::new();

    // Define the when macro
    let define_result = interp.eval_str(
        r#"
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))
"#,
    );

    if let Err(e) = define_result {
        panic!("Failed to define when macro: {}", e);
    }

    // Test single body
    let result = interp.eval_str("(when #t 42)");
    match &result {
        Ok(val) => println!("when macro result: {}", val),
        Err(e) => panic!("when macro expansion error: {}", e),
    }
    let result = result.unwrap();
    assert!(matches!(result, Value::Integer(42)));

    // Test multiple body forms
    let result = interp.eval_str("(when #t 1 2 3)").unwrap();
    assert!(matches!(result, Value::Integer(3)));

    // Test false condition
    let result = interp.eval_str("(when #f 42)").unwrap();
    assert!(matches!(result, Value::Unspecified));
}

#[test]
fn test_macro_unless() {
    let interp = Interpreter::new();

    // Define the unless macro
    interp
        .eval_str(
            r#"
(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))
"#,
        )
        .unwrap();

    // Test with false condition (should execute)
    let result = interp.eval_str("(unless #f 42)").unwrap();
    assert!(matches!(result, Value::Integer(42)));

    // Test with true condition (should not execute)
    let result = interp.eval_str("(unless #t 42)").unwrap();
    assert!(matches!(result, Value::Unspecified));
}

#[test]
fn test_gcd_with_let_values() {
    let interp = Interpreter::new();
    let result = interp
        .eval_program(
            r#"
            (define (quotient-and-remainder a b)
              (values (quotient a b) (remainder a b)))

            (define (gcd a b)
              (if (= b 0)
                  a
                  (let-values (((q r) (quotient-and-remainder a b)))
                    (gcd b r))))

            (gcd 48 18)
        "#,
        )
        .unwrap();

    assert_eq!(format!("{}", result), "6");
}
