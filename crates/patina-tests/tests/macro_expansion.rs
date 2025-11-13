//! Macro expansion test framework
//!
//! This module provides a test framework for asserting macro expansions.
//! It allows testing macro expansion at various levels:
//! 1. Pattern parsing
//! 2. Pattern matching
//! 3. Template expansion
//! 4. Full macro expansion with hygiene
//!
//! The goal is to make the macro system testable and transparent, which will
//! be crucial for debugging and IDE integration (e.g., "show expanded macro").
//!
//! # Design Inspiration
//!
//! The PVREF-based macro system is inspired by Gauche Scheme's macro.c
//! implementation by Shiro Kawai. Gauche's design uses:
//! - PVREF encoding (level + index) for pattern variables
//! - Tree-based storage for nested ellipsis matches
//! - Two-phase compilation (compile pattern once, expand many times)
//!
//! Reference: ~/Project/reference/Gauche/src/macro.c

use patina_frontend::macro_expander::{Pattern, Template, parse_pattern, parse_template};
use patina_interpreter::Interpreter;
use patina_runtime::Value;
use std::rc::Rc;

/// Helper to create a list from values
fn make_list(items: Vec<Value>) -> Value {
    items
        .into_iter()
        .rev()
        .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
}

/// Helper to parse Scheme code into a Value
fn parse(code: &str) -> Value {
    let interp = Interpreter::new();
    // Use the parser from the interpreter
    // For now, we'll use eval_str which parses internally
    // TODO: expose parser directly in interpreter API
    interp
        .eval_str(&format!("'({})", code))
        .expect("Failed to parse code")
}

/// Assert that a pattern parses to the expected Pattern structure
///
/// # Example
/// ```ignore
/// assert_pattern_parses(
///     "(when test body ...)",
///     Pattern::Ellipsis {
///         before: vec![
///             Pattern::Variable("when".into()),
///             Pattern::Variable("test".into()),
///         ],
///         repeated: Box::new(Pattern::Variable("body".into())),
///         after: vec![],
///     }
/// );
/// ```
#[allow(dead_code)]
fn assert_pattern_parses(code: &str, expected: Pattern) {
    let value = parse(code);
    let pattern = parse_pattern(&value).expect("Failed to parse pattern");

    // For now, compare debug representations
    // TODO: implement PartialEq for Pattern
    assert_eq!(
        format!("{:?}", pattern),
        format!("{:?}", expected),
        "Pattern mismatch for: {}",
        code
    );
}

/// Assert that a template parses to the expected Template structure
#[allow(dead_code)]
fn assert_template_parses(code: &str, expected: Template) {
    let value = parse(code);
    let template = parse_template(&value).expect("Failed to parse template");

    assert_eq!(
        format!("{:?}", template),
        format!("{:?}", expected),
        "Template mismatch for: {}",
        code
    );
}

/// Assert that a macro expands from input to expected output
///
/// # Example
/// ```ignore
/// assert_expands_to(
///     // Macro definition
///     "(define-syntax when
///        (syntax-rules ()
///          ((when test body ...)
///           (if test (begin body ...)))))",
///     // Input
///     "(when #t (display \"hello\") (newline))",
///     // Expected expansion
///     "(if #t (begin (display \"hello\") (newline)))"
/// );
/// ```
#[allow(dead_code)]
fn assert_expands_to(macro_def: &str, input: &str, expected: &str) {
    let interp = Interpreter::new();

    // Define the macro
    interp.eval_str(macro_def).expect("Failed to define macro");

    // Expand the input (without evaluating)
    // TODO: add expand_only method to Interpreter
    // For now, we'll evaluate and compare the result
    let result = interp.eval_str(input).expect("Failed to expand/eval macro");
    let expected_val = interp.eval_str(expected).expect("Failed to eval expected");

    assert_eq!(
        format!("{}", result),
        format!("{}", expected_val),
        "Macro expansion mismatch.\nInput: {}\nExpected: {}\nGot: {}",
        input,
        expected,
        result
    );
}

/// Assert that a macro expands to a specific string representation
///
/// This is useful when you want to test the expansion itself, not the evaluation.
/// It compares the string representation of the expanded form.
#[allow(dead_code)]
fn assert_expands_to_string(macro_def: &str, input: &str, expected_str: &str) {
    let interp = Interpreter::new();

    // Define the macro
    interp.eval_str(macro_def).expect("Failed to define macro");

    // TODO: Implement macro expansion without evaluation
    // For now, we'll use quote to prevent evaluation
    let quoted_input = format!("'{}", input);
    let result = interp
        .eval_str(&quoted_input)
        .expect("Failed to expand macro");

    let result_str = format!("{}", result);
    assert_eq!(
        result_str.trim(),
        expected_str.trim(),
        "Macro expansion string mismatch"
    );
}

// =============================================================================
// Pattern Parsing Tests
// =============================================================================

#[test]
fn test_parse_simple_pattern() {
    let value = make_list(vec![
        Value::Symbol("when".into()),
        Value::Symbol("test".into()),
        Value::Symbol("body".into()),
    ]);

    let pattern = parse_pattern(&value).expect("Failed to parse");

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            assert!(matches!(&patterns[0], Pattern::Variable(s) if s.as_ref() == "when"));
            assert!(matches!(&patterns[1], Pattern::Variable(s) if s.as_ref() == "test"));
            assert!(matches!(&patterns[2], Pattern::Variable(s) if s.as_ref() == "body"));
        }
        _ => panic!("Expected Pattern::List"),
    }
}

#[test]
fn test_parse_pattern_with_ellipsis() {
    let value = make_list(vec![
        Value::Symbol("when".into()),
        Value::Symbol("test".into()),
        Value::Symbol("body".into()),
        Value::Symbol("...".into()),
    ]);

    let pattern = parse_pattern(&value).expect("Failed to parse");

    match pattern {
        Pattern::Ellipsis {
            before,
            repeated,
            after,
        } => {
            assert_eq!(before.len(), 2);
            assert!(matches!(&before[0], Pattern::Variable(s) if s.as_ref() == "when"));
            assert!(matches!(&before[1], Pattern::Variable(s) if s.as_ref() == "test"));
            assert!(matches!(*repeated, Pattern::Variable(s) if s.as_ref() == "body"));
            assert_eq!(after.len(), 0);
        }
        _ => panic!("Expected Pattern::Ellipsis, got {:?}", pattern),
    }
}

#[test]
fn test_parse_nested_ellipsis_pattern() {
    // Pattern: ((var init step ...) ...)
    // This is for the do macro
    let inner_list = make_list(vec![
        Value::Symbol("var".into()),
        Value::Symbol("init".into()),
        Value::Symbol("step".into()),
        Value::Symbol("...".into()),
    ]);

    let value = make_list(vec![inner_list, Value::Symbol("...".into())]);

    let pattern = parse_pattern(&value).expect("Failed to parse");

    match pattern {
        Pattern::Ellipsis {
            before,
            repeated,
            after,
        } => {
            assert_eq!(before.len(), 0);
            assert_eq!(after.len(), 0);

            // The repeated pattern should itself be an ellipsis
            match *repeated {
                Pattern::Ellipsis {
                    before: ref inner_before,
                    repeated: ref inner_repeated,
                    after: ref inner_after,
                } => {
                    assert_eq!(inner_before.len(), 2); // var, init
                    assert!(
                        matches!(**inner_repeated, Pattern::Variable(ref s) if s.as_ref() == "step")
                    );
                    assert_eq!(inner_after.len(), 0);
                }
                _ => panic!("Expected nested Ellipsis pattern, got {:?}", repeated),
            }
        }
        _ => panic!("Expected Pattern::Ellipsis, got {:?}", pattern),
    }
}

// =============================================================================
// Template Parsing Tests
// =============================================================================

#[test]
fn test_parse_simple_template() {
    let value = make_list(vec![
        Value::Symbol("if".into()),
        Value::Symbol("test".into()),
        Value::Symbol("body".into()),
    ]);

    let template = parse_template(&value).expect("Failed to parse");

    match template {
        Template::List(templates) => {
            assert_eq!(templates.len(), 3);
            assert!(matches!(&templates[0], Template::Variable(s) if s.as_ref() == "if"));
            assert!(matches!(&templates[1], Template::Variable(s) if s.as_ref() == "test"));
            assert!(matches!(&templates[2], Template::Variable(s) if s.as_ref() == "body"));
        }
        _ => panic!("Expected Template::List"),
    }
}

#[test]
fn test_parse_template_with_ellipsis() {
    let value = make_list(vec![
        Value::Symbol("begin".into()),
        Value::Symbol("body".into()),
        Value::Symbol("...".into()),
    ]);

    let template = parse_template(&value).expect("Failed to parse");

    match template {
        Template::Ellipsis {
            before,
            repeated,
            after,
        } => {
            assert_eq!(before.len(), 1);
            assert!(matches!(&before[0], Template::Variable(s) if s.as_ref() == "begin"));
            assert!(matches!(*repeated, Template::Variable(s) if s.as_ref() == "body"));
            assert_eq!(after.len(), 0);
        }
        _ => panic!("Expected Template::Ellipsis, got {:?}", template),
    }
}

#[test]
fn test_parse_ellipsis_escape() {
    // (... x) should produce literal ...
    let value = make_list(vec![Value::Symbol("...".into()), Value::Symbol("x".into())]);

    let template = parse_template(&value).expect("Failed to parse");

    match template {
        Template::EllipsisEscape(inner) => {
            assert!(matches!(*inner, Template::Variable(s) if s.as_ref() == "x"));
        }
        _ => panic!("Expected Template::EllipsisEscape, got {:?}", template),
    }
}

// =============================================================================
// Integration Tests - Simple Macros
// =============================================================================

#[test]
fn test_when_macro_simple() {
    let interp = Interpreter::new();

    // Define when macro
    interp
        .eval_str(
            r#"
            (define-syntax when
              (syntax-rules ()
                ((when test body ...)
                 (if test (begin body ...)))))
            "#,
        )
        .expect("Failed to define when");

    // Test expansion - with no body
    let result = interp.eval_str("(when #f)").expect("Failed to eval");
    assert_eq!(format!("{}", result), "#<unspecified>");

    // Test expansion - with single body
    let result = interp.eval_str("(when #t 42)").expect("Failed to eval");
    assert_eq!(format!("{}", result), "42");

    // Test expansion - with multiple bodies
    let result = interp
        .eval_str(
            r#"
            (when #t
              (define x 10)
              (+ x 5))
            "#,
        )
        .expect("Failed to eval");
    assert_eq!(format!("{}", result), "15");
}

#[test]
fn test_let_macro_simple() {
    let interp = Interpreter::new();

    // Test basic let
    let result = interp
        .eval_str("(let ((x 10) (y 20)) (+ x y))")
        .expect("Failed to eval");
    assert_eq!(format!("{}", result), "30");

    // Test let with no bindings
    let result = interp.eval_str("(let () 42)").expect("Failed to eval");
    assert_eq!(format!("{}", result), "42");
}

#[test]
#[ignore] // TODO: This test requires proper nested ellipsis support
fn test_do_macro_with_variable_steps() {
    let interp = Interpreter::new();

    // Test the problematic do case: some variables have steps, some don't
    let result = interp
        .eval_str(
            r#"
            (do ((i 0 (+ i 1))
                 (sum 0 (+ sum i)))
                ((> i 5) sum))
            "#,
        )
        .expect("Failed to eval do");

    assert_eq!(format!("{}", result), "15"); // 0+1+2+3+4+5 = 15
}

// =============================================================================
// Future Tests - For PVREF Implementation
// =============================================================================

#[test]
#[ignore] // TODO: Implement after PVREF system
fn test_double_ellipsis() {
    let interp = Interpreter::new();

    // Define a macro with double ellipsis
    // This should expand each sublist with its elements repeated
    interp
        .eval_str(
            r#"
            (define-syntax double-expand
              (syntax-rules ()
                ((double-expand ((a b ...) ...))
                 ((a b ... ...) ...))))
            "#,
        )
        .expect("Failed to define macro");

    let result = interp
        .eval_str("(double-expand ((1 2 3) (4 5)))")
        .expect("Failed to eval");

    // Expected: ((1 2 3 2 3) (4 5))
    // The b values (2 3) in first sublist appear twice due to ... ...
    // The b value (5) in second sublist appears twice as well
    println!("Result: {}", result);
}

#[test]
#[ignore] // TODO: Implement after PVREF system
fn test_nested_ellipsis_levels() {
    let interp = Interpreter::new();

    // Test proper level tracking in nested ellipsis
    interp
        .eval_str(
            r#"
            (define-syntax nested-test
              (syntax-rules ()
                ((nested-test ((a b) ...) ...)
                 (list (list a b) ... ...))))
            "#,
        )
        .expect("Failed to define macro");

    let result = interp
        .eval_str("(nested-test ((1 2) (3 4)) ((5 6)))")
        .expect("Failed to eval");

    // Should properly track that 'a' and 'b' are at level 2
    // and expand them correctly
    println!("Result: {}", result);
}

#[test]
#[ignore] // TODO: Requires hygiene testing infrastructure
fn test_hygiene_renaming() {
    let interp = Interpreter::new();

    // Test that hygiene properly renames introduced identifiers
    interp
        .eval_str(
            r#"
            (define-syntax swap
              (syntax-rules ()
                ((swap a b)
                 (let ((tmp a))
                   (set! a b)
                   (set! b tmp)))))
            "#,
        )
        .expect("Failed to define macro");

    // 'tmp' should be renamed hygienically and not conflict with user's tmp
    let result = interp
        .eval_str(
            r#"
            (let ((x 1) (y 2) (tmp 999))
              (swap x y)
              (list x y tmp))
            "#,
        )
        .expect("Failed to eval");

    assert_eq!(format!("{}", result), "(2 1 999)");
}
