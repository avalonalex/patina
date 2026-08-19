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
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::features::FeatureRegistry;

/// Evaluate a feature requirement from TaggedValue.
///
/// Returns true if the requirement is satisfied.
///
/// The `can_load_library` callback is used to check if a library can be loaded.
/// This allows the evaluator to provide library checking without creating a
/// circular dependency.
pub fn evaluate_feature_requirement_tagged<F>(
    req: TaggedValue,
    shared_heap: &SharedHeap,
    features: &FeatureRegistry,
    can_load_library: &F,
) -> Result<bool, ParseError>
where
    F: Fn(&[String]) -> bool + ?Sized,
{
    let heap = shared_heap.borrow();

    // Simple feature identifier (symbol)
    if let Some(name) = heap.get_symbol_name(req) {
        return Ok(features.has_feature(name));
    }

    // Check for identifier (from macro expansion)
    if let Some((name, _)) = heap.get_identifier_data_any(req) {
        return Ok(features.has_feature(&name));
    }

    // Compound requirement (pair)
    if req.is_pair() {
        drop(heap);
        let items = tagged_list_to_vec(req, shared_heap)?;
        if items.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty feature requirement".to_string(),
            ));
        }

        let operator = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(items[0]) {
                Some(s.to_string())
            } else if let Some((name, _)) = heap.get_identifier_data_any(items[0]) {
                Some(name.to_string())
            } else {
                None
            }
        };

        match operator.as_deref() {
            Some("and") => {
                for &sub_req in &items[1..] {
                    if !evaluate_feature_requirement_tagged(
                        sub_req,
                        shared_heap,
                        features,
                        can_load_library,
                    )? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Some("or") => {
                for &sub_req in &items[1..] {
                    if evaluate_feature_requirement_tagged(
                        sub_req,
                        shared_heap,
                        features,
                        can_load_library,
                    )? {
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
                Ok(!evaluate_feature_requirement_tagged(
                    items[1],
                    shared_heap,
                    features,
                    can_load_library,
                )?)
            }
            Some("library") => {
                if items.len() != 2 {
                    return Err(ParseError::InvalidSyntax(
                        "library requires exactly one argument".to_string(),
                    ));
                }
                let lib_name = parse_library_name_tagged(items[1], shared_heap)?;
                Ok(can_load_library(&lib_name))
            }
            _ => {
                let heap = shared_heap.borrow();
                Err(ParseError::InvalidSyntax(format!(
                    "Unknown feature requirement operator: {}",
                    heap.type_name(items[0])
                )))
            }
        }
    } else {
        Err(ParseError::InvalidSyntax(format!(
            "Invalid feature requirement: {}",
            shared_heap.borrow().type_name(req)
        )))
    }
}

/// Convert a TaggedValue list to Vec<TaggedValue>
pub(crate) fn tagged_list_to_vec(
    value: TaggedValue,
    shared_heap: &SharedHeap,
) -> Result<Vec<TaggedValue>, ParseError> {
    let mut result = Vec::new();
    let mut current = value;

    loop {
        if current == TaggedValue::NULL {
            return Ok(result);
        }

        // Try native pair first
        {
            let heap = shared_heap.borrow();
            if current.is_pair() {
                let (car, cdr) = heap.get_pair(current);
                result.push(car);
                current = cdr;
                continue;
            }
        }

        let pair = shared_heap.borrow().try_pair(current);
        if let Some((car, cdr)) = pair {
            result.push(car);
            current = cdr;
            continue;
        }

        return Err(ParseError::InvalidSyntax(
            "Expected proper list in feature requirement".to_string(),
        ));
    }
}

/// Parse a library name from TaggedValue.
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
pub(crate) fn parse_library_name_tagged(
    value: TaggedValue,
    shared_heap: &SharedHeap,
) -> Result<Vec<String>, ParseError> {
    let mut items = tagged_list_to_vec(value, shared_heap)?;

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

    let heap = shared_heap.borrow();
    let mut name = Vec::new();
    for item in items {
        if let Some(s) = heap.get_symbol_name(item) {
            name.push(s.to_string());
        } else if let Some((id_name, _)) = heap.get_identifier_data_any(item) {
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
                heap.type_name(item)
            )));
        }
    }

    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_runtime::FeatureRegistry;

    fn test_heap() -> SharedHeap {
        patina_core::new_shared_heap()
    }

    fn sym(s: &str, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().intern_symbol(s)
    }

    fn fixnum(n: i64) -> TaggedValue {
        TaggedValue::fixnum(n)
    }

    fn list(items: Vec<TaggedValue>, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().list_from_iter(items)
    }

    #[test]
    fn test_simple_feature() {
        let heap = test_heap();
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // r7rs should be present
        assert!(
            evaluate_feature_requirement_tagged(sym("r7rs", &heap), &heap, &features, &no_load)
                .unwrap()
        );

        // patina should be present
        assert!(
            evaluate_feature_requirement_tagged(sym("patina", &heap), &heap, &features, &no_load)
                .unwrap()
        );

        // nonexistent should be false
        assert!(
            !evaluate_feature_requirement_tagged(
                sym("nonexistent", &heap),
                &heap,
                &features,
                &no_load
            )
            .unwrap()
        );
    }

    #[test]
    fn test_and_requirement() {
        let heap = test_heap();
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (and r7rs patina) should be true
        let req = list(
            vec![sym("and", &heap), sym("r7rs", &heap), sym("patina", &heap)],
            &heap,
        );
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (and r7rs nonexistent) should be false
        let req = list(
            vec![
                sym("and", &heap),
                sym("r7rs", &heap),
                sym("nonexistent", &heap),
            ],
            &heap,
        );
        assert!(!evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (and) with no args should be true (vacuous truth)
        let req = list(vec![sym("and", &heap)], &heap);
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());
    }

    #[test]
    fn test_or_requirement() {
        let heap = test_heap();
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (or nonexistent r7rs) should be true
        let req = list(
            vec![
                sym("or", &heap),
                sym("nonexistent", &heap),
                sym("r7rs", &heap),
            ],
            &heap,
        );
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (or nonexistent1 nonexistent2) should be false
        let req = list(
            vec![
                sym("or", &heap),
                sym("nonexistent1", &heap),
                sym("nonexistent2", &heap),
            ],
            &heap,
        );
        assert!(!evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (or) with no args should be false (no true found)
        let req = list(vec![sym("or", &heap)], &heap);
        assert!(!evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());
    }

    #[test]
    fn test_not_requirement() {
        let heap = test_heap();
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (not nonexistent) should be true
        let req = list(vec![sym("not", &heap), sym("nonexistent", &heap)], &heap);
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (not r7rs) should be false
        let req = list(vec![sym("not", &heap), sym("r7rs", &heap)], &heap);
        assert!(!evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());
    }

    #[test]
    fn test_library_requirement() {
        let heap = test_heap();
        let features = FeatureRegistry::new();

        // Simulate library loader that knows about (scheme base)
        let can_load = |name: &[String]| name == ["scheme", "base"];

        // (library (scheme base)) should be true
        let req = list(
            vec![
                sym("library", &heap),
                list(vec![sym("scheme", &heap), sym("base", &heap)], &heap),
            ],
            &heap,
        );
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &can_load).unwrap());

        // (library (nonexistent lib)) should be false
        let req = list(
            vec![
                sym("library", &heap),
                list(vec![sym("nonexistent", &heap), sym("lib", &heap)], &heap),
            ],
            &heap,
        );
        assert!(!evaluate_feature_requirement_tagged(req, &heap, &features, &can_load).unwrap());

        // (library (srfi 1)) with integer
        let can_load_srfi = |name: &[String]| name == ["srfi", "1"];
        let req = list(
            vec![
                sym("library", &heap),
                list(vec![sym("srfi", &heap), fixnum(1)], &heap),
            ],
            &heap,
        );
        assert!(
            evaluate_feature_requirement_tagged(req, &heap, &features, &can_load_srfi).unwrap()
        );
    }

    #[test]
    fn test_nested_requirements() {
        let heap = test_heap();
        let features = FeatureRegistry::new();
        let no_load = |_: &[String]| false;

        // (or (and r7rs patina) nonexistent) should be true
        let req = list(
            vec![
                sym("or", &heap),
                list(
                    vec![sym("and", &heap), sym("r7rs", &heap), sym("patina", &heap)],
                    &heap,
                ),
                sym("nonexistent", &heap),
            ],
            &heap,
        );
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());

        // (and r7rs (not nonexistent)) should be true
        let req = list(
            vec![
                sym("and", &heap),
                sym("r7rs", &heap),
                list(vec![sym("not", &heap), sym("nonexistent", &heap)], &heap),
            ],
            &heap,
        );
        assert!(evaluate_feature_requirement_tagged(req, &heap, &features, &no_load).unwrap());
    }
}
