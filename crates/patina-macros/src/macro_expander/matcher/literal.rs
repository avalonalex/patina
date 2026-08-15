//! Literal matching and hygiene support
//!
//! This module implements literal matching for pattern matching, including
//! R7RS hygiene support for shadowed literals.

use patina_core::{Heap, TaggedValue};
use patina_runtime::{LiteralBinding, ScopeSet};
use std::collections::HashSet;
use std::rc::Rc;

/// Check if a TaggedValue literal identifier is shadowed at the macro use site.
///
/// R7RS 4.3.2: A literal identifier matches an input identifier if both have
/// the same binding, or both are unbound and have the same name.
///
/// This function implements the `bound-identifier=?` semantics:
/// - If both literal and input are bound with the same scopes, they match
/// - If both are unbound and have the same name, they match
/// - If one is bound and the other is unbound (or differently bound), they DON'T match
pub fn is_literal_shadowed_tagged(
    lit: TaggedValue,
    input: TaggedValue,
    heap: &Heap,
    shadowed_names: &HashSet<Rc<str>>,
    use_site_scopes: &ScopeSet,
    literals: &[LiteralBinding],
) -> bool {
    // Extract the literal name from the pattern TaggedValue
    let lit_name: Rc<str> = match heap.get_symbol_or_identifier_name(lit) {
        Some(name) => Rc::from(name),
        None => return false, // Non-identifier literals can't be shadowed
    };

    // Find the literal binding information for this literal
    let literal_binding = literals
        .iter()
        .find(|lb| lb.name.as_ref() == lit_name.as_ref());

    // If this literal is not in the macro's literals list, it's not subject to shadowing checks
    let literal_binding = match literal_binding {
        Some(lb) => lb,
        None => return false,
    };

    // Get input name from TaggedValue
    let input_name: Rc<str> = match heap.get_symbol_or_identifier_name(input) {
        Some(name) => Rc::from(name),
        None => return false, // Input is not an identifier
    };

    // Names must match for shadowing to be relevant
    if lit_name.as_ref() != input_name.as_ref() {
        return false;
    }

    // Apply bound-identifier=? semantics
    match &literal_binding.binding_scopes {
        None => {
            // Literal was unbound at macro definition time
            if shadowed_names.contains(&input_name) {
                return true; // Shadowed (should NOT match)
            }
            false // Not shadowed (should match)
        }
        Some(literal_scopes) => {
            // Literal was bound at macro definition time
            if shadowed_names.contains(&input_name) {
                if literal_scopes == use_site_scopes {
                    false // Same scopes = same binding
                } else {
                    true // Different scopes = different bindings
                }
            } else {
                // Check input identifier scopes (native or boxed via unified method)
                if let Some((_, id_scopes)) = heap.get_identifier_data_any(input) {
                    if literal_scopes.is_subset_of(&id_scopes) {
                        false // Same or compatible binding
                    } else {
                        true // Different binding contexts
                    }
                } else {
                    // Input is a bare Symbol - at top level
                    !literal_scopes.is_empty()
                }
            }
        }
    }
}

/// Check if a TaggedValue input matches a TaggedValue literal from the pattern.
///
/// Both pattern and input are TaggedValues stored on the heap.
pub fn tagged_matches_literal(input: TaggedValue, pattern_lit: TaggedValue, heap: &Heap) -> bool {
    // Check if pattern is a symbol
    if let Some(pat_name) = heap.get_symbol_name(pattern_lit) {
        // Pattern is a Symbol - input must be an identifier with matching name
        return match heap.get_symbol_or_identifier_name(input) {
            Some(input_name) => pat_name == input_name,
            None => false,
        };
    }

    // Check if pattern is an identifier (native or boxed, unified)
    if let Some((pat_name, _)) = heap.get_identifier_data_any(pattern_lit) {
        // Names alone, matching the three other arms of this function.
        //
        // R7RS 4.3.2: a literal matches an input identifier when both denote
        // the same binding, *or both are unbound and have the same name*. This
        // function cannot see bindings — that is what the caller's
        // `is_literal_shadowed_tagged` veto is for, and it runs first. So the
        // only question left here is whether the spellings agree.
        //
        // Requiring the literal's scopes to be a subset of the input's answered
        // a different question. It made an *introduced* literal (which carries
        // the enclosing expansion's scopes) unable to match a *substituted*
        // input (which carries none), even with both unbound:
        //
        //   (define-syntax m
        //     (syntax-rules ()
        //       ((_ e) (let-syntax ((n (syntax-rules (k) ((n k) 'lit)
        //                                                ((n x) 'notlit))))
        //                (n e)))))
        //   (m k)  ;; => lit, per Chez and Gauche; was 'notlit
        return match heap.get_symbol_or_identifier_name(input) {
            Some(input_name) => pat_name.as_ref() == input_name,
            None => false,
        };
    }

    // For non-identifier types (booleans, numbers, etc.), use heap equality
    heap.tagged_values_equal(pattern_lit, input)
}
