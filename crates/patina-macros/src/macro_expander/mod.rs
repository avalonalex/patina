//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic macro expansion using Racket-style scope sets
//!
//! Based on Gauche Scheme's PVREF encoding and Racket's "Binding as Sets of Scopes" (Flatt 2016).

pub mod compiler;
mod debug;
pub mod expander;
pub mod identifier_key;
pub mod interface;
pub mod matcher;
pub mod syntax_rules_parser;
pub mod utils;
pub mod validator;

#[cfg(test)]
mod ellipsis_edge_cases_tests;
#[cfg(test)]
mod pattern_template_tests;

// Re-export core types from patina-runtime
pub use patina_runtime::{Identifier, Pattern, Template};

// Re-export macro system types
pub use compiler::{CompiledMacro, CompiledRule, Compiler};
pub use expander::{ExpandError, Expander};
pub use identifier_key::IdentifierKey;
pub use matcher::{MatchError, Matcher};
pub use syntax_rules_parser::{ParsedSyntaxRules, SyntaxRulesParseError, parse_syntax_rules};

// Re-export test helpers
pub use interface::TestExpander;

// Re-export utility functions and constants
pub use utils::{
    ELLIPSIS, MACRO_DEFINITION_FORMS, WILDCARD, pattern_to_string, pattern_to_string_with_names,
    template_to_string_with_names,
};

/// Core macro expansion with TaggedValue input and output
///
/// Matches directly on TaggedValue input using `match_pattern_tagged`
/// and produces TaggedValue output via the expander.
///
/// # Arguments
/// * `compiled_macro` - The compiled macro definition
/// * `flipped_args` - TaggedValue arguments with macro_scope already flipped
/// * `macro_scope` - The fresh scope for this expansion
/// * `original_args` - Original unflipped TaggedValue args (for debug logging)
/// * `shared_heap` - Shared heap for TaggedValue operations
/// * `use_site` - Where an input identifier resolves when it meets a literal
fn expand_macro_core_tagged(
    compiled_macro: &CompiledMacro,
    flipped_args: patina_core::TaggedValue,
    macro_scope: patina_runtime::ScopeId,
    original_args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
) -> Result<patina_core::TaggedValue, crate::error::MacroError> {
    use debug::{DebugContext, record_expansion_step};

    // Set up debug context
    let debug_ctx = DebugContext::new(
        patina_runtime::macro_debug::is_enabled(),
        crate::tracer::MacroTracer::should_trace(&compiled_macro.name),
    );

    // Log expansion start (debug functions handle TaggedValue directly)
    debug_ctx.log_expansion_start(
        &compiled_macro.name,
        macro_scope,
        &compiled_macro.definition_scopes,
        &compiled_macro.rules,
        original_args,
        shared_heap,
    );
    debug_ctx.log_input_flip(macro_scope, flipped_args, shared_heap);

    // Create expander with macro scope for hygiene
    let expander = Expander::new_with_heap(macro_scope, shared_heap.clone());

    // Try each rule until we find a match
    for (rule_idx, rule) in compiled_macro.rules.iter().enumerate() {
        debug_ctx.log_trying_rule(rule_idx, rule);

        // Create matcher for this rule with hygiene support and shared heap
        let matcher =
            Matcher::new_with_heap(rule.num_pvars, rule.pvar_names.clone(), shared_heap.clone())
                .with_sites(definition, use_site)
                .with_macro_scope(macro_scope);

        // Try to match against the pattern
        match matcher.match_pattern_tagged(&rule.pattern, flipped_args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                debug_ctx.log_match_success(&match_env, &rule.pvar_names, &rule.template);

                // Expand the template into a TaggedValue
                let expanded_tagged = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::MacroError::InvalidSyntax(e.to_string()))?;

                // Debug logging (functions handle TaggedValue directly)
                debug_ctx.log_before_output_flip(expanded_tagged, shared_heap);
                debug_ctx.log_expansion_complete(
                    &compiled_macro.name,
                    macro_scope,
                    expanded_tagged,
                    shared_heap,
                );

                // Record expansion step for tracing
                record_expansion_step(
                    debug_ctx.should_trace,
                    &compiled_macro.name,
                    rule_idx,
                    compiled_macro.rules.len(),
                    original_args,
                    shared_heap,
                );

                // Return TaggedValue directly (no conversion needed!)
                return Ok(expanded_tagged);
            }
            // Not a failed match: the literal comparison has no answer, and
            // trying the next rule would give it one by rule order.
            Err(MatchError::AmbiguousLiteral(message)) => {
                return Err(crate::error::MacroError::AmbiguousReference(message));
            }
            Err(e) => {
                // This rule didn't match, try next one
                debug_ctx.log_match_failure(&e);
                continue;
            }
        }
    }

    // No rule matched
    debug_ctx.log_no_rules_matched();

    Err(crate::error::MacroError::NoMatchingPattern(
        compiled_macro.name.to_string(),
    ))
}

/// Flip a scope on all identifiers in a TaggedValue tree (Racket-style hygiene)
///
/// Traverses the heap structure (pairs, vectors, identifiers) and toggles
/// the given scope on every identifier found.
///
/// Only allocates new heap objects when scopes actually change.
///
/// # Arguments
/// * `tv` - The TaggedValue tree to flip scopes on
/// * `scope` - The scope to flip (add if absent, remove if present)
/// * `shared_heap` - Shared heap for reading and allocating values
pub fn flip_scope_on_tagged(
    tv: patina_core::TaggedValue,
    scope: patina_runtime::ScopeId,
    shared_heap: &patina_core::SharedHeap,
) -> patina_core::TaggedValue {
    // Early exit: if no identifiers present, return value unchanged
    if !contains_edit_target(tv, ScopeEdit::Flip, shared_heap) {
        return tv;
    }

    edit_scope_on_tagged(tv, scope, ScopeEdit::Flip, shared_heap)
}

/// Add `scope` to every identifier in `tv` that already carries scopes of
/// its own; plain symbols and empty-scoped identifiers are left alone.
///
/// This is the desugarer's way of doing what Racket's expander does when it
/// enters a binding form: put the form's scope on the body *as written*, so
/// that a reference the body contains can be told from one a transformer
/// will introduce later. Symbols do not need it — the desugarer resolves
/// them with the scopes it has accumulated, which include this one — and
/// neither does an identifier with no scopes, which is a user's symbol that
/// passed through a pattern variable and resolves the same way. What needs
/// it is a macro-introduced identifier, which carries its own scopes and
/// nothing of where it now sits: the `(n z)` in chibi's `(m k)`, whose
/// binder `n` came from the same template.
pub fn add_scope_to_bound_names(
    tv: patina_core::TaggedValue,
    names: &std::collections::HashSet<std::rc::Rc<str>>,
    scope: patina_runtime::ScopeId,
    shared_heap: &patina_core::SharedHeap,
) -> patina_core::TaggedValue {
    let edit = ScopeEdit::AddToBound(names);
    if !contains_edit_target(tv, edit, shared_heap) {
        return tv;
    }
    edit_scope_on_tagged(tv, scope, edit, shared_heap)
}

pub fn add_scope_to_scoped_identifiers(
    tv: patina_core::TaggedValue,
    scope: patina_runtime::ScopeId,
    shared_heap: &patina_core::SharedHeap,
) -> patina_core::TaggedValue {
    if !contains_edit_target(tv, ScopeEdit::AddToScoped, shared_heap) {
        return tv;
    }
    edit_scope_on_tagged(tv, scope, ScopeEdit::AddToScoped, shared_heap)
}

/// What a scope walk does to each identifier it meets.
#[derive(Clone, Copy)]
enum ScopeEdit<'a> {
    /// Add the scope to identifiers naming one of these that already carry
    /// scopes of their own.
    AddToBound(&'a std::collections::HashSet<std::rc::Rc<str>>),
    /// Toggle the scope — the expander's input/output flip.
    Flip,
    /// Add the scope to identifiers that already have scopes; leave
    /// empty-scoped ones as they are.
    AddToScoped,
}

impl ScopeEdit<'_> {
    /// Would this edit change `tv`, an identifier with `scopes`?
    fn affects(self, name: &str, scopes: &patina_runtime::ScopeSet) -> bool {
        match self {
            ScopeEdit::Flip => true,
            ScopeEdit::AddToScoped => !scopes.is_empty(),
            ScopeEdit::AddToBound(names) => !scopes.is_empty() && names.contains(name),
        }
    }
}

/// Check if a TaggedValue tree contains any Identifier nodes
///
/// Fast traversal that returns true as soon as an Identifier is found. The
/// tree may be cyclic — a quoted datum with labels (`'#0=(a b . #0#)`) is a
/// legitimate macro argument, and Larceny's `base` suite hands `test` several
/// — so pairs and vectors are recorded once the walk is long enough to
/// suggest a cycle and never entered twice. Cycles only come from the
/// reader, whose data holds symbols, not identifiers, so a revisited
/// container contributes nothing.
fn contains_edit_target(
    tv: patina_core::TaggedValue,
    edit: ScopeEdit<'_>,
    shared_heap: &patina_core::SharedHeap,
) -> bool {
    let heap = shared_heap.borrow();
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![tv];
    while let Some(value) = pending.pop() {
        if !seen.insert(value.raw_bits()) {
            continue;
        }
        if !heap.is_source_identifier(value)
            && heap
                .get_identifier_data(value)
                .is_some_and(|(name, scopes)| edit.affects(name, scopes))
        {
            return true;
        }
        if value.is_pair() {
            let (car, cdr) = heap.get_pair(value);
            pending.extend([cdr, car]);
        } else if value.is_vector() {
            pending.extend(heap.vector_slice(value));
        }
    }
    false
}

/// Implementation of the scope walks for TaggedValue
///
/// Edit each syntax identifier once, retaining source provenance and graph
/// sharing. The heap's iterative copier closes cycles on the copied container
/// and leaves unaffected data untouched, including vectors supplied to `eval`.
fn edit_scope_on_tagged(
    tv: patina_core::TaggedValue,
    scope: patina_runtime::ScopeId,
    edit: ScopeEdit<'_>,
    shared_heap: &patina_core::SharedHeap,
) -> patina_core::TaggedValue {
    shared_heap
        .borrow_mut()
        .map_syntax_identifiers(tv, |heap, value, name, scopes| {
            if heap.is_source_identifier(value) || !edit.affects(&name, &scopes) {
                return None;
            }
            let new_scopes = match edit {
                ScopeEdit::Flip => scopes.flip_scope(scope),
                ScopeEdit::AddToScoped | ScopeEdit::AddToBound(_) => scopes.with_scope(scope),
            };
            Some(heap.alloc_identifier(name, new_scopes))
        })
}

/// One side of a literal comparison: where a macro was defined, or where it is
/// being used. The environment an identifier resolves in, and the scopes a
/// reference written there without scopes of its own stands in — the rule
/// `Desugarer::resolve_syntax` applies to every head it resolves, so a literal
/// is compared by the binding a reference in the same place would reach.
#[derive(Clone, Copy)]
pub struct Site<'a> {
    pub env: &'a std::rc::Rc<patina_runtime::Environment>,
    pub scopes: &'a patina_runtime::ScopeSet,
}

/// What one expansion produced.
pub struct MacroExpansion {
    /// The expanded form.
    pub form: patina_core::TaggedValue,
    /// The scope minted for this expansion. After the output flip it is on
    /// every identifier the template *introduced* and on nothing that came
    /// in through a pattern variable — the input flip put it there and the
    /// output flip took it off again — so a consumer of the form can tell
    /// the two apart by asking whether an identifier carries it.
    pub scope: patina_runtime::ScopeId,
}

/// Expand a macro, returning the expanded form and the scope this expansion
/// minted.
///
/// The desugarer's relinker needs that scope: a template's free reference to
/// `list` is aliased to the definition site's `list`, and only the
/// references the template introduced may be — the `(list 1 2 3)` the user
/// wrote *inside* the macro call means the use site's `list`, even when the
/// two differ.
///
/// # Arguments
/// * `compiled_macro` - The compiled macro definition
/// * `args` - The macro call arguments as TaggedValue
/// * `shared_heap` - Shared heap for conversions (Rc<RefCell<Heap>>)
/// * `use_site` - Where the macro is being used, which is where an input
///   identifier resolves when it is compared with a literal (R7RS §4.3.2).
///   `None` for the direct-API paths, where literals compare by spelling
pub fn expand_macro_with_scope(
    compiled_macro: &CompiledMacro,
    args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    use_site: Option<Site<'_>>,
) -> Result<MacroExpansion, crate::error::MacroError> {
    let definition = compiled_macro.definition_env.as_ref().map(|env| Site {
        env,
        scopes: &compiled_macro.definition_scopes,
    });
    expand_macro_at_sites(compiled_macro, args, shared_heap, definition, use_site)
}

/// Expand using explicit lookup sites. The frontend supplies transient views of
/// declarations encountered in the current form, without capturing those views
/// in the compiled transformer or changing its runtime definition environment.
pub fn expand_macro_at_sites(
    compiled_macro: &CompiledMacro,
    args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
) -> Result<MacroExpansion, crate::error::MacroError> {
    use crate::tracer::MacroTracer;

    // Enter macro expansion (for depth tracking)
    MacroTracer::enter_expansion();

    // Create a fresh macro scope for this expansion (Racket-style hygiene)
    let macro_scope = patina_runtime::ScopeId::fresh();

    // Step 1: Flip input scopes on TaggedValue (avoids Value allocation for flip)
    let flipped_args = flip_scope_on_tagged(args, macro_scope, shared_heap);

    // Step 2-3: Call core expansion with TaggedValue input and output (no conversion!)
    // Pattern matching works directly on TaggedValue
    // Template expansion now produces TaggedValue directly!
    let expanded_tagged = expand_macro_core_tagged(
        compiled_macro,
        flipped_args,
        macro_scope,
        args, // original args for debug logging
        shared_heap,
        definition,
        use_site,
    );

    // Exit expansion (decrement depth) — on the error path too. A caller that
    // recovers from a failed expansion, as the desugarer's body pre-pass does,
    // would otherwise leave the tracer one level deeper for each failure.
    MacroTracer::exit_expansion();
    let expanded_tagged = expanded_tagged?;

    // Step 4: Flip output scopes on expanded result
    let result = flip_scope_on_tagged(expanded_tagged, macro_scope, shared_heap);

    Ok(MacroExpansion {
        form: result,
        scope: macro_scope,
    })
}

/// A literal comparison the set-of-scopes rule cannot decide stops the
/// expansion and is reported as an ambiguous reference, rather than failing
/// the rule and letting a later one decide it by order; a comparison it can
/// decide picks a rule as usual.
#[cfg(test)]
mod ambiguous_literal_tests {
    use super::*;
    use patina_core::TaggedValue;
    use patina_runtime::{Environment, ScopeId, ScopeSet};
    use std::rc::Rc;

    /// Expand `(m else)`, where `m` is `(syntax-rules (else) ((_ else)
    /// 'literal) ((_ x) 'fallback))` defined where `else` is unbound, and the
    /// use site binds `else` locally at each of `bindings`. The input `else`
    /// carries `reference`.
    fn expand_against_local_elses(
        bindings: &[ScopeSet],
        reference: ScopeSet,
    ) -> Result<String, crate::error::MacroError> {
        use patina_frontend::parser::Parser;

        let definition_env = Rc::new(Environment::new());
        let heap = definition_env.heap().clone();
        let form = Parser::new_with_heap(
            "(syntax-rules (else) ((_ else) 'literal) ((_ x) 'fallback))",
            heap.clone(),
        )
        .expect("parser")
        .parse()
        .expect("syntax-rules form");
        let parsed = parse_syntax_rules(form, &heap.borrow()).expect("syntax-rules");
        let mut compiler = Compiler::with_env(
            parsed.literals,
            parsed.custom_ellipsis,
            definition_env.clone(),
            heap.clone(),
        );
        let compiled = compiler
            .compile_macro("m".into(), parsed.rules)
            .expect("compiled");

        let use_site_env = Rc::new(Environment::with_parent(definition_env));
        for scopes in bindings {
            use_site_env.define_with_scopes("else", scopes.clone(), TaggedValue::UNSPECIFIED);
        }
        let args = {
            let mut h = heap.borrow_mut();
            let input = h.alloc_identifier(Rc::from("else"), reference);
            let tail = h.alloc_pair(input, TaggedValue::NULL);
            let head = h.intern_symbol("m");
            h.alloc_pair(head, tail)
        };
        let use_site_scopes = ScopeSet::new();
        let expansion = expand_macro_with_scope(
            &compiled,
            args,
            &heap,
            Some(Site {
                env: &use_site_env,
                scopes: &use_site_scopes,
            }),
        )?;
        Ok(patina_core::format_tagged(expansion.form, &heap.borrow()))
    }

    #[test]
    fn an_undecidable_literal_stops_the_expansion_as_an_ambiguous_reference() {
        let (s1, s2) = (ScopeId::fresh(), ScopeId::fresh());
        let result = expand_against_local_elses(
            &[ScopeSet::singleton(s1), ScopeSet::singleton(s2)],
            ScopeSet::singleton(s1).with_scope(s2),
        );
        match result {
            Err(crate::error::MacroError::AmbiguousReference(message)) => {
                assert!(message.contains("ambiguous"), "{message}");
            }
            other => panic!(
                "expected an ambiguous reference, with the fallback rule never tried: {other:?}"
            ),
        }
    }

    #[test]
    fn a_local_else_the_rule_decides_takes_the_fallback_rule() {
        let (s1, s2) = (ScopeId::fresh(), ScopeId::fresh());
        let form = expand_against_local_elses(
            &[ScopeSet::singleton(s1)],
            ScopeSet::singleton(s1).with_scope(s2),
        )
        .expect("one candidate is not ambiguous");
        assert!(form.contains("fallback"), "{form}");
    }

    #[test]
    fn an_unbound_else_takes_the_literal_rule() {
        let form = expand_against_local_elses(&[], ScopeSet::singleton(ScopeId::fresh()))
            .expect("nothing to resolve");
        assert!(form.contains("literal"), "{form}");
    }
}
