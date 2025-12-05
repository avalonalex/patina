//! Debug output helpers for macro expansion
//!
//! This module contains all the debug printing logic for macro expansion,
//! keeping it separate from the core expansion algorithm.

use patina_runtime::debug_format::format_with_scopes;
use patina_runtime::{CompiledRule, MatchEnv, PVRef, ScopeId, Value};
use std::collections::HashMap;
use std::rc::Rc;

use super::{Template, pattern_to_string_with_names, template_to_string_with_names};

/// Debug context for macro expansion
///
/// Holds the state needed for debug output during expansion.
pub struct DebugContext {
    /// Whether debug output is enabled
    pub debug_enabled: bool,
    /// Whether tracing is enabled for this specific macro
    pub should_trace: bool,
}

impl DebugContext {
    /// Create a new debug context
    pub fn new(debug_enabled: bool, should_trace: bool) -> Self {
        Self {
            debug_enabled,
            should_trace,
        }
    }

    /// Check if any debug output should be produced
    pub fn is_active(&self) -> bool {
        self.debug_enabled || self.should_trace
    }

    /// Log the start of macro expansion
    pub fn log_expansion_start(
        &self,
        macro_name: &str,
        macro_scope: ScopeId,
        definition_scopes: &patina_runtime::ScopeSet,
        rules: &[CompiledRule],
        args: &Value,
    ) {
        if !self.is_active() {
            return;
        }

        println!("[MACRO] ========================================");
        println!("[MACRO] Expanding macro: {}", macro_name);
        println!("[MACRO]   Fresh macro scope: {}", macro_scope);
        println!("[MACRO]   Definition scopes: {}", definition_scopes);

        self.log_macro_rules(rules);
        self.log_input(args);

        println!("[MACRO]   Trying {} rule(s):", rules.len());
    }

    /// Log the macro rules (patterns)
    fn log_macro_rules(&self, rules: &[CompiledRule]) {
        if rules.is_empty() {
            return;
        }

        println!("[MACRO]   Definition ({} rule(s)):", rules.len());
        for (idx, rule) in rules.iter().enumerate() {
            println!(
                "[MACRO]     Rule {}: {} -> <template>",
                idx + 1,
                pattern_to_string_with_names(&rule.pattern, &rule.pvar_names)
            );
        }
        println!("[MACRO]   ");
    }

    /// Log the input arguments
    fn log_input(&self, args: &Value) {
        println!("[MACRO]   Input (normal):      {}", args);
        println!(
            "[MACRO]   Input (with scopes): {}",
            format_with_scopes(args)
        );
    }

    /// Log the result of flipping scopes on input
    pub fn log_input_flip(&self, macro_scope: ScopeId, flipped_args: &Value) {
        if !self.debug_enabled {
            return;
        }

        println!("[MACRO]   ");
        println!(
            "[MACRO]   After input flip (scope {} toggled):",
            macro_scope
        );
        println!("[MACRO]     {}", format_with_scopes(flipped_args));
    }

    /// Log the start of trying a rule
    pub fn log_trying_rule(&self, rule_idx: usize, rule: &CompiledRule) {
        if !self.debug_enabled {
            return;
        }

        println!("[MACRO]   ");
        println!("[MACRO]   === Trying rule {} ===", rule_idx + 1);
        println!(
            "[MACRO]   Pattern: {}",
            pattern_to_string_with_names(&rule.pattern, &rule.pvar_names)
        );
    }

    /// Log a successful match
    pub fn log_match_success(
        &self,
        match_env: &MatchEnv,
        pvar_names: &HashMap<PVRef, Rc<str>>,
        template: &Template,
    ) {
        if !self.debug_enabled {
            return;
        }

        println!("[MACRO]   ✓ Match successful!");
        println!("[MACRO]   ");

        self.log_pattern_bindings(match_env, pvar_names);

        println!("[MACRO]   ");
        println!("[MACRO]   === Expanding template ===");
        println!(
            "[MACRO]   Template: {}",
            template_to_string_with_names(template, pvar_names)
        );
    }

    /// Log pattern variable bindings
    fn log_pattern_bindings(&self, match_env: &MatchEnv, pvar_names: &HashMap<PVRef, Rc<str>>) {
        println!("[MACRO]   === Pattern variable bindings ===");
        for (pvref, name) in pvar_names {
            if let Some(value) = match_env.get(*pvref, &[]) {
                println!("[MACRO]     {} = {}", name, format_with_scopes(&value));
            }
        }
    }

    /// Log the expanded result before output flip
    pub fn log_before_output_flip(&self, expanded: &Value) {
        if !self.debug_enabled {
            return;
        }

        println!("[MACRO]   ");
        println!("[MACRO]   Before output flip:");
        println!("[MACRO]     {}", format_with_scopes(expanded));
    }

    /// Log the final result after output flip
    pub fn log_expansion_complete(&self, macro_name: &str, macro_scope: ScopeId, result: &Value) {
        if !self.is_active() {
            return;
        }

        println!("[MACRO]   ");
        println!(
            "[MACRO]   After output flip (scope {} toggled):",
            macro_scope
        );
        println!("[MACRO]     (with scopes) {}", format_with_scopes(result));
        println!("[MACRO] ");
        println!("[MACRO] ========================================");
        println!("[MACRO] Expansion of '{}' complete!", macro_name);
        println!("[MACRO] Result: {}", result);
        println!("[MACRO] ========================================");
        println!("[MACRO] ");
    }

    /// Log a match failure
    pub fn log_match_failure(&self, error: &super::MatchError) {
        if !self.debug_enabled {
            return;
        }

        println!("[MACRO]   ✗ Match failed: {}", error);
        println!("[MACRO]   ");
    }

    /// Log when no rules matched
    pub fn log_no_rules_matched(&self) {
        if !self.is_active() {
            return;
        }

        println!("[MACRO]   No rules matched!");
        println!("[MACRO] ");
    }
}

/// Helper to record an expansion step for tracing
pub fn record_expansion_step(
    should_trace: bool,
    macro_name: &Rc<str>,
    rule_idx: usize,
    num_rules: usize,
    args: &Value,
) {
    if !should_trace {
        return;
    }

    use crate::error::ExpansionStep;
    use crate::tracer::MacroTracer;

    let step = ExpansionStep::new(macro_name.clone(), rule_idx, num_rules, format!("{}", args));
    MacroTracer::record_step(step);
}
