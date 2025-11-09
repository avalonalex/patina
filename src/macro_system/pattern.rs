//! Pattern matching for macro expansion
//!
//! Implements R7RS pattern matching algorithm with support for:
//! - Literal constants
//! - Pattern variables
//! - Ellipsis patterns (zero-or-more repetition)
//! - Literal identifier matching

use super::{Bindings, Pattern};
use crate::value::Value;
use crate::EvalError;
use std::rc::Rc;

/// Match a pattern against an expression
///
/// Returns `Some(bindings)` if match succeeds, `None` otherwise.
///
/// # Arguments
/// - `pattern`: The pattern to match
/// - `expr`: The expression to match against
/// - `literals`: Identifiers that should match by binding identity
///
/// # Example
/// ```ignore
/// let pattern = Pattern::Variable("x".into());
/// let expr = Value::Integer(42);
/// let bindings = match_pattern(&pattern, &expr, &[]).unwrap();
/// assert_eq!(bindings.get("x"), Some(&BindingValue::Single(Value::Integer(42))));
/// ```
pub fn match_pattern(pattern: &Pattern, expr: &Value, literals: &[Rc<str>]) -> Option<Bindings> {
    let mut bindings = Bindings::new();
    if match_pattern_impl(pattern, expr, literals, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

/// Internal pattern matching implementation
fn match_pattern_impl(
    pattern: &Pattern,
    expr: &Value,
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    match pattern {
        // Wildcard: always matches, binds nothing
        Pattern::Wildcard => true,

        // Literal: must match exactly
        Pattern::Literal(lit) => values_equal(lit, expr),

        // Variable: bind or check literal
        Pattern::Variable(name) => {
            if literals.contains(name) {
                // Literal identifier: check if expr is same identifier
                matches!(expr, Value::Symbol(sym) if sym == name)
            } else {
                // Pattern variable: bind
                bindings.insert(name.clone(), super::BindingValue::Single(expr.clone()));
                true
            }
        }

        // List: match elements
        Pattern::List(patterns) => match expr {
            Value::Pair(_) | Value::Null => {
                let exprs = match collect_list_items(expr) {
                    Ok(items) => items,
                    Err(_) => return false, // Improper list
                };
                match_list_patterns(patterns, &exprs, literals, bindings)
            }
            _ => false,
        },

        // Vector: match elements
        Pattern::Vector(patterns) => match expr {
            Value::Vector(items) => {
                let items = items.borrow();
                match_list_patterns(patterns, &items, literals, bindings)
            }
            _ => false,
        },

        // Ellipsis: complex matching
        Pattern::Ellipsis {
            before,
            repeated,
            after,
        } => match expr {
            Value::Pair(_) | Value::Null => {
                let exprs = match collect_list_items(expr) {
                    Ok(items) => items,
                    Err(_) => return false,
                };
                match_ellipsis_pattern(before, repeated, after, &exprs, literals, bindings)
            }
            _ => false,
        },
    }
}

/// Match a list of patterns against a list of expressions
fn match_list_patterns(
    patterns: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    // Exact length match required
    if patterns.len() != exprs.len() {
        return false;
    }

    for (pattern, expr) in patterns.iter().zip(exprs.iter()) {
        if !match_pattern_impl(pattern, expr, literals, bindings) {
            return false;
        }
    }

    true
}

/// Match ellipsis pattern: (before ... repeated ... after)
fn match_ellipsis_pattern(
    before: &[Pattern],
    repeated: &Pattern,
    after: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    use std::collections::HashMap;

    // Check minimum length
    let min_len = before.len() + after.len();
    if exprs.len() < min_len {
        return false;
    }

    // Match 'before' patterns
    for (i, pattern) in before.iter().enumerate() {
        if !match_pattern_impl(pattern, &exprs[i], literals, bindings) {
            return false;
        }
    }

    // Match 'after' patterns (from end)
    for (i, pattern) in after.iter().enumerate() {
        let expr_idx = exprs.len() - after.len() + i;
        if !match_pattern_impl(pattern, &exprs[expr_idx], literals, bindings) {
            return false;
        }
    }

    // Match repeated pattern (middle section, zero or more times)
    let middle_start = before.len();
    let middle_end = exprs.len() - after.len();
    let middle_exprs = &exprs[middle_start..middle_end];

    // Collect bindings for repeated section
    let mut repeated_bindings: HashMap<Rc<str>, Vec<Value>> = HashMap::new();

    // If there are zero matches, we need to find pattern variables in the repeated pattern
    // and create empty bindings for them
    if middle_exprs.is_empty() {
        // Collect pattern variables from the repeated pattern
        let mut temp_bindings = Bindings::new();
        collect_pattern_variables(repeated, literals, &mut temp_bindings);
        for name in temp_bindings.keys() {
            repeated_bindings.insert(name.clone(), Vec::new());
        }
    } else {
        for expr in middle_exprs {
            let mut temp_bindings = Bindings::new();
            if !match_pattern_impl(repeated, expr, literals, &mut temp_bindings) {
                return false;
            }

            // Accumulate bindings
            for (name, binding) in temp_bindings {
                match binding {
                    super::BindingValue::Single(val) => {
                        repeated_bindings.entry(name).or_default().push(val);
                    }
                    super::BindingValue::Multiple(_) => {
                        // Nested ellipsis - not supported in basic implementation
                        return false;
                    }
                }
            }
        }
    }

    // Insert accumulated bindings as Multiple
    for (name, values) in repeated_bindings {
        bindings.insert(name, super::BindingValue::Multiple(values));
    }

    true
}

/// Helper: Collect all pattern variables from a pattern (for zero-match ellipsis)
/// This creates dummy bindings so we know which variables to create empty Multiple bindings for
fn collect_pattern_variables(pattern: &Pattern, literals: &[Rc<str>], bindings: &mut Bindings) {
    match pattern {
        Pattern::Wildcard | Pattern::Literal(_) => {}
        Pattern::Variable(name) => {
            if !literals.contains(name) {
                // Only collect non-literal pattern variables
                bindings.insert(name.clone(), super::BindingValue::Single(Value::Null));
            }
        }
        Pattern::List(patterns) | Pattern::Vector(patterns) => {
            for p in patterns {
                collect_pattern_variables(p, literals, bindings);
            }
        }
        Pattern::Ellipsis {
            before,
            repeated,
            after,
        } => {
            for p in before {
                collect_pattern_variables(p, literals, bindings);
            }
            collect_pattern_variables(repeated, literals, bindings);
            for p in after {
                collect_pattern_variables(p, literals, bindings);
            }
        }
    }
}

/// Helper: Check if two values are equal for pattern matching
fn values_equal(a: &Value, b: &Value) -> bool {
    use Value::*;

    match (a, b) {
        // Booleans
        (Boolean(a), Boolean(b)) => a == b,

        // Numbers
        (Integer(a), Integer(b)) => a == b,
        (BigInteger(a), BigInteger(b)) => a == b,
        (Rational(a), Rational(b)) => a == b,
        (Real(a), Real(b)) => a == b,
        (Complex(ar, ai), Complex(br, bi)) => ar == br && ai == bi,

        // Characters
        (Character(a), Character(b)) => a == b,

        // Strings
        (String(a), String(b)) => a.borrow().as_str() == b.borrow().as_str(),

        // Symbols
        (Symbol(a), Symbol(b)) => a == b,

        // Null
        (Null, Null) => true,

        // Pairs (structural equality)
        (Pair(a), Pair(b)) => values_equal(&a.0, &b.0) && values_equal(&a.1, &b.1),

        // Vectors
        (Vector(a), Vector(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }

        // Bytevectors
        (Bytevector(a), Bytevector(b)) => a == b,

        // Different types are not equal
        _ => false,
    }
}

/// Helper: Collect items from a list Value into a Vec
fn collect_list_items(expr: &Value) -> Result<Vec<Value>, EvalError> {
    let mut items = Vec::new();
    let mut current = expr;

    loop {
        match current {
            Value::Null => break,
            Value::Pair(pair) => {
                items.push(pair.0.clone());
                current = &pair.1;
            }
            _ => {
                // Improper list (dotted tail that's not null)
                return Err(EvalError::TypeError(format!(
                    "Improper list in pattern matching: {}",
                    expr
                )));
            }
        }
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_system::BindingValue;
    use std::cell::RefCell;

    // Helper to create a list from values
    fn make_list(items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
    }

    #[test]
    fn test_wildcard_matches_anything() {
        let pattern = Pattern::Wildcard;

        assert!(match_pattern(&pattern, &Value::Integer(42), &[]).is_some());
        assert!(match_pattern(&pattern, &Value::Boolean(true), &[]).is_some());
        assert!(match_pattern(&pattern, &Value::Null, &[]).is_some());
    }

    #[test]
    fn test_literal_integer() {
        let pattern = Pattern::Literal(Value::Integer(42));

        assert!(match_pattern(&pattern, &Value::Integer(42), &[]).is_some());
        assert!(match_pattern(&pattern, &Value::Integer(43), &[]).is_none());
    }

    #[test]
    fn test_literal_boolean() {
        let pattern = Pattern::Literal(Value::Boolean(true));

        assert!(match_pattern(&pattern, &Value::Boolean(true), &[]).is_some());
        assert!(match_pattern(&pattern, &Value::Boolean(false), &[]).is_none());
    }

    #[test]
    fn test_literal_symbol() {
        let pattern = Pattern::Literal(Value::Symbol("foo".into()));

        assert!(match_pattern(&pattern, &Value::Symbol("foo".into()), &[]).is_some());
        assert!(match_pattern(&pattern, &Value::Symbol("bar".into()), &[]).is_none());
    }

    #[test]
    fn test_variable_binds_value() {
        let pattern = Pattern::Variable("x".into());
        let expr = Value::Integer(42);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 1);
        assert!(matches!(
            bindings.get(&Rc::from("x")),
            Some(BindingValue::Single(Value::Integer(42)))
        ));
    }

    #[test]
    fn test_literal_identifier_matches() {
        let pattern = Pattern::Variable("else".into());
        let expr = Value::Symbol("else".into());
        let literals = vec![Rc::from("else")];

        let bindings = match_pattern(&pattern, &expr, &literals).unwrap();

        // Literal identifier should match but not bind
        assert_eq!(bindings.len(), 0);
    }

    #[test]
    fn test_literal_identifier_fails_on_different_symbol() {
        let pattern = Pattern::Variable("else".into());
        let expr = Value::Symbol("then".into());
        let literals = vec![Rc::from("else")];

        assert!(match_pattern(&pattern, &expr, &literals).is_none());
    }

    #[test]
    fn test_list_pattern_exact_match() {
        let pattern = Pattern::List(vec![
            Pattern::Variable("op".into()),
            Pattern::Variable("x".into()),
            Pattern::Variable("y".into()),
        ]);

        let expr = make_list(vec![
            Value::Symbol("+".into()),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 3);
        assert!(matches!(
            bindings.get(&Rc::from("op")),
            Some(BindingValue::Single(Value::Symbol(_)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("x")),
            Some(BindingValue::Single(Value::Integer(1)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("y")),
            Some(BindingValue::Single(Value::Integer(2)))
        ));
    }

    #[test]
    fn test_list_pattern_length_mismatch() {
        let pattern = Pattern::List(vec![
            Pattern::Variable("x".into()),
            Pattern::Variable("y".into()),
        ]);

        let expr = make_list(vec![Value::Integer(1)]);

        assert!(match_pattern(&pattern, &expr, &[]).is_none());
    }

    #[test]
    fn test_vector_pattern() {
        let pattern = Pattern::Vector(vec![
            Pattern::Variable("x".into()),
            Pattern::Variable("y".into()),
        ]);

        let expr = Value::Vector(Rc::new(RefCell::new(vec![
            Value::Integer(1),
            Value::Integer(2),
        ])));

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn test_ellipsis_zero_matches() {
        // Pattern: (when test body ...)
        let pattern = Pattern::Ellipsis {
            before: vec![
                Pattern::Variable("keyword".into()),
                Pattern::Variable("test".into()),
            ],
            repeated: Box::new(Pattern::Variable("body".into())),
            after: vec![],
        };

        // Expression: (when #t)
        let expr = make_list(vec![Value::Symbol("when".into()), Value::Boolean(true)]);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        // Should have 3 bindings: keyword, test, and body (empty Multiple)
        assert_eq!(bindings.len(), 3);
        assert!(matches!(
            bindings.get(&Rc::from("keyword")),
            Some(BindingValue::Single(Value::Symbol(_)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("test")),
            Some(BindingValue::Single(Value::Boolean(true)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("body")),
            Some(BindingValue::Multiple(v)) if v.is_empty()
        ));
    }

    #[test]
    fn test_ellipsis_multiple_matches() {
        // Pattern: (when test body ...)
        let pattern = Pattern::Ellipsis {
            before: vec![
                Pattern::Variable("keyword".into()),
                Pattern::Variable("test".into()),
            ],
            repeated: Box::new(Pattern::Variable("body".into())),
            after: vec![],
        };

        // Expression: (when #t (display "hello") (newline))
        let expr = make_list(vec![
            Value::Symbol("when".into()),
            Value::Boolean(true),
            make_list(vec![
                Value::Symbol("display".into()),
                Value::String(Rc::new(RefCell::new("hello".to_string()))),
            ]),
            make_list(vec![Value::Symbol("newline".into())]),
        ]);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 3);
        assert!(matches!(
            bindings.get(&Rc::from("keyword")),
            Some(BindingValue::Single(Value::Symbol(_)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("test")),
            Some(BindingValue::Single(Value::Boolean(true)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("body")),
            Some(BindingValue::Multiple(v)) if v.len() == 2
        ));
    }

    #[test]
    fn test_ellipsis_with_after() {
        // Pattern: (let name (bindings ...) body)
        let pattern = Pattern::Ellipsis {
            before: vec![
                Pattern::Variable("keyword".into()),
                Pattern::Variable("name".into()),
            ],
            repeated: Box::new(Pattern::Variable("binding".into())),
            after: vec![Pattern::Variable("body".into())],
        };

        // Expression: (let loop ((i 0) (sum 0)) result)
        let expr = make_list(vec![
            Value::Symbol("let".into()),
            Value::Symbol("loop".into()),
            make_list(vec![Value::Integer(0)]),
            make_list(vec![Value::Integer(1)]),
            Value::Symbol("result".into()),
        ]);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 4);
        assert!(matches!(
            bindings.get(&Rc::from("binding")),
            Some(BindingValue::Multiple(v)) if v.len() == 2
        ));
        assert!(matches!(
            bindings.get(&Rc::from("body")),
            Some(BindingValue::Single(Value::Symbol(_)))
        ));
    }

    #[test]
    fn test_nested_list_patterns() {
        // Pattern: (let ((var val)) body)
        let pattern = Pattern::List(vec![
            Pattern::Variable("keyword".into()),
            Pattern::List(vec![Pattern::List(vec![
                Pattern::Variable("var".into()),
                Pattern::Variable("val".into()),
            ])]),
            Pattern::Variable("body".into()),
        ]);

        // Expression: (let ((x 42)) (+ x 1))
        let expr = make_list(vec![
            Value::Symbol("let".into()),
            make_list(vec![make_list(vec![
                Value::Symbol("x".into()),
                Value::Integer(42),
            ])]),
            make_list(vec![
                Value::Symbol("+".into()),
                Value::Symbol("x".into()),
                Value::Integer(1),
            ]),
        ]);

        let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

        assert_eq!(bindings.len(), 4);
        assert!(matches!(
            bindings.get(&Rc::from("var")),
            Some(BindingValue::Single(Value::Symbol(_)))
        ));
        assert!(matches!(
            bindings.get(&Rc::from("val")),
            Some(BindingValue::Single(Value::Integer(42)))
        ));
    }
}
