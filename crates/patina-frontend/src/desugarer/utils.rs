// DesugarError is large, but boxing it would add complexity for minimal benefit
#![allow(clippy::result_large_err)]

//! Utility functions for desugaring

use super::error::{DesugarError, Result};
use patina_core::{Heap, SharedHeap, TaggedValue};
use patina_ir::{Formals, ScopedParam, Symbol};
use patina_runtime::ScopeSet;
use std::collections::HashSet;
use std::rc::Rc;

// ============================================================================
// TaggedValue Utilities
// ============================================================================

/// Convert a TaggedValue list to a Vec<TaggedValue>
///
/// This is the TaggedValue equivalent of `list_to_vec`.
/// Takes SharedHeap for heap operations.
pub fn list_to_vec_tagged(
    value: TaggedValue,
    shared_heap: &SharedHeap,
) -> Result<Vec<TaggedValue>> {
    let mut result = Vec::new();
    let mut current = value;

    loop {
        if current == TaggedValue::NULL {
            return Ok(result);
        }

        // Fast path: native heap pair - just use immutable borrow
        if current.is_pair() {
            let heap = shared_heap.borrow();
            let (car, cdr) = heap.get_pair(current);
            result.push(car);
            current = cdr;
            continue;
        }

        // Not a native pair and not null — improper list
        return Err(DesugarError::ExpectedProperList(format!(
            "Expected proper list, got improper list ending with {:?}",
            current
        )));
    }
}

/// Convert Scheme formals (TaggedValue) to Formals enum
///
/// This is the TaggedValue equivalent of `convert_formals`.
/// Takes SharedHeap for heap operations.
pub fn convert_formals_tagged(formals: TaggedValue, shared_heap: &SharedHeap) -> Result<Formals> {
    // Fixed arity: ()
    if formals == TaggedValue::NULL {
        return Ok(Formals::Fixed(vec![]));
    }

    // Check for single symbol (variadic) or identifier
    {
        let heap = shared_heap.borrow();
        // Variadic: single symbol (args)
        if let Some(name) = heap.get_symbol_name(formals) {
            return Ok(Formals::Variadic(ScopedParam::simple(Rc::from(name))));
        }

        // Variadic with identifier - preserve scopes
        if heap.is_identifier(formals)
            && let Some(id) = get_identifier_info(formals, &heap)
        {
            return Ok(Formals::Variadic(ScopedParam::with_scopes(id.0, id.1)));
        }
    }

    // Either proper list (fixed) or improper list (mixed)
    if formals.is_pair() {
        let mut params: Vec<ScopedParam> = Vec::new();
        let mut current = formals;

        loop {
            if current == TaggedValue::NULL {
                // Proper list - fixed arity
                check_no_duplicates_scoped(&params, "lambda")?;
                return Ok(Formals::Fixed(params));
            }

            // Fast path: native pair
            if current.is_pair() {
                let heap = shared_heap.borrow();
                let (car, cdr) = heap.get_pair(current);

                if let Some(name) = heap.get_symbol_name(car) {
                    params.push(ScopedParam::simple(Rc::from(name)));
                } else if let Some(id) = get_identifier_info(car, &heap) {
                    params.push(ScopedParam::with_scopes(id.0, id.1));
                } else {
                    return Err(DesugarError::InvalidFormals(format!(
                        "Parameter must be a symbol, got {:?}",
                        car
                    )));
                }
                current = cdr;
                continue;
            }

            // Improper list terminator (not a pair)
            {
                let heap = shared_heap.borrow();
                if let Some(rest_name) = heap.get_symbol_name(current) {
                    // Improper list with symbol - mixed arity: (x y . rest)
                    check_no_duplicates_scoped(&params, "lambda")?;
                    let rest_param = ScopedParam::simple(Rc::from(rest_name));
                    if binds_identifier(&params, rest_name, &rest_param.scopes) {
                        return Err(DesugarError::DuplicateParameter {
                            name: rest_name.to_string(),
                            context: "lambda".to_string(),
                        });
                    }
                    return Ok(Formals::Mixed {
                        fixed: params,
                        rest: rest_param,
                    });
                } else if let Some(id) = get_identifier_info(current, &heap) {
                    // Improper list with identifier - mixed arity: (x y . rest)
                    check_no_duplicates_scoped(&params, "lambda")?;
                    if binds_identifier(&params, &id.0, &id.1) {
                        return Err(DesugarError::DuplicateParameter {
                            name: id.0.to_string(),
                            context: "lambda".to_string(),
                        });
                    }
                    let rest_param = ScopedParam::with_scopes(id.0, id.1);
                    return Ok(Formals::Mixed {
                        fixed: params,
                        rest: rest_param,
                    });
                } else {
                    return Err(DesugarError::InvalidFormals(format!(
                        "Invalid formal parameters: {:?}",
                        current
                    )));
                }
            }
        }
    }

    Err(DesugarError::InvalidFormals(format!(
        "Invalid formal parameters: {:?}",
        formals
    )))
}

/// Get identifier name and scopes from a TaggedValue
///
/// Returns Some((name, scopes)) if the value is an identifier, None otherwise.
pub fn get_identifier_info(tv: TaggedValue, heap: &Heap) -> Option<(Rc<str>, ScopeSet)> {
    heap.get_identifier_data_any(tv)
}

/// Strip identifiers from a TaggedValue tree, replacing them with plain symbols.
///
/// In quoted data, identifiers from macro expansion should become plain symbols.
/// Identifier scopes are only needed during desugaring for binding resolution,
/// not in quoted output data.
pub fn strip_identifiers_tagged(tv: TaggedValue, shared_heap: &SharedHeap) -> TaggedValue {
    let mut seen = std::collections::HashSet::new();
    strip_identifiers_impl(tv, shared_heap, &mut seen)
}

fn strip_identifiers_impl(
    tv: TaggedValue,
    shared_heap: &SharedHeap,
    seen: &mut std::collections::HashSet<u64>,
) -> TaggedValue {
    let heap = shared_heap.borrow();

    // Check if it's an identifier (native or boxed) — replace with symbol
    if let Some((name, _)) = heap.get_identifier_data_any(tv) {
        drop(heap);
        return shared_heap.borrow_mut().intern_symbol(&name);
    }

    // Check if it's a pair — recursively strip car and cdr
    if tv.is_pair() {
        // Cycle detection: if we've already visited this pair, return as-is
        if !seen.insert(tv.raw_bits()) {
            return tv;
        }

        let (car, cdr) = match heap.try_pair(tv) {
            Some(pair) => pair,
            None => return tv,
        };
        drop(heap);

        let new_car = strip_identifiers_impl(car, shared_heap, seen);
        let new_cdr = strip_identifiers_impl(cdr, shared_heap, seen);

        // Only allocate a new pair if something changed
        if new_car == car && new_cdr == cdr {
            return tv;
        }

        return shared_heap.borrow_mut().alloc_pair(new_car, new_cdr);
    }

    // Check if it's a vector — recursively strip elements
    if tv.is_vector() {
        if !seen.insert(tv.raw_bits()) {
            return tv;
        }

        let len = heap.vector_len(tv);
        let elements: Vec<TaggedValue> = (0..len).map(|i| heap.vector_ref(tv, i)).collect();
        drop(heap);

        let mut changed = false;
        let new_elements: Vec<TaggedValue> = elements
            .iter()
            .map(|elem| {
                let new_elem = strip_identifiers_impl(*elem, shared_heap, seen);
                if new_elem != *elem {
                    changed = true;
                }
                new_elem
            })
            .collect();

        if !changed {
            return tv;
        }

        return shared_heap.borrow_mut().alloc_vector(new_elements);
    }

    // Everything else (symbols, numbers, strings, etc.) passes through
    tv
}

/// Parse define function syntax from TaggedValue
///
/// Returns (name, formals_tagged) where formals is the rest of the list.
/// Takes SharedHeap for heap operations.
pub fn parse_define_function_tagged(
    pattern: TaggedValue,
    shared_heap: &SharedHeap,
) -> Result<(Symbol, TaggedValue)> {
    // Fast path: native pair
    if pattern.is_pair() {
        let heap = shared_heap.borrow();
        let (car, cdr) = heap.get_pair(pattern);

        let name = if let Some(s) = heap.get_symbol_name(car) {
            Rc::from(s)
        } else if let Some(id) = get_identifier_info(car, &heap) {
            id.0
        } else {
            return Err(DesugarError::InvalidSyntax(
                "define function name must be a symbol".to_string(),
            ));
        };

        return Ok((name, cdr));
    }

    // Not a native pair — error
    Err(DesugarError::InvalidSyntax(
        "define function requires (name params...) pattern".to_string(),
    ))
}

/// Check for duplicate parameters.
///
/// Two parameters collide only when they share a name *and* a scope set. A
/// recursive macro that introduces the same template identifier on each
/// expansion produces several params spelled alike, but each carries the
/// distinct scope of the expansion that introduced it, so they are separate
/// bindings — comparing names alone would reject hygienic code such as:
///
/// ```scheme
/// (define-syntax gen
///   (syntax-rules ()
///     ((_ () (args ...)) (lambda (args ...) (list args ...)))
///     ((_ (x . rest) (args ...)) (gen rest (args ... a)))))
/// ((gen (1 2) ()) 10 20)  ;; => (10 20)
/// ```
pub fn check_no_duplicates_scoped(params: &[ScopedParam], context: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for param in params {
        if !seen.insert((param.name.as_ref(), &param.scopes)) {
            return Err(DesugarError::DuplicateParameter {
                name: param.name.to_string(),
                context: context.to_string(),
            });
        }
    }
    Ok(())
}

/// Whether `params` already binds the identifier `(name, scopes)`.
///
/// The rest parameter of an improper formals list is checked against the fixed
/// params with the same name-plus-scopes rule as [`check_no_duplicates_scoped`].
fn binds_identifier(params: &[ScopedParam], name: &str, scopes: &ScopeSet) -> bool {
    params
        .iter()
        .any(|p| p.name.as_ref() == name && &p.scopes == scopes)
}

/// Extract all parameter names from a Formals structure
///
/// Used for tracking which names are shadowed by lambda parameters,
/// so they are not treated as macro calls.
pub fn formals_to_names(formals: &Formals) -> Vec<Rc<str>> {
    match formals {
        Formals::Fixed(params) => params.iter().map(|p| p.name.clone()).collect(),
        Formals::Variadic(p) => vec![p.name.clone()],
        Formals::Mixed { fixed, rest } => {
            let mut names: Vec<Rc<str>> = fixed.iter().map(|p| p.name.clone()).collect();
            names.push(rest.name.clone());
            names
        }
    }
}
