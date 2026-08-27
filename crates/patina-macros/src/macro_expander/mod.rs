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
/// * `shadowed_names` - Identifiers shadowed by local bindings
/// * `macro_scope` - The fresh scope for this expansion
/// * `original_args` - Original unflipped TaggedValue args (for debug logging)
/// * `shared_heap` - Shared heap for TaggedValue operations
#[allow(clippy::too_many_arguments)]
fn expand_macro_core_tagged(
    compiled_macro: &CompiledMacro,
    flipped_args: patina_core::TaggedValue,
    shadowed_names: &std::collections::HashSet<std::rc::Rc<str>>,
    macro_scope: patina_runtime::ScopeId,
    original_args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    use_site_env: Option<&std::rc::Rc<patina_runtime::Environment>>,
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
        let matcher = Matcher::new_with_heap(
            rule.num_pvars,
            rule.pvar_names.clone(),
            shadowed_names.clone(),
            compiled_macro.literals.clone(),
            shared_heap.clone(),
        )
        .with_environments(compiled_macro.definition_env.clone(), use_site_env.cloned())
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
            Err(e) => {
                // This rule didn't match, try next one
                debug_ctx.log_match_failure(&e);
                continue;
            }
        }
    }

    // No rule matched
    debug_ctx.log_no_rules_matched();

    Err(crate::error::MacroError::InvalidSyntax(format!(
        "No matching pattern for macro {}",
        compiled_macro.name
    )))
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
    if !contains_identifier_tagged(tv, shared_heap) {
        return tv;
    }

    flip_scope_on_tagged_impl(tv, scope, shared_heap)
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
fn contains_identifier_tagged(
    tv: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
) -> bool {
    let mut guard = CycleGuard::default();
    contains_identifier_impl(tv, shared_heap, &mut guard)
}

/// A revisit guard for walks over reader data that may be cyclic. Records
/// nothing for the first `BUDGET` pairs — the acyclic common case — and every
/// pair after that; `enter` says whether a pair is new.
#[derive(Default)]
pub(crate) struct CycleGuard {
    steps: usize,
    seen: std::collections::HashSet<u64>,
}

impl CycleGuard {
    const BUDGET: usize = 4096;

    /// `true` if the pair should be walked, `false` if it was walked already.
    pub(crate) fn enter(&mut self, tv: patina_core::TaggedValue) -> bool {
        self.steps += 1;
        self.steps <= Self::BUDGET || self.seen.insert(tv.raw_bits())
    }
}

fn contains_identifier_impl(
    tv: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    guard: &mut CycleGuard,
) -> bool {
    // Immediate values (fixnum, char, bool, null) never contain identifiers
    if tv.is_fixnum() || tv.is_char() || tv.is_special() {
        return false;
    }

    if tv.is_pair() {
        if !guard.enter(tv) {
            return false;
        }
        let (car, cdr) = shared_heap.borrow().get_pair(tv);
        return contains_identifier_impl(car, shared_heap, guard)
            || contains_identifier_impl(cdr, shared_heap, guard);
    }

    if tv.is_vector() {
        if !guard.enter(tv) {
            return false;
        }
        let elems = vector_elements(tv, shared_heap);
        return elems
            .into_iter()
            .any(|e| contains_identifier_impl(e, shared_heap, guard));
    }

    // Non-object types can't contain identifiers
    if !tv.is_object() {
        return false;
    }

    // Check for identifier (native or boxed) via unified method
    shared_heap.borrow().get_identifier_data_any(tv).is_some()
}

/// The elements of a heap vector, copied out so no borrow outlives the call.
fn vector_elements(
    tv: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
) -> Vec<patina_core::TaggedValue> {
    let heap = shared_heap.borrow();
    (0..heap.vector_len(tv))
        .map(|i| heap.vector_ref(tv, i))
        .collect()
}

/// Implementation of flip_scope for TaggedValue
///
/// Copies the pair and vector structure, flipping every identifier's
/// scopes. The copy is memoized container by container, from the first one:
/// expander output is a DAG (a pattern variable used twice shares its
/// pairs), and a quoted datum with labels is a cycle — a memo makes the copy
/// share where the original shared and close on itself where the original
/// did, instead of either walking forever or splicing the original's tail
/// into the copy. The new container is registered *before* its fields are
/// copied, which is what lets a back edge find it.
///
/// Vectors are walked because they are part of the form: a quasiquoted
/// `#(,(helper x))` in a template evaluates its elements, and the `helper`
/// in it is a reference the template introduced like any other. The
/// desugarer's relinker recognises an introduced reference by this very
/// scope, so an identifier the flip skipped would be one it could not
/// relink (`test_quasiquoted_vector_elements_are_rewritten`).
fn flip_scope_on_tagged_impl(
    tv: patina_core::TaggedValue,
    scope: patina_runtime::ScopeId,
    shared_heap: &patina_core::SharedHeap,
) -> patina_core::TaggedValue {
    let mut memo: std::collections::HashMap<u64, patina_core::TaggedValue> =
        std::collections::HashMap::new();
    flip_scope_memo(tv, scope, shared_heap, &mut memo)
}

fn flip_scope_memo(
    tv: patina_core::TaggedValue,
    scope: patina_runtime::ScopeId,
    shared_heap: &patina_core::SharedHeap,
    memo: &mut std::collections::HashMap<u64, patina_core::TaggedValue>,
) -> patina_core::TaggedValue {
    // Immediate values pass through unchanged
    if tv.is_fixnum() || tv.is_char() || tv.is_special() {
        return tv;
    }

    if tv.is_pair() {
        if let Some(copy) = memo.get(&tv.raw_bits()) {
            return *copy;
        }
        let (car, cdr) = shared_heap.borrow().get_pair(tv);
        let copy = shared_heap.borrow_mut().alloc_pair(car, cdr);
        memo.insert(tv.raw_bits(), copy);
        let new_car = flip_scope_memo(car, scope, shared_heap, memo);
        let new_cdr = flip_scope_memo(cdr, scope, shared_heap, memo);
        let mut heap = shared_heap.borrow_mut();
        heap.set_car(copy, new_car);
        heap.set_cdr(copy, new_cdr);
        return copy;
    }

    if tv.is_vector() {
        if let Some(copy) = memo.get(&tv.raw_bits()) {
            return *copy;
        }
        let elems = vector_elements(tv, shared_heap);
        let copy = shared_heap.borrow_mut().alloc_vector(elems.clone());
        memo.insert(tv.raw_bits(), copy);
        for (i, e) in elems.into_iter().enumerate() {
            let flipped = flip_scope_memo(e, scope, shared_heap, memo);
            shared_heap.borrow_mut().vector_set(copy, i, flipped);
        }
        return copy;
    }

    // Non-object types pass through unchanged
    if !tv.is_object() {
        return tv;
    }

    // Handle symbols (pass through - they don't participate in hygiene)
    if shared_heap.borrow().get_symbol_name(tv).is_some() {
        return tv;
    }

    // Handle identifiers (native or boxed) via unified method
    // Extract to binding first to avoid RefCell borrow conflict with alloc_identifier
    let id_data = shared_heap.borrow().get_identifier_data_any(tv);
    if let Some((name, scopes)) = id_data {
        let new_scopes = scopes.flip_scope(scope);
        return shared_heap.borrow_mut().alloc_identifier(name, new_scopes);
    }

    // All other values (vectors, etc.) pass through unchanged
    tv
}

/// Expand a macro with compile-time shadowing information using TaggedValue input
///
/// This is the primary entry point for macro expansion from the desugarer.
/// It uses TaggedValue-based operations for better performance:
///
/// 1. Flip input scopes on TaggedValue (avoids Value allocation)
/// 2. Pattern match directly on TaggedValue (no conversion!)
/// 3. Template expansion produces Value (still needed)
/// 4. Convert result to TaggedValue
/// 5. Flip output scopes on TaggedValue (avoids Value allocation)
///
/// # Arguments
/// * `compiled_macro` - The compiled macro definition
/// * `args` - The macro call arguments as TaggedValue
/// * `shared_heap` - Shared heap for conversions (Rc<RefCell<Heap>>)
/// * `shadowed_names` - Identifiers shadowed by local bindings at use site
pub fn expand_macro_with_shadowed_tagged(
    compiled_macro: &CompiledMacro,
    args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    shadowed_names: &std::collections::HashSet<std::rc::Rc<str>>,
    use_site_env: Option<&std::rc::Rc<patina_runtime::Environment>>,
) -> Result<patina_core::TaggedValue, crate::error::MacroError> {
    expand_macro_with_scope(
        compiled_macro,
        args,
        shared_heap,
        shadowed_names,
        use_site_env,
    )
    .map(|expansion| expansion.form)
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

/// [`expand_macro_with_shadowed_tagged`], also returning the expansion's
/// scope. The desugarer's relinker needs it: a template's free reference to
/// `list` is aliased to the definition site's `list`, and only the
/// references the template introduced may be — the `(list 1 2 3)` the user
/// wrote *inside* the macro call means the use site's `list`, even when the
/// two differ.
pub fn expand_macro_with_scope(
    compiled_macro: &CompiledMacro,
    args: patina_core::TaggedValue,
    shared_heap: &patina_core::SharedHeap,
    shadowed_names: &std::collections::HashSet<std::rc::Rc<str>>,
    use_site_env: Option<&std::rc::Rc<patina_runtime::Environment>>,
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
        shadowed_names,
        macro_scope,
        args, // original args for debug logging
        shared_heap,
        use_site_env,
    )?;

    // Step 4: Flip output scopes on expanded result
    let result = flip_scope_on_tagged(expanded_tagged, macro_scope, shared_heap);

    // Exit expansion (decrement depth)
    MacroTracer::exit_expansion();

    Ok(MacroExpansion {
        form: result,
        scope: macro_scope,
    })
}
