//! Literal matching and hygiene support
//!
//! This module implements literal matching for pattern matching, including
//! R7RS hygiene support for shadowed literals.

use patina_runtime::Value;
use std::collections::HashSet;
use std::rc::Rc;

/// Check if a literal identifier is shadowed at the macro use site.
///
/// R7RS 4.3.2: A literal identifier matches an input identifier if both have
/// the same binding, or both are unbound and have the same name.
///
/// If the literal is shadowed by a local binding at the use site, it should
/// NOT match, allowing the clause to fall through to other patterns.
///
/// # Arguments
/// * `lit` - The literal value from the pattern
/// * `input` - The input value being matched
/// * `shadowed_names` - Names shadowed by local bindings at macro use site
/// * `literals` - Literal identifiers from the macro definition
pub fn is_literal_shadowed(
    lit: &Value,
    input: &Value,
    shadowed_names: &HashSet<Rc<str>>,
    literals: &[Rc<str>],
) -> bool {
    // If no shadowed names, nothing can be shadowed
    if shadowed_names.is_empty() {
        return false;
    }

    // Extract the literal name from the pattern
    let lit_name = match lit {
        Value::Symbol(s) => s.clone(),
        Value::Identifier(id) => id.name.clone(),
        _ => return false, // Non-identifier literals can't be shadowed
    };

    // Only check literals that are in the macro's literals list
    if !literals.iter().any(|l| l.as_ref() == lit_name.as_ref()) {
        return false;
    }

    // Extract the input name
    let input_name = match input {
        Value::Symbol(s) => s.clone(),
        Value::Identifier(id) => id.name.clone(),
        _ => return false, // Input is not an identifier
    };

    // Names must match for shadowing to be relevant
    if lit_name.as_ref() != input_name.as_ref() {
        return false;
    }

    // Check if the input name is in the shadowed_names set
    // This is compile-time shadowing from lambda parameters, let bindings, etc.
    shadowed_names.contains(&input_name)
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
