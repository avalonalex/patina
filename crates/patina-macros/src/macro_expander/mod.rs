//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic identifier renaming using marks-and-ribs
//!
//! Based on Steel-scheme's native Rust approach.

pub mod compiler;
pub mod expander;
pub mod interface;
pub mod matcher;
pub mod pattern;
pub mod template;
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
    )
}

/// Expand a macro with compile-time shadowing information
///
/// This variant accepts `shadowed_names` which contains identifiers that are
/// shadowed by local bindings at the macro use site (e.g., lambda parameters).
/// When a literal identifier (like `=>` in cond) is shadowed, it should NOT
/// match as a literal (R7RS 4.3.2).
///
/// TODO: This will be simplified in DEFINE_SYNTAX_ELIMINATION.md
pub fn expand_macro_with_shadowed(
    compiled_macro: &CompiledMacro,
    args: &patina_runtime::Value,
    expansion_env: &std::rc::Rc<patina_runtime::Environment>,
    shadowed_names: &std::collections::HashSet<std::rc::Rc<str>>,
) -> Result<patina_runtime::Value, crate::error::MacroError> {
    use crate::tracer::MacroTracer;
    use patina_runtime::debug_format::format_with_scopes;

    // Enter macro expansion (for depth tracking)
    MacroTracer::enter_expansion();

    // Check if this macro should be traced
    let should_trace = MacroTracer::should_trace(&compiled_macro.name);
    let debug_enabled = patina_runtime::macro_debug::is_enabled();

    // Create a fresh macro scope for this expansion (Racket-style hygiene)
    let macro_scope = patina_runtime::ScopeId::fresh();

    if debug_enabled || should_trace {
        println!("[MACRO] ========================================");
        println!("[MACRO] Expanding macro: {}", compiled_macro.name);
        println!("[MACRO]   Fresh macro scope: {}", macro_scope);
        println!(
            "[MACRO]   Definition scopes: {}",
            compiled_macro.definition_scopes
        );

        // Print macro definition (pattern -> template for each rule)
        if !compiled_macro.rules.is_empty() {
            println!(
                "[MACRO]   Definition ({} rule(s)):",
                compiled_macro.rules.len()
            );
            for (idx, rule) in compiled_macro.rules.iter().enumerate() {
                println!(
                    "[MACRO]     Rule {}: {} -> <template>",
                    idx + 1,
                    pattern_to_string_with_names(&rule.pattern, &rule.pvar_names)
                );
            }
            println!("[MACRO]   ");
        }

        // Show input with scopes (this is the key improvement!)
        println!("[MACRO]   Input (normal):      {}", args);
        println!(
            "[MACRO]   Input (with scopes): {}",
            format_with_scopes(args)
        );
        println!("[MACRO]   Trying {} rule(s):", compiled_macro.rules.len());
    }

    // Step 1: Flip macro_scope on INPUT (adds scope to all use-site identifiers)
    // This is the first half of Racket's hygiene algorithm
    let flipped_args = flip_scope_on_value(args, macro_scope);

    if debug_enabled {
        println!("[MACRO]   ");
        println!(
            "[MACRO]   After input flip (scope {} toggled):",
            macro_scope
        );
        println!("[MACRO]     {}", format_with_scopes(&flipped_args));
    }

    // Create expander with expansion-time environment and macro scope
    let expander = Expander::new(expansion_env.clone(), macro_scope);

    // Try each rule until we find a match
    for (rule_idx, rule) in compiled_macro.rules.iter().enumerate() {
        if debug_enabled {
            println!("[MACRO]   ");
            println!("[MACRO]   === Trying rule {} ===", rule_idx + 1);
            println!(
                "[MACRO]   Pattern: {}",
                pattern_to_string_with_names(&rule.pattern, &rule.pvar_names)
            );
        }

        // Create matcher for this rule with hygiene support
        // Pass shadowed_names and literals for checking shadowed literals (R7RS 4.3.2)
        // TODO: This will be simplified in DEFINE_SYNTAX_ELIMINATION.md
        let matcher = Matcher::new_with_hygiene(
            rule.num_pvars,
            rule.pvar_names.clone(),
            shadowed_names.clone(),
            compiled_macro.literals.clone(),
        );

        // Try to match the pattern against the FLIPPED arguments
        // The flipped_args have macro_scope added to all identifiers
        match matcher.match_pattern(&rule.pattern, &flipped_args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                if debug_enabled {
                    println!("[MACRO]   ✓ Match successful!");
                    println!("[MACRO]   ");

                    // Show pattern variable bindings with scopes
                    println!("[MACRO]   === Pattern variable bindings ===");
                    for (pvref, name) in &rule.pvar_names {
                        if let Some(value) = match_env.get(*pvref, &[]) {
                            println!("[MACRO]     {} = {}", name, format_with_scopes(&value));
                        }
                    }

                    println!("[MACRO]   ");
                    println!("[MACRO]   === Expanding template ===");
                    println!(
                        "[MACRO]   Template: {}",
                        template_to_string(&rule.template, &rule.pvar_names)
                    );
                }

                let expanded = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::MacroError::InvalidSyntax(e.to_string()))?;

                if debug_enabled {
                    println!("[MACRO]   ");
                    println!("[MACRO]   Before output flip:");
                    println!("[MACRO]     {}", format_with_scopes(&expanded));
                }

                // Step 2: Flip macro_scope on OUTPUT (Racket-style hygiene)
                // - Use-site identifiers (from pattern vars): macro_scope gets removed
                // - Introduced identifiers (from template): macro_scope gets added
                let result = flip_scope_on_value(&expanded, macro_scope);

                if debug_enabled || should_trace {
                    println!("[MACRO]   ");
                    println!(
                        "[MACRO]   After output flip (scope {} toggled):",
                        macro_scope
                    );
                    println!("[MACRO]     (with scopes) {}", format_with_scopes(&result));
                    println!("[MACRO] ");
                    println!("[MACRO] ========================================");
                    println!("[MACRO] Expansion of '{}' complete!", compiled_macro.name);
                    println!("[MACRO] Result: {}", result);
                    println!("[MACRO] ========================================");
                    println!("[MACRO] ");
                }

                // Record expansion step for tracing
                if should_trace {
                    use crate::error::ExpansionStep;
                    let step = ExpansionStep::new(
                        compiled_macro.name.clone(),
                        rule_idx,
                        compiled_macro.rules.len(),
                        format!("{}", args),
                    );
                    MacroTracer::record_step(step);
                }

                // Exit expansion (decrement depth)
                MacroTracer::exit_expansion();

                return Ok(result);
            }
            Err(e) => {
                // This rule didn't match, try next one
                if debug_enabled {
                    println!("[MACRO]   ✗ Match failed: {}", e);
                    println!("[MACRO]   ");
                }
                continue;
            }
        }
    }

    // No rule matched
    if debug_enabled || should_trace {
        println!("[MACRO]   No rules matched!");
        println!("[MACRO] ");
    }

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
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let new_car = flip_scope_on_value(&borrowed.0, scope);
            let new_cdr = flip_scope_on_value(&borrowed.1, scope);
            Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
        }

        // Vectors: recursively flip all elements
        Value::Vector(vec) => {
            let new_elements: Vec<_> = vec
                .borrow()
                .iter()
                .map(|elem| flip_scope_on_value(elem, scope))
                .collect();
            Value::Vector(Rc::new(RefCell::new(new_elements)))
        }

        // All other values pass through unchanged
        _ => value.clone(),
    }
}

/// Convert a pattern to a readable string with variable names for debug output
fn pattern_to_string_with_names(
    pattern: &Pattern,
    names: &std::collections::HashMap<patina_runtime::PVRef, std::rc::Rc<str>>,
) -> String {
    match pattern {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Literal(v) => format!("{}", v),
        Pattern::Var(pv) => {
            // Look up the actual variable name
            names
                .get(pv)
                .map(|n| n.to_string())
                .unwrap_or_else(|| "var".to_string())
        }
        Pattern::List(patterns) => {
            let inner = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        Pattern::DottedList { patterns, tail } => {
            let mut parts: Vec<String> = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect();
            parts.push(".".to_string());
            parts.push(pattern_to_string_with_names(tail, names));
            format!("({})", parts.join(" "))
        }
        Pattern::Vector(patterns) => {
            let inner = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({})", inner)
        }
        Pattern::Ellipsis { subpattern, .. } => {
            format!("({} ...)", pattern_to_string_with_names(subpattern, names))
        }
    }
}

/// Convert a template to a readable string with variable names for debug output
fn template_to_string(
    template: &Template,
    names: &std::collections::HashMap<patina_runtime::PVRef, std::rc::Rc<str>>,
) -> String {
    match template {
        Template::Literal(v) => format!("{}", v),
        Template::Symbol(id) => id.name().to_string(),
        Template::Var(pv) => {
            // Look up the actual variable name
            names
                .get(pv)
                .map(|n| n.to_string())
                .unwrap_or_else(|| "var".to_string())
        }
        Template::List(templates) => {
            let inner = templates
                .iter()
                .map(|t| template_to_string(t, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        Template::DottedList { templates, tail } => {
            let mut parts: Vec<String> = templates
                .iter()
                .map(|t| template_to_string(t, names))
                .collect();
            parts.push(".".to_string());
            parts.push(template_to_string(tail, names));
            format!("({})", parts.join(" "))
        }
        Template::Vector(templates) => {
            let inner = templates
                .iter()
                .map(|t| template_to_string(t, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({})", inner)
        }
        Template::Ellipsis {
            subtemplate,
            nesting,
            ..
        } => {
            let dots = (0..*nesting).map(|_| "...").collect::<Vec<_>>().join(" ");
            format!("({} {})", template_to_string(subtemplate, names), dots)
        }
    }
}
