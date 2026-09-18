//! Unit tests for the pattern/template compiler

use crate::macro_expander::compiler::Compiler;
use crate::macro_expander::utils::WILDCARD;
use crate::macro_expander::{Pattern, Template};
use patina_core::{SharedHeap, TaggedValue};

fn test_heap() -> SharedHeap {
    patina_core::new_shared_heap()
}

/// Helper to intern a symbol on the heap
fn sym(heap: &SharedHeap, s: &str) -> TaggedValue {
    heap.borrow_mut().intern_symbol(s)
}

/// Helper to create a list from TaggedValues
fn list(heap: &SharedHeap, items: Vec<TaggedValue>) -> TaggedValue {
    items.iter().rev().fold(TaggedValue::NULL, |acc, tv| {
        heap.borrow_mut().alloc_pair(*tv, acc)
    })
}

#[test]
fn test_compile_simple_pattern() {
    let heap = test_heap();
    // Pattern: (when test body)
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let pattern_form = list(
        &heap,
        vec![sym(&heap, "when"), sym(&heap, "test"), sym(&heap, "body")],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

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
    let heap = test_heap();
    // Pattern: (when test body ...)
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let pattern_form = list(
        &heap,
        vec![
            sym(&heap, "when"),
            sym(&heap, "test"),
            sym(&heap, "body"),
            sym(&heap, "..."),
        ],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

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
    let heap = test_heap();
    // Pattern: (do bindings ... (test result))
    // The ellipsis should have num_following = 1
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let inner_list = list(&heap, vec![sym(&heap, "test"), sym(&heap, "result")]);
    let pattern_form = list(
        &heap,
        vec![
            sym(&heap, "do"),
            sym(&heap, "bindings"),
            sym(&heap, "..."),
            inner_list,
        ],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

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
    let heap = test_heap();
    // First compile a pattern to establish variables
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());
    let pat = list(
        &heap,
        vec![sym(&heap, "when"), sym(&heap, "test"), sym(&heap, "body")],
    );
    let _pattern = compiler.compile_pattern(pat, 0).unwrap();

    // Now compile template: (if test body)
    let template_form = list(
        &heap,
        vec![sym(&heap, "if"), sym(&heap, "test"), sym(&heap, "body")],
    );
    let template = compiler.compile_template(template_form, 0).unwrap();

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
    let heap = test_heap();
    // Pattern: (begin body ...)
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());
    let pat = list(
        &heap,
        vec![sym(&heap, "begin"), sym(&heap, "body"), sym(&heap, "...")],
    );
    let _pattern = compiler.compile_pattern(pat, 0).unwrap();

    // Template: (lambda () body ...)
    let template_form = list(
        &heap,
        vec![
            sym(&heap, "lambda"),
            TaggedValue::NULL,
            sym(&heap, "body"),
            sym(&heap, "..."),
        ],
    );
    let template = compiler.compile_template(template_form, 0).unwrap();

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
    let heap = test_heap();
    // Pattern with literal "else"
    let mut compiler = Compiler::new(vec!["else".into()], Some("...".into()), heap.clone());

    let pattern_form = list(
        &heap,
        vec![sym(&heap, "cond"), sym(&heap, "else"), sym(&heap, "body")],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

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
    let heap = test_heap();
    // Compile a complete when macro
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let pattern = list(
        &heap,
        vec![
            sym(&heap, "when"),
            sym(&heap, "test"),
            sym(&heap, "body"),
            sym(&heap, "..."),
        ],
    );
    let template = list(
        &heap,
        vec![
            sym(&heap, "if"),
            sym(&heap, "test"),
            list(
                &heap,
                vec![sym(&heap, "begin"), sym(&heap, "body"), sym(&heap, "...")],
            ),
        ],
    );

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
    let heap = test_heap();
    // Pattern: (test test) - duplicate variable
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let pattern_form = list(&heap, vec![sym(&heap, "test"), sym(&heap, "test")]);
    let result = compiler.compile_pattern(pattern_form, 0);

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
    let heap = test_heap();
    // Pattern: (foo x ...)
    // This establishes 'foo' at level 0 and 'x' at level 1
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());
    let pattern_form = list(
        &heap,
        vec![sym(&heap, "foo"), sym(&heap, "x"), sym(&heap, "...")],
    );
    let _pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

    // Using 'foo' (level 0) at level 0 in template should work
    let template1 = sym(&heap, "foo");
    assert!(compiler.compile_template(template1, 0).is_ok());

    // Using 'x' (level 1) in an ellipsis (level 1) should work
    let template2 = list(&heap, vec![sym(&heap, "x"), sym(&heap, "...")]);
    assert!(compiler.compile_template(template2, 0).is_ok());

    // Using 'x' (level 1) at level 0 should fail - wrong level
    let template3 = sym(&heap, "x");
    let result = compiler.compile_template(template3, 0);
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
    let heap = test_heap();
    // When _ is NOT in literals, it should be Wildcard
    let mut compiler = Compiler::new(vec![], Some("...".into()), heap.clone());

    let pattern_form = list(
        &heap,
        vec![sym(&heap, "foo"), sym(&heap, WILDCARD), sym(&heap, "bar")],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

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
    let heap = test_heap();
    // When _ IS in literals, it should be Literal, not Wildcard
    let mut compiler = Compiler::new(vec![WILDCARD.into()], Some("...".into()), heap.clone());

    let pattern_form = list(
        &heap,
        vec![sym(&heap, "foo"), sym(&heap, WILDCARD), sym(&heap, "bar")],
    );
    let pattern = compiler.compile_pattern(pattern_form, 0).unwrap();

    match pattern {
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
}

/// A generated macro's template is made of identifiers an outer expansion
/// produced, and each one the compiler keeps verbatim is also *recorded* —
/// name and scopes — so the relinker can carry it back to the definition
/// site (#402). A pattern variable is not a mention, and a symbol written in
/// the template is `template_symbols`' business, not this list's.
#[test]
fn test_identifiers_from_an_outer_expansion_are_recorded() {
    use patina_runtime::{ScopeId, ScopeSet};

    let heap = test_heap();
    let outer = ScopeSet::new().with_scope(ScopeId::fresh());
    let ident = |name: &str| {
        heap.borrow_mut()
            .alloc_identifier(name.into(), outer.clone())
    };

    // As `(define-syntax m (syntax-rules () ((m v) (helper v written))))`
    // looks once an outer expansion has produced it: `helper` and `v` are
    // identifiers carrying that expansion's scope, `written` a plain symbol.
    let v = ident("v");
    let pattern = list(&heap, vec![sym(&heap, "m"), v]);
    let template = list(&heap, vec![ident("helper"), v, sym(&heap, "written")]);

    let mut compiler = Compiler::new(vec![], None, heap.clone());
    let compiled = compiler
        .compile_macro("m".into(), vec![(pattern, template)])
        .unwrap();

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
    let pattern = list(&heap, vec![sym(&heap, "n")]);
    let template = list(&heap, vec![sym(&heap, "written")]);
    let second = compiler
        .compile_macro("n".into(), vec![(pattern, template)])
        .unwrap();
    assert!(second.inherited_identifiers.is_empty());

    // So does one that follows a macro that failed to compile, after its
    // template had already recorded something: `(helper (written ...))` has
    // an ellipsis over no pattern variable.
    let dots = sym(&heap, "...");
    let bad_tail = list(&heap, vec![sym(&heap, "written"), dots]);
    let bad = list(&heap, vec![ident("helper"), bad_tail]);
    assert!(
        compiler
            .compile_macro("bad".into(), vec![(pattern, bad)])
            .is_err()
    );
    let third = compiler
        .compile_macro("n".into(), vec![(pattern, template)])
        .unwrap();
    assert!(third.inherited_identifiers.is_empty());
}
