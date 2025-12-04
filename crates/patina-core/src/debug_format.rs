//! Debug formatting utilities for macro hygiene debugging
//!
//! This module provides scope-aware formatting of Values to help debug macro
//! expansion and hygiene issues. When enabled, identifiers are displayed with
//! their scope sets, making it possible to trace how hygiene is being applied.
//!
//! ## Example Output
//!
//! Normal display: `(let ((temp x)) (if temp temp (my-or y)))`
//!
//! Debug display:  `(let{S3} ((temp{S3} x{S1,S2})) (if{S3} temp{S3} temp{S3} (my-or y{S1,S2})))`
//!
//! This makes it clear which identifiers came from the macro template (have S3)
//! versus which came from the use site (have S1,S2 but not S3).

use crate::value::Value;
use std::fmt::Write;

/// Format a Value with full scope information for debugging
///
/// Unlike the normal Display implementation which hides scopes,
/// this shows the scope set for each Identifier, making hygiene visible.
pub fn format_with_scopes(value: &Value) -> String {
    let mut buf = String::new();
    format_value_with_scopes(value, &mut buf);
    buf
}

/// Internal recursive formatter
fn format_value_with_scopes(value: &Value, buf: &mut String) {
    match value {
        Value::Symbol(s) => {
            // Symbols have no scopes - just print the name
            buf.push_str(s);
        }
        Value::Identifier(id) => {
            // Show identifier with its scope set
            buf.push_str(&id.name);
            if !id.scopes.is_empty() {
                write!(buf, "{}", id.scopes).unwrap();
            }
        }
        Value::Pair(_) => {
            // Format as list, handling proper lists and dotted lists
            buf.push('(');
            format_list_with_scopes(value, buf);
            buf.push(')');
        }
        Value::Null => {
            buf.push_str("()");
        }
        Value::Vector(v) => {
            buf.push_str("#(");
            let vec = v.borrow();
            for (i, elem) in vec.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                format_value_with_scopes(elem, buf);
            }
            buf.push(')');
        }
        Value::Boolean(b) => {
            buf.push_str(if *b { "#t" } else { "#f" });
        }
        Value::Integer(n) => {
            write!(buf, "{}", n).unwrap();
        }
        Value::BigInteger(n) => {
            write!(buf, "{}", n).unwrap();
        }
        Value::Rational(r) => {
            write!(buf, "{}", r).unwrap();
        }
        Value::Real(r) => {
            write!(buf, "{}", r).unwrap();
        }
        Value::Complex(_) => {
            // Use the Display impl which handles the new boxed format
            write!(buf, "{}", value).unwrap();
        }
        Value::Character(c) => {
            write!(buf, "#\\{}", c).unwrap();
        }
        Value::String(s) => {
            write!(buf, "\"{}\"", s.borrow()).unwrap();
        }
        Value::Procedure(proc) => {
            write!(buf, "#<procedure {:?}>", proc).unwrap();
        }
        Value::Macro(m) => {
            write!(buf, "#<macro {}>", m.name).unwrap();
        }
        // For other types, use their default display
        other => {
            write!(buf, "{}", other).unwrap();
        }
    }
}

/// Format list contents, handling dotted lists
fn format_list_with_scopes(value: &Value, buf: &mut String) {
    let mut current = value.clone();
    let mut first = true;

    loop {
        match current {
            Value::Null => break,
            Value::Pair(p) => {
                let borrowed = p.borrow();
                if !first {
                    buf.push(' ');
                }
                first = false;
                format_value_with_scopes(&borrowed.0, buf);
                current = borrowed.1.clone();
            }
            // Dotted list tail
            other => {
                buf.push_str(" . ");
                format_value_with_scopes(&other, buf);
                break;
            }
        }
    }
}

/// Format a Value showing scope changes from a flip operation
///
/// This is useful for debugging the flip-scope operation:
/// - Shows identifiers that had the scope ADDED (weren't in original, now have it)
/// - Shows identifiers that had the scope REMOVED (were in original, now don't)
pub fn format_with_scope_diff(value: &Value, scope: crate::scope::ScopeId, action: &str) -> String {
    let mut buf = String::new();
    write!(
        buf,
        "[{}] Scope {} on: {}",
        action,
        scope,
        format_with_scopes(value)
    )
    .unwrap();
    buf
}

/// Print a detailed expansion step showing before/after with scopes
pub fn print_expansion_step(
    step_name: &str,
    macro_name: &str,
    macro_scope: crate::scope::ScopeId,
    before: &Value,
    after: &Value,
) {
    println!(
        "[MACRO-DEBUG] === {} (macro: {}) ===",
        step_name, macro_name
    );
    println!("[MACRO-DEBUG]   Macro scope: {}", macro_scope);
    println!("[MACRO-DEBUG]   Before: {}", format_with_scopes(before));
    println!("[MACRO-DEBUG]   After:  {}", format_with_scopes(after));
    println!("[MACRO-DEBUG]");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{ScopeId, ScopeSet};
    use crate::value::IdentifierData;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_format_symbol() {
        let val = Value::Symbol("foo".into());
        assert_eq!(format_with_scopes(&val), "foo");
    }

    #[test]
    fn test_format_identifier_no_scopes() {
        let val = Value::Identifier(Box::new(IdentifierData {
            name: "bar".into(),
            scopes: ScopeSet::new(),
        }));
        assert_eq!(format_with_scopes(&val), "bar");
    }

    #[test]
    fn test_format_identifier_with_scopes() {
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let val = Value::Identifier(Box::new(IdentifierData {
            name: "x".into(),
            scopes: ScopeSet::from_iter([s1, s2]),
        }));
        assert_eq!(format_with_scopes(&val), "x{S1, S2}");
    }

    #[test]
    fn test_format_list_with_scopes() {
        let s1 = ScopeId(1);

        // Build (let{S1} ((temp{S1} 42)))
        let let_id = Value::Identifier(Box::new(IdentifierData {
            name: "let".into(),
            scopes: ScopeSet::singleton(s1),
        }));
        let temp_id = Value::Identifier(Box::new(IdentifierData {
            name: "temp".into(),
            scopes: ScopeSet::singleton(s1),
        }));
        let forty_two = Value::Integer(42);

        // (temp{S1} 42)
        let binding = Value::Pair(Rc::new(RefCell::new((
            temp_id,
            Value::Pair(Rc::new(RefCell::new((forty_two, Value::Null)))),
        ))));

        // ((temp{S1} 42))
        let bindings = Value::Pair(Rc::new(RefCell::new((binding, Value::Null))));

        // (let{S1} ((temp{S1} 42)))
        let expr = Value::Pair(Rc::new(RefCell::new((
            let_id,
            Value::Pair(Rc::new(RefCell::new((bindings, Value::Null)))),
        ))));

        let result = format_with_scopes(&expr);
        assert_eq!(result, "(let{S1} ((temp{S1} 42)))");
    }
}
