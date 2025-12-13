//! Unit tests for the pattern/template compiler

use crate::macro_expander::compiler::Compiler;
use crate::macro_expander::utils::WILDCARD;
use crate::macro_expander::{Pattern, Template};
use patina_runtime::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Helper to create a symbol
fn sym(s: &str) -> Value {
    Value::Symbol(s.into())
}

/// Helper to create a list
fn list(items: Vec<Value>) -> Value {
    items.into_iter().rev().fold(Value::Null, |acc, val| {
        Value::Pair(Rc::new(RefCell::new((val, acc))))
    })
}

#[test]
fn test_compile_simple_pattern() {
    // Pattern: (when test body)
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern_form = list(vec![sym("when"), sym("test"), sym("body")]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            assert!(matches!(&patterns[0], Pattern::Var(_)));
            assert!(matches!(&patterns[1], Pattern::Var(_)));
            assert!(matches!(&patterns[2], Pattern::Var(_)));
        }
        _ => panic!("Expected Pattern2::List"),
    }

    // Should have 3 pattern variables
    assert_eq!(compiler.pvar_count, 3);
}

#[test]
fn test_compile_pattern_with_ellipsis() {
    // Pattern: (when test body ...)
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern_form = list(vec![sym("when"), sym("test"), sym("body"), sym("...")]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            // First two are normal vars
            assert!(matches!(&patterns[0], Pattern::Var(_)));
            assert!(matches!(&patterns[1], Pattern::Var(_)));
            // Third is ellipsis
            match &patterns[2] {
                Pattern::Ellipsis {
                    subpattern,
                    level,
                    num_following,
                    vars,
                } => {
                    assert_eq!(*level, 1);
                    assert_eq!(*num_following, 0); // No items after ellipsis
                    assert_eq!(vars.len(), 1);
                    assert!(matches!(**subpattern, Pattern::Var(_)));
                }
                _ => panic!("Expected Pattern2::Ellipsis"),
            }
        }
        _ => panic!("Expected Pattern2::List"),
    }
}

#[test]
fn test_compile_pattern_ellipsis_with_following() {
    // Pattern: (do bindings ... (test result))
    // The ellipsis should have num_following = 1
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern_form = list(vec![
        sym("do"),
        sym("bindings"),
        sym("..."),
        list(vec![sym("test"), sym("result")]),
    ]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3); // do, bindings..., (test result)
            match &patterns[1] {
                Pattern::Ellipsis { num_following, .. } => {
                    assert_eq!(*num_following, 1); // One item follows
                }
                _ => panic!("Expected ellipsis"),
            }
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_compile_simple_template() {
    // First compile a pattern to establish variables
    let mut compiler = Compiler::new(vec![], Some("...".into()));
    let _pattern = compiler
        .compile_pattern(&list(vec![sym("when"), sym("test"), sym("body")]), 0)
        .unwrap();

    // Now compile template: (if test body)
    let template_form = list(vec![sym("if"), sym("test"), sym("body")]);
    let template = compiler.compile_template(&template_form, 0).unwrap();

    match template {
        Template::List(templates) => {
            assert_eq!(templates.len(), 3);
            // "if" is introduced symbol
            assert!(matches!(&templates[0], Template::Symbol(_)));
            // "test" and "body" are pattern variables
            assert!(matches!(&templates[1], Template::Var(_)));
            assert!(matches!(&templates[2], Template::Var(_)));
        }
        _ => panic!("Expected Template2::List"),
    }
}

#[test]
fn test_compile_template_with_ellipsis() {
    // Pattern: (begin body ...)
    let mut compiler = Compiler::new(vec![], Some("...".into()));
    let _pattern = compiler
        .compile_pattern(&list(vec![sym("begin"), sym("body"), sym("...")]), 0)
        .unwrap();

    // Template: (lambda () body ...)
    let template_form = list(vec![sym("lambda"), Value::Null, sym("body"), sym("...")]);
    let template = compiler.compile_template(&template_form, 0).unwrap();

    match template {
        Template::List(templates) => {
            assert_eq!(templates.len(), 3); // lambda, (), (body ...)
            match &templates[2] {
                Template::Ellipsis {
                    subtemplate,
                    level,
                    nesting,
                    vars,
                } => {
                    assert_eq!(*level, 1);
                    assert_eq!(*nesting, 1);
                    assert_eq!(vars.len(), 1);
                    assert!(matches!(**subtemplate, Template::Var(_)));
                }
                _ => panic!("Expected ellipsis"),
            }
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_compile_with_literals() {
    // Pattern with literal "else"
    let mut compiler = Compiler::new(vec!["else".into()], Some("...".into()));

    let pattern_form = list(vec![sym("cond"), sym("else"), sym("body")]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            // "cond" is a variable
            assert!(matches!(&patterns[0], Pattern::Var(_)));
            // "else" is a literal
            assert!(matches!(&patterns[1], Pattern::Literal(_)));
            // "body" is a variable
            assert!(matches!(&patterns[2], Pattern::Var(_)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_compile_full_macro() {
    // Compile a complete when macro
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern = list(vec![sym("when"), sym("test"), sym("body"), sym("...")]);
    let template = list(vec![
        sym("if"),
        sym("test"),
        list(vec![sym("begin"), sym("body"), sym("...")]),
    ]);

    let compiled = compiler
        .compile_macro("when".into(), vec![(pattern, template)])
        .unwrap();

    assert_eq!(compiled.name.as_ref(), "when");
    assert_eq!(compiled.rules.len(), 1);
    // R7RS: First element "when" is the macro keyword placeholder (compiled as wildcard)
    // Only "test" and "body" are actual pattern variables
    assert_eq!(compiled.rules[0].num_pvars, 2); // test and body (not when!)
    assert_eq!(compiled.rules[0].max_level, 1); // body is at level 1
}

#[test]
fn test_error_duplicate_pattern_var() {
    // Pattern: (test test) - duplicate variable
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern_form = list(vec![sym("test"), sym("test")]);
    let result = compiler.compile_pattern(&pattern_form, 0);

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Duplicate pattern variable")
    );
}

#[test]
fn test_var_level_validation() {
    // Pattern: (foo x ...)
    // This establishes 'foo' at level 0 and 'x' at level 1
    let mut compiler = Compiler::new(vec![], Some("...".into()));
    let pattern_form = list(vec![sym("foo"), sym("x"), sym("...")]);
    let _pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    // Using 'foo' (level 0) at level 0 in template should work
    let template1 = sym("foo");
    assert!(compiler.compile_template(&template1, 0).is_ok());

    // Using 'x' (level 1) in an ellipsis (level 1) should work
    let template2 = list(vec![sym("x"), sym("...")]);
    assert!(compiler.compile_template(&template2, 0).is_ok());

    // Using 'x' (level 1) at level 0 should fail - wrong level
    let template3 = sym("x");
    let result = compiler.compile_template(&template3, 0);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("at level 1 used at level 0")
    );
}

#[test]
fn test_underscore_as_wildcard() {
    // When _ is NOT in literals, it should be Wildcard
    let mut compiler = Compiler::new(vec![], Some("...".into()));

    let pattern_form = list(vec![sym("foo"), sym(WILDCARD), sym("bar")]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            // "foo" is a pattern variable
            assert!(matches!(&patterns[0], Pattern::Var(_)));
            // "_" is a wildcard
            assert!(matches!(&patterns[1], Pattern::Wildcard));
            // "bar" is a pattern variable
            assert!(matches!(&patterns[2], Pattern::Var(_)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_underscore_as_literal() {
    // When _ IS in literals, it should be Literal, not Wildcard
    let mut compiler = Compiler::new(vec![WILDCARD.into()], Some("...".into()));

    let pattern_form = list(vec![sym("foo"), sym(WILDCARD), sym("bar")]);
    let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

    match pattern {
        Pattern::List(patterns) => {
            assert_eq!(patterns.len(), 3);
            // "foo" is a pattern variable
            assert!(matches!(&patterns[0], Pattern::Var(_)));
            // "_" is a LITERAL (not wildcard!)
            assert!(
                matches!(&patterns[1], Pattern::Literal(Value::Symbol(s)) if s.as_ref() == WILDCARD),
                "Expected Literal(_), got {:?}",
                patterns[1]
            );
            // "bar" is a pattern variable
            assert!(matches!(&patterns[2], Pattern::Var(_)));
        }
        _ => panic!("Expected list"),
    }
}
