//! Unit tests for template expansion

use super::*;
use crate::macro_expander::Identifier;
use crate::macro_expander::interface::TestExpander;
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::{MatchEnv, MatchValue, PVRef};
use std::cell::RefCell;
use std::rc::Rc;

/// Helper to create a fixnum TaggedValue for tests
fn fixnum(n: i64) -> TaggedValue {
    TaggedValue::fixnum(n)
}

/// Helper to create an expander with a shared heap for tests
fn make_test_expander() -> (Expander, SharedHeap) {
    let shared_heap: SharedHeap = Rc::new(RefCell::new(patina_core::Heap::new()));
    let macro_scope = patina_runtime::ScopeId::fresh();
    let expander = Expander::new_with_heap(macro_scope, shared_heap.clone());
    (expander, shared_heap)
}

/// Helper to build a TaggedValue list from a Vec
fn make_list_tagged(values: Vec<TaggedValue>, heap: &SharedHeap) -> TaggedValue {
    let mut result = TaggedValue::NULL;
    for value in values.into_iter().rev() {
        result = heap.borrow_mut().alloc_pair(value, result);
    }
    result
}

/// Helper to intern a symbol on the shared heap
fn sym(name: &str, heap: &SharedHeap) -> TaggedValue {
    heap.borrow_mut().intern_symbol(name)
}

#[test]
fn test_expand_literal() {
    let (expander, _heap) = make_test_expander();
    let template = Template::Literal(TaggedValue::fixnum(42));
    let env = MatchEnv::new(0);

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    let tv = result.unwrap();
    assert_eq!(tv.as_fixnum(), Some(42));
}

#[test]
fn test_expand_symbol() {
    let (expander, heap) = make_test_expander();
    let template = Template::Symbol(Identifier::new("if"));
    let env = MatchEnv::new(0);

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    let tv = result.unwrap();
    // After hygiene implementation, symbols become Identifiers with scope sets
    let name = heap
        .borrow()
        .get_symbol_or_identifier_name(tv)
        .map(|s| s.to_string());
    assert_eq!(
        name,
        Some("if".to_string()),
        "Expected identifier with name 'if'"
    );
}

#[test]
fn test_expand_var() {
    let (expander, _heap) = make_test_expander();
    let pvref = PVRef::new(0, 0);
    let template = Template::Var(pvref);

    let mut env = MatchEnv::new(1);
    env.insert(pvref, fixnum(42));

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_fixnum(), Some(42));
}

#[test]
fn test_expand_simple_list() {
    let (expander, heap) = make_test_expander();
    let x = PVRef::new(0, 0);
    let y = PVRef::new(0, 1);

    // Template: (if x y)
    let template = Template::List(vec![
        Template::Symbol(Identifier::new("if")),
        Template::Var(x),
        Template::Var(y),
    ]);

    let mut env = MatchEnv::new(2);
    env.insert(x, fixnum(1));
    env.insert(y, fixnum(2));

    let result = expander.expand(&template, &env).unwrap();

    // Should produce (if 1 2) where 'if' is an Identifier (for hygiene)
    let if_sym = sym("if", &heap);
    let expected = make_list_tagged(vec![if_sym, fixnum(1), fixnum(2)], &heap);
    assert!(
        TestExpander::tagged_forms_equal_ignoring_gensym(result, expected, &heap.borrow()),
        "Expected (if 1 2), got different structure"
    );
}

#[test]
fn test_expand_ellipsis_simple() {
    let (expander, heap) = make_test_expander();
    let x = PVRef::new(1, 0);

    // Template: (x ...)
    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let mut env = MatchEnv::new(1);
    // x has 3 values: 1, 2, 3
    env.insert_branch(
        x,
        vec![
            MatchValue::Leaf(fixnum(1)),
            MatchValue::Leaf(fixnum(2)),
            MatchValue::Leaf(fixnum(3)),
        ],
    );

    let result = expander.expand(&template, &env).unwrap();

    // Should produce (1 2 3)
    let expected = make_list_tagged(vec![fixnum(1), fixnum(2), fixnum(3)], &heap);
    assert!(
        heap.borrow().tagged_values_equal(result, expected),
        "Expected (1 2 3)"
    );
}

#[test]
fn test_expand_ellipsis_with_constant() {
    let (expander, heap) = make_test_expander();
    let x = PVRef::new(1, 0);

    // Template: ((+ x 1) ...)
    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::List(vec![
            Template::Symbol(Identifier::new("+")),
            Template::Var(x),
            Template::Literal(TaggedValue::fixnum(1)),
        ])),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let mut env = MatchEnv::new(1);
    // x has 2 values: 10, 20
    env.insert_branch(
        x,
        vec![MatchValue::Leaf(fixnum(10)), MatchValue::Leaf(fixnum(20))],
    );

    let result = expander.expand(&template, &env).unwrap();

    // Should produce ((+ 10 1) (+ 20 1)) where + is an Identifier (hygiene)
    let plus = sym("+", &heap);
    let elem1 = make_list_tagged(vec![plus, fixnum(10), fixnum(1)], &heap);
    let elem2 = make_list_tagged(vec![plus, fixnum(20), fixnum(1)], &heap);
    let expected = make_list_tagged(vec![elem1, elem2], &heap);
    assert!(
        TestExpander::tagged_forms_equal_ignoring_gensym(result, expected, &heap.borrow()),
        "Expected ((+ 10 1) (+ 20 1))"
    );
}

#[test]
fn test_expand_ellipsis_with_following() {
    let (expander, heap) = make_test_expander();
    let x = PVRef::new(1, 0);
    let y = PVRef::new(0, 1);

    // Template: (begin x ... y)
    let template = Template::List(vec![
        Template::Symbol(Identifier::new("begin")),
        Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        },
        Template::Var(y),
    ]);

    let mut env = MatchEnv::new(2);
    // x has 2 values: 1, 2
    env.insert_branch(
        x,
        vec![MatchValue::Leaf(fixnum(1)), MatchValue::Leaf(fixnum(2))],
    );
    // y is a single value: 99
    env.insert(y, fixnum(99));

    let result = expander.expand(&template, &env).unwrap();

    // Should produce (begin 1 2 99) where 'begin' is an Identifier (for hygiene)
    let begin = sym("begin", &heap);
    let expected = make_list_tagged(vec![begin, fixnum(1), fixnum(2), fixnum(99)], &heap);
    assert!(
        TestExpander::tagged_forms_equal_ignoring_gensym(result, expected, &heap.borrow()),
        "Expected (begin 1 2 99)"
    );
}

#[test]
fn test_expand_double_ellipsis() {
    let (expander, shared_heap) = make_test_expander();
    let step = PVRef::new(2, 0); // level 2 because it's in nested ellipsis

    // Template: (loop step ... ...)
    let template = Template::List(vec![
        Template::Symbol(Identifier::new("loop")),
        Template::Ellipsis {
            subtemplate: Box::new(Template::Var(step)),
            level: 1,
            nesting: 2,
            vars: vec![step],
        },
    ]);

    let mut env = MatchEnv::new(1);

    let step_i = make_list_tagged(
        vec![sym("+", &shared_heap), sym("i", &shared_heap), fixnum(1)],
        &shared_heap,
    );
    let step_j = make_list_tagged(
        vec![sym("-", &shared_heap), sym("j", &shared_heap), fixnum(1)],
        &shared_heap,
    );

    // Build inner branches first
    let inner1 = MatchValue::branch(vec![MatchValue::Leaf(step_i)], env.branches_mut());
    let inner2 = MatchValue::branch(vec![MatchValue::Leaf(step_j)], env.branches_mut());
    env.insert_branch(step, vec![inner1, inner2]);

    let result = expander.expand(&template, &env).unwrap();

    let result_str = patina_core::format_tagged(result, &shared_heap.borrow());
    assert!(
        result_str.contains("loop"),
        "Result should contain 'loop': {}",
        result_str
    );
    assert!(
        result_str.contains("+") && result_str.contains("i"),
        "Result should contain (+ i 1): {}",
        result_str
    );
    assert!(
        result_str.contains("-") && result_str.contains("j"),
        "Result should contain (- j 1): {}",
        result_str
    );
}

#[test]
fn test_expand_double_ellipsis_empty_inner() {
    let (expander, heap) = make_test_expander();
    let b = PVRef::new(2, 0);

    let template = Template::List(vec![
        Template::Symbol(Identifier::new("result")),
        Template::Ellipsis {
            subtemplate: Box::new(Template::Var(b)),
            level: 1,
            nesting: 2,
            vars: vec![b],
        },
    ]);

    let mut env = MatchEnv::new(1);
    let inner1 = MatchValue::branch(
        vec![MatchValue::Leaf(fixnum(1)), MatchValue::Leaf(fixnum(2))],
        env.branches_mut(),
    );
    let inner2 = MatchValue::branch(vec![], env.branches_mut()); // Empty
    env.insert_branch(b, vec![inner1, inner2]);

    let result = expander.expand(&template, &env).unwrap();

    // Should produce (result 1 2) - second group contributes nothing
    let result_sym = sym("result", &heap);
    let expected = make_list_tagged(vec![result_sym, fixnum(1), fixnum(2)], &heap);
    assert!(
        TestExpander::tagged_forms_equal_ignoring_gensym(result, expected, &heap.borrow()),
        "Expected (result 1 2)"
    );
}

// === Error Condition Tests ===

#[test]
fn test_error_undefined_variable() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(0, 5); // Out of bounds!
    let template = Template::Var(x);

    let env = MatchEnv::new(1);

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
        "Expected UndefinedVariable error"
    );
}

#[test]
fn test_error_inconsistent_repetition() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(1, 0);
    let y = PVRef::new(1, 1);

    // Template: ((x y) ...)
    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::List(vec![Template::Var(x), Template::Var(y)])),
        level: 1,
        nesting: 1,
        vars: vec![x, y],
    }]);

    let mut env = MatchEnv::new(2);
    env.insert_branch(
        x,
        vec![
            MatchValue::Leaf(fixnum(1)),
            MatchValue::Leaf(fixnum(2)),
            MatchValue::Leaf(fixnum(3)),
        ],
    );
    env.insert_branch(
        y,
        vec![MatchValue::Leaf(fixnum(10)), MatchValue::Leaf(fixnum(20))],
    );

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(
            result.unwrap_err(),
            ExpandError::InconsistentRepetition { .. }
        ),
        "Expected InconsistentRepetition error"
    );
}

#[test]
fn test_error_triple_ellipsis_not_supported() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(3, 0);

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 3,
        vars: vec![x],
    }]);

    let env = MatchEnv::new(1);

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
        "Expected InvalidTemplate error for triple ellipsis"
    );
}

#[test]
fn test_error_double_ellipsis_level_zero() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(1, 0);

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 0,
        nesting: 2,
        vars: vec![x],
    }]);

    let env = MatchEnv::new(1);

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
        "Expected InvalidTemplate error for level-0 double ellipsis"
    );
}

#[test]
fn test_error_ellipsis_expands_to_non_list() {
    // Placeholder - ellipsis always produces lists in the current implementation
}

#[test]
fn test_error_level_mismatch_leaf_instead_of_branch() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(1, 0);

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let mut env = MatchEnv::new(1);
    env.insert(x, fixnum(42));

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(
            result.unwrap_err(),
            ExpandError::LevelMismatch { .. } | ExpandError::UndefinedVariable { .. }
        ),
        "Expected LevelMismatch or UndefinedVariable error"
    );
}

#[test]
fn test_error_undefined_in_nested_context() {
    let (expander, _heap) = make_test_expander();
    let x = PVRef::new(1, 5); // Out of bounds!

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let env = MatchEnv::new(1);

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
        "Expected UndefinedVariable error"
    );
}
