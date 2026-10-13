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

use patina_core::{Cx, Value};

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

/// Core macro expansion, in a context: values in, values out
///
/// Matches the input with `match_pattern_tagged` and produces the expansion
/// with the expander, both through `cx`.
///
/// # Arguments
/// * `cx` - The context the input lives in and the expansion is allocated in
/// * `compiled_macro` - The compiled macro definition
/// * `mac` - The macro object it is the body of, which holds its literals
/// * `flipped_args` - Arguments with macro_scope already flipped
/// * `macro_scope` - The fresh scope for this expansion
/// * `original_args` - Original unflipped args (for debug logging)
/// * `use_site` - Where an input identifier resolves when it meets a literal
/// * `depth` - How many forms deep the use is ([`Expander::at_depth`])
#[expect(
    clippy::too_many_arguments,
    reason = "each is one fact about the expansion, and the one caller passes them through"
)]
fn expand_macro_core_tagged<'gc>(
    cx: &mut Cx<'gc>,
    compiled_macro: &CompiledMacro,
    mac: Value<'gc>,
    flipped_args: Value<'gc>,
    macro_scope: patina_runtime::ScopeId,
    original_args: Value<'gc>,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
    depth: usize,
) -> Result<Value<'gc>, crate::error::MacroError> {
    use debug::{DebugContext, record_expansion_step};

    // Set up debug context
    let debug_ctx = DebugContext::new(
        patina_runtime::macro_debug::is_enabled(),
        crate::tracer::MacroTracer::should_trace(&compiled_macro.name),
    );

    // Log expansion start (debug functions handle values directly)
    debug_ctx.log_expansion_start(
        &compiled_macro.name,
        macro_scope,
        &compiled_macro.definition_scopes,
        &compiled_macro.rules,
        original_args,
        cx,
    );
    debug_ctx.log_input_flip(macro_scope, flipped_args, cx);

    // Create expander with macro scope for hygiene
    let expander = Expander::new(macro_scope).for_macro(mac).at_depth(depth);

    // Try each rule until we find a match
    for (rule_idx, rule) in compiled_macro.rules.iter().enumerate() {
        debug_ctx.log_trying_rule(rule_idx, rule);

        // Create matcher for this rule with hygiene support
        let matcher = Matcher::new_with_names(rule.num_pvars, rule.pvar_names.clone())
            .with_sites(definition, use_site)
            .with_macro_scope(macro_scope)
            .for_macro(mac);

        // Try to match against the pattern
        match matcher.match_pattern_tagged(cx, &rule.pattern, flipped_args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                debug_ctx.log_match_success(&match_env, &rule.pvar_names, &rule.template);

                let expanded =
                    expander
                        .expand(cx, &rule.template, &match_env)
                        .map_err(|e| match e {
                            ExpandError::NestedTooDeeply => {
                                crate::error::MacroError::NestedTooDeeply
                            }
                            e => crate::error::MacroError::InvalidSyntax(e.to_string()),
                        })?;

                debug_ctx.log_before_output_flip(expanded, cx);
                debug_ctx.log_expansion_complete(&compiled_macro.name, macro_scope, expanded, cx);

                // Record expansion step for tracing
                record_expansion_step(
                    debug_ctx.should_trace,
                    &compiled_macro.name,
                    rule_idx,
                    compiled_macro.rules.len(),
                    original_args,
                    cx,
                );

                return Ok(expanded);
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

/// Flip a scope on all identifiers in a tree (Racket-style hygiene)
///
/// Traverses the heap structure (pairs, vectors, identifiers) and toggles
/// the given scope on every identifier found.
///
/// Only allocates new heap objects when scopes actually change.
///
/// # Arguments
/// * `tv` - The tree to flip scopes on
/// * `scope` - The scope to flip (add if absent, remove if present)
/// * `cx` - The context it is read and allocated through
pub fn flip_scope_on_tagged<'gc>(
    tv: Value<'gc>,
    scope: patina_runtime::ScopeId,
    cx: &mut Cx<'gc>,
) -> Value<'gc> {
    edit_scopes(tv, scope, ScopeEdit::Flip, cx)
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
pub fn add_scope_to_bound_names<'gc>(
    tv: Value<'gc>,
    names: &std::collections::HashSet<std::rc::Rc<str>>,
    scope: patina_runtime::ScopeId,
    cx: &mut Cx<'gc>,
) -> Value<'gc> {
    edit_scopes(tv, scope, ScopeEdit::AddToBound(names), cx)
}

pub fn add_scope_to_scoped_identifiers<'gc>(
    tv: Value<'gc>,
    scope: patina_runtime::ScopeId,
    cx: &mut Cx<'gc>,
) -> Value<'gc> {
    edit_scopes(tv, scope, ScopeEdit::AddToScoped, cx)
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

/// The scope walks: each identifier `edit` affects gets `scope` flipped or
/// added, through [`Cx::rescope_identifiers`], which copies only what
/// changes, shared structure and cycles included, and answers `tv` itself
/// when nothing does.
fn edit_scopes<'gc>(
    tv: Value<'gc>,
    scope: patina_runtime::ScopeId,
    edit: ScopeEdit<'_>,
    cx: &mut Cx<'gc>,
) -> Value<'gc> {
    cx.rescope_identifiers(
        tv,
        |name, scopes| edit.affects(name, scopes),
        |scopes| match edit {
            ScopeEdit::Flip => scopes.flip_scope(scope),
            ScopeEdit::AddToScoped | ScopeEdit::AddToBound(_) => scopes.with_scope(scope),
        },
    )
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

/// What one expansion produced, in the context it was expanded in.
pub struct MacroExpansion<'gc> {
    /// The expanded form.
    pub form: Value<'gc>,
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
/// * `mac` - The macro object: its compiled body, and the literals it names
/// * `args` - The macro call
/// * `cx` - The context the call lives in and the expansion is allocated in
/// * `use_site` - Where the macro is being used, which is where an input
///   identifier resolves when it is compared with a literal (R7RS §4.3.2).
///   `None` for the direct-API paths, where literals compare by spelling
///
/// # Panics
///
/// If `mac` is no macro object.
pub fn expand_macro_with_scope<'gc>(
    mac: Value<'gc>,
    args: Value<'gc>,
    cx: &mut Cx<'gc>,
    use_site: Option<Site<'_>>,
) -> Result<MacroExpansion<'gc>, crate::error::MacroError> {
    let compiled_macro = macro_body(mac, cx);
    let definition = compiled_macro.definition_env.as_ref().map(|env| Site {
        env,
        scopes: &compiled_macro.definition_scopes,
    });
    expand_macro_at_sites(mac, args, cx, definition, use_site, 0)
}

/// The compiled body of the macro object `mac`.
fn macro_body<'gc>(mac: Value<'gc>, cx: &Cx<'gc>) -> std::rc::Rc<CompiledMacro> {
    cx.get_macro(mac)
        .expect("a macro is expanded through its macro object")
}

/// Expand using explicit lookup sites. The frontend supplies transient views of
/// declarations encountered in the current form, without capturing those views
/// in the compiled transformer or changing its runtime definition environment.
///
/// `depth` is how many forms deep the use is, counting expansions, which
/// limits how deep a value it substitutes may nest ([`Expander::at_depth`],
/// #617).
///
/// # Panics
///
/// If `mac` is no macro object.
pub fn expand_macro_at_sites<'gc>(
    mac: Value<'gc>,
    args: Value<'gc>,
    cx: &mut Cx<'gc>,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
    depth: usize,
) -> Result<MacroExpansion<'gc>, crate::error::MacroError> {
    use crate::tracer::MacroTracer;

    let compiled_macro = macro_body(mac, cx);

    // Enter macro expansion (for depth tracking)
    MacroTracer::enter_expansion();

    // Create a fresh macro scope for this expansion (Racket-style hygiene)
    let macro_scope = patina_runtime::ScopeId::fresh();

    // Step 1: Flip input scopes
    let flipped_args = flip_scope_on_tagged(args, macro_scope, cx);

    // Step 2-3: match and expand, values in and out
    let expanded = expand_macro_core_tagged(
        cx,
        &compiled_macro,
        mac,
        flipped_args,
        macro_scope,
        args, // original args for debug logging
        definition,
        use_site,
        depth,
    );

    // Exit expansion (decrement depth) — on the error path too. A caller that
    // recovers from a failed expansion, as the desugarer's body pre-pass does,
    // would otherwise leave the tracer one level deeper for each failure.
    MacroTracer::exit_expansion();
    let expanded = expanded?;

    // Step 4: Flip output scopes on expanded result
    let result = flip_scope_on_tagged(expanded, macro_scope, cx);

    Ok(MacroExpansion {
        form: result,
        scope: macro_scope,
    })
}

/// A macro object with no rules, holding `literals`: what a test of the
/// expander or the matcher reads a literal template or pattern through
/// (#718).
#[cfg(test)]
pub(crate) fn test_macro_object<'gc>(cx: &mut Cx<'gc>, literals: Vec<Value<'gc>>) -> Value<'gc> {
    let body = CompiledMacro {
        name: "test".into(),
        rules: vec![],
        max_pvars: 0,
        definition_scopes: patina_runtime::ScopeSet::new(),
        template_symbols: Default::default(),
        inherited_identifiers: Default::default(),
        definition_env: None,
        foreign_expansions: vec![],
    };
    cx.alloc_macro(std::rc::Rc::new(body), literals)
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
        // The parser answers a word: it crosses into the context through a
        // handle, as the desugarer's do.
        let form = heap.borrow().hold(form);
        let instance = Rc::clone(heap.borrow().instance());
        let mut driver = patina_core::GcDriver::acquire(&heap).expect("the driver is free");
        driver.mutate(&instance, |cx| {
            let form = cx.held(&form);
            let parsed = parse_syntax_rules(form, cx).expect("syntax-rules");
            let mut compiler = Compiler::with_env(
                parsed.literals,
                parsed.custom_ellipsis,
                definition_env.clone(),
            );
            let (compiled, literals) = compiler
                .compile_macro(cx, "m".into(), parsed.rules)
                .expect("compiled");
            let mac = cx.alloc_macro(Rc::new(compiled), literals);

            #[expect(
                clippy::disallowed_methods,
                reason = "a test's use-site environment, binding only `UNSPECIFIED`; the test \
                          evaluates nothing"
            )]
            let use_site_env = Rc::new(Environment::with_parent(definition_env));
            for scopes in bindings {
                use_site_env.define_with_scopes("else", scopes.clone(), TaggedValue::UNSPECIFIED);
            }
            let input = cx.alloc_identifier(Rc::from("else"), reference);
            let tail = cx.alloc_pair(input, Value::NULL);
            let head = cx.intern_symbol("m");
            let args = cx.alloc_pair(head, tail);
            let use_site_scopes = ScopeSet::new();
            let expansion = expand_macro_with_scope(
                mac,
                args,
                cx,
                Some(Site {
                    env: &use_site_env,
                    scopes: &use_site_scopes,
                }),
            )?;
            Ok(cx.format(expansion.form))
        })
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
