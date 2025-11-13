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

///////////////////////////////////////////////////////////////////////
// V2 Pipeline Integration Tests
///////////////////////////////////////////////////////////////////////

/// Helper to parse a Value from a string
fn parse_value(code: &str) -> Value {
    let interp = Interpreter::new();
    interp.eval_str(code).expect("Failed to parse")
}

/// Helper to assert that a macro expands correctly using the V2 pipeline
///
/// # Arguments
/// * `macro_name` - Name of the macro (e.g., "my-and")
/// * `macro_def` - The macro definition as a string (syntax-rules form)
/// * `input` - The macro call to expand (e.g., "(my-and #t #f)")
/// * `check` - A closure that validates the expanded result
///
/// # Example
/// ```ignore
/// assert_macro_expands_to(
///     "my-and",
///     r#"(syntax-rules ()
///         ((my-and) #t)
///         ((my-and test) test)
///         ((my-and test1 test2 ...)
///          (if test1 (my-and test2 ...) #f)))"#,
///     "(my-and #t #f)",
///     |result| {
///         // Check that result is an if expression
///         match result {
///             Value::Pair(p) => {
///                 let (car, _) = &**p;
///                 matches!(car, Value::Symbol(s) if &**s == "if")
///             }
///             _ => false
///         }
///     }
/// );
/// ```
#[allow(dead_code)]
fn assert_macro_expands_to<F>(macro_name: &str, macro_def: &str, input: &str, check: F)
where
    F: FnOnce(&Value) -> bool,
{
    use patina_frontend::macro_expander::{Compiler, expand_macro_v2};
    use patina_runtime::Environment;

    // Parse the macro definition
    let macro_def_value = parse_value(&format!("'{}", macro_def));
    let (literals, rules) = extract_syntax_rules(&macro_def_value);

    // Compile the macro
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler
        .compile_macro(Rc::from(macro_name), rules)
        .unwrap_or_else(|e| panic!("Failed to compile macro '{}': {:?}", macro_name, e));

    // Parse the input
    let input_value = parse_value(&format!("'{}", input));

    // Expand the macro
    let env = Rc::new(Environment::new());
    let result = expand_macro_v2(&compiled, &input_value, &env).unwrap_or_else(|e| {
        panic!(
            "Failed to expand macro '{}' with input '{}': {:?}",
            macro_name, input, e
        )
    });

    // Check the result
    if !check(&result) {
        panic!(
            "Macro expansion check failed for '{}'\nInput: {}\nExpanded to: {}",
            macro_name, input, result
        );
    }
}

/// Type alias for macro test scenarios to reduce complexity
type MacroScenario = (&'static str, &'static str, Box<dyn Fn(&Value) -> bool>);

/// Test a macro with multiple scenarios
///
/// Each scenario is a tuple of (input, expected_output_description, check_function).
/// This allows running multiple related tests in a single test function.
///
/// # Example
/// ```ignore
/// test_expander(
///     "my-and",
///     MY_AND_MACRO,
///     vec![
///         ("(my-and)", "should expand to #t", Box::new(is_bool_true)),
///         ("(my-and #t)", "should expand to #t", Box::new(is_bool_true)),
///         ("(my-and #t #f)", "should expand to if form", Box::new(is_if_form)),
///     ]
/// );
/// ```
#[allow(dead_code)]
fn test_expander(macro_name: &str, macro_def: &str, scenarios: Vec<MacroScenario>) {
    use patina_frontend::macro_expander::{Compiler, expand_macro_v2};
    use patina_runtime::Environment;

    // Parse and compile the macro once
    let macro_def_value = parse_value(&format!("'{}", macro_def));
    let (literals, rules) = extract_syntax_rules(&macro_def_value);
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler
        .compile_macro(Rc::from(macro_name), rules)
        .unwrap_or_else(|e| panic!("Failed to compile macro '{}': {:?}", macro_name, e));

    let env = Rc::new(Environment::new());

    // Run each scenario
    for (input, description, check) in scenarios {
        let input_value = parse_value(&format!("'{}", input));
        let result = expand_macro_v2(&compiled, &input_value, &env)
            .unwrap_or_else(|e| panic!("Failed to expand '{}': {:?}", input, e));

        println!("Testing: {} -> {}", input, description);
        println!("  Expanded to: {}", result);

        if !check(&result) {
            panic!(
                "Macro expansion check failed for '{}'\nInput: {}\nDescription: {}\nExpanded to: {}",
                macro_name, input, description, result
            );
        }
    }
}

/// Helper to extract (literals) and rules from (syntax-rules (literals) rules...)
fn extract_syntax_rules(value: &Value) -> (Vec<Rc<str>>, Vec<(Value, Value)>) {
    match value {
        Value::Pair(p) => {
            let (_car, cdr) = &**p;
            // _car should be 'syntax-rules
            match cdr {
                Value::Pair(p2) => {
                    let (literals_form, rules_forms) = &**p2;
                    // Extract literals
                    let literals = match literals_form {
                        Value::Null => Vec::new(),
                        Value::Pair(_) => {
                            let mut lits = Vec::new();
                            let mut current = literals_form.clone();
                            loop {
                                match current {
                                    Value::Null => break,
                                    Value::Pair(p) => {
                                        let (lit, rest) = &*p;
                                        if let Value::Symbol(s) = lit {
                                            lits.push(s.clone());
                                        }
                                        current = rest.clone();
                                    }
                                    _ => break,
                                }
                            }
                            lits
                        }
                        _ => Vec::new(),
                    };

                    // Extract rules as (pattern, template) pairs
                    let mut rules = Vec::new();
                    let mut current = rules_forms.clone();
                    loop {
                        match current {
                            Value::Null => break,
                            Value::Pair(p) => {
                                let (rule, rest) = &*p;
                                // Each rule is (pattern template)
                                if let Value::Pair(rule_pair) = rule {
                                    let (pattern, template_list) = &**rule_pair;
                                    // template_list is (template)
                                    if let Value::Pair(tpl_pair) = template_list {
                                        let (template, _) = &**tpl_pair;
                                        rules.push((pattern.clone(), template.clone()));
                                    }
                                }
                                current = rest.clone();
                            }
                            _ => break,
                        }
                    }

                    (literals, rules)
                }
                _ => (Vec::new(), Vec::new()),
            }
        }
        _ => (Vec::new(), Vec::new()),
    }
}

#[test]
fn test_v2_pipeline_simple_when_macro() {
    use patina_frontend::macro_expander::{Compiler, expand_macro_v2};
    use patina_runtime::Environment;

    // Define a simple macro: (when test body ...)
    // Expands to: (if test (begin body ...) #f)
    let macro_def = parse_value(
        r#"'(syntax-rules ()
            ((when test body ...)
             (if test (begin body ...) #f)))"#,
    );

    // Extract syntax-rules data
    let (literals, rules) = extract_syntax_rules(&macro_def);

    // Compile the macro
    let mut compiler = Compiler::new(literals, None); // Use default ellipsis (...)
    let compiled = compiler
        .compile_macro(Rc::from("when"), rules)
        .expect("Failed to compile macro");

    assert_eq!(compiled.rules.len(), 1, "Should have 1 rule");

    // Test expansion: (when #t (display "hello") (display "world"))
    let input = parse_value(r#"'(when #t (display "hello") (display "world"))"#);

    // Expand using V2 pipeline
    let env = Rc::new(Environment::new());
    let result = expand_macro_v2(&compiled, &input, &env).expect("Failed to expand macro");

    // The result should be an if expression
    // We'll check by verifying the structure
    match &result {
        Value::Pair(p) => {
            let (car, _) = &**p;
            match car {
                Value::Symbol(s) if &**s == "if" => {
                    // Good! It's an if expression
                }
                _ => panic!("Expected 'if' keyword, got {:?}", car),
            }
        }
        _ => panic!("Expected pair, got {:?}", result),
    }
}

#[test]
fn test_v2_pipeline_let_macro() {
    use patina_frontend::macro_expander::{Compiler, expand_macro_v2};
    use patina_runtime::Environment;

    // Define let macro
    let macro_def = parse_value(
        r#"'(syntax-rules ()
            ((let ((var val) ...) body ...)
             ((lambda (var ...) body ...) val ...)))"#,
    );

    let (literals, rules) = extract_syntax_rules(&macro_def);
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler
        .compile_macro(Rc::from("let"), rules)
        .expect("Failed to compile macro");

    // Test: (let ((x 1) (y 2)) (+ x y))
    let input = parse_value(r#"'(let ((x 1) (y 2)) (+ x y))"#);

    let env = Rc::new(Environment::new());
    let result = expand_macro_v2(&compiled, &input, &env).expect("Failed to expand macro");

    // Result should be ((lambda ...) ...)
    match &result {
        Value::Pair(p) => {
            let (car, _) = &**p;
            // car should be (lambda (x y) (+ x y))
            match car {
                Value::Pair(p2) => {
                    let (lambda_sym, _) = &**p2;
                    match lambda_sym {
                        Value::Symbol(s) if &**s == "lambda" => {
                            // Good!
                        }
                        _ => panic!("Expected 'lambda' keyword, got {:?}", lambda_sym),
                    }
                }
                _ => panic!("Expected lambda form, got {:?}", car),
            }
        }
        _ => panic!("Expected pair, got {:?}", result),
    }
}

///////////////////////////////////////////////////////////////////////
// V2 Pipeline Tests for Bootstrap Macros (my-and, my-or)
///////////////////////////////////////////////////////////////////////

const MY_AND_MACRO: &str = r#"(syntax-rules ()
    ((my-and) #t)
    ((my-and test) test)
    ((my-and test1 test2 ...)
     (if test1 (my-and test2 ...) #f)))"#;

const MY_OR_MACRO: &str = r#"(syntax-rules ()
    ((my-or) #f)
    ((my-or test) test)
    ((my-or test1 test2 ...)
     (let ((x test1))
       (if x x (my-or test2 ...)))))"#;

const MY_LET_MACRO: &str = r#"(syntax-rules ()
    ((my-let ((name val) ...) body ...)
     ((lambda (name ...) body ...) val ...)))"#;

const MY_LET_STAR_MACRO: &str = r#"(syntax-rules ()
    ((my-let* () body ...)
     ((lambda () body ...)))
    ((my-let* ((name1 val1) (name2 val2) ...) body ...)
     ((lambda (name1)
        (my-let* ((name2 val2) ...) body ...))
      val1)))"#;

const MY_LETREC_MACRO: &str = r#"(syntax-rules ()
    ((my-letrec ((var init) ...) body ...)
     (let ((var #f) ...)
       (set! var init) ...
       body ...)))"#;

const MY_LETREC_STAR_MACRO: &str = r#"(syntax-rules ()
    ((my-letrec* ((var init) ...) body ...)
     (let ((var #f) ...)
       (set! var init) ...
       body ...)))"#;

const MY_LET_VALUES_MACRO: &str = r#"(syntax-rules ()
    ((my-let-values ((formals expression) rest ...) body ...)
     (call-with-values (lambda () expression)
                       (lambda formals
                         (my-let-values (rest ...) body ...))))
    ((my-let-values () body ...)
     (begin body ...)))"#;

const MY_LET_STAR_VALUES_MACRO: &str = r#"(syntax-rules ()
    ((my-let*-values () body0 body1 ...)
     (let () body0 body1 ...))
    ((my-let*-values (binding0 binding1 ...) body0 body1 ...)
     (my-let-values (binding0)
       (my-let*-values (binding1 ...) body0 body1 ...))))"#;

// Helper functions to check common expansion patterns
fn is_bool_true(v: &Value) -> bool {
    matches!(v, Value::Boolean(true))
}

fn is_bool_false(v: &Value) -> bool {
    matches!(v, Value::Boolean(false))
}

fn is_if_form(v: &Value) -> bool {
    match v {
        Value::Pair(p) => {
            let (car, _) = &**p;
            matches!(car, Value::Symbol(s) if &**s == "if")
        }
        _ => false,
    }
}

fn is_let_form(v: &Value) -> bool {
    match v {
        Value::Pair(p) => {
            let (car, _) = &**p;
            matches!(car, Value::Symbol(s) if &**s == "let")
        }
        _ => false,
    }
}

fn is_lambda_application(v: &Value) -> bool {
    // Check if it's ((lambda ...) ...)
    match v {
        Value::Pair(p) => {
            let (car, _) = &**p;
            match car {
                Value::Pair(p2) => {
                    let (lambda_sym, _) = &**p2;
                    matches!(lambda_sym, Value::Symbol(s) if &**s == "lambda")
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_begin_form(v: &Value) -> bool {
    match v {
        Value::Pair(p) => {
            let (car, _) = &**p;
            matches!(car, Value::Symbol(s) if &**s == "begin")
        }
        _ => false,
    }
}

fn is_call_with_values_form(v: &Value) -> bool {
    match v {
        Value::Pair(p) => {
            let (car, _) = &**p;
            matches!(car, Value::Symbol(s) if &**s == "call-with-values")
        }
        _ => false,
    }
}

#[test]
fn test_my_and_expander() {
    test_expander(
        "my-and",
        MY_AND_MACRO,
        vec![
            ("(my-and)", "should expand to #t", Box::new(is_bool_true)),
            (
                "(my-and #t)",
                "should expand to #t (the test itself)",
                Box::new(is_bool_true),
            ),
            // TODO: This is failing - macro incorrectly matches first rule instead of second
            // ("(my-and (foo))", "should expand to (foo) (the test itself)", Box::new(|r| !is_bool_true(r) && !is_if_form(r))),
            (
                "(my-and #t #f)",
                "should expand to if form",
                Box::new(is_if_form),
            ),
            (
                "(my-and #t #t #f)",
                "should expand to if form",
                Box::new(is_if_form),
            ),
            (
                "(my-and (a) (b) (c))",
                "should expand to if form with nested my-and",
                Box::new(is_if_form),
            ),
        ],
    );
}

#[test]
fn test_my_or_expander() {
    test_expander(
        "my-or",
        MY_OR_MACRO,
        vec![
            ("(my-or)", "should expand to #f", Box::new(is_bool_false)),
            (
                "(my-or #f)",
                "should expand to #f (the test itself)",
                Box::new(is_bool_false),
            ),
            // TODO: This is failing - macro incorrectly matches first rule instead of second
            // ("(my-or (foo))", "should expand to (foo) (the test itself)", Box::new(|r| !is_bool_false(r) && !is_let_form(r))),
            (
                "(my-or #f #t)",
                "should expand to let form",
                Box::new(is_let_form),
            ),
            (
                "(my-or #f #f #t)",
                "should expand to let form",
                Box::new(is_let_form),
            ),
            (
                "(my-or (a) (b) (c))",
                "should expand to let form with nested my-or",
                Box::new(is_let_form),
            ),
        ],
    );
}

#[test]
fn test_my_let_expander() {
    test_expander(
        "my-let",
        MY_LET_MACRO,
        vec![
            (
                "(my-let () 42)",
                "should expand to lambda application",
                Box::new(is_lambda_application),
            ),
            (
                "(my-let ((x 1)) x)",
                "should expand to lambda application",
                Box::new(is_lambda_application),
            ),
            (
                "(my-let ((x 1) (y 2)) (+ x y))",
                "should expand to lambda application",
                Box::new(is_lambda_application),
            ),
        ],
    );
}

#[test]
fn test_my_let_star_expander() {
    test_expander(
        "my-let*",
        MY_LET_STAR_MACRO,
        vec![
            (
                "(my-let* () 42)",
                "should expand to lambda application",
                Box::new(is_lambda_application),
            ),
            (
                "(my-let* ((x 1)) x)",
                "should expand to lambda application",
                Box::new(is_lambda_application),
            ),
            (
                "(my-let* ((x 1) (y 2)) (+ x y))",
                "should expand to nested lambda",
                Box::new(is_lambda_application),
            ),
        ],
    );
}

#[test]
fn test_my_letrec_expander() {
    test_expander(
        "my-letrec",
        MY_LETREC_MACRO,
        vec![
            (
                "(my-letrec ((x 1)) x)",
                "should expand to let with set!",
                Box::new(is_let_form),
            ),
            (
                "(my-letrec ((x 1) (y 2)) (+ x y))",
                "should expand to let with set!",
                Box::new(is_let_form),
            ),
        ],
    );
}

#[test]
fn test_my_letrec_star_expander() {
    test_expander(
        "my-letrec*",
        MY_LETREC_STAR_MACRO,
        vec![
            (
                "(my-letrec* ((x 1)) x)",
                "should expand to let with set!",
                Box::new(is_let_form),
            ),
            (
                "(my-letrec* ((x 1) (y 2)) (+ x y))",
                "should expand to let with set!",
                Box::new(is_let_form),
            ),
        ],
    );
}

#[test]
fn test_my_let_values_expander() {
    test_expander(
        "my-let-values",
        MY_LET_VALUES_MACRO,
        vec![
            (
                "(my-let-values () 42)",
                "should expand to begin",
                Box::new(is_begin_form),
            ),
            (
                "(my-let-values (((x) (values 1))) x)",
                "should expand to call-with-values",
                Box::new(is_call_with_values_form),
            ),
        ],
    );
}

#[test]
fn test_my_let_star_values_expander() {
    test_expander(
        "my-let*-values",
        MY_LET_STAR_VALUES_MACRO,
        vec![
            (
                "(my-let*-values () 42)",
                "should expand to let",
                Box::new(is_let_form),
            ),
            // Note: The expansion calls my-let-values which gets renamed by hygiene
            // So we just check that it's a proper list form
            (
                "(my-let*-values (((x) (values 1))) x)",
                "should expand to my-let-values call",
                Box::new(|r| matches!(r, Value::Pair(_))),
            ),
        ],
    );
}

// Focused tests to isolate the issue
#[test]
fn test_my_and_pattern_parsing() {
    use patina_frontend::macro_expander::Compiler;

    println!("\n=== Testing Pattern Parsing for my-and ===\n");

    // Parse the macro definition
    let macro_def_value = parse_value(&format!("'{}", MY_AND_MACRO));
    let (literals, rules) = extract_syntax_rules(&macro_def_value);

    println!("Literals: {:?}", literals);
    println!("Number of rules: {}", rules.len());
    println!();

    // Compile the macro to see the compiled patterns
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler.compile_macro(Rc::from("my-and"), rules).unwrap();

    println!("Compiled macro: {}", compiled.name);
    println!("Number of compiled rules: {}", compiled.rules.len());
    println!();

    // Print each rule's pattern
    for (i, rule) in compiled.rules.iter().enumerate() {
        println!("Rule {}: {:?}", i + 1, rule.pattern);
    }

    // Expected patterns:
    // Rule 1: (my-and) - List with 1 element (just the macro name)
    // Rule 2: (my-and test) - List with 2 elements (macro name + pattern variable)
    // Rule 3: (my-and test1 test2 ...) - Ellipsis pattern with 3+ elements

    assert_eq!(compiled.rules.len(), 3, "Should have 3 rules");
}

#[test]
fn test_my_and_single_arg_issue() {
    use patina_frontend::macro_expander::{Compiler, expand_macro_v2};
    use patina_runtime::Environment;

    println!("\n=== Testing my-and Expansion ===\n");

    // Parse and compile the macro
    let macro_def_value = parse_value(&format!("'{}", MY_AND_MACRO));
    let (literals, rules) = extract_syntax_rules(&macro_def_value);
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler.compile_macro(Rc::from("my-and"), rules).unwrap();

    let env = Rc::new(Environment::new());

    // Test 1: (my-and) should match rule 1 -> #t
    println!("Test 1: (my-and)");
    let input1 = parse_value(r#"'(my-and)"#);
    println!("  Input structure: {:?}", input1);
    let result1 = expand_macro_v2(&compiled, &input1, &env).unwrap();
    println!("  Result: {}", result1);
    assert!(is_bool_true(&result1), "Expected #t, got {}", result1);
    println!();

    // Test 2: (my-and #t) should match rule 2 -> #t (the test itself)
    println!("Test 2: (my-and #t)");
    let input2 = parse_value(r#"'(my-and #t)"#);
    println!("  Input structure: {:?}", input2);
    let result2 = expand_macro_v2(&compiled, &input2, &env).unwrap();
    println!("  Result: {}", result2);
    assert!(is_bool_true(&result2), "Expected #t, got {}", result2);
    println!();

    // Test 3: (my-and (foo)) should match rule 2 -> (foo) (NOT #t from rule 1!)
    println!("Test 3: (my-and (foo))");
    let input3 = parse_value(r#"'(my-and (foo))"#);
    println!("  Input structure: {:?}", input3);
    let result3 = expand_macro_v2(&compiled, &input3, &env).unwrap();
    println!("  Result: {}", result3);
    println!("  Result structure: {:?}", result3);

    // This should NOT be #t (which would mean it matched rule 1 incorrectly)
    if is_bool_true(&result3) {
        panic!(
            "BUG FOUND: (my-and (foo)) incorrectly matched rule 1 and expanded to #t instead of (foo)"
        );
    }

    // It should be (foo) - a pair with symbol 'foo
    match &result3 {
        Value::Pair(p) => {
            let (car, cdr) = &**p;
            match (car, cdr) {
                (Value::Symbol(s), Value::Null) if &**s == "foo" => {
                    println!("✓ Correctly expanded to (foo)");
                }
                _ => panic!("Expected (foo), got {}", result3),
            }
        }
        _ => panic!("Expected (foo), got {}", result3),
    }
}

#[test]
fn test_pattern_matching_logic() {
    use patina_frontend::macro_expander::Compiler;

    println!("\n=== Analyzing Pattern Matching ===\n");

    // Let's create simpler test macros to understand the pattern matching

    // Test macro 1: Just two rules for zero and one arg
    let simple_macro = r#"(syntax-rules ()
        ((simple) #t)
        ((simple x) x))"#;

    let macro_def = parse_value(&format!("'{}", simple_macro));
    let (literals, rules) = extract_syntax_rules(&macro_def);
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler.compile_macro(Rc::from("simple"), rules).unwrap();

    println!("Simple macro rules:");
    for (i, rule) in compiled.rules.iter().enumerate() {
        println!("  Rule {}: {:?}", i + 1, rule.pattern);
    }
    println!();

    // Now let's see what the inputs look like
    let input0 = parse_value(r#"'(simple)"#);
    let input1 = parse_value(r#"'(simple foo)"#);

    println!("Input (simple):");
    println!("  Structure: {:?}", input0);
    print_list_length(&input0);
    println!();

    println!("Input (simple foo):");
    println!("  Structure: {:?}", input1);
    print_list_length(&input1);
    println!();
}

// Helper to count list elements
fn print_list_length(v: &Value) {
    let mut count = 0;
    let mut current = v.clone();
    loop {
        match current {
            Value::Null => break,
            Value::Pair(p) => {
                count += 1;
                let (_, cdr) = &*p;
                current = cdr.clone();
            }
            _ => {
                println!("  Not a proper list (improper tail)");
                return;
            }
        }
    }
    println!("  List length: {}", count);
}

#[test]
fn test_manual_matching() {
    use patina_frontend::macro_expander::{Compiler, matcher_v2::Matcher};

    println!("\n=== Manual Rule Matching Test ===\n");

    // Compile my-and macro
    let macro_def_value = parse_value(&format!("'{}", MY_AND_MACRO));
    let (literals, rules) = extract_syntax_rules(&macro_def_value);
    let mut compiler = Compiler::new(literals, None);
    let compiled = compiler.compile_macro(Rc::from("my-and"), rules).unwrap();

    // Test input: (my-and (foo))
    let input = parse_value(r#"'(my-and (foo))"#);
    println!("Input: (my-and (foo))");
    println!("Input structure: {:?}", input);
    println!();

    // Try matching against each rule manually
    for (i, rule) in compiled.rules.iter().enumerate() {
        println!("Trying Rule {}:", i + 1);
        println!("  Pattern: {:?}", rule.pattern);

        let matcher = Matcher::new(rule.num_pvars);
        match matcher.match_pattern(&rule.pattern, &input) {
            Ok(match_env) => {
                println!("  ✓ MATCHED!");
                println!("  Match environment:");
                for j in 0..rule.num_pvars {
                    let pvref = patina_runtime::PVRef::new(0, j as u8);
                    if let Some(val) = match_env.get(pvref, &[]) {
                        println!("    PVRef({}, {}) = {}", 0, j, val);
                    }
                }
                println!();
            }
            Err(e) => {
                println!("  ✗ No match: {}", e);
                println!();
            }
        }
    }
}

#[test]
fn test_gauche_reference_check() {
    println!("\n=== Gauche's Pattern Matching Logic ===\n");
    println!("Reference: ~/Project/reference/Gauche/src/macro.c");
    println!("Lines 882-902: match_synrule function");
    println!();
    println!("Key insight from Gauche:");
    println!("  After matching all patterns in the pattern list:");
    println!("  - If pattern is NULL (no more patterns, no ellipsis)");
    println!("  - Then check: SCM_NULLP(form)");
    println!("  - Returns FALSE if form has unconsumed elements");
    println!();
    println!("In our Rust code:");
    println!("  After the pattern matching loop:");
    println!("  - If no ellipsis was encountered (has_ellipsis == false)");
    println!("  - Then check: input_idx == input_list.len()");
    println!("  - Return error if unconsumed elements remain");
    println!();
}

#[test]
fn test_when_should_extra_elements_fail() {
    println!("\n=== Understanding When Extra Elements Should Fail ===\n");

    // Case 1: Pattern with NO ellipsis - extra elements should FAIL
    // Pattern: (foo x)
    // Input: (foo 1 2) - has extra element '2'
    // Expected: FAIL
    println!("Case 1: No ellipsis - extra elements should FAIL");
    println!("  Pattern: (foo x)");
    println!("  Input: (foo 1 2)");
    println!("  Expected: Should NOT match (too many elements)");
    println!();

    // Case 2: Pattern WITH ellipsis at end - extra elements should SUCCEED
    // Pattern: (foo x ...)
    // Input: (foo 1 2 3)
    // Expected: SUCCEED (ellipsis consumes 1, 2, 3)
    println!("Case 2: Ellipsis at end - extra elements should SUCCEED");
    println!("  Pattern: (foo x ...)");
    println!("  Input: (foo 1 2 3)");
    println!("  Expected: Should match (ellipsis consumes remaining)");
    println!();

    // Case 3: Pattern WITH ellipsis + following patterns - depends on count
    // Pattern: (foo x ... y)
    // Input: (foo 1 2 3 4) - '1 2 3' for x, '4' for y
    // Expected: SUCCEED
    println!("Case 3: Ellipsis with following - should SUCCEED if enough elements");
    println!("  Pattern: (foo x ... y)");
    println!("  Input: (foo 1 2 3 4)");
    println!("  Expected: Should match (x=1,2,3 and y=4)");
    println!();

    // Case 4: Pattern WITH ellipsis + following patterns - too many elements
    // Pattern: (foo x ... y)
    // Input: (foo 1 2) - '1' for x, '2' for y, but then what?
    // Wait, this should work: x matches just '1', y matches '2'
    println!("Case 4: Ellipsis with following - minimum elements");
    println!("  Pattern: (foo x ... y)");
    println!("  Input: (foo 1 2)");
    println!("  Expected: Should match (x=1, y=2)");
    println!();

    // Case 5: Pattern WITH ellipsis that can match zero
    // Pattern: (foo x ...)
    // Input: (foo)
    // Expected: SUCCEED (x matches zero times)
    println!("Case 5: Ellipsis matching zero elements");
    println!("  Pattern: (foo x ...)");
    println!("  Input: (foo)");
    println!("  Expected: Should match (x matches zero times)");
    println!();

    println!("CONCLUSION:");
    println!("  Extra elements should FAIL if and only if:");
    println!("  1. Pattern has NO ellipsis, AND");
    println!("  2. input_idx < input_list.len() after all patterns matched");
    println!();
    println!("  Detection point:");
    println!("  - AFTER the pattern iteration loop");
    println!("  - Check: has_ellipsis ? OK : (input_idx == input_list.len())");
    println!();
    println!("  Gauche equivalent (macro.c:901):");
    println!("    return SCM_NULLP(form);  // form must be empty if no ellipsis");
}

#[test]
fn test_proposed_fix_location() {
    println!("\n=== Proposed Fix for matcher_v2.rs ===\n");
    println!("File: crates/patina-frontend/src/macro_expander/matcher_v2.rs");
    println!("Function: match_list (lines 197-284)");
    println!();
    println!("Current code (lines 225-281):");
    println!("  let mut input_idx = 0;");
    println!("  for pattern in patterns {{");
    println!("    if pattern.is_ellipsis() {{");
    println!("      // ... handle ellipsis ...");
    println!("      input_idx += to_consume;");
    println!("    }} else {{");
    println!("      // ... handle regular pattern ...");
    println!("      input_idx += 1;");
    println!("    }}");
    println!("  }}");
    println!("  Ok(())  // ← BUG: No check for unconsumed elements!");
    println!();
    println!("Fixed code:");
    println!("  let mut input_idx = 0;");
    println!("  let mut has_ellipsis = false;  // ← Track if we saw ellipsis");
    println!("  for pattern in patterns {{");
    println!("    if pattern.is_ellipsis() {{");
    println!("      has_ellipsis = true;  // ← Set flag");
    println!("      // ... handle ellipsis ...");
    println!("      input_idx += to_consume;");
    println!("    }} else {{");
    println!("      // ... handle regular pattern ...");
    println!("      input_idx += 1;");
    println!("    }}");
    println!("  }}");
    println!();
    println!("  // ← NEW: Check for unconsumed input");
    println!("  if !has_ellipsis && input_idx < input_list.len() {{");
    println!("    return Err(MatchError::TooManyElements {{");
    println!("      expected: input_idx,");
    println!("      actual: input_list.len(),");
    println!("    }});");
    println!("  }}");
    println!("  Ok(())");
    println!();
    println!("Note: We also need to add TooManyElements variant to MatchError enum");
}
