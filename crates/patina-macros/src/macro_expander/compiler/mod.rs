//! Pattern and template compiler for PVREF-based macro system
//!
//! This module compiles Scheme syntax-rules patterns and templates into
//! efficient PVREF (Pattern Variable Reference) based representations.
//!
//! Inspired by Gauche Scheme's pattern compilation (macro.c:400-683)
//! by Shiro Kawai.
//!
//! Key concepts:
//! - Two-phase design: compile pattern once, match many times
//! - PVREF encoding for O(1) variable lookup
//! - Precomputed metadata (num_following, vars) for optimization
//!
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c
//!
//! # Module Organization
//!
//! - `mod.rs` - Compiler struct, constructors, and `compile_macro()`
//! - `pattern.rs` - Pattern compilation logic
//! - `template.rs` - Template compilation logic
//! - `escape.rs` - Ellipsis escape handling
//! - `helpers.rs` - Utility methods
//! - `tests.rs` - Unit tests

mod escape;
mod helpers;
mod pattern;
mod template;
#[cfg(test)]
mod tests;

use super::utils::ELLIPSIS;
use crate::error::MacroError;
use patina_runtime::{Environment, PVRef, ScopeSet, Value};
use std::collections::HashMap;
use std::rc::Rc;

// Re-export CompiledMacro and CompiledRule from patina-core (via patina-runtime)
pub use patina_runtime::{CompiledMacro, CompiledRule};

/// Pattern and template compiler
///
/// Compiles Scheme S-expressions into PVREF-based Pattern2/Template2.
///
/// Based on Gauche's compile_rules (macro.c:604-683).
pub struct Compiler {
    /// Literal identifiers
    pub(super) literals: Vec<Rc<str>>,

    /// Symbol used for ellipsis (usually "...")
    /// None means ellipsis is disabled (inside escape)
    pub(super) ellipsis: Option<Rc<str>>,

    /// Lexical environment where the macro is being defined (for hygiene)
    ///
    /// Free variables in templates will capture this environment.
    /// This enables proper lexical scoping for macros following Gauche's approach.
    pub(super) env: Option<Rc<Environment>>,

    /// Scope set at macro definition time (for scope-based hygiene)
    ///
    /// Free variables will carry this scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    pub(super) definition_scopes: ScopeSet,

    // Per-rule compilation context
    /// Map from pattern variable name to PVREF
    pub(super) pvars: HashMap<Rc<str>, PVRef>,

    /// Counter for assigning PVREF indices
    pub(super) pvar_count: usize,

    /// Maximum ellipsis level seen so far
    pub(super) max_level: usize,
}

impl Compiler {
    /// Create a new compiler
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    pub fn new(literals: Vec<Rc<str>>, ellipsis: Option<Rc<str>>) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            env: None,
            definition_scopes: ScopeSet::new(),
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Create a new compiler with environment capture (for hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    pub fn with_env(
        literals: Vec<Rc<str>>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
    ) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            env: Some(env),
            definition_scopes: ScopeSet::new(),
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Create a new compiler with environment and scope set (for scope-based hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    /// - `scopes`: Scope set at macro definition time
    ///
    /// Free variables in templates will carry the scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    pub fn with_env_and_scopes(
        literals: Vec<Rc<str>>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
        scopes: ScopeSet,
    ) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            env: Some(env),
            definition_scopes: scopes,
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Compile a complete macro definition
    ///
    /// # Arguments
    /// - `name`: Macro name
    /// - `rules`: List of (pattern, template) pairs as S-expressions
    ///
    /// # Returns
    /// Compiled macro with all rules in PVREF form
    pub fn compile_macro(
        &mut self,
        name: Rc<str>,
        rules: Vec<(Value, Value)>,
    ) -> Result<CompiledMacro, MacroError> {
        let mut compiled_rules = Vec::new();
        let mut max_pvars = 0;

        for (pat_form, tmpl_form) in rules {
            // Reset per-rule context
            self.pvars.clear();
            self.pvar_count = 0;
            self.max_level = 0;

            // R7RS: The first element of each pattern is the macro keyword placeholder
            // and should be ignored (treated as wildcard). This is true even if the
            // symbol appears in the literals list (e.g., when _ is in literals).
            let pattern = self.compile_rule_pattern(&pat_form, 0)?;
            let template = self.compile_template(&tmpl_form, 0)?;

            // Build reverse mapping: PVREF -> name (for debug output)
            let pvar_names: HashMap<PVRef, Rc<str>> = self
                .pvars
                .iter()
                .map(|(name, pvref)| (*pvref, name.clone()))
                .collect();

            // Validate the rule before adding it
            if let Err(e) = super::validator::validate_rule(&pattern, &template, &pvar_names) {
                return Err(MacroError::InvalidSyntax(format!(
                    "Macro '{}' validation failed: {}",
                    name, e
                )));
            }

            compiled_rules.push(CompiledRule {
                pattern,
                template,
                num_pvars: self.pvar_count,
                max_level: self.max_level,
                pvar_names,
            });

            max_pvars = max_pvars.max(self.pvar_count);
        }

        Ok(CompiledMacro {
            name,
            literals: self.literals.clone(),
            rules: compiled_rules,
            max_pvars,
            definition_scopes: self.definition_scopes.clone(),
        })
    }
}
