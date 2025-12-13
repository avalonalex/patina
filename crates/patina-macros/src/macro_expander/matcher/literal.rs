//! Literal matching and hygiene support
//!
//! This module implements literal matching for pattern matching, including
//! R7RS hygiene support for shadowed literals.

use patina_runtime::{LiteralBinding, ScopeSet, Value};
use std::collections::HashSet;
use std::rc::Rc;

/// Check if a literal identifier is shadowed at the macro use site.
///
/// R7RS 4.3.2: A literal identifier matches an input identifier if both have
/// the same binding, or both are unbound and have the same name.
///
/// This function implements the `bound-identifier=?` semantics:
/// - If both literal and input are bound with the same scopes, they match
/// - If both are unbound and have the same name, they match
/// - If one is bound and the other is unbound (or differently bound), they DON'T match
///
/// # Arguments
/// * `lit` - The literal value from the pattern
/// * `input` - The input value being matched
/// * `shadowed_names` - Names shadowed by local bindings at macro use site
/// * `use_site_scopes` - Scopes at the macro use site for binding comparison
/// * `literals` - Literal identifiers with binding information from macro definition
pub fn is_literal_shadowed(
    lit: &Value,
    input: &Value,
    shadowed_names: &HashSet<Rc<str>>,
    use_site_scopes: &ScopeSet,
    literals: &[LiteralBinding],
) -> bool {
    // Extract the literal name from the pattern
    let lit_name = match lit {
        Value::Symbol(s) => s.clone(),
        Value::Identifier(id) => id.name.clone(),
        _ => return false, // Non-identifier literals can't be shadowed
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

    // Extract the input name (scopes are extracted later from the input Value itself)
    let input_name = match input {
        Value::Symbol(s) => s.clone(),
        Value::Identifier(id) => id.name.clone(),
        _ => return false, // Input is not an identifier
    };

    // Names must match for shadowing to be relevant
    if lit_name.as_ref() != input_name.as_ref() {
        return false;
    }

    // Now apply bound-identifier=? semantics:
    // Two identifiers are bound-identifier=? if they have the same binding.
    //
    // Case 1: Both unbound (literal.binding_scopes = None, input not shadowed)
    //         -> They should MATCH, so return false (not shadowed)
    //
    // Case 2: Both bound with the same scopes
    //         -> They should MATCH, so return false (not shadowed)
    //
    // Case 3: Literal is bound, input has a different binding (shadowed at use site)
    //         -> They should NOT match, so return true (shadowed)
    //
    // Case 4: Literal is unbound, input is bound (shadowed at use site)
    //         -> They should NOT match, so return true (shadowed)

    match &literal_binding.binding_scopes {
        None => {
            // Literal was unbound at macro definition time
            // Check if input is also unbound (not shadowed)
            // If input is in shadowed_names, it means a new binding exists at use site
            if shadowed_names.contains(&input_name) {
                // Input is bound at use site, but literal was unbound -> different bindings
                return true; // Shadowed (should NOT match)
            }
            // Both unbound with same name -> same binding
            false // Not shadowed (should match)
        }
        Some(literal_scopes) => {
            // Literal was bound at macro definition time
            // Check if input has the same binding

            // First, check if input is explicitly shadowed by a new binding
            if shadowed_names.contains(&input_name) {
                // Input has a new binding at use site
                // Compare the binding scopes: if different, they don't match
                // We consider them the same binding if the literal's scopes equals the use-site scopes
                if literal_scopes == use_site_scopes {
                    // Same scopes = same binding (this is the key fix!)
                    false // Not shadowed (should match)
                } else {
                    // Different scopes = different bindings
                    true // Shadowed (should NOT match)
                }
            } else {
                // Input is not shadowed by a new binding
                // If literal was bound but input is in a context without that binding,
                // they may still match if they're in the same lexical scope

                // Check if input identifier has scopes that include the literal's scopes
                if let Value::Identifier(id) = input {
                    // If input's scopes are a superset of literal's scopes, they're in the same context
                    if literal_scopes.is_subset_of(&id.scopes) {
                        false // Same or compatible binding
                    } else {
                        // Different scopes suggest different bindings
                        // But if the literal has scopes and input doesn't, and input is not shadowed,
                        // we need to check if they refer to the same thing
                        true // Different binding contexts
                    }
                } else {
                    // Input is a bare Symbol - it's at top level
                    // If literal was bound with scopes, they might be different
                    // Only return false if the literal scopes are empty (global binding)
                    !literal_scopes.is_empty()
                }
            }
        }
    }
}

/// Check if two values match as literals.
///
/// For identifier types (Symbol and Identifier), we compare by name only.
/// This is necessary because during recursive macro expansion, a literal
/// identifier may be transformed from a Symbol to an Identifier with scopes
/// when it passes through pattern variable substitution.
///
/// Example: In `(cond (#f 1) (else 2))`:
/// 1. First expansion: `else` is captured by pattern var `clause`
/// 2. During substitution, `else` becomes Identifier with macro_scope
/// 3. Recursive expansion: `(cond (else 2))` should still match the `else` literal
///
/// This implements R7RS literal matching semantics:
/// - Symbols match Symbols by name
/// - Symbols match Identifiers by name (for compatibility during macro expansion)
/// - Identifiers with scopes match using bound-identifier=? semantics:
///   they must have the same name AND the same scopes
///
/// The bound-identifier=? check is crucial for nested macro hygiene:
/// when a macro generates another macro, identifiers introduced by
/// the outer template should only match input with the same binding.
pub fn values_match_as_literal(pattern_lit: &Value, input: &Value) -> bool {
    match (pattern_lit, input) {
        // Symbol vs Symbol: compare by name
        (Value::Symbol(pat_name), Value::Symbol(inp_name)) => pat_name == inp_name,

        // Symbol vs Identifier: compare by name (Symbol acts as "any binding")
        (Value::Symbol(pat_name), Value::Identifier(inp_id)) => {
            pat_name.as_ref() == inp_id.name.as_ref()
        }

        // Identifier vs Symbol: compare by name
        (Value::Identifier(pat_id), Value::Symbol(inp_name)) => {
            pat_id.name.as_ref() == inp_name.as_ref()
        }

        // Identifier vs Identifier: bound-identifier=? semantics
        //
        // R7RS 4.3.2: A literal identifier matches an input identifier if both have
        // the same binding, or both are unbound and have the same name.
        //
        // In our scope-set based system:
        // 1. Empty pattern scopes = substituted from outer expansion = matches anything
        // 2. Otherwise, pattern.scopes must be a SUBSET of input.scopes
        //    - This handles the case where input passes through additional expansions
        //    - The subset relationship means they come from the same binding context
        //
        // Note: Shadowing is checked separately in is_literal_shadowed() before we get here.
        // If the input identifier is shadowed by a local binding, that check will fail
        // the match before reaching this function.
        //
        // Example:
        //   (let-syntax ((m (syntax-rules ()
        //     ((m _) (let-syntax ((n (syntax-rules (k) ((n k) 'match) ((n y) 'no))))
        //              (n k))))))
        //     (m x))
        //   => 'match
        // The literal `k` in `n`'s pattern has scopes {S1, S2}.
        // The input `k` in `(n k)` has scopes {S1, S2, S3} after flip.
        // Since {S1, S2} ⊆ {S1, S2, S3}, they match.
        (Value::Identifier(pat_id), Value::Identifier(inp_id)) => {
            // Pattern with empty scopes = substituted from outer expansion
            // It matches ANY identifier (regardless of name or scopes)
            if pat_id.scopes.is_empty() {
                return true;
            }
            // Otherwise, bound-identifier=? check using subset relationship:
            // Same name AND pattern's scopes are a subset of input's scopes
            pat_id.name.as_ref() == inp_id.name.as_ref()
                && pat_id.scopes.is_subset_of(&inp_id.scopes)
        }

        // For non-identifier types, use exact comparison via Debug format
        _ => format!("{:?}", pattern_lit) == format!("{:?}", input),
    }
}
