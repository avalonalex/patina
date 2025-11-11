//! Template expansion for macro output generation
//!
//! Implements R7RS template expansion with support for:
//! - Literal values
//! - Pattern variable substitution
//! - Ellipsis expansion (repeating templates)
//! - Ellipsis escaping (for literal `...`)

use super::{Bindings, Template};
use patina_runtime::Value;
use crate::error::FrontendError;
use std::rc::Rc;

/// Expand a template using pattern variable bindings
///
/// Returns the generated expression.
///
/// # Arguments
/// - `template`: The template to expand
/// - `bindings`: Variable bindings from pattern matching
///
/// # Example
/// ```ignore
/// let template = Template::List(vec![
///     Template::Literal(Value::Symbol("if".into())),
///     Template::Variable("test".into()),
///     Template::List(vec![
///         Template::Literal(Value::Symbol("begin".into())),
///         // ... body templates
///     ]),
/// ]);
/// let mut bindings = Bindings::new();
/// bindings.insert("test".into(), BindingValue::Single(Value::Boolean(true)));
/// let result = expand_template(&template, &bindings).unwrap();
/// ```
pub fn expand_template(template: &Template, bindings: &Bindings) -> Result<Value, crate::error::FrontendError> {
    expand_template_impl(template, bindings, 0)
}

/// Internal template expansion implementation
///
/// # Arguments
/// - `template`: The template to expand
/// - `bindings`: Variable bindings from pattern matching
/// - `depth`: Current ellipsis depth (for nested ellipsis)
#[allow(clippy::only_used_in_recursion)]
fn expand_template_impl(
    template: &Template,
    bindings: &Bindings,
    depth: usize,
) -> Result<Value, crate::error::FrontendError> {
    match template {
        // Literal: return as-is
        Template::Literal(val) => Ok(val.clone()),

        // Variable: substitute from bindings
        Template::Variable(name) => match bindings.get(name) {
            Some(super::BindingValue::Single(val)) => Ok(val.clone()),
            Some(super::BindingValue::Multiple(_)) => Err(FrontendError::InvalidSyntax(format!(
                "Pattern variable {} used outside ellipsis context",
                name
            ))),
            None => {
                // Not a pattern variable - treat as literal identifier
                // This will be renamed for hygiene later
                Ok(Value::Symbol(name.clone()))
            }
        },

        // List: expand elements
        Template::List(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_impl(t, bindings, depth)?);
            }
            Ok(list_from_vec(expanded))
        }

        // Vector: expand elements
        Template::Vector(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_impl(t, bindings, depth)?);
            }
            Ok(Value::Vector(Rc::new(std::cell::RefCell::new(expanded))))
        }

        // Dotted list: expand elements with dotted tail
        Template::DottedList { templates, tail } => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_impl(t, bindings, depth)?);
            }
            let tail_value = expand_template_impl(tail, bindings, depth)?;
            // Build improper list: (a b . tail)
            Ok(build_dotted_list(&expanded, tail_value))
        }

        // Ellipsis: repeat expansion
        Template::Ellipsis {
            before,
            repeated,
            after,
        } => {
            let mut result = Vec::new();

            // Expand 'before' templates
            for t in before {
                result.push(expand_template_impl(t, bindings, depth)?);
            }

            // Find pattern variables in repeated template that are actually bound
            let vars = find_pattern_vars_in_bindings(repeated, bindings);

            if vars.is_empty() {
                return Err(FrontendError::InvalidSyntax(
                    "Ellipsis template contains no pattern variables".to_string(),
                ));
            }

            // Get length of repeated section (all vars should have same length)
            let repeat_count = match bindings.get(&vars[0]) {
                Some(super::BindingValue::Multiple(values)) => values.len(),
                Some(super::BindingValue::Single(_)) => {
                    return Err(FrontendError::InvalidSyntax(format!(
                        "Pattern variable {} used with ellipsis but not bound with ellipsis",
                        vars[0]
                    )));
                }
                None => {
                    return Err(FrontendError::InvalidSyntax(format!(
                        "Unbound pattern variable: {}",
                        vars[0]
                    )));
                }
            };

            // Verify all variables have same length
            for var in &vars[1..] {
                match bindings.get(var) {
                    Some(super::BindingValue::Multiple(values)) if values.len() == repeat_count => {
                    }
                    _ => {
                        return Err(FrontendError::InvalidSyntax(
                            "Pattern variables in ellipsis have different lengths".to_string(),
                        ));
                    }
                }
            }

            // Expand repeated template for each iteration
            for i in 0..repeat_count {
                // Create temporary bindings for this iteration
                let mut iter_bindings = bindings.clone();

                for var in &vars {
                    if let Some(super::BindingValue::Multiple(values)) = bindings.get(var) {
                        iter_bindings
                            .insert(var.clone(), super::BindingValue::Single(values[i].clone()));
                    }
                }

                result.push(expand_template_impl(repeated, &iter_bindings, depth + 1)?);
            }

            // Expand 'after' templates
            for t in after {
                let expanded = expand_template_impl(t, bindings, depth)?;
                // If the after template is itself an ellipsis that expands to a list,
                // we need to splice its elements in, not add it as a nested list
                if matches!(t, Template::Ellipsis { .. }) {
                    // Flatten the expanded list into our result
                    match expanded {
                        Value::Null => {} // Empty list, nothing to add
                        Value::Pair(_) => {
                            // Convert list to vec and append all elements
                            if let Ok(items) = list_to_vec(&expanded) {
                                result.extend(items);
                            } else {
                                // Improper list, just push as-is
                                result.push(expanded);
                            }
                        }
                        other => result.push(other), // Not a list, push as-is
                    }
                } else {
                    result.push(expanded);
                }
            }

            Ok(list_from_vec(result))
        }

        // Ellipsis escape: (... template) → literal ...
        Template::EllipsisEscape(inner) => {
            // Return the literal ellipsis symbol followed by expanded template
            let ellipsis = Value::Symbol("...".into());
            let expanded = expand_template_impl(inner, bindings, depth)?;
            Ok(list_from_vec(vec![ellipsis, expanded]))
        }
    }
}

/// Find all pattern variables in a template that are actually bound
/// This filters out literal identifiers that aren't in the bindings
fn find_pattern_vars_in_bindings(template: &Template, bindings: &Bindings) -> Vec<Rc<str>> {
    let mut vars = Vec::new();
    find_pattern_vars_impl(template, &mut vars);
    // Filter to only include variables that are actually in bindings
    vars.into_iter()
        .filter(|v| bindings.contains_key(v))
        .collect()
}

/// Find all pattern variables in a template (without filtering)
#[allow(dead_code)]
fn find_pattern_vars(template: &Template) -> Vec<Rc<str>> {
    let mut vars = Vec::new();
    find_pattern_vars_impl(template, &mut vars);
    vars
}

fn find_pattern_vars_impl(template: &Template, vars: &mut Vec<Rc<str>>) {
    match template {
        Template::Variable(name) => {
            if !vars.contains(name) {
                vars.push(name.clone());
            }
        }
        Template::List(templates) | Template::Vector(templates) => {
            for t in templates {
                find_pattern_vars_impl(t, vars);
            }
        }
        Template::Ellipsis {
            before: _,
            repeated,
            after: _,
        } => {
            // Only collect vars from the repeated section
            // (before/after are not repeated)
            find_pattern_vars_impl(repeated, vars);
        }
        Template::EllipsisEscape(inner) => {
            find_pattern_vars_impl(inner, vars);
        }
        Template::DottedList { templates, tail } => {
            for t in templates {
                find_pattern_vars_impl(t, vars);
            }
            find_pattern_vars_impl(tail, vars);
        }
        Template::Literal(_) => {}
    }
}

/// Build a dotted list from values and a tail
fn build_dotted_list(items: &[Value], tail: Value) -> Value {
    items
        .iter()
        .rev()
        .fold(tail, |acc, val| Value::Pair(Rc::new((val.clone(), acc))))
}

/// Helper: Convert a Vec of Values into a Scheme list (chain of pairs)
fn list_from_vec(items: Vec<Value>) -> Value {
    items
        .into_iter()
        .rev()
        .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
}

/// Helper: Convert a Scheme list into a Vec of Values
fn list_to_vec(value: &Value) -> Result<Vec<Value>, ()> {
    let mut items = Vec::new();
    let mut current = value;

    loop {
        match current {
            Value::Null => return Ok(items),
            Value::Pair(pair) => {
                items.push(pair.0.clone());
                current = &pair.1;
            }
            _ => return Err(()), // Improper list
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::BindingValue;
    use std::cell::RefCell;

    // Helper to create a list from values
    fn make_list(items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
    }

    // Helper to compare two values for equality
    fn values_equal(a: &Value, b: &Value) -> bool {
        format!("{}", a) == format!("{}", b)
    }

    #[test]
    fn test_literal_expansion() {
        let template = Template::Literal(Value::Integer(42));
        let bindings = Bindings::new();

        let result = expand_template(&template, &bindings).unwrap();

        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_variable_substitution() {
        let template = Template::Variable("x".into());
        let mut bindings = Bindings::new();
        bindings.insert("x".into(), BindingValue::Single(Value::Integer(42)));

        let result = expand_template(&template, &bindings).unwrap();

        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_unbound_variable_as_symbol() {
        let template = Template::Variable("if".into());
        let bindings = Bindings::new();

        let result = expand_template(&template, &bindings).unwrap();

        assert!(matches!(result, Value::Symbol(s) if s.as_ref() == "if"));
    }

    #[test]
    fn test_list_template() {
        // Template: (if test body)
        let template = Template::List(vec![
            Template::Variable("if".into()),
            Template::Variable("test".into()),
            Template::Variable("body".into()),
        ]);

        let mut bindings = Bindings::new();
        bindings.insert("test".into(), BindingValue::Single(Value::Boolean(true)));
        bindings.insert("body".into(), BindingValue::Single(Value::Integer(42)));

        let result = expand_template(&template, &bindings).unwrap();

        let expected = make_list(vec![
            Value::Symbol("if".into()),
            Value::Boolean(true),
            Value::Integer(42),
        ]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_vector_template() {
        let template = Template::Vector(vec![
            Template::Variable("x".into()),
            Template::Variable("y".into()),
        ]);

        let mut bindings = Bindings::new();
        bindings.insert("x".into(), BindingValue::Single(Value::Integer(1)));
        bindings.insert("y".into(), BindingValue::Single(Value::Integer(2)));

        let result = expand_template(&template, &bindings).unwrap();

        match result {
            Value::Vector(v) => {
                let v = v.borrow();
                assert_eq!(v.len(), 2);
                assert!(matches!(v[0], Value::Integer(1)));
                assert!(matches!(v[1], Value::Integer(2)));
            }
            _ => panic!("Expected vector"),
        }
    }

    #[test]
    fn test_ellipsis_zero_matches() {
        // Template: (begin body ...)
        let template = Template::Ellipsis {
            before: vec![Template::Variable("begin".into())],
            repeated: Box::new(Template::Variable("body".into())),
            after: vec![],
        };

        let mut bindings = Bindings::new();
        bindings.insert("body".into(), BindingValue::Multiple(vec![]));

        let result = expand_template(&template, &bindings).unwrap();

        // Should expand to just (begin)
        let expected = make_list(vec![Value::Symbol("begin".into())]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_ellipsis_single_match() {
        // Template: (begin body ...)
        let template = Template::Ellipsis {
            before: vec![Template::Variable("begin".into())],
            repeated: Box::new(Template::Variable("body".into())),
            after: vec![],
        };

        let mut bindings = Bindings::new();
        bindings.insert(
            "body".into(),
            BindingValue::Multiple(vec![Value::Integer(42)]),
        );

        let result = expand_template(&template, &bindings).unwrap();

        let expected = make_list(vec![Value::Symbol("begin".into()), Value::Integer(42)]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_ellipsis_multiple_matches() {
        // Template: (begin body ...)
        let template = Template::Ellipsis {
            before: vec![Template::Variable("begin".into())],
            repeated: Box::new(Template::Variable("body".into())),
            after: vec![],
        };

        let mut bindings = Bindings::new();
        bindings.insert(
            "body".into(),
            BindingValue::Multiple(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ]),
        );

        let result = expand_template(&template, &bindings).unwrap();

        let expected = make_list(vec![
            Value::Symbol("begin".into()),
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_ellipsis_with_after() {
        // Template: (let ((var val) ...) body)
        let template = Template::Ellipsis {
            before: vec![Template::Variable("let".into())],
            repeated: Box::new(Template::List(vec![
                Template::Variable("var".into()),
                Template::Variable("val".into()),
            ])),
            after: vec![Template::Variable("body".into())],
        };

        let mut bindings = Bindings::new();
        bindings.insert(
            "var".into(),
            BindingValue::Multiple(vec![Value::Symbol("x".into()), Value::Symbol("y".into())]),
        );
        bindings.insert(
            "val".into(),
            BindingValue::Multiple(vec![Value::Integer(1), Value::Integer(2)]),
        );
        bindings.insert("body".into(), BindingValue::Single(Value::Integer(99)));

        let result = expand_template(&template, &bindings).unwrap();

        let expected = make_list(vec![
            Value::Symbol("let".into()),
            make_list(vec![Value::Symbol("x".into()), Value::Integer(1)]),
            make_list(vec![Value::Symbol("y".into()), Value::Integer(2)]),
            Value::Integer(99),
        ]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_when_macro_expansion() {
        // Template: (if test (begin body ...))
        // The inner (begin body ...) is itself an Ellipsis template
        let template = Template::List(vec![
            Template::Variable("if".into()),
            Template::Variable("test".into()),
            Template::Ellipsis {
                before: vec![Template::Variable("begin".into())],
                repeated: Box::new(Template::Variable("body".into())),
                after: vec![],
            },
        ]);

        let mut bindings = Bindings::new();
        bindings.insert("test".into(), BindingValue::Single(Value::Boolean(true)));
        bindings.insert(
            "body".into(),
            BindingValue::Multiple(vec![
                make_list(vec![
                    Value::Symbol("display".into()),
                    Value::String(Rc::new(RefCell::new("hello".to_string()))),
                ]),
                make_list(vec![Value::Symbol("newline".into())]),
            ]),
        );

        let result = expand_template(&template, &bindings).unwrap();

        // Should expand to: (if #t (begin (display "hello") (newline)))
        let expected = make_list(vec![
            Value::Symbol("if".into()),
            Value::Boolean(true),
            make_list(vec![
                Value::Symbol("begin".into()),
                make_list(vec![
                    Value::Symbol("display".into()),
                    Value::String(Rc::new(RefCell::new("hello".to_string()))),
                ]),
                make_list(vec![Value::Symbol("newline".into())]),
            ]),
        ]);

        if !values_equal(&result, &expected) {
            println!("Result:   {}", result);
            println!("Expected: {}", expected);
        }
        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_ellipsis_escape() {
        // Template: (... x)
        let template = Template::EllipsisEscape(Box::new(Template::Variable("x".into())));

        let mut bindings = Bindings::new();
        bindings.insert("x".into(), BindingValue::Single(Value::Integer(42)));

        let result = expand_template(&template, &bindings).unwrap();

        // Should expand to: (... 42)
        let expected = make_list(vec![Value::Symbol("...".into()), Value::Integer(42)]);

        assert!(values_equal(&result, &expected));
    }

    #[test]
    fn test_error_multiple_outside_ellipsis() {
        let template = Template::Variable("x".into());
        let mut bindings = Bindings::new();
        bindings.insert("x".into(), BindingValue::Multiple(vec![Value::Integer(1)]));

        let result = expand_template(&template, &bindings);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("outside ellipsis context"));
    }

    #[test]
    fn test_error_mismatched_lengths() {
        // Template with two variables in ellipsis
        let template = Template::Ellipsis {
            before: vec![],
            repeated: Box::new(Template::List(vec![
                Template::Variable("x".into()),
                Template::Variable("y".into()),
            ])),
            after: vec![],
        };

        let mut bindings = Bindings::new();
        bindings.insert(
            "x".into(),
            BindingValue::Multiple(vec![Value::Integer(1), Value::Integer(2)]),
        );
        bindings.insert(
            "y".into(),
            BindingValue::Multiple(vec![Value::Integer(3)]), // Different length!
        );

        let result = expand_template(&template, &bindings);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("different lengths"));
    }
}
