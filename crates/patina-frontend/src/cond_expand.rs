//! Feature requirement evaluation for cond-expand
//!
//! R7RS §4.2.1 and §5.6.1 define `cond-expand` which uses feature requirements
//! to conditionally include code.
//!
//! Feature requirements can be:
//! - `<identifier>` - true if the feature is supported
//! - `(library <library-name>)` - true if the library can be loaded
//! - `(and <req> ...)` - true if all requirements are true
//! - `(or <req> ...)` - true if any requirement is true
//! - `(not <req>)` - true if the requirement is false

use crate::ParseError;
use patina_runtime::Value;
use patina_runtime::features::FeatureRegistry;

/// Evaluate a feature requirement.
///
/// Returns true if the requirement is satisfied.
///
/// The `can_load_library` callback is used to check if a library can be loaded.
/// This allows the evaluator to provide library checking without creating a
/// circular dependency.
pub fn evaluate_feature_requirement<F>(
    req: &Value,
    features: &FeatureRegistry,
    can_load_library: &F,
) -> Result<bool, ParseError>
where
    F: Fn(&[String]) -> bool,
{
    match req {
        // Simple feature identifier
        Value::Symbol(name) => Ok(features.has_feature(name)),

        // Compound requirement
        Value::Pair(_) => {
            let list = value_to_vec(req)?;
            if list.is_empty() {
                return Err(ParseError::InvalidSyntax(
                    "Empty feature requirement".to_string(),
                ));
            }

            match list[0].as_symbol() {
                Some("and") => {
                    // (and req1 req2 ...) - all must be true
                    for sub_req in &list[1..] {
                        if !evaluate_feature_requirement(sub_req, features, can_load_library)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }

                Some("or") => {
                    // (or req1 req2 ...) - any must be true
                    for sub_req in &list[1..] {
                        if evaluate_feature_requirement(sub_req, features, can_load_library)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }

                Some("not") => {
                    // (not req) - must be false
                    if list.len() != 2 {
                        return Err(ParseError::InvalidSyntax(
                            "not requires exactly one argument".to_string(),
                        ));
                    }
                    Ok(!evaluate_feature_requirement(
                        &list[1],
                        features,
                        can_load_library,
                    )?)
                }

                Some("library") => {
                    // (library <library-name>) - check if library exists
                    if list.len() != 2 {
                        return Err(ParseError::InvalidSyntax(
                            "library requires exactly one argument".to_string(),
                        ));
                    }
                    let lib_name = parse_library_name(&list[1])?;
                    Ok(can_load_library(&lib_name))
                }

                _ => Err(ParseError::InvalidSyntax(format!(
                    "Unknown feature requirement operator: {}",
                    list[0]
                ))),
            }
        }

        _ => Err(ParseError::InvalidSyntax(format!(
            "Invalid feature requirement: {}",
            req
        ))),
    }
}

/// Helper to get symbol from Value
trait AsSymbol {
    fn as_symbol(&self) -> Option<&str>;
}

impl AsSymbol for Value {
    fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Symbol(s) => Some(s),
            _ => None,
        }
    }
}

/// Convert a Value list to Vec
fn value_to_vec(value: &Value) -> Result<Vec<Value>, ParseError> {
    let mut items = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => return Ok(items),
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                items.push(borrowed.0.clone());
                current = borrowed.1.clone();
            }
            _ => {
                return Err(ParseError::InvalidSyntax(format!(
                    "Expected proper list, got improper list ending with: {}",
                    current
                )));
            }
        }
    }
}

/// Parse a library name from a Value
fn parse_library_name(value: &Value) -> Result<Vec<String>, ParseError> {
    let list = value_to_vec(value)?;

    if list.is_empty() {
        return Err(ParseError::InvalidSyntax(
            "Library name cannot be empty".to_string(),
        ));
    }

    let mut name = Vec::new();
    for part in list {
        match &part {
            Value::Symbol(s) => name.push(s.to_string()),
            Value::Integer(n) if *n >= 0 => name.push(n.to_string()),
            _ => {
                return Err(ParseError::InvalidSyntax(format!(
                    "Library name parts must be identifiers or non-negative integers, got: {}",
                    part
                )));
            }
        }
    }

    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_runtime::FeatureRegistry;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn symbol(s: &str) -> Value {
        Value::Symbol(s.into())
    }

    fn list(items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::Null, |acc, item| {
            Value::Pair(Rc::new(RefCell::new((item, acc))))
        })
    }

    fn integer(n: i64) -> Value {
        Value::Integer(n)
    }

    #[test]
    fn test_simple_feature() {
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // r7rs should be present
        assert!(evaluate_feature_requirement(&symbol("r7rs"), &features, &no_load).unwrap());

        // patina should be present
        assert!(evaluate_feature_requirement(&symbol("patina"), &features, &no_load).unwrap());

        // nonexistent should be false
        assert!(
            !evaluate_feature_requirement(&symbol("nonexistent"), &features, &no_load).unwrap()
        );
    }

    #[test]
    fn test_and_requirement() {
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (and r7rs patina) should be true
        let req = list(vec![symbol("and"), symbol("r7rs"), symbol("patina")]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (and r7rs nonexistent) should be false
        let req = list(vec![symbol("and"), symbol("r7rs"), symbol("nonexistent")]);
        assert!(!evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (and) with no args should be true (vacuous truth)
        let req = list(vec![symbol("and")]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());
    }

    #[test]
    fn test_or_requirement() {
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (or nonexistent r7rs) should be true
        let req = list(vec![symbol("or"), symbol("nonexistent"), symbol("r7rs")]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (or nonexistent1 nonexistent2) should be false
        let req = list(vec![
            symbol("or"),
            symbol("nonexistent1"),
            symbol("nonexistent2"),
        ]);
        assert!(!evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (or) with no args should be false (no true found)
        let req = list(vec![symbol("or")]);
        assert!(!evaluate_feature_requirement(&req, &features, &no_load).unwrap());
    }

    #[test]
    fn test_not_requirement() {
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (not nonexistent) should be true
        let req = list(vec![symbol("not"), symbol("nonexistent")]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (not r7rs) should be false
        let req = list(vec![symbol("not"), symbol("r7rs")]);
        assert!(!evaluate_feature_requirement(&req, &features, &no_load).unwrap());
    }

    #[test]
    fn test_library_requirement() {
        let features = FeatureRegistry::new();

        // Simulate library loader that knows about (scheme base)
        let can_load = |name: &[String]| name == ["scheme", "base"];

        // (library (scheme base)) should be true
        let req = list(vec![
            symbol("library"),
            list(vec![symbol("scheme"), symbol("base")]),
        ]);
        assert!(evaluate_feature_requirement(&req, &features, &can_load).unwrap());

        // (library (nonexistent lib)) should be false
        let req = list(vec![
            symbol("library"),
            list(vec![symbol("nonexistent"), symbol("lib")]),
        ]);
        assert!(!evaluate_feature_requirement(&req, &features, &can_load).unwrap());

        // (library (srfi 1)) with integer
        let can_load_srfi = |name: &[String]| name == ["srfi", "1"];
        let req = list(vec![
            symbol("library"),
            list(vec![symbol("srfi"), integer(1)]),
        ]);
        assert!(evaluate_feature_requirement(&req, &features, &can_load_srfi).unwrap());
    }

    #[test]
    fn test_nested_requirements() {
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (or (and r7rs patina) nonexistent) should be true
        let req = list(vec![
            symbol("or"),
            list(vec![symbol("and"), symbol("r7rs"), symbol("patina")]),
            symbol("nonexistent"),
        ]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());

        // (and r7rs (not nonexistent)) should be true
        let req = list(vec![
            symbol("and"),
            symbol("r7rs"),
            list(vec![symbol("not"), symbol("nonexistent")]),
        ]);
        assert!(evaluate_feature_requirement(&req, &features, &no_load).unwrap());
    }
}
