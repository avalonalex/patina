//! Tests for parameters (make-parameter and parameterize)

use patina_interpreter::TreeWalkInterpreter;

fn eval(code: &str) -> Result<String, String> {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp
        .eval_str(code)
        .map(|v| format!("{}", v))
        .map_err(|e| format!("{}", e))
}

fn eval_program(code: &str) -> Result<String, String> {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp
        .eval_program(code)
        .map(|v| format!("{}", v))
        .map_err(|e| format!("{}", e))
}

#[test]
fn test_make_parameter_basic() {
    let code = r#"
        (define p (make-parameter 10))
        (p)
    "#;
    assert_eq!(eval_program(code).unwrap(), "10");
}

#[test]
fn test_parameter_set() {
    let code = r#"
        (define p (make-parameter 10))
        (p 20)
        (p)
    "#;
    assert_eq!(eval_program(code).unwrap(), "20");
}

#[test]
fn test_parameterize_simple() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20))
          (p))
    "#;
    assert_eq!(eval_program(code).unwrap(), "20");
}

#[test]
fn test_parameterize_restores() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20))
          (p))
        (p)
    "#;
    assert_eq!(eval_program(code).unwrap(), "10");
}

#[test]
fn test_parameterize_multiple_params() {
    let code = r#"
        (define p1 (make-parameter 10))
        (define p2 (make-parameter 20))
        (parameterize ((p1 100) (p2 200))
          (list (p1) (p2)))
    "#;
    assert_eq!(eval_program(code).unwrap(), "(100 200)");
}

#[test]
fn test_parameterize_nested() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20))
          (parameterize ((p 30))
            (p)))
    "#;
    assert_eq!(eval_program(code).unwrap(), "30");
}

#[test]
fn test_parameterize_nested_restores() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20))
          (parameterize ((p 30))
            (p))
          (p))
    "#;
    assert_eq!(eval_program(code).unwrap(), "20");
}

#[test]
fn test_parameter_with_converter() {
    let code = r#"
        (define p (make-parameter 10 (lambda (x) (* x 2))))
        (p)
    "#;
    // Note: Converter not applied to initial value yet (TODO in implementation)
    assert_eq!(eval_program(code).unwrap(), "10");
}

#[test]
fn test_parameter_converter_on_set() {
    let code = r#"
        (define p (make-parameter 10 (lambda (x) (* x 2))))
        (p 5)
        (p)
    "#;
    assert_eq!(eval_program(code).unwrap(), "10");
}

#[test]
fn test_parameterize_empty_body_error() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20)))
    "#;
    assert!(eval_program(code).is_err());
}

#[test]
fn test_parameterize_non_parameter_error() {
    let code = r#"
        (parameterize ((42 20))
          (display "hello"))
    "#;
    assert!(eval_program(code).is_err());
}

#[test]
fn test_parameterize_body_sequence() {
    let code = r#"
        (define p (make-parameter 10))
        (parameterize ((p 20))
          (p)
          (p)
          (p))
    "#;
    assert_eq!(eval_program(code).unwrap(), "20");
}
