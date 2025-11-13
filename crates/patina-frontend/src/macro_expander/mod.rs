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
    let expander = Expander::new();

    // Try each rule until we find a match
    for rule in &compiled_macro.rules {
        // Create matcher for this rule
        let matcher = Matcher::new(rule.num_pvars);

        // Try to match the pattern against the arguments
        match matcher.match_pattern(&rule.pattern, args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                let expanded = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::FrontendError::InvalidSyntax(e.to_string()))?;

                // Apply hygiene: rename free identifiers
                // For now, we collect all symbols from the input to prevent renaming them
                let mut pattern_vars = std::collections::HashSet::new();
                collect_symbols_from_value(args, &mut pattern_vars);

                let hygienic = hygiene::apply_hygiene(&expanded, &pattern_vars, env);

                return Ok(hygienic);
            }
            Err(_) => {
                // This rule didn't match, try next one
                continue;
            }
        }
    }

    // No rule matched
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
