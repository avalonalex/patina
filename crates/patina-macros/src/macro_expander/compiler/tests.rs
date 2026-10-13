//! Unit tests for the pattern/template compiler

use crate::macro_expander::compiler::Compiler;
use crate::macro_expander::utils::WILDCARD;
use crate::macro_expander::{Pattern, Template};
use patina_core::{Cx, GcDriver, Instance, LiteralId, Value, new_shared_heap};

/// Run `f` with a context over a fresh heap.
fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
    let heap = new_shared_heap();
    let instance = Instance::default();
    let mut driver = GcDriver::acquire(&heap).expect("the carrier's driver is free");
    driver.mutate(&instance, f)
}

/// Helper to intern a symbol
fn sym<'gc>(s: &str, cx: &mut Cx<'gc>) -> Value<'gc> {
    cx.intern_symbol(s)
}

/// Helper to create a list
fn list<'gc>(items: Vec<Value<'gc>>, cx: &mut Cx<'gc>) -> Value<'gc> {
    cx.list_from_iter(items)
}

#[test]
fn test_compile_simple_pattern() {
    with_cx(|cx| {
        // Pattern: (when test body)
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("when", cx), sym("test", cx), sym("body", cx)], cx);
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
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
    });
}

#[test]
fn test_compile_pattern_with_ellipsis() {
    with_cx(|cx| {
        // Pattern: (when test body ...)
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(
            vec![
                sym("when", cx),
                sym("test", cx),
                sym("body", cx),
                sym("...", cx),
            ],
            cx,
        );
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
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
    });
}

#[test]
fn test_compile_pattern_ellipsis_with_following() {
    with_cx(|cx| {
        // Pattern: (do bindings ... (test result))
        // The ellipsis should have num_following = 1
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let inner_list = list(vec![sym("test", cx), sym("result", cx)], cx);
        let pattern_form = list(
            vec![
                sym("do", cx),
                sym("bindings", cx),
                sym("...", cx),
                inner_list,
            ],
            cx,
        );
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
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
    });
}

#[test]
fn test_compile_simple_template() {
    with_cx(|cx| {
        // First compile a pattern to establish variables
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let pat = list(vec![sym("when", cx), sym("test", cx), sym("body", cx)], cx);
        let _pattern = compiler.compile_pattern(cx, pat, 0).unwrap();

        // Now compile template: (if test body)
        let template_form = list(vec![sym("if", cx), sym("test", cx), sym("body", cx)], cx);
        let template = compiler.compile_template(cx, template_form, 0).unwrap();

        match &template {
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
    });
}

#[test]
fn test_compile_template_with_ellipsis() {
    with_cx(|cx| {
        // Pattern: (begin body ...)
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let pat = list(vec![sym("begin", cx), sym("body", cx), sym("...", cx)], cx);
        let _pattern = compiler.compile_pattern(cx, pat, 0).unwrap();

        // Template: (lambda () body ...)
        let template_form = list(
            vec![
                sym("lambda", cx),
                Value::NULL,
                sym("body", cx),
                sym("...", cx),
            ],
            cx,
        );
        let template = compiler.compile_template(cx, template_form, 0).unwrap();

        match &template {
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
    });
}

#[test]
fn test_compile_with_literals() {
    with_cx(|cx| {
        // Pattern with literal "else"
        let mut compiler = Compiler::new(vec!["else".into()], Some("...".into()));

        let pattern_form = list(vec![sym("cond", cx), sym("else", cx), sym("body", cx)], cx);
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
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
    });
}

#[test]
fn test_compile_full_macro() {
    with_cx(|cx| {
        // Compile a complete when macro
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern = list(
            vec![
                sym("when", cx),
                sym("test", cx),
                sym("body", cx),
                sym("...", cx),
            ],
            cx,
        );
        let template = list(
            vec![
                sym("if", cx),
                sym("test", cx),
                list(vec![sym("begin", cx), sym("body", cx), sym("...", cx)], cx),
            ],
            cx,
        );

        let (compiled, _) = compiler
            .compile_macro(cx, "when".into(), vec![(pattern, template)])
            .unwrap();

        assert_eq!(compiled.name.as_ref(), "when");
        assert_eq!(compiled.rules.len(), 1);
        // R7RS: First element "when" is the macro keyword placeholder (compiled as wildcard)
        // Only "test" and "body" are actual pattern variables
        assert_eq!(compiled.rules[0].num_pvars, 2); // test and body (not when!)
        assert_eq!(compiled.rules[0].max_level, 1); // body is at level 1
    });
}

#[test]
fn test_error_duplicate_pattern_var() {
    with_cx(|cx| {
        // Pattern: (test test) - duplicate variable
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("test", cx), sym("test", cx)], cx);
        let result = compiler.compile_pattern(cx, pattern_form, 0);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate pattern variable")
        );
    });
}

#[test]
fn test_var_level_validation() {
    with_cx(|cx| {
        // Pattern: (foo x ...)
        // This establishes 'foo' at level 0 and 'x' at level 1
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let pattern_form = list(vec![sym("foo", cx), sym("x", cx), sym("...", cx)], cx);
        let _pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        // Using 'foo' (level 0) at level 0 in template should work
        let template1 = sym("foo", cx);
        assert!(compiler.compile_template(cx, template1, 0).is_ok());

        // Using 'x' (level 1) in an ellipsis (level 1) should work
        let template2 = list(vec![sym("x", cx), sym("...", cx)], cx);
        assert!(compiler.compile_template(cx, template2, 0).is_ok());

        // Using 'x' (level 1) at level 0 should fail - wrong level
        let template3 = sym("x", cx);
        let result = compiler.compile_template(cx, template3, 0);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at level 1 used at level 0")
        );
    });
}

#[test]
fn test_underscore_as_wildcard() {
    with_cx(|cx| {
        // When _ is NOT in literals, it should be Wildcard
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("foo", cx), sym(WILDCARD, cx), sym("bar", cx)], cx);
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
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
    });
}

#[test]
fn test_underscore_as_literal() {
    with_cx(|cx| {
        // When _ IS in literals, it should be Literal, not Wildcard
        let mut compiler = Compiler::new(vec![WILDCARD.into()], Some("...".into()));

        let pattern_form = list(vec![sym("foo", cx), sym(WILDCARD, cx), sym("bar", cx)], cx);
        let pattern = compiler.compile_pattern(cx, pattern_form, 0).unwrap();

        match &pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                // "foo" is a pattern variable
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                // "_" is a LITERAL (not wildcard!)
                assert!(
                    matches!(&patterns[1], Pattern::Literal(_)),
                    "Expected Literal(_), got {:?}",
                    patterns[1]
                );
                // "bar" is a pattern variable
                assert!(matches!(&patterns[2], Pattern::Var(_)));
            }
            _ => panic!("Expected list"),
        }
    });
}

/// A generated macro's template is made of identifiers an outer expansion
/// produced, and each one the compiler keeps verbatim is also *recorded* —
/// name and scopes — so the relinker can carry it back to the definition
/// site (#402). A pattern variable is not a mention, and a symbol written in
/// the template is `template_symbols`' business, not this list's.
#[test]
fn test_identifiers_from_an_outer_expansion_are_recorded() {
    with_cx(|cx| {
        use patina_runtime::{ScopeId, ScopeSet};
        let outer = ScopeSet::new().with_scope(ScopeId::fresh());

        // As `(define-syntax m (syntax-rules () ((m v) (helper v written))))`
        // looks once an outer expansion has produced it: `helper` and `v` are
        // identifiers carrying that expansion's scope, `written` a plain symbol.
        let v = cx.alloc_identifier("v".into(), outer.clone());
        let helper = cx.alloc_identifier("helper".into(), outer.clone());
        let pattern = list(vec![sym("m", cx), v], cx);
        let template = list(vec![helper, v, sym("written", cx)], cx);

        let mut compiler = Compiler::new(vec![], None);
        let (compiled, literals) = compiler
            .compile_macro(cx, "m".into(), vec![(pattern, template)])
            .unwrap();
        assert_eq!(literals, [helper], "the inherited `helper` is a literal");

        assert_eq!(
            compiled.inherited_identifiers.get("helper"),
            Some(&vec![outer.clone()]),
            "the outer expansion's `helper` is a mention, under its own scopes"
        );
        assert!(
            !compiled.inherited_identifiers.contains_key("v"),
            "a pattern variable is substituted, not mentioned"
        );
        assert!(!compiled.inherited_identifiers.contains_key("written"));
        assert!(compiled.template_symbols.contains("written"));
        assert!(!compiled.template_symbols.contains("helper"));

        // A second macro from the same compiler starts clean.
        let pattern = list(vec![sym("n", cx)], cx);
        let template = list(vec![sym("written", cx)], cx);
        let (second, literals) = compiler
            .compile_macro(cx, "n".into(), vec![(pattern, template)])
            .unwrap();
        assert!(second.inherited_identifiers.is_empty());
        assert!(
            literals.is_empty(),
            "nor does the first's literal carry over"
        );

        // So does one that follows a macro that failed to compile, after its
        // template had already recorded something: `(helper (written ...))` has
        // an ellipsis over no pattern variable.
        let dots = sym("...", cx);
        let bad_tail = list(vec![sym("written", cx), dots], cx);
        let bad = list(
            vec![
                cx.alloc_identifier("helper".into(), outer.clone()),
                bad_tail,
            ],
            cx,
        );
        assert!(
            compiler
                .compile_macro(cx, "bad".into(), vec![(pattern, bad)])
                .is_err()
        );
        let (third, literals) = compiler
            .compile_macro(cx, "n".into(), vec![(pattern, template)])
            .unwrap();
        assert!(third.inherited_identifiers.is_empty());
        assert!(literals.is_empty(), "nor the failed one's literal");
    });
}

/// A literal the rules name more than once is one entry of the macro
/// object's literals (#718): `(m 1)` matches a literal `1`, and the template
/// `(list 1 1)` names it twice more.
#[test]
fn each_literal_is_collected_once() {
    with_cx(|cx| {
        let one = Value::fixnum(1);
        let pattern = list(vec![sym("m", cx), one], cx);
        let template = list(vec![sym("list", cx), one, one], cx);
        let mut compiler = Compiler::new(vec![], None);
        let (compiled, literals) = compiler
            .compile_macro(cx, "m".into(), vec![(pattern, template)])
            .unwrap();
        assert_eq!(literals, [one]);
        let rule = &compiled.rules[0];
        let Pattern::List(patterns) = &rule.pattern else {
            panic!("a list pattern: {}", rule.pattern);
        };
        let Template::List(templates) = &rule.template else {
            panic!("a list template: {}", rule.template);
        };
        let first = LiteralId::new(0);
        assert!(matches!(patterns[1], Pattern::Literal(id) if id == first));
        assert!(matches!(templates[1], Template::Literal(id) if id == first));
        assert!(matches!(templates[2], Template::Literal(id) if id == first));
    });
}
