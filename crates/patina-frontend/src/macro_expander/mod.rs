//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic identifier renaming
//!
//! Based on Steel-scheme's native Rust approach.

pub mod compiler;
pub mod expander;
pub mod hygiene;
pub mod interface;
pub mod matcher;
pub mod pattern;
pub mod template;

// Re-export main functions
pub use hygiene::apply_hygiene;

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
pub fn expand_macro_v2(
    compiled_macro: &CompiledMacro,
    args: &patina_runtime::Value,
    env: &std::rc::Rc<patina_runtime::Environment>,
) -> Result<patina_runtime::Value, crate::error::FrontendError> {
    if patina_runtime::macro_debug::is_enabled() {
        println!("[MACRO] Expanding macro: {}", compiled_macro.name);

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

        println!("[MACRO]   Input: {}", args);
        println!("[MACRO]   Trying {} rule(s):", compiled_macro.rules.len());
    }

    let expander = Expander::new();

    // Try each rule until we find a match
    for (rule_idx, rule) in compiled_macro.rules.iter().enumerate() {
        if patina_runtime::macro_debug::is_enabled() {
            println!("[MACRO]   ");
            println!("[MACRO]   Trying rule {}:", rule_idx + 1);
        }

        // Create matcher for this rule
        // Pass pvar_names for better debug output
        let matcher = Matcher::new_with_names(rule.num_pvars, rule.pvar_names.clone());

        // Try to match the pattern against the arguments
        match matcher.match_pattern(&rule.pattern, args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[MACRO]   ");
                    println!("[MACRO]   Expanding template...");
                }

                let expanded = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::FrontendError::InvalidSyntax(e.to_string()))?;

                if patina_runtime::macro_debug::is_enabled() {
                    println!("[MACRO]   Template result: {}", expanded);
                }

                // Apply hygiene: rename free identifiers
                // For now, we collect all symbols from the input to prevent renaming them
                let mut pattern_vars = std::collections::HashSet::new();
                collect_symbols_from_value(args, &mut pattern_vars);

                if patina_runtime::macro_debug::is_enabled() {
                    println!("[MACRO]   ");
                    println!("[MACRO]   Applying hygiene...");
                }

                let hygienic = hygiene::apply_hygiene(&expanded, &pattern_vars, env);

                if patina_runtime::macro_debug::is_enabled() {
                    println!("[MACRO]   Final result: {}", hygienic);
                    println!("[MACRO] ");
                }

                return Ok(hygienic);
            }
            Err(_) => {
                // This rule didn't match, try next one
                continue;
            }
        }
    }

    // No rule matched
    if patina_runtime::macro_debug::is_enabled() {
        println!("[MACRO]   No rules matched!");
        println!("[MACRO] ");
    }

    Err(crate::error::FrontendError::InvalidSyntax(format!(
        "No matching pattern for macro {}",
        compiled_macro.name
    )))
}

/// Recursively collect all symbols from a value
///
/// Used for hygiene: extracts all symbol identifiers from matched values
/// so they won't be renamed by the macro system.
fn collect_symbols_from_value(
    value: &patina_runtime::Value,
    symbols: &mut std::collections::HashSet<std::rc::Rc<str>>,
) {
    match value {
        patina_runtime::Value::Symbol(name) => {
            symbols.insert(name.clone());
        }
        patina_runtime::Value::Pair(pair) => {
            collect_symbols_from_value(&pair.0, symbols);
            collect_symbols_from_value(&pair.1, symbols);
        }
        patina_runtime::Value::Vector(vec) => {
            for item in vec.borrow().iter() {
                collect_symbols_from_value(item, symbols);
            }
        }
        // All other values (numbers, strings, booleans, etc.) have no symbols
        _ => {}
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
