//! Tests for let-syntax and letrec-syntax special forms
//!
//! # Important Note on Macro Recursion
//!
//! **WARNING**: Do NOT use `letrec-syntax` to implement runtime recursion!
//!
//! Macros operate on **syntax** at **compile time**, not values at runtime.
//! A pattern like `((even? 0) #t)` only matches the literal syntax `(even? 0)`,
//! not expressions that evaluate to 0.
//!
//! **This will cause infinite macro expansion:**
//! ```scheme
//! (letrec-syntax
//!   ((even? (syntax-rules ()
//!            ((even? 0) #t)                    ; Only matches literal (even? 0)
//!            ((even? n) (odd? (- n 1))))))     ; Infinite expansion!
//!   (even? 4))  ; Expands forever: (even? 4) → (odd? (- 4 1)) → (even? (- (- 4 1) 1)) → ...
//! ```
//!
//! **Use regular recursive functions instead:**
//! ```scheme
//! (define (even? n)
//!   (if (= n 0) #t (odd? (- n 1))))
//! ```
//!
//! `letrec-syntax` is useful when macros need to reference each other in their
//! expansions, but each expansion should make progress toward termination.

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
fn test_let_syntax_simple() {
    let code = r#"
        (let-syntax ((when (syntax-rules ()
                             ((when test body ...)
                              (if test (begin body ...))))))
          (when #t 42))
    "#;
    assert_eq!(eval(code).unwrap(), "42");
}

#[test]
fn test_let_syntax_multiple_macros() {
    let code = r#"
        (let-syntax ((inc (syntax-rules () ((inc x) (+ x 1))))
                     (dec (syntax-rules () ((dec x) (- x 1)))))
          (list (inc 5) (dec 5)))
    "#;
    assert_eq!(eval(code).unwrap(), "(6 4)");
}

#[test]
fn test_let_syntax_scope() {
    // Macros defined in let-syntax should not be visible outside
    let code = r#"
        (define result
          (let-syntax ((local-macro (syntax-rules ()
                                      ((local-macro x) (* x 2)))))
            (local-macro 5)))
        result
    "#;
    assert_eq!(eval_program(code).unwrap(), "10");

    // This should fail because local-macro is not in scope
    let code_fail = r#"
        (let-syntax ((local-macro (syntax-rules ()
                                    ((local-macro x) (* x 2)))))
          (local-macro 5))
        (local-macro 10)
    "#;
    assert!(eval_program(code_fail).is_err());
}

#[test]
fn test_letrec_syntax_recursive() {
    // Recursive macro that expands my-or
    let code = r#"
        (letrec-syntax ((my-or (syntax-rules ()
                                 ((my-or) #f)
                                 ((my-or e) e)
                                 ((my-or e1 e2 ...)
                                  (let ((temp e1))
                                    (if temp temp (my-or e2 ...)))))))
          (my-or #f #f 42 #f))
    "#;
    assert_eq!(eval(code).unwrap(), "42");
}

#[test]
fn test_letrec_syntax_mutual_recursion() {
    // Demonstrate that letrec-syntax allows macros to reference each other
    // Note: This is a simple demo - real mutual recursion in macros is complex
    let code = r#"
        (letrec-syntax
          ((macro-a (syntax-rules ()
                      ((macro-a x) (macro-b x))))
           (macro-b (syntax-rules ()
                      ((macro-b x) (* x 2)))))
          (macro-a 21))
    "#;
    assert_eq!(eval(code).unwrap(), "42");
}

#[test]
fn test_let_syntax_body_sequence() {
    // Body can have multiple expressions
    let code = r#"
        (let-syntax ((inc (syntax-rules () ((inc x) (+ x 1)))))
          (inc 1)
          (inc 2)
          (inc 3))
    "#;
    assert_eq!(eval(code).unwrap(), "4");
}

#[test]
fn test_letrec_syntax_single_expression() {
    // Works with single expression too
    let code = r#"
        (letrec-syntax ((double (syntax-rules ()
                                  ((double x) (* x 2)))))
          (double 21))
    "#;
    assert_eq!(eval(code).unwrap(), "42");
}

#[test]
fn test_let_syntax_nested() {
    // Nested let-syntax
    let code = r#"
        (let-syntax ((outer (syntax-rules ()
                              ((outer x) (* x 2)))))
          (let-syntax ((inner (syntax-rules ()
                                ((inner y) (+ y 1)))))
            (outer (inner 5))))
    "#;
    assert_eq!(eval(code).unwrap(), "12");
}

#[test]
fn test_let_syntax_empty_body_error() {
    // Empty body should be an error
    let code = r#"
        (let-syntax ((foo (syntax-rules () ((foo x) x)))))
    "#;
    assert!(eval(code).is_err());
}

#[test]
fn test_let_syntax_invalid_binding_error() {
    // Binding must be (name transformer)
    let code = r#"
        (let-syntax ((foo)) 42)
    "#;
    assert!(eval(code).is_err());
}

#[test]
fn test_let_syntax_non_symbol_name_error() {
    // Name must be a symbol
    let code = r#"
        (let-syntax ((123 (syntax-rules () ((123 x) x)))) 42)
    "#;
    assert!(eval(code).is_err());
}

#[test]
fn test_let_syntax_lexical_scoping() {
    // Test case from chibi-scheme r7rs-tests.scm
    // This tests that free variables in macro templates capture the
    // lexical environment at macro definition time, not at use site.
    //
    // The macro 'm' is defined when x='outer is in scope.
    // When m is used, x='inner is in scope at the use site.
    // The macro should expand to 'outer (definition-time binding), not 'inner.
    let code = r#"
        (let ((x 'outer))
          (let-syntax ((m (syntax-rules () ((m) x))))
            (let ((x 'inner))
              (m))))
    "#;
    assert_eq!(eval(code).unwrap(), "outer");
}

#[test]
fn test_let_syntax_lexical_scoping_multiple_vars() {
    // Similar test but with multiple free variables
    let code = r#"
        (let ((x 'outer-x)
              (y 'outer-y))
          (let-syntax ((m (syntax-rules ()
                            ((m) (list x y)))))
            (let ((x 'inner-x)
                  (y 'inner-y))
              (m))))
    "#;
    assert_eq!(eval(code).unwrap(), "(outer-x outer-y)");
}

#[test]
fn test_let_syntax_nested_lexical_scoping() {
    // Nested let-syntax with lexical scoping
    let code = r#"
        (let ((x 'outer))
          (let-syntax ((m1 (syntax-rules () ((m1) x))))
            (let ((x 'middle))
              (let-syntax ((m2 (syntax-rules () ((m2) x))))
                (let ((x 'inner))
                  (list (m1) (m2)))))))
    "#;
    assert_eq!(eval(code).unwrap(), "(outer middle)");
}
