//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic identifier renaming using marks-and-ribs
//!
//! Based on Steel-scheme's native Rust approach.

pub mod compiler;
mod debug;
pub mod expander;
pub mod interface;
pub mod matcher;
pub mod pattern;
pub mod template;
pub mod utils;
pub mod validator;

#[cfg(test)]
mod ellipsis_edge_cases_tests;

// Re-export V2 types (now the only types)
pub use compiler::{CompiledMacro, CompiledRule, Compiler};
pub use expander::{ExpandError, Expander};
pub use interface::{CompiledMacroExpander, ExpansionResult, MacroExpander};
pub use matcher::{MatchError, Matcher};
pub use pattern::Pattern;
pub use template::{Identifier, Template};

// Re-export test helpers
pub use interface::TestExpander;

// Re-export utility functions and constants
pub use utils::{
    ELLIPSIS, MACRO_DEFINITION_FORMS, WILDCARD, pattern_to_string, pattern_to_string_with_names,
    template_to_string_with_names,
};

/// Expand a macro using the V2 PVREF-based system
///
/// This is the new macro expansion pipeline:
/// 1. Try each compiled rule in order
/// 2. Use Matcher to match pattern against input
/// 3. Use Expander to expand template with match environment
/// 4. Apply hygiene to result
///
/// Based on Gauche's macro expansion pipeline.
pub fn expand_macro(
    compiled_macro: &CompiledMacro,
    args: &patina_runtime::Value,
    expansion_env: &std::rc::Rc<patina_runtime::Environment>,
) -> Result<patina_runtime::Value, crate::error::MacroError> {
    // No shadowed names - use default behavior
    expand_macro_with_shadowed(
        compiled_macro,
        args,
        expansion_env,
        &std::collections::HashSet::new(),
        &patina_runtime::ScopeSet::new(),
    )
}

/// Expand a macro with compile-time shadowing information
///
/// This variant accepts:
/// - `shadowed_names`: identifiers shadowed by local bindings at the macro use site
///   (e.g., lambda parameters). When a literal identifier is shadowed, it should NOT
///   match as a literal (R7RS 4.3.2).
/// - `use_site_scopes`: the scope set at the macro use site, used for comparing
///   whether the use-site binding is the same as the literal's definition-time binding.
pub fn expand_macro_with_shadowed(
    compiled_macro: &CompiledMacro,
    args: &patina_runtime::Value,
    expansion_env: &std::rc::Rc<patina_runtime::Environment>,
    shadowed_names: &std::collections::HashSet<std::rc::Rc<str>>,
    use_site_scopes: &patina_runtime::ScopeSet,
) -> Result<patina_runtime::Value, crate::error::MacroError> {
    use crate::tracer::MacroTracer;
    use debug::{DebugContext, record_expansion_step};

    // Enter macro expansion (for depth tracking)
    MacroTracer::enter_expansion();

    // Set up debug context
    let dbg = DebugContext::new(
        patina_runtime::macro_debug::is_enabled(),
        MacroTracer::should_trace(&compiled_macro.name),
    );

    // Create a fresh macro scope for this expansion (Racket-style hygiene)
    let macro_scope = patina_runtime::ScopeId::fresh();

    // Log expansion start
    dbg.log_expansion_start(
        &compiled_macro.name,
        macro_scope,
        &compiled_macro.definition_scopes,
        &compiled_macro.rules,
        args,
    );

    // Step 1: Flip macro_scope on INPUT (adds scope to all use-site identifiers)
    // This is the first half of Racket's hygiene algorithm
    let flipped_args = flip_scope_on_value(args, macro_scope);
    dbg.log_input_flip(macro_scope, &flipped_args);

    // Create expander with expansion-time environment and macro scope
    let expander = Expander::new(expansion_env.clone(), macro_scope);

    // Try each rule until we find a match
    for (rule_idx, rule) in compiled_macro.rules.iter().enumerate() {
        dbg.log_trying_rule(rule_idx, rule);

        // Create matcher for this rule with hygiene support
        // Pass shadowed_names, use_site_scopes, and literals for checking shadowed literals (R7RS 4.3.2)
        let matcher = Matcher::new_with_hygiene(
            rule.num_pvars,
            rule.pvar_names.clone(),
            shadowed_names.clone(),
            use_site_scopes.clone(),
            compiled_macro.literals.clone(),
        );

        // Try to match the pattern against the FLIPPED arguments
        // The flipped_args have macro_scope added to all identifiers
        match matcher.match_pattern(&rule.pattern, &flipped_args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                dbg.log_match_success(&match_env, &rule.pvar_names, &rule.template);

                let expanded = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::MacroError::InvalidSyntax(e.to_string()))?;

                dbg.log_before_output_flip(&expanded);

                // Step 2: Flip macro_scope on OUTPUT (Racket-style hygiene)
                // - Use-site identifiers (from pattern vars): macro_scope gets removed
                // - Introduced identifiers (from template): macro_scope gets added
                let result = flip_scope_on_value(&expanded, macro_scope);

                dbg.log_expansion_complete(&compiled_macro.name, macro_scope, &result);

                // Record expansion step for tracing
                record_expansion_step(
                    dbg.should_trace,
                    &compiled_macro.name,
                    rule_idx,
                    compiled_macro.rules.len(),
                    args,
                );

                // Exit expansion (decrement depth)
                MacroTracer::exit_expansion();

                return Ok(result);
            }
            Err(e) => {
                // This rule didn't match, try next one
                dbg.log_match_failure(&e);
                continue;
            }
        }
    }

    // No rule matched
    dbg.log_no_rules_matched();

    // Exit expansion (decrement depth)
    MacroTracer::exit_expansion();

    Err(crate::error::MacroError::InvalidSyntax(format!(
        "No matching pattern for macro {}",
        compiled_macro.name
    )))
}

/// Flip a scope on all identifiers in a Value tree (Racket-style hygiene)
///
/// This is the core hygiene operation that distinguishes use-site vs introduced identifiers:
/// - Called BEFORE pattern matching: adds scope to input identifiers
/// - Called AFTER template expansion: toggles scope
///   - Use-site identifiers (from input): scope gets removed
///   - Introduced identifiers (from template): scope gets added
///
/// Note: Only flips on existing Identifiers, NOT on Symbols. Symbols remain as Symbols.
/// This preserves special forms and built-in names that should not participate in hygiene.
///
/// Based on Racket's `flip-scope` operation from "Binding as Sets of Scopes" (Flatt 2016)
fn flip_scope_on_value(
    value: &patina_runtime::Value,
    scope: patina_runtime::ScopeId,
) -> patina_runtime::Value {
    use patina_runtime::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    match value {
        // Identifiers: flip the scope
        Value::Identifier(id) => Value::Identifier(Box::new(patina_runtime::IdentifierData {
            name: id.name.clone(),
            scopes: id.scopes.flip_scope(scope),
        })),

        // Symbols stay as Symbols - they don't participate in hygiene flip
        // This preserves special forms like `if`, `define`, `lambda`, etc.
        Value::Symbol(_) => value.clone(),

        // Pairs: recursively flip both car and cdr
        // Note: We DO recurse into (quote ...) forms because:
        // 1. If the quoted datum came from a pattern variable substitution, it will have scopes
        //    and those scopes need to be flipped for proper hygiene
        // 2. If the quoted datum is literal data (plain Symbols), flip_scope is a no-op on Symbols
        //    so they remain unchanged
        // This distinction allows nested macro definitions to work correctly - the substituted
        // values inside (quote y) get their scopes flipped, while literal symbols stay as symbols.
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let new_car = flip_scope_on_value(&borrowed.0, scope);
            let new_cdr = flip_scope_on_value(&borrowed.1, scope);
            Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
        }

        // Vectors are self-quoting data - don't flip their contents
        // Symbols inside vectors should remain unchanged
        Value::Vector(_) => value.clone(),

        // All other values pass through unchanged
        _ => value.clone(),
    }
}

// Note: pattern_to_string_with_names and template_to_string_with_names
// are now in utils.rs and re-exported above
