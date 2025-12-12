//! Unit tests for template expansion

use super::*;
use crate::macro_expander::template::Identifier;
use crate::macro_expander::utils::vec_to_list;
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};

// Use vec_to_list from utils as make_list alias for test readability
fn make_list(values: Vec<Value>) -> Value {
    vec_to_list(values)
}

#[test]
fn test_expand_literal() {
    let expander = Expander::default();
    let template = Template::Literal(Value::Integer(42));
    let env = MatchEnv::new(0);

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    assert_eq!(
        format!("{:?}", result.unwrap()),
        format!("{:?}", Value::Integer(42))
    );
}

#[test]
fn test_expand_symbol() {
    let expander = Expander::default();
    let template = Template::Symbol(Identifier::new("if"));
    let env = MatchEnv::new(0);

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    // After hygiene implementation, symbols become Identifiers with scope sets
    if let Value::Identifier(id) = result.unwrap() {
        assert_eq!(&*id.name, "if");
    } else {
        panic!("Expected Identifier (hygiene-wrapped symbol)");
    }
}

#[test]
fn test_expand_var() {
    let expander = Expander::default();
    let pvref = PVRef::new(0, 0);
    let template = Template::Var(pvref);

    let mut env = MatchEnv::new(1);
    env.insert(pvref, Value::Integer(42));

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    assert_eq!(
        format!("{:?}", result.unwrap()),
        format!("{:?}", Value::Integer(42))
    );
}

#[test]
fn test_expand_simple_list() {
    let expander = Expander::default();
    let x = PVRef::new(0, 0);
    let y = PVRef::new(0, 1);

    // Template: (if x y)
    let template = Template::List(vec![
        Template::Symbol(Identifier::new("if")),
        Template::Var(x),
        Template::Var(y),
    ]);

    let mut env = MatchEnv::new(2);
    env.insert(x, Value::Integer(1));
    env.insert(y, Value::Integer(2));

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());

    // Should produce (if 1 2) where 'if' is an Identifier (for hygiene)
    let expected = make_list(vec![
        Value::identifier("if"),
        Value::Integer(1),
        Value::Integer(2),
    ]);
    assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
}

#[test]
fn test_expand_ellipsis_simple() {
    let expander = Expander::default();
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
            MatchValue::Leaf(Value::Integer(1)),
            MatchValue::Leaf(Value::Integer(2)),
            MatchValue::Leaf(Value::Integer(3)),
        ],
    );

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());

    // Should produce (1 2 3)
    let expected = make_list(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
}

#[test]
fn test_expand_ellipsis_with_constant() {
    let expander = Expander::default();
    let x = PVRef::new(1, 0);

    // Template: ((+ x 1) ...)
    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::List(vec![
            Template::Symbol(Identifier::new("+")),
            Template::Var(x),
            Template::Literal(Value::Integer(1)),
        ])),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let mut env = MatchEnv::new(1);
    // x has 2 values: 10, 20
    env.insert_branch(
        x,
        vec![
            MatchValue::Leaf(Value::Integer(10)),
            MatchValue::Leaf(Value::Integer(20)),
        ],
    );

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());

    // Should produce ((+ 10 1) (+ 20 1)) where + is an Identifier (hygiene)
    let elem1 = make_list(vec![
        Value::identifier("+"),
        Value::Integer(10),
        Value::Integer(1),
    ]);
    let elem2 = make_list(vec![
        Value::identifier("+"),
        Value::Integer(20),
        Value::Integer(1),
    ]);
    let expected = make_list(vec![elem1, elem2]);
    assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
}

#[test]
fn test_expand_ellipsis_with_following() {
    let expander = Expander::default();
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
        vec![
            MatchValue::Leaf(Value::Integer(1)),
            MatchValue::Leaf(Value::Integer(2)),
        ],
    );
    // y is a single value: 99
    env.insert(y, Value::Integer(99));

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());

    // Should produce (begin 1 2 99) where 'begin' is an Identifier (for hygiene)
    let expected = make_list(vec![
        Value::identifier("begin"),
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(99),
    ]);
    assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
}

#[test]
fn test_expand_double_ellipsis() {
    // This tests the do macro use case with BOTH bindings having steps:
    // Pattern: ((var init step ...) ...)
    // Template: (loop step ... ...)
    // Input: ((i 0 (+ i 1)) (j 10 (- j 1)))
    // Expected: (loop (+ i 1) (- j 1))

    use patina_runtime::Environment;
    let macro_scope = patina_runtime::ScopeId::fresh();
    let expander = Expander::new(std::rc::Rc::new(Environment::new()), macro_scope);
    let step = PVRef::new(2, 0); // level 2 because it's in nested ellipsis

    // Template: (loop step ... ...)
    let template = Template::List(vec![
        Template::Symbol(Identifier::new("loop")),
        Template::Ellipsis {
            subtemplate: Box::new(Template::Var(step)),
            level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
            nesting: 2,
            vars: vec![step],
        },
    ]);

    let mut env = MatchEnv::new(1);

    // step is a doubly-nested structure:
    // Branch([
    //   Branch([Leaf((+ i 1))]),    // First binding has 1 step
    //   Branch([Leaf((- j 1))])     // Second binding has 1 step
    // ])
    // NOTE: Input values are symbols, but mark_substituted_value converts them to
    // identifiers with macro_scope when they flow through pattern variables
    let step_i = make_list(vec![
        Value::symbol("+"),
        Value::symbol("i"),
        Value::Integer(1),
    ]);
    let step_j = make_list(vec![
        Value::symbol("-"),
        Value::symbol("j"),
        Value::Integer(1),
    ]);

    env.insert_branch(
        step,
        vec![
            MatchValue::Branch(vec![MatchValue::Leaf(step_i.clone())]), // i binding: 1 step
            MatchValue::Branch(vec![MatchValue::Leaf(step_j.clone())]), // j binding: 1 step
        ],
    );

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());
    let result = result.unwrap();

    // Verify structure: (loop <step1> <step2>)
    // 'loop' is introduced by template
    // step_i and step_j come from input but get macro_scope added by mark_substituted_value
    let result_str = format!("{}", result);
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
    // Test case where inner ellipsis has 0 elements for some iterations
    // Pattern: ((a b ...) ...)
    // Template: (result b ... ...)
    // Input: ((x 1 2) (y))
    // Expected: (result 1 2) - empty inner branch contributes nothing

    let expander = Expander::default();
    let b = PVRef::new(2, 0);

    let template = Template::List(vec![
        Template::Symbol(Identifier::new("result")),
        Template::Ellipsis {
            subtemplate: Box::new(Template::Var(b)),
            level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
            nesting: 2,
            vars: vec![b],
        },
    ]);

    let mut env = MatchEnv::new(1);
    env.insert_branch(
        b,
        vec![
            MatchValue::Branch(vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
            ]), // First group: 2 elements
            MatchValue::Branch(vec![]), // Second group: 0 elements
        ],
    );

    let result = expander.expand(&template, &env);
    assert!(result.is_ok());

    // Should produce (result 1 2) - second group contributes nothing
    // 'result' is introduced by template (so becomes Identifier)
    let expected = make_list(vec![
        Value::identifier("result"),
        Value::Integer(1),
        Value::Integer(2),
    ]);

    assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
}

// === Error Condition Tests ===

#[test]
fn test_error_undefined_variable() {
    // Template references a variable that's not in the environment
    let expander = Expander::default();
    let x = PVRef::new(0, 5); // Out of bounds!
    let template = Template::Var(x);

    let env = MatchEnv::new(1); // Only has space for 1 var (index 0)

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
        "Expected UndefinedVariable error"
    );
}

#[test]
fn test_error_inconsistent_repetition() {
    // Two variables in same ellipsis with different repetition counts
    let expander = Expander::default();
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
    // x has 3 values
    env.insert_branch(
        x,
        vec![
            MatchValue::Leaf(Value::Integer(1)),
            MatchValue::Leaf(Value::Integer(2)),
            MatchValue::Leaf(Value::Integer(3)),
        ],
    );
    // y has only 2 values - INCONSISTENT!
    env.insert_branch(
        y,
        vec![
            MatchValue::Leaf(Value::Integer(10)),
            MatchValue::Leaf(Value::Integer(20)),
        ],
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
    // Triple ellipsis (nesting = 3) should be rejected
    let expander = Expander::default();
    let x = PVRef::new(3, 0);

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 3, // Triple ellipsis!
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
    // Double ellipsis with level 0 is invalid
    let expander = Expander::default();
    let x = PVRef::new(1, 0);

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 0, // Level 0 with double ellipsis is invalid!
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
    // This tests that ellipsis expansion produces a list
    // Actually, this is hard to trigger with the current implementation
    // because ellipsis always produces lists. Keeping this as a placeholder
    // in case we add validation later.
}

#[test]
fn test_error_level_mismatch_leaf_instead_of_branch() {
    // Try to expand an ellipsis when the variable is a Leaf instead of Branch
    let expander = Expander::default();
    let x = PVRef::new(1, 0);

    // Template: (x ...)
    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    let mut env = MatchEnv::new(1);
    // Insert x as a Leaf instead of Branch - level mismatch!
    env.insert(x, Value::Integer(42));

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    // This will likely be UndefinedVariable or LevelMismatch
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
    // Variable undefined in nested ellipsis context (out of bounds)
    let expander = Expander::default();
    let x = PVRef::new(1, 5); // Out of bounds!

    let template = Template::List(vec![Template::Ellipsis {
        subtemplate: Box::new(Template::Var(x)),
        level: 1,
        nesting: 1,
        vars: vec![x],
    }]);

    // Create env with limited space
    let env = MatchEnv::new(1); // Only has space for index 0

    let result = expander.expand(&template, &env);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
        "Expected UndefinedVariable error"
    );
}
