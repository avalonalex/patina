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
use patina_core::features::FeatureRegistry;
use patina_core::{Cx, Instance, Value};

/// Query the interpreter's live catalogue. A standalone frontend heap has no
/// loaders and therefore no libraries.
pub fn library_available(instance: &Instance, name: &[String]) -> bool {
    let availability = instance.library_availability();
    availability.is_some_and(|catalogue| catalogue.is_available(name))
}

/// Evaluate a feature requirement.
///
/// Returns true if the requirement is satisfied.
///
/// The `can_load_library` callback is used to check if a library can be loaded.
/// This allows the evaluator to provide library checking without creating a
/// circular dependency.
pub fn evaluate_feature_requirement_tagged<'gc, F>(
    req: Value<'gc>,
    cx: &Cx<'gc>,
    features: &FeatureRegistry,
    can_load_library: &F,
) -> Result<bool, ParseError>
where
    F: Fn(&[String]) -> bool + ?Sized,
{
    evaluate_requirement_at(req, cx, features, can_load_library, 0)
}

/// [`evaluate_feature_requirement_tagged`] for a requirement `depth` levels
/// inside the one being evaluated. A requirement nests as code does, and is
/// held to the same limit, [`MAX_FORM_DEPTH`], with its stack grown as it goes
/// (#617). That also refuses one that contains itself, which a datum label can
/// write and which recursed until the stack overflowed.
///
/// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
fn evaluate_requirement_at<'gc, F>(
    req: Value<'gc>,
    cx: &Cx<'gc>,
    features: &FeatureRegistry,
    can_load_library: &F,
    depth: usize,
) -> Result<bool, ParseError>
where
    F: Fn(&[String]) -> bool + ?Sized,
{
    if depth >= patina_core::walk::MAX_FORM_DEPTH {
        return Err(ParseError::InvalidSyntax(format!(
            "feature requirement nested too deeply: more than {} levels",
            patina_core::walk::MAX_FORM_DEPTH
        )));
    }
    patina_core::walk::ensure_sufficient_stack(|| {
        evaluate_requirement_node(req, cx, features, can_load_library, depth)
    })
}

/// [`evaluate_requirement_at`] for one requirement.
fn evaluate_requirement_node<'gc, F>(
    req: Value<'gc>,
    cx: &Cx<'gc>,
    features: &FeatureRegistry,
    can_load_library: &F,
    depth: usize,
) -> Result<bool, ParseError>
where
    F: Fn(&[String]) -> bool + ?Sized,
{
    // Simple feature identifier (symbol)
    if let Some(name) = cx.get_symbol_name(req) {
        return Ok(features.has_feature(&name));
    }

    // Check for identifier (from macro expansion)
    if let Some((name, _)) = cx.get_identifier_data_any(req) {
        return Ok(features.has_feature(&name));
    }

    // Compound requirement (pair)
    if req.is_pair() {
        let items = tagged_list_to_vec(req, cx)?;
        if items.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty feature requirement".to_string(),
            ));
        }

        let operator = if let Some(s) = cx.get_symbol_name(items[0]) {
            Some(s.to_string())
        } else {
            cx.get_identifier_data_any(items[0])
                .map(|(name, _)| name.to_string())
        };

        match operator.as_deref() {
            Some("and") => {
                for &sub_req in &items[1..] {
                    if !evaluate_requirement_at(sub_req, cx, features, can_load_library, depth + 1)?
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Some("or") => {
                for &sub_req in &items[1..] {
                    if evaluate_requirement_at(sub_req, cx, features, can_load_library, depth + 1)?
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Some("not") => {
                if items.len() != 2 {
                    return Err(ParseError::InvalidSyntax(
                        "not requires exactly one argument".to_string(),
                    ));
                }
                Ok(!evaluate_requirement_at(
                    items[1],
                    cx,
                    features,
                    can_load_library,
                    depth + 1,
                )?)
            }
            Some("library") => {
                if items.len() != 2 {
                    return Err(ParseError::InvalidSyntax(
                        "library requires exactly one argument".to_string(),
                    ));
                }
                let lib_name = parse_library_name_tagged(items[1], cx)?;
                Ok(can_load_library(&lib_name))
            }
            _ => Err(ParseError::InvalidSyntax(format!(
                "Unknown feature requirement operator: {}",
                cx.type_name(items[0])
            ))),
        }
    } else {
        Err(ParseError::InvalidSyntax(format!(
            "Invalid feature requirement: {}",
            cx.type_name(req)
        )))
    }
}

/// A proper list's elements.
///
/// Refuses a circular list with an improper one, where a plain walk followed a
/// circular one forever (#459).
pub(crate) fn tagged_list_to_vec<'gc>(
    value: Value<'gc>,
    cx: &Cx<'gc>,
) -> Result<Vec<Value<'gc>>, ParseError> {
    cx.list_to_vec(value).ok_or_else(|| {
        ParseError::InvalidSyntax("Expected proper list in feature requirement".to_string())
    })
}

/// Parse a library name.
///
/// A trailing R6RS version is accepted and discarded — `(rnrs base (6))`,
/// `(rnrs base ((>= 6)))` and `(rnrs base)` all name the same library here.
/// R6RS §7.1 puts the version last and makes it a *list*, while an R7RS
/// library name is a flat sequence of identifiers and non-negative integers,
/// so a nested list in the final position is unambiguously a version and
/// nothing else. Discarding rather than matching it is the honest behaviour
/// for an implementation with no version resolution: there is one library
/// under the name, and pretending to check a constraint against it would
/// report agreement it never established.
pub(crate) fn parse_library_name_tagged<'gc>(
    value: Value<'gc>,
    cx: &Cx<'gc>,
) -> Result<Vec<String>, ParseError> {
    let mut items = tagged_list_to_vec(value, cx)?;

    if items.is_empty() {
        return Err(ParseError::InvalidSyntax(
            "Library name cannot be empty".to_string(),
        ));
    }

    let has_version = items.len() > 1
        && items
            .last()
            .is_some_and(|last| last.is_null() || last.is_pair());
    if has_version {
        if !crate::dialect::allow_r6rs() {
            return Err(ParseError::InvalidSyntax(
                "R6RS version reference in a library name is not R7RS \
                 (pass --allow-r6rs to read it)"
                    .to_string(),
            ));
        }
        items.pop();
    }

    let mut name = Vec::new();
    for item in items {
        if let Some(s) = cx.get_symbol_name(item) {
            name.push(s.to_string());
        } else if let Some((id_name, _)) = cx.get_identifier_data_any(item) {
            name.push(id_name.to_string());
        } else if item.is_fixnum() {
            let n = item.as_fixnum_unchecked();
            if n >= 0 {
                name.push(n.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "Library name parts must be identifiers or non-negative integers, got negative: {}",
                    n
                )));
            }
        } else {
            return Err(ParseError::InvalidSyntax(format!(
                "Library name parts must be identifiers or non-negative integers, got: {}",
                cx.type_name(item)
            )));
        }
    }

    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_runtime::FeatureRegistry;

    /// Run `f` in a context over a fresh heap.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        patina_core::NoGcScope::with_cx(&patina_core::new_shared_heap(), f)
    }

    fn sym<'gc>(s: &str, cx: &mut Cx<'gc>) -> Value<'gc> {
        cx.intern_symbol(s)
    }

    fn fixnum<'gc>(n: i64) -> Value<'gc> {
        Value::fixnum(n)
    }

    fn list<'gc>(items: Vec<Value<'gc>>, cx: &mut Cx<'gc>) -> Value<'gc> {
        cx.list_from_iter(items)
    }

    #[test]
    fn test_simple_feature() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();
            let no_load = |_: &[String]| false;

            // r7rs should be present
            assert!(
                evaluate_feature_requirement_tagged(sym("r7rs", cx), cx, &features, &no_load)
                    .unwrap()
            );

            // patina should be present
            assert!(
                evaluate_feature_requirement_tagged(sym("patina", cx), cx, &features, &no_load)
                    .unwrap()
            );

            // nonexistent should be false
            assert!(
                !evaluate_feature_requirement_tagged(
                    sym("nonexistent", cx),
                    cx,
                    &features,
                    &no_load
                )
                .unwrap()
            );
        });
    }

    #[test]
    fn test_and_requirement() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();
            let no_load = |_: &[String]| false;

            // (and r7rs patina) should be true
            let req = list(vec![sym("and", cx), sym("r7rs", cx), sym("patina", cx)], cx);
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (and r7rs nonexistent) should be false
            let req = list(
                vec![sym("and", cx), sym("r7rs", cx), sym("nonexistent", cx)],
                cx,
            );
            assert!(!evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (and) with no args should be true (vacuous truth)
            let req = list(vec![sym("and", cx)], cx);
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());
        });
    }

    #[test]
    fn test_or_requirement() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();
            let no_load = |_: &[String]| false;

            // (or nonexistent r7rs) should be true
            let req = list(
                vec![sym("or", cx), sym("nonexistent", cx), sym("r7rs", cx)],
                cx,
            );
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (or nonexistent1 nonexistent2) should be false
            let req = list(
                vec![
                    sym("or", cx),
                    sym("nonexistent1", cx),
                    sym("nonexistent2", cx),
                ],
                cx,
            );
            assert!(!evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (or) with no args should be false (no true found)
            let req = list(vec![sym("or", cx)], cx);
            assert!(!evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());
        });
    }

    #[test]
    fn test_not_requirement() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();
            let no_load = |_: &[String]| false;

            // (not nonexistent) should be true
            let req = list(vec![sym("not", cx), sym("nonexistent", cx)], cx);
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (not r7rs) should be false
            let req = list(vec![sym("not", cx), sym("r7rs", cx)], cx);
            assert!(!evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());
        });
    }

    #[test]
    fn test_library_requirement() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();

            // Simulate library loader that knows about (scheme base)
            let can_load = |name: &[String]| name == ["scheme", "base"];

            // (library (scheme base)) should be true
            let req = list(
                vec![
                    sym("library", cx),
                    list(vec![sym("scheme", cx), sym("base", cx)], cx),
                ],
                cx,
            );
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &can_load).unwrap());

            // (library (nonexistent lib)) should be false
            let req = list(
                vec![
                    sym("library", cx),
                    list(vec![sym("nonexistent", cx), sym("lib", cx)], cx),
                ],
                cx,
            );
            assert!(!evaluate_feature_requirement_tagged(req, cx, &features, &can_load).unwrap());

            // (library (srfi 1)) with integer
            let can_load_srfi = |name: &[String]| name == ["srfi", "1"];
            let req = list(
                vec![
                    sym("library", cx),
                    list(vec![sym("srfi", cx), fixnum(1)], cx),
                ],
                cx,
            );
            assert!(
                evaluate_feature_requirement_tagged(req, cx, &features, &can_load_srfi).unwrap()
            );
        });
    }

    #[test]
    fn test_nested_requirements() {
        with_cx(|cx| {
            let features = FeatureRegistry::new();
            let no_load = |_: &[String]| false;

            // (or (and r7rs patina) nonexistent) should be true
            let req = list(
                vec![
                    sym("or", cx),
                    list(vec![sym("and", cx), sym("r7rs", cx), sym("patina", cx)], cx),
                    sym("nonexistent", cx),
                ],
                cx,
            );
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());

            // (and r7rs (not nonexistent)) should be true
            let req = list(
                vec![
                    sym("and", cx),
                    sym("r7rs", cx),
                    list(vec![sym("not", cx), sym("nonexistent", cx)], cx),
                ],
                cx,
            );
            assert!(evaluate_feature_requirement_tagged(req, cx, &features, &no_load).unwrap());
        });
    }
}
