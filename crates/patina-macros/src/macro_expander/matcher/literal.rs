//! Literal matching: when an input matches a literal in a `syntax-rules`
//! pattern.
//!
//! R7RS §4.3.2 gives two ways for an input identifier to match a literal: the
//! two occurrences have the same lexical binding, or they are the same
//! identifier and neither has one. Both are answered here by *resolving* the
//! two — the literal where the macro was defined, the input where it is used —
//! against the scoped bindings the desugarer records for every binding form it
//! enters (`enter_binding_form`).

use crate::macro_expander::Site;
use patina_core::{Heap, TaggedValue};
use patina_runtime::{ScopeId, ScopeSet};

/// Does `input` match the pattern literal `lit`?
///
/// Identifiers match when both reach the same local binding, or neither
/// reaches one and they are spelled alike and not two different globals
/// ([`distinct_globals`]) — or, spelled differently, when neither is local and
/// both name one global value ([`denotes_same_binding`]).
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
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
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
        let (lit_local, input_local) = (lit_binding()?, input_binding()?);
        if lit_local.is_some() || input_local.is_some() {
            return Ok(lit_local == input_local);
        }
        return Ok(!distinct_globals(lit_name, definition, use_site));
    }
    // Spelled differently, the two can still be one binding — a global
    // imported under a rename. A local binding is never renamed, so either
    // side reaching one rules it out.
    Ok(
        denotes_same_binding(lit_name, input_name, definition, use_site)
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
    site: Option<Site<'_>>,
) -> Result<Option<ScopeSet>, String> {
    let Some(site) = site else {
        return Ok(None);
    };
    let scopes = if own.iter().all(|scope| Some(*scope) == ignoring) {
        site.scopes
    } else {
        &own
    };
    site.env
        .scoped_binding_of(name, scopes)
        .map_err(|ambiguous| ambiguous.to_string())
}

/// Whether `name` is a global on both sides, and a different one on each.
///
/// Neither side reaching a local binding used to settle a pair spelled alike
/// as a match, whatever the two globals were. So once a program defined over
/// an imported `else`, `(cond (#f 0) (else 1))` still took the else clause —
/// the program's variable and `(scheme base)`'s keyword read as one binding
/// because neither is local. chibi and Gauche evaluate the variable (#450).
/// The same shape for a template's *reference* was #407: an import is the
/// exporting library's location (#406), so where the two names lead is a
/// question [`Environment::binding_location`] answers exactly.
///
/// Only two bound names at different locations *holding different values*
/// are told apart. A name bound on one side and unbound on the other matches
/// as it did, as does a name with no site to look it up in.
///
/// The value check is for the one import that is not the library's location:
/// an export `share_binding` cannot share — a definition a macro introduced,
/// or one reached through an alias — is installed as a copy, so the importer
/// has a location of its own holding what the library's holds. By location
/// alone a library that exports such a name and uses it as its macro's
/// literal stopped matching its own importer's use of it, where chibi, Gauche
/// and the matcher before #450 all match. A program's definition over an
/// import holds a value of its own, so this does not undo #450.
///
/// [`Environment::binding_location`]: patina_runtime::Environment::binding_location
fn distinct_globals(name: &str, definition: Option<Site<'_>>, use_site: Option<Site<'_>>) -> bool {
    let (Some(definition), Some(use_site)) = (definition, use_site) else {
        return false;
    };
    match (
        definition.env.binding_location(name),
        use_site.env.binding_location(name),
    ) {
        (Some(lit), Some(input)) if lit != input => {
            definition.env.get(name) != use_site.env.get(name)
        }
        _ => false,
    }
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
/// specifies. [`matches_literal`] asks only about names spelled differently,
/// and only once neither reaches a local binding; it settles a pair spelled
/// alike by binding before it gets here.
///
/// The comparison is on the *value*, and only when that value is a heap object.
/// Environments hold values, not binding identities (an import under a rename
/// copies the value into a fresh binding, so slot identity would say "different"
/// for the very case this exists for), and restricting it to heap objects keeps
/// two unrelated names that merely both hold `#t` or `0` from being called the
/// same binding. Syntactic keywords — what auxiliary literals actually are —
/// are interned markers, so identity is exact for them.
fn denotes_same_binding(
    lit_name: &str,
    input_name: &str,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
) -> bool {
    let (Some(definition), Some(use_site)) = (definition, use_site) else {
        return false;
    };
    match (definition.env.get(lit_name), use_site.env.get(input_name)) {
        (Some(lit_value), Some(input_value)) => lit_value.is_object() && lit_value == input_value,
        _ => false,
    }
}
