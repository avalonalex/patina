//! Literal matching: when an input matches a literal in a `syntax-rules`
//! pattern.
//!
//! R7RS §4.3.2 gives two ways for an input identifier to match a literal: the
//! two occurrences have the same lexical binding, or they are the same
//! identifier and neither has one. Both are answered here by *resolving* the
//! two — the literal where the macro was defined, the input where it is used —
//! against the scoped bindings the desugarer records for every binding form it
//! enters (`enter_binding_form`).

use patina_core::{Heap, TaggedValue};
use patina_runtime::{Environment, ScopeId, ScopeSet};
use std::rc::Rc;

/// One side of a literal comparison: the environment an identifier resolves
/// in, and the scopes a reference written there stands in when it carries
/// none of its own — the rule `Desugarer::resolve_syntax` applies to every
/// head it resolves, so a literal is compared by the binding a reference in
/// the same place would reach.
#[derive(Clone, Copy)]
pub struct Site<'a> {
    pub env: Option<&'a Rc<Environment>>,
    pub scopes: &'a ScopeSet,
}

/// Does `input` match the pattern literal `lit`?
///
/// Identifiers match when both reach the same local binding, or neither
/// reaches one and they are spelled alike — or, spelled differently, when
/// neither is local and both name one global value ([`denotes_same_binding`]).
/// A literal that is not an identifier is a datum and compares as one.
///
/// This used to be a spelling test with a veto: the desugarer kept the set of
/// names bound anywhere around the use site, and an input whose spelling was
/// in it could not match. A set of spellings cannot say *which* binding, and
/// the rules added to decide whose identifiers the veto could reach moved the
/// error between shapes rather than removing it. A template that binds `token`
/// and passes it to a helper whose literal is an outer `token` took the
/// literal arm; the same template passing the user's own `token` — the outer
/// one, and so the literal's binding — was refused. Triage family 41; chibi and
/// Gauche answer both by binding, and so does this.
///
/// `Err` is an ambiguous resolution. It is refused rather than treated as a
/// mismatch, which would hand the reference to the next rule to decide.
pub fn matches_literal(
    lit: TaggedValue,
    input: TaggedValue,
    heap: &Heap,
    definition: Site<'_>,
    use_site: Site<'_>,
    macro_scope: Option<ScopeId>,
) -> Result<bool, String> {
    let Some(lit_name) = heap.get_symbol_or_identifier_name(lit) else {
        return Ok(heap.tagged_values_equal(lit, input));
    };
    let Some(input_name) = heap.get_symbol_or_identifier_name(input) else {
        return Ok(false);
    };
    // The expansion flipped its own scope onto the input, so an input
    // identifier carrying nothing else was written at the use site. The scope
    // can stay in the set it is resolved with: it was minted after every
    // binding that exists, so no candidate carries it.
    let lit_binding = || local_binding(lit_name, scopes_of(lit, heap), None, definition);
    let input_binding = || local_binding(input_name, scopes_of(input, heap), macro_scope, use_site);

    if lit_name == input_name {
        return Ok(lit_binding()? == input_binding()?);
    }
    // Spelled differently, the two can still be one binding — a global
    // imported under a rename. A local binding is never renamed, so either
    // side reaching one rules it out.
    Ok(
        denotes_same_binding(lit, input, heap, definition.env, use_site.env)
            && lit_binding()?.is_none()
            && input_binding()?.is_none(),
    )
}

/// The local binding `name` reaches from `site`, named by its scope set; `None`
/// for a global or an unbound name.
///
/// `own` is what the identifier carries. Carrying nothing but `ignoring` means
/// it was written at the site, and so stands in the site's scopes.
fn local_binding(
    name: &str,
    own: ScopeSet,
    ignoring: Option<ScopeId>,
    site: Site<'_>,
) -> Result<Option<ScopeSet>, String> {
    let Some(env) = site.env else {
        return Ok(None);
    };
    let scopes = if own.iter().all(|scope| Some(*scope) == ignoring) {
        site.scopes
    } else {
        &own
    };
    env.scoped_binding_of(name, scopes)
        .map_err(|ambiguous| ambiguous.to_string())
}

fn scopes_of(tv: TaggedValue, heap: &Heap) -> ScopeSet {
    heap.get_identifier_data_any(tv)
        .map(|(_, scopes)| scopes)
        .unwrap_or_default()
}

/// R7RS §4.3.2's same-binding half for two *differently spelled* names: a
/// keyword imported under a rename still matches its own literal.
///
/// ```scheme
/// (import (scheme base) (rename (scheme base) (else alt)))
/// (cond (#f 1) (alt 42))     ;; chibi => 42
/// ```
///
/// Each name is resolved in its own environment — the literal's in the one the
/// macro was defined in, the input's at the use site — which is what the report
/// specifies. [`matches_literal`] only asks once neither side is local.
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
