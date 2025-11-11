//! Hygiene support for macro expansion
//!
//! Implements identifier renaming to prevent variable capture.
//! Uses a simplified gensym approach:
//! - Generated identifiers have format: ##original#counter
//! - Counter is globally incremented to ensure uniqueness
//! - Only free identifiers in templates are renamed

use crate::value::Value;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique identifiers
static GENSYM_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Apply hygiene to an expanded macro result
///
/// Renames free identifiers (those not bound by pattern variables)
/// to prevent variable capture. Does NOT rename user-defined macros
/// to allow nested macro calls.
///
/// # Arguments
/// - `expr`: The expanded expression
/// - `pattern_vars`: Set of variables that came from the pattern (do NOT rename these)
/// - `env`: The environment to check for macro bindings
///
/// # Example
/// ```ignore
/// // Macro expands to: (let ((x 1)) ...)
/// // The 'x' introduced by the macro should be renamed to prevent capture
/// let expanded = Value::List(...);
/// let pattern_vars = HashSet::new(); // Empty if 'x' is not from pattern
/// let hygienic = apply_hygiene(&expanded, &pattern_vars, &env);
/// // Result: (let ((##x#0 1)) ...)
/// ```
pub fn apply_hygiene(
    expr: &Value,
    pattern_vars: &std::collections::HashSet<Rc<str>>,
    env: &Rc<crate::env::Environment>,
) -> Value {
    // Find all free identifiers (symbols not in pattern_vars)
    let free_identifiers = find_free_identifiers(expr, pattern_vars, env);

    // Create renaming map: original name -> gensym
    let mut renamings = std::collections::HashMap::new();
    for name in free_identifiers {
        renamings.insert(name.clone(), gensym(&name));
    }

    // Apply renamings to the expression
    rename_identifiers(expr, &renamings)
}

/// Generate a unique identifier based on an original name
///
/// Format: ##original#counter
///
/// # Example
/// ```
/// use patina::macro_system::hygiene::gensym;
/// use std::rc::Rc;
///
/// let sym1 = gensym(&Rc::from("x"));
/// let sym2 = gensym(&Rc::from("x"));
/// // sym1 might be "##x#0", sym2 might be "##x#1"
/// assert_ne!(sym1, sym2);
/// ```
pub fn gensym(base: &Rc<str>) -> Rc<str> {
    let counter = GENSYM_COUNTER.fetch_add(1, Ordering::Relaxed);
    Rc::from(format!("##{base}#{counter}"))
}

/// Check if an identifier is a generated symbol
///
/// Generated symbols have the format ##name#counter
pub fn is_gensym(name: &str) -> bool {
    name.starts_with("##") && name.len() > 2 && name[2..].contains('#')
}

/// Find all free identifiers in an expression
///
/// Free identifiers are symbols that are NOT pattern variables and NOT macros.
/// These are the identifiers introduced by the macro template that should be renamed.
///
/// # Arguments
/// - `expr`: The expression to analyze
/// - `pattern_vars`: Set of pattern variables (to exclude)
/// - `env`: The environment to check for macro bindings
///
/// # Returns
/// Set of free identifier names
fn find_free_identifiers(
    expr: &Value,
    pattern_vars: &std::collections::HashSet<Rc<str>>,
    env: &Rc<crate::env::Environment>,
) -> std::collections::HashSet<Rc<str>> {
    let mut free_ids = std::collections::HashSet::new();
    collect_free_identifiers(expr, pattern_vars, env, &mut free_ids);
    free_ids
}

/// Check if a symbol is a special form keyword
///
/// Special forms are part of the language syntax and should not be renamed.
fn is_special_form(name: &str) -> bool {
    matches!(
        name,
        "quote"
            | "if"
            | "define"
            | "define-syntax"
            | "set!"
            | "lambda"
            | "begin"
            | "cond"
            | "case"
            | "let"
            | "let*"
            | "letrec"
            | "letrec*"
            // Note: let-values and let*-values are now macros (defined in bootstrap.scm)
            | "and"
            | "or"
            | "apply"
            | "call-with-values"  // Special form for tail call optimization
            | "do"
    )
}

/// Check if a symbol is bound to a macro in the environment
///
/// Macros should not be renamed for hygiene, as they need to be
/// expanded during evaluation. This allows nested macro calls to work.
fn is_macro(name: &Rc<str>, env: &Rc<crate::env::Environment>) -> bool {
    if let Some(value) = env.get(name) {
        matches!(value, Value::Macro(_))
    } else {
        false
    }
}

/// Recursively collect free identifiers from an expression
fn collect_free_identifiers(
    expr: &Value,
    pattern_vars: &std::collections::HashSet<Rc<str>>,
    env: &Rc<crate::env::Environment>,
    free_ids: &mut std::collections::HashSet<Rc<str>>,
) {
    match expr {
        Value::Symbol(name) => {
            // Don't rename if:
            // 1. It's a pattern variable
            // 2. It's already a gensym
            // 3. It's a special form keyword
            // 4. It's a macro keyword (allows nested macro calls)
            if !pattern_vars.contains(name)
                && !is_gensym(name.as_ref())
                && !is_special_form(name.as_ref())
                && !is_macro(name, env)
            {
                free_ids.insert(name.clone());
            }
        }
        Value::Pair(pair) => {
            // Check if this is a quote form - if so, don't recurse into it
            // Quote forms are literal data and should not have identifiers renamed
            if let Value::Symbol(sym) = &pair.0 {
                if sym.as_ref() == "quote" {
                    return; // Don't collect identifiers inside quote
                }
            }

            collect_free_identifiers(&pair.0, pattern_vars, env, free_ids);
            collect_free_identifiers(&pair.1, pattern_vars, env, free_ids);
        }
        Value::Vector(vec) => {
            for item in vec.borrow().iter() {
                collect_free_identifiers(item, pattern_vars, env, free_ids);
            }
        }
        // All other values (numbers, strings, etc.) have no identifiers
        _ => {}
    }
}

/// Rename identifiers in an expression according to a renaming map
///
/// # Arguments
/// - `expr`: The expression to process
/// - `renamings`: Map from original names to renamed versions
///
/// # Returns
/// New expression with renamings applied
fn rename_identifiers(
    expr: &Value,
    renamings: &std::collections::HashMap<Rc<str>, Rc<str>>,
) -> Value {
    match expr {
        Value::Symbol(name) => {
            // If this symbol should be renamed, use the renamed version
            if let Some(renamed) = renamings.get(name) {
                Value::Symbol(renamed.clone())
            } else {
                expr.clone()
            }
        }
        Value::Pair(pair) => {
            // Check if this is a quote form - if so, don't rename inside it
            // Quote forms are literal data and should not be modified
            if let Value::Symbol(sym) = &pair.0 {
                if sym.as_ref() == "quote" {
                    return expr.clone(); // Return quote form unchanged
                }
            }

            let car = rename_identifiers(&pair.0, renamings);
            let cdr = rename_identifiers(&pair.1, renamings);
            Value::Pair(Rc::new((car, cdr)))
        }
        Value::Vector(vec) => {
            let renamed_items: Vec<Value> = vec
                .borrow()
                .iter()
                .map(|item| rename_identifiers(item, renamings))
                .collect();
            Value::Vector(Rc::new(std::cell::RefCell::new(renamed_items)))
        }
        // All other values are returned as-is
        _ => expr.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: create empty environment for tests
    fn empty_env() -> Rc<crate::env::Environment> {
        Rc::new(crate::env::Environment::new())
    }

    #[test]
    fn test_gensym_uniqueness() {
        let base = Rc::from("test");
        let sym1 = gensym(&base);
        let sym2 = gensym(&base);
        assert_ne!(sym1, sym2);
        assert!(sym1.starts_with("##test#"));
        assert!(sym2.starts_with("##test#"));
    }

    #[test]
    fn test_is_gensym() {
        assert!(is_gensym("##x#0"));
        assert!(is_gensym("##test#42"));
        assert!(!is_gensym("x"));
        assert!(!is_gensym("#x"));
        assert!(!is_gensym("##x"));
    }

    #[test]
    fn test_find_free_identifiers_empty() {
        // Expression with no symbols
        let expr = Value::Integer(42);
        let pattern_vars = std::collections::HashSet::new();
        let env = empty_env();

        let free_ids = find_free_identifiers(&expr, &pattern_vars, &env);

        assert!(free_ids.is_empty());
    }

    #[test]
    fn test_find_free_identifiers_all_free() {
        // Expression: (foo ((x 1)) x)
        // Pattern vars: none
        // All symbols should be free (except special forms)
        let expr = list_from_vec(vec![
            Value::Symbol(Rc::from("foo")),
            list_from_vec(vec![list_from_vec(vec![
                Value::Symbol(Rc::from("x")),
                Value::Integer(1),
            ])]),
            Value::Symbol(Rc::from("x")),
        ]);

        let pattern_vars = std::collections::HashSet::new();
        let env = empty_env();
        let free_ids = find_free_identifiers(&expr, &pattern_vars, &env);

        assert!(free_ids.contains(&Rc::from("foo")));
        assert!(free_ids.contains(&Rc::from("x")));
        assert_eq!(free_ids.len(), 2); // foo and x
    }

    #[test]
    fn test_find_free_identifiers_with_pattern_vars() {
        // Expression: (foo test body)
        // Pattern vars: {test, body}
        // Only 'foo' should be free
        let expr = list_from_vec(vec![
            Value::Symbol(Rc::from("foo")),
            Value::Symbol(Rc::from("test")),
            Value::Symbol(Rc::from("body")),
        ]);

        let mut pattern_vars = std::collections::HashSet::new();
        pattern_vars.insert(Rc::from("test"));
        pattern_vars.insert(Rc::from("body"));
        let env = empty_env();

        let free_ids = find_free_identifiers(&expr, &pattern_vars, &env);

        assert!(free_ids.contains(&Rc::from("foo")));
        assert!(!free_ids.contains(&Rc::from("test")));
        assert!(!free_ids.contains(&Rc::from("body")));
        assert_eq!(free_ids.len(), 1); // only 'foo'
    }

    #[test]
    fn test_find_free_identifiers_with_gensyms() {
        // Expression contains already-gensymed symbols - should be excluded
        let expr = list_from_vec(vec![
            Value::Symbol(Rc::from("foo")),
            Value::Symbol(Rc::from("##temp#0")),
        ]);

        let pattern_vars = std::collections::HashSet::new();
        let env = empty_env();
        let free_ids = find_free_identifiers(&expr, &pattern_vars, &env);

        assert!(free_ids.contains(&Rc::from("foo")));
        assert!(!free_ids.contains(&Rc::from("##temp#0")));
        assert_eq!(free_ids.len(), 1);
    }

    #[test]
    fn test_rename_identifiers_simple() {
        // Expression: (let x)
        // Rename: {let -> ##let#0}
        let expr = list_from_vec(vec![
            Value::Symbol(Rc::from("let")),
            Value::Symbol(Rc::from("x")),
        ]);

        let mut renamings = std::collections::HashMap::new();
        renamings.insert(Rc::from("let"), Rc::from("##let#0"));

        let renamed = rename_identifiers(&expr, &renamings);

        // Check that 'let' was renamed but 'x' was not
        if let Value::Pair(pair) = renamed {
            assert!(matches!(pair.0, Value::Symbol(ref s) if s.as_ref() == "##let#0"));
            if let Value::Pair(inner) = &pair.1 {
                assert!(matches!(inner.0, Value::Symbol(ref s) if s.as_ref() == "x"));
            }
        } else {
            panic!("Expected pair");
        }
    }

    #[test]
    fn test_apply_hygiene_simple() {
        // Expression: (foo test body)
        // Pattern vars: {test, body}
        // Should rename 'foo' to ##foo#N, but not test/body
        let expr = list_from_vec(vec![
            Value::Symbol(Rc::from("foo")),
            Value::Symbol(Rc::from("test")),
            Value::Symbol(Rc::from("body")),
        ]);

        let mut pattern_vars = std::collections::HashSet::new();
        pattern_vars.insert(Rc::from("test"));
        pattern_vars.insert(Rc::from("body"));
        let env = empty_env();

        let hygienic = apply_hygiene(&expr, &pattern_vars, &env);

        // Extract first symbol - should be gensym
        if let Value::Pair(pair) = hygienic {
            if let Value::Symbol(sym) = &pair.0 {
                assert!(is_gensym(sym.as_ref()));
                assert!(sym.starts_with("##foo#"));
            } else {
                panic!("Expected symbol");
            }

            // Check that pattern vars weren't renamed
            if let Value::Pair(inner1) = &pair.1 {
                assert!(matches!(inner1.0, Value::Symbol(ref s) if s.as_ref() == "test"));
                if let Value::Pair(inner2) = &inner1.1 {
                    assert!(matches!(inner2.0, Value::Symbol(ref s) if s.as_ref() == "body"));
                }
            }
        } else {
            panic!("Expected pair");
        }
    }

    // Helper function for building lists in tests
    fn list_from_vec(items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
    }
}
