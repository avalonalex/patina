//! Literal matching and hygiene support
//!
//! This module implements literal matching for pattern matching, including
//! R7RS hygiene support for shadowed literals.

use patina_core::{Heap, TaggedValue};
use patina_runtime::{Environment, LiteralBinding};
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
    literals: &[LiteralBinding],
    current_macro_scope: Option<patina_runtime::ScopeId>,
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

    // The shadow set is keyed by *spelling*: it says "something named this is
    // locally bound somewhere in the enclosing desugar", with no scope to say
    // where. That is enough for an identifier the user wrote, and not enough
    // for one a macro introduced — which denotes what it denoted where its
    // template was written, the whole point of hygiene.
    //
    // Scope tells the two apart. This expansion has already flipped its own
    // `macro_scope` onto everything it is matching, so that scope is on both.
    // An identifier the user wrote carries nothing else — a substitution
    // through an outer macro marks it and unmarks it again on the way out —
    // while an introduced one keeps the scope of the macro that introduced it.
    let introduced = |tv, flipped: bool| {
        heap.get_identifier_data_any(tv).is_some_and(|(_, scopes)| {
            scopes
                .iter()
                .any(|scope| !flipped || Some(*scope) != current_macro_scope)
        })
    };
    // The literal's own origin decides whose shadows can reach the input.
    //
    // A literal written in plain source — `cond`'s `else`, in `(scheme base)` —
    // can only be shadowed for an input written in plain source too. A
    // template-introduced `else` denoting base's `else` is a different
    // identifier from the use site's `(let ((else #f)) …)`, and vetoing it
    // rejected a legal program: since #89 the `else` clause demoted to a test
    // clause and `else` was then read as a value.
    //
    // A literal a template introduced — `(let-syntax ((n (syntax-rules (k) …`
    // inside a macro that also writes `(let ((k 99)) (n k))` — is in that
    // template's world, where the template's own bindings do shadow it.
    //
    // R7RS §4.3.2's own example is the first kind and still behaves:
    // `(let ((=> #f)) (cond (#t => 'ok)))` vetoes, because the `=>` is the
    // user's.
    let shadows_can_reach_input = introduced(lit, false) || !introduced(input, true);
    let shadowed_at_use_site = shadows_can_reach_input && shadowed_names.contains(&input_name);

    // Apply bound-identifier=? semantics
    match &literal_binding.binding_scopes {
        None => {
            // Literal was unbound at macro definition time
            if shadowed_at_use_site {
                return true; // Shadowed (should NOT match)
            }
            false // Not shadowed (should match)
        }
        Some(literal_scopes) => {
            // Literal was bound at macro definition time
            if shadowed_at_use_site {
                // Both name a bound identifier, so this is `bound-identifier=?`
                // proper: same binding only if they were written with the same
                // scopes. A literal a macro's own template introduced and bound
                // — `(let ((k 1)) (let-syntax ((n (syntax-rules (k) …))))` —
                // is not the user's `k`, whatever the two are spelled.
                let scopes_of = |tv| {
                    heap.get_identifier_data_any(tv)
                        .map(|(_, scopes)| scopes)
                        .unwrap_or_default()
                };
                scopes_of(lit) != scopes_of(input)
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

/// R7RS §4.3.2's other half: a literal also matches an input identifier that
/// *denotes the same binding*, however it is spelled.
///
/// [`tagged_matches_literal`] can only compare spellings, so a keyword imported
/// under a rename never matched its own literal:
///
/// ```scheme
/// (import (scheme base) (rename (scheme base) (else alt)))
/// (cond (#f 1) (alt 42))     ;; chibi => 42
/// ```
///
/// Each name is resolved in its own environment — the literal's in the one the
/// macro was defined in, the input's at the use site — which is what the report
/// specifies and what the spelling test approximates.
///
/// The comparison is on the *value*, and only when that value is a heap object.
/// Environments hold values, not binding identities (an import under a rename
/// copies the value into a fresh binding, so slot identity would say "different"
/// for the very case this exists for), and restricting it to heap objects keeps
/// two unrelated names that merely both hold `#t` or `0` from being called the
/// same binding. Syntactic keywords — what auxiliary literals actually are —
/// are interned markers, so identity is exact for them.
pub fn denotes_same_binding(
    lit: TaggedValue,
    input: TaggedValue,
    heap: &Heap,
    definition_env: Option<&Rc<Environment>>,
    use_site_env: Option<&Rc<Environment>>,
) -> bool {
    let (Some(definition_env), Some(use_site_env)) = (definition_env, use_site_env) else {
        return false;
    };
    let (Some(lit_name), Some(input_name)) = (
        heap.get_symbol_or_identifier_name(lit),
        heap.get_symbol_or_identifier_name(input),
    ) else {
        return false;
    };
    if lit_name == input_name {
        // The spelling test already answered this one, either way.
        return false;
    }
    match (definition_env.get(lit_name), use_site_env.get(input_name)) {
        (Some(lit_value), Some(input_value)) => lit_value.is_object() && lit_value == input_value,
        _ => false,
    }
}
