// DesugarError is large, but boxing it would add complexity for minimal benefit
#![allow(clippy::result_large_err)]

//! Utility functions for desugaring

use super::error::{DesugarError, Result};
use patina_core::debug_format::format_tagged;
use patina_core::{Heap, SharedHeap, SpineEnd, TaggedValue};
use patina_ir::{Formals, ScopedParam, Symbol};
use patina_runtime::ScopeSet;
use std::collections::{HashMap, HashSet};
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
    let heap = shared_heap.borrow();
    match heap.spine(value) {
        (elements, SpineEnd::Null) => Ok(elements),
        // The tail as Scheme writes it (#457): its `{:?}` was
        // `TaggedValue::fixnum(3)`.
        (_, SpineEnd::Improper(tail)) => Err(DesugarError::ExpectedProperList(format!(
            "got an improper list ending with {}",
            format_tagged(tail, &heap)
        ))),
        // Walked, this collected elements until memory ran out (#459).
        (_, SpineEnd::Circular) => Err(DesugarError::ExpectedProperList(format!(
            "got a circular list: {}",
            format_tagged(value, &heap)
        ))),
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

    // Walked below, a circular list collected parameters until memory ran
    // out (#459).
    if shared_heap.borrow().spine_is_circular(formals) {
        return Err(DesugarError::InvalidFormals(format!(
            "a parameter list cannot be circular: {}",
            format_tagged(formals, &shared_heap.borrow())
        )));
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
                        "Parameter must be a symbol, got {}",
                        format_tagged(car, &heap)
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
                        "a rest parameter must be a symbol, got {}",
                        format_tagged(current, &heap)
                    )));
                }
            }
        }
    }

    Err(DesugarError::InvalidFormals(format!(
        "expected a symbol or a list of symbols, got {}",
        format_tagged(formals, &shared_heap.borrow())
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
pub fn strip_identifiers_tagged(
    tv: TaggedValue,
    shared_heap: &SharedHeap,
    memo: &mut HashMap<u64, TaggedValue>,
) -> TaggedValue {
    shared_heap
        .borrow_mut()
        .map_syntax_identifiers_memo(tv, memo, |heap, _, name, _| Some(heap.intern_symbol(&name)))
}

/// Parse define function syntax from TaggedValue
///
/// Returns (name, name_scopes, formals_tagged) where formals is the rest of the list.
/// Takes SharedHeap for heap operations.
/// A symbol or a scoped identifier as `(name, scopes)`.
///
/// The one answer to "what is this identifier called and what scopes does it
/// carry" — `Desugarer::identifier_of` is this function with the borrow taken
/// for you. Worth having exactly one: a copy of it that dropped the scopes is
/// what let a recursive macro's per-expansion definitions collapse onto a
/// single binding.
pub fn symbol_or_identifier(tv: TaggedValue, heap: &Heap) -> Option<(Symbol, ScopeSet)> {
    if let Some(s) = heap.get_symbol_name(tv) {
        return Some((Rc::from(s), ScopeSet::new()));
    }
    get_identifier_info(tv, heap)
}

pub fn parse_define_function_tagged(
    pattern: TaggedValue,
    shared_heap: &SharedHeap,
) -> Result<(Symbol, ScopeSet, TaggedValue)> {
    // Fast path: native pair
    if pattern.is_pair() {
        let heap = shared_heap.borrow();
        let (car, cdr) = heap.get_pair(pattern);

        let (name, scopes) = symbol_or_identifier(car, &heap).ok_or_else(|| {
            DesugarError::InvalidSyntax("define function name must be a symbol".to_string())
        })?;

        return Ok((name, scopes, cdr));
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
pub fn formals_to_binders(formals: &Formals) -> Vec<(Rc<str>, ScopeSet)> {
    let binder = |p: &patina_ir::ScopedParam| (p.name.clone(), p.scopes.clone());
    match formals {
        Formals::Fixed(params) => params.iter().map(binder).collect(),
        Formals::Variadic(p) => vec![binder(p)],
        Formals::Mixed { fixed, rest } => {
            let mut binders: Vec<(Rc<str>, ScopeSet)> = fixed.iter().map(binder).collect();
            binders.push(binder(rest));
            binders
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripping_quoted_identifiers_copies_only_changed_pairs() {
        let heap = patina_core::new_shared_heap();
        let (original, identifier, unchanged_tail) = {
            let mut heap = heap.borrow_mut();
            let identifier = heap.alloc_identifier(Rc::from("x"), ScopeSet::new());
            let unchanged_tail = heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
            let changed = heap.alloc_pair(identifier, unchanged_tail);
            let original = heap.alloc_pair(TaggedValue::fixnum(1), changed);
            (original, identifier, unchanged_tail)
        };
        let stripped = strip_identifiers_tagged(original, &heap, &mut HashMap::new());
        let heap = heap.borrow();
        assert_ne!(stripped, original);
        assert_eq!(heap.car(stripped), TaggedValue::fixnum(1));
        let changed = heap.cdr(stripped);
        assert_eq!(heap.get_symbol_name(heap.car(changed)), Some("x"));
        assert_eq!(heap.cdr(changed), unchanged_tail);
        assert_eq!(heap.car(heap.cdr(original)), identifier);
    }

    #[test]
    fn stripping_a_quoted_improper_tail_converts_its_identifier() {
        let heap = patina_core::new_shared_heap();
        let (original, identifier) = {
            let mut heap = heap.borrow_mut();
            let identifier = heap.alloc_identifier(Rc::from("tail"), ScopeSet::new());
            (
                heap.alloc_pair(TaggedValue::fixnum(1), identifier),
                identifier,
            )
        };
        let stripped = strip_identifiers_tagged(original, &heap, &mut HashMap::new());
        let heap = heap.borrow();
        assert_eq!(heap.get_symbol_name(heap.cdr(stripped)), Some("tail"));
        assert_eq!(heap.cdr(original), identifier);
    }

    #[test]
    fn stripping_nested_pairs_and_vectors_preserves_unchanged_cycles() {
        let heap = patina_core::new_shared_heap();
        let (original, cycle, identifier) = {
            let mut heap = heap.borrow_mut();
            let identifier = heap.alloc_identifier(Rc::from("nested"), ScopeSet::new());
            let cycle = heap.alloc_pair(TaggedValue::fixnum(7), TaggedValue::NULL);
            heap.set_cdr(cycle, cycle);
            let pair = heap.alloc_pair(identifier, cycle);
            let vector = heap.alloc_vector(vec![pair, cycle]);
            (heap.alloc_pair(vector, cycle), cycle, identifier)
        };
        let stripped = strip_identifiers_tagged(original, &heap, &mut HashMap::new());
        let heap = heap.borrow();
        let vector = heap.car(stripped);
        assert_ne!(vector, heap.car(original));
        assert_eq!(
            heap.get_symbol_name(heap.car(heap.vector_ref(vector, 0))),
            Some("nested")
        );
        assert_eq!(heap.cdr(heap.vector_ref(vector, 0)), cycle);
        assert_eq!(heap.vector_ref(vector, 1), cycle);
        assert_eq!(heap.cdr(stripped), cycle);
        assert_eq!(heap.cdr(cycle), cycle);
        assert_eq!(heap.car(heap.vector_ref(heap.car(original), 0)), identifier);
    }
}
